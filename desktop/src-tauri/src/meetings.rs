//! YV94 — the Notetaker's storage shape and its two pure surfaces.
//!
//! This module deliberately holds NO SQLite connection. It owns:
//!
//!   * the row types the rest of the app passes around (`Meeting`,
//!     `MeetingSegment`, `MeetingStats`),
//!   * the **migration-1 SQL** that introduces the meeting tables under the
//!     `PRAGMA user_version` ladder `db.rs` gains in the same item (plan
//!     finding #26 — `db.rs` had `CREATE TABLE IF NOT EXISTS` plus
//!     `let _ = conn.execute("ALTER TABLE …")`, which discards errors, and no
//!     `user_version` anywhere; the meeting tables are the right moment to fix
//!     that because nothing needs backfilling yet), and
//!   * the Markdown renderer, which is a pure function of a meeting and its
//!     segments so "does the export round-trip readably" is a test, not an eyeball.
//!
//! Scope discipline (22-A), each cut traceable:
//!   * no `consent_ack` column — finding #13 closed O1 as "one TERMS line + one
//!     one-time notice", so a single `settings_kv` key records it app-wide
//!     (YV96). A per-row column with no reader is worse than nothing.
//!   * no `speaker_id` / `speaker_label` / `cluster_index` / `overlapped` —
//!     diarization is yap23. They arrive as migration 2, additive-only.
//!   * no `track` column — 22-A captures one mic track. 22-B adds it (and
//!     `sys_wav_path`) as its own migration step.
//!   * retention is **time-based, 7 days** (finding #28), not
//!     delete-after-summarize: there is no summarize stage yet, so
//!     delete-after-summarize means either "never delete" or "the transcript is
//!     the only artifact and you can never re-hear the room". Flip the default
//!     once YV97 has shipped and proven itself.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Schema version this build expects. `db.rs` walks the ladder up to it.
///
/// 1 = the meeting tables below. Bump ONLY by appending a new arm to the
/// migration match in `db::run_migrations`; never edit a shipped step.
pub const SCHEMA_VERSION: i64 = 1;

/// How long a meeting's audio is kept before the startup/hygiene sweep purges
/// it. The transcript is kept forever — only the WAV goes (finding #28).
pub const AUDIO_RETENTION_DAYS: i64 = 7;

/// 22-A records one track: the mic, which is the person holding the Mac.
/// Labelling it costs nothing today and is what makes the 22-B upgrade legible
/// ("now it can tell you apart from the room") — finding #10.
pub const MIC_SPEAKER_LABEL: &str = "Me";

/// Lifecycle states a meeting row may hold. Kept as `&str` rather than a SQL
/// CHECK so a future phase can add one without a migration, but validated on
/// the way in by [`MeetingState::parse`] so a typo cannot reach the DB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeetingState {
    Recording,
    Transcribing,
    Summarizing,
    Complete,
    Failed,
    /// Some chunks landed, some did not — the honest outcome of YV93's
    /// per-chunk timeout isolation.
    Partial,
}

impl MeetingState {
    pub fn as_str(self) -> &'static str {
        match self {
            MeetingState::Recording => "recording",
            MeetingState::Transcribing => "transcribing",
            MeetingState::Summarizing => "summarizing",
            MeetingState::Complete => "complete",
            MeetingState::Failed => "failed",
            MeetingState::Partial => "partial",
        }
    }

    pub fn parse(s: &str) -> Option<MeetingState> {
        Some(match s {
            "recording" => MeetingState::Recording,
            "transcribing" => MeetingState::Transcribing,
            "summarizing" => MeetingState::Summarizing,
            "complete" => MeetingState::Complete,
            "failed" => MeetingState::Failed,
            "partial" => MeetingState::Partial,
            _ => return None,
        })
    }
}

/// One recorded meeting. Mirrors the `meetings` table one-for-one, plus
/// `segment_count`, which is a JOIN count rather than a stored column so it can
/// never drift from the segments actually present.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub id: String,
    pub title: String,
    /// `manual` in 22-A. `calendar` / `detected` are yap24 / yap26.
    pub source: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_seconds: f64,
    pub state: String,
    pub error: Option<String>,
    /// YV93's resume anchor: seconds of captured audio already transcribed, so
    /// a relaunch after a crash resumes instead of re-decoding from zero.
    #[serde(default)]
    pub processed_through_seconds: f64,
    pub audio_kept: bool,
    /// `None` once the retention sweep has purged the WAV (or it was never kept).
    pub mic_wav_path: Option<String>,
    /// Populated by YV97. `None` here is not an error state.
    pub summary: Option<String>,
    pub summary_model: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub segment_count: i64,
}

/// One chronological transcript segment. 22-A has no speaker columns — every
/// segment is the mic track, rendered as [`MIC_SPEAKER_LABEL`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSegment {
    pub id: String,
    pub meeting_id: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
    pub confidence: Option<f64>,
    pub created_at: DateTime<Utc>,
}

/// A segment on the way IN, before it has an id or a timestamp of its own.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMeetingSegment {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
    #[serde(default)]
    pub confidence: Option<f64>,
}

impl NewMeetingSegment {
    pub fn new(start_seconds: f64, end_seconds: f64, text: impl Into<String>) -> Self {
        Self {
            start_seconds,
            end_seconds,
            text: text.into(),
            confidence: None,
        }
    }
}

/// YV94 / finding #29 — the local rollup that stands in for the telemetry this
/// app deliberately does not have. Everything here is answerable from the two
/// meeting tables; the DER proxy and the "share of meetings with an action
/// checked off" line from the finding need diarization (yap23) and summaries
/// (yap25) and are therefore NOT faked here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingStats {
    pub total_meetings: i64,
    /// Meetings started in the last 7 local days — the retention signal.
    pub meetings_last_7: i64,
    pub total_seconds: f64,
    /// Reliability split. `complete` + `partial` + `failed` may be less than
    /// `total_meetings` while a meeting is still recording/transcribing.
    pub complete_meetings: i64,
    pub partial_meetings: i64,
    pub failed_meetings: i64,
    pub segments_indexed: i64,
    /// Activation: when the FIRST meeting was recorded, and how many days after
    /// the first dictation that was. `None` until a meeting exists.
    pub first_meeting_at: Option<DateTime<Utc>>,
    pub days_to_first_meeting: Option<i64>,
    pub last_meeting_at: Option<DateTime<Utc>>,
    /// Meetings still holding a WAV on disk (retention window not yet elapsed).
    pub meetings_with_audio: i64,
    /// Echoed so the UI never hardcodes the number it shows the user.
    pub audio_retention_days: i64,
}

/// Migration 1 — the meeting tables, their indexes, the FTS5 index over segment
/// text, and the ai/ad/au trigger trio that keeps it in sync.
///
/// The trigger trio is copied verbatim in shape from `transcripts_fts`, which
/// has been the app's search index since v0.1: for an external-content FTS5
/// table the 'delete' command must be given the OLD text, which is why the
/// update trigger deletes-then-inserts rather than replacing in place.
///
/// `ON DELETE CASCADE` is a safety net, not the delete path: `secure_delete=ON`
/// makes the cascade real work, and `Database::delete_meeting` deletes the
/// segments explicitly first so the `ad` trigger fires under every SQLite build
/// regardless of how that build treats triggers on FK actions.
pub const MIGRATION_1_MEETINGS: &str = r#"
CREATE TABLE IF NOT EXISTS meetings (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL DEFAULT 'Meeting',
  source TEXT NOT NULL DEFAULT 'manual',       -- manual only in 22-A (calendar/detected are 24/26)
  started_at TEXT NOT NULL,
  ended_at TEXT,
  duration_seconds REAL NOT NULL DEFAULT 0,
  state TEXT NOT NULL DEFAULT 'recording',     -- recording|transcribing|summarizing|complete|failed|partial
  error TEXT,
  processed_through_seconds REAL NOT NULL DEFAULT 0,  -- YV93 resume anchor
  audio_kept INTEGER NOT NULL DEFAULT 1,
  mic_wav_path TEXT,                           -- NULL once purged; sys_wav_path added in 22-B migration
  summary TEXT,
  summary_model TEXT,                          -- both populated by YV97
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_meetings_started ON meetings(started_at DESC);

CREATE TABLE IF NOT EXISTS meeting_segments (
  id TEXT PRIMARY KEY,
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  start_seconds REAL NOT NULL,
  end_seconds REAL NOT NULL,
  text TEXT NOT NULL DEFAULT '',
  confidence REAL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_segments_meeting ON meeting_segments(meeting_id, start_seconds);

CREATE VIRTUAL TABLE IF NOT EXISTS meeting_segments_fts
  USING fts5(text, content='meeting_segments', content_rowid='rowid');

CREATE TRIGGER IF NOT EXISTS meeting_segments_ai AFTER INSERT ON meeting_segments BEGIN
  INSERT INTO meeting_segments_fts(rowid, text) VALUES (new.rowid, new.text);
END;
CREATE TRIGGER IF NOT EXISTS meeting_segments_ad AFTER DELETE ON meeting_segments BEGIN
  INSERT INTO meeting_segments_fts(meeting_segments_fts, rowid, text)
    VALUES('delete', old.rowid, old.text);
END;
CREATE TRIGGER IF NOT EXISTS meeting_segments_au AFTER UPDATE ON meeting_segments BEGIN
  INSERT INTO meeting_segments_fts(meeting_segments_fts, rowid, text)
    VALUES('delete', old.rowid, old.text);
  INSERT INTO meeting_segments_fts(rowid, text) VALUES (new.rowid, new.text);
END;
"#;

/// `3725.4` → `01:02:05`. Always hh:mm:ss so a 3-hour meeting and a 3-minute one
/// line up in a monospace column, and so the exported Markdown sorts as text.
pub fn format_offset(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds.floor() as i64
    } else {
        0
    };
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    format!("{h:02}:{m:02}:{s:02}")
}

/// Human duration for the list row: `1h 04m`, `12m 30s`, `48s`.
pub fn format_duration(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds.round() as i64
    } else {
        0
    };
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// A filename stem that survives the Finder, a shell, and a zip: ASCII-ish,
/// no separators, no leading dot, never empty.
pub fn export_file_stem(title: &str, started_at: DateTime<Utc>) -> String {
    let mut slug = String::new();
    let mut last_dash = true; // suppresses a leading dash
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
        if slug.len() >= 40 {
            break;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug.push_str("meeting");
    }
    let stamp = started_at
        .with_timezone(&chrono::Local)
        .format("%Y%m%d-%H%M%S");
    format!("{slug}-{stamp}")
}

/// Render one meeting as Markdown. Pure — no DB, no clock, no filesystem — so
/// the "round-trips readably" acceptance is a real assertion.
///
/// Shape (stable; `tests/meeting_markdown_export.rs` pins it):
///
/// ```text
/// # <title>
///
/// - **Started:** 2026-08-11 09:03 (local)
/// - **Duration:** 12m 30s
/// - **Source:** manual · **State:** complete
///
/// ## Summary          <- only when YV97 has written one
///
/// ## Transcript
///
/// **[00:00:00] Me:** first thing said
/// ```
///
/// Every transcript line begins with the bold timestamp, so segment text that
/// happens to start with `#`, `-` or `>` can never be reinterpreted as Markdown
/// structure — the export is untrusted ASR output, not authored prose.
pub fn render_markdown(meeting: &Meeting, segments: &[MeetingSegment]) -> String {
    let mut out = String::with_capacity(256 + segments.len() * 64);
    out.push_str(&format!("# {}\n\n", one_line(&meeting.title)));
    out.push_str(&format!(
        "- **Started:** {} (local)\n",
        meeting
            .started_at
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
    ));
    out.push_str(&format!(
        "- **Duration:** {}\n",
        format_duration(meeting.duration_seconds)
    ));
    out.push_str(&format!(
        "- **Source:** {} · **State:** {}\n",
        one_line(&meeting.source),
        one_line(&meeting.state)
    ));
    if let Some(err) = meeting.error.as_deref().filter(|e| !e.trim().is_empty()) {
        out.push_str(&format!("- **Error:** {}\n", one_line(err)));
    }
    out.push('\n');

    if let Some(summary) = meeting.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("## Summary\n\n");
        out.push_str(summary.trim());
        out.push_str("\n\n");
    }

    out.push_str("## Transcript\n\n");
    if segments.is_empty() {
        out.push_str("_No transcript segments._\n");
        return out;
    }
    for seg in segments {
        let text = one_line(&seg.text);
        if text.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "**[{}] {}:** {}\n\n",
            format_offset(seg.start_seconds),
            MIC_SPEAKER_LABEL,
            text
        ));
    }
    out
}

/// Collapse any whitespace (including the newlines a transcript should never
/// contain but might) into single spaces, so one segment is always one line.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn meeting() -> Meeting {
        Meeting {
            id: "m1".into(),
            title: "Weekly sync".into(),
            source: "manual".into(),
            started_at: Utc.with_ymd_and_hms(2026, 8, 11, 16, 3, 0).unwrap(),
            ended_at: None,
            duration_seconds: 750.0,
            state: "complete".into(),
            error: None,
            processed_through_seconds: 750.0,
            audio_kept: true,
            mic_wav_path: None,
            summary: None,
            summary_model: None,
            created_at: Utc.with_ymd_and_hms(2026, 8, 11, 16, 3, 0).unwrap(),
            segment_count: 0,
        }
    }

    fn seg(start: f64, text: &str) -> MeetingSegment {
        MeetingSegment {
            id: format!("s{start}"),
            meeting_id: "m1".into(),
            start_seconds: start,
            end_seconds: start + 4.0,
            text: text.into(),
            confidence: None,
            created_at: Utc.with_ymd_and_hms(2026, 8, 11, 16, 3, 0).unwrap(),
        }
    }

    #[test]
    fn offsets_are_zero_padded_hms() {
        assert_eq!(format_offset(0.0), "00:00:00");
        assert_eq!(format_offset(65.9), "00:01:05");
        assert_eq!(format_offset(3725.4), "01:02:05");
        // Negative / NaN can only arrive from a broken decoder; clamp, never panic.
        assert_eq!(format_offset(-4.0), "00:00:00");
        assert_eq!(format_offset(f64::NAN), "00:00:00");
    }

    #[test]
    fn durations_read_like_a_human_wrote_them() {
        assert_eq!(format_duration(48.0), "48s");
        assert_eq!(format_duration(750.0), "12m 30s");
        assert_eq!(format_duration(3840.0), "1h 04m");
    }

    #[test]
    fn markdown_has_headers_timestamps_and_body() {
        let md = render_markdown(
            &meeting(),
            &[seg(0.0, "hello there"), seg(65.0, "second thing")],
        );
        assert!(md.starts_with("# Weekly sync\n"), "{md}");
        assert!(md.contains("## Transcript"));
        assert!(md.contains("**[00:00:00] Me:** hello there"));
        assert!(md.contains("**[00:01:05] Me:** second thing"));
        assert!(md.contains("- **Duration:** 12m 30s"));
    }

    #[test]
    fn segment_text_can_never_become_markdown_structure() {
        // ASR output is untrusted: a segment beginning "# " must not become a
        // heading in the exported file.
        let md = render_markdown(&meeting(), &[seg(0.0, "# not a heading\n- not a list")]);
        assert!(md.contains("**[00:00:00] Me:** # not a heading - not a list"));
        assert_eq!(
            md.lines().filter(|l| l.starts_with("# ")).count(),
            1,
            "only the title may be an h1:\n{md}"
        );
    }

    #[test]
    fn summary_section_appears_only_when_written() {
        let mut m = meeting();
        assert!(!render_markdown(&m, &[]).contains("## Summary"));
        m.summary = Some("We agreed to ship on Friday.".into());
        let md = render_markdown(&m, &[]);
        assert!(md.contains("## Summary"));
        assert!(md.contains("We agreed to ship on Friday."));
    }

    #[test]
    fn export_stem_is_filesystem_safe() {
        let started = Utc.with_ymd_and_hms(2026, 8, 11, 16, 3, 0).unwrap();
        let stem = export_file_stem("Weekly sync / Q3 (planning)", started);
        assert!(stem.starts_with("weekly-sync-q3-planning-"), "{stem}");
        assert!(!stem.contains('/'));
        assert!(!stem.contains(' '));
        // A title with nothing usable still yields a name.
        assert!(export_file_stem("///", started).starts_with("meeting-"));
    }

    #[test]
    fn states_round_trip() {
        for s in [
            MeetingState::Recording,
            MeetingState::Transcribing,
            MeetingState::Summarizing,
            MeetingState::Complete,
            MeetingState::Failed,
            MeetingState::Partial,
        ] {
            assert_eq!(MeetingState::parse(s.as_str()), Some(s));
        }
        assert_eq!(MeetingState::parse("done"), None);
    }
}
