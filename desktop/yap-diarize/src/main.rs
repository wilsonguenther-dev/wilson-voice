//! `yap-diarize` — Yap's speaker diarization stage, as a sidecar process (YV121).
//!
//! Why a third binary instead of a module in the app: `vad-rs` runs Silero
//! through `ort` and links **onnxruntime statically into `wilson-voice`**, and
//! `sherpa-onnx` — the crate YV122 adds here — statically links its own
//! vendored copy. Two independently-vendored copies of one C++ runtime in one
//! link unit is the identical duplicate-symbol failure that already forced
//! `yap-polish` out of process. Two link units solve it outright, and the
//! process boundary buys the same second thing: a wedged or OOM diarizer can be
//! killed, and a diarization that fails degrades to a plain transcript instead
//! of taking the app down.
//!
//! Protocol: newline-delimited JSON on stdin/stdout, one request per line, one
//! response per line — see `diarize_protocol.rs`, which is compiled into both
//! binaries from a single file so the ends cannot drift. **stdout carries JSON
//! and nothing else**; every diagnostic goes to stderr, and the test at the
//! bottom of this file holds that rule to the source.
//!
//! ```text
//! yap-diarize            # no arguments: models arrive as a request, not argv
//! ```
//!
//! The FIRST line on stdout is the readiness announcement
//! (`{"type":"ready","version":"0.1.0"}`), written as soon as the process is
//! up — **before** any model is loaded. That is the one shape difference from
//! `yap-polish`, whose model is fixed on argv and whose handshake therefore has
//! to carry `model_loaded`. Here "are the models in?" is answered by the
//! `load_models` response and nowhere else, which keeps a multi-second ONNX
//! session build out of the parent's spawn budget.
//!
//! ## The models (YV122)
//!
//! Two files, both named by the parent on a `load_models` request — this
//! process holds no catalog, no download logic and no default path, so a
//! re-vendoring (YV123) never touches this binary:
//!
//! * **pyannote-segmentation-3.0** (MIT) through
//!   [`sherpa_onnx::OfflineSpeakerDiarization`] — segments the track and
//!   clusters the turns.
//! * **wespeaker_en_voxceleb_CAM++** (Apache-2.0 toolkit, CC-BY-4.0 weights)
//!   through [`sherpa_onnx::SpeakerEmbeddingExtractor`] — one embedding per
//!   turn, and the whole payload of an `embed` request.
//!
//! The embedding width is read off the extractor
//! ([`SpeakerEmbeddingExtractor::dim`]) and reported on the `load_models`
//! response. It is whatever the model reports: the shipped
//! `wespeaker_en_voxceleb_CAM++` **measures 512** (its own ONNX metadata says
//! `output_dim: 512`) and the `wespeaker_en_voxceleb_resnet34` control measures
//! **256**. Audit finding #19 predicted 192 for CAM++ — its *mechanism* (never
//! assume a width) is right and its *number* is wrong, which is precisely how
//! the mechanism earned its keep. Nothing in this file, and nothing on the
//! parent's side of the wire, writes any of those numbers down.
//!
//! ## Not one clustering number lives in this file
//!
//! `FastClusteringConfig.threshold` is a cosine **DISTANCE** — smaller is more
//! similar — and it arrives on every `diarize` request. It is never stored,
//! never defaulted here, and never converted: the one place a bare `f32` and
//! that field meet is [`clustering_from`], four lines long, and the value it
//! reads came off the request line the parent sent.
//!
//! The diarizer is therefore built **per request** rather than at load time.
//! That is the mechanism, not a style choice: a resident diarizer would hold a
//! clustering threshold from some earlier call (or from
//! `FastClusteringConfig::default()`'s 0.5, a vendor number this epic is not
//! allowed to let decide anything), and a `set_config` forgotten on one path
//! would silently label a meeting at the wrong threshold with nothing to
//! observe it. A `Backend` that cannot hold a threshold cannot leak one.
//!
//! ## `stdout` is still only JSON, and now something else writes to it
//!
//! sherpa-onnx's C++ core logs — a bad model path prints
//! `speaker-embedding-extractor.cc:Validate:40 …` — which would be a
//! catastrophe on this stream. It logs to **stderr**; measured, not assumed,
//! and `tests/sherpa_load_smoke.rs::sherpa_logs_never_reach_the_protocol_stream`
//! is the standing proof, run with zero model bytes against the refusal path.

// The wire contract lives with the app it talks to. Compiled, not copied.
#[path = "../../src-tauri/src/diarize_protocol.rs"]
mod diarize_protocol;

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Instant;

use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig, Wave,
};

use diarize_protocol::{
    recover_id, DiarizeReady, DiarizeRequest, DiarizeResponse, DiarizeSegment,
    ERR_AUDIO_NOT_FOUND, ERR_AUDIO_TOO_SHORT, ERR_AUDIO_UNREADABLE, ERR_BACKEND_FAILED,
    ERR_BAD_REQUEST, ERR_MISSING_FIELD, ERR_MODEL_LOAD_FAILED, ERR_MODEL_NOT_FOUND,
    ERR_NO_MODELS, ERR_SAMPLE_RATE, ERR_UNSUPPORTED_KIND, KIND_DIARIZE, KIND_EMBED,
    KIND_LOAD_MODELS,
};

/// `FastClusteringConfig.num_clusters`' "I do not know how many people are in
/// this room" sentinel — which is the whole reason the threshold path is used
/// at all. Plan §2.3: for a meeting we do not know the speaker count, and for a
/// class we really do not. Fixing the count would be a different product.
const CLUSTER_COUNT_UNKNOWN: i32 = -1;

/// The loaded model pair.
///
/// The embedding extractor is resident (one ONNX session, reused by every turn
/// of every track and by every enrollment utterance); the segmentation model is
/// held as a **path**, and a diarizer is built from it per request. There is
/// deliberately no `OfflineSpeakerDiarization` field: that type carries a
/// clustering threshold, and a threshold that outlives the request it arrived
/// on is a wrong-threshold labeling nothing downstream can see.
///
/// `embedding_dim` is read off the model, never assumed: the plan's schema
/// guessed a width, audit finding #19 "corrected" that guess to 192, and the
/// shipped CAM++ measures 512 (the ResNet34 control, 256). The only place any
/// of those claims can be checked is here, where the file actually is.
struct Backend {
    segmentation_path: String,
    embedding_path: String,
    extractor: SpeakerEmbeddingExtractor,
    embedding_dim: u32,
}

/// Turn the request's wire value into sherpa's clustering config.
///
/// **The only line in this binary where a bare `f32` and a clustering threshold
/// meet.** No conversion happens: `clustering_distance_threshold` is a cosine
/// distance on the wire and `FastClusteringConfig.threshold` is a cosine
/// distance in sherpa, so writing `1.0 - x` here — the reflex when the plan's
/// enrollment bands are quoted as similarities two paragraphs away — is the
/// whole of merged finding #20. `tests/diarize_wire_unit_discipline.rs` reads
/// this file to hold the line.
fn clustering_from(clustering_distance_threshold: f32) -> FastClusteringConfig {
    FastClusteringConfig {
        num_clusters: CLUSTER_COUNT_UNKNOWN,
        threshold: clustering_distance_threshold,
    }
}

/// Build a diarizer over a model pair.
///
/// `clustering` is passed in as sherpa's own typed struct so this function has
/// no opinion about the number inside it — the load-time probe hands it
/// `FastClusteringConfig::default()` and never processes a sample, and every
/// real pass hands it [`clustering_from`]'s value off the request.
///
/// ## `num_threads` is sherpa's default of **1**, deliberately and expensively
///
/// Measured on this machine (M-series, 12 cores / 8 performance, release build,
/// 183 s of two-voice audio, identical 44 turns at every setting — threads move
/// throughput and nothing else):
///
/// ```text
/// num_threads   1 -> 29.7 s   RTF 0.162
///               2 -> 17.9 s   RTF 0.098
///               4 -> 12.4 s   RTF 0.068
///               8 -> 10.8 s   RTF 0.059
/// ```
///
/// So the shipped default costs roughly 2.7x, and a 45-minute meeting lands
/// near 7 minutes rather than 3. It is still what ships out of this item,
/// because the right number is not "8": diarization runs on a machine that is
/// also transcribing, and a thread budget picked here in isolation is the same
/// class of unmeasured constant this epic exists to stop. YV126 owns it, with
/// the table above as its starting evidence and `meeting_eval`'s fixtures as
/// the place to run the sweep against a real workload.
///
/// The number worth carrying forward regardless: the plan's cited prior for
/// this exact stack is **RTF 0.011** (~30 s for a 45-minute meeting,
/// OpenWhispr), and §2.3 flags it as "a strong prior, not a measurement". It is
/// **15x optimistic** at the shipped setting and still **5x optimistic** at 8
/// threads. That is the real benchmark §2.3 asked for.
fn build_diarizer(
    segmentation: &str,
    embedding: &str,
    clustering: FastClusteringConfig,
) -> Option<OfflineSpeakerDiarization> {
    OfflineSpeakerDiarization::create(&OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(segmentation.to_string()),
            },
            ..Default::default()
        },
        embedding: SpeakerEmbeddingExtractorConfig {
            model: Some(embedding.to_string()),
            ..Default::default()
        },
        clustering,
        ..Default::default()
    })
}

/// Does this file's first bytes look like a serialized ONNX model?
///
/// **This is a sniff, not validation, and it exists because onnxruntime's
/// failure mode is not a return value.** Handing `Ort` a file that is not a
/// model throws a C++ `Ort::Exception` straight through sherpa's C API; there
/// is no Rust frame that can catch it, so `libc++abi` aborts the process
/// (measured: `signal: 6, SIGABRT`, feeding this sidecar its own `Cargo.toml`).
/// A refusal cannot be built out of a return code that never arrives.
///
/// So the realistic bad-bytes cases — an HTML error page saved as `.onnx`, a
/// `.tar.bz2` that was never extracted (YV123's archive path), a zero-length
/// file from an interrupted download — are turned away before onnxruntime sees
/// them. An ONNX file is a protobuf whose first field is `ir_version`: byte
/// `0x08` (field 1, varint) followed by a single-byte version.
///
/// It does not catch a *truncated* valid model, and nothing in this process
/// can. Three layers cover that, and this is only the cheapest: YV123's sha256
/// verification means the parent never names a file whose bytes were not
/// checked, and the process boundary means even an abort costs a restart rather
/// than the app. That containment is the third independent argument for this
/// sidecar existing, and the first one that was measured instead of predicted.
fn looks_like_onnx(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    matches!(bytes.first(), Some(0x08)) && matches!(bytes.get(1), Some(&v) if v < 0x80)
}

/// Load a segmentation + embedding model pair.
///
/// Both models are OPENED here, not merely path-checked, and the embedding
/// width comes off the extractor that was just built. A `load_models` that
/// answered `ok` on a truncated download would hand YV123's vendoring work a
/// green light for bytes that cannot infer, and would turn `sherpa_load_smoke`'s
/// width assertions (`dim == 512` for CAM++, explicitly `dim != 192`, and
/// `== 256` for the ResNet34 control) into claims about a filename.
///
/// The segmentation model is proved by building a diarizer and dropping it
/// again — it is a local, so no clustering config from this function can reach
/// a labeling decision. That probe is the reason `load_models` costs a second
/// ONNX session; the alternative is discovering a bad segmentation file on the
/// first `diarize` of a three-hour meeting.
fn load_backend(segmentation: &Path, embedding: &Path) -> Result<Backend, &'static str> {
    for path in [segmentation, embedding] {
        if !path.is_file() {
            return Err(ERR_MODEL_NOT_FOUND);
        }
        if !looks_like_onnx(path) {
            return Err(ERR_MODEL_LOAD_FAILED);
        }
    }
    let segmentation_path = segmentation.to_string_lossy().into_owned();
    let embedding_path = embedding.to_string_lossy().into_owned();

    let extractor = SpeakerEmbeddingExtractor::create(&SpeakerEmbeddingExtractorConfig {
        model: Some(embedding_path.clone()),
        ..Default::default()
    })
    .ok_or(ERR_MODEL_LOAD_FAILED)?;
    let dim = extractor.dim();
    if dim <= 0 {
        return Err(ERR_MODEL_LOAD_FAILED);
    }

    // The probe. Built, checked, dropped — see the doc comment.
    let probe = build_diarizer(
        &segmentation_path,
        &embedding_path,
        FastClusteringConfig::default(),
    )
    .ok_or(ERR_MODEL_LOAD_FAILED)?;
    let model_rate = probe.sample_rate();
    drop(probe);
    if model_rate <= 0 {
        return Err(ERR_MODEL_LOAD_FAILED);
    }
    eprintln!("yap-diarize: models loaded, embedding_dim={dim}, segmentation rate={model_rate}Hz");

    Ok(Backend {
        segmentation_path,
        embedding_path,
        extractor,
        embedding_dim: dim as u32,
    })
}

/// Segment one track, cluster the turns, and embed each one.
///
/// Takes the whole request rather than an unpacked threshold: an `f32`
/// parameter named for a threshold is exactly what the unit discipline forbids
/// outside [`clustering_from`], and passing the request means the wire value
/// reaches sherpa without a stop in between where a sign could flip.
fn diarize_wav(backend: &Backend, req: &DiarizeRequest) -> Result<Vec<DiarizeSegment>, &'static str> {
    let wav = req.wav_path.as_deref().ok_or(ERR_MISSING_FIELD)?;
    let clustering = clustering_from(
        req.clustering_distance_threshold
            .ok_or(ERR_MISSING_FIELD)?,
    );
    let wave = Wave::read(wav).ok_or(ERR_AUDIO_UNREADABLE)?;

    let diarization = build_diarizer(
        &backend.segmentation_path,
        &backend.embedding_path,
        clustering,
    )
    .ok_or(ERR_MODEL_LOAD_FAILED)?;

    // `process` takes samples and NO sample rate: it reads them at whatever the
    // segmentation model runs at. Feeding it 44.1 kHz audio for a 16 kHz model
    // returns turn boundaries that are silently 2.76x off, at times that still
    // look like times. Refuse instead — resampling belongs upstream, behind the
    // anti-alias filter OS-8 shipped, not in a guess made here.
    let model_rate = diarization.sample_rate();
    if wave.sample_rate() != model_rate {
        return Err(ERR_SAMPLE_RATE);
    }

    let samples = wave.samples();
    let result = diarization
        .process(samples)
        .ok_or(ERR_BACKEND_FAILED)?;

    Ok(result
        .sort_by_start_time()
        .into_iter()
        .map(|turn| DiarizeSegment {
            start: turn.start as f64,
            end: turn.end as f64,
            // sherpa numbers speakers from 0 within one pass. Cluster ids are
            // local to this call — cluster 0 of two meetings is two people
            // until an enrollment match says otherwise (YV129).
            cluster: turn.speaker.max(0) as u32,
            embedding: embed_span(backend, samples, model_rate, turn.start, turn.end),
        })
        .collect())
}

/// The embedding for one turn's span of `samples`, or an EMPTY vector.
///
/// Empty is a real answer here and not an error: pyannote's `min_duration_on`
/// admits turns shorter than CAM++ needs to compute anything, and failing the
/// whole meeting because one 0.4 s "yeah" has no voiceprint would throw away
/// every turn that does. The turn's times and cluster are still measured, so it
/// still belongs in the transcript; `DiarizeResponse::into_embedding` already
/// treats an empty vector as "not an embedding", which is what YV129's
/// enrollment matching reads.
fn embed_span(backend: &Backend, samples: &[f32], rate: i32, start: f32, end: f32) -> Vec<f32> {
    let index = |seconds: f32| -> usize {
        if seconds <= 0.0 {
            return 0;
        }
        ((seconds * rate as f32) as usize).min(samples.len())
    };
    let (lo, hi) = (index(start), index(end));
    if hi <= lo {
        return Vec::new();
    }
    match compute_embedding(backend, &samples[lo..hi], rate) {
        Ok(embedding) => embedding,
        Err(tag) => {
            eprintln!("yap-diarize: turn at {start:.2}s..{end:.2}s has no embedding ({tag})");
            Vec::new()
        }
    }
}

/// One embedding over one span of audio.
///
/// The sample rate is handed to `accept_waveform` rather than checked against a
/// constant: unlike the diarizer's `process`, this API takes the rate, so the
/// extractor is TOLD what it is being given instead of assuming. That is also
/// why `embed` has no sample-rate refusal while `diarize` does — the asymmetry
/// is in sherpa's own signatures, not a policy invented here.
fn compute_embedding(
    backend: &Backend,
    samples: &[f32],
    rate: i32,
) -> Result<Vec<f32>, &'static str> {
    let stream = backend
        .extractor
        .create_stream()
        .ok_or(ERR_BACKEND_FAILED)?;
    stream.accept_waveform(rate, samples);
    stream.input_finished();
    if !backend.extractor.is_ready(&stream) {
        return Err(ERR_AUDIO_TOO_SHORT);
    }
    backend
        .extractor
        .compute(&stream)
        .ok_or(ERR_BACKEND_FAILED)
}

/// Embed one whole enrollment utterance — no segmentation, no clustering: the
/// caller already knows this audio is one person (YV129).
fn embed_wav(backend: &Backend, req: &DiarizeRequest) -> Result<Vec<f32>, &'static str> {
    let wav = req.wav_path.as_deref().ok_or(ERR_MISSING_FIELD)?;
    let wave = Wave::read(wav).ok_or(ERR_AUDIO_UNREADABLE)?;
    compute_embedding(backend, wave.samples(), wave.sample_rate())
}

fn main() {
    // No arguments at all. A caller passing one is a version skew between the
    // app and a stale staged binary, and guessing at what it meant is worse
    // than refusing.
    if let Some(unexpected) = std::env::args().nth(1) {
        eprintln!("yap-diarize: unknown argument '{unexpected}'");
        eprintln!("usage: yap-diarize   (models are loaded by request, not by argv)");
        std::process::exit(2);
    }
    if let Err(e) = run() {
        eprintln!("yap-diarize: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    // The handshake, on the PROTOCOL stream. The parent cannot tell "still
    // starting" from "wedged" by watching a silent pipe, and a log line must
    // never be load-bearing — YV75 learned that on the polish path the
    // expensive way.
    let ready = DiarizeReady::new(env!("CARGO_PKG_VERSION"));
    let line = serde_json::to_string(&ready).map_err(|e| format!("encode ready: {e}"))?;
    let mut stdout = std::io::stdout();
    writeln!(stdout, "{line}").map_err(|e| format!("stdout: {e}"))?;
    stdout.flush().map_err(|e| format!("stdout flush: {e}"))?;
    eprintln!("yap-diarize ready: v{}", env!("CARGO_PKG_VERSION"));
    serve()
}

/// Read requests until stdin closes. Every line produces exactly one response
/// line whenever an id can be recovered, so the parent never waits out its
/// deadline for a line this process silently dropped.
fn serve() -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut backend: Option<Backend> = None;

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("stdin: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<DiarizeRequest>(&line) {
            Ok(req) => handle(&mut backend, &req),
            // Recover the id if we can, so the caller can stop waiting. With no
            // id there is nothing to answer and nothing to correlate — drop it.
            Err(_) => match recover_id(&line) {
                Some(id) => DiarizeResponse::err(id, ERR_BAD_REQUEST),
                None => continue,
            },
        };
        let encoded = serde_json::to_string(&response).map_err(|e| format!("encode: {e}"))?;
        writeln!(stdout, "{encoded}").map_err(|e| format!("stdout: {e}"))?;
        stdout.flush().map_err(|e| format!("stdout flush: {e}"))?;
    }
    Ok(())
}

/// One request, one response. Never panics and never exits: a refusal is an
/// answer, and a sidecar that dies on a bad line spends the parent's restart
/// budget on a typo.
fn handle(backend: &mut Option<Backend>, req: &DiarizeRequest) -> DiarizeResponse {
    let started = Instant::now();
    match req.kind.as_str() {
        KIND_LOAD_MODELS => {
            let (Some(segmentation), Some(embedding)) =
                (req.segmentation_path.as_deref(), req.embedding_path.as_deref())
            else {
                return DiarizeResponse::err(req.id, ERR_MISSING_FIELD);
            };
            match load_backend(Path::new(segmentation), Path::new(embedding)) {
                Ok(loaded) => {
                    let dim = loaded.embedding_dim;
                    *backend = Some(loaded);
                    DiarizeResponse::loaded(req.id, dim, started.elapsed().as_millis() as u64)
                }
                // A failed load leaves ANY previously loaded pair in place: a
                // parent that asks for a second pair and is refused still has
                // the one it had, and `no_models` would be a lie.
                Err(tag) => DiarizeResponse::err(req.id, tag),
            }
        }
        KIND_DIARIZE | KIND_EMBED => {
            let Some(wav) = req.wav_path.as_deref() else {
                return DiarizeResponse::err(req.id, ERR_MISSING_FIELD);
            };
            let Some(loaded) = backend.as_ref() else {
                return DiarizeResponse::err(req.id, ERR_NO_MODELS);
            };
            if !Path::new(wav).is_file() {
                return DiarizeResponse::err(req.id, ERR_AUDIO_NOT_FOUND);
            }
            let ms = || started.elapsed().as_millis() as u64;
            if req.kind == KIND_DIARIZE {
                match diarize_wav(loaded, req) {
                    Ok(segments) => DiarizeResponse::diarized(req.id, segments, ms()),
                    Err(tag) => DiarizeResponse::err(req.id, tag),
                }
            } else {
                match embed_wav(loaded, req) {
                    Ok(embedding) => DiarizeResponse::embedded(req.id, embedding, ms()),
                    Err(tag) => DiarizeResponse::err(req.id, tag),
                }
            }
        }
        // An unknown kind is a version skew between the app and a stale staged
        // sidecar. Answer, so the caller stops waiting, and run nothing.
        _ => DiarizeResponse::err(req.id, ERR_UNSUPPORTED_KIND),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **stdout is the protocol.** One stray `println!` — a debug line, a
    /// progress counter, a library that logs helpfully — puts non-JSON on the
    /// stream the parent de-multiplexes responses from. `yap-polish` has to
    /// silence llama.cpp's logging for exactly this reason; here the rule is
    /// pinned at the source level, which is the only place it can be checked
    /// without a resident model.
    #[test]
    fn stdout_carries_the_protocol_and_nothing_else() {
        // Assembled at runtime so this assertion cannot match its own source.
        let allowed = format!("e{}!(", "println");
        let banned = format!("{}!(", "println");
        for (n, line) in include_str!("main.rs").lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            // `eprintln!` ENDS in the banned needle, so the stderr calls are
            // removed before the search rather than special-cased inside it.
            assert!(
                !code.replace(&allowed, "").contains(&banned),
                "main.rs:{}: stdout carries JSON only — diagnostics go to stderr: {}",
                n + 1,
                line.trim()
            );
        }
    }

    /// Bad bytes are turned away BEFORE onnxruntime, because after it there is
    /// no turning away.
    ///
    /// The three shapes here are the three realistic corrupt-download cases:
    /// a `.tar.bz2` that was never extracted (YV123's archive path is exactly
    /// where that happens), an HTML error page saved under an `.onnx` name, and
    /// a zero-length file from an interrupted transfer. Each one, handed to
    /// `Ort`, is a C++ exception and a `SIGABRT` — not a `None` this process
    /// could answer `model_load_failed` to.
    #[test]
    fn bytes_that_are_not_an_onnx_model_never_reach_onnxruntime() {
        let dir = std::env::temp_dir().join("yap-diarize-sniff");
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let cases: [(&str, &[u8]); 4] = [
            ("archive.onnx", b"BZh91AY&SY"),
            ("error-page.onnx", b"<!DOCTYPE html><html><body>404"),
            ("empty.onnx", b""),
            // A protobuf-shaped first byte with a MULTI-byte varint after it:
            // close enough to fool a one-byte check, which is why the second
            // byte is checked too.
            ("nearly.onnx", &[0x08, 0xff, 0xff, 0xff]),
        ];
        for (name, bytes) in cases {
            let path = dir.join(name);
            std::fs::write(&path, bytes).expect("write fixture");
            assert!(!looks_like_onnx(&path), "{name} must not pass the sniff");

            let mut backend = None;
            let response = handle(&mut backend, &DiarizeRequest::load_models(1, &path, &path));
            assert_eq!(response.err_tag(), Some(ERR_MODEL_LOAD_FAILED), "{name}");
            assert_eq!(response.embedding_dim, None, "{name}");
            assert!(backend.is_none(), "{name}");
        }

        // …and the sniff is not simply "always false": a real ONNX header
        // passes it. (`sherpa_load_smoke` is where a real FILE is opened.)
        let real = dir.join("header.onnx");
        std::fs::write(&real, [0x08, 0x07, 0x12, 0x07]).expect("write fixture");
        assert!(looks_like_onnx(&real), "an ir_version header must pass");
    }

    /// A file that EXISTS but is not a model reports no dimension.
    ///
    /// This is the assertion YV121 wrote as `no_backend` and YV122 has to keep
    /// meaning something: `embedding_dim` may only ever be a number read off an
    /// opened model. Two real files that are certainly not ONNX go in; the
    /// answer is a load failure, the response carries no width, and nothing is
    /// held — which is what makes `sherpa_load_smoke`'s width assertions
    /// (`== 512` on CAM++, `!= 192`, `== 256` on the ResNet34 control) a
    /// measurement rather than a restatement of the plan's arithmetic.
    #[test]
    fn a_file_that_is_not_a_model_reports_no_dimension() {
        let mut backend = None;
        // Two files that certainly exist: this crate's own manifest and source.
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
        let req = DiarizeRequest::load_models(1, &manifest, &source);
        let response = handle(&mut backend, &req);
        assert!(!response.ok, "a TOML file is not a speaker embedding model");
        assert_eq!(response.err_tag(), Some(ERR_MODEL_LOAD_FAILED));
        assert_eq!(response.embedding_dim, None);
        assert!(backend.is_none(), "nothing was loaded, so nothing is held");
    }

    /// The clustering threshold is passed through, never converted.
    ///
    /// `clustering_from` is the single point where the wire's `f32` becomes
    /// sherpa's `FastClusteringConfig.threshold`, and both are cosine
    /// DISTANCES. The failure this guards is one character wide — `1.0 - x` —
    /// and it is invisible everywhere else: 0.35 and 0.65 are both plausible
    /// numbers, and a meeting clustered at the wrong one produces a plausible
    /// number of plausible speakers.
    #[test]
    fn the_clustering_distance_reaches_sherpa_unconverted() {
        // 0.5 is deliberately NOT an input: it is its own similarity twin, so
        // an inverted implementation passes on it. That it is also sherpa's
        // `FastClusteringConfig::default()` is the reason a resident diarizer
        // carrying that default would have hidden this bug rather than shown it.
        for distance in [0.0_f32, 0.35, 0.65, 1.0] {
            let config = clustering_from(distance);
            assert_eq!(
                config.threshold, distance,
                "a cosine DISTANCE on the wire is a cosine distance in sherpa \
                 (merged finding #20); {distance} must not arrive as \
                 {}",
                config.threshold
            );
            assert_ne!(
                config.threshold,
                1.0 - distance,
                "the similarity twin of {distance} reached FastClusteringConfig"
            );
        }
        // And the speaker count is never fixed: the threshold path is the whole
        // point (plan §2.3), so `num_clusters` stays sherpa's "unknown".
        assert_eq!(clustering_from(0.35).num_clusters, CLUSTER_COUNT_UNKNOWN);
        assert!(CLUSTER_COUNT_UNKNOWN < 0);
    }

    /// No clustering number is written down in this file.
    ///
    /// The EVAL-DISCIPLINE rule for this epic is that every threshold is tuned
    /// against YV120's harness (YV126 for clustering), never inherited from a
    /// vendor default or a blog. sherpa's own `FastClusteringConfig::default()`
    /// is 0.5; the moment a float literal lands next to `threshold` here, this
    /// binary has an opinion nobody measured, and the parent's value silently
    /// stops mattering.
    #[test]
    fn no_clustering_threshold_literal_lives_in_this_file() {
        for (n, line) in include_str!("main.rs").lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            if !code.contains("threshold") {
                continue;
            }
            let has_float_literal = code
                .split(|c: char| !(c.is_ascii_digit() || c == '.'))
                .any(|token| token.contains('.') && token.trim_matches('.').parse::<f32>().is_ok());
            assert!(
                !has_float_literal,
                "main.rs:{}: the clustering threshold is the PARENT's to send \
                 and YV126's to tune, never a literal here: {}",
                n + 1,
                line.trim()
            );
        }
    }

    /// A path that is not there is its own answer, distinct from "this build
    /// has no backend" — YV123's vendoring work needs to tell them apart, and
    /// the tag must never carry the path itself.
    #[test]
    fn a_missing_model_is_a_clean_refusal_that_names_no_path() {
        let mut backend = None;
        let missing = Path::new("/nonexistent/yap-diarize-fixture/segmentation.onnx");
        let req = DiarizeRequest::load_models(4, missing, missing);
        let response = handle(&mut backend, &req);
        assert_eq!(response.err_tag(), Some(ERR_MODEL_NOT_FOUND));
        let encoded = serde_json::to_string(&response).expect("encode");
        assert!(
            !encoded.contains("nonexistent") && !encoded.contains(".onnx"),
            "an error tag must never carry a path: {encoded}"
        );
    }

    /// Every other way a request can be wrong gets an ANSWER, never a crash and
    /// never silence — the parent is sitting on a deadline for each one.
    #[test]
    fn every_malformed_request_still_gets_an_answer() {
        let mut backend = None;
        // A kind this build does not implement.
        let skewed = DiarizeRequest {
            id: 5,
            kind: "transcribe".to_string(),
            segmentation_path: None,
            embedding_path: None,
            wav_path: None,
            clustering_distance_threshold: None,
        };
        assert_eq!(handle(&mut backend, &skewed).err_tag(), Some(ERR_UNSUPPORTED_KIND));

        // The right kind, missing the field that kind needs.
        let mut headless = DiarizeRequest::load_models(6, Path::new("/a"), Path::new("/b"));
        headless.embedding_path = None;
        assert_eq!(handle(&mut backend, &headless).err_tag(), Some(ERR_MISSING_FIELD));

        let mut silent = DiarizeRequest::embed(7, Path::new("/a.wav"));
        silent.wav_path = None;
        assert_eq!(handle(&mut backend, &silent).err_tag(), Some(ERR_MISSING_FIELD));

        // Audio requests before any load: `no_models`, and it is checked BEFORE
        // the file — a caller with neither problem fixed should hear about the
        // one it has to fix first.
        assert_eq!(
            handle(&mut backend, &DiarizeRequest::embed(8, Path::new("/nope.wav"))).err_tag(),
            Some(ERR_NO_MODELS)
        );
        assert_eq!(
            handle(
                &mut backend,
                &DiarizeRequest::diarize(9, Path::new("/nope.wav"), 0.35)
            )
            .err_tag(),
            Some(ERR_NO_MODELS)
        );

        // Every answer carries the id it was asked with, or the parent waits
        // out a deadline it did not need to.
        for (id, req) in [
            (5u64, skewed),
            (6, headless),
            (7, silent),
            (8, DiarizeRequest::embed(8, Path::new("/nope.wav"))),
        ] {
            assert_eq!(handle(&mut backend, &req).id, id);
        }
    }
}
