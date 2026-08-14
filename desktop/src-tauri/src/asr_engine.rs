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

use serde::{Deserialize, Serialize};
use transcribe_cpp::{
    Backend, CancelToken, Model, ModelOptions, RunExtension, RunOptions, Session, TimestampKind,
    WhisperRunOptions,
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
    /// YV70 — the cooperative cancel token installed on the session at load.
    /// transcribe-cpp polls it between decode steps, so calling
    /// [`cancel`] from another thread ends an in-flight [`transcribe`] with
    /// `Error::Aborted` instead of leaving the caller to wait it out. Used by
    /// the exit drain, which has to get the engine (and its Metal device) back
    /// before the process reaches `exit()`.
    cancel: CancelToken,
}

impl AsrEngine {
    /// Whether this model accepts an `initial_prompt` bias (YV47).
    fn accepts_initial_prompt(&self) -> bool {
        self.arch == "whisper"
    }
}

/// A clone of this engine's cancel token (YV70) — cheap, shares one flag, and
/// can be held while the engine itself is leased out to a transcription thread.
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
pub fn reset_cancel(engine: &AsrEngine) {
    engine.cancel.reset();
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
    let transcript = engine
        .session
        .run(samples_16k_mono, &options)
        .map_err(|e| format!("transcription failed: {e}"))?;
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
            kind.trim_start_matches("max_timestamp_kind: ").len() > 0,
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
    #[test]
    fn auto_timestamps_are_not_mistaken_for_real_ones() {
        assert_eq!(TimedKind::from_native(TimestampKind::Auto), TimedKind::None);
        assert_eq!(TimedKind::from_native(TimestampKind::None), TimedKind::None);
        assert_eq!(
            TimedKind::from_native(TimestampKind::Word),
            TimedKind::Word
        );
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
