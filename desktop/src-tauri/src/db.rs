//! Local data stack for Wilson Voice.
//!
//! SQLite in WAL mode + FTS5 full-text search — same pattern used by
//! OpenWhispr, Muesli, and sflow for dictation history.
//! GraphQL is overkill for a single-user desktop app; typed Tauri commands
//! over this SQLite layer give fast retrieval without a network hop.

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
            ",
        )
        .map_err(|e| OpenErr::classify("schema", e))?;

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
    pub fn recompute_daily_stats(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM daily_stats", [])
            .map_err(|e| e.to_string())?;
        // Group by local calendar day derived from created_at (ISO stored as UTC).
        let mut stmt = conn
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
            conn.execute(
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
        Ok(())
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
        // Preserve the raw ASR transcript (YV10); default to the polished `text`
        // when the caller supplies none (raw == polished, e.g. cleanup off).
        let raw = raw_text
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| text.clone());
        let entry = TranscriptEntry {
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
        };
        let _ = self.learn_from_transcript(&entry.text);
        {
            let conn = self.conn.lock().map_err(|e| e.to_string())?;
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
        }
        // Source-of-truth rollup (never trust incremental counters alone)
        self.recompute_daily_stats()?;
        Ok(entry)
    }

    /// Local "fine-tune" signal: learn coding jargon / proper nouns from transcripts.
    /// Stores high-value tokens in the dictionary table (hits++). Applied on next ASR polish.
    pub fn learn_from_transcript(&self, text: &str) -> Result<usize, String> {
        let mut learned = 0usize;
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

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

    pub fn clear_transcripts(&self) -> Result<(), String> {
        {
            let conn = self.conn.lock().map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM transcripts", [])
                .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM daily_stats", [])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Starred terms first (always-bias), then usage-ranked by hits (YV47).
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

    /// Export transcripts as JSON lines for backup / LoRA corpus later.
    pub fn export_transcripts_json(&self) -> Result<String, String> {
        let entries = self.list_transcripts(10_000, None)?;
        serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())
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
}
