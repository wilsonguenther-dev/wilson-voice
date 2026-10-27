//! Y8-D — coaching nudges: the backend half.
//!
//! The ENGINE is pure TypeScript (`src/pill/nudge.ts`): it decides which lesson,
//! if any, the pill teaches after a take. This module only supplies the two
//! facts the frontend cannot know on its own, and keeps the one record that has
//! to outlive a launch:
//!
//! 1. [`TakeContext`], emitted once per finished dictation as
//!    [`TAKE_CONTEXT_EVENT`]: did a text field have focus when the paste was
//!    decided, and which app family (`dictation::mode_for_app`) was frontmost.
//!    Today a take with no focused field succeeds silently into the clipboard
//!    and the user concludes Yap is broken; this is the signal that turns that
//!    take into a lesson.
//! 2. [`NudgeLedger`], one `settings_kv` row ([`NUDGE_LEDGER_KEY`]): when the
//!    ledger was first read (the "days since install" clock) and every nudge
//!    condition that has ever fired. "At most once per condition, EVER" is only
//!    true if "ever" survives a relaunch, so it lives in the database and not in
//!    the webview's storage.
//!
//! PRIVACY: neither carries text. The app family is a coarse bucket, never the
//! app name, and the ledger holds condition ids and one timestamp.

use crate::dictation::DictationMode;
use serde::{Deserialize, Serialize};

/// The Tauri event carrying a [`TakeContext`] (nudge.ts `TAKE_CONTEXT_EVENT`).
pub const TAKE_CONTEXT_EVENT: &str = "take_context";

/// The `settings_kv` key the ledger is stored under. Versioned so a reshaped
/// ledger can start clean instead of misreading an old one.
pub const NUDGE_LEDGER_KEY: &str = "nudge_ledger_v1";

/// A condition id is a short slug; anything else is refused rather than stored.
pub const MAX_CONDITION_LEN: usize = 40;

/// What one finished take tells the nudge engine. serde camelCase so the
/// TypeScript side reads the same names it declares (`TakeContextPayload`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeContext {
    /// Did the focused element accept text when the paste was decided?
    /// (`focus::should_auto_paste`, which answers `true` when Accessibility is
    /// not granted: without AX there is no way to know, so no lesson fires.)
    pub text_field_focused: bool,
    /// `dictation::mode_for_app` of the frontmost app, as a lowercase family.
    pub app_category: &'static str,
    /// Did the text actually land in the app (vs. clipboard only)?
    pub pasted: bool,
}

/// The family name the frontend keys its app-aware nudges off.
pub fn category_name(mode: DictationMode) -> &'static str {
    match mode {
        DictationMode::Email => "email",
        DictationMode::Document => "document",
        DictationMode::Notes => "notes",
        DictationMode::Code => "code",
        DictationMode::Chat => "chat",
        DictationMode::Plain => "plain",
        DictationMode::List => "list",
    }
}

impl TakeContext {
    pub fn new(text_field_focused: bool, source_app: Option<&str>, pasted: bool) -> Self {
        let mode = crate::dictation::mode_for_app(source_app.unwrap_or_default());
        Self {
            text_field_focused,
            app_category: category_name(mode),
            pasted,
        }
    }
}

/// The persisted half of the ledger: exactly what is stored in the row.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredLedger {
    /// Wall-clock ms of the first ledger read on this install.
    #[serde(default)]
    pub first_seen_ms: i64,
    /// Every condition that has ever fired, in firing order, no duplicates.
    #[serde(default)]
    pub shown: Vec<String>,
}

/// What the pill reads at mount: the stored ledger plus the two counts the
/// engine's caps need, read from the history it already keeps.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NudgeLedger {
    pub first_seen_ms: i64,
    pub shown: Vec<String>,
    /// Dictations ever saved (`Insights::total_sessions`).
    pub successful_takes: i64,
    /// Meetings ever recorded (`MeetingStats::total_meetings`).
    pub meetings_recorded: i64,
}

/// Parse a stored row, starting the install clock at `now_ms` when the row is
/// missing or unreadable. A corrupt row costs the user, at worst, one repeated
/// lesson; it never costs an error dialog.
pub fn load(raw: Option<&str>, now_ms: i64) -> (StoredLedger, bool) {
    let parsed = raw.and_then(|r| serde_json::from_str::<StoredLedger>(r).ok());
    match parsed {
        Some(l) if l.first_seen_ms > 0 => (l, false),
        Some(mut l) => {
            l.first_seen_ms = now_ms;
            (l, true)
        }
        None => (
            StoredLedger {
                first_seen_ms: now_ms,
                shown: Vec::new(),
            },
            true,
        ),
    }
}

/// Record that `condition` fired. Returns `false` (and changes nothing) when it
/// had already fired or is not a well-formed slug: a condition fires once, ever.
pub fn mark(ledger: &mut StoredLedger, condition: &str) -> bool {
    let ok = !condition.is_empty()
        && condition.len() <= MAX_CONDITION_LEN
        && condition
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok || ledger.shown.iter().any(|s| s == condition) {
        return false;
    }
    ledger.shown.push(condition.to_string());
    true
}

/// Read the ledger, creating it (and so starting the install clock) on first use.
pub fn read_ledger(db: &crate::db::Database, now_ms: i64) -> NudgeLedger {
    let (stored, fresh) = load(db.setting_get(NUDGE_LEDGER_KEY).as_deref(), now_ms);
    if fresh {
        if let Ok(json) = serde_json::to_string(&stored) {
            if let Err(e) = db.setting_set(NUDGE_LEDGER_KEY, &json) {
                log::warn!("nudge ledger not written: {e}");
            }
        }
    }
    NudgeLedger {
        first_seen_ms: stored.first_seen_ms,
        shown: stored.shown,
        successful_takes: db.insights().map(|i| i.total_sessions).unwrap_or(0),
        meetings_recorded: db.meeting_stats().map(|m| m.total_meetings).unwrap_or(0),
    }
}

/// Persist that `condition` fired. Idempotent.
pub fn mark_shown(db: &crate::db::Database, condition: &str, now_ms: i64) -> Result<bool, String> {
    let (mut stored, _) = load(db.setting_get(NUDGE_LEDGER_KEY).as_deref(), now_ms);
    if !mark(&mut stored, condition) {
        return Ok(false);
    }
    let json = serde_json::to_string(&stored).map_err(|e| e.to_string())?;
    db.setting_set(NUDGE_LEDGER_KEY, &json)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_row_starts_the_install_clock_now() {
        let (l, fresh) = load(None, 1_000);
        assert!(fresh);
        assert_eq!(l.first_seen_ms, 1_000);
        assert!(l.shown.is_empty());
    }

    #[test]
    fn a_stored_row_keeps_its_original_clock() {
        let (l, fresh) = load(
            Some(r#"{"firstSeenMs":42,"shown":["no-text-field"]}"#),
            9_999,
        );
        assert!(!fresh);
        assert_eq!(l.first_seen_ms, 42);
        assert_eq!(l.shown, vec!["no-text-field".to_string()]);
    }

    #[test]
    fn a_corrupt_row_is_a_fresh_ledger_not_an_error() {
        let (l, fresh) = load(Some("{not json"), 5);
        assert!(fresh);
        assert_eq!(l.first_seen_ms, 5);
    }

    #[test]
    fn a_condition_is_recorded_once_ever() {
        let mut l = StoredLedger::default();
        assert!(mark(&mut l, "no-text-field"));
        assert!(!mark(&mut l, "no-text-field"));
        assert_eq!(l.shown.len(), 1);
    }

    #[test]
    fn only_slugs_are_stored() {
        let mut l = StoredLedger::default();
        assert!(!mark(&mut l, ""));
        assert!(!mark(&mut l, "Has Spaces"));
        assert!(!mark(&mut l, &"x".repeat(MAX_CONDITION_LEN + 1)));
        assert!(l.shown.is_empty());
    }

    #[test]
    fn the_payload_speaks_camel_case_and_carries_no_app_name() {
        let ctx = TakeContext::new(false, Some("Visual Studio Code"), false);
        let json = serde_json::to_string(&ctx).unwrap();
        assert_eq!(
            json,
            r#"{"textFieldFocused":false,"appCategory":"code","pasted":false}"#
        );
        assert!(!json.contains("Visual"));
    }
}
