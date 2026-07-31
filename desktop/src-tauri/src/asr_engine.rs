//! Embedded ASR engine (YV30) — transcribe-cpp (GGUF/ggml native inference).
//!
//! Same crate + version Handy ships: `transcribe-cpp 0.1.3`, built with the
//! Metal backend on macOS (see Cargo.toml target table). It exposes `load()` /
//! `transcribe()` and nothing else; the warm lifecycle around it lives in
//! `transcription`, which YV32 wired into the dictation pipeline as the primary
//! path (the Python sidecar is now the fallback).
#![allow(dead_code)]

use std::path::Path;
use std::sync::Once;

use transcribe_cpp::{Backend, Model, ModelOptions, RunOptions, Session};

static BACKEND_INIT: Once = Once::new();

/// Initialize the transcribe-cpp native backend once per process: route
/// native/ggml diagnostics into the `log` facade and register compute backend
/// modules. In our static build (macOS Metal) `init_backends_default` is a
/// harmless no-op, but it must still run before the first model load.
pub fn init_backends() {
    BACKEND_INIT.call_once(|| {
        transcribe_cpp::init_logging();
        if let Err(e) = transcribe_cpp::init_backends_default() {
            log::warn!("transcribe-cpp backend init failed: {e}");
        }
    });
}

/// A loaded GGUF model ready to transcribe. Holds the live `Session`, which
/// keeps its `Model` alive for the engine's lifetime.
pub struct AsrEngine {
    session: Session,
}

/// Load a GGUF model from disk into a ready-to-run engine.
///
/// `Backend::Auto` lets the library pick the best registered device (Metal on
/// Apple Silicon) with CPU fallback, and `gpu_device: 0` is transcribe-cpp's
/// "auto / first match" sentinel — so a load never fails outright on a machine
/// without a GPU backend.
pub fn load(model_path: &Path) -> Result<AsrEngine, String> {
    init_backends();
    let options = ModelOptions {
        backend: Backend::Auto,
        gpu_device: 0,
    };
    let model = Model::load_with(model_path, &options)
        .map_err(|e| format!("failed to load model {}: {e}", model_path.display()))?;
    // The bound backend may differ from the request (e.g. CPU fallback under
    // Auto); log what actually loaded.
    let bound_backend = model.backend();
    let session = model
        .session()
        .map_err(|e| format!("failed to create session: {e}"))?;
    log::info!(
        "loaded ASR model {} (bound backend '{}')",
        model_path.display(),
        bound_backend
    );
    Ok(AsrEngine { session })
}

/// Batch-transcribe 16 kHz mono f32 samples into text.
pub fn transcribe(engine: &mut AsrEngine, samples_16k_mono: &[f32]) -> Result<String, String> {
    engine
        .session
        .run(samples_16k_mono, &RunOptions::default())
        .map(|t| t.text)
        .map_err(|e| format!("transcription failed: {e}"))
}
