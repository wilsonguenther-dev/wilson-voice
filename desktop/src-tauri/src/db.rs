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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictEntry {
    pub id: String,
    pub term: String,
    pub preferred: Option<String>,
    pub hits: i64,
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

/// Tokens worth learning for STT polish: camelCase, snake_case, PascalCase, ALLCAPS, product names.
fn extract_learnable_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace() || ".,!?;:()[]{}\"'`".contains(c)) {
        let t = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
        if t.len() < 3 || t.len() > 48 {
            continue;
        }
        // skip pure lowercase common words
        let has_upper = t.chars().any(|c| c.is_uppercase());
        let has_digit = t.chars().any(|c| c.is_ascii_digit());
        let has_under = t.contains('_') || t.contains('-');
        let camel = t.chars().any(|c| c.is_lowercase()) && has_upper;
        if !(has_upper || has_digit || has_under || camel) {
            continue;
        }
        // skip pure numbers
        if t.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        out.push(t.to_string());
    }
    out.sort();
    out.dedup();
    out.into_iter().take(40).collect()
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
              backend TEXT NOT NULL DEFAULT 'mlx',
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
        let _ = conn.execute(
            "ALTER TABLE daily_stats ADD COLUMN speech_ms INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE daily_stats ADD COLUMN pipeline_ms INTEGER NOT NULL DEFAULT 0",
            [],
        );

        // Seed default dictionary terms useful for Wilson
        {
            let defaults = [
                "Drivia",
                "Wilson",
                "Jeisil",
                "Aidan",
                "Supabase",
                "Vercel",
                "Whisper",
                "Tauri",
                "Kokori",
            ];
            let now = Utc::now().to_rfc3339();
            for term in defaults {
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO dictionary (id, term, preferred, hits, created_at)
                     VALUES (?1, ?2, NULL, 0, ?3)",
                    params![Uuid::new_v4().to_string(), term, now],
                );
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
        let entry = TranscriptEntry {
            id: Uuid::new_v4().to_string(),
            word_count: word_count(&text),
            text,
            backend,
            asr_seconds,
            speech_seconds: speech_seconds.max(0.0),
            pipeline_ms: pipeline_ms.max(0),
            created_at: Utc::now(),
            source_app,
        };
        let _ = self.learn_from_transcript(&entry.text);
        {
            let conn = self.conn.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO transcripts
                 (id, text, backend, asr_seconds, speech_seconds, pipeline_ms,
                  word_count, source_app, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
                    "INSERT INTO dictionary (id, term, preferred, hits, created_at)
                     VALUES (?1, ?2, ?2, 1, ?3)
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
                            COALESCE(pipeline_ms,0), word_count, source_app, created_at
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
                            COALESCE(t.pipeline_ms,0), t.word_count, t.source_app, t.created_at
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
                                    COALESCE(pipeline_ms,0), word_count, source_app, created_at
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

    pub fn list_dictionary(&self) -> Result<Vec<DictEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, term, preferred, hits, created_at FROM dictionary
                 ORDER BY hits DESC, term COLLATE NOCASE ASC",
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
        };
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO dictionary (id, term, preferred, hits, created_at)
             VALUES (?1, ?2, ?3, 0, ?4)
             ON CONFLICT(term) DO UPDATE SET preferred = excluded.preferred",
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

    pub fn delete_dictionary_term(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM dictionary WHERE id = ?1", params![id])
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

        // Fallback: if daily_stats empty but we have transcripts today, sum live
        let (words_today, sessions_today) = if words_today == 0 && total_sessions > 0 {
            let mut w = 0i64;
            let mut s = 0i64;
            let mut stmt = conn
                .prepare("SELECT created_at, word_count FROM transcripts")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (created, wc) = row.map_err(|e| e.to_string())?;
                let day = parse_dt(created)
                    .with_timezone(&Local)
                    .date_naive()
                    .to_string();
                if day == today {
                    w += wc;
                    s += 1;
                }
            }
            (w, s)
        } else {
            (words_today, sessions_today)
        };

        // WPM = words_with_speech / (speech_minutes). Exclude rows without speech_seconds.
        // Never use asr_seconds as a speaking-time proxy (that was a bad heuristic).
        let (wpm_words, total_speech, wpm_sessions): (i64, f64, i64) = conn
            .query_row(
                "SELECT COALESCE(SUM(word_count),0),
                        COALESCE(SUM(speech_seconds),0),
                        COUNT(*)
                 FROM transcripts
                 WHERE speech_seconds > 0.05",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0.0, 0));
        let avg_wpm = if total_speech > 0.5 && wpm_words > 0 {
            (wpm_words as f64) / (total_speech / 60.0)
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
    // id, text, backend, asr_seconds, speech_seconds, pipeline_ms, word_count, source_app, created_at
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
            "mlx".into(),
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
    fn healthy_db_reopen_not_quarantined() {
        let dir = std::env::temp_dir().join(format!("wv-reopen-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wilson_voice.db");

        // Create + populate, then close.
        {
            let db = Database::open(path.clone()).unwrap();
            db.insert_transcript("keep this".into(), "mlx".into(), 1.0, 1.0, 10, None)
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
}
