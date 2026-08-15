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
//! ## What this build does NOT do, and says so
//!
//! YV121 carries **zero model bytes and zero onnxruntime** — `Cargo.toml` names
//! `serde` and `serde_json` and nothing else. So `load_models` validates its
//! paths and then answers `{"ok":false,"err":"no_backend"}`, and `diarize` /
//! `embed` answer `no_models`. It never invents a plausible `embedding_dim` or
//! a plausible segment list: a scaffold that answered as though it had run
//! would make every test above it vacuous, and YV122's first real assertion
//! (`embedding_dim == 192`, audit finding #19) would already be "passing".
//!
//! YV122 replaces exactly one function — [`load_backend`] — plus the two arms
//! that call into it. Everything else here is the shape that ships.

// The wire contract lives with the app it talks to. Compiled, not copied.
#[path = "../../src-tauri/src/diarize_protocol.rs"]
mod diarize_protocol;

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Instant;

use diarize_protocol::{
    recover_id, DiarizeReady, DiarizeRequest, DiarizeResponse, ERR_AUDIO_NOT_FOUND,
    ERR_BAD_REQUEST, ERR_MISSING_FIELD, ERR_MODEL_NOT_FOUND, ERR_NO_BACKEND, ERR_NO_MODELS,
    ERR_UNSUPPORTED_KIND, KIND_DIARIZE, KIND_EMBED, KIND_LOAD_MODELS,
};

/// The loaded model pair, once there is a backend that can load one.
///
/// `embedding_dim` is read off the model, never assumed: the plan's schema
/// assumed 512 and the shipped CAM++ is 192 (finding #19), and the only place
/// that discrepancy can be caught is here, where the file actually is.
struct Backend {
    embedding_dim: u32,
}

/// Load a segmentation + embedding model pair.
///
/// **YV122 replaces this function body** with `sherpa_onnx`'s
/// `OfflineSpeakerDiarization` + `SpeakerEmbeddingExtractor` construction. Its
/// signature is already the one that will be needed, so the change is one
/// function and not a reshaping of the serve loop.
///
/// Until then it returns the honest answer: this build has no inference backend
/// compiled in. The paths are still validated first, because "that file is not
/// there" is a different bug from "this build cannot use it" and the parent
/// (and YV123's vendoring work) needs to be able to tell them apart.
fn load_backend(segmentation: &Path, embedding: &Path) -> Result<Backend, &'static str> {
    for path in [segmentation, embedding] {
        if !path.is_file() {
            return Err(ERR_MODEL_NOT_FOUND);
        }
    }
    Err(ERR_NO_BACKEND)
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
            if backend.is_none() {
                return DiarizeResponse::err(req.id, ERR_NO_MODELS);
            }
            if !Path::new(wav).is_file() {
                return DiarizeResponse::err(req.id, ERR_AUDIO_NOT_FOUND);
            }
            // Unreachable in this build — `backend` is never `Some` without a
            // backend to have loaded it. YV122 fills these two arms in.
            DiarizeResponse::err(req.id, ERR_NO_BACKEND)
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

    /// The scaffold answers HONESTLY. A `load_models` against files that exist
    /// must not report a dimension this build did not read off a model — that
    /// answer would make YV122's `embedding_dim == 192` assertion pass against
    /// nothing at all.
    #[test]
    fn a_backendless_build_never_reports_a_dimension_it_did_not_measure() {
        let mut backend = None;
        // Two files that certainly exist: this crate's own manifest and source.
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
        let req = DiarizeRequest::load_models(1, &manifest, &source);
        let response = handle(&mut backend, &req);
        assert!(!response.ok, "this build cannot load a model");
        assert_eq!(response.err_tag(), Some(ERR_NO_BACKEND));
        assert_eq!(response.embedding_dim, None);
        assert!(backend.is_none(), "nothing was loaded, so nothing is held");
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
