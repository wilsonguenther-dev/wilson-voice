//! Embedded ASR engine (YV30) — transcribe-cpp (GGUF/ggml native inference).
//!
//! Same crate + version Handy ships: `transcribe-cpp 0.1.3`, built with the
//! Metal backend on macOS (see Cargo.toml target table). It exposes `load()` /
//! `transcribe()` and nothing else; the warm lifecycle around it lives in
//! `transcription`, which YV32 wired into the dictation pipeline and YV34 made
//! the app's only ASR path (the Python sidecar is gone). YV47 added the one
//! decode-bias hook the crate actually has — the Whisper run extension's
//! `initial_prompt` — fed from the auto-learning dictionary.
#![allow(dead_code)]

use std::path::Path;
use std::sync::Once;

use transcribe_cpp::{
    Backend, Model, ModelOptions, RunExtension, RunOptions, Session, WhisperRunOptions,
};

static BACKEND_INIT: Once = Once::new();

/// Hard ceiling on the decode prompt, in tokens (YV47).
///
/// The only bias hook transcribe-cpp 0.1.3 exposes is the Whisper run
/// extension's `initial_prompt` (`RunExtension::Whisper`) — there is no hotword
/// list. The native side tokenizes it as HF does and then LEFT-truncates to
/// `max_prev_context_tokens`, whose default is 223 (`max_target_positions / 2 -
/// 1`); see `transcribe/whisper.h`. Anything past that is silently dropped.
pub const BIAS_PROMPT_MAX_TOKENS: usize = 223;

/// Bytes-per-token floor used to stay under [`BIAS_PROMPT_MAX_TOKENS`].
///
/// We cannot run Whisper's BPE from here, so we budget pessimistically: even
/// hostile proper nouns ("Jeisil", "Drivia") come apart into pieces of at least
/// two characters, so 2 chars/token can only ever over-reserve. Under-filling
/// the window is free; overflowing it would silently drop the terms at the head
/// of the prompt.
const MIN_CHARS_PER_TOKEN: usize = 2;

/// Character budget for the assembled prompt.
pub const BIAS_PROMPT_MAX_CHARS: usize = BIAS_PROMPT_MAX_TOKENS * MIN_CHARS_PER_TOKEN;

/// Assemble the decoder bias prompt from dictionary terms (YV47).
///
/// `terms` arrives LEAST-important-first (see `Database::bias_terms`) because a
/// Whisper prompt weights later tokens more heavily and both this cap and the
/// native truncation drop from the front — so the tail is what always survives.
/// Terms are joined with ", " (the shape upstream Handy feeds `initial_prompt`).
///
/// Returns `None` when there is nothing to bias with. Terms carrying a special
/// token marker (`<|…|>`) are dropped: the native side rejects the whole run
/// with `InvalidArgument` if one survives into the prompt.
pub fn build_bias_prompt(terms: &[String]) -> Option<String> {
    const SEP: &str = ", ";
    let usable: Vec<&str> = terms
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty() && !t.contains('<') && !t.contains('|') && !t.contains('\0'))
        .collect();

    // Fill from the END (most important) backwards until the budget is spent.
    let mut kept: Vec<&str> = Vec::new();
    let mut len = 0usize;
    for t in usable.iter().rev() {
        let cost = t.chars().count() + if kept.is_empty() { 0 } else { SEP.len() };
        if len + cost > BIAS_PROMPT_MAX_CHARS {
            break;
        }
        len += cost;
        kept.push(t);
    }
    if kept.is_empty() {
        return None;
    }
    kept.reverse();
    Some(kept.join(SEP))
}

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
    /// `general.architecture` of the loaded GGUF. Gates the Whisper run
    /// extension: the extension is kind-tagged, so a parakeet/nemotron model
    /// rejects it with `InvalidArgument` and would fail the whole take. Handy
    /// gates on the arch string for the same reason — a non-whisper arch can
    /// advertise `Feature::InitialPrompt` yet still refuse the extension.
    arch: String,
}

impl AsrEngine {
    /// Whether this model accepts an `initial_prompt` bias (YV47).
    fn accepts_initial_prompt(&self) -> bool {
        self.arch == "whisper"
    }
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
    let arch = model.arch();
    let session = model
        .session()
        .map_err(|e| format!("failed to create session: {e}"))?;
    log::info!(
        "loaded ASR model {} (arch '{}', bound backend '{}')",
        model_path.display(),
        arch,
        bound_backend
    );
    Ok(AsrEngine { session, arch })
}

/// Batch-transcribe 16 kHz mono f32 samples into text.
///
/// `language` is the user's "Language I speak" setting as an ISO code, carried
/// over from the deleted sidecar's `--language` flag (YV34) so the picker keeps
/// working; `None` leaves transcribe-cpp on autodetect.
///
/// `bias_prompt` is the YV47 dictionary bias — starred + usage-ranked terms as
/// assembled by [`build_bias_prompt`]. It rides the Whisper run extension's
/// `initial_prompt` and is dropped for models that cannot take one.
pub fn transcribe(
    engine: &mut AsrEngine,
    samples_16k_mono: &[f32],
    language: Option<&str>,
    bias_prompt: Option<&str>,
) -> Result<String, String> {
    let family = bias_prompt
        .filter(|p| !p.is_empty() && engine.accepts_initial_prompt())
        .map(|p| {
            RunExtension::Whisper(WhisperRunOptions {
                initial_prompt: Some(p.to_string()),
                ..WhisperRunOptions::default()
            })
        });
    let options = RunOptions {
        language: language.map(str::to_string),
        family,
        ..RunOptions::default()
    };
    engine
        .session
        .run(samples_16k_mono, &options)
        .map(|t| t.text)
        .map_err(|e| format!("transcription failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// YV47 — the prompt is capped to what transcribe-cpp will actually keep.
    /// Overflow drops the LEAST-important terms (the head), never the starred
    /// ones the caller put at the tail.
    #[test]
    fn bias_prompt_caps_at_the_mechanisms_real_limit() {
        assert_eq!(build_bias_prompt(&[]), None, "nothing to bias with");

        let short = vec!["Drivia".to_string(), "Jeisil".to_string()];
        assert_eq!(build_bias_prompt(&short).unwrap(), "Drivia, Jeisil");

        // 200 twelve-character terms is ~2.6 KB — far past the window.
        let mut many: Vec<String> = (0..200).map(|i| format!("Filler{i:06}")).collect();
        many.push("MostImportant".to_string()); // highest priority = last
        let prompt = build_bias_prompt(&many).expect("prompt");
        assert!(
            prompt.chars().count() <= BIAS_PROMPT_MAX_CHARS,
            "prompt overflowed the {BIAS_PROMPT_MAX_CHARS}-char budget: {} chars",
            prompt.chars().count()
        );
        assert!(
            prompt.ends_with("MostImportant"),
            "cap dropped the most-important term: {prompt}"
        );
        assert!(
            !prompt.contains("Filler000000"),
            "cap kept the least-important term instead: {prompt}"
        );
    }

    /// A single term longer than the whole budget can't be truncated into
    /// something meaningful, so it is dropped rather than half-fed.
    #[test]
    fn bias_prompt_skips_unusable_terms() {
        let oversized = "x".repeat(BIAS_PROMPT_MAX_CHARS + 1);
        assert_eq!(build_bias_prompt(&[oversized]), None);

        // Special-token markers are rejected by the native side with
        // InvalidArgument, which would fail the whole take — drop them here.
        let terms = vec![
            "<|endoftext|>".to_string(),
            "  ".to_string(),
            "Supabase".to_string(),
        ];
        assert_eq!(build_bias_prompt(&terms).unwrap(), "Supabase");
    }
}
