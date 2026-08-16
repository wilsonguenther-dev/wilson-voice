//! Local data stack for Wilson Voice.
//!
//! SQLite in WAL mode + FTS5 full-text search — same pattern used by
//! OpenWhispr, Muesli, and sflow for dictation history.
//! GraphQL is overkill for a single-user desktop app; typed Tauri commands
//! over this SQLite layer give fast retrieval without a network hop.

use crate::meetings::{
    self, Meeting, MeetingConsent, MeetingSegment, MeetingStats, NewMeetingSegment, SCHEMA_VERSION,
};
use crate::snippets::SnippetRule;
use crate::speaker_profiles;
use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntry {
    pub id: String,
    pub text: String,
    pub backend: String,
    /// Model inference wall time (seconds). Never used for WPM.
    pub asr_seconds: f64,
    /// Audio duration of the utterance (seconds) — sole input for WPM.
    #[serde(default)]
    pub speech_seconds: f64,
    /// Release → clipboard wall ms (latency north-star metric).
    #[serde(default)]
    pub pipeline_ms: i64,
    pub word_count: i64,
    pub created_at: DateTime<Utc>,
    pub source_app: Option<String>,
    /// Raw ASR transcript before the cleanup pipeline (YV10). `text` holds the
    /// polished result; this preserves the verbatim dictation for raw↔polished
    /// display / "Undo AI edit". Defaults to `text` when no raw is supplied and
    /// is `None` only for legacy rows written before the column existed.
    #[serde(default)]
    pub raw_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictEntry {
    pub id: String,
    pub term: String,
    pub preferred: Option<String>,
    pub hits: i64,
    pub created_at: DateTime<Utc>,
    /// YV47 — "always bias": a starred term is pinned to the head of the
    /// ranking, is never purged, and is the last thing dropped when the decoder
    /// prompt is capped.
    #[serde(default)]
    pub starred: bool,
}

/// YV47 — a word Yap noticed the user fixing, waiting to be accepted into the
/// dictionary. Learned only from an explicit "Fix transcription" edit, never
/// from guessing at the clipboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictCandidate {
    pub id: String,
    /// The form the user actually wants.
    pub term: String,
    /// What ASR produced, when the fix REPLACED a word (`None` when the user
    /// typed in a word that was missing entirely).
    pub wrong: Option<String>,
    /// How many separate corrections produced this candidate.
    pub use_count: i64,
    pub created_at: DateTime<Utc>,
}

/// YV47 — one word changed by a "Fix transcription" edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    /// What ASR produced, or `None` for a word the user inserted.
    pub wrong: Option<String>,
    /// The form the user typed instead.
    pub term: String,
}

/// YV52 — a take whose transcription FAILED, kept so the user can retry it.
///
/// The audio the pipeline had already written is parked outside the recordings
/// dir and pointed at by `wav_path`; nothing about the take's words is stored
/// (there are none — that is the whole point of the row).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedDictation {
    pub id: String,
    /// Absolute path to the preserved 16 kHz mono WAV.
    pub wav_path: String,
    /// Voiced seconds of the take, carried over to the transcript row a
    /// successful retry writes (so WPM stays honest for a recovered take).
    pub speech_seconds: f64,
    /// The failure the user saw ("No speech model installed — …").
    pub error: String,
    /// The app that was focused when the take was captured.
    pub source_app: Option<String>,
    /// When the take was SPOKEN — a retry keeps this, not the retry instant.
    pub created_at: DateTime<Utc>,
}

/// YV52 — how long a failed take's audio is kept before it is purged, matching
/// the app's "audio never lingers" privacy stance: recoverable for a week, gone
/// after that whether or not the user ever retried it.
pub const FAILED_TAKE_RETENTION_DAYS: i64 = 7;

/// YV64 — one crash Yap has evidence of: a Rust panic the hook logged, a native
/// crash macOS wrote a `.ips` report for, or a watchdog kill of a wedged
/// process. Assembled by [`crate::crash`] from an allowlist of structured
/// fields ONLY — no transcript text can reach a row (see that module's docs),
/// and nothing here is ever uploaded: the rows exist so Settings → Privacy &
/// Diagnostics can say "Yap had a problem last session" instead of the app
/// being the last thing to know.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashEvent {
    pub id: String,
    /// When the crash happened (the report's own timestamp, not the scan's).
    pub occurred_at: DateTime<Utc>,
    /// `panic` | `native` | `watchdog` — see the `crash::KIND_*` constants.
    pub kind: String,
    /// The short line the UI lists, e.g. `EXC_CRASH (SIGABRT)`.
    pub signature: String,
    /// The on-disk artifact the row was derived from: the `.ips` FILENAME for a
    /// native/watchdog report, the panic site (`src/foo.rs:12:3`) for a panic.
    /// Half of the row's identity — `(kind, source_file, occurred_at)` is
    /// UNIQUE, so re-scanning the same evidence every launch is a no-op.
    pub source_file: String,
    /// Bounded summary text (allowlisted fields only).
    pub details: String,
    /// Cleared once the user has seen it in Diagnostics; an UNacknowledged row
    /// at launch is what raises the one non-blocking toast.
    pub acknowledged: bool,
}

/// YV64 — how many crash rows the Diagnostics list and the exported summary
/// carry. Old rows past this are still in the DB; the surfaces stay readable.
pub const CRASH_EVENT_LIMIT: usize = 50;

/// YV48 — a saved `trigger phrase → expansion text` rule. The matcher lives in
/// [`crate::snippets`]; this is only its storage + UI shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub id: String,
    /// The spoken phrase that fires the expansion.
    pub trigger: String,
    /// What it expands to (multi-line allowed).
    pub expansion: String,
    /// Disabled snippets stay in the list but never match.
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchNote {
    pub id: String,
    pub title: String,
    pub body: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Insights {
    pub total_words: i64,
    pub total_sessions: i64,
    pub words_today: i64,
    pub sessions_today: i64,
    /// Words per minute from speech_seconds only (rows with speech > 0.05s).
    pub avg_wpm: f64,
    pub streak_days: i64,
    pub longest_streak: i64,
    pub words_last_7: Vec<DayCount>,
    pub top_apps: Vec<AppCount>,
    pub avg_asr_seconds: f64,
    /// p50 release→clipboard ms (0 if no measured rows yet).
    #[serde(default)]
    pub p50_pipeline_ms: i64,
    /// p95 release→clipboard ms.
    #[serde(default)]
    pub p95_pipeline_ms: i64,
    /// Sum of speech_seconds used in WPM (hygiene / debug).
    #[serde(default)]
    pub speech_seconds_total: f64,
    /// Sessions that have real speech_seconds (eligible for WPM).
    #[serde(default)]
    pub wpm_sample_sessions: i64,
    /// YV94 / finding #29 — the Meetings strip. The app has no telemetry by
    /// design, so this local rollup is the ONLY signal that the Notetaker is
    /// being used at all. Defaulted so an older frontend bundle keeps parsing.
    #[serde(default)]
    pub meetings: MeetingStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCount {
    pub date: String,
    pub words: i64,
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppCount {
    pub app: String,
    pub words: i64,
    pub sessions: i64,
}

pub struct Database {
    conn: Mutex<Connection>,
}

fn word_count(text: &str) -> i64 {
    text.split_whitespace().filter(|w| !w.is_empty()).count() as i64
}

/// The row a dictation becomes — pure, no DB access. Shared by the live path
/// (`insert_transcript_at`) and the recovery path (`convert_failed_dictation`)
/// so both obey the same raw-text rule and both can be written inside a caller
/// owned transaction (YV68).
#[allow(clippy::too_many_arguments)]
fn new_transcript_entry(
    text: String,
    backend: String,
    asr_seconds: f64,
    speech_seconds: f64,
    pipeline_ms: i64,
    source_app: Option<String>,
    created_at: DateTime<Utc>,
    raw_text: Option<String>,
) -> TranscriptEntry {
    // Preserve the raw ASR transcript (YV10); default to the polished `text`
    // when the caller supplies none (raw == polished, e.g. cleanup off).
    let raw = raw_text
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| text.clone());
    TranscriptEntry {
        id: Uuid::new_v4().to_string(),
        word_count: word_count(&text),
        text,
        backend,
        asr_seconds,
        speech_seconds: speech_seconds.max(0.0),
        pipeline_ms: pipeline_ms.max(0),
        created_at,
        source_app,
        raw_text: Some(raw),
    }
}

/// Seed dictionary terms — proper nouns that deliberately WON'T auto-harvest
/// (they're plain first-capitalized words; see is_jargon_token).
const SEED_TERMS: &[&str] = &[
    "Drivia", "Wilson", "Jeisil", "Aidan", "Supabase", "Vercel", "Whisper", "Tauri", "Kokori",
];

/// Auto-harvest ONLY structurally-unambiguous jargon: an internal capital
/// (camelCase / PascalCase mid-word: RunPod, McApp), an ALL-CAPS acronym
/// (SQL, MLX, GPU), a digit (GPT-4, H2E), or `_`/`-` (snake_case, kebab-case).
///
/// We deliberately do NOT learn plain first-letter-capitalized words. Whisper
/// capitalizes every sentence start, so "The / Doing / Actually / Drivia" are
/// ~95% common-word noise — that flood was the whole dictionary-junk problem.
/// Real proper nouns come from SEED_TERMS + the user's manual dictionary entries.
fn is_jargon_token(t: &str) -> bool {
    if t.len() < 3 || t.len() > 48 {
        return false;
    }
    if t.chars().all(|c| c.is_ascii_digit()) {
        return false; // pure number
    }
    let has_sep = t.contains('_') || t.contains('-');
    let has_digit = t.chars().any(|c| c.is_ascii_digit());
    // uppercase anywhere but position 0 → mid-word cap (camelCase/PascalCase)
    let internal_cap = t.chars().skip(1).any(|c| c.is_uppercase());
    let uppers = t.chars().filter(|c| c.is_uppercase()).count();
    let has_lower = t.chars().any(|c| c.is_lowercase());
    let all_caps_acronym = uppers >= 2 && !has_lower && t.len() <= 8;
    has_sep || has_digit || internal_cap || all_caps_acronym
}

fn extract_learnable_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace() || ".,!?;:()[]{}\"'`".contains(c)) {
        let t = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
        if is_jargon_token(t) {
            out.push(t.to_string());
        }
    }
    out.sort();
    out.dedup();
    out.into_iter().take(40).collect()
}

/// Most candidates a single "Fix transcription" edit may produce.
const MAX_CORRECTIONS_PER_EDIT: usize = 20;
/// Above this many tokens per side the O(n·m) alignment below stops being worth
/// it; a dictation that long is not a "fix one word" edit anyway.
const MAX_DIFF_TOKENS: usize = 800;

/// The word part of a token — punctuation and quotes stripped from both ends,
/// but internal `_` / `-` / digits kept so `snake_case` and `GPT-4` survive.
fn bare_token(t: &str) -> &str {
    t.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
}

/// Is this word worth putting in front of the user as a dictionary candidate?
///
/// Unlike the ASR-side [`is_jargon_token`] harvest — which must reject plain
/// first-capitalized words because Whisper capitalizes every sentence start —
/// this runs on text the user TYPED, so a proper noun ("Jeisil", "Drivia") is a
/// deliberate signal and is kept. Common lowercase fixes ("teh" → "the") are
/// still dropped: they are typos, not vocabulary.
fn is_candidate_term(t: &str) -> bool {
    let n = t.chars().count();
    if !(2..=48).contains(&n) {
        return false;
    }
    if t.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let starts_upper = t.chars().next().is_some_and(char::is_uppercase);
    is_jargon_token(t) || starts_upper
}

/// Diff a transcript against the user's corrected version and extract the words
/// they actually changed (YV47).
///
/// Token-level LCS alignment: everything outside the common subsequence is a
/// run of removals paired positionally with a run of insertions. A pair is a
/// substitution (`wrong` = what ASR heard); a leftover insertion is a word the
/// user added. Comparison is on the bare token and case-SENSITIVE, so a pure
/// casing fix ("drivia" → "Drivia") still registers as a correction.
pub fn diff_corrections(original: &str, corrected: &str) -> Vec<Correction> {
    let a: Vec<&str> = original.split_whitespace().map(bare_token).collect();
    let b: Vec<&str> = corrected.split_whitespace().map(bare_token).collect();
    if a.len() > MAX_DIFF_TOKENS || b.len() > MAX_DIFF_TOKENS {
        return Vec::new();
    }

    // LCS table over tokens (rows = original, cols = corrected).
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[at(i, j)] = if a[i] == b[j] {
                lcs[at(i + 1, j + 1)] + 1
            } else {
                lcs[at(i + 1, j)].max(lcs[at(i, j + 1)])
            };
        }
    }

    // Walk the table into an edit script (equal / removed / inserted).
    enum Op<'t> {
        Eq,
        Del(&'t str),
        Ins(&'t str),
    }
    let mut script: Vec<Op> = Vec::with_capacity(n + m);
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            script.push(Op::Eq);
            i += 1;
            j += 1;
        } else if lcs[at(i + 1, j)] >= lcs[at(i, j + 1)] {
            script.push(Op::Del(a[i]));
            i += 1;
        } else {
            script.push(Op::Ins(b[j]));
            j += 1;
        }
    }
    script.extend(a[i..].iter().map(|t| Op::Del(t)));
    script.extend(b[j..].iter().map(|t| Op::Ins(t)));

    // Each maximal non-equal run is one edit: removals paired positionally with
    // insertions (substitutions), any surplus insertion being an added word.
    let mut out: Vec<Correction> = Vec::new();
    let mut removed: Vec<&str> = Vec::new();
    let mut added: Vec<&str> = Vec::new();
    let flush = |removed: &mut Vec<&str>, added: &mut Vec<&str>, out: &mut Vec<Correction>| {
        for (k, term) in added.iter().enumerate() {
            if !is_candidate_term(term) {
                continue;
            }
            let wrong = removed.get(k).copied().filter(|w| !w.is_empty());
            if wrong.is_some_and(|w| w == *term) {
                continue; // nothing actually changed for this word
            }
            out.push(Correction {
                wrong: wrong.map(str::to_string),
                term: (*term).to_string(),
            });
        }
        removed.clear();
        added.clear();
    };
    for op in &script {
        match op {
            Op::Eq => flush(&mut removed, &mut added, &mut out),
            Op::Del(t) => removed.push(t),
            Op::Ins(t) => added.push(t),
        }
    }
    flush(&mut removed, &mut added, &mut out);

    out.dedup();
    out.truncate(MAX_CORRECTIONS_PER_EDIT);
    out
}

/// Distinguishes a genuinely corrupt DB (safe to quarantine + recreate) from a
/// transient/environmental open failure (must be propagated, NEVER quarantined —
/// renaming a healthy DB aside because another instance briefly held a lock, or
/// the disk was full, is the exact data loss the rescue path exists to prevent).
enum OpenErr {
    Corrupt(String),
    Other(String),
}

impl OpenErr {
    fn classify(ctx: &str, e: rusqlite::Error) -> OpenErr {
        use rusqlite::ErrorCode;
        let corrupt = matches!(
            &e,
            rusqlite::Error::SqliteFailure(err, _)
                if matches!(err.code, ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
        );
        let msg = format!("{ctx}: {e}");
        if corrupt {
            OpenErr::Corrupt(msg)
        } else {
            OpenErr::Other(msg)
        }
    }

    fn into_string(self) -> String {
        match self {
            OpenErr::Corrupt(s) | OpenErr::Other(s) => s,
        }
    }
}

/// YV94 — walk `PRAGMA user_version` up to [`SCHEMA_VERSION`], one transaction
/// per step.
///
/// Rules this encodes, all of them a reaction to what `db.rs` did before
/// (finding #26 — `let _ = conn.execute("ALTER TABLE …")` discards the error, so
/// a failed migration was indistinguishable from a successful one):
///
///   * **Errors propagate.** A step that fails aborts the open; `Database::open`
///     then decides quarantine-vs-propagate the same way it does for any other
///     schema failure. A half-applied schema never gets written down as done.
///   * **One transaction per step**, and `PRAGMA user_version` is set INSIDE it.
///     SQLite keeps `user_version` in the database header and the write is
///     transactional, so a crash mid-migration rolls the whole step back and the
///     next launch retries it.
///   * **A newer DB is left alone.** If a future build wrote version 2 and the
///     user reopens this build, we do not "downgrade" — we log and carry on with
///     the tables we understand. Every step is additive, so that is safe.
///   * **Shipped steps are immutable.** Add an arm; never edit one.
fn run_migrations(conn: &Connection) -> Result<(), rusqlite::Error> {
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        log::warn!(
            "DB schema version {version} is newer than this build expects ({SCHEMA_VERSION}); \
             leaving it alone"
        );
        return Ok(());
    }
    while version < SCHEMA_VERSION {
        let next = version + 1;
        let sql = match next {
            1 => meetings::MIGRATION_1_MEETINGS,
            2 => meetings::MIGRATION_2_MEETING_DIAGNOSTICS,
            3 => meetings::MIGRATION_3_TWO_TRACK,
            4 => meetings::MIGRATION_4_MEETING_KIND,
            5 => meetings::MIGRATION_5_SPEAKER_PROFILES,
            // Unreachable while SCHEMA_VERSION and this match are edited
            // together, which is the point of failing loudly if they are not.
            other => {
                log::error!("no migration defined for schema version {other}");
                break;
            }
        };
        log::info!("applying DB migration {next}");
        // `PRAGMA user_version` takes no bind parameter, hence the format!; `next`
        // is a local integer, never user input.
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE;\n{sql}\nPRAGMA user_version = {next};\nCOMMIT;"
        ))
        .inspect_err(|_| {
            // BEGIN succeeded but a later statement failed: leave no open tx
            // behind for the rest of the session.
            let _ = conn.execute_batch("ROLLBACK;");
        })?;
        version = next;
    }
    Ok(())
}

impl Database {
    /// Open the DB, recovering ONLY from genuine corruption: on a corrupt/unreadable
    /// file the bad files are quarantined (kept for manual recovery) and a fresh DB
    /// is created. Transient failures (another instance holding a lock, disk full,
    /// I/O, permissions) are propagated — a healthy DB is never renamed aside. This
    /// is the "rescue" path.
    pub fn open(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        match Self::open_inner(&path) {
            Ok(db) => Ok(db),
            Err(OpenErr::Corrupt(e)) => {
                log::error!("DB corrupt ({e}); quarantining and recreating");
                Self::quarantine(&path);
                Self::open_inner(&path).map_err(OpenErr::into_string)
            }
            // BUSY / disk-full / I/O / permission — do NOT rename a valid DB aside.
            Err(OpenErr::Other(e)) => Err(e),
        }
    }

    /// Last-resort in-memory database, used only when the on-disk DB cannot be
    /// opened even after a retry (disk full / locked / permissions). Keeps the
    /// app usable for the current session; transcripts are NOT persisted across
    /// restarts. The schema is created the same way as the on-disk path.
    pub fn open_in_memory() -> Result<Self, String> {
        Self::open_inner(std::path::Path::new(":memory:")).map_err(OpenErr::into_string)
    }

    fn open_inner(path: &std::path::Path) -> Result<Self, OpenErr> {
        let conn = Connection::open(path).map_err(|e| OpenErr::classify("sqlite open", e))?;
        // busy_timeout FIRST — the journal_mode=WAL step below can itself return
        // SQLITE_BUSY under contention, and must be protected by the timeout.
        conn.execute_batch(
            "
            PRAGMA busy_timeout = 5000;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            -- YV78 privacy: SQLite's default is 0, so a DELETE only unlinks the
            -- row and leaves its bytes legible in the freed page — which the
            -- exit wal_checkpoint(TRUNCATE) then folds into the main .db. ON
            -- zeroes deleted content as it is freed, for EVERY delete path
            -- (single transcript, clear-all, purge).
            PRAGMA secure_delete = ON;
            PRAGMA temp_store = MEMORY;
            PRAGMA mmap_size = 268435456;
            PRAGMA wal_autocheckpoint = 400;
            ",
        )
        .map_err(|e| OpenErr::classify("pragma", e))?;

        // Integrity gate — a corrupt/garbage file returns non-'ok' or SQLITE_NOTADB;
        // classify so ONLY corruption triggers quarantine, never a transient lock.
        let check: String = conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(|e| OpenErr::classify("quick_check", e))?;
        if check != "ok" {
            return Err(OpenErr::Corrupt(format!("integrity check failed: {check}")));
        }

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS transcripts (
              id TEXT PRIMARY KEY,
              text TEXT NOT NULL,
              backend TEXT NOT NULL DEFAULT 'native',
              asr_seconds REAL NOT NULL DEFAULT 0,
              word_count INTEGER NOT NULL DEFAULT 0,
              source_app TEXT,
              created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_transcripts_created
              ON transcripts(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_transcripts_words
              ON transcripts(word_count);

            CREATE VIRTUAL TABLE IF NOT EXISTS transcripts_fts
              USING fts5(text, content='transcripts', content_rowid='rowid');

            CREATE TRIGGER IF NOT EXISTS transcripts_ai AFTER INSERT ON transcripts BEGIN
              INSERT INTO transcripts_fts(rowid, text) VALUES (new.rowid, new.text);
            END;
            CREATE TRIGGER IF NOT EXISTS transcripts_ad AFTER DELETE ON transcripts BEGIN
              INSERT INTO transcripts_fts(transcripts_fts, rowid, text)
                VALUES('delete', old.rowid, old.text);
            END;
            CREATE TRIGGER IF NOT EXISTS transcripts_au AFTER UPDATE ON transcripts BEGIN
              INSERT INTO transcripts_fts(transcripts_fts, rowid, text)
                VALUES('delete', old.rowid, old.text);
              INSERT INTO transcripts_fts(rowid, text) VALUES (new.rowid, new.text);
            END;

            CREATE TABLE IF NOT EXISTS dictionary (
              id TEXT PRIMARY KEY,
              term TEXT NOT NULL UNIQUE COLLATE NOCASE,
              preferred TEXT,
              hits INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_dictionary_term ON dictionary(term);

            CREATE TABLE IF NOT EXISTS dict_candidates (
              id TEXT PRIMARY KEY,
              term TEXT NOT NULL UNIQUE COLLATE NOCASE,
              wrong TEXT,
              use_count INTEGER NOT NULL DEFAULT 1,
              dismissed INTEGER NOT NULL DEFAULT 0,
              created_at TEXT NOT NULL
            );

            -- YV52 dictation recovery: takes whose transcription failed, with
            -- the preserved WAV so ASR can be re-run on the same audio.
            CREATE TABLE IF NOT EXISTS failed_dictations (
              id TEXT PRIMARY KEY,
              wav_path TEXT NOT NULL,
              speech_seconds REAL NOT NULL DEFAULT 0,
              error TEXT NOT NULL,
              source_app TEXT,
              created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_failed_dictations_created
              ON failed_dictations(created_at DESC);
            -- YV48 snippets: trigger phrase → expansion text. `trigger` is a
            -- SQL keyword, hence the column name; the struct field is `trigger`.
            CREATE TABLE IF NOT EXISTS snippets (
              id TEXT PRIMARY KEY,
              trigger_phrase TEXT NOT NULL UNIQUE COLLATE NOCASE,
              expansion TEXT NOT NULL,
              enabled INTEGER NOT NULL DEFAULT 1,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS scratchpad (
              id TEXT PRIMARY KEY,
              title TEXT NOT NULL DEFAULT 'Note',
              body TEXT NOT NULL DEFAULT '',
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS daily_stats (
              day TEXT PRIMARY KEY,
              words INTEGER NOT NULL DEFAULT 0,
              sessions INTEGER NOT NULL DEFAULT 0,
              asr_ms INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS settings_kv (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );

            -- YV64 crash telemetry: one row per crash Yap has evidence of,
            -- written by `crash::ingest` at startup from macOS .ips reports and
            -- the panic hook's own log lines. LOCAL ONLY — nothing in here is
            -- ever uploaded, and no transcript text can reach it (the row is
            -- built from an allowlist of structured fields). The UNIQUE key is
            -- what makes re-scanning the same evidence on every launch a no-op.
            CREATE TABLE IF NOT EXISTS crash_events (
              id TEXT PRIMARY KEY,
              occurred_at TEXT NOT NULL,
              kind TEXT NOT NULL,
              signature TEXT NOT NULL,
              source_file TEXT NOT NULL,
              details TEXT NOT NULL DEFAULT '',
              acknowledged INTEGER NOT NULL DEFAULT 0,
              UNIQUE (kind, source_file, occurred_at)
            );
            CREATE INDEX IF NOT EXISTS idx_crash_events_occurred
              ON crash_events(occurred_at DESC);

            -- YP2 licensing: the CORROBORATING home for the trial's timestamps
            -- (`license.rs` owns the primary copy in license.json). Two
            -- independent stores is what stops deleting the license file from
            -- handing out a second 14-day trial — whichever survives is
            -- authoritative, and the earlier start always wins.
            --
            -- It is deliberately its OWN table, not a row in settings_kv: this
            -- must not be reachable by anything that resets, exports or clears
            -- settings, and a reader should be able to see at a glance that
            -- nothing here is a decided `licensed` flag. It holds two integers.
            CREATE TABLE IF NOT EXISTS license_state (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            ",
        )
        .map_err(|e| OpenErr::classify("schema", e))?;

        // YV94 — the versioned ladder. Everything ABOVE this point is the
        // pre-ladder baseline (`CREATE TABLE IF NOT EXISTS` + the error-swallowing
        // `ALTER TABLE`s below), which is left exactly as it is: it has shipped to
        // every install since v0.1 and rewriting it as migration 0 would mean
        // asserting, without proof, what shape those DBs are actually in.
        //
        // The ladder starts at the meeting tables (finding #26) because that is
        // the first schema in this app with FTS5 sync triggers and a foreign key,
        // and because nothing needs backfilling yet — a fresh install and a
        // three-year-old one both go 0 → 1 the same way.
        run_migrations(&conn).map_err(|e| OpenErr::classify("migrate", e))?;

        // Migrations (idempotent)
        let _ = conn.execute(
            "ALTER TABLE transcripts ADD COLUMN speech_seconds REAL NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE transcripts ADD COLUMN pipeline_ms INTEGER NOT NULL DEFAULT 0",
            [],
        );
        // YV10: store the raw ASR transcript alongside the polished `text`. Nullable
        // so legacy rows (written before the cleanup pipeline) stay valid as NULL.
        let _ = conn.execute("ALTER TABLE transcripts ADD COLUMN raw_text TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE daily_stats ADD COLUMN speech_ms INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE daily_stats ADD COLUMN pipeline_ms INTEGER NOT NULL DEFAULT 0",
            [],
        );
        // Provenance so the junk-purge NEVER deletes manual or seed terms — only
        // 'harvest'. (Manual term-only adds are also preferred IS NULL, so preferred
        // alone can't distinguish them.)
        let _ = conn.execute(
            "ALTER TABLE dictionary ADD COLUMN source TEXT NOT NULL DEFAULT 'harvest'",
            [],
        );
        // YV47: "always bias" flag. Starred terms head the ranking, survive the
        // purge below, and are the last thing dropped from the decoder prompt.
        let _ = conn.execute(
            "ALTER TABLE dictionary ADD COLUMN starred INTEGER NOT NULL DEFAULT 0",
            [],
        );

        // Seed default dictionary terms useful for Wilson
        {
            let now = Utc::now().to_rfc3339();
            for term in SEED_TERMS {
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO dictionary (id, term, preferred, hits, created_at, source)
                     VALUES (?1, ?2, NULL, 0, ?3, 'seed')",
                    params![Uuid::new_v4().to_string(), term, now],
                );
                // Existing rows (pre-source-column) defaulted to 'harvest' — promote seeds.
                let _ = conn.execute(
                    "UPDATE dictionary SET source = 'seed' WHERE term = ?1 COLLATE NOCASE",
                    params![term],
                );
            }
        }

        // Purge harvest junk on every open: (a) old self-referential rows
        // (preferred == term, the old harvest bug) and (b) auto-harvested
        // (preferred IS NULL) non-seed terms that aren't structurally jargon
        // (The / Doing / Keeps / ...). Seeds and real jargon are preserved.
        let _ = conn.execute(
            "DELETE FROM dictionary WHERE preferred IS NOT NULL AND preferred = term",
            [],
        );
        {
            let seeds: std::collections::HashSet<String> =
                SEED_TERMS.iter().map(|s| s.to_lowercase()).collect();
            let mut junk: Vec<String> = Vec::new();
            // Only unstarred 'harvest' rows are purge candidates — manual, seed
            // and starred (YV47 always-bias) rows are safe.
            if let Ok(mut stmt) = conn.prepare(
                "SELECT id, term FROM dictionary
                 WHERE source = 'harvest' AND preferred IS NULL AND starred = 0",
            ) {
                if let Ok(rows) = stmt.query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                }) {
                    for (id, term) in rows.flatten() {
                        if !seeds.contains(&term.to_lowercase()) && !is_jargon_token(&term) {
                            junk.push(id);
                        }
                    }
                }
            }
            for id in junk {
                let _ = conn.execute("DELETE FROM dictionary WHERE id = ?1", params![id]);
            }
        }

        let db = Self {
            conn: Mutex::new(conn),
        };
        // Always rebuild daily rollups from transcripts (source of truth)
        if let Err(e) = db.recompute_daily_stats() {
            log::warn!("daily_stats recompute on open: {e}");
        }
        Ok(db)
    }

    /// Rename a corrupt DB (+ its -wal / -shm) aside, timestamped, for manual
    /// recovery. Best-effort — failures are logged, not fatal.
    fn quarantine(path: &std::path::Path) {
        // Sub-second component so a fast fail→relaunch loop can't overwrite an
        // earlier recovery copy (fs::rename replaces the target on POSIX).
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let stamp = format!("{}-{nanos:09}", Local::now().format("%Y%m%d-%H%M%S"));
        for suffix in ["", "-wal", "-shm"] {
            let mut from = path.as_os_str().to_os_string();
            from.push(suffix);
            let from = PathBuf::from(from);
            if from.exists() {
                let mut to = from.clone().into_os_string();
                to.push(format!(".corrupt-{stamp}"));
                match std::fs::rename(&from, PathBuf::from(to)) {
                    Ok(()) => log::warn!("quarantined {}", from.display()),
                    Err(e) => log::warn!("quarantine {} failed: {e}", from.display()),
                }
            }
        }
    }

    /// Flush the WAL back into the main DB and truncate it. Call on app exit so
    /// the -wal doesn't grow unbounded and the .db isn't left a deceptive stub.
    pub fn checkpoint(&self) {
        if let Ok(conn) = self.conn.lock() {
            if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
                log::warn!("wal_checkpoint failed: {e}");
            }
        }
    }

    /// Rebuild `daily_stats` from `transcripts`. Call after insert/delete/clear.
    ///
    /// speech_ms only sums real speech_seconds (> 0.05). Never substitutes asr_seconds.
    ///
    /// The wipe-and-rebuild runs in ONE transaction: a failure part-way through
    /// must leave the previous rollup standing, never an empty `daily_stats`.
    /// Callers now only warn on failure (see `insert_transcript_at`), so a torn
    /// rebuild would otherwise silently show the user zero words for the day.
    pub fn recompute_daily_stats(&self) -> Result<(), String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM daily_stats", [])
            .map_err(|e| e.to_string())?;
        // Group by local calendar day derived from created_at (ISO stored as UTC).
        let mut stmt = tx
            .prepare(
                "SELECT created_at, word_count, asr_seconds,
                        COALESCE(speech_seconds,0), COALESCE(pipeline_ms,0)
                 FROM transcripts",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, f64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })
            .map_err(|e| e.to_string())?;

        use std::collections::HashMap;
        struct Acc {
            words: i64,
            sessions: i64,
            asr_ms: i64,
            speech_ms: i64,
            pipeline_ms: i64,
        }
        let mut map: HashMap<String, Acc> = HashMap::new();
        for row in rows {
            let (created, words, asr_s, speech_s, pipe_ms) = row.map_err(|e| e.to_string())?;
            let day = parse_dt(created)
                .with_timezone(&Local)
                .date_naive()
                .to_string();
            let e = map.entry(day).or_insert(Acc {
                words: 0,
                sessions: 0,
                asr_ms: 0,
                speech_ms: 0,
                pipeline_ms: 0,
            });
            e.words += words;
            e.sessions += 1;
            e.asr_ms += (asr_s * 1000.0).round() as i64;
            // Honest: only count measured speech. Legacy 0 rows do not pollute WPM.
            if speech_s > 0.05 {
                e.speech_ms += (speech_s * 1000.0).round() as i64;
            }
            e.pipeline_ms += pipe_ms.max(0);
        }
        drop(stmt);

        for (day, a) in map {
            tx.execute(
                "INSERT INTO daily_stats (day, words, sessions, asr_ms, speech_ms, pipeline_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(day) DO UPDATE SET
                   words = excluded.words,
                   sessions = excluded.sessions,
                   asr_ms = excluded.asr_ms,
                   speech_ms = excluded.speech_ms,
                   pipeline_ms = excluded.pipeline_ms",
                params![day, a.words, a.sessions, a.asr_ms, a.speech_ms, a.pipeline_ms],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn insert_transcript(
        &self,
        text: String,
        backend: String,
        asr_seconds: f64,
        speech_seconds: f64,
        pipeline_ms: i64,
        source_app: Option<String>,
    ) -> Result<TranscriptEntry, String> {
        self.insert_transcript_at(
            text,
            backend,
            asr_seconds,
            speech_seconds,
            pipeline_ms,
            source_app,
            Utc::now(),
            None,
        )
    }

    /// Same as `insert_transcript` but with an explicit timestamp. Production
    /// always uses `Utc::now()`; the timestamp seam exists so the analytics tests
    /// can place sessions on specific calendar days (streaks, words-today).
    #[allow(clippy::too_many_arguments)]
    pub fn insert_transcript_at(
        &self,
        text: String,
        backend: String,
        asr_seconds: f64,
        speech_seconds: f64,
        pipeline_ms: i64,
        source_app: Option<String>,
        created_at: DateTime<Utc>,
        raw_text: Option<String>,
    ) -> Result<TranscriptEntry, String> {
        let entry = new_transcript_entry(
            text,
            backend,
            asr_seconds,
            speech_seconds,
            pipeline_ms,
            source_app,
            created_at,
            raw_text,
        );
        {
            // YV68 — ONE transaction per dictation: the dictionary harvest and
            // the transcript row land together or not at all. The lock is taken
            // once here, so everything inside must use the `_tx` helpers — a
            // method that re-locks `self.conn` would deadlock.
            let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            let _ = self.learn_from_transcript_tx(&tx, &entry.text);
            self.insert_transcript_row_tx(&tx, &entry)?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        // Source-of-truth rollup (never trust incremental counters alone).
        // YV68 — the transcript is already durable and its text is already in
        // the user's app, so a rollup failure is a STATS problem, never a lost
        // dictation. Propagating it here made the caller park a recovery WAV and
        // toast "Failed" for a take that was saved — and a later Retry then
        // wrote a SECOND row for the same utterance.
        if let Err(e) = self.recompute_daily_stats() {
            log::warn!("daily rollup: {e}");
        }
        Ok(entry)
    }

    /// Write one transcript row on an ALREADY-OPEN connection/transaction. The
    /// caller owns the lock, the transaction and the `daily_stats` rollup, so a
    /// dictation can be committed as a single unit with whatever else it must
    /// be atomic with (the harvest; the failed-take row it replaces).
    fn insert_transcript_row_tx(
        &self,
        conn: &Connection,
        entry: &TranscriptEntry,
    ) -> Result<(), String> {
        conn.execute(
            "INSERT INTO transcripts
             (id, text, backend, asr_seconds, speech_seconds, pipeline_ms,
              word_count, source_app, created_at, raw_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entry.id,
                entry.text,
                entry.backend,
                entry.asr_seconds,
                entry.speech_seconds,
                entry.pipeline_ms,
                entry.word_count,
                entry.source_app,
                entry.created_at.to_rfc3339(),
                entry.raw_text,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Local "fine-tune" signal: learn coding jargon / proper nouns from transcripts.
    /// Stores high-value tokens in the dictionary table (hits++). Applied on next ASR polish.
    ///
    /// Standalone entry point: takes the lock itself. The dictation paths do NOT
    /// call this — they harvest inside their own transaction (YV68) — so today
    /// its only callers are the dictionary tests.
    #[allow(dead_code)]
    pub fn learn_from_transcript(&self, text: &str) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        self.learn_from_transcript_tx(&conn, text)
    }

    /// The harvest itself, on a connection the caller already holds. Takes no
    /// lock, so it is safe to call inside an open transaction (YV68).
    fn learn_from_transcript_tx(&self, conn: &Connection, text: &str) -> Result<usize, String> {
        let mut learned = 0usize;
        let now = Utc::now().to_rfc3339();

        for token in extract_learnable_tokens(text) {
            let n = conn
                .execute(
                    // Harvested tokens are CANDIDATES (preferred=NULL), not self-
                    // rewrites. They bias recognition via initial_prompt (by `term`
                    // + `hits`); only a real correction sets a distinct `preferred`.
                    // (The old `?2,?2` made preferred==term → apply_dictionary a no-op.)
                    "INSERT INTO dictionary (id, term, preferred, hits, created_at, source)
                     VALUES (?1, ?2, NULL, 1, ?3, 'harvest')
                     ON CONFLICT(term) DO UPDATE SET hits = hits + 1",
                    params![Uuid::new_v4().to_string(), token, now],
                )
                .map_err(|e| e.to_string())?;
            learned += n;
        }
        Ok(learned)
    }

    // ── YP2 licensing: the corroborating trial rows ──
    //
    // Read on every entitlement check and written whenever the trial's start or
    // the clock floor moves. Both are plain integers (ms since epoch); NOTHING
    // here decides whether the app is licensed — that answer is recomputed from
    // the Ed25519 signature every time it is asked for (see `license.rs`).

    /// Read a license bookkeeping value. A missing table or row is `None`, never
    /// an error: licensing must never be the reason the app fails to open.
    pub fn license_state_get(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        conn.query_row(
            "SELECT value FROM license_state WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .ok()
    }

    /// Write a license bookkeeping value.
    pub fn license_state_set(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO license_state (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    pub fn list_transcripts(&self, limit: i64, query: Option<String>) -> Result<Vec<TranscriptEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let q = query.unwrap_or_default().trim().to_string();

        let mut out = Vec::new();
        if q.is_empty() {
            let mut stmt = conn
                .prepare(
                    "SELECT id, text, backend, asr_seconds, COALESCE(speech_seconds,0),
                            COALESCE(pipeline_ms,0), word_count, source_app, created_at, raw_text
                     FROM transcripts ORDER BY created_at DESC LIMIT ?1",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![limit], map_transcript)
                .map_err(|e| e.to_string())?;
            for r in rows {
                out.push(r.map_err(|e| e.to_string())?);
            }
        } else {
            // FTS5: prefix-friendly query
            let fts = q
                .split_whitespace()
                .map(|t| format!("\"{}\"*", t.replace('"', "")))
                .collect::<Vec<_>>()
                .join(" ");
            let mut stmt = conn
                .prepare(
                    "SELECT t.id, t.text, t.backend, t.asr_seconds, COALESCE(t.speech_seconds,0),
                            COALESCE(t.pipeline_ms,0), t.word_count, t.source_app, t.created_at, t.raw_text
                     FROM transcripts_fts f
                     JOIN transcripts t ON t.rowid = f.rowid
                     WHERE transcripts_fts MATCH ?1
                     ORDER BY t.created_at DESC
                     LIMIT ?2",
                )
                .map_err(|e| e.to_string())?;
            let rows = match stmt.query_map(params![fts, limit], map_transcript) {
                Ok(r) => r,
                Err(_) => {
                    // Fallback substring if FTS query invalid
                    let mut stmt2 = conn
                        .prepare(
                            "SELECT id, text, backend, asr_seconds, COALESCE(speech_seconds,0),
                                    COALESCE(pipeline_ms,0), word_count, source_app, created_at, raw_text
                             FROM transcripts
                             WHERE text LIKE ?1
                             ORDER BY created_at DESC LIMIT ?2",
                        )
                        .map_err(|e| e.to_string())?;
                    let like = format!("%{q}%");
                    let rows2 = stmt2
                        .query_map(params![like, limit], map_transcript)
                        .map_err(|e| e.to_string())?;
                    for r in rows2 {
                        out.push(r.map_err(|e| e.to_string())?);
                    }
                    return Ok(out);
                }
            };
            for r in rows {
                out.push(r.map_err(|e| e.to_string())?);
            }
        }
        Ok(out)
    }

    pub fn delete_transcript(&self, id: &str) -> Result<(), String> {
        {
            let conn = self.conn.lock().map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM transcripts WHERE id = ?1", params![id])
                .map_err(|e| e.to_string())?;
        }
        self.recompute_daily_stats()
    }

    /// YV78 — "Clear all transcript history" must DESTROY the words, not just
    /// unlink the rows.
    ///
    /// Everything on disk that physically holds transcript text, verified
    /// against the schema above:
    ///   * `transcripts` — `text` and `raw_text`.
    ///   * `transcripts_fts` — external-content FTS5 over `transcripts`; its
    ///     shadow tables hold the tokens themselves.
    ///   * `dictionary` — the harvest (`learn_from_transcript_tx`) copies rare
    ///     tokens VERBATIM out of dictations. Only rows that are still purely
    ///     machine-written are dropped: `source = 'harvest' AND preferred IS
    ///     NULL AND starred = 0`, the same "never user-touched" predicate the
    ///     junk-purge in `open_inner` uses. Seed, manual, corrected and starred
    ///     terms are the user's own vocabulary and survive.
    ///   * `daily_stats` — counts only, but it IS the history rollup, so it
    ///     goes with the rows it was computed from.
    ///
    /// Deliberately NOT touched: `failed_dictations` (YV52) stores a WAV path
    /// and the engine's error string, never transcript text, and its clips are
    /// the user's only copy of a take that was never transcribed;
    /// `dict_candidates`, `snippets` and `scratchpad` are text the user typed,
    /// with their own delete paths; `crash_events` is built from an allowlist
    /// of structured fields.
    ///
    /// The DELETEs run in ONE transaction so "erase everything" is
    /// all-or-nothing. Then, after the commit and still under the lock:
    ///   1. rebuild the FTS index from the (now empty) content table,
    ///   2. fold the WAL into the main DB and truncate it, so the pre-clear
    ///      frames stop being readable,
    ///   3. VACUUM — rewrites the file without its free pages. This is what
    ///      scrubs residue left by copies installed BEFORE `secure_delete`
    ///      existed. VACUUM cannot run inside a transaction, hence the order.
    ///   4. checkpoint again — VACUUM's clean image lands in the WAL, so
    ///      without this the stale pages survive in the .db until app exit.
    pub fn clear_transcripts(&self) -> Result<(), String> {
        {
            let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM transcripts", [])
                .map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM dictionary
                 WHERE source = 'harvest' AND preferred IS NULL AND starred = 0",
                [],
            )
            .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM daily_stats", [])
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;

            conn.execute_batch(
                "
                INSERT INTO transcripts_fts(transcripts_fts) VALUES('rebuild');
                PRAGMA wal_checkpoint(TRUNCATE);
                VACUUM;
                PRAGMA wal_checkpoint(TRUNCATE);
                ",
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    // --- YV94: meetings ---------------------------------------------------
    //
    // Everything below reads or writes the two tables migration 1 introduces.
    // The invariants worth stating once, here, rather than in every method:
    //
    //   * `meeting_segments_fts` is an EXTERNAL-CONTENT index. It is never
    //     written directly — the ai/ad/au triggers own it. Any code path that
    //     mutates `meeting_segments.text` outside SQL would desync it.
    //   * Deleting a meeting deletes its segments EXPLICITLY first, then the
    //     row. The `ON DELETE CASCADE` stays as a net, but the explicit delete
    //     is what guarantees the `ad` trigger runs and the FTS shadow rows go
    //     with the words.
    //   * `secure_delete = ON` (set in `open_inner`) means those deletes
    //     overwrite the freed pages rather than just unlinking them. That is
    //     the whole point for a privacy feature, and it is why the delete path
    //     must never run on the UI thread — Tauri already runs non-async
    //     commands on a worker, and `delete_meeting` is called from one.

    /// The applied schema version. Exposed so the migration test can assert it
    /// rather than reaching into the connection.
    pub fn schema_version(&self) -> Result<i64, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| e.to_string())
    }

    // ── YV96 · `settings_kv`, and the one key the consent notice needs ──
    //
    // `settings_kv (key TEXT PRIMARY KEY, value TEXT NOT NULL)` has existed in
    // the baseline schema since v0.1 with no reader and no writer anywhere in
    // the tree — every user-facing setting lives in the Tauri store. The
    // one-time meeting notice is the first thing to use it, which is exactly
    // what finding #13 asked for: an app-wide acknowledgement belongs in one
    // app-wide row, not in a `meetings.consent_ack` column that every future
    // row would carry and nothing would ever read.

    /// Read one `settings_kv` value. A missing row is `None`, never an error.
    pub fn setting_get(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().ok()?;
        conn.query_row(
            "SELECT value FROM settings_kv WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .ok()
    }

    /// Write one `settings_kv` value, overwriting whatever was there.
    pub fn setting_set(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO settings_kv (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// Has the one-time meeting-capture notice been shown and closed yet?
    ///
    /// Never fails: a consent notice that errors is a notice that either blocks
    /// a recording or shows itself forever, and both are worse than the answer
    /// "assume it still needs showing".
    pub fn meeting_consent(&self) -> MeetingConsent {
        MeetingConsent::from_ack(self.setting_get(meetings::CONSENT_NOTICE_KEY))
    }

    /// Record that the notice was shown and closed — by acknowledging it OR by
    /// dismissing it, because a one-time notice is about display, not assent
    /// (the liability sits with the user per TERMS.md, and Yap adjudicates
    /// nothing).
    ///
    /// Idempotent, and the FIRST timestamp wins: `INSERT … ON CONFLICT DO
    /// NOTHING` means a second close cannot move the date the user first saw
    /// this text.
    pub fn acknowledge_meeting_consent(&self) -> Result<MeetingConsent, String> {
        {
            let conn = self.conn.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO settings_kv (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO NOTHING",
                params![meetings::CONSENT_NOTICE_KEY, Utc::now().to_rfc3339()],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(self.meeting_consent())
    }

    // ── YV102 · the system-audio setup step's own row ──────────────────────

    /// What Yap currently believes about system-audio permission on this Mac.
    ///
    /// Reads its OWN key. `meeting_consent()` above reads a different one, and
    /// `tests/settings_kv.rs` asserts that acking either leaves the other
    /// exactly where it was — the two facts are independent and the storage has
    /// to keep them that way.
    pub fn system_audio_setup(&self) -> meetings::SystemAudioSetup {
        meetings::SystemAudioSetup::from_row(
            self.setting_get(meetings::SYSTEM_AUDIO_SETUP_ACK_KEY),
        )
    }

    /// Record what the setup step (or a later meeting's discriminator) found.
    ///
    /// **Overwrites**, where `acknowledge_meeting_consent` deliberately does
    /// not. The consent row answers "when did this user first see the notice",
    /// which cannot change; this row answers "what is the permission state
    /// now", which can — a user who allows the tap after a denial must not be
    /// stuck looking at the denied banner forever.
    pub fn record_system_audio_setup(
        &self,
        verdict: meetings::SetupVerdict,
    ) -> Result<meetings::SystemAudioSetup, String> {
        self.setting_set(
            meetings::SYSTEM_AUDIO_SETUP_ACK_KEY,
            &meetings::SystemAudioSetup::encode(verdict, &Utc::now().to_rfc3339()),
        )?;
        Ok(self.system_audio_setup())
    }

    /// Open a new meeting row in state `recording`.
    pub fn create_meeting(&self, title: &str, source: &str) -> Result<Meeting, String> {
        // YV125 — the SKIP path, spelled out. A caller that does not name a
        // kind has not silently chosen one: `Unknown` is the answer "I did not
        // say", and `meetings::diarization_target` treats it as the general
        // case (cluster Track A) rather than as a call.
        self.create_meeting_with_kind(title, source, meetings::MeetingKind::Unknown)
    }

    /// The same row, with the kind the user picked at the start-of-meeting
    /// surface (YV125). Stored at INSERT rather than patched afterwards: the
    /// branch it feeds is read by everything downstream of the recording, and a
    /// row that exists for a moment without a kind is a row that can be read
    /// during that moment.
    pub fn create_meeting_with_kind(
        &self,
        title: &str,
        source: &str,
        kind: meetings::MeetingKind,
    ) -> Result<Meeting, String> {
        let now = Utc::now();
        let title = {
            let t = title.trim();
            if t.is_empty() { "Meeting" } else { t }.to_string()
        };
        let row = Meeting {
            id: Uuid::new_v4().to_string(),
            title,
            source: source.to_string(),
            kind: kind.as_str().to_string(),
            started_at: now,
            ended_at: None,
            duration_seconds: 0.0,
            state: meetings::MeetingState::Recording.as_str().to_string(),
            error: None,
            processed_through_seconds: 0.0,
            audio_kept: true,
            mic_wav_path: None,
            sys_wav_path: None,
            tap_rebuilds: None,
            summary: None,
            summary_model: None,
            created_at: now,
            segment_count: 0,
            diagnostics: None,
        };
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO meetings
             (id, title, source, kind, started_at, state, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                row.id,
                row.title,
                row.source,
                row.kind,
                row.started_at.to_rfc3339(),
                row.state,
                row.created_at.to_rfc3339(),
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(row)
    }

    /// Rename a meeting. An all-whitespace title is refused rather than stored,
    /// so the list can never render a blank row.
    pub fn rename_meeting(&self, id: &str, title: &str) -> Result<(), String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("a meeting needs a title".into());
        }
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE meetings SET title = ?2 WHERE id = ?1",
            params![id, title],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// Move a meeting through its lifecycle. `error` is written on every call so
    /// a recovered meeting does not keep a stale failure string.
    pub fn set_meeting_state(
        &self,
        id: &str,
        state: meetings::MeetingState,
        error: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE meetings SET state = ?2, error = ?3 WHERE id = ?1",
            params![id, state.as_str(), error],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// YV93's resume anchor. Monotonic by construction: a lower value than the
    /// one already stored is ignored, so a retried chunk cannot rewind progress.
    pub fn set_meeting_progress(&self, id: &str, processed_through: f64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE meetings SET processed_through_seconds = MAX(processed_through_seconds, ?2)
             WHERE id = ?1",
            params![id, processed_through.max(0.0)],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// Close out capture: end time, duration, and the WAV the retention sweep
    /// will later purge.
    pub fn finish_meeting(
        &self,
        id: &str,
        duration_seconds: f64,
        mic_wav_path: Option<&Path>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let wav = mic_wav_path.map(|p| p.to_string_lossy().to_string());
        conn.execute(
            "UPDATE meetings
             SET ended_at = ?2, duration_seconds = ?3, mic_wav_path = ?4,
                 audio_kept = CASE WHEN ?4 IS NULL THEN 0 ELSE 1 END
             WHERE id = ?1",
            params![id, Utc::now().to_rfc3339(), duration_seconds.max(0.0), wav],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// YV106 — record the system-audio track's WAV for a two-track meeting.
    ///
    /// Separate from [`Self::finish_meeting`] rather than a fifth parameter on
    /// it, for the reason `set_meeting_diagnostics` is separate: `finish_meeting`
    /// is 22-A's shipped signature with a live caller, and a mic-only meeting
    /// must keep taking exactly the path it takes today. A meeting with no tap
    /// simply never calls this.
    ///
    /// `audio_kept` is deliberately NOT touched here: it tracks the meeting's
    /// audio as one thing, and it is `finish_meeting`'s to set.
    pub fn set_meeting_sys_wav_path(&self, id: &str, path: Option<&Path>) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let wav = path.map(|p| p.to_string_lossy().to_string());
        conn.execute(
            "UPDATE meetings SET sys_wav_path = ?2 WHERE id = ?1",
            params![id, wav],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// YV104 / YV106 — persist the tap's rebuild log onto the meeting row.
    ///
    /// Called once, at stop, with `syscapture::TapRebuildLog::to_json`. A
    /// meeting that never needed a rebuild never calls this, so the column
    /// stays `NULL` and "no rebuilds" is distinguishable from "an empty log was
    /// written", which is the distinction a diagnosis a week later turns on.
    pub fn set_meeting_tap_rebuilds(&self, id: &str, json: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE meetings SET tap_rebuilds = ?2 WHERE id = ?1",
            params![id, json],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// YV95 / OS-12 — write (or rewrite) the session's diagnostics blob.
    ///
    /// Called twice per meeting: once at start with the preflight readings, so a
    /// meeting that never reaches its stop path (a crash, a kill) still carries
    /// the state the machine was in when it began, and once at stop with the
    /// thermal transitions and the closing battery sample. The second call is a
    /// full overwrite rather than a merge because the controller holds the whole
    /// blob in memory — there is no partial writer to race with.
    pub fn set_meeting_diagnostics(&self, id: &str, json: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE meetings SET diagnostics = ?2 WHERE id = ?1",
            params![id, json],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// YV97 writes here. Kept separate from `set_meeting_state` so a summary can
    /// land without implying the meeting's state changed.
    pub fn set_meeting_summary(
        &self,
        id: &str,
        summary: &str,
        model: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE meetings SET summary = ?2, summary_model = ?3 WHERE id = ?1",
            params![id, summary, model],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// Append transcript segments. ONE transaction for the whole batch: YV93
    /// hands over a chunk at a time, and half a chunk in the index would be a
    /// silently wrong search result rather than a visible failure.
    pub fn append_meeting_segments(
        &self,
        meeting_id: &str,
        segments: &[NewMeetingSegment],
    ) -> Result<usize, String> {
        if segments.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let now = Utc::now().to_rfc3339();
        let mut written = 0usize;
        for seg in segments {
            tx.execute(
                "INSERT INTO meeting_segments
                 (id, meeting_id, start_seconds, end_seconds, text, confidence, created_at, track)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    Uuid::new_v4().to_string(),
                    meeting_id,
                    seg.start_seconds,
                    seg.end_seconds,
                    seg.text,
                    seg.confidence,
                    now,
                    seg.track,
                ],
            )
            .map_err(|e| e.to_string())?;
            written += 1;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(written)
    }

    /// Meetings newest-first. A non-empty `query` searches segment text through
    /// FTS5 **and** the title, so "yesterday's standup" is findable both by what
    /// it was called and by what was said in it.
    pub fn list_meetings(&self, limit: i64, query: Option<String>) -> Result<Vec<Meeting>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let q = query.unwrap_or_default().trim().to_string();
        let mut out = Vec::new();

        if q.is_empty() {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {MEETING_COLS} FROM meetings m
                     ORDER BY m.started_at DESC LIMIT ?1"
                ))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![limit], map_meeting)
                .map_err(|e| e.to_string())?;
            for r in rows {
                out.push(r.map_err(|e| e.to_string())?);
            }
            return Ok(out);
        }

        let fts = q
            .split_whitespace()
            .map(|t| format!("\"{}\"*", t.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ");
        let like = format!("%{q}%");
        let sql = format!(
            "SELECT {MEETING_COLS} FROM meetings m
             WHERE m.title LIKE ?2
                OR m.id IN (
                     SELECT s.meeting_id FROM meeting_segments_fts f
                     JOIN meeting_segments s ON s.rowid = f.rowid
                     WHERE meeting_segments_fts MATCH ?3
                   )
             ORDER BY m.started_at DESC LIMIT ?1"
        );
        // An FTS5 MATCH can reject a query string outright (a lone `*`, an
        // unbalanced quote). That must degrade to a substring search, never to
        // an error toast — same posture as `list_transcripts`.
        let mut fts_ok = false;
        {
            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
            let mapped = stmt.query_map(params![limit, like, fts], map_meeting);
            if let Ok(rows) = mapped {
                for r in rows {
                    out.push(r.map_err(|e| e.to_string())?);
                }
                fts_ok = true;
            }
        }
        if fts_ok {
            return Ok(out);
        }

        out.clear();
        let mut stmt2 = conn
            .prepare(&format!(
                "SELECT {MEETING_COLS} FROM meetings m
                 WHERE m.title LIKE ?2
                    OR m.id IN (SELECT s.meeting_id FROM meeting_segments s
                                WHERE s.text LIKE ?2)
                 ORDER BY m.started_at DESC LIMIT ?1"
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt2
            .query_map(params![limit, like], map_meeting)
            .map_err(|e| e.to_string())?;
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn get_meeting(&self, id: &str) -> Result<Option<Meeting>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            &format!("SELECT {MEETING_COLS} FROM meetings m WHERE m.id = ?1"),
            params![id],
            map_meeting,
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    /// Segments in wall-clock order — the order YV93 produced them and the only
    /// order a transcript may ever be read in.
    ///
    /// YV108 adds `track` as the tiebreak BEFORE `created_at`. Two tracks are
    /// appended in separate batches, so `created_at` for a tap segment says
    /// when transcription got to it, not when it was said: at an exact
    /// `start_seconds` tie it would order the interleave by the transcriber's
    /// scheduling. Track order (mic first) is at least a fixed, explainable
    /// answer. A mic-only meeting is unaffected — `track` is constant there, so
    /// this is the same sequence 22-A returned.
    pub fn list_meeting_segments(&self, meeting_id: &str) -> Result<Vec<MeetingSegment>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, meeting_id, start_seconds, end_seconds, text, confidence, created_at,
                        track
                 FROM meeting_segments WHERE meeting_id = ?1
                 ORDER BY start_seconds ASC, track ASC, created_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![meeting_id], map_meeting_segment)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// The Markdown for one meeting (see `meetings::render_markdown`).
    pub fn meeting_markdown(&self, id: &str) -> Result<(Meeting, String), String> {
        let meeting = self
            .get_meeting(id)?
            .ok_or_else(|| format!("no meeting {id}"))?;
        let segments = self.list_meeting_segments(id)?;
        let md = meetings::render_markdown(&meeting, &segments);
        Ok((meeting, md))
    }

    /// Delete a meeting and everything the DB holds about it, returning the
    /// audio paths the caller must unlink. Prefer [`Self::delete_meeting_with_audio`].
    ///
    /// Order matters: segments first (so `meeting_segments_ad` fires and the
    /// FTS5 shadow rows are told to forget those tokens), then the row. Both in
    /// one transaction — a delete that half-succeeds on a privacy feature is
    /// worse than one that fails loudly.
    ///
    /// After the commit, `wal_checkpoint(TRUNCATE)` folds the WAL into the main
    /// DB: `secure_delete` zeroes the freed pages, but the pre-delete frames sit
    /// in the -wal until a checkpoint, and "I deleted it" must not mean "until
    /// the next restart". No `VACUUM` here — that rewrites the whole file and is
    /// reserved for `clear_transcripts`, which is a rarer, heavier promise.
    pub fn delete_meeting(&self, id: &str) -> Result<Vec<PathBuf>, String> {
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let wavs: Vec<PathBuf> = {
            // YV106: BOTH tracks. A delete that removed only the mic wav would
            // leave the other people on the call on disk after the user asked
            // for the meeting to be gone — a privacy feature that half-deletes
            // is worse than none, and the second track is the half that would
            // have been forgotten.
            let mut stmt = conn
                .prepare("SELECT mic_wav_path, sys_wav_path FROM meetings WHERE id = ?1")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![id], |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            let mut v = Vec::new();
            for r in rows {
                let (mic, sys) = r.map_err(|e| e.to_string())?;
                v.extend(mic.map(PathBuf::from));
                v.extend(sys.map(PathBuf::from));
            }
            v
        };

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM meeting_segments WHERE meeting_id = ?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM meetings WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;

        if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
            // The rows are gone either way; a busy checkpoint is not a failure
            // of the delete, and the exit checkpoint will finish the job.
            log::warn!("meeting delete: checkpoint after delete failed: {e}");
        }
        Ok(wavs)
    }

    /// "Delete this meeting" as the user means it: rows, search index, and the
    /// audio on disk. A privacy feature that half-deletes is worse than none.
    pub fn delete_meeting_with_audio(&self, id: &str) -> Result<(), String> {
        let wavs = self.delete_meeting(id)?;
        for wav in &wavs {
            match std::fs::remove_file(wav) {
                Ok(()) => log::info!("meeting delete: removed {}", wav.display()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                // Reported, not returned: the words are already gone, and
                // failing the command would tell the user nothing was deleted.
                Err(e) => log::warn!("meeting delete: {} not removed: {e}", wav.display()),
            }
        }
        Ok(())
    }

    /// YV94 retention (finding #28) — meetings older than `cutoff` lose their
    /// AUDIO, never their transcript. Returns the WAV paths to unlink; the row
    /// is marked `audio_kept = 0` so the UI can say "audio expired" instead of
    /// offering a play button that fails.
    pub fn purge_meeting_audio(&self, cutoff: DateTime<Utc>) -> Result<Vec<PathBuf>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let cutoff = cutoff.to_rfc3339();
        let mut paths = Vec::new();
        {
            // YV106: both tracks age out together — the retention promise is
            // about the meeting's AUDIO, and a system track left behind after
            // the mic track expired would be the promise quietly broken.
            let mut stmt = conn
                .prepare(
                    "SELECT mic_wav_path, sys_wav_path FROM meetings
                     WHERE (mic_wav_path IS NOT NULL OR sys_wav_path IS NOT NULL)
                       AND started_at < ?1",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![cutoff], |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            for r in rows {
                let (mic, sys) = r.map_err(|e| e.to_string())?;
                paths.extend(mic.map(PathBuf::from));
                paths.extend(sys.map(PathBuf::from));
            }
        }
        conn.execute(
            "UPDATE meetings SET mic_wav_path = NULL, sys_wav_path = NULL, audio_kept = 0
             WHERE (mic_wav_path IS NOT NULL OR sys_wav_path IS NOT NULL)
               AND started_at < ?1",
            params![cutoff],
        )
        .map_err(|e| e.to_string())?;
        Ok(paths)
    }

    /// The Insights strip (finding #29). One pass over two small tables.
    pub fn meeting_stats(&self) -> Result<MeetingStats, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stats = MeetingStats {
            audio_retention_days: meetings::AUDIO_RETENTION_DAYS,
            ..Default::default()
        };

        let (total, seconds, with_audio): (i64, f64, i64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(duration_seconds),0),
                        COALESCE(SUM(CASE WHEN mic_wav_path IS NOT NULL THEN 1 ELSE 0 END),0)
                 FROM meetings",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| e.to_string())?;
        stats.total_meetings = total;
        stats.total_seconds = seconds;
        stats.meetings_with_audio = with_audio;

        for (state, slot) in [("complete", 0usize), ("partial", 1), ("failed", 2)] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM meetings WHERE state = ?1",
                    params![state],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            match slot {
                0 => stats.complete_meetings = n,
                1 => stats.partial_meetings = n,
                _ => stats.failed_meetings = n,
            }
        }

        stats.segments_indexed = conn
            .query_row("SELECT COUNT(*) FROM meeting_segments", [], |r| r.get(0))
            .unwrap_or(0);

        let week_ago = (Utc::now() - Duration::days(7)).to_rfc3339();
        stats.meetings_last_7 = conn
            .query_row(
                "SELECT COUNT(*) FROM meetings WHERE started_at >= ?1",
                params![week_ago],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let first: Option<String> = conn
            .query_row("SELECT MIN(started_at) FROM meetings", [], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?
            .flatten();
        let last: Option<String> = conn
            .query_row("SELECT MAX(started_at) FROM meetings", [], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?
            .flatten();
        stats.first_meeting_at = first.clone().map(parse_dt);
        stats.last_meeting_at = last.map(parse_dt);

        // Activation: days from the first thing this user ever dictated to the
        // first meeting they recorded. `None` when either end is missing — a
        // zero would read as "same day", which is a different claim.
        if let Some(first_meeting) = stats.first_meeting_at {
            let first_dictation: Option<String> = conn
                .query_row("SELECT MIN(created_at) FROM transcripts", [], |r| r.get(0))
                .optional()
                .map_err(|e| e.to_string())?
                .flatten();
            // Calendar days, not elapsed 24-hour blocks: "3 days to first
            // meeting" is a claim about dates on a calendar, and truncating an
            // elapsed duration turns 6 h 59 m into "0 days" one day and "1 day"
            // the next for the same pair of events.
            stats.days_to_first_meeting = first_dictation.map(parse_dt).map(|d| {
                (first_meeting.with_timezone(&Local).date_naive()
                    - d.with_timezone(&Local).date_naive())
                .num_days()
                .max(0)
            });
        }

        Ok(stats)
    }

    // ── YV128 · enrolled voices ─────────────────────────────────────────────
    //
    // The rows behind `speaker_profiles.rs`. Every read of a stored embedding
    // goes through `NormalizedEmbedding::from_blob`, which enforces finding
    // #19's `blob.len() == embedding_dim * 4` invariant at USE time — a
    // truncated or half-written BLOB fails here, loudly, with both numbers,
    // rather than becoming a shorter vector that quietly scores against
    // everything.
    //
    // Nothing in this build calls these yet: enrolment is YV129 (matching) and
    // YV130 (correction UX). They ship now because the schema they read is what
    // this item is, and a table with no accessor is a table nobody has proved
    // round-trips.

    /// Enrol a voice. `embedding_dim` is the width the sidecar REPORTED when it
    /// loaded the embedding model (`DiarizeResponse::embedding_dim`) — there is
    /// no default for it in the schema and no constant for it in this codebase,
    /// which is finding #19's fix in the two places it has to hold.
    ///
    /// `embedding_model` is the catalog id the width came from: two 192-dim
    /// models produce two incomparable 192-dim spaces, so a profile that cannot
    /// name its model cannot be safely compared against a later one.
    ///
    /// `locked` starts `true`. A profile exists because a person put a name to
    /// a voice; that is a user-confirmed assignment by definition, and the rule
    /// YV129/YV130 inherit is that an automated pass never overwrites one.
    pub fn create_speaker_profile(
        &self,
        display_name: &str,
        embedding_dim: u32,
        embedding_model: &str,
        is_me: bool,
    ) -> Result<speaker_profiles::SpeakerProfile, String> {
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Err("a speaker profile needs a name".into());
        }
        if embedding_dim == 0 {
            return Err("embedding_dim must be the width the model reported, not zero".into());
        }
        if embedding_model.trim().is_empty() {
            return Err("a speaker profile needs the embedding model it was enrolled with".into());
        }
        let now = Utc::now().to_rfc3339();
        let row = speaker_profiles::SpeakerProfile {
            id: Uuid::new_v4().to_string(),
            display_name: display_name.to_string(),
            embedding_dim,
            embedding_model: embedding_model.trim().to_string(),
            locked: true,
            is_me,
            created_at: now.clone(),
            updated_at: now,
        };
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO speaker_profiles
             (id, display_name, embedding_dim, embedding_model, locked, is_me, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                row.id,
                row.display_name,
                row.embedding_dim,
                row.embedding_model,
                row.locked as i64,
                row.is_me as i64,
                row.created_at,
                row.updated_at,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(row)
    }

    /// Every enrolled voice with every centroid it has accumulated — the unit
    /// [`speaker_profiles::best_match`] scores against.
    ///
    /// Fails on the FIRST unreadable blob rather than skipping it. A profile
    /// silently missing one of its conditions is a profile that quietly stops
    /// matching on the device that condition came from, which is precisely the
    /// failure finding #21 is about; a loud error is recoverable, a silent
    /// degradation is not.
    pub fn list_speaker_profiles(&self) -> Result<Vec<speaker_profiles::ProfileCentroids>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, display_name, embedding_dim, embedding_model, locked, is_me,
                        created_at, updated_at
                 FROM speaker_profiles ORDER BY created_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let profiles: Vec<speaker_profiles::SpeakerProfile> = stmt
            .query_map([], |r| {
                Ok(speaker_profiles::SpeakerProfile {
                    id: r.get(0)?,
                    display_name: r.get(1)?,
                    embedding_dim: r.get::<_, i64>(2)?.max(0) as u32,
                    embedding_model: r.get(3)?,
                    locked: r.get::<_, i64>(4)? != 0,
                    is_me: r.get::<_, i64>(5)? != 0,
                    created_at: r.get(6)?,
                    updated_at: r.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;

        let mut out = Vec::with_capacity(profiles.len());
        for profile in profiles {
            let centroids = Self::read_centroids(&conn, &profile)?;
            out.push(speaker_profiles::ProfileCentroids { profile, centroids });
        }
        Ok(out)
    }

    /// One profile, or `None`. Same read-side invariant as the list.
    pub fn get_speaker_profile(
        &self,
        id: &str,
    ) -> Result<Option<speaker_profiles::ProfileCentroids>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let profile = Self::read_profile(&conn, id)?;
        let Some(profile) = profile else {
            return Ok(None);
        };
        let centroids = Self::read_centroids(&conn, &profile)?;
        Ok(Some(speaker_profiles::ProfileCentroids { profile, centroids }))
    }

    /// Fold one newly-heard embedding into the profile's centroid FOR THAT
    /// CONDITION, creating it if this is the first time the voice has been heard
    /// under it.
    ///
    /// This is the multi-centroid update in one call: a sample recorded on
    /// AirPods updates the `bluetooth_near` centroid and leaves the
    /// `laptop_mic_near` one exactly as it was, which is what stops one
    /// device's samples from dragging the other's average somewhere that
    /// matches neither.
    ///
    /// The sample arrives already L2-normalised (it cannot be anything else —
    /// [`speaker_profiles::NormalizedEmbedding`]'s constructor IS the
    /// normalisation), and the stored result is renormalised by
    /// [`speaker_profiles::Centroid::updated_with`]. Both halves of the
    /// discipline are types here, not steps a caller could forget.
    pub fn observe_speaker_embedding(
        &self,
        profile_id: &str,
        condition_key: &str,
        sample: &speaker_profiles::NormalizedEmbedding,
    ) -> Result<speaker_profiles::Centroid, String> {
        let condition_key = condition_key.trim();
        if condition_key.is_empty() {
            return Err("a centroid needs a condition key".into());
        }
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let profile = Self::read_profile(&conn, profile_id)?
            .ok_or_else(|| format!("no speaker profile {profile_id}"))?;
        if sample.dim() != profile.embedding_dim as usize {
            return Err(format!(
                "speaker profile {profile_id}: {}",
                speaker_profiles::EmbeddingError::DimensionMismatch {
                    expected: profile.embedding_dim as usize,
                    actual: sample.dim(),
                }
            ));
        }

        let existing: Option<(Vec<u8>, i64)> = conn
            .query_row(
                "SELECT embedding, sample_count FROM speaker_centroids
                 WHERE profile_id = ?1 AND condition_key = ?2",
                params![profile.id, condition_key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;

        let updated = match existing {
            Some((blob, count)) => {
                let current = Self::decode_centroid(&profile, condition_key, &blob)?;
                speaker_profiles::Centroid {
                    condition_key: condition_key.to_string(),
                    vector: current,
                    sample_count: count.clamp(1, u32::MAX as i64) as u32,
                }
                .updated_with(sample)
                .map_err(|e| format!("speaker profile {}: {e}", profile.id))?
            }
            None => speaker_profiles::Centroid::first(condition_key, sample.clone()),
        };

        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO speaker_centroids
               (profile_id, condition_key, embedding, sample_count, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(profile_id, condition_key) DO UPDATE SET
               embedding = excluded.embedding,
               sample_count = excluded.sample_count,
               updated_at = excluded.updated_at",
            params![
                profile.id,
                updated.condition_key,
                updated.vector.to_blob(),
                updated.sample_count as i64,
                now,
            ],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE speaker_profiles SET updated_at = ?2 WHERE id = ?1",
            params![profile.id, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(updated)
    }

    /// Rename an enrolled voice, or (re)confirm it. Renaming is a user action,
    /// so it locks the profile.
    pub fn rename_speaker_profile(&self, id: &str, display_name: &str) -> Result<(), String> {
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Err("a speaker profile needs a name".into());
        }
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE speaker_profiles SET display_name = ?2, locked = 1, updated_at = ?3
             WHERE id = ?1",
            params![id, display_name, Utc::now().to_rfc3339()],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// Forget a voice, centroids and all. The `ON DELETE CASCADE` is real here
    /// (`PRAGMA foreign_keys = ON` is set at open), and the centroids are
    /// deleted explicitly first anyway so the row count is knowable and
    /// `secure_delete` runs over both tables regardless of how a given SQLite
    /// build treats cascade deletes.
    pub fn delete_speaker_profile(&self, id: &str) -> Result<bool, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM speaker_centroids WHERE profile_id = ?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
        let n = conn
            .execute("DELETE FROM speaker_profiles WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(n > 0)
    }

    fn read_profile(
        conn: &Connection,
        id: &str,
    ) -> Result<Option<speaker_profiles::SpeakerProfile>, String> {
        conn.query_row(
            "SELECT id, display_name, embedding_dim, embedding_model, locked, is_me,
                    created_at, updated_at
             FROM speaker_profiles WHERE id = ?1",
            params![id],
            |r| {
                Ok(speaker_profiles::SpeakerProfile {
                    id: r.get(0)?,
                    display_name: r.get(1)?,
                    embedding_dim: r.get::<_, i64>(2)?.max(0) as u32,
                    embedding_model: r.get(3)?,
                    locked: r.get::<_, i64>(4)? != 0,
                    is_me: r.get::<_, i64>(5)? != 0,
                    created_at: r.get(6)?,
                    updated_at: r.get(7)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    fn read_centroids(
        conn: &Connection,
        profile: &speaker_profiles::SpeakerProfile,
    ) -> Result<Vec<speaker_profiles::Centroid>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT condition_key, embedding, sample_count FROM speaker_centroids
                 WHERE profile_id = ?1 ORDER BY condition_key ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, Vec<u8>, i64)> = stmt
            .query_map(params![profile.id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        rows.into_iter()
            .map(|(condition_key, blob, count)| {
                let vector = Self::decode_centroid(profile, &condition_key, &blob)?;
                Ok(speaker_profiles::Centroid {
                    condition_key,
                    vector,
                    sample_count: count.clamp(1, u32::MAX as i64) as u32,
                })
            })
            .collect()
    }

    /// The read-side invariant, in the one place every read goes through, with
    /// the profile and condition named so the error identifies the ROW and not
    /// just the arithmetic.
    fn decode_centroid(
        profile: &speaker_profiles::SpeakerProfile,
        condition_key: &str,
        blob: &[u8],
    ) -> Result<speaker_profiles::NormalizedEmbedding, String> {
        speaker_profiles::NormalizedEmbedding::from_blob(blob, profile.embedding_dim as usize)
            .map_err(|e| {
                format!(
                    "speaker profile {} ({}) centroid {condition_key}: {e}",
                    profile.id, profile.display_name
                )
            })
    }

    /// Starred terms first (always-bias), then usage-ranked by hits (YV47).
    /// YV52 — remember a take whose transcription failed, so it can be retried
    /// later against the WAV already on disk. `wav_path` must already be parked
    /// outside the recordings dir (see `record::ClipWav::keep_for_recovery`).
    pub fn record_failed_dictation(
        &self,
        wav_path: &Path,
        speech_seconds: f64,
        error: &str,
        source_app: Option<String>,
    ) -> Result<FailedDictation, String> {
        let row = FailedDictation {
            id: Uuid::new_v4().to_string(),
            wav_path: wav_path.to_string_lossy().to_string(),
            speech_seconds: speech_seconds.max(0.0),
            error: error.to_string(),
            source_app,
            created_at: Utc::now(),
        };
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO failed_dictations
             (id, wav_path, speech_seconds, error, source_app, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                row.id,
                row.wav_path,
                row.speech_seconds,
                row.error,
                row.source_app,
                row.created_at.to_rfc3339(),
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(row)
    }

    /// YV52 — recoverable takes, newest first.
    pub fn list_failed_dictations(&self) -> Result<Vec<FailedDictation>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, wav_path, COALESCE(speech_seconds,0), error, source_app, created_at
                 FROM failed_dictations ORDER BY created_at DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], map_failed_dictation)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// YV52 — one recoverable take, or `None` once it has been retried/discarded.
    pub fn get_failed_dictation(&self, id: &str) -> Result<Option<FailedDictation>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id, wav_path, COALESCE(speech_seconds,0), error, source_app, created_at
             FROM failed_dictations WHERE id = ?1",
            params![id],
            map_failed_dictation,
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    /// YV52 — drop a recoverable take. Returns the WAV path that is now orphaned
    /// so the caller can unlink the audio (the row is the only reference to it).
    pub fn delete_failed_dictation(&self, id: &str) -> Result<Option<String>, String> {
        let path = self.get_failed_dictation(id)?.map(|r| r.wav_path);
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM failed_dictations WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(path)
    }

    /// YV52 retention — drop every recoverable take older than `cutoff`,
    /// returning their WAV paths so the caller can delete the audio. Callers
    /// pass `Utc::now() - Duration::days(FAILED_TAKE_RETENTION_DAYS)`; the seam
    /// exists so the cutoff itself is testable without waiting a week.
    pub fn purge_failed_dictations(&self, cutoff: DateTime<Utc>) -> Result<Vec<String>, String> {
        let stale: Vec<(String, String)> = {
            let conn = self.conn.lock().map_err(|e| e.to_string())?;
            let mut stmt = conn
                .prepare("SELECT id, wav_path, created_at FROM failed_dictations")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?;
            // Compare as instants, not as strings: created_at is RFC3339 but the
            // offset is not guaranteed to be the same on every row.
            rows.flatten()
                .filter(|(_, _, created)| parse_dt(created.clone()) < cutoff)
                .map(|(id, wav, _)| (id, wav))
                .collect()
        };
        let mut purged = Vec::with_capacity(stale.len());
        {
            // YV68 — one transaction for the whole sweep, not one autocommit per
            // row: the paths handed back for unlinking are exactly the rows that
            // are really gone, so a failure part-way can't orphan a WAV whose
            // row survived.
            let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            for (id, wav) in stale {
                tx.execute("DELETE FROM failed_dictations WHERE id = ?1", params![id])
                    .map_err(|e| e.to_string())?;
                purged.push(wav);
            }
            tx.commit().map_err(|e| e.to_string())?;
        }
        Ok(purged)
    }

    /// YV64 — remember one crash. `Ok(true)` means the row is NEW; `Ok(false)`
    /// means this exact evidence (same kind, same artifact, same instant) is
    /// already recorded, which is the normal answer on every launch after the
    /// one that first read it.
    pub fn record_crash_event(&self, event: &CrashEvent) -> Result<bool, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let inserted = conn
            .execute(
                "INSERT OR IGNORE INTO crash_events
                 (id, occurred_at, kind, signature, source_file, details, acknowledged)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    event.id,
                    event.occurred_at.to_rfc3339(),
                    event.kind,
                    event.signature,
                    event.source_file,
                    event.details,
                    i64::from(event.acknowledged),
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(inserted > 0)
    }

    /// YV64 — recorded crashes, newest first, capped at `limit`.
    pub fn list_crash_events(&self, limit: usize) -> Result<Vec<CrashEvent>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, occurred_at, kind, signature, source_file, details, acknowledged
                 FROM crash_events ORDER BY occurred_at DESC LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit as i64], map_crash_event)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// YV64 — the user has now seen the Stability list: nothing is news any
    /// more, so no future launch raises the toast for these rows. Returns how
    /// many rows were still unacknowledged.
    pub fn acknowledge_crash_events(&self) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE crash_events SET acknowledged = 1 WHERE acknowledged = 0",
            [],
        )
        .map_err(|e| e.to_string())
    }

    /// YV64 — throw the crash history away. The reports themselves are still in
    /// `~/Library/Logs/DiagnosticReports/` (they are macOS', not ours), so a
    /// later launch can legitimately re-read one; this clears what Yap stored.
    pub fn clear_crash_events(&self) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM crash_events", [])
            .map_err(|e| e.to_string())
    }

    /// YV52 — a retry succeeded: write the transcript row the take should have
    /// produced and drop the failed row, so a recovered take can never sit in
    /// both lists. The recovered entry keeps the ORIGINAL take's timestamp,
    /// speech seconds and source app, so it lands in history where the user
    /// expects it instead of at the moment they pressed Retry.
    pub fn convert_failed_dictation(
        &self,
        id: &str,
        text: String,
        backend: String,
        asr_seconds: f64,
        raw_text: Option<String>,
    ) -> Result<TranscriptEntry, String> {
        let row = self
            .get_failed_dictation(id)?
            .ok_or_else(|| "That recovery clip is no longer in the list".to_string())?;
        let entry = new_transcript_entry(
            text,
            backend,
            asr_seconds,
            row.speech_seconds,
            0,
            row.source_app,
            row.created_at,
            raw_text,
        );
        {
            // YV68 — history gains the row in the SAME transaction the failed
            // list loses it, so the doc comment above is now enforced rather
            // than hoped for: two autocommits could leave the take in both
            // lists (delete failed) or, on a rollup error, in neither.
            let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            let _ = self.learn_from_transcript_tx(&tx, &entry.text);
            self.insert_transcript_row_tx(&tx, &entry)?;
            tx.execute("DELETE FROM failed_dictations WHERE id = ?1", params![id])
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        // Only once the swap is committed is the WAV the caller's to unlink.
        if let Err(e) = self.recompute_daily_stats() {
            log::warn!("daily rollup: {e}");
        }
        Ok(entry)
    }

    pub fn list_dictionary(&self) -> Result<Vec<DictEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, term, preferred, hits, created_at, starred FROM dictionary
                 ORDER BY starred DESC, hits DESC, term COLLATE NOCASE ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(DictEntry {
                    id: row.get(0)?,
                    term: row.get(1)?,
                    preferred: row.get(2)?,
                    hits: row.get(3)?,
                    created_at: parse_dt(row.get::<_, String>(4)?),
                    starred: row.get::<_, i64>(5)? != 0,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn add_dictionary_term(
        &self,
        term: String,
        preferred: Option<String>,
    ) -> Result<DictEntry, String> {
        let term = term.trim().to_string();
        if term.is_empty() {
            return Err("empty term".into());
        }
        let entry = DictEntry {
            id: Uuid::new_v4().to_string(),
            term,
            preferred,
            hits: 0,
            created_at: Utc::now(),
            starred: false,
        };
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO dictionary (id, term, preferred, hits, created_at, source)
             VALUES (?1, ?2, ?3, 0, ?4, 'manual')
             ON CONFLICT(term) DO UPDATE SET preferred = excluded.preferred, source = 'manual'",
            params![
                entry.id,
                entry.term,
                entry.preferred,
                entry.created_at.to_rfc3339()
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(entry)
    }

    /// Edit an existing entry in place (YV47 dictionary editing). Renaming onto
    /// another row's term is rejected by the UNIQUE index rather than silently
    /// merging the two.
    pub fn update_dictionary_term(
        &self,
        id: &str,
        term: String,
        preferred: Option<String>,
    ) -> Result<(), String> {
        let term = term.trim().to_string();
        if term.is_empty() {
            return Err("empty term".into());
        }
        let preferred = preferred
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let n = conn
            .execute(
                "UPDATE dictionary SET term = ?2, preferred = ?3, source = 'manual'
                 WHERE id = ?1",
                params![id, term, preferred],
            )
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("no such dictionary term".into());
        }
        Ok(())
    }

    /// Star / unstar a term (YV47). Starred terms are pinned to the head of the
    /// ranking and always make it into the decoder prompt.
    pub fn set_dictionary_starred(&self, id: &str, starred: bool) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let n = conn
            .execute(
                "UPDATE dictionary SET starred = ?2 WHERE id = ?1",
                params![id, i64::from(starred)],
            )
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("no such dictionary term".into());
        }
        Ok(())
    }

    pub fn delete_dictionary_term(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM dictionary WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Terms to bias the decoder with, LEAST important first (YV47).
    ///
    /// Ordering is starred → hits → age, and the list is reversed so the most
    /// important term is last: a Whisper prompt weights later tokens more, and
    /// both our own cap ([`crate::asr_engine::build_bias_prompt`]) and
    /// transcribe-cpp's own `max_prev_context_tokens` truncation drop from the
    /// FRONT, so the tail is what always survives.
    ///
    /// A term with a `preferred` form yields that form — biasing the decoder
    /// toward the misheard spelling would defeat the point.
    pub fn bias_terms(&self, limit: i64) -> Result<Vec<String>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(NULLIF(preferred, ''), term) FROM dictionary
                 ORDER BY starred DESC, hits DESC, created_at ASC LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        let mut terms: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for r in rows {
            let t = r.map_err(|e| e.to_string())?;
            if seen.insert(t.to_lowercase()) {
                terms.push(t);
            }
        }
        terms.reverse(); // most-important last
        Ok(terms)
    }

    /// Apply a user's "Fix transcription" edit (YV47): rewrite the stored
    /// transcript and turn the words they changed into dictionary candidates.
    ///
    /// This is the honest auto-learn signal — an explicit correction of a known
    /// transcript, not a guess at what happened in someone else's text field.
    pub fn record_correction(
        &self,
        transcript_id: &str,
        corrected: &str,
    ) -> Result<Vec<Correction>, String> {
        let corrected = corrected.trim();
        if corrected.is_empty() {
            return Err("empty correction".into());
        }
        let now = Utc::now().to_rfc3339();
        let corrections = {
            let conn = self.conn.lock().map_err(|e| e.to_string())?;
            let original: String = conn
                .query_row(
                    "SELECT text FROM transcripts WHERE id = ?1",
                    params![transcript_id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if original.trim() == corrected {
                return Ok(Vec::new());
            }
            conn.execute(
                "UPDATE transcripts SET text = ?2, word_count = ?3 WHERE id = ?1",
                params![transcript_id, corrected, word_count(corrected)],
            )
            .map_err(|e| e.to_string())?;

            let corrections = diff_corrections(&original, corrected);
            for c in &corrections {
                // Skip anything the dictionary already knows: an existing
                // wrong→right rewrite, or a term already listed on its own.
                let known: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM dictionary
                         WHERE (?1 IS NOT NULL AND term = ?1 COLLATE NOCASE
                                AND preferred = ?2 COLLATE NOCASE)
                            OR (?1 IS NULL AND term = ?2 COLLATE NOCASE)",
                        params![c.wrong, c.term],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                if known > 0 {
                    continue;
                }
                conn.execute(
                    "INSERT INTO dict_candidates (id, term, wrong, use_count, dismissed, created_at)
                     VALUES (?1, ?2, ?3, 1, 0, ?4)
                     ON CONFLICT(term) DO UPDATE SET
                       use_count = use_count + 1,
                       dismissed = 0,
                       wrong = COALESCE(excluded.wrong, dict_candidates.wrong)",
                    params![Uuid::new_v4().to_string(), c.term, c.wrong, now],
                )
                .map_err(|e| e.to_string())?;
            }
            corrections
        };
        // The edit changed word_count, so the rollups have to be rebuilt.
        self.recompute_daily_stats()?;
        Ok(corrections)
    }

    /// Pending "Yap noticed: …" suggestions, most-corrected first (YV47).
    pub fn list_dict_candidates(&self, limit: i64) -> Result<Vec<DictCandidate>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, term, wrong, use_count, created_at FROM dict_candidates
                 WHERE dismissed = 0
                 ORDER BY use_count DESC, created_at DESC LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok(DictCandidate {
                    id: row.get(0)?,
                    term: row.get(1)?,
                    wrong: row.get(2)?,
                    use_count: row.get(3)?,
                    created_at: parse_dt(row.get::<_, String>(4)?),
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// Accept a candidate into the dictionary (YV47). A substitution becomes a
    /// `wrong → right` rewrite rule; an inserted word becomes a bias-only term.
    pub fn promote_dict_candidate(&self, id: &str) -> Result<DictEntry, String> {
        let (term, wrong) = {
            let conn = self.conn.lock().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT term, wrong FROM dict_candidates WHERE id = ?1",
                params![id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .map_err(|e| e.to_string())?
        };
        let entry = match wrong {
            Some(w) => self.add_dictionary_term(w, Some(term))?,
            None => self.add_dictionary_term(term, None)?,
        };
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM dict_candidates WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(entry)
    }

    /// Hide a suggestion without learning it (YV47). Kept (not deleted) so the
    /// same word isn't re-suggested on the next correction.
    pub fn dismiss_dict_candidate(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE dict_candidates SET dismissed = 1 WHERE id = ?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Apply dictionary preferred forms to ASR text (case-insensitive whole-token replace).
    pub fn apply_dictionary(&self, text: &str) -> Result<String, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT term, preferred FROM dictionary
                 WHERE preferred IS NOT NULL AND preferred != ''",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        let mut replacements: Vec<(String, String)> = Vec::new();
        for r in rows {
            replacements.push(r.map_err(|e| e.to_string())?);
        }
        let mut out = text.to_string();
        for (term, preferred) in replacements {
            let lower = out.to_lowercase();
            let needle = term.to_lowercase();
            if !lower.contains(&needle) {
                continue;
            }
            // token-wise replace preserving surrounding punctuation loosely
            let tokens: Vec<String> = out
                .split_whitespace()
                .map(|tok| {
                    let bare = tok.trim_matches(|c: char| !c.is_alphanumeric());
                    if bare.eq_ignore_ascii_case(&term) {
                        tok.replacen(bare, &preferred, 1)
                    } else {
                        tok.to_string()
                    }
                })
                .collect();
            let next = tokens.join(" ");
            if next != out {
                out = next;
                let _ = conn.execute(
                    "UPDATE dictionary SET hits = hits + 1 WHERE term = ?1 COLLATE NOCASE",
                    params![term],
                );
            }
        }
        Ok(out)
    }

    // ── YV48 snippets ────────────────────────────────────────────────────────

    /// Every snippet for the Settings list, enabled ones first then A→Z.
    pub fn list_snippets(&self) -> Result<Vec<Snippet>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, trigger_phrase, expansion, enabled, created_at FROM snippets
                 ORDER BY enabled DESC, trigger_phrase COLLATE NOCASE ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Snippet {
                    id: row.get(0)?,
                    trigger: row.get(1)?,
                    expansion: row.get(2)?,
                    enabled: row.get::<_, i64>(3)? != 0,
                    created_at: parse_dt(row.get::<_, String>(4)?),
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn add_snippet(&self, trigger: String, expansion: String) -> Result<Snippet, String> {
        let trigger = trigger.trim().to_string();
        if trigger.is_empty() {
            return Err("empty trigger".into());
        }
        if expansion.trim().is_empty() {
            return Err("empty expansion".into());
        }
        let snippet = Snippet {
            id: Uuid::new_v4().to_string(),
            trigger,
            expansion,
            enabled: true,
            created_at: Utc::now(),
        };
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO snippets (id, trigger_phrase, expansion, enabled, created_at)
             VALUES (?1, ?2, ?3, 1, ?4)
             ON CONFLICT(trigger_phrase) DO UPDATE SET expansion = excluded.expansion, enabled = 1",
            params![
                snippet.id,
                snippet.trigger,
                snippet.expansion,
                snippet.created_at.to_rfc3339()
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(snippet)
    }

    /// Edit a snippet in place. Renaming onto another row's trigger is rejected
    /// by the UNIQUE index rather than silently merging the two.
    pub fn update_snippet(
        &self,
        id: &str,
        trigger: String,
        expansion: String,
    ) -> Result<(), String> {
        let trigger = trigger.trim().to_string();
        if trigger.is_empty() {
            return Err("empty trigger".into());
        }
        if expansion.trim().is_empty() {
            return Err("empty expansion".into());
        }
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let n = conn
            .execute(
                "UPDATE snippets SET trigger_phrase = ?2, expansion = ?3 WHERE id = ?1",
                params![id, trigger, expansion],
            )
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("no such snippet".into());
        }
        Ok(())
    }

    /// Turn a snippet off without deleting it — disabled rows never match.
    pub fn set_snippet_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let n = conn
            .execute(
                "UPDATE snippets SET enabled = ?2 WHERE id = ?1",
                params![id, i64::from(enabled)],
            )
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("no such snippet".into());
        }
        Ok(())
    }

    pub fn delete_snippet(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM snippets WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// The ENABLED rules the matcher runs with (YV48). Read once per take on the
    /// dictation path; an error here degrades to "no expansion" at the call site
    /// so the transcript still pastes.
    pub fn snippet_rules(&self) -> Result<Vec<SnippetRule>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT trigger_phrase, expansion FROM snippets WHERE enabled = 1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SnippetRule {
                    trigger: row.get(0)?,
                    expansion: row.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn list_scratch(&self) -> Result<Vec<ScratchNote>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, title, body, updated_at FROM scratchpad
                 ORDER BY updated_at DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ScratchNote {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    body: row.get(2)?,
                    updated_at: parse_dt(row.get::<_, String>(3)?),
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn save_scratch(
        &self,
        id: Option<String>,
        title: String,
        body: String,
    ) -> Result<ScratchNote, String> {
        let note = ScratchNote {
            id: id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            title,
            body,
            updated_at: Utc::now(),
        };
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO scratchpad (id, title, body, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
               title = excluded.title,
               body = excluded.body,
               updated_at = excluded.updated_at",
            params![
                note.id,
                note.title,
                note.body,
                note.updated_at.to_rfc3339()
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(note)
    }

    pub fn delete_scratch(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM scratchpad WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn insights(&self) -> Result<Insights, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        let total_words: i64 = conn
            .query_row("SELECT COALESCE(SUM(word_count),0) FROM transcripts", [], |r| {
                r.get(0)
            })
            .unwrap_or(0);
        let total_sessions: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcripts", [], |r| r.get(0))
            .unwrap_or(0);
        let avg_asr: f64 = conn
            .query_row(
                "SELECT COALESCE(AVG(asr_seconds),0) FROM transcripts",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0.0);

        // `daily_stats` is the authoritative rollup: it's fully rebuilt from
        // `transcripts` on every insert/delete/clear AND healed once at startup
        // (see recompute_daily_stats + open()), so a today row exists whenever a
        // transcript exists for today. Reading it is an O(1) indexed lookup — we
        // deliberately do NOT fall back to a full-table scan here, which used to
        // fire on every fresh-day render (words_today == 0) and would become an
        // O(N) hotspot as multi-year history accumulates.
        let today = Local::now().date_naive().to_string();
        let (words_today, sessions_today): (i64, i64) = conn
            .query_row(
                "SELECT COALESCE(words,0), COALESCE(sessions,0) FROM daily_stats WHERE day = ?1",
                params![today],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or((0, 0));

        // WPM = words / speaking-minutes, pooled across all sessions. Only rows
        // with measured speech (>0.05s) count — never asr_seconds (inference time).
        //
        // Guard against VAD under-measurement: a quiet clip with one loud transient
        // can yield a tiny voiced value while carrying many words → an impossible
        // per-session rate that inflates the pooled average. Floor each row's
        // speaking time at the word-count-implied minimum (words / MAX_WPM) so no
        // session can claim a super-human rate, and cap the reported average. The
        // RAW speech sum is kept separately as an honest hygiene stat.
        const MAX_WPM: f64 = 400.0;
        let wpm_sql = format!(
            "SELECT COALESCE(SUM(word_count),0),
                    COALESCE(SUM(MAX(speech_seconds, word_count * 60.0 / {MAX_WPM})),0),
                    COALESCE(SUM(speech_seconds),0),
                    COUNT(*)
             FROM transcripts
             WHERE speech_seconds > 0.05"
        );
        let (wpm_words, wpm_speech, total_speech, wpm_sessions): (i64, f64, f64, i64) = conn
            .query_row(&wpm_sql, [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap_or((0, 0.0, 0.0, 0));
        let avg_wpm = if wpm_speech > 0.5 && wpm_words > 0 {
            ((wpm_words as f64) / (wpm_speech / 60.0)).min(MAX_WPM)
        } else {
            0.0
        };

        // Pipeline latency percentiles (release → clipboard)
        let pipe_vals: Vec<i64> = {
            let mut stmt = conn
                .prepare(
                    "SELECT pipeline_ms FROM transcripts
                     WHERE pipeline_ms > 0
                     ORDER BY pipeline_ms ASC",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| r.get::<_, i64>(0))
                .map_err(|e| e.to_string())?;
            let mut v = Vec::new();
            for r in rows {
                v.push(r.map_err(|e| e.to_string())?);
            }
            v
        };
        let p50_pipeline_ms = percentile_ms(&pipe_vals, 0.50);
        let p95_pipeline_ms = percentile_ms(&pipe_vals, 0.95);

        // last 7 days
        let mut words_last_7 = Vec::new();
        for i in (0..7).rev() {
            let d = (Local::now().date_naive() - Duration::days(i)).to_string();
            let (w, s): (i64, i64) = conn
                .query_row(
                    "SELECT COALESCE(words,0), COALESCE(sessions,0) FROM daily_stats WHERE day = ?1",
                    params![d],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?
                .unwrap_or((0, 0));
            words_last_7.push(DayCount {
                date: d,
                words: w,
                sessions: s,
            });
        }

        // streak from active days (sessions > 0)
        let days: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT day FROM daily_stats WHERE sessions > 0 ORDER BY day DESC")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            let mut v = Vec::new();
            for r in rows {
                v.push(r.map_err(|e| e.to_string())?);
            }
            v
        };
        let (streak_days, longest_streak) = compute_streaks(&days);

        // top apps — hide "Unknown" noise when we have real labels mixed in
        let mut top_apps = Vec::new();
        {
            let mut stmt = conn
                .prepare(
                    "SELECT COALESCE(NULLIF(TRIM(source_app), ''), 'Unknown') as app,
                            SUM(word_count) as w, COUNT(*) as c
                     FROM transcripts
                     GROUP BY 1
                     ORDER BY w DESC
                     LIMIT 8",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(AppCount {
                        app: r.get(0)?,
                        words: r.get(1)?,
                        sessions: r.get(2)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            for r in rows {
                top_apps.push(r.map_err(|e| e.to_string())?);
            }
        }

        // YV94 — the Meetings strip. `meeting_stats` takes the same
        // (non-reentrant) connection lock, so this guard MUST be released first;
        // holding it here would deadlock the Insights screen.
        drop(conn);
        let meetings = self.meeting_stats().unwrap_or_else(|e| {
            // Insights is a read-only screen: a meeting rollup that fails must
            // not blank the dictation numbers next to it.
            log::warn!("meeting stats unavailable: {e}");
            MeetingStats {
                audio_retention_days: meetings::AUDIO_RETENTION_DAYS,
                ..Default::default()
            }
        });

        Ok(Insights {
            total_words,
            total_sessions,
            words_today,
            sessions_today,
            avg_wpm,
            streak_days,
            longest_streak,
            words_last_7,
            top_apps,
            avg_asr_seconds: avg_asr,
            p50_pipeline_ms,
            p95_pipeline_ms,
            speech_seconds_total: total_speech,
            wpm_sample_sessions: wpm_sessions,
            meetings,
        })
    }

    /// Contiguous day-by-day series for the last `days` days (today inclusive),
    /// oldest first, zero-filled for days with no activity. This is the primitive
    /// behind every Insights chart — bar/line series and the GitHub-style activity
    /// heatmap. `daily_stats` retains every active day (nothing is pruned), so any
    /// window works, including "beyond 365 days". Reads the authoritative rollup,
    /// so it's a single indexed range scan, not a transcript rescan.
    pub fn daily_series(&self, days: i64) -> Result<Vec<DayCount>, String> {
        let days = days.clamp(1, 3660); // cap at ~10y to bound the vector
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let start = (Local::now().date_naive() - Duration::days(days - 1)).to_string();

        use std::collections::HashMap;
        let mut map: HashMap<String, (i64, i64)> = HashMap::new();
        {
            let mut stmt = conn
                .prepare("SELECT day, words, sessions FROM daily_stats WHERE day >= ?1")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![start], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
                })
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (d, w, s) = row.map_err(|e| e.to_string())?;
                map.insert(d, (w, s));
            }
        }

        let mut out = Vec::with_capacity(days as usize);
        for i in (0..days).rev() {
            let d = (Local::now().date_naive() - Duration::days(i)).to_string();
            let (w, s) = map.get(&d).copied().unwrap_or((0, 0));
            out.push(DayCount { date: d, words: w, sessions: s });
        }
        Ok(out)
    }

    /// Contiguous month-by-month rollup for the last `months` months (this month
    /// inclusive), oldest first, zero-filled. `date` is "YYYY-MM". Powers the
    /// year/all-time Insights views without shipping day-granular data to the UI.
    /// Aggregates `daily_stats` by calendar month in SQL (one grouped scan).
    pub fn monthly_series(&self, months: i64) -> Result<Vec<DayCount>, String> {
        use chrono::Datelike;
        let months = months.clamp(1, 240); // cap at 20y
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        use std::collections::HashMap;
        let mut map: HashMap<String, (i64, i64)> = HashMap::new();
        {
            // day is "YYYY-MM-DD" → the month key is its first 7 chars.
            let mut stmt = conn
                .prepare(
                    "SELECT substr(day,1,7) AS ym, COALESCE(SUM(words),0), COALESCE(SUM(sessions),0)
                     FROM daily_stats GROUP BY ym",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
                })
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (ym, w, s) = row.map_err(|e| e.to_string())?;
                map.insert(ym, (w, s));
            }
        }

        // Build the contiguous list of month keys, newest → oldest, then reverse.
        let today = Local::now().date_naive();
        let (mut y, mut m) = (today.year(), today.month() as i32);
        let mut keys: Vec<String> = Vec::with_capacity(months as usize);
        for _ in 0..months {
            keys.push(format!("{y:04}-{m:02}"));
            m -= 1;
            if m == 0 {
                m = 12;
                y -= 1;
            }
        }
        keys.reverse(); // oldest first
        let out = keys
            .into_iter()
            .map(|ym| {
                let (w, s) = map.get(&ym).copied().unwrap_or((0, 0));
                DayCount { date: ym, words: w, sessions: s }
            })
            .collect();
        Ok(out)
    }

    /// Stream EVERY transcript, OLDEST first, into `f` — one row at a time.
    ///
    /// YV77: the export used to go through `list_transcripts` with a hard-coded
    /// limit of 10,000, which is `ORDER BY created_at DESC LIMIT ?1` — so a
    /// history longer than that silently dropped the OLDEST rows. This is
    /// the bulk-read seam: no LIMIT, ascending so the file reads like a journal,
    /// and `query_map` hands back one row at a time so memory stays constant no
    /// matter how long the history gets (nothing is collected into a Vec).
    ///
    /// Returns the number of rows handed to `f`.
    pub fn for_each_transcript(
        &self,
        mut f: impl FnMut(TranscriptEntry) -> Result<(), String>,
    ) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, text, backend, asr_seconds, COALESCE(speech_seconds,0),
                        COALESCE(pipeline_ms,0), word_count, source_app, created_at, raw_text
                 FROM transcripts ORDER BY created_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], map_transcript).map_err(|e| e.to_string())?;
        let mut n = 0usize;
        for r in rows {
            f(r.map_err(|e| e.to_string())?)?;
            n += 1;
        }
        Ok(n)
    }

    /// Write the whole history to `w` as JSON Lines for backup / LoRA corpus
    /// later: one COMPACT JSON object per line, oldest first. Line-delimited
    /// (not one pretty array) so the file is appendable and streamable — a
    /// corpus reader never has to hold the history in memory, and neither does
    /// this writer. Returns the row count. See [`Self::for_each_transcript`].
    pub fn write_transcripts_jsonl(&self, w: &mut impl std::io::Write) -> Result<usize, String> {
        self.for_each_transcript(|entry| {
            let line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
            w.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
            w.write_all(b"\n").map_err(|e| e.to_string())
        })
    }

    /// Migrate legacy JSON history if present.
    pub fn migrate_json_if_needed(&self, json_path: PathBuf) -> Result<usize, String> {
        if !json_path.exists() {
            return Ok(0);
        }
        let s = std::fs::read_to_string(&json_path).map_err(|e| e.to_string())?;
        #[derive(Deserialize)]
        struct Legacy {
            entries: Vec<LegacyEntry>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct LegacyEntry {
            text: String,
            backend: String,
            asr_seconds: f64,
        }
        let legacy: Legacy = serde_json::from_str(&s).map_err(|e| e.to_string())?;
        let mut n = 0;
        for e in legacy.entries {
            let _ = self.insert_transcript(e.text, e.backend, e.asr_seconds, 0.0, 0, None)?;
            n += 1;
        }
        let _ = std::fs::rename(&json_path, json_path.with_extension("json.migrated"));
        Ok(n)
    }
}

/// YV94 — the `meetings` projection every read uses, including the JOIN count
/// of segments. Named once so the column ORDER and `map_meeting` cannot drift.
/// The table must be aliased `m` by the caller.
const MEETING_COLS: &str = "m.id, m.title, m.source, m.started_at, m.ended_at, \
     m.duration_seconds, m.state, m.error, m.processed_through_seconds, m.audio_kept, \
     m.mic_wav_path, m.summary, m.summary_model, m.created_at, \
     (SELECT COUNT(*) FROM meeting_segments s WHERE s.meeting_id = m.id), \
     m.diagnostics, m.sys_wav_path, m.tap_rebuilds, m.kind";

fn map_meeting(row: &rusqlite::Row<'_>) -> rusqlite::Result<Meeting> {
    Ok(Meeting {
        id: row.get(0)?,
        title: row.get(1)?,
        source: row.get(2)?,
        started_at: parse_dt(row.get(3)?),
        ended_at: row.get::<_, Option<String>>(4)?.map(parse_dt),
        duration_seconds: row.get(5)?,
        state: row.get(6)?,
        error: row.get(7)?,
        processed_through_seconds: row.get(8)?,
        audio_kept: row.get::<_, i64>(9)? != 0,
        mic_wav_path: row.get(10)?,
        summary: row.get(11)?,
        summary_model: row.get(12)?,
        created_at: parse_dt(row.get(13)?),
        segment_count: row.get(14)?,
        // YV95 — migration 2's diagnostics blob. `None` for every meeting
        // recorded before this build, which is why it is Option and not a
        // defaulted string: "we never measured" and "we measured nothing" are
        // different answers.
        diagnostics: row.get(15)?,
        // YV106 — migration 3. `None` is the honest answer for every mic-only
        // meeting, which is all of 22-A's and most of 22-B's.
        sys_wav_path: row.get(16)?,
        tap_rebuilds: row.get(17)?,
        // YV125 — migration 4. `NOT NULL DEFAULT 'unknown'`, so this is never
        // NULL and never absent; a row written before the column existed reads
        // as `unknown`, which is the branch that clusters Track A.
        kind: row.get(18)?,
    })
}

fn map_meeting_segment(row: &rusqlite::Row<'_>) -> rusqlite::Result<MeetingSegment> {
    Ok(MeetingSegment {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        start_seconds: row.get(2)?,
        end_seconds: row.get(3)?,
        text: row.get(4)?,
        confidence: row.get(5)?,
        created_at: parse_dt(row.get(6)?),
        track: row.get(7)?,
    })
}

fn map_transcript(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranscriptEntry> {
    // id, text, backend, asr_seconds, speech_seconds, pipeline_ms, word_count, source_app, created_at, raw_text
    Ok(TranscriptEntry {
        id: row.get(0)?,
        text: row.get(1)?,
        backend: row.get(2)?,
        asr_seconds: row.get(3)?,
        speech_seconds: row.get(4)?,
        pipeline_ms: row.get(5)?,
        word_count: row.get(6)?,
        source_app: row.get(7)?,
        created_at: parse_dt(row.get::<_, String>(8)?),
        raw_text: row.get(9)?,
    })
}

fn map_crash_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<CrashEvent> {
    // id, occurred_at, kind, signature, source_file, details, acknowledged
    Ok(CrashEvent {
        id: row.get(0)?,
        occurred_at: parse_dt(row.get::<_, String>(1)?),
        kind: row.get(2)?,
        signature: row.get(3)?,
        source_file: row.get(4)?,
        details: row.get(5)?,
        acknowledged: row.get::<_, i64>(6)? != 0,
    })
}

fn map_failed_dictation(row: &rusqlite::Row<'_>) -> rusqlite::Result<FailedDictation> {
    // id, wav_path, speech_seconds, error, source_app, created_at
    Ok(FailedDictation {
        id: row.get(0)?,
        wav_path: row.get(1)?,
        speech_seconds: row.get(2)?,
        error: row.get(3)?,
        source_app: row.get(4)?,
        created_at: parse_dt(row.get::<_, String>(5)?),
    })
}

fn percentile_ms(sorted: &[i64], p: f64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn parse_dt(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn compute_streaks(days_desc: &[String]) -> (i64, i64) {
    if days_desc.is_empty() {
        return (0, 0);
    }
    let mut set: std::collections::HashSet<NaiveDate> = std::collections::HashSet::new();
    for d in days_desc {
        if let Ok(nd) = NaiveDate::parse_from_str(d, "%Y-%m-%d") {
            set.insert(nd);
        }
    }
    let today = Local::now().date_naive();
    let mut streak = 0i64;
    let mut cursor = today;
    // allow missing today if yesterday active
    if !set.contains(&today) {
        cursor = today - Duration::days(1);
    }
    while set.contains(&cursor) {
        streak += 1;
        cursor -= Duration::days(1);
    }

    let mut longest = 0i64;
    let mut cur = 0i64;
    let mut dates: Vec<NaiveDate> = set.into_iter().collect();
    dates.sort();
    let mut prev: Option<NaiveDate> = None;
    for d in dates {
        if let Some(p) = prev {
            if d == p + Duration::days(1) {
                cur += 1;
            } else {
                cur = 1;
            }
        } else {
            cur = 1;
        }
        longest = longest.max(cur);
        prev = Some(d);
    }
    (streak, longest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescue_recreates_corrupt_db() {
        let dir = std::env::temp_dir().join(format!("wv-db-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wilson_voice.db");

        // A garbage (non-SQLite) file — the WAL pragma / quick_check will reject it.
        std::fs::write(&path, b"not a sqlite database, just garbage bytes").unwrap();

        // open() must NOT brick — it quarantines the bad file and recreates.
        let db = Database::open(path.clone()).expect("open should rescue a corrupt DB");
        // The rescued DB is usable.
        db.insights().expect("insights query on rescued DB");

        // A timestamped quarantine copy of the bad file exists.
        let quarantined = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(quarantined, "corrupt DB should have been quarantined");

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_truncates_wal() {
        let dir = std::env::temp_dir().join(format!("wv-ckpt-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wilson_voice.db");

        let db = Database::open(path.clone()).unwrap();
        // Generate WAL frames, then checkpoint(TRUNCATE) → wal file shrinks to 0.
        db.insert_transcript(
            "hello world one two three".into(),
            "native".into(),
            1.0,
            2.0,
            100,
            Some("Test".into()),
        )
        .unwrap();
        db.checkpoint();
        let wal = std::path::PathBuf::from(format!("{}-wal", path.display()));
        if wal.exists() {
            assert_eq!(std::fs::metadata(&wal).unwrap().len(), 0, "WAL not truncated");
        }

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stores_raw_and_polished_text() {
        let dir = std::env::temp_dir().join(format!("wv-raw-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wilson_voice.db");

        let db = Database::open(path.clone()).unwrap();
        // Polished text differs from the raw ASR transcript; both are stored (YV10).
        let entry = db
            .insert_transcript_at(
                "the report is done".into(),
                "native".into(),
                1.0,
                2.0,
                100,
                Some("Notes".into()),
                Utc::now(),
                Some("um the report is uh done".into()),
            )
            .unwrap();
        assert_eq!(entry.text, "the report is done");
        assert_eq!(entry.raw_text.as_deref(), Some("um the report is uh done"));

        // Survives a round-trip through SQLite (column persists + maps back).
        let listed = db.list_transcripts(10, None).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].text, "the report is done");
        assert_eq!(listed[0].raw_text.as_deref(), Some("um the report is uh done"));

        // Default: with no raw supplied, raw_text mirrors the final text.
        let plain = db
            .insert_transcript("plain text".into(), "native".into(), 1.0, 1.0, 0, None)
            .unwrap();
        assert_eq!(plain.raw_text.as_deref(), Some("plain text"));

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn healthy_db_reopen_not_quarantined() {
        let dir = std::env::temp_dir().join(format!("wv-reopen-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wilson_voice.db");

        // Create + populate, then close.
        {
            let db = Database::open(path.clone()).unwrap();
            db.insert_transcript("keep this".into(), "native".into(), 1.0, 1.0, 10, None)
                .unwrap();
        }
        // Reopening a VALID db must not quarantine it, and data must survive.
        let db2 = Database::open(path.clone()).unwrap();
        assert_eq!(db2.list_transcripts(10, None).unwrap().len(), 1, "data lost on reopen");
        let quarantined = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(!quarantined, "healthy DB was wrongly quarantined");

        drop(db2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn learning_harvest_and_biasing() {
        let dir = std::env::temp_dir().join(format!("wv-learn-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::open(dir.join("wilson_voice.db")).unwrap();

        // "The"/"and" are stopwords (dropped); Drivia/RunPod/JAX are jargon (kept).
        // RunPod appears twice → higher hits.
        db.learn_from_transcript("The Drivia RunPod deploy and JAX").unwrap();
        db.learn_from_transcript("RunPod scaled and Supabase synced").unwrap();

        let dict = db.list_dictionary().unwrap();
        let get = |t: &str| dict.iter().find(|d| d.term.eq_ignore_ascii_case(t));
        assert!(get("Drivia").is_some(), "jargon not harvested");
        assert!(get("RunPod").is_some());
        assert!(get("JAX").is_some());
        assert!(get("The").is_none(), "stopword 'The' was harvested");
        assert!(get("and").is_none(), "stopword 'and' was harvested");
        // No term may have preferred == term (the old apply_dictionary no-op bug).
        for d in &dict {
            assert!(
                d.preferred.as_deref() != Some(d.term.as_str()),
                "term {} has preferred==term",
                d.term
            );
        }
        // Most-frequent term is LAST (Whisper weights later prompt tokens more).
        let top = db.bias_terms(50).unwrap();
        assert_eq!(top.last().map(String::as_str), Some("RunPod"), "not most-frequent-last: {top:?}");

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manual_term_survives_purge() {
        let dir = std::env::temp_dir().join(format!("wv-manual-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wilson_voice.db");

        {
            let db = Database::open(path.clone()).unwrap();
            // A manual, term-ONLY add (no preferred) of a plain first-cap proper noun —
            // exactly the row the purge used to wrongly delete.
            db.add_dictionary_term("Anthropic".into(), None).unwrap();
        }
        // Reopen → runs the startup purge. Manual term must survive.
        let db2 = Database::open(path.clone()).unwrap();
        let present = db2
            .list_dictionary()
            .unwrap()
            .iter()
            .any(|d| d.term.eq_ignore_ascii_case("Anthropic"));
        assert!(present, "manual term 'Anthropic' was wrongly purged");

        drop(db2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── YV47 auto-learning dictionary ────────────────────────────────────────

    /// The correction diff is the whole auto-learn signal: it must pull the
    /// words the user actually changed out of an edit, and nothing else.
    #[test]
    fn correction_diff_extracts_candidates() {
        // Substitution of a proper noun — the canonical "Yap misheard it" case.
        let out = diff_corrections(
            "ship the Divya release with Jason today",
            "ship the Drivia release with Jeisil today",
        );
        assert_eq!(
            out,
            vec![
                Correction {
                    wrong: Some("Divya".into()),
                    term: "Drivia".into()
                },
                Correction {
                    wrong: Some("Jason".into()),
                    term: "Jeisil".into()
                },
            ],
            "substituted proper nouns not extracted: {out:?}"
        );

        // A word typed in where ASR dropped one is a candidate with no `wrong`.
        let out = diff_corrections("deploy to today", "deploy to Supabase today");
        assert_eq!(
            out,
            vec![Correction {
                wrong: None,
                term: "Supabase".into()
            }]
        );

        // Casing counts (Whisper lowercases jargon constantly)…
        let out = diff_corrections("run the runpod job", "run the RunPod job");
        assert_eq!(
            out,
            vec![Correction {
                wrong: Some("runpod".into()),
                term: "RunPod".into()
            }]
        );

        // …but ordinary typo fixes and punctuation churn are NOT vocabulary.
        assert!(diff_corrections("teh cat sat", "the cat sat").is_empty());
        assert!(diff_corrections("hello there", "hello, there.").is_empty());
        assert!(diff_corrections("same text", "same text").is_empty());
    }

    /// End to end: fixing a transcript rewrites it and leaves suggestions that
    /// can be accepted into the dictionary (or dismissed).
    #[test]
    fn correction_feeds_candidates_into_the_dictionary() {
        let db = Database::open_in_memory().unwrap();
        let entry = db
            .insert_transcript(
                "meet Divya at noon".into(),
                "native".into(),
                1.0,
                1.0,
                4,
                None,
            )
            .unwrap();

        let learned = db
            .record_correction(&entry.id, "meet Drivia at noon")
            .unwrap();
        assert_eq!(learned.len(), 1, "correction not mined: {learned:?}");

        // The stored transcript is now the corrected text.
        let stored = db.list_transcripts(10, None).unwrap();
        assert_eq!(stored[0].text, "meet Drivia at noon");

        let candidates = db.list_dict_candidates(10).unwrap();
        let c = candidates
            .iter()
            .find(|c| c.term == "Drivia")
            .expect("no 'Drivia' candidate");
        assert_eq!(c.wrong.as_deref(), Some("Divya"));
        assert_eq!(c.use_count, 1);

        // The same correction again is the SAME candidate, ranked higher.
        let entry2 = db
            .insert_transcript(
                "meet Divya again".into(),
                "native".into(),
                1.0,
                1.0,
                3,
                None,
            )
            .unwrap();
        db.record_correction(&entry2.id, "meet Drivia again")
            .unwrap();
        let c = db
            .list_dict_candidates(10)
            .unwrap()
            .into_iter()
            .find(|c| c.term == "Drivia")
            .expect("candidate vanished on repeat");
        assert_eq!(c.use_count, 2, "repeat correction did not raise use_count");

        // Accepting it installs the Divya → Drivia rewrite and clears the suggestion.
        db.promote_dict_candidate(&c.id).unwrap();
        assert_eq!(db.apply_dictionary("meet Divya").unwrap(), "meet Drivia");
        assert!(
            !db.list_dict_candidates(10)
                .unwrap()
                .iter()
                .any(|x| x.term == "Drivia"),
            "promoted candidate still suggested"
        );

        // A known rewrite is never suggested again.
        let entry3 = db
            .insert_transcript("Divya ships".into(), "native".into(), 1.0, 1.0, 2, None)
            .unwrap();
        db.record_correction(&entry3.id, "Drivia ships").unwrap();
        assert!(
            !db.list_dict_candidates(10)
                .unwrap()
                .iter()
                .any(|x| x.term == "Drivia"),
            "already-learned rewrite was re-suggested"
        );
    }

    /// Ranking is what actually reaches the decoder: starred terms outrank
    /// everything, then usage, and the most important term is LAST.
    #[test]
    fn bias_ranking_puts_starred_terms_last() {
        let db = Database::open_in_memory().unwrap();
        // Three terms, hits ascending: rare < middling < frequent.
        db.add_dictionary_term("Rare".into(), None).unwrap();
        db.add_dictionary_term("Middling".into(), None).unwrap();
        db.add_dictionary_term("Frequent".into(), None).unwrap();
        for _ in 0..2 {
            db.learn_from_transcript("Mid_dling").unwrap(); // jargon token, harvested
        }
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("UPDATE dictionary SET hits = 9 WHERE term = 'Frequent'", [])
                .unwrap();
            conn.execute("UPDATE dictionary SET hits = 4 WHERE term = 'Middling'", [])
                .unwrap();
        }

        let ranked = db.bias_terms(50).unwrap();
        let pos = |t: &str| ranked.iter().position(|x| x == t).expect("term missing");
        assert!(
            pos("Rare") < pos("Middling"),
            "usage ranking wrong: {ranked:?}"
        );
        assert!(
            pos("Middling") < pos("Frequent"),
            "usage ranking wrong: {ranked:?}"
        );

        // Starring the LEAST-used term pins it past everything else.
        let rare = db
            .list_dictionary()
            .unwrap()
            .into_iter()
            .find(|d| d.term == "Rare")
            .unwrap();
        db.set_dictionary_starred(&rare.id, true).unwrap();
        assert!(
            db.list_dictionary()
                .unwrap()
                .first()
                .is_some_and(|d| d.term == "Rare" && d.starred),
            "starred term is not at the head of the list"
        );
        let ranked = db.bias_terms(50).unwrap();
        assert_eq!(
            ranked.last().map(String::as_str),
            Some("Rare"),
            "starred term is not the last (highest-weighted) prompt term: {ranked:?}"
        );

        // A rewrite biases toward the PREFERRED spelling, never the misheard one.
        db.add_dictionary_term("Divya".into(), Some("Drivia".into()))
            .unwrap();
        let ranked = db.bias_terms(50).unwrap();
        assert!(ranked.iter().any(|t| t == "Drivia"), "{ranked:?}");
        assert!(!ranked.iter().any(|t| t == "Divya"), "{ranked:?}");
    }

    /// A starred term must survive the startup harvest purge — otherwise
    /// "always bias" silently expires on the next launch.
    #[test]
    fn starred_term_survives_purge() {
        let dir = std::env::temp_dir().join(format!("wv-star-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wilson_voice.db");

        {
            let db = Database::open(path.clone()).unwrap();
            // A harvested, non-jargon row — exactly what the purge deletes.
            {
                let conn = db.conn.lock().unwrap();
                conn.execute(
                    "INSERT INTO dictionary (id, term, preferred, hits, created_at, source, starred)
                     VALUES ('star-1', 'Keeps', NULL, 3, '2026-07-31T00:00:00Z', 'harvest', 1)",
                    [],
                )
                .unwrap();
            }
        }
        let db2 = Database::open(path.clone()).unwrap();
        assert!(
            db2.list_dictionary()
                .unwrap()
                .iter()
                .any(|d| d.term == "Keeps" && d.starred),
            "starred term was purged on reopen"
        );

        drop(db2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Analytics math (total words / WPM / streaks / words-today) ────────────
    // These prove the numbers Wilson watches are exactly right, under the real
    // edge cases: legacy speech=0 rows, multi-day history, streak grace + gaps.

    fn fresh_db(tag: &str) -> (Database, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("wv-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::open(dir.join("wilson_voice.db")).unwrap();
        (db, dir)
    }

    /// UTC instant for local noon `n` days ago — anchored at noon so day-boundary
    /// timezone conversion can never land the session on the wrong calendar day.
    fn days_ago_noon(n: i64) -> DateTime<Utc> {
        let day = Local::now().date_naive() - Duration::days(n);
        day.and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn words(n: usize) -> String {
        (0..n).map(|i| format!("w{i}")).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn total_words_and_sessions_are_exact() {
        let (db, dir) = fresh_db("total");
        db.insert_transcript(words(3), "native".into(), 0.5, 1.0, 0, None).unwrap();
        db.insert_transcript(words(2), "native".into(), 0.5, 1.0, 0, None).unwrap();
        db.insert_transcript(words(1), "native".into(), 0.5, 1.0, 0, None).unwrap();
        let ins = db.insights().unwrap();
        assert_eq!(ins.total_words, 6, "total words must sum every session");
        assert_eq!(ins.total_sessions, 3);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wpm_is_speech_weighted_and_excludes_legacy_zero_rows() {
        let (db, dir) = fresh_db("wpm");
        // 30 words / 30s + 30 words / 30s = 60 words / 60s = exactly 60 WPM.
        db.insert_transcript(words(30), "native".into(), 5.0, 30.0, 0, None).unwrap();
        db.insert_transcript(words(30), "native".into(), 5.0, 30.0, 0, None).unwrap();
        // A legacy row with NO measured speech (speech_seconds = 0). It contributes
        // 100 words to total, but MUST NOT enter the WPM math (no speaking time).
        db.insert_transcript(words(100), "native".into(), 9.0, 0.0, 0, None).unwrap();

        let ins = db.insights().unwrap();
        assert_eq!(ins.total_words, 160, "all words count toward the total");
        assert!(
            (ins.avg_wpm - 60.0).abs() < 1e-6,
            "WPM must be 60 (speech-weighted), got {}",
            ins.avg_wpm
        );
        assert_eq!(ins.wpm_sample_sessions, 2, "only speech rows are WPM samples");
        assert!(
            (ins.speech_seconds_total - 60.0).abs() < 1e-6,
            "speech total must exclude the zero-speech row"
        );
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wpm_is_clamped_against_transient_fooled_short_speech() {
        let (db, dir) = fresh_db("wpmclamp");
        // Normal session: 60 words / 60s = 60 WPM.
        db.insert_transcript(words(60), "native".into(), 5.0, 60.0, 0, None).unwrap();
        // Pathological: 200 words but only 0.3s "voiced" (a loud transient fooled
        // the VAD). Un-clamped this single session is 40,000 WPM and would drag the
        // pooled average to ~259. The word-count floor caps its speaking time at
        // 200/400*60 = 30s → pooled = 260 words / ((60+30)/60) min = 173.3 WPM.
        db.insert_transcript(words(200), "native".into(), 3.0, 0.3, 0, None).unwrap();

        let ins = db.insights().unwrap();
        assert!(ins.avg_wpm <= 400.0 + 1e-6, "WPM must be capped, got {}", ins.avg_wpm);
        assert!(
            (ins.avg_wpm - 173.33).abs() < 0.5,
            "expected ~173.3 pooled WPM after the plausibility floor, got {}",
            ins.avg_wpm
        );
        // Raw hygiene stat is still the true (un-floored) speech sum.
        assert!(
            (ins.speech_seconds_total - 60.3).abs() < 1e-6,
            "speech_seconds_total must stay raw, got {}",
            ins.speech_seconds_total
        );
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn words_today_ignores_other_days() {
        let (db, dir) = fresh_db("today");
        // Anchor the "today" row at local noon too, so an insert landing a
        // microsecond before local midnight can't race the day boundary.
        db.insert_transcript_at(words(10), "native".into(), 0.5, 2.0, 0, None, days_ago_noon(0), None)
            .unwrap();
        db.insert_transcript_at(words(100), "native".into(), 0.5, 2.0, 0, None, days_ago_noon(3), None)
            .unwrap();
        let ins = db.insights().unwrap();
        assert_eq!(ins.words_today, 10, "words_today must exclude prior days");
        assert_eq!(ins.sessions_today, 1);
        assert_eq!(ins.total_words, 110, "total still spans all days");
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn streak_counts_consecutive_days_including_today() {
        let (db, dir) = fresh_db("streak");
        for n in [0, 1, 2] {
            db.insert_transcript_at(words(5), "native".into(), 0.5, 2.0, 0, None, days_ago_noon(n), None)
                .unwrap();
        }
        // A gap, then an old day — must not extend the current streak.
        db.insert_transcript_at(words(5), "native".into(), 0.5, 2.0, 0, None, days_ago_noon(5), None)
            .unwrap();
        let ins = db.insights().unwrap();
        assert_eq!(ins.streak_days, 3, "today + 2 prior consecutive days");
        assert_eq!(ins.longest_streak, 3);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn streak_grace_allows_missing_today() {
        let (db, dir) = fresh_db("grace");
        // Active yesterday/-2/-3 but NOT today → streak still counts (day not over).
        for n in [1, 2, 3] {
            db.insert_transcript_at(words(5), "native".into(), 0.5, 2.0, 0, None, days_ago_noon(n), None)
                .unwrap();
        }
        let ins = db.insights().unwrap();
        assert_eq!(ins.streak_days, 3, "missing-today grace: yesterday anchors the streak");
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn longest_streak_spans_gaps() {
        let (db, dir) = fresh_db("longest");
        // Current run of 2 (yesterday, -2), older run of 4 (-5..-8).
        for n in [1, 2, 5, 6, 7, 8] {
            db.insert_transcript_at(words(5), "native".into(), 0.5, 2.0, 0, None, days_ago_noon(n), None)
                .unwrap();
        }
        let ins = db.insights().unwrap();
        assert_eq!(ins.streak_days, 2, "current streak is the recent 2-day run");
        assert_eq!(ins.longest_streak, 4, "longest is the older 4-day run");
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn daily_series_is_contiguous_and_zero_filled() {
        let (db, dir) = fresh_db("series");
        db.insert_transcript_at(words(10), "native".into(), 0.5, 2.0, 0, None, days_ago_noon(0), None).unwrap();
        db.insert_transcript_at(words(20), "native".into(), 0.5, 2.0, 0, None, days_ago_noon(3), None).unwrap();
        db.insert_transcript_at(words(30), "native".into(), 0.5, 2.0, 0, None, days_ago_noon(5), None).unwrap();

        let s = db.daily_series(7).unwrap();
        assert_eq!(s.len(), 7, "series must have exactly one entry per requested day");
        // oldest first, strictly consecutive calendar days
        for w in s.windows(2) {
            let a = NaiveDate::parse_from_str(&w[0].date, "%Y-%m-%d").unwrap();
            let b = NaiveDate::parse_from_str(&w[1].date, "%Y-%m-%d").unwrap();
            assert_eq!(b, a + Duration::days(1), "days must be contiguous & ascending");
        }
        // last entry is today with 10 words; days 3 and 5 back carry their counts
        assert_eq!(s[6].words, 10, "today");
        assert_eq!(s[3].words, 20, "3 days ago"); // index 6-3
        assert_eq!(s[1].words, 30, "5 days ago"); // index 6-5
        // untouched days are zero-filled, not missing
        assert_eq!(s[5].words, 0, "yesterday had no activity → 0, not absent");
        let total: i64 = s.iter().map(|d| d.words).sum();
        assert_eq!(total, 60, "series words sum to the inserted total in-window");
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Noon on the 15th of the month `n` months ago (mid-month → no boundary flake).
    fn months_ago_15th(n: i64) -> DateTime<Utc> {
        use chrono::Datelike;
        let today = Local::now().date_naive();
        let (mut y, mut m) = (today.year(), today.month() as i32);
        for _ in 0..n {
            m -= 1;
            if m == 0 {
                m = 12;
                y -= 1;
            }
        }
        NaiveDate::from_ymd_opt(y, m as u32, 15)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn monthly_series_rolls_up_and_zero_fills() {
        let (db, dir) = fresh_db("monthly");
        // this month, and 2 months back — 1 month back is intentionally left empty.
        db.insert_transcript_at(words(10), "native".into(), 0.5, 2.0, 0, None, months_ago_15th(0), None).unwrap();
        db.insert_transcript_at(words(7), "native".into(), 0.5, 2.0, 0, None, months_ago_15th(0), None).unwrap();
        db.insert_transcript_at(words(40), "native".into(), 0.5, 2.0, 0, None, months_ago_15th(2), None).unwrap();

        let s = db.monthly_series(3).unwrap();
        assert_eq!(s.len(), 3, "one entry per requested month");
        // keys look like YYYY-MM and are contiguous ascending
        for e in &s {
            assert!(e.date.len() == 7 && e.date.as_bytes()[4] == b'-', "month key YYYY-MM: {}", e.date);
        }
        assert_eq!(s[2].words, 17, "this month sums both sessions (10+7)");
        assert_eq!(s[2].sessions, 2);
        assert_eq!(s[1].words, 0, "the empty middle month is zero-filled, not absent");
        assert_eq!(s[0].words, 40, "two months ago");
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_and_clear_actually_remove_rows() {
        // Proves the History / Dictionary / Scratchpad delete+clear paths the UI
        // calls really remove data (the §1 "verify delete works" item), at the DB
        // layer we can test headlessly.
        let (db, dir) = fresh_db("delete");

        // History: delete one, then clear all.
        let a = db.insert_transcript(words(3), "native".into(), 0.5, 1.0, 0, None).unwrap();
        db.insert_transcript(words(4), "native".into(), 0.5, 1.0, 0, None).unwrap();
        assert_eq!(db.list_transcripts(50, None).unwrap().len(), 2);
        db.delete_transcript(&a.id).unwrap();
        let after = db.list_transcripts(50, None).unwrap();
        assert_eq!(after.len(), 1, "delete_transcript did not remove the row");
        assert!(!after.iter().any(|e| e.id == a.id), "deleted id still present");
        db.clear_transcripts().unwrap();
        assert_eq!(db.list_transcripts(50, None).unwrap().len(), 0, "clear_transcripts failed");

        // Dictionary: add then delete by id.
        let term = db.add_dictionary_term("Anthropic".into(), None).unwrap();
        assert!(db.list_dictionary().unwrap().iter().any(|d| d.id == term.id));
        db.delete_dictionary_term(&term.id).unwrap();
        assert!(
            !db.list_dictionary().unwrap().iter().any(|d| d.id == term.id),
            "delete_dictionary_term did not remove the term"
        );

        // Scratchpad: save then delete by id.
        let note = db.save_scratch(None, "Title".into(), "body".into()).unwrap();
        assert!(db.list_scratch().unwrap().iter().any(|n| n.id == note.id));
        db.delete_scratch(&note.id).unwrap();
        assert!(
            !db.list_scratch().unwrap().iter().any(|n| n.id == note.id),
            "delete_scratch did not remove the note"
        );

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// YV77 — the export writes the WHOLE history, oldest first.
    ///
    /// The old export was `list_transcripts` capped at 10,000, i.e. `ORDER BY
    /// created_at DESC LIMIT`, so the cap fell on the OLDEST rows: at 10,050
    /// takes the "backup" silently omitted the first 50 a user ever dictated.
    /// The seed size here is deliberately just over that old cap — replaying
    /// the pre-YV77 body against this same seed yields 10,000 rows with
    /// t-00000..t-00049 missing and t-10049 first.
    #[test]
    fn export_streams_every_transcript() {
        const N: usize = 10_050;
        let (db, dir) = fresh_db("export");

        // Seed through the row writer directly, in ONE transaction: the ids can
        // then embed their insertion index (production ids are random UUIDs) and
        // a 10k+ seed stays fast.
        let base = days_ago_noon(1);
        {
            let mut conn = db.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            for i in 0..N {
                let entry = TranscriptEntry {
                    id: format!("t-{i:05}"),
                    text: format!("transcript {i}"),
                    backend: "native".into(),
                    asr_seconds: 0.5,
                    speech_seconds: 1.0,
                    pipeline_ms: 0,
                    word_count: 2,
                    created_at: base + Duration::seconds(i as i64),
                    source_app: None,
                    raw_text: None,
                };
                db.insert_transcript_row_tx(&tx, &entry).unwrap();
            }
            tx.commit().unwrap();
        }

        // Drive the same writer the `export_history` command drives.
        let out = dir.join("export.jsonl");
        let count = {
            let mut w = std::io::BufWriter::new(std::fs::File::create(&out).unwrap());
            let n = db.write_transcripts_jsonl(&mut w).unwrap();
            std::io::Write::flush(&mut w).unwrap();
            n
        };

        let body = std::fs::read_to_string(&out).unwrap();
        let lines: Vec<&str> = body.lines().collect();

        // (a) every row reached the file — not 10,000 of them.
        assert_eq!(count, N, "writer reported the wrong row count");
        assert_eq!(lines.len(), N, "export dropped rows (the old cap was 10,000)");

        // (b) oldest first — line 1 is exactly the row the DESC+LIMIT cap discarded.
        let first: TranscriptEntry = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first.id, "t-00000", "export is not oldest-first");
        let last: TranscriptEntry = serde_json::from_str(lines[N - 1]).unwrap();
        assert_eq!(last.id, format!("t-{:05}", N - 1), "newest row is not last");

        // (c) real JSON Lines: every line stands alone as a TranscriptEntry.
        for (i, line) in lines.iter().enumerate() {
            let e: TranscriptEntry = serde_json::from_str(line)
                .unwrap_or_else(|err| panic!("line {i} is not a TranscriptEntry: {err}"));
            assert_eq!(e.id, format!("t-{i:05}"), "line {i} is out of order");
        }

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Non-overlapping byte occurrences of `needle` in the file at `path`.
    /// A missing file counts 0 — SQLite removes the -wal / -shm on a clean
    /// close, and a file that is gone obviously leaks nothing.
    fn sentinel_hits(path: &std::path::Path, needle: &str) -> usize {
        let bytes = std::fs::read(path).unwrap_or_default();
        let needle = needle.as_bytes();
        if needle.is_empty() || bytes.len() < needle.len() {
            return 0;
        }
        let (mut hits, mut i) = (0usize, 0usize);
        while i + needle.len() <= bytes.len() {
            if &bytes[i..i + needle.len()] == needle {
                hits += 1;
                i += needle.len();
            } else {
                i += 1;
            }
        }
        hits
    }

    /// YV78 — "Clear all transcript history" must leave NOTHING readable on
    /// disk, not merely nothing queryable. Measured on 6aec852 (pre-fix), this
    /// exact test found 197 of the 200 sentinels still byte-readable in
    /// wilson_voice.db after clear + the app-exit wal_checkpoint(TRUNCATE).
    #[test]
    fn clear_history_leaves_no_plaintext_on_disk() {
        const SENTINEL: &str = "MYSOCIALSECURITYIS-078051120-PLAINTEXTSENTINEL";
        let (db, dir) = fresh_db("yv78-scrub");
        let path = dir.join("wilson_voice.db");

        for i in 0..200 {
            db.insert_transcript(
                format!("take {i} my number is {SENTINEL} thanks"),
                "native".into(),
                1.0,
                2.0,
                10,
                Some("Test".into()),
            )
            .unwrap();
        }

        // Guard: the words really did reach the disk. Without this the test
        // would still pass against a DB that never persisted anything.
        db.checkpoint();
        assert!(
            sentinel_hits(&path, SENTINEL) > 0,
            "sentinel never reached the .db — this test would prove nothing"
        );

        db.clear_transcripts().unwrap();
        // The app-exit path (lib.rs) — pre-fix this is what folded the residue
        // out of the WAL and INTO the main .db.
        db.checkpoint();
        drop(db);

        let mut total = 0usize;
        for suffix in ["", "-wal", "-shm"] {
            let mut p = path.clone().into_os_string();
            p.push(suffix);
            total += sentinel_hits(std::path::Path::new(&p), SENTINEL);
        }
        assert_eq!(
            total, 0,
            "transcript text is still byte-readable on disk after clear_transcripts"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- YV52 dictation recovery -----------------------------------------

    /// A failed take becomes a RECOVERABLE row: the WAV is remembered with the
    /// error the user saw, and it does not pollute transcript history.
    #[test]
    fn failed_take_becomes_a_recoverable_row() {
        let (db, dir) = fresh_db("yv52-failed");
        let wav = dir.join("take.wav");
        std::fs::write(&wav, b"riff").unwrap();

        let row = db
            .record_failed_dictation(
                &wav,
                3.5,
                "No speech model installed — open Settings → Models and download one.",
                Some("Mail".into()),
            )
            .unwrap();

        let listed = db.list_failed_dictations().unwrap();
        assert_eq!(listed.len(), 1, "failed take was not kept for recovery");
        assert_eq!(listed[0].id, row.id);
        assert_eq!(listed[0].wav_path, wav.to_string_lossy());
        assert_eq!(listed[0].speech_seconds, 3.5);
        assert!(listed[0].error.contains("No speech model installed"));
        assert_eq!(listed[0].source_app.as_deref(), Some("Mail"));
        // A failure is NOT a dictation — nothing lands in history yet.
        assert!(db.list_transcripts(50, None).unwrap().is_empty());

        // Discard hands back the orphaned WAV so the caller can unlink it.
        let orphan = db.delete_failed_dictation(&row.id).unwrap();
        assert_eq!(orphan.as_deref(), Some(wav.to_string_lossy().as_ref()));
        assert!(db.list_failed_dictations().unwrap().is_empty());

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A successful retry CONVERTS the row: it leaves the recoverable list and
    /// enters history as a normal entry, keeping the moment it was spoken.
    #[test]
    fn successful_retry_converts_the_row_to_history() {
        let (db, dir) = fresh_db("yv52-retry");
        let wav = dir.join("take.wav");
        std::fs::write(&wav, b"riff").unwrap();
        let spoken = days_ago_noon(2);

        let row = db
            .record_failed_dictation(&wav, 4.0, "Empty transcript", Some("Notes".into()))
            .unwrap();
        // Backdate to when the take was actually spoken (production stamps the
        // row at the moment of failure, which is the same thing).
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "UPDATE failed_dictations SET created_at = ?1 WHERE id = ?2",
                params![spoken.to_rfc3339(), row.id],
            )
            .unwrap();
        }

        let entry = db
            .convert_failed_dictation(
                &row.id,
                "ship the recovery item".into(),
                "native".into(),
                1.25,
                Some("ship the uh recovery item".into()),
            )
            .unwrap();

        assert_eq!(entry.text, "ship the recovery item");
        assert_eq!(entry.raw_text.as_deref(), Some("ship the uh recovery item"));
        // Carried over from the failed take, not invented at retry time.
        assert_eq!(entry.speech_seconds, 4.0);
        assert_eq!(entry.source_app.as_deref(), Some("Notes"));
        assert_eq!(entry.created_at, spoken);

        let history = db.list_transcripts(50, None).unwrap();
        assert_eq!(history.len(), 1, "recovered take is missing from history");
        assert_eq!(history[0].id, entry.id);
        assert!(
            db.list_failed_dictations().unwrap().is_empty(),
            "a recovered take must not stay in the failed list"
        );
        // Retrying the same id twice can't duplicate the entry.
        assert!(db
            .convert_failed_dictation(&row.id, "again".into(), "native".into(), 1.0, None)
            .is_err());

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Retention: failed takes older than the 7-day cutoff are purged (and their
    /// WAVs handed back for deletion); anything inside the window survives.
    #[test]
    fn purge_honors_the_seven_day_cutoff() {
        let (db, dir) = fresh_db("yv52-purge");
        let old_wav = dir.join("old.wav");
        let fresh_wav = dir.join("fresh.wav");
        std::fs::write(&old_wav, b"riff").unwrap();
        std::fs::write(&fresh_wav, b"riff").unwrap();

        let old = db
            .record_failed_dictation(&old_wav, 1.0, "Empty transcript", None)
            .unwrap();
        let recent = db
            .record_failed_dictation(&fresh_wav, 1.0, "Empty transcript", None)
            .unwrap();
        {
            let conn = db.conn.lock().unwrap();
            // 8 days old — one day past retention; and 6 days old — inside it.
            for (id, age) in [(&old.id, 8), (&recent.id, 6)] {
                conn.execute(
                    "UPDATE failed_dictations SET created_at = ?1 WHERE id = ?2",
                    params![(Utc::now() - Duration::days(age)).to_rfc3339(), id],
                )
                .unwrap();
            }
        }

        let cutoff = Utc::now() - Duration::days(FAILED_TAKE_RETENTION_DAYS);
        let purged = db.purge_failed_dictations(cutoff).unwrap();
        assert_eq!(
            purged,
            vec![old_wav.to_string_lossy().to_string()],
            "purge must return exactly the expired WAVs to unlink"
        );

        let left = db.list_failed_dictations().unwrap();
        assert_eq!(left.len(), 1, "purge took a take inside the 7-day window");
        assert_eq!(left[0].id, recent.id);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- YV68 one transaction per dictation -------------------------------

    /// A broken `daily_stats` rollup is a STATS problem, never a lost dictation:
    /// the take is already durable and its text is already in the user's app, so
    /// the call still succeeds and nothing is parked as a failed take.
    #[test]
    fn rollup_failure_never_fails_the_dictation() {
        let (db, dir) = fresh_db("yv68-rollup");

        db.insert_transcript_at(
            "first take".into(),
            "native".into(),
            1.0,
            2.0,
            100,
            None,
            Utc::now(),
            None,
        )
        .unwrap();

        // Break the rollup exactly as a real failure would: recompute_daily_stats
        // can no longer touch its table.
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("DROP TABLE daily_stats", []).unwrap();
        }

        let second = db.insert_transcript_at(
            "second take".into(),
            "native".into(),
            1.0,
            2.0,
            100,
            None,
            Utc::now(),
            None,
        );
        assert!(
            second.is_ok(),
            "a rollup failure was reported as a failed dictation: {second:?}"
        );
        assert_eq!(
            db.list_transcripts(10, None).unwrap().len(),
            2,
            "the saved take is missing from history"
        );
        assert!(
            db.list_failed_dictations().unwrap().is_empty(),
            "a saved take must never be parked as a recoverable failure"
        );

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The harvest and the transcript row are ONE unit: if the INSERT fails, the
    /// dictionary tokens that text would have taught roll back with it.
    #[test]
    fn insert_transcript_at_is_atomic() {
        let (db, dir) = fresh_db("yv68-atomic");

        // The row id is minted inside insert_transcript_at, so manufacture the
        // same class of failure by hand: a duplicate-key conflict on the
        // transcripts INSERT, via a unique index the second row must violate.
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("CREATE UNIQUE INDEX yv68_one_row ON transcripts(backend)", [])
                .unwrap();
        }
        db.insert_transcript_at(
            "first take".into(),
            "native".into(),
            1.0,
            2.0,
            0,
            None,
            Utc::now(),
            None,
        )
        .unwrap();

        let doomed = db.insert_transcript_at(
            "deploy the yv68RollbackToken build".into(),
            "native".into(),
            1.0,
            2.0,
            0,
            None,
            Utc::now(),
            None,
        );
        assert!(doomed.is_err(), "the duplicate key should have failed the INSERT");
        assert_eq!(
            db.list_transcripts(10, None).unwrap().len(),
            1,
            "a failed INSERT still wrote a row"
        );
        assert!(
            !db.list_dictionary()
                .unwrap()
                .iter()
                .any(|d| d.term == "yv68RollbackToken"),
            "the harvest committed even though the transcript INSERT failed"
        );

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The retry SWAP is atomic: if dropping the failed row fails, the transcript
    /// row it would have become rolls back too — never both lists, never neither.
    #[test]
    fn convert_failed_dictation_never_leaves_both_rows() {
        let (db, dir) = fresh_db("yv68-convert");
        let wav = dir.join("take.wav");
        std::fs::write(&wav, b"riff").unwrap();

        let row = db
            .record_failed_dictation(&wav, 4.0, "Empty transcript", Some("Notes".into()))
            .unwrap();

        // Break ONLY the DELETE. Dropping the table would break the lookup that
        // runs first, which would prove nothing about the INSERT/DELETE pair.
        {
            let conn = db.conn.lock().unwrap();
            conn.execute_batch(
                "CREATE TRIGGER yv68_block_delete BEFORE DELETE ON failed_dictations
                 BEGIN SELECT RAISE(ABORT, 'yv68 test: delete blocked'); END;",
            )
            .unwrap();
        }

        let before = db.list_transcripts(50, None).unwrap().len();
        let out = db.convert_failed_dictation(
            &row.id,
            "ship the recovery item".into(),
            "native".into(),
            1.0,
            None,
        );
        assert!(out.is_err(), "a failed DELETE must fail the retry");
        assert_eq!(
            db.list_transcripts(50, None).unwrap().len(),
            before,
            "the transcript INSERT did not roll back with the failed DELETE"
        );
        assert_eq!(
            db.list_failed_dictations().unwrap().len(),
            1,
            "the take must stay recoverable when its retry did not commit"
        );

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// End to end over both APIs with the rollup broken: one utterance is one
    /// row. The old bug turned a saved take into a "failed" one, and Retry then
    /// wrote a second row for the same words.
    #[test]
    fn retry_after_rollup_failure_does_not_duplicate() {
        let (db, dir) = fresh_db("yv68-retry-dup");
        {
            let conn = db.conn.lock().unwrap();
            conn.execute("DROP TABLE daily_stats", []).unwrap();
        }

        // A live take still saves, so the caller never parks a recovery WAV for
        // it — there is nothing to retry into a duplicate.
        db.insert_transcript_at(
            "the live take".into(),
            "native".into(),
            1.0,
            2.0,
            0,
            Some("Notes".into()),
            Utc::now(),
            None,
        )
        .unwrap();
        assert!(
            db.list_failed_dictations().unwrap().is_empty(),
            "a saved take was parked for retry"
        );

        // And a genuinely failed take still converts exactly once, rollup or no.
        let wav = dir.join("take.wav");
        std::fs::write(&wav, b"riff").unwrap();
        let row = db
            .record_failed_dictation(&wav, 3.0, "Empty transcript", Some("Notes".into()))
            .unwrap();
        db.convert_failed_dictation(
            &row.id,
            "the recovered take".into(),
            "native".into(),
            1.0,
            None,
        )
        .unwrap();
        assert!(
            db.convert_failed_dictation(
                &row.id,
                "the recovered take".into(),
                "native".into(),
                1.0,
                None
            )
            .is_err(),
            "a second retry of the same take must not write a second row"
        );

        let history = db.list_transcripts(50, None).unwrap();
        assert_eq!(history.len(), 2, "one utterance produced more than one row");
        assert_eq!(
            history
                .iter()
                .filter(|e| e.text == "the recovered take")
                .count(),
            1
        );
        assert_eq!(history.iter().filter(|e| e.text == "the live take").count(), 1);

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// YV48 — snippet CRUD round-trips, and only ENABLED rows reach the matcher.
    #[test]
    fn snippet_crud_and_only_enabled_rules_are_served() {
        let (db, dir) = fresh_db("snippets");

        let email = db
            .add_snippet("my email".into(), "wilson@drivia.consulting".into())
            .unwrap();
        let sig = db
            .add_snippet("sign off".into(), "Thanks,\nWilson".into())
            .unwrap();
        assert_eq!(db.list_snippets().unwrap().len(), 2);
        // Multi-line expansions survive the round-trip verbatim.
        assert_eq!(
            db.list_snippets()
                .unwrap()
                .iter()
                .find(|s| s.id == sig.id)
                .map(|s| s.expansion.clone()),
            Some("Thanks,\nWilson".into())
        );

        // Edit in place.
        db.update_snippet(&email.id, "my address".into(), "1 Main St".into())
            .unwrap();
        let rules = db.snippet_rules().unwrap();
        assert!(rules
            .iter()
            .any(|r| r.trigger == "my address" && r.expansion == "1 Main St"));

        // Disabled rows stay listed but are never served to the matcher.
        db.set_snippet_enabled(&sig.id, false).unwrap();
        assert_eq!(db.list_snippets().unwrap().len(), 2);
        let rules = db.snippet_rules().unwrap();
        assert_eq!(
            rules.len(),
            1,
            "disabled snippet was still served: {rules:?}"
        );
        assert!(!rules.iter().any(|r| r.trigger == "sign off"));

        // Empty trigger / expansion are rejected, delete removes the row.
        assert!(db.add_snippet("  ".into(), "x".into()).is_err());
        assert!(db.add_snippet("trigger".into(), "  ".into()).is_err());
        db.delete_snippet(&email.id).unwrap();
        assert!(
            !db.list_snippets().unwrap().iter().any(|s| s.id == email.id),
            "delete_snippet did not remove the snippet"
        );

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
