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
    pub asr_seconds: f64,
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
    pub avg_wpm: f64,
    pub streak_days: i64,
    pub longest_streak: i64,
    pub words_last_7: Vec<DayCount>,
    pub top_apps: Vec<AppCount>,
    pub avg_asr_seconds: f64,
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

impl Database {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(&path).map_err(|e| format!("sqlite open: {e}"))?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA temp_store = MEMORY;
            PRAGMA mmap_size = 268435456;
            ",
        )
        .map_err(|e| format!("pragma: {e}"))?;

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
        .map_err(|e| format!("schema: {e}"))?;

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

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert_transcript(
        &self,
        text: String,
        backend: String,
        asr_seconds: f64,
        source_app: Option<String>,
    ) -> Result<TranscriptEntry, String> {
        let entry = TranscriptEntry {
            id: Uuid::new_v4().to_string(),
            word_count: word_count(&text),
            text,
            backend,
            asr_seconds,
            created_at: Utc::now(),
            source_app,
        };
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO transcripts (id, text, backend, asr_seconds, word_count, source_app, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.id,
                entry.text,
                entry.backend,
                entry.asr_seconds,
                entry.word_count,
                entry.source_app,
                entry.created_at.to_rfc3339(),
            ],
        )
        .map_err(|e| e.to_string())?;

        let day = Local::now().date_naive().to_string();
        let asr_ms = (entry.asr_seconds * 1000.0) as i64;
        conn.execute(
            "INSERT INTO daily_stats (day, words, sessions, asr_ms)
             VALUES (?1, ?2, 1, ?3)
             ON CONFLICT(day) DO UPDATE SET
               words = words + excluded.words,
               sessions = sessions + 1,
               asr_ms = asr_ms + excluded.asr_ms",
            params![day, entry.word_count, asr_ms],
        )
        .map_err(|e| e.to_string())?;

        Ok(entry)
    }

    pub fn list_transcripts(&self, limit: i64, query: Option<String>) -> Result<Vec<TranscriptEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let q = query.unwrap_or_default().trim().to_string();

        let mut out = Vec::new();
        if q.is_empty() {
            let mut stmt = conn
                .prepare(
                    "SELECT id, text, backend, asr_seconds, word_count, source_app, created_at
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
                    "SELECT t.id, t.text, t.backend, t.asr_seconds, t.word_count, t.source_app, t.created_at
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
                            "SELECT id, text, backend, asr_seconds, word_count, source_app, created_at
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
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM transcripts WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn clear_transcripts(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM transcripts", [])
            .map_err(|e| e.to_string())?;
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

        // Approx WPM: words / (sum asr seconds / 60) — dictation pace
        let total_asr: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(asr_seconds),0) FROM transcripts",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0.0);
        let avg_wpm = if total_asr > 0.5 {
            (total_words as f64) / (total_asr / 60.0)
        } else {
            0.0
        };

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

        // streak
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

        // top apps
        let mut top_apps = Vec::new();
        {
            let mut stmt = conn
                .prepare(
                    "SELECT COALESCE(source_app, 'Unknown') as app,
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

        let _ = days; // silence if unused path
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
            let _ = self.insert_transcript(e.text, e.backend, e.asr_seconds, None)?;
            n += 1;
        }
        let _ = std::fs::rename(&json_path, json_path.with_extension("json.migrated"));
        Ok(n)
    }
}

fn map_transcript(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranscriptEntry> {
    Ok(TranscriptEntry {
        id: row.get(0)?,
        text: row.get(1)?,
        backend: row.get(2)?,
        asr_seconds: row.get(3)?,
        word_count: row.get(4)?,
        source_app: row.get(5)?,
        created_at: parse_dt(row.get::<_, String>(6)?),
    })
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
