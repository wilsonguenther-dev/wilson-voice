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
/// 1 = the meeting tables below.
/// 2 = YV95's `diagnostics` column (OS-12's thermal + battery instrumentation).
/// 3 = YV106's two-track columns (`sys_wav_path`, [`TAP_REBUILDS_COLUMN`],
///     `meeting_segments.track`).
///
/// Bump ONLY by appending a new arm to the migration match in
/// `db::run_migrations`; never edit a shipped step.
pub const SCHEMA_VERSION: i64 = 3;

/// YV104 / OS-4 — the column migration 3 (YV106) adds for the system-audio
/// tap's rebuild log, named here rather than invented there.
///
/// It holds [`crate::syscapture::TapRebuildLog::to_json`] verbatim: the count,
/// the budget, each attempt's meeting-relative millisecond stamp and host time,
/// why it fired, the innocent causes that were also plausible at the time, and
/// the verdict if the meeting ran out of rebuilds. `NULL` for every meeting
/// that never needed one, which is almost all of them.
///
/// YV104 wrote the structure; **YV106 (migration 3, below) adds the column** —
/// `sys_wav_path`, `meeting_segments.track` and this one together, as one
/// honest step 3. The name is fixed here so the producer and the destination
/// cannot be written down differently in two places, and
/// `migration_three_uses_the_column_name_yv104_published` is what proves the SQL
/// below still spells it this way.
pub const TAP_REBUILDS_COLUMN: &str = crate::syscapture::TAP_REBUILDS_COLUMN;

/// How long a meeting's audio is kept before the startup/hygiene sweep purges
/// it. The transcript is kept forever — only the WAV goes (finding #28).
pub const AUDIO_RETENTION_DAYS: i64 = 7;

/// 22-A records one track: the mic, which is the person holding the Mac.
/// Labelling it costs nothing today and is what makes the 22-B upgrade legible
/// ("now it can tell you apart from the room") — finding #10.
pub const MIC_SPEAKER_LABEL: &str = "Me";

/// YV108 — the counterpart finding #10 promised: everything that reached the
/// Mac's speakers rather than its microphone, which in a virtual meeting is
/// whoever else is in the call.
///
/// Deliberately NOT a name and not per-person: the system track is one mixed
/// stream of every remote participant, and telling those apart is diarization
/// (yap23). "Them" is the honest granularity this phase actually has.
pub const SYSTEM_SPEAKER_LABEL: &str = "Them";

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
    /// YV106 — the system-audio track's WAV, for a two-track (virtual) meeting.
    ///
    /// `None` means "this meeting has no second track", which covers every
    /// meeting recorded before migration 3, every in-person one after it, and
    /// one whose tap degraded to `track_lost` (matrix row #2) without ever
    /// delivering audio. It goes `None` again when retention purges the audio,
    /// exactly as `mic_wav_path` does.
    #[serde(default)]
    pub sys_wav_path: Option<String>,
    /// YV104 / YV106 — the system-audio tap's rebuild log
    /// (`syscapture::TapRebuildLog::to_json`), or `None` for the overwhelming
    /// majority of meetings, which never needed a rebuild.
    #[serde(default)]
    pub tap_rebuilds: Option<String>,
    /// Populated by YV97. `None` here is not an error state.
    pub summary: Option<String>,
    pub summary_model: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub segment_count: i64,
    /// YV95 / OS-12 — the session's energy diagnostics as a JSON blob
    /// (`meeting_energy::MeetingDiagnostics`): thermal state at start, every
    /// thermal transition observed while recording, and the battery preflight.
    /// `None` for meetings recorded before migration 2, and for a session whose
    /// probes all failed — "never measured" is not "measured nothing".
    #[serde(default)]
    pub diagnostics: Option<String>,
}

/// One chronological transcript segment. Track 0 is the mic — the person
/// holding the Mac — and is rendered as [`MIC_SPEAKER_LABEL`].
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
    /// YV106 — which recorded track this segment was transcribed from.
    /// [`MIC_TRACK`] (0) or [`SYSTEM_TRACK`] (1). Every 22-A row reads as 0
    /// without a backfill, because 22-A only ever recorded the mic.
    #[serde(default)]
    pub track: i64,
}

/// The mic track, as the DB spells it. Mirrors `meeting::MIC_TRACK`, which is
/// the same number on the capture side; `tracks_agree_across_the_two_modules`
/// keeps the pair from drifting.
pub const MIC_TRACK: i64 = 0;

/// The system-audio track (22-B's process tap). Mirrors `meeting::SYSTEM_TRACK`.
pub const SYSTEM_TRACK: i64 = 1;

/// A segment on the way IN, before it has an id or a timestamp of its own.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMeetingSegment {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
    #[serde(default)]
    pub confidence: Option<f64>,
    /// Defaulted to [`MIC_TRACK`] by [`NewMeetingSegment::new`], so every
    /// existing caller keeps writing exactly the rows it wrote before. The tap's
    /// transcription says otherwise with [`NewMeetingSegment::on_track`].
    #[serde(default)]
    pub track: i64,
}

impl NewMeetingSegment {
    pub fn new(start_seconds: f64, end_seconds: f64, text: impl Into<String>) -> Self {
        Self {
            start_seconds,
            end_seconds,
            text: text.into(),
            confidence: None,
            track: MIC_TRACK,
        }
    }

    /// The same segment, attributed to `track`.
    pub fn on_track(mut self, track: i64) -> Self {
        self.track = track;
        self
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

// ── YV96 · the one-time meeting-capture notice ──────────────────────────────
//
// O1 was closed by Wilson as "consent liability on the user — one TERMS line +
// one-time notice", which is why this is three constants and a struct rather
// than the six-item consent system §8 of the plan originally specified
// (finding #13). There is deliberately NO reminder toggle, no home-state legal
// setting, no gate-vs-nudge branching, and no per-meeting `consent_ack` column:
// the acknowledgement is app-wide, so one row in the existing `settings_kv`
// table is the whole mechanism.

/// The `settings_kv` key that records the notice has been shown and closed.
///
/// Versioned in the name on purpose: if the copy is ever materially rewritten
/// (a new jurisdiction claim, a change to what Yap captures), the honest move is
/// a `_v2` key so the new text is shown once, rather than silently treating an
/// old acknowledgement as covering new words.
pub const CONSENT_NOTICE_KEY: &str = "meeting_consent_notice_ack_v1";

// ── YV102 · the system-audio setup step ─────────────────────────────────────
//
// A SECOND key, deliberately, sitting beside [`CONSENT_NOTICE_KEY`] rather than
// reusing it. "The user read the notice about recording other people" and "the
// user ran the tap pre-warm so macOS has shown its permission alert" are
// different facts about different mechanisms, and collapsing them would mean
// one of two bugs: a user who acknowledged the notice never gets the pre-warm
// (so the alert ambushes them mid-Zoom, which is the entire thing YV102 exists
// to prevent), or a user who ran the pre-warm never sees the notice. Neither is
// recoverable from a single boolean, so there are two.

/// The `settings_kv` key that records the system-audio setup step's last run.
///
/// Versioned like the consent key, and for the same reason: if what the step
/// does materially changes, a `_v2` key re-runs it rather than treating an old
/// run as covering new behaviour.
pub const SYSTEM_AUDIO_SETUP_ACK_KEY: &str = "meeting_system_audio_setup_ack_v1";

/// What the last run of the setup step concluded.
///
/// Note what is NOT here: `Denied`. Yap cannot read TCC — there is no public API
/// — so the strongest thing it is allowed to say is [`SetupVerdict::LooksDenied`],
/// and only after `syscapture::permission_verdict` has watched a tap run for
/// [`crate::syscapture::DENIAL_GRACE`] without a single non-zero sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupVerdict {
    /// The step has never run on this Mac.
    NotRun,
    /// The tap opened and was torn down. The alert has been shown (or was
    /// already answered). Says nothing about the answer — that is the point:
    /// 200 ms of silence is not evidence of a refusal.
    Ran,
    /// A non-zero sample actually arrived. The only positive proof available.
    Granted,
    /// A tap ran past the grace window and never delivered anything, ever.
    LooksDenied,
    /// This Mac is below macOS 14.4, so there is no process-tap API to ask
    /// with. Mic-only meeting recording is unaffected (YV101).
    Unavailable,
    /// A CoreAudio call failed. Distinct from `LooksDenied` on purpose: a
    /// failure is a bug report, a denial is a Settings trip.
    Failed,
}

impl SetupVerdict {
    fn as_str(self) -> &'static str {
        match self {
            SetupVerdict::NotRun => "not_run",
            SetupVerdict::Ran => "ran",
            SetupVerdict::Granted => "granted",
            SetupVerdict::LooksDenied => "looks_denied",
            SetupVerdict::Unavailable => "unavailable",
            SetupVerdict::Failed => "failed",
        }
    }

    fn parse(text: &str) -> SetupVerdict {
        match text {
            "ran" => SetupVerdict::Ran,
            "granted" => SetupVerdict::Granted,
            "looks_denied" => SetupVerdict::LooksDenied,
            "unavailable" => SetupVerdict::Unavailable,
            "failed" => SetupVerdict::Failed,
            // Anything unrecognised — including a value written by a future
            // build and read by an older one — degrades to "it ran", never to a
            // denial. A wrong "looks denied" sends the user to System Settings
            // to fix something that is not broken.
            _ => SetupVerdict::Ran,
        }
    }
}

/// The state the setup step's UI renders from the raw `settings_kv` value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemAudioSetup {
    /// False until the step has run once. Drives the "Set up meeting
    /// recording" call to action.
    pub has_run: bool,
    /// When it last ran. Unlike the consent notice — where the FIRST close wins
    /// because the question is "when did you first see this text" — the LAST run
    /// wins here, because the question is "what does Yap currently believe about
    /// this Mac's permission", and that changes.
    pub last_run_at: Option<String>,
    pub verdict: SetupVerdict,
    /// Always `false`, and asserted by `tests/settings_kv.rs`.
    ///
    /// Mic-only recording (22-A) runs on the macOS 12 floor and is never gated
    /// on a system-audio permission. A surface that puts this step in front of
    /// `toggle_meeting_recording` has to actively ignore this field to break it.
    pub blocks_recording: bool,
}

impl SystemAudioSetup {
    /// The stored row: `"<verdict> <rfc3339>"`. One row, self-describing, and
    /// parseable by an older build (which reads the verdict it does not know as
    /// [`SetupVerdict::Ran`]).
    pub fn encode(verdict: SetupVerdict, at: &str) -> String {
        format!("{} {at}", verdict.as_str())
    }

    /// Build the state from whatever is in the row — including nothing, and
    /// including something malformed. Never fails: a permission surface that
    /// errors is a surface that either nags forever or hides a real denial.
    pub fn from_row(row: Option<String>) -> Self {
        let Some(row) = row else {
            return Self {
                has_run: false,
                last_run_at: None,
                verdict: SetupVerdict::NotRun,
                blocks_recording: false,
            };
        };
        let (verdict, at) = row.split_once(' ').unwrap_or((row.as_str(), ""));
        Self {
            has_run: true,
            last_run_at: (!at.is_empty()).then(|| at.to_string()),
            verdict: SetupVerdict::parse(verdict),
            blocks_recording: false,
        }
    }

    /// Should the UI show the honest denied state and the Settings deep link?
    pub fn looks_denied(&self) -> bool {
        self.verdict == SetupVerdict::LooksDenied
    }
}

/// The Tauri event name the meeting status is broadcast under — the ONE place
/// it is written on the Rust side.
///
/// This exists because the notice's trigger crosses a process boundary on a
/// bare string. YV95's `meeting_status_sink` emits it once a second; the
/// webview's `watchMeetingConsent` (`src/meetings/consentWatch.ts`) listens for
/// it and raises the one-time capture notice. Nothing type-checks that pair:
/// rename one end and the app still builds, every unit test still passes, and
/// the notice — the whole of YV96 — silently never renders again.
///
/// So both ends name the constant instead of the string, and
/// `tests/meeting_event_contract.rs` asserts the Rust value, the TypeScript
/// value and the listener's subscription are the same word. Emitters must use
/// `MEETING_EVENT` rather than a `"meeting"` literal; that test fails them if
/// they do not.
pub const MEETING_EVENT: &str = "meeting";

/// What the acknowledgement row actually stores: an RFC3339 timestamp of the
/// first close. The value is bookkeeping, not a decision — nothing reads it back
/// except the UI, which shows it in Settings → Privacy.
///
/// The notice **never** gates recording (see [`MeetingConsent::blocks_recording`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingConsent {
    /// True until the notice has been shown once and closed. The UI opens the
    /// sheet on a record attempt when this is true, and never otherwise.
    pub should_show: bool,
    /// When the notice was first closed. `None` until then; never overwritten by
    /// a later close, so "when did this user first see it" stays answerable.
    pub acknowledged_at: Option<String>,
    /// Always `false`, and asserted by `tests/consent_sheet_shown_once.rs`.
    ///
    /// This field exists so the nudge-not-gate decision is a value the UI reads
    /// rather than a convention a future entry point can forget: a surface that
    /// wires the sheet in front of `toggle_meeting_recording` has to actively
    /// ignore this to break O1.
    pub blocks_recording: bool,
}

impl MeetingConsent {
    /// Build the state the UI renders from the raw `settings_kv` value.
    ///
    /// Any stored value — even an empty string from a half-written write — counts
    /// as acknowledged: the failure mode of re-showing a legal notice forever is
    /// worse than the failure mode of not showing it twice.
    pub fn from_ack(ack: Option<String>) -> Self {
        Self {
            should_show: ack.is_none(),
            acknowledged_at: ack,
            blocks_recording: false,
        }
    }
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

/// Migration 2 — YV95 / OS-12's diagnostics column.
///
/// One nullable TEXT column holding a JSON blob, not five typed columns: this is
/// diagnostic exhaust nothing queries or filters on, and a column per reading
/// would mean a migration every time a later phase learns to record one more
/// thing about a session. `ALTER TABLE … ADD COLUMN` with a NULL default is an
/// O(1) header rewrite in SQLite — it does not touch existing rows — so this is
/// free even on a database full of meetings.
///
/// Not `IF NOT EXISTS` (SQLite's ADD COLUMN has no such clause): the ladder in
/// `db::run_migrations` guarantees a step runs exactly once, inside its own
/// transaction, with `user_version` written in the same transaction. That is the
/// mechanism finding #26 asked for, and re-deriving idempotency per statement
/// would say we do not trust it.
pub const MIGRATION_2_MEETING_DIAGNOSTICS: &str = r#"
ALTER TABLE meetings ADD COLUMN diagnostics TEXT;
"#;

/// Migration 3 — YV106's two-track columns.
///
/// Three additive columns, one step, because they are one change: a meeting can
/// now have a second recorded track, and all three answer a question about it.
///
///   * `sys_wav_path` — the system-audio track's wav, parallel to the
///     `mic_wav_path` migration 1 shipped. `NULL` for every mic-only meeting,
///     which is every meeting recorded before this build and every in-person one
///     after it. A separate column rather than a `wav_paths` JSON array because
///     the retention sweep and the delete path both need to find these paths
///     with a `WHERE … IS NOT NULL`, which is a query, not exhaust.
///   * [`TAP_REBUILDS_COLUMN`] — YV104's rebuild log, verbatim
///     `TapRebuildLog::to_json`. Exhaust, so one nullable TEXT blob, following
///     migration 2's own stated policy.
///   * `meeting_segments.track` — which track a transcript segment came from,
///     `0` = mic, `1` = system audio. `NOT NULL DEFAULT 0` so every segment
///     already in the wild reads correctly as the mic track with **no backfill**:
///     22-A only ever recorded the mic, so the default is not a guess, it is the
///     fact. This is what lets a stored segment be attributed before YV107's
///     host-time merge exists, and what YV108's Me/Them rendering reads.
///
/// `ALTER TABLE … ADD COLUMN` with a constant default is an O(1) header rewrite
/// in SQLite — it does not touch existing rows — so this is free even on a
/// database full of meetings. Not `IF NOT EXISTS` (SQLite's ADD COLUMN has no
/// such clause): the ladder in `db::run_migrations` guarantees a step runs
/// exactly once, inside its own transaction, with `user_version` written in the
/// same transaction.
pub const MIGRATION_3_TWO_TRACK: &str = r#"
ALTER TABLE meetings ADD COLUMN sys_wav_path TEXT;
ALTER TABLE meetings ADD COLUMN tap_rebuilds TEXT;
ALTER TABLE meeting_segments ADD COLUMN track INTEGER NOT NULL DEFAULT 0;
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

// ── YV108 · the mixed Me/Them transcript ────────────────────────────────────
//
// YV106 gave every stored segment a `track`; YV107 orders the two tracks
// against one common host-time base. This is the third and last step: turning
// that sequence into something a person reads.
//
// The shape is ONE interleaved list, not two columns. Finding #10's whole
// point is a legible conversation ("now it can tell the Thems apart"), and two
// parallel dumps leave the reader doing the interleave by eye — which is the
// work the merge was for. The rules live here and the Markdown export calls
// them directly; a UI-only fix would leave every exported file still labelling
// the room as "Me".
//
// The Meetings UI cannot call this function — it is React, rendering rows it
// already holds — so it runs a hand-written mirror, `src/meetings/transcript.ts`.
// A mirror agrees only where something checks that it does, which is why every
// rule here (order, tie-break, labels, whitespace collapsing, dropped blank
// spans, what counts as a second track) has the SAME fixture asserted on both
// sides. The first review of YV108 found this exact seam open — Rust dropped
// whitespace-only spans and the mirror did not, so a blank tap segment drew a
// labelled empty "Them" row on a screen whose export had no such line. Adding a
// rule below without adding its fixture to `transcript.test.ts` reopens it.

/// The speaker label for a stored segment's track.
///
/// [`MIC_TRACK`] is the person holding the Mac. Anything else came out of the
/// process tap — i.e. it was NOT this user's microphone — so it is "Them". The
/// catch-all is deliberate rather than a `panic!` or an `Option`: `track` is a
/// plain `INTEGER` column with no CHECK constraint, and a transcript that
/// renders is worth more than one that refuses to over a number no writer in
/// this codebase produces.
pub fn speaker_label(track: i64) -> &'static str {
    if track == MIC_TRACK {
        MIC_SPEAKER_LABEL
    } else {
        SYSTEM_SPEAKER_LABEL
    }
}

/// Does this meeting have a second track worth labelling?
///
/// Drives the UI's decision to show a speaker column at all: a 22-A (mic-only)
/// meeting must render exactly as it always did, with no phantom "Them" and no
/// layout that implies a second speaker who was never recorded.
///
/// Asked of the segments that RENDER, not of the raw rows. A tap track whose
/// spans all collapse to nothing produces no lines out of [`render_transcript`],
/// so treating it as a second speaker would reserve gutter width for someone
/// who appears neither on the screen nor in the exported file — which is the
/// same "the two surfaces disagree" failure the shared rendering exists to
/// prevent, just moved into the layout.
pub fn is_two_track(segments: &[MeetingSegment]) -> bool {
    segments
        .iter()
        .any(|s| s.track != MIC_TRACK && !one_line(&s.text).is_empty())
}

/// One rendered transcript line — the unit both surfaces draw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptLine {
    pub segment_id: String,
    /// Which track spoke it, kept so a surface can style Me and Them apart
    /// without re-deriving it from the label string.
    pub track: i64,
    pub speaker: &'static str,
    /// `hh:mm:ss` from the start of the meeting, already formatted.
    pub offset: String,
    pub start_seconds: f64,
    /// Whitespace-collapsed to exactly one line (see [`one_line`]).
    pub text: String,
}

/// Order a meeting's segments into the single interleaved transcript the UI and
/// the export both render.
///
/// Ordering is by `start_seconds` — which, for a two-track meeting, YV107 has
/// already rebased onto one common host-time origin, so comparing them is
/// meaningful across tracks rather than comparing two unrelated clocks. Ties
/// break to the mic first, so an answer and the question it overlaps land in a
/// fixed order instead of whichever row SQLite happened to return.
///
/// The sort is STABLE and the comparator is a total order (a non-finite
/// `start_seconds` from a broken decoder sorts as 0, matching
/// [`format_offset`]'s clamp rather than poisoning the whole comparison the way
/// a raw `partial_cmp().unwrap()` would). On an already-ascending single-track
/// input — every 22-A meeting, and everything `db::list_meeting_segments`
/// returns — it is the identity, which is what makes the byte-identical
/// no-regression guarantee hold.
///
/// Empty segments are dropped, exactly as the 22-A renderer already dropped
/// them: a blank line with a timestamp is noise in the UI and in the file.
pub fn render_transcript(segments: &[MeetingSegment]) -> Vec<TranscriptLine> {
    let mut lines: Vec<TranscriptLine> = segments
        .iter()
        .filter_map(|seg| {
            let text = one_line(&seg.text);
            if text.is_empty() {
                return None;
            }
            Some(TranscriptLine {
                segment_id: seg.id.clone(),
                track: seg.track,
                speaker: speaker_label(seg.track),
                offset: format_offset(seg.start_seconds),
                start_seconds: seg.start_seconds,
                text,
            })
        })
        .collect();
    lines.sort_by(|a, b| {
        let key = |v: f64| if v.is_finite() { v } else { 0.0 };
        key(a.start_seconds)
            .total_cmp(&key(b.start_seconds))
            .then(a.track.cmp(&b.track))
    });
    lines
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
///
/// YV108 — the transcript body is [`render_transcript`], so a two-track meeting
/// exports as one interleaved `Me:` / `Them:` conversation in the same file
/// shape, and a mic-only meeting exports BYTE-IDENTICALLY to what 22-A shipped
/// (`tests/meeting_markdown_export_two_track.rs` pins that against a fixture
/// captured from the pre-YV108 renderer).
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
    for line in render_transcript(segments) {
        out.push_str(&format!(
            "**[{}] {}:** {}\n\n",
            line.offset, line.speaker, line.text
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
            sys_wav_path: None,
            tap_rebuilds: None,
            diagnostics: None,
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
            track: MIC_TRACK,
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

    /// YV104 published the column name; migration 3 is where it lands. Nothing
    /// else type-checks that pair — the SQL is a string literal, and a rename on
    /// either side would leave the writer storing a blob into a column that does
    /// not exist, discovered by a user with a broken tap and no log.
    #[test]
    fn migration_three_uses_the_column_name_yv104_published() {
        assert_eq!(TAP_REBUILDS_COLUMN, "tap_rebuilds");
        assert!(
            MIGRATION_3_TWO_TRACK.contains(&format!("ADD COLUMN {TAP_REBUILDS_COLUMN} TEXT")),
            "migration 3 no longer adds the column YV104 named:\n{MIGRATION_3_TWO_TRACK}"
        );
    }

    /// The capture side and the storage side both number the tracks, in two
    /// modules that never call each other. They must agree, or a segment
    /// transcribed from the tap's wav is stored as the mic's.
    #[test]
    fn tracks_agree_across_the_two_modules() {
        assert_eq!(MIC_TRACK as usize, crate::meeting::MIC_TRACK);
        assert_eq!(SYSTEM_TRACK as usize, crate::meeting::SYSTEM_TRACK);
    }

    /// A segment written by any 22-A caller is still a mic segment: the default
    /// is the fact, not a guess.
    #[test]
    fn a_new_segment_is_the_mic_track_unless_it_says_otherwise() {
        assert_eq!(NewMeetingSegment::new(0.0, 1.0, "hello").track, MIC_TRACK);
        assert_eq!(
            NewMeetingSegment::new(0.0, 1.0, "hello")
                .on_track(SYSTEM_TRACK)
                .track,
            SYSTEM_TRACK
        );
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
