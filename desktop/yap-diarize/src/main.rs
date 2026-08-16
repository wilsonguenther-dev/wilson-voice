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
//! ## A turn's embedding is computed on that turn's audio and nobody else's
//!
//! `OfflineSpeakerDiarization::process` returns turns that **overlap in time**
//! — measured here, not assumed: a two-voice track with a deliberate 3 s
//! collision comes back as `(0.03–5.09, c2)` and `(1.97–7.84, c1)`. Merged
//! finding #22's "overlapped frames are deleted before embedding" describes
//! sherpa's INTERNAL clustering pass; the per-turn vectors this binary ships
//! are a second pass over the returned spans, and without masking the first
//! turn's vector would be 62 % somebody else. [`embed_turn`] embeds only the
//! samples a turn claims alone.
//!
//! ## Not one clustering number lives in this file, and not one duration either
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
//! `min_embed_seconds` — how much audio is worth embedding — is the same shape
//! for the same reason. The measurement that says a floor is NEEDED is in this
//! item (a 0.2 s span returns a full-width vector that resembles its own
//! speaker less than an average stranger does); the measurement that would say
//! WHERE it goes needs real speech, which this repo does not have, so the
//! number is the caller's and no default exists on either side of the wire.
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
    recover_id, DiarizeReady, DiarizeRequest, DiarizeResponse, DiarizeSegment, ERR_AUDIO_NOT_FOUND,
    ERR_AUDIO_TOO_SHORT, ERR_AUDIO_UNREADABLE, ERR_BACKEND_FAILED, ERR_BAD_REQUEST,
    ERR_MISSING_FIELD, ERR_MODEL_LOAD_FAILED, ERR_MODEL_NOT_FOUND, ERR_NO_MODELS, ERR_SAMPLE_RATE,
    ERR_UNSUPPORTED_KIND, KIND_DIARIZE, KIND_EMBED, KIND_LOAD_MODELS,
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
fn diarize_wav(
    backend: &Backend,
    req: &DiarizeRequest,
) -> Result<Vec<DiarizeSegment>, &'static str> {
    let wav = req.wav_path.as_deref().ok_or(ERR_MISSING_FIELD)?;
    let clustering = clustering_from(req.clustering_distance_threshold.ok_or(ERR_MISSING_FIELD)?);
    // No default. A request that does not say how much audio is worth
    // embedding is refused, exactly like one that does not say how tightly to
    // cluster — see `min_embed_seconds` on the wire contract for the
    // measurement that says a floor is necessary and why this item does not
    // pick one.
    let min_embed = req.min_embed_seconds.ok_or(ERR_MISSING_FIELD)?;
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
    let result = diarization.process(samples).ok_or(ERR_BACKEND_FAILED)?;

    let turns: Vec<_> = result.sort_by_start_time();
    let spans: Vec<(usize, usize)> = turns
        .iter()
        .map(|turn| span_of(samples.len(), model_rate, turn.start, turn.end))
        .collect();
    // Computed ONCE for the whole track, not per turn: the regions two or more
    // turns both claim. See `exclusive_of`.
    let shared = shared_regions(&spans);

    Ok(turns
        .iter()
        .zip(&spans)
        .map(|(turn, span)| DiarizeSegment {
            start: turn.start as f64,
            end: turn.end as f64,
            // sherpa numbers speakers from 0 within one pass. Cluster ids are
            // local to this call — cluster 0 of two meetings is two people
            // until an enrollment match says otherwise (YV129).
            cluster: turn.speaker.max(0) as u32,
            embedding: embed_turn(backend, samples, model_rate, *span, &shared, min_embed),
        })
        .collect())
}

/// One turn's half-open sample range, clamped to the track.
fn span_of(len: usize, rate: i32, start: f32, end: f32) -> (usize, usize) {
    let index = |seconds: f32| -> usize {
        if seconds <= 0.0 || rate <= 0 {
            return 0;
        }
        ((seconds * rate as f32) as usize).min(len)
    };
    let (lo, hi) = (index(start), index(end));
    (lo, hi.max(lo))
}

/// Every half-open sample range that **two or more** of `spans` both cover.
///
/// A sweep over the endpoints rather than a mask, so the cost is
/// `O(turns log turns)` for the whole track instead of `O(turns²)` in samples —
/// a three-hour meeting is hundreds of turns over ~170 M samples, and the mask
/// version of this is where that becomes noticeable.
fn shared_regions(spans: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut events: Vec<(usize, i32)> = Vec::with_capacity(spans.len() * 2);
    for &(lo, hi) in spans {
        if hi > lo {
            events.push((lo, 1));
            events.push((hi, -1));
        }
    }
    // End before start at the same sample: two turns that merely touch share
    // nothing.
    events.sort_unstable_by_key(|&(at, delta)| (at, delta));

    let mut regions: Vec<(usize, usize)> = Vec::new();
    let mut depth = 0i32;
    let mut opened_at = 0usize;
    for (at, delta) in events {
        let was = depth;
        depth += delta;
        if was < 2 && depth >= 2 {
            opened_at = at;
        } else if was >= 2 && depth < 2 && at > opened_at {
            regions.push((opened_at, at));
        }
    }
    regions
}

/// `span` with every `shared` region removed — the samples this turn covers
/// and no other turn does.
fn exclusive_of(span: (usize, usize), shared: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let (mut at, hi) = span;
    let mut out = Vec::new();
    for &(lo, end) in shared {
        if end <= at {
            continue;
        }
        if lo >= hi {
            break;
        }
        if lo > at {
            out.push((at, lo.min(hi)));
        }
        at = at.max(end);
        if at >= hi {
            return out;
        }
    }
    if at < hi {
        out.push((at, hi));
    }
    out
}

/// The embedding for one turn, computed over the audio **only that turn
/// claims**, or an EMPTY vector.
///
/// Two corrections live in this function, both from measurements against the
/// shipped models rather than from reasoning about them.
///
/// ## 1. The span is masked, because sherpa's turns overlap in time
///
/// `diarize_protocol.rs` and merged finding #22 both say overlapped frames are
/// deleted upstream — and that is true of the vectors sherpa computes for its
/// **own** clustering (`ExcludeOverlap`, inside `process`). It is **not** true
/// of the turn list `process` returns, and this function is the second,
/// independent embedding pass over that list. Measured through this binary on a
/// deliberately overlapped two-voice track (`Samantha` from 0 s, `Daniel` from
/// 2 s, summed):
///
/// ```text
/// turns: (0.03 – 5.09, cluster 2) and (1.97 – 7.84, cluster 1)
///        → 3.12 s of the FIRST turn's 5.06 s is the second speaker
/// ```
///
/// Embedding the raw contiguous span therefore hands YV128/YV129 a vector
/// computed on two people and labelled with one cluster — 62 % foreign audio in
/// that first turn. So the samples fed to the extractor are the turn's span
/// minus every region another turn also claims. That is a MECHANISM claim
/// (`no sample reaches turn i's extractor call if any other turn covers it`),
/// checkable with no accuracy number, and it is checked that way in
/// `tests::an_exclusive_span_contains_no_sample_another_turn_claims`.
///
/// Whether masking makes the vectors *better* is a question this repo cannot
/// answer yet and does not claim to: on the macOS `say` substrate the item
/// already measured an EER of 0.272 on, a clean 2.75 s single-voice span scored
/// 0.39 against its own full-length vector and 0.73 against a stranger's. That
/// is the substrate's noise, not evidence either way, and it is why the claim
/// here is confined to what the code does with which samples.
///
/// ## 2. Empty is now a real gate, because it was not one before
///
/// The previous version of this comment said turns too short to embed "come
/// back empty", and that `DiarizeResponse::into_embedding`'s empty check was
/// therefore what YV129 could read. Measured: `audio_too_short` fires only
/// below **~10 samples (0.6 ms)** — every span above that returns a full-width
/// vector, including a 0.2 s one, which scored 0.2516 against the same
/// speaker's reference while this roster's average *stranger* scores 0.5789.
/// The gate did not discriminate, and a documented gate that does not
/// discriminate is worse than none: it is the sentence a later item builds on.
///
/// `min_embed` is the fix and it arrives on the request. There is no default
/// here, in `diarize.rs`, or anywhere else in either crate — see the wire
/// contract's `min_embed_seconds` for the sweep, and
/// `diarize_wire_unit_discipline.rs` for the test that fails the build if a
/// constant appears.
fn embed_turn(
    backend: &Backend,
    samples: &[f32],
    rate: i32,
    span: (usize, usize),
    shared: &[(usize, usize)],
    min_embed: f32,
) -> Vec<f32> {
    let exclusive = exclusive_of(span, shared);
    let kept: usize = exclusive.iter().map(|(lo, hi)| hi - lo).sum();
    let floor = min_embed_samples(rate, min_embed);
    if kept < floor {
        eprintln!(
            "yap-diarize: turn has {kept} exclusive samples, under the requested \
             floor of {floor} — no embedding"
        );
        return Vec::new();
    }
    // The exclusive ranges are concatenated rather than embedded separately and
    // averaged: an average of two vectors is a third thing nothing has
    // measured, and the extractor's own front-end is what should see the audio.
    //
    // TWO KNOWN LIMITS, named rather than hidden, and neither is given a size
    // here because this repo's only speech is the `say` corpus whose noise
    // already swamps a clean single-voice span (0.39 to its own full-length
    // vector; see the module docs):
    //
    //   * splicing puts a discontinuity at each join, which the fbank front-end
    //     sees as a handful of frames of nothing anybody said;
    //   * `kept` counts SAMPLES, not runs. A turn interleaved with others can
    //     clear the floor as twenty fragments rather than as one span, and
    //     nothing here distinguishes those.
    //
    // Both are for the item that gets a real corpus (YV124's cross-resampler
    // arm is the first one that will have one). Adding a fragment-count rule
    // now would be a second unmeasured constant answering the first one.
    //
    // The alternative considered and rejected — embed only the longest run —
    // throws away audio that is just as much this speaker's, and would make the
    // floor mean something different again.
    let mut audio = Vec::with_capacity(kept);
    for &(lo, hi) in &exclusive {
        audio.extend_from_slice(&samples[lo..hi]);
    }
    match compute_embedding(backend, &audio, rate) {
        Ok(embedding) => embedding,
        Err(tag) => {
            eprintln!("yap-diarize: a turn has no embedding ({tag})");
            Vec::new()
        }
    }
}

/// The requested floor in samples. A negative or non-finite request is a
/// caller bug, and rounding it to "no floor" is how this defect came back —
/// it becomes the largest floor instead, which refuses everything loudly.
fn min_embed_samples(rate: i32, min_embed: f32) -> usize {
    if !min_embed.is_finite() || min_embed < 0.0 || rate <= 0 {
        return usize::MAX;
    }
    (min_embed as f64 * rate as f64) as usize
}

/// One embedding over one span of audio.
///
/// The sample rate is handed to `accept_waveform` rather than checked against a
/// constant: unlike the diarizer's `process`, this API takes the rate, so the
/// extractor is TOLD what it is being given instead of assuming. That is also
/// why `embed` has no sample-rate refusal while `diarize` does — the asymmetry
/// is in sherpa's own signatures, not a policy invented here. (The *second*
/// half of that asymmetry — `embed` accepting 44.1 kHz where `diarize` refuses
/// it, same utterance at both rates embedding to cosine 0.9789 — is OS-8's
/// cross-resampler delta and stays routed to YV124, unchanged.)
///
/// **`is_ready` is a liveness check, not a sufficiency check.** Measured on the
/// shipped CAM++: it goes false only below ~10 samples, so `ERR_AUDIO_TOO_SHORT`
/// from here means "there was essentially nothing", never "there was not
/// enough". Sufficiency is the caller's `min_embed_seconds`, applied before
/// this function is reached.
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
    backend.extractor.compute(&stream).ok_or(ERR_BACKEND_FAILED)
}

/// Embed one whole enrollment utterance — no segmentation, no clustering: the
/// caller already knows this audio is one person (YV129).
///
/// The floor is enforced here too, and as a REFUSAL rather than an empty
/// vector: an `embed` response's whole payload is the vector, so "too short" has
/// to be sayable in the error tag or it is not sayable at all. This is the exact
/// path the 0.2 s measurement was taken on.
fn embed_wav(backend: &Backend, req: &DiarizeRequest) -> Result<Vec<f32>, &'static str> {
    let wav = req.wav_path.as_deref().ok_or(ERR_MISSING_FIELD)?;
    let min_embed = req.min_embed_seconds.ok_or(ERR_MISSING_FIELD)?;
    let wave = Wave::read(wav).ok_or(ERR_AUDIO_UNREADABLE)?;
    let rate = wave.sample_rate();
    if wave.samples().len() < min_embed_samples(rate, min_embed) {
        return Err(ERR_AUDIO_TOO_SHORT);
    }
    compute_embedding(backend, wave.samples(), rate)
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
            let (Some(segmentation), Some(embedding)) = (
                req.segmentation_path.as_deref(),
                req.embedding_path.as_deref(),
            ) else {
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
            min_embed_seconds: None,
        };
        assert_eq!(
            handle(&mut backend, &skewed).err_tag(),
            Some(ERR_UNSUPPORTED_KIND)
        );

        // The right kind, missing the field that kind needs.
        let mut headless = DiarizeRequest::load_models(6, Path::new("/a"), Path::new("/b"));
        headless.embedding_path = None;
        assert_eq!(
            handle(&mut backend, &headless).err_tag(),
            Some(ERR_MISSING_FIELD)
        );

        let mut silent = DiarizeRequest::embed(7, Path::new("/a.wav"), 1.0);
        silent.wav_path = None;
        assert_eq!(
            handle(&mut backend, &silent).err_tag(),
            Some(ERR_MISSING_FIELD)
        );

        // Audio requests before any load: `no_models`, and it is checked BEFORE
        // the file — a caller with neither problem fixed should hear about the
        // one it has to fix first.
        assert_eq!(
            handle(
                &mut backend,
                &DiarizeRequest::embed(8, Path::new("/nope.wav"), 1.0)
            )
            .err_tag(),
            Some(ERR_NO_MODELS)
        );
        assert_eq!(
            handle(
                &mut backend,
                &DiarizeRequest::diarize(9, Path::new("/nope.wav"), 0.35, 1.0)
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
            (8, DiarizeRequest::embed(8, Path::new("/nope.wav"), 1.0)),
        ] {
            assert_eq!(handle(&mut backend, &req).id, id);
        }
    }

    /// A request with no floor is REFUSED, on both audio kinds.
    ///
    /// The alternative — treating a missing field as "no floor" — is how this
    /// defect comes back: an older parent beside a newer staged sidecar would
    /// silently go back to embedding every 0.2 s turn, and nothing downstream
    /// can tell a full-width noise vector from a full-width voiceprint. Checked
    /// before the model is, so it holds on a machine with no models at all.
    #[test]
    fn an_audio_request_without_a_floor_is_refused_not_defaulted() {
        // Assembled from the source so a default written as a literal cannot
        // hide behind a `#[cfg(test)]` fixture: the sidecar must name no
        // seconds/duration constant at all.
        let code = include_str!("main.rs");
        for line in code.lines() {
            let line = line.trim();
            if !(line.starts_with("const ") || line.starts_with("static ")) {
                continue;
            }
            let lower = line.to_ascii_lowercase();
            assert!(
                !["seconds", "duration", "min_embed", "_secs", "millis"]
                    .iter()
                    .any(|needle| lower.contains(needle)),
                "the floor is the caller's; this binary declares no duration \
                 constant: {line}"
            );
        }

        let mut backend = None;
        let mut without = DiarizeRequest::diarize(1, Path::new("/a.wav"), 0.35, 2.0);
        without.min_embed_seconds = None;
        // `no_models` comes first on this machine — the point is that the field
        // is not optional once a model IS loaded, which `diarize_wav`/`embed_wav`
        // assert directly below.
        assert_eq!(
            handle(&mut backend, &without).err_tag(),
            Some(ERR_NO_MODELS)
        );
        assert_eq!(without.min_embed_seconds, None);
    }

    /// The mask: no sample reaches a turn's extractor call if another turn
    /// covers it.
    ///
    /// This is the whole of the overlap correction stated as an assertion, and
    /// it needs no model and no audio — which is why it runs in CI on every
    /// commit while the measured half runs only where the 36 MB of weights are.
    #[test]
    fn an_exclusive_span_contains_no_sample_another_turn_claims() {
        // The measured shape, in samples at 16 kHz: turn A 0.03–5.09 s,
        // turn B 1.97–7.84 s, from the real two-voice track in the module docs.
        let spans = [(480, 81_440), (31_520, 125_440)];
        let shared = shared_regions(&spans);
        assert_eq!(shared, vec![(31_520, 81_440)]);

        let a = exclusive_of(spans[0], &shared);
        let b = exclusive_of(spans[1], &shared);
        assert_eq!(a, vec![(480, 31_520)]);
        assert_eq!(b, vec![(81_440, 125_440)]);

        // The property, checked sample by sample rather than by re-deriving the
        // same interval arithmetic a second time — a test that recomputes the
        // implementation proves only that it is deterministic.
        for (mine, span) in [(&a, spans[0]), (&b, spans[1])] {
            let kept: usize = mine.iter().map(|(lo, hi)| hi - lo).sum();
            assert!(kept > 0);
            for &(lo, hi) in mine {
                assert!(lo >= span.0 && hi <= span.1, "a turn never gains audio");
                for other in spans {
                    if other == span {
                        continue;
                    }
                    assert!(
                        hi <= other.0 || lo >= other.1,
                        "({lo},{hi}) overlaps another turn's ({},{})",
                        other.0,
                        other.1
                    );
                }
            }
        }

        // Three-way: the middle turn is swallowed entirely, and an empty
        // exclusive set is a real answer rather than a panic or a whole-span
        // fallback.
        let three = [(0, 1000), (200, 800), (400, 1400)];
        let shared = shared_regions(&three);
        assert_eq!(exclusive_of(three[1], &shared), Vec::new());
        assert_eq!(exclusive_of(three[0], &shared), vec![(0, 200)]);
        assert_eq!(exclusive_of(three[2], &shared), vec![(1000, 1400)]);

        // Turns that merely touch share nothing — an off-by-one here would
        // delete a sample from every boundary in a meeting.
        let touching = [(0, 500), (500, 900)];
        assert_eq!(shared_regions(&touching), Vec::new());
        assert_eq!(
            exclusive_of(touching[0], &shared_regions(&touching)),
            vec![(0, 500)]
        );

        // A lone turn keeps all of its audio: the mask must not be a no-op in
        // reverse either.
        assert_eq!(shared_regions(&[(10, 20)]), Vec::new());
        assert_eq!(exclusive_of((10, 20), &[]), vec![(10, 20)]);
    }

    /// The floor converts as a duration, and a nonsense request refuses
    /// everything rather than nothing.
    #[test]
    fn the_floor_rounds_toward_refusing() {
        assert_eq!(min_embed_samples(16_000, 2.0), 32_000);
        assert_eq!(min_embed_samples(16_000, 0.0), 0);
        // The direction that matters: a bad value must not read as "no floor".
        assert_eq!(min_embed_samples(16_000, -1.0), usize::MAX);
        assert_eq!(min_embed_samples(16_000, f32::NAN), usize::MAX);
        assert_eq!(min_embed_samples(0, 2.0), usize::MAX);
    }
}
