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

#[cfg(feature = "in-process-asr")]
use std::path::Path;
#[cfg(feature = "in-process-asr")]
use std::sync::Once;

use serde::{Deserialize, Serialize};
// yap-1106-engine-owner: the native engine is compiled in only with
// `--features in-process-asr`; the shipped app links none of it (the `yap-asr`
// helper does). Everything else in this module is plain data and stays.
#[cfg(feature = "in-process-asr")]
use transcribe_cpp::{
    Backend, CancelToken, Model, ModelOptions, RunExtension, RunOptions, Session, TimestampKind,
    WhisperRunOptions,
};

#[cfg(feature = "in-process-asr")]
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
#[cfg(feature = "in-process-asr")]
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
#[cfg(feature = "in-process-asr")]
pub struct AsrEngine {
    session: Session,
    /// Y10-A — the language the model detected on the most recent run, or
    /// `None` when a hint was given, the model has no detection, or nothing has
    /// run yet. Read by `languages::decode_in_set` to hold auto-detect to the
    /// user's language set.
    last_language: Option<String>,
    /// `general.architecture` of the loaded GGUF. Gates the Whisper run
    /// extension: the extension is kind-tagged, so a parakeet/nemotron model
    /// rejects it with `InvalidArgument` and would fail the whole take. Handy
    /// gates on the arch string for the same reason — a non-whisper arch can
    /// advertise `Feature::InitialPrompt` yet still refuse the extension.
    arch: String,
    /// YV70 — the cooperative cancel token installed on the session at load.
    /// transcribe-cpp polls it between decode steps, so calling
    /// [`cancel`] from another thread ends an in-flight [`transcribe`] with
    /// `Error::Aborted` instead of leaving the caller to wait it out. Used by
    /// the exit drain, which has to get the engine (and its Metal device) back
    /// before the process reaches `exit()`.
    cancel: CancelToken,
}

#[cfg(feature = "in-process-asr")]
impl AsrEngine {
    /// Whether this model accepts an `initial_prompt` bias (YV47).
    fn accepts_initial_prompt(&self) -> bool {
        self.arch == "whisper"
    }
}

/// A clone of this engine's cancel token (YV70) — cheap, shares one flag, and
/// can be held while the engine itself is leased out to a transcription thread.
#[cfg(feature = "in-process-asr")]
pub fn cancel_token(engine: &AsrEngine) -> CancelToken {
    engine.cancel.clone()
}

/// Clear a cancellation left over from a previous run (YV93).
///
/// `CancelToken` is STICKY — `cancel()` sets a flag that stays set until
/// `reset()`, and the session polls it between decode steps forever after. YV70
/// only ever cancelled on the way out of the process, so nothing noticed; YV93
/// cancels a meeting chunk whenever a dictation wants the engine, and without
/// this every decode after the first preemption would abort instantly and the
/// warm engine would be permanently useless.
///
/// Called by the lease with the engine exclusively in hand, immediately before
/// it is published as in-flight, so a cancel from that instant on is honoured
/// and one from a previous run cannot abort this one.
#[cfg(feature = "in-process-asr")]
pub fn reset_cancel(engine: &AsrEngine) {
    engine.cancel.reset();
}

/// Load a GGUF model from disk into a ready-to-run engine.
///
/// `Backend::Auto` lets the library pick the best registered device (Metal on
/// Apple Silicon) with CPU fallback, and `gpu_device: 0` is transcribe-cpp's
/// "auto / first match" sentinel — so a load never fails outright on a machine
/// without a GPU backend.
#[cfg(feature = "in-process-asr")]
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
    let mut session = model
        .session()
        .map_err(|e| format!("failed to create session: {e}"))?;
    // YV70: install the cancel hook up front — a token can only be set on an
    // idle session, so it has to be here rather than at the moment we want to
    // stop a run. Nothing ever cancels except the exit drain.
    let cancel = CancelToken::new();
    session.set_cancel_token(&cancel);
    log::info!(
        "loaded ASR model {} (arch '{}', bound backend '{}')",
        model_path.display(),
        arch,
        bound_backend
    );
    Ok(AsrEngine {
        session,
        last_language: None,
        arch,
        cancel,
    })
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
#[cfg(feature = "in-process-asr")]
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
    engine.last_language = None;
    let transcript = engine
        .session
        .run(samples_16k_mono, &options)
        .map_err(|e| format!("transcription failed: {e}"))?;
    engine.last_language = transcript.language.clone();
    Ok(transcript.text)
}

/// Y10-A — the language the last run detected (see `AsrEngine::last_language`).
#[cfg(feature = "in-process-asr")]
pub fn last_detected_language(engine: &AsrEngine) -> Option<String> {
    engine.last_language.clone()
}

/// Y10-A — the probed language capabilities, in the shape the picker uses.
#[cfg(feature = "in-process-asr")]
pub fn language_support(engine: &AsrEngine) -> crate::languages::LanguageSupport {
    crate::languages::LanguageSupport::from_capabilities(&capabilities(engine))
}

// ---------------------------------------------------------------------------
// YV93 — the TIMED decode
// ---------------------------------------------------------------------------

/// One timed row of a decode — a segment or a word, depending on which list it
/// came out of. Times are SECONDS from the start of the buffer that was
/// decoded; the meeting chunker shifts them onto the meeting's timeline.
///
/// Deliberately not `transcribe_cpp::Segment`: the native rows carry index
/// bookkeeping (`first_word`, `n_tokens`, …) that means nothing once a row has
/// been moved onto another timeline, and re-exporting a native type would put
/// the ASR crate in the signature of everything downstream of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedSpan {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
}

impl TimedSpan {
    /// Shift onto another timeline (a chunk's own start, in the meeting case).
    pub fn shifted(&self, by_seconds: f64) -> TimedSpan {
        TimedSpan {
            start_seconds: self.start_seconds + by_seconds,
            end_seconds: self.end_seconds + by_seconds,
            text: self.text.clone(),
        }
    }
}

/// What granularity of timestamp a decode actually came back with. This is the
/// answer to plan finding #11 ("nobody has confirmed what the shipped Parakeet
/// build returns") at RUN time rather than at spike time, because it is what
/// the merge has to branch on: with [`TimedKind::None`] there are no times to
/// dedupe on and the seam falls back to text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimedKind {
    None,
    Segment,
    Word,
    Token,
}

impl TimedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TimedKind::None => "none",
            TimedKind::Segment => "segment",
            TimedKind::Word => "word",
            TimedKind::Token => "token",
        }
    }

    /// True when the decode carried times the seam merge can use as its PRIMARY
    /// key (finding #11 keeps text-LCS as the fallback, never the primary).
    pub fn has_times(self) -> bool {
        !matches!(self, TimedKind::None)
    }

    #[cfg(feature = "in-process-asr")]
    fn from_native(kind: TimestampKind) -> TimedKind {
        match kind {
            TimestampKind::Segment => TimedKind::Segment,
            TimestampKind::Word => TimedKind::Word,
            TimestampKind::Token => TimedKind::Token,
            // `Auto` is a REQUEST, never an answer: a result tagged Auto is one
            // the native side did not resolve, so treat it as "no times".
            TimestampKind::None | TimestampKind::Auto => TimedKind::None,
        }
    }
}

/// A decode with its alignment kept — the whole point of [`transcribe_timed`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedTranscript {
    pub text: String,
    /// The finest granularity actually populated below.
    pub kind: TimedKind,
    /// Segment rows, empty when the model produced none.
    pub segments: Vec<TimedSpan>,
    /// Word rows, empty when the model produced none.
    pub words: Vec<TimedSpan>,
}

impl TimedTranscript {
    /// A transcript with text and nothing else — what a caller gets from a
    /// model that produces no alignment at all, and what the stub engines in
    /// the lifecycle tests return.
    pub fn text_only(text: impl Into<String>) -> TimedTranscript {
        TimedTranscript {
            text: text.into(),
            kind: TimedKind::None,
            segments: Vec::new(),
            words: Vec::new(),
        }
    }

    /// The best timed rows this transcript has: words when the model gave word
    /// (or finer) alignment, else segments, else nothing.
    pub fn best_spans(&self) -> &[TimedSpan] {
        if !self.words.is_empty() {
            &self.words
        } else {
            &self.segments
        }
    }
}

#[cfg(feature = "in-process-asr")]
fn span(t0_ms: i64, t1_ms: i64, text: &str) -> TimedSpan {
    TimedSpan {
        start_seconds: t0_ms as f64 / 1000.0,
        end_seconds: t1_ms as f64 / 1000.0,
        text: text.trim().to_string(),
    }
}

/// Batch-transcribe 16 kHz mono f32 samples, KEEPING the alignment (YV93).
///
/// A second method on purpose (plan finding #39): [`transcribe`] has two live
/// callers — the dictation path and `cli.rs` — and neither wants the extra rows
/// or the extra allocation. Nothing about this call changes the decode itself;
/// it asks for `TimestampKind::Auto` ("richest this family supports") and keeps
/// what comes back instead of throwing it away at `.map(|t| t.text)`.
#[cfg(feature = "in-process-asr")]
pub fn transcribe_timed(
    engine: &mut AsrEngine,
    samples_16k_mono: &[f32],
    language: Option<&str>,
    bias_prompt: Option<&str>,
) -> Result<TimedTranscript, String> {
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
        timestamps: TimestampKind::Auto,
        family,
        ..RunOptions::default()
    };
    engine.last_language = None;
    let transcript = engine
        .session
        .run(samples_16k_mono, &options)
        .map_err(|e| format!("transcription failed: {e}"))?;
    engine.last_language = transcript.language.clone();
    Ok(TimedTranscript {
        text: transcript.text.trim().to_string(),
        kind: TimedKind::from_native(transcript.timestamp_kind),
        segments: transcript
            .segments
            .iter()
            .map(|s| span(s.t0_ms, s.t1_ms, &s.text))
            .collect(),
        words: transcript
            .words
            .iter()
            .map(|w| span(w.t0_ms, w.t1_ms, &w.text))
            .collect(),
    })
}

/// What the loaded model says it can do (YV93 spike, plan finding #11).
///
/// The two fields the chunker is built on are `max_timestamp_kind` (is there an
/// alignment to dedupe seams with at all?) and `max_audio_ms` (is a 35 s window
/// even accepted?). Printed by `wilson-voice --asr-capabilities`.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCapabilities {
    pub arch: String,
    pub backend: String,
    pub native_sample_rate: i32,
    pub languages: Vec<String>,
    pub max_timestamp_kind: TimedKind,
    pub max_audio_ms: i64,
    pub supports_language_detect: bool,
    pub supports_streaming: bool,
}

/// Read the loaded model's capabilities. Cheap — GGUF metadata, no decode.
#[cfg(feature = "in-process-asr")]
pub fn capabilities(engine: &AsrEngine) -> ModelCapabilities {
    let model = engine.session.model();
    let caps = model.capabilities();
    ModelCapabilities {
        arch: model.arch(),
        backend: model.backend(),
        native_sample_rate: caps.native_sample_rate,
        languages: caps.languages.clone(),
        max_timestamp_kind: TimedKind::from_native(caps.max_timestamp_kind),
        max_audio_ms: caps.max_audio_ms,
        supports_language_detect: caps.supports_language_detect,
        supports_streaming: caps.supports_streaming,
    }
}

impl ModelCapabilities {
    /// The probe's report, one `key: value` per line. The two lines the YV93
    /// acceptance criteria grep for (`max_timestamp_kind: …`, `max_audio_ms: …`)
    /// are produced here, so the format is asserted by a unit test rather than
    /// by whoever last edited the printer.
    pub fn report(&self) -> String {
        let langs = if self.languages.is_empty() {
            "(language-agnostic)".to_string()
        } else {
            self.languages.join(",")
        };
        format!(
            "arch: {}\nbackend: {}\nnative_sample_rate: {}\nlanguages: {}\nmax_timestamp_kind: {}\nmax_audio_ms: {}\nsupports_language_detect: {}\nsupports_streaming: {}",
            self.arch,
            self.backend,
            self.native_sample_rate,
            langs,
            self.max_timestamp_kind.as_str(),
            self.max_audio_ms,
            self.supports_language_detect,
            self.supports_streaming,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The probe prints the two lines the YV93 acceptance criteria grep for,
    /// in the shape they grep for them (`max_timestamp_kind: .+`,
    /// `max_audio_ms: [0-9]+`) — provable without a 731 MB model on disk.
    #[test]
    fn capabilities_report_carries_the_two_lines_the_spike_is_for() {
        let caps = ModelCapabilities {
            arch: "parakeet".into(),
            backend: "Metal".into(),
            native_sample_rate: 16_000,
            languages: vec!["en".into()],
            max_timestamp_kind: TimedKind::Word,
            max_audio_ms: 0,
            supports_language_detect: false,
            supports_streaming: true,
        };
        let report = caps.report();
        let kind = report
            .lines()
            .find(|l| l.starts_with("max_timestamp_kind: "))
            .expect("max_timestamp_kind line");
        assert!(
            !kind.trim_start_matches("max_timestamp_kind: ").is_empty(),
            "empty timestamp kind: {kind}"
        );
        let audio = report
            .lines()
            .find(|l| l.starts_with("max_audio_ms: "))
            .expect("max_audio_ms line");
        assert!(
            audio
                .trim_start_matches("max_audio_ms: ")
                .chars()
                .all(|c| c.is_ascii_digit()),
            "max_audio_ms is not a plain integer: {audio}"
        );
    }

    /// `Auto` is what a decode is ASKED for, never what it comes back with — a
    /// result still tagged Auto means the native side resolved nothing, which
    /// the merge must read as "no usable times" and not as "some times".
    #[cfg(feature = "in-process-asr")]
    #[test]
    fn auto_timestamps_are_not_mistaken_for_real_ones() {
        assert_eq!(TimedKind::from_native(TimestampKind::Auto), TimedKind::None);
        assert_eq!(TimedKind::from_native(TimestampKind::None), TimedKind::None);
        assert_eq!(TimedKind::from_native(TimestampKind::Word), TimedKind::Word);
        assert!(!TimedKind::None.has_times());
        assert!(TimedKind::Segment.has_times());
    }

    /// Words win over segments when both are present: they are the finer key,
    /// and the seam merge wants the finest one the model produced.
    #[test]
    fn best_spans_prefers_words_then_segments_then_nothing() {
        let seg = TimedSpan {
            start_seconds: 0.0,
            end_seconds: 1.0,
            text: "hello there".into(),
        };
        let word = TimedSpan {
            start_seconds: 0.0,
            end_seconds: 0.4,
            text: "hello".into(),
        };
        let t = TimedTranscript {
            text: "hello there".into(),
            kind: TimedKind::Word,
            segments: vec![seg.clone()],
            words: vec![word.clone()],
        };
        assert_eq!(t.best_spans(), &[word]);
        let t = TimedTranscript {
            kind: TimedKind::Segment,
            words: Vec::new(),
            ..t
        };
        assert_eq!(t.best_spans(), &[seg]);
        assert!(TimedTranscript::text_only("hi").best_spans().is_empty());
    }

    /// A span moved onto the meeting timeline keeps its width and its text.
    #[test]
    fn shifting_a_span_moves_it_without_stretching_it() {
        let s = TimedSpan {
            start_seconds: 1.5,
            end_seconds: 2.25,
            text: "walnut".into(),
        };
        let moved = s.shifted(28.0);
        assert_eq!(moved.start_seconds, 29.5);
        assert_eq!(moved.end_seconds, 30.25);
        assert_eq!(moved.text, "walnut");
    }

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

// ---------------------------------------------------------------------------
// yap-1106-sp-10-ane-engine-spike — the second engine, and the rule that
// decides what it becomes.
//
// `yap-engine-ane` (desktop/yap-engine-ane, Swift + FluidAudio) runs Parakeet
// on the Neural Engine behind the SAME wire protocol as `yap-asr`, so the
// engine owner drives either one through `HelperEngine` unchanged. This item
// does NOT change the default: `EngineKind::DEFAULT` is `Gguf`, and the ANE
// helper is reached only when `YAP_ASR_ENGINE=ane` is set (the SP-10 bench,
// scripts/qa/ane-bench.sh) or when the governor asks for its "constrained"
// engine and the verdict below allows it.
// ---------------------------------------------------------------------------

/// Which speech helper the engine owner spawns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    /// `yap-asr`: transcribe-cpp on Metal (GGUF). The shipped default.
    Gguf,
    /// `yap-engine-ane`: Parakeet on the Neural Engine via Core ML.
    Ane,
}

/// The environment switch the bench (and only the bench) sets.
pub const ENGINE_ENV: &str = "YAP_ASR_ENGINE";

impl EngineKind {
    /// No change to the default engine in SP-10.
    pub const DEFAULT: EngineKind = EngineKind::Gguf;

    /// The sidecar's file name next to the app (tauri `externalBin` stem).
    pub fn helper_bin(self) -> &'static str {
        match self {
            EngineKind::Gguf => "yap-asr",
            EngineKind::Ane => "yap-engine-ane",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EngineKind::Gguf => "gguf",
            EngineKind::Ane => "ane",
        }
    }

    /// Parse the env value. Anything unknown is the default, never an error:
    /// a typo must not leave the user with no engine.
    pub fn parse(value: Option<&str>) -> EngineKind {
        match value.map(|v| v.trim().to_ascii_lowercase()) {
            Some(v) if v == "ane" => EngineKind::Ane,
            _ => EngineKind::DEFAULT,
        }
    }

    /// The engine the owner should spawn right now.
    pub fn selected() -> EngineKind {
        EngineKind::parse(std::env::var(ENGINE_ENV).ok().as_deref())
    }

    /// The governor's pick. Under pressure ("constrained": battery, thermal,
    /// another heavy job) it may move to the ANE engine — but only once the
    /// bench has run and the ANE helper is installed; otherwise it stays on
    /// the default. `AdoptAsDefault` makes the ANE the pick everywhere, which
    /// is a later item's change, not this one's.
    pub fn for_governor(
        constrained: bool,
        verdict: Option<AneVerdict>,
        ane_installed: bool,
    ) -> EngineKind {
        if !ane_installed {
            return EngineKind::DEFAULT;
        }
        match (verdict, constrained) {
            (Some(AneVerdict::AdoptAsDefault), _) => EngineKind::Ane,
            (Some(AneVerdict::ConstrainedOnly), true) => EngineKind::Ane,
            _ => EngineKind::DEFAULT,
        }
    }
}

/// One engine's SP-03 bench numbers on the 4 min fixture (bench-240s.wav).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct EngineBench {
    /// Word error rate, 0.0..=1.0.
    pub wer: f64,
    /// Warm real-time factor as "times faster than real time" (audio / decode).
    pub warm_rtfx: f64,
    /// Mean package power (CPU + GPU + ANE, powermetrics "Combined Power") in mW.
    pub package_mw: f64,
    /// Mean GPU power in mW (powermetrics "GPU Power"). The rule's first
    /// power bar: the spec's "GPU package power".
    pub gpu_mw: f64,
    /// First load on a Mac with no Core ML cache (ANE compile included), ms.
    pub first_load_ms: u64,
    /// Peak resident memory of the helper, MB.
    pub peak_rss_mb: u64,
}

/// SP-10's ship decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AneVerdict {
    /// All three bars met: a later item may make the ANE engine the default.
    AdoptAsDefault,
    /// Any bar missed: kept as the governor's "constrained" engine only.
    ConstrainedOnly,
}

/// WER may be at most this many percentage points worse than the default.
pub const ANE_MAX_WER_DELTA_PT: f64 = 0.5;
/// Warm decode must be at least this many times faster than real time.
pub const ANE_MIN_WARM_RTFX: f64 = 100.0;
/// GPU power, and package power, must each drop by at least this percentage
/// against the default engine.
pub const ANE_MIN_POWER_DROP_PCT: f64 = 30.0;

/// Percentage drop from `baseline` to `value`. A missing (zero, negative or
/// non-finite) baseline is `-inf`, so it can never pass a power bar.
fn power_drop_pct(baseline_mw: f64, value_mw: f64) -> f64 {
    if baseline_mw.is_finite() && baseline_mw > 0.0 && value_mw.is_finite() {
        (baseline_mw - value_mw) / baseline_mw * 100.0
    } else {
        f64::NEG_INFINITY
    }
}

/// The rule written into the PR: adopt as the default only if WER is within
/// 0.5 pt, warm RTF >= 100x, and GPU power is lower by at least 30 %. Package
/// power (CPU + GPU + ANE) must drop by the same 30 % too: moving work off the
/// GPU always lowers GPU power by itself, so the GPU bar alone could adopt an
/// engine that costs the battery more. A missing baseline never passes.
pub fn ane_verdict(default_engine: &EngineBench, ane: &EngineBench) -> AneVerdict {
    let wer_delta_pt = (ane.wer - default_engine.wer) * 100.0;
    let gpu_drop_pct = power_drop_pct(default_engine.gpu_mw, ane.gpu_mw);
    let package_drop_pct = power_drop_pct(default_engine.package_mw, ane.package_mw);
    let pass = wer_delta_pt <= ANE_MAX_WER_DELTA_PT
        && ane.warm_rtfx >= ANE_MIN_WARM_RTFX
        && gpu_drop_pct >= ANE_MIN_POWER_DROP_PCT
        && package_drop_pct >= ANE_MIN_POWER_DROP_PCT;
    log::info!(
        "asr_engine ane_verdict wer_delta_pt={wer_delta_pt:.2} warm_rtfx={:.1} gpu_drop_pct={gpu_drop_pct:.1} package_drop_pct={package_drop_pct:.1} pass={pass}",
        ane.warm_rtfx
    );
    if pass {
        AneVerdict::AdoptAsDefault
    } else {
        AneVerdict::ConstrainedOnly
    }
}

#[cfg(test)]
mod ane_engine_tests {
    use super::*;

    const SWIFT_WIRE: &str =
        include_str!("../../yap-engine-ane/Sources/YapEngineANECore/Wire.swift");
    const SWIFT_PACKAGE: &str = include_str!("../../yap-engine-ane/Package.swift");
    const CATALOG: &str = include_str!("catalog.json");
    const TAURI_CONF: &str = include_str!("../tauri.conf.json");

    /// GPU power moves with package power (40 % of it) unless a test sets it.
    fn bench(wer: f64, rtfx: f64, mw: f64) -> EngineBench {
        EngineBench {
            wer,
            warm_rtfx: rtfx,
            package_mw: mw,
            gpu_mw: mw * 0.4,
            ..EngineBench::default()
        }
    }

    #[test]
    fn the_default_engine_is_unchanged() {
        assert_eq!(EngineKind::DEFAULT, EngineKind::Gguf);
        assert_eq!(EngineKind::DEFAULT.helper_bin(), "yap-asr");
        assert_eq!(EngineKind::parse(None), EngineKind::Gguf);
        assert_eq!(EngineKind::parse(Some("metal")), EngineKind::Gguf);
        assert_eq!(EngineKind::parse(Some(" ANE ")), EngineKind::Ane);
        assert_eq!(EngineKind::Ane.helper_bin(), "yap-engine-ane");
    }

    #[test]
    fn ane_verdict_needs_all_three_bars() {
        let base = bench(0.050, 40.0, 10_000.0);
        assert_eq!(
            ane_verdict(&base, &bench(0.054, 120.0, 6_500.0)),
            AneVerdict::AdoptAsDefault
        );
        // WER 0.6 pt worse.
        assert_eq!(
            ane_verdict(&base, &bench(0.056, 120.0, 6_500.0)),
            AneVerdict::ConstrainedOnly
        );
        // Warm RTF 99x.
        assert_eq!(
            ane_verdict(&base, &bench(0.050, 99.0, 6_500.0)),
            AneVerdict::ConstrainedOnly
        );
        // Power only 29 % lower.
        assert_eq!(
            ane_verdict(&base, &bench(0.050, 150.0, 7_100.0)),
            AneVerdict::ConstrainedOnly
        );
        // No baseline power: never adopted.
        assert_eq!(
            ane_verdict(&bench(0.05, 40.0, 0.0), &bench(0.05, 150.0, 1.0)),
            AneVerdict::ConstrainedOnly
        );
        // Package power down 35 % but GPU power only 20 %: the GPU bar fails.
        let mut gpu_heavy = bench(0.050, 150.0, 6_500.0);
        gpu_heavy.gpu_mw = base.gpu_mw * 0.8;
        assert_eq!(ane_verdict(&base, &gpu_heavy), AneVerdict::ConstrainedOnly);
        // GPU power down 90 % but package power only 10 %: the battery bar fails.
        let mut cpu_heavy = bench(0.050, 150.0, 9_000.0);
        cpu_heavy.gpu_mw = base.gpu_mw * 0.1;
        assert_eq!(ane_verdict(&base, &cpu_heavy), AneVerdict::ConstrainedOnly);
        // No baseline GPU power: never adopted.
        let mut no_gpu = base;
        no_gpu.gpu_mw = 0.0;
        assert_eq!(
            ane_verdict(&no_gpu, &bench(0.050, 150.0, 6_500.0)),
            AneVerdict::ConstrainedOnly
        );
    }

    #[test]
    fn governor_uses_ane_only_when_constrained_and_allowed() {
        let c = Some(AneVerdict::ConstrainedOnly);
        assert_eq!(EngineKind::for_governor(false, c, true), EngineKind::Gguf);
        assert_eq!(EngineKind::for_governor(true, c, true), EngineKind::Ane);
        assert_eq!(EngineKind::for_governor(true, c, false), EngineKind::Gguf);
        assert_eq!(EngineKind::for_governor(true, None, true), EngineKind::Gguf);
    }

    #[test]
    fn ane_wire_mirror_matches_the_rust_protocol() {
        assert!(
            SWIFT_WIRE.contains(&format!(
                "asrProtocolVersion = \"{}\"",
                crate::asr_protocol::ASR_PROTOCOL_VERSION
            )),
            "the Swift helper must speak the same protocol version as yap-asr"
        );
        // Every multi-word serde key of the Rust side must appear, quoted, in
        // the Swift CodingKeys (single-word keys match by construction).
        let mut keys: Vec<String> = Vec::new();
        let req = crate::asr_protocol::AsrRequest::transcribe(1, "/p", Some("en"), Some("b"), true);
        let mut load = crate::asr_protocol::AsrRequest::load(1, "/m");
        load.samples_path = req.samples_path.clone();
        for v in [
            serde_json::to_value(&req).unwrap(),
            serde_json::to_value(&load).unwrap(),
            serde_json::to_value(crate::asr_protocol::WireCaps::default()).unwrap(),
            serde_json::to_value(crate::asr_protocol::WireSpan::default()).unwrap(),
        ] {
            keys.extend(v.as_object().unwrap().keys().cloned());
        }
        for key in keys.iter().filter(|k| k.contains('_')) {
            assert!(
                SWIFT_WIRE.contains(&format!("\"{key}\"")),
                "Wire.swift is missing the key {key}"
            );
        }
    }

    #[test]
    fn fluidaudio_is_pinned_and_licenses_are_recorded() {
        let catalog: serde_json::Value = serde_json::from_str(CATALOG).unwrap();
        let ane = catalog["ane_engines"]
            .as_array()
            .expect("catalog.json carries ane_engines");
        let entry = &ane[0];
        assert_eq!(entry["license"], "cc-by-4.0", "Parakeet weights license");
        assert_eq!(
            entry["runtime"]["license"], "apache-2.0",
            "FluidAudio license"
        );
        assert_eq!(entry["default"], false, "SP-10 changes no default");
        let version = entry["runtime"]["version"].as_str().unwrap();
        assert!(
            SWIFT_PACKAGE.contains(&format!("exact: \"{version}\"")),
            "Package.swift must pin FluidAudio exactly at the catalog's {version}"
        );
        assert!(entry["revision"].as_str().unwrap().len() == 40);
    }

    #[test]
    fn ane_helper_is_bundled_as_a_sidecar() {
        let conf: serde_json::Value = serde_json::from_str(TAURI_CONF).unwrap();
        let bins: Vec<&str> = conf["bundle"]["externalBin"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert!(bins.contains(&"binaries/yap-asr"));
        assert!(bins.contains(&"binaries/yap-engine-ane"));
    }
}
