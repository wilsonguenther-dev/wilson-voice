//! Wire protocol for the `yap-diarize` sidecar (YV121).
//!
//! Diarization runs in a THIRD process, not in this binary, for the same
//! reason the polish stage does — and this time the collision is already in the
//! tree rather than hypothetical. `vad-rs` runs Silero v4 through `ort`
//! (onnxruntime) and links it **statically** into the app (see
//! `src-tauri/Cargo.toml`'s own comment on that dependency, and `vad.rs`).
//! `sherpa-onnx` — the crate YV122 adds to the sidecar — statically links its
//! **own** vendored onnxruntime. Linking two independently-vendored copies of
//! one C++ runtime into a single binary is the identical duplicate-symbol
//! failure class that forced `yap-polish` out of process, documented verbatim
//! at the head of `polish_protocol.rs` (llama.cpp #9267/#11303/#11491,
//! whisper.cpp #1887). Two link units solve it outright, and the process
//! boundary buys the same second thing it bought there: a wedged or OOM
//! diarizer can be **killed**, which an in-process ONNX session can never be
//! made to do safely, and a diarization that fails degrades to a plain
//! transcript instead of taking the app down.
//!
//! Transport is newline-delimited JSON on the child's stdin/stdout — one
//! request per line, one response per line. No port, no listener, no network
//! surface.
//!
//! ```jsonc
//! // ← stdout, once, as soon as the child is UP — before any model is loaded.
//! {"type":"ready","version":"0.1.0"}
//! // → stdin: load a vendored model pair (YV123 supplies the paths)
//! {"id":1,"kind":"load_models","segmentation_path":"…","embedding_path":"…"}
//! // ← stdout
//! {"id":1,"ok":true,"embedding_dim":192}
//! // → stdin: diarize one track's audio (YV126 wires this to Track A/B)
//! {"id":2,"kind":"diarize","wav_path":"…","clustering_distance_threshold":0.35}
//! // ← stdout
//! {"id":2,"ok":true,"segments":[{"start":0.0,"end":4.2,"cluster":0,"embedding":[…]}]}
//! // → stdin: embed one enrollment utterance (YV129)
//! {"id":3,"kind":"embed","wav_path":"…"}
//! // ← stdout
//! {"id":3,"ok":true,"embedding":[…]}
//! ```
//!
//! ## Readiness is the PROCESS, model load is a separate signal
//!
//! `yap-polish`'s handshake carries `model_loaded` because that sidecar takes
//! its model on argv and cannot answer anything until a GGUF is resident. This
//! one is shaped the other way round: the child takes **no** model argument,
//! announces itself the moment it is up, and loads a model only when a
//! [`KIND_LOAD_MODELS`] request names one. So [`DiarizeReady`] has no
//! `model_loaded` field at all — the answer to "are the models in?" is the
//! response to that request, and nothing else.
//!
//! That split is deliberate. Merging the two would put a multi-second ONNX
//! session build inside the parent's spawn budget, and would make "the process
//! is wedged" indistinguishable from "the model is big" — which is exactly the
//! confusion YV75 had to remove from the polish path after the fact.
//!
//! ## `embedding_dim` is reported, never assumed
//!
//! Audit finding #19 said the shipped `wespeaker_en_voxceleb_CAM++` was **192**
//! wide against the plan's assumed 512. **That is wrong about the file this
//! catalog actually pins.** Read off the artefact `src/catalog.json` pins by
//! sha256 `c46fad10…`, the ONNX metadata says `output_dim = 512` and the graph
//! output is `embs [B, 512]`; the reproduction command is in
//! `docs/yap23-asnorm-measurement.md`. Wherever 192 came from, it does not
//! describe this model.
//!
//! **The mechanism is unchanged and was never the part that was wrong.** The
//! dimension is a value the CHILD reports at load time and the parent stores —
//! there is no dimension constant on the Rust side of this wire, and
//! `tests/diarize_sidecar_pool.rs` holds the parent to that by answering with a
//! number no model has. That discipline is exactly why a wrong number in a
//! comment cost nothing: had 192 been a constant, every YV131 ranking would have
//! degraded silently to raw cosine through `CohortError::DimMismatch`.
//!
//! ## `clustering_distance_threshold` is a DISTANCE
//!
//! JSON has no newtypes, so the field is a bare `f32` here. Every Rust caller
//! wraps it at the boundary: `diarize.rs`'s public API takes a
//! [`CosineDistance`](crate::diarize_metrics::CosineDistance) and calls `.get()`
//! exactly once, when it builds the request. Smaller is more similar — the unit
//! `sherpa_onnx::FastClusteringConfig.threshold` runs in, and the opposite of
//! the similarities the plan's enrollment bands are quoted in (merged finding
//! #20).
//!
//! **This file is the single source of truth for both sides.** `yap-diarize`
//! compiles it directly (`#[path = "../../src-tauri/src/diarize_protocol.rs"]`)
//! rather than keeping a second copy, so the two ends cannot drift. That is why
//! it depends on nothing but `serde` + `std` — no Tauri, no crate-local types,
//! and in particular not `diarize_metrics`, which the sidecar does not compile.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// ── Request kinds ───────────────────────────────────────────────────────────

/// Load a segmentation + embedding model pair. Answers with the embedding
/// dimension the loaded model actually has.
pub const KIND_LOAD_MODELS: &str = "load_models";
/// Diarize one WAV: segment it, embed each segment, cluster the embeddings.
pub const KIND_DIARIZE: &str = "diarize";
/// Embed ONE utterance — the enrollment path (YV129). No segmentation, no
/// clustering: the caller already knows this audio is one person.
pub const KIND_EMBED: &str = "embed";

// ── Error tags ──────────────────────────────────────────────────────────────
//
// Tags, never sentences, and never a path: an `err` string crosses a process
// boundary into the parent's rotating log, and a model path on this machine
// carries the user's account name. `DiarizeResponse::err` takes a
// `&'static str` so a formatted string cannot be passed at all — the
// restriction is in the type, not in a review comment.

/// The line was not a request this build can parse.
pub const ERR_BAD_REQUEST: &str = "bad_request";
/// A `kind` this build does not implement — an app/sidecar version skew.
pub const ERR_UNSUPPORTED_KIND: &str = "unsupported_kind";
/// The request omitted a field its own kind requires.
pub const ERR_MISSING_FIELD: &str = "missing_field";
/// A named model file is not there.
pub const ERR_MODEL_NOT_FOUND: &str = "model_not_found";
/// A named audio file is not there.
pub const ERR_AUDIO_NOT_FOUND: &str = "audio_not_found";
/// A `diarize`/`embed` arrived before any successful `load_models`.
pub const ERR_NO_MODELS: &str = "no_models";
/// This build has no inference backend compiled in (YV121's scaffold). YV122
/// replaces the arm that returns it with the real `sherpa-onnx` load; until
/// then the sidecar says so rather than answering with a plausible shape it
/// did not compute.
pub const ERR_NO_BACKEND: &str = "no_backend";

/// The `type` value of the readiness line. The only message on this wire that
/// is not keyed by a request id, which is how the two are told apart.
pub const READY_KIND: &str = "ready";

/// The sidecar's readiness announcement, written to stdout ONCE, as soon as the
/// process is up and before any request is read.
///
/// No `model_loaded` field, on purpose — see the module docs. A parent that
/// wants to know whether models are resident asks for [`KIND_LOAD_MODELS`] and
/// reads the answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiarizeReady {
    /// Always [`READY_KIND`]. Present so a readiness line and a response line
    /// can never deserialize as each other.
    #[serde(rename = "type")]
    pub kind: String,
    /// The sidecar binary's version — a stale staged binary beside a new app is
    /// a real failure mode once the bundle ships an updater.
    pub version: String,
}

impl DiarizeReady {
    pub fn new(version: &str) -> Self {
        Self {
            kind: READY_KIND.to_string(),
            version: version.to_string(),
        }
    }
}

/// Parse one stdout line as the readiness announcement, or `None` if it is
/// anything else (a response, a stray line, half a line from a child that
/// died).
pub fn parse_ready(line: &str) -> Option<DiarizeReady> {
    let parsed: DiarizeReady = serde_json::from_str(line.trim()).ok()?;
    (parsed.kind == READY_KIND).then_some(parsed)
}

/// One request. Serialized as a single line on the sidecar's stdin.
///
/// One struct for every kind, with the per-kind fields optional and skipped
/// when absent, so the bytes on the wire are exactly the documented shapes and
/// a request cannot be built with another kind's fields set — the constructors
/// below are the only way in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiarizeRequest {
    /// Monotonic per-process request id. A response that does not carry it
    /// answers a job the parent already gave up on and MUST be discarded — see
    /// [`parse_response_for`].
    pub id: u64,
    /// [`KIND_LOAD_MODELS`], [`KIND_DIARIZE`] or [`KIND_EMBED`].
    pub kind: String,
    /// [`KIND_LOAD_MODELS`]: the segmentation model file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segmentation_path: Option<String>,
    /// [`KIND_LOAD_MODELS`]: the speaker-embedding model file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_path: Option<String>,
    /// [`KIND_DIARIZE`] / [`KIND_EMBED`]: the audio to read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wav_path: Option<String>,
    /// [`KIND_DIARIZE`]: the clustering threshold, as a cosine **distance**
    /// (smaller = more similar). A bare `f32` because JSON has no newtypes;
    /// every Rust caller holds it as a
    /// [`CosineDistance`](crate::diarize_metrics::CosineDistance) and converts
    /// here and nowhere else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clustering_distance_threshold: Option<f32>,
}

impl DiarizeRequest {
    /// Load a model pair. Paths are rendered lossily on purpose: a path this
    /// process cannot express as UTF-8 is a path the child could not open
    /// anyway, and a panic here would take the app's diarization down for a
    /// filename.
    pub fn load_models(id: u64, segmentation: &std::path::Path, embedding: &std::path::Path) -> Self {
        Self {
            id,
            kind: KIND_LOAD_MODELS.to_string(),
            segmentation_path: Some(segmentation.to_string_lossy().into_owned()),
            embedding_path: Some(embedding.to_string_lossy().into_owned()),
            wav_path: None,
            clustering_distance_threshold: None,
        }
    }

    /// Diarize one track. `clustering_distance_threshold` is a DISTANCE — the
    /// caller holds a `CosineDistance` and unwraps it here.
    pub fn diarize(id: u64, wav: &std::path::Path, clustering_distance_threshold: f32) -> Self {
        Self {
            id,
            kind: KIND_DIARIZE.to_string(),
            segmentation_path: None,
            embedding_path: None,
            wav_path: Some(wav.to_string_lossy().into_owned()),
            clustering_distance_threshold: Some(clustering_distance_threshold),
        }
    }

    /// Embed one enrollment utterance.
    pub fn embed(id: u64, wav: &std::path::Path) -> Self {
        Self {
            id,
            kind: KIND_EMBED.to_string(),
            segmentation_path: None,
            embedding_path: None,
            wav_path: Some(wav.to_string_lossy().into_owned()),
            clustering_distance_threshold: None,
        }
    }
}

/// One diarized turn. Times are seconds from the start of the WAV.
///
/// There is deliberately **no `overlapped` flag** — see YV127. The segmentation
/// model's own ceiling makes an overlap column something the pipeline could
/// only guess at, and a guessed column that reads as measured is worse than no
/// column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiarizeSegment {
    pub start: f64,
    pub end: f64,
    /// The cluster this turn was assigned. Local to ONE diarize call — cluster
    /// 0 of two different meetings is two different people until an enrollment
    /// match says otherwise (YV129).
    pub cluster: u32,
    /// The turn's speaker embedding. Length is whatever `embedding_dim` the
    /// loaded model reported; nothing on the Rust side assumes a number.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub embedding: Vec<f32>,
}

/// One response line. `ok` discriminates: `true` carries the payload for the
/// request's kind, `false` carries `err`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiarizeResponse {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub err: Option<String>,
    /// [`KIND_LOAD_MODELS`]: the dimension of the embedding model that was just
    /// loaded, as the model reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_dim: Option<u32>,
    /// [`KIND_DIARIZE`]: the turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<DiarizeSegment>>,
    /// [`KIND_EMBED`]: the one embedding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
}

impl DiarizeResponse {
    fn empty(id: u64, ok: bool) -> Self {
        Self {
            id,
            ok,
            err: None,
            embedding_dim: None,
            segments: None,
            embedding: None,
            ms: None,
        }
    }

    /// Models are in, and this is the dimension the embedding model reports.
    pub fn loaded(id: u64, embedding_dim: u32, ms: u64) -> Self {
        Self {
            embedding_dim: Some(embedding_dim),
            ms: Some(ms),
            ..Self::empty(id, true)
        }
    }

    /// A finished diarization. An empty segment list is a legitimate answer
    /// (silence), which is why it is `Some(vec![])` and not `None`.
    pub fn diarized(id: u64, segments: Vec<DiarizeSegment>, ms: u64) -> Self {
        Self {
            segments: Some(segments),
            ms: Some(ms),
            ..Self::empty(id, true)
        }
    }

    /// One enrollment embedding.
    pub fn embedded(id: u64, embedding: Vec<f32>, ms: u64) -> Self {
        Self {
            embedding: Some(embedding),
            ms: Some(ms),
            ..Self::empty(id, true)
        }
    }

    /// A refusal. `tag` is `&'static str` so only the constants above (or
    /// another literal) can be passed — a `format!`ed path physically cannot
    /// reach this wire, and therefore cannot reach the parent's log.
    pub fn err(id: u64, tag: &'static str) -> Self {
        Self {
            err: Some(tag.to_string()),
            ..Self::empty(id, false)
        }
    }

    /// The error tag this response carries, if it is a refusal.
    pub fn err_tag(&self) -> Option<&str> {
        (!self.ok).then(|| self.err.as_deref().unwrap_or(ERR_BAD_REQUEST))
    }

    /// The embedding dimension a successful [`KIND_LOAD_MODELS`] reports.
    /// `None` on any failure and on a success that omitted it — a load that
    /// cannot say how wide its vectors are has not told the parent what it
    /// needs, and inventing a width here — 192, 512 or anything else — is the
    /// exact bug finding #19 is about. (The pinned model measures 512; finding
    /// #19's 192 does not describe it. Neither number belongs in this file.)
    pub fn into_embedding_dim(self) -> Option<u32> {
        self.ok.then_some(self.embedding_dim).flatten()
    }

    /// The turns a successful [`KIND_DIARIZE`] carries.
    pub fn into_segments(self) -> Option<Vec<DiarizeSegment>> {
        self.ok.then_some(self.segments).flatten()
    }

    /// The embedding a successful [`KIND_EMBED`] carries. An empty vector is
    /// not an embedding.
    pub fn into_embedding(self) -> Option<Vec<f32>> {
        self.ok
            .then_some(self.embedding)
            .flatten()
            .filter(|e| !e.is_empty())
    }
}

/// Parse one response line, accepting it only when it answers `expect_id`.
///
/// The child is fed many requests over one job. A response that arrives after
/// the parent gave up is still sitting in the pipe when the NEXT request's read
/// starts; returning it would attribute one meeting's turns to another's audio.
/// Unparseable lines (a stray diagnostic, half a line from a child that died)
/// are dropped the same way.
pub fn parse_response_for(line: &str, expect_id: u64) -> Option<DiarizeResponse> {
    let parsed: DiarizeResponse = serde_json::from_str(line.trim()).ok()?;
    (parsed.id == expect_id).then_some(parsed)
}

/// Recover just the `id` from a line that failed to parse as a request, so a
/// malformed line can still be ANSWERED and the parent stops waiting on it.
pub fn recover_id(line: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(line.trim())
        .ok()?
        .get("id")?
        .as_u64()
}

// These tests are compiled and run by BOTH ends of the wire — by
// `wilson-voice` as library tests, and by `yap-diarize`, which compiles this
// same file through `#[path]`. That is the point: the contract is checked from
// inside each binary that has to honour it.
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The two message shapes on this stream must be mutually unparseable, or a
    /// readiness line could be handed to a caller waiting on a response.
    #[test]
    fn diarize_protocol_ready_line_is_distinct_from_a_response() {
        let ready = serde_json::to_string(&DiarizeReady::new("0.1.0")).expect("encode");
        let response = serde_json::to_string(&DiarizeResponse::loaded(1, 192, 4)).expect("encode");

        assert!(parse_ready(&ready).is_some());
        assert!(parse_ready(&response).is_none(), "a response is not a ready");
        assert!(parse_response_for(&ready, 1).is_none(), "a ready is not a response");
        assert!(parse_response_for(&response, 1).is_some());
        // …and a ready line carries no model claim at all: this sidecar's model
        // state is the answer to `load_models`, never the handshake.
        assert!(
            !ready.contains("model_loaded"),
            "readiness is the PROCESS here, not the model: {ready}"
        );
    }

    /// A response for another id is dropped rather than returned — the rule
    /// that keeps one job's turns out of another job's meeting.
    #[test]
    fn diarize_protocol_discards_a_response_for_another_id() {
        let line = serde_json::to_string(&DiarizeResponse::embedded(9, vec![0.5, 0.5], 1))
            .expect("encode");
        assert!(parse_response_for(&line, 8).is_none());
        assert!(parse_response_for(&line, 9).is_some());
        assert_eq!(recover_id(&line), Some(9));
        assert_eq!(recover_id("{not json"), None);
        assert_eq!(recover_id(r#"{"kind":"diarize"}"#), None);
    }

    /// Every request kind round-trips through the SHARED file, and each one
    /// carries only its own fields — the shapes in the module docs, byte for
    /// byte.
    #[test]
    fn diarize_protocol_requests_round_trip_with_only_their_own_fields() {
        let load = DiarizeRequest::load_models(1, Path::new("/seg.onnx"), Path::new("/emb.onnx"));
        let encoded = serde_json::to_string(&load).expect("encode");
        assert_eq!(
            encoded,
            r#"{"id":1,"kind":"load_models","segmentation_path":"/seg.onnx","embedding_path":"/emb.onnx"}"#
        );
        assert_eq!(serde_json::from_str::<DiarizeRequest>(&encoded).expect("decode"), load);

        let diarize = DiarizeRequest::diarize(2, Path::new("/a.wav"), 0.35);
        let encoded = serde_json::to_string(&diarize).expect("encode");
        assert_eq!(
            encoded,
            r#"{"id":2,"kind":"diarize","wav_path":"/a.wav","clustering_distance_threshold":0.35}"#
        );
        assert_eq!(serde_json::from_str::<DiarizeRequest>(&encoded).expect("decode"), diarize);

        let embed = DiarizeRequest::embed(3, Path::new("/b.wav"));
        let encoded = serde_json::to_string(&embed).expect("encode");
        assert_eq!(encoded, r#"{"id":3,"kind":"embed","wav_path":"/b.wav"}"#);
        assert_eq!(serde_json::from_str::<DiarizeRequest>(&encoded).expect("decode"), embed);
    }

    /// Successes carry their kind's payload and nothing else; a refusal carries
    /// a tag and no payload at all.
    #[test]
    fn diarize_protocol_responses_carry_one_kind_of_payload() {
        assert_eq!(DiarizeResponse::loaded(1, 192, 3).into_embedding_dim(), Some(192));
        assert_eq!(DiarizeResponse::loaded(1, 192, 3).into_segments(), None);
        assert_eq!(DiarizeResponse::err(1, ERR_MODEL_NOT_FOUND).into_embedding_dim(), None);
        assert_eq!(
            DiarizeResponse::err(1, ERR_MODEL_NOT_FOUND).err_tag(),
            Some(ERR_MODEL_NOT_FOUND)
        );
        assert_eq!(DiarizeResponse::loaded(1, 192, 3).err_tag(), None);

        let segments = vec![DiarizeSegment {
            start: 0.0,
            end: 4.2,
            cluster: 0,
            embedding: vec![0.1, 0.2],
        }];
        let encoded = serde_json::to_string(&DiarizeResponse::diarized(2, segments.clone(), 7))
            .expect("encode");
        assert_eq!(
            encoded,
            r#"{"id":2,"ok":true,"segments":[{"start":0.0,"end":4.2,"cluster":0,"embedding":[0.1,0.2]}],"ms":7}"#
        );
        assert_eq!(
            parse_response_for(&encoded, 2).expect("decode").into_segments(),
            Some(segments)
        );
        // Silence really did diarize into nothing — distinct from a failure.
        assert_eq!(
            DiarizeResponse::diarized(3, Vec::new(), 1).into_segments(),
            Some(Vec::new())
        );
        // An "embedding" of no numbers is not an embedding.
        assert_eq!(DiarizeResponse::embedded(4, Vec::new(), 1).into_embedding(), None);
        assert_eq!(
            DiarizeResponse::embedded(4, vec![1.0], 1).into_embedding(),
            Some(vec![1.0])
        );
    }

    /// A turn carries no overlap flag (YV127) and the schema names no embedding
    /// width (finding #19). Both are absences, and an absence is exactly the
    /// kind of thing that gets re-added by accident, so both are asserted.
    #[test]
    fn diarize_protocol_names_no_overlap_flag_and_no_embedding_width() {
        let encoded = serde_json::to_string(&DiarizeSegment {
            start: 1.0,
            end: 2.0,
            cluster: 3,
            embedding: vec![0.0; 192],
        })
        .expect("encode");
        assert!(!encoded.contains("overlap"), "YV127: no overlap column in v1");
        // No CONSTANT in this file names an embedding width. `embedding_dim` is
        // a wire field the child fills in; the moment somebody writes
        // `const EMBEDDING_DIM: u32 = 192;` here, the parent has an opinion
        // about a model it has not loaded — which is finding #19 exactly.
        for (n, line) in include_str!("diarize_protocol.rs").lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default().trim_start();
            let declares = code.starts_with("const ")
                || code.starts_with("pub const ")
                || code.starts_with("static ")
                || code.starts_with("pub static ");
            assert!(
                !(declares && code.to_ascii_lowercase().contains("dim")),
                "diarize_protocol.rs:{}: the embedding width is the CHILD's to \
                 report, never a constant here: {}",
                n + 1,
                line.trim()
            );
        }
    }
}
