//! Y5-F — the take path's failures, as stable CODES the frontend catalogue reads.
//!
//! Before this, the take pipeline answered `Result<_, String>` and whatever the
//! string said — a cpal error, an io error's `Debug`, a DB message — reached the
//! status chip, the pill and the macOS notification verbatim. Now the pipeline
//! closure in `stop_and_transcribe` returns `Result<_, TakeError>`, so every
//! `Err` it can produce carries a code BY TYPE: there is no way to add a new
//! early return to that closure without choosing one (a bare `String` still
//! converts, through `From<String>`, to the generic [`TAKE_FAILED`] code, whose
//! line tells the user what to do next instead of echoing the raw string).
//!
//! The ONE `Err` arm of the take-result match then emits `transcript_error`
//! with [`error_payload`], so a code always reaches the frontend: the main
//! window's toast (line + action) and the pill's `error` phase (line only) both
//! read `desktop/src/errors.ts`'s catalogue. The test at the bottom of this file
//! reads that catalogue and fails if a code here has no line there.

use serde::Serialize;

/// The microphone could not be opened when a take started.
pub const MIC_UNAVAILABLE: &str = "mic_unavailable";
/// The input device dropped out in the middle of a take (YV67).
pub const MIC_DISCONNECTED: &str = "mic_disconnected";
/// No speech model is downloaded, so nothing can be transcribed (YV34).
pub const MODEL_MISSING: &str = "model_missing";
/// The capture could not be finalized into a clip.
pub const CAPTURE_FAILED: &str = "capture_failed";
/// The speech engine failed on this clip.
pub const TRANSCRIPTION_FAILED: &str = "transcription_failed";
/// The transcript could not be written to History.
pub const HISTORY_WRITE_FAILED: &str = "history_write_failed";
/// The transcript could not even be put on the clipboard.
pub const CLIPBOARD_FAILED: &str = "clipboard_failed";
/// The paste went out but nothing confirmed it landed (YV74).
pub const PASTE_NOT_CONFIRMED: &str = "paste_not_confirmed";
/// Anything on the take path that has no better code yet.
pub const TAKE_FAILED: &str = "take_failed";

/// What a person reads for [`TAKE_FAILED`] — never the raw string.
pub const TAKE_FAILED_LINE: &str = "Yap could not finish that take.";

/// Every code the take path can put on `transcript_error`, `take_failed`,
/// `last_error` or a refused command — the explicit list the "a failure is
/// never silent" test walks. Add a code here and the test makes you catalogue
/// it in errors.ts.
pub const TAKE_PATH_CODES: &[&str] = &[
    MIC_UNAVAILABLE,
    MIC_DISCONNECTED,
    MODEL_MISSING,
    CAPTURE_FAILED,
    TRANSCRIPTION_FAILED,
    HISTORY_WRITE_FAILED,
    CLIPBOARD_FAILED,
    PASTE_NOT_CONFIRMED,
    TAKE_FAILED,
    // Emitted elsewhere on the take path with their own channels:
    "silent_capture",          // lib.rs SILENT_CAPTURE_CODE (take_failed)
    "mic_permission_required", // the PERM-C gate event
    "license_required",        // CommandError::license_required
    "cancelled",               // take_cancelled (not a failure, still a line)
];

/// One failure on the take path: the code the UI branches on, the sentence a
/// person may read (status chip, notification, recovery row), and the raw
/// detail that only ever goes to the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeError {
    pub code: &'static str,
    pub message: String,
    pub detail: String,
}

impl TakeError {
    pub fn new(code: &'static str, message: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: detail.into(),
        }
    }
}

/// A bare string on the take path is a failure nobody has classified yet: it
/// keeps its text for the LOG and shows the generic line.
impl From<String> for TakeError {
    fn from(detail: String) -> Self {
        Self::new(TAKE_FAILED, TAKE_FAILED_LINE, detail)
    }
}

pub fn capture_failed(detail: String) -> TakeError {
    TakeError::new(CAPTURE_FAILED, "Yap could not save that recording.", detail)
}

pub fn transcription_failed(detail: String) -> TakeError {
    TakeError::new(
        TRANSCRIPTION_FAILED,
        "The speech engine could not transcribe that take — the audio was kept, press Retry.",
        detail,
    )
}

pub fn history_write_failed(detail: String) -> TakeError {
    TakeError::new(
        HISTORY_WRITE_FAILED,
        "Yap transcribed that take but could not save it to History.",
        detail,
    )
}

/// The take's hard error as the status carries it: a code plus a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastError {
    pub code: &'static str,
    pub message: String,
}

impl LastError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<&TakeError> for LastError {
    fn from(e: &TakeError) -> Self {
        Self::new(e.code, e.message.clone())
    }
}

/// The `transcript_error` payload: `{ code, message, failed }`. One builder so
/// the event can never go out without its code.
pub fn error_payload<T: Serialize>(e: &TakeError, failed: Option<T>) -> serde_json::Value {
    serde_json::json!({ "code": e.code, "message": &e.message, "failed": failed })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend catalogue, read from the other end of the wire.
    const CATALOGUE: &str = include_str!("../../src/errors.ts");

    #[test]
    fn every_take_path_code_has_a_catalogue_line() {
        for code in TAKE_PATH_CODES {
            assert!(
                CATALOGUE.contains(&format!("  {code}: {{")),
                "take-path code `{code}` has no entry in desktop/src/errors.ts ERROR_CATALOGUE"
            );
        }
    }

    /// The explicit enumeration of `stop_and_transcribe`'s error returns (the
    /// closure's early `return Err` / `?` sites and the record-start failure),
    /// each with the code it surfaces under. Update this list when you add a
    /// return: the closure's type already forces the code, this list forces
    /// the catalogue line.
    #[test]
    fn take_path_failures_are_never_silent() {
        let sites: &[(&str, TakeError)] = &[
            (
                "capture-stop thread panicked",
                capture_failed("panic".into()),
            ),
            ("record::finish_recording", capture_failed("io".into())),
            (
                "device failed mid-take",
                TakeError::new(MIC_DISCONNECTED, "m", "d"),
            ),
            ("no native model", TakeError::new(MODEL_MISSING, "m", "d")),
            ("transcribe_native", transcription_failed("ggml".into())),
            (
                "insert_transcript_at",
                history_write_failed("sqlite".into()),
            ),
            ("unclassified string", TakeError::from("boom".to_string())),
        ];
        for (site, e) in sites {
            assert!(
                TAKE_PATH_CODES.contains(&e.code),
                "{site}: {} not listed",
                e.code
            );
            assert!(!e.message.trim().is_empty(), "{site}: empty line");
            let payload = error_payload::<()>(e, None);
            assert_eq!(payload["code"], e.code, "{site}: payload lost its code");
            assert_eq!(payload["message"], e.message.as_str());
        }
    }

    #[test]
    fn an_unclassified_string_never_reaches_the_user() {
        let raw = "Os { code: 2, kind: NotFound, message: \"No such file\" }".to_string();
        let e = TakeError::from(raw.clone());
        assert_eq!(e.code, TAKE_FAILED);
        assert_eq!(e.message, TAKE_FAILED_LINE);
        assert_eq!(e.detail, raw, "the raw text is kept for the log");
        let last = LastError::from(&e);
        assert!(!last.message.contains("Os {"));
    }

    #[test]
    fn take_path_lines_leak_no_rust() {
        for e in [
            capture_failed("x".into()),
            transcription_failed("x".into()),
            history_write_failed("x".into()),
            TakeError::from("x".to_string()),
        ] {
            for bad in ["Error(", "::", "unwrap", "/Users", ".rs"] {
                assert!(!e.message.contains(bad), "{} leaks {bad}", e.code);
            }
        }
    }
}
