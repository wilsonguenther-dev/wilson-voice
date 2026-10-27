//! DB-C — History, FTS search and export hold up at real volume, and Clear
//! History still destroys the words.
//!
//! History is the surface the license promises to keep working forever
//! (`KEEP_FOREVER_LINE`), so it is proven here at 10,000 takes rather than at
//! the handful the unit tests use. Every table is seeded through the REAL take
//! path (`insert_transcript_at`: the dictionary harvest, the row, the daily
//! rollup, one transaction each), and every query plan read below is the plan
//! of the SQL constant `db.rs` actually executes.
//!
//! No test here prints transcript text: only counts, timings and query plans.

mod support;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{Connection, StatementStatus};
use support::temp_dir;
use wilson_voice_lib::db::{
    Database, HistoryCursor, TranscriptEntry, DAILY_SERIES_SQL, DAY_ROLLUP_SQL,
    HISTORY_FIRST_PAGE_SQL, HISTORY_NEXT_PAGE_SQL, SEARCH_FIRST_PAGE_SQL, SEARCH_NEXT_PAGE_SQL,
};

const TAKES: usize = 10_000;
/// Takes are committed in bursts of this many that share ONE timestamp, so
/// every page size below lands boundaries inside a tie.
const TIE: usize = 3;
const LONG_FORM_WORDS: usize = 4_000;
/// A word no generated take contains, planted in a few takes so Clear History
/// has a known needle to prove is gone. Lower-case: FTS5's unicode61 folds case.
const NEEDLE: &str = "quokkazanzibar";
const DB_FILE: &str = "wilson_voice.db";

const VOCAB: &[&str] = &[
    "the",
    "and",
    "to",
    "of",
    "a",
    "in",
    "that",
    "is",
    "for",
    "it",
    "on",
    "with",
    "as",
    "we",
    "this",
    "be",
    "at",
    "have",
    "from",
    "or",
    "meeting",
    "deploy",
    "branch",
    "review",
    "customer",
    "invoice",
    "schedule",
    "tomorrow",
    "migration",
    "latency",
    "pipeline",
    "dictation",
    "window",
    "release",
    "budget",
    "draft",
    "follow",
    "up",
    "email",
    "call",
    "notes",
    "design",
    "ticket",
    "should",
    "could",
    "would",
    "think",
    "maybe",
    "really",
    "quick",
    "send",
    "check",
    "update",
    "Postgres",
    "Kubernetes",
    "TypeScript",
    "Supabase",
    "React",
    "Tauri",
    "SQLite",
    "Vercel",
    "please",
    "thanks",
    "today",
    "next",
    "week",
    "after",
    "before",
    "because",
    "about",
    "team",
];

struct Seeded {
    template: PathBuf,
    ids: Vec<String>,
    /// `daily_stats` exactly as the per-take incremental rollup left it,
    /// captured BEFORE any reopen (open() runs a full rebuild).
    incremental_rollup: Vec<(String, i64, i64, i64, i64, i64)>,
}

/// Deterministic, dependency-free PRNG (SplitMix64).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn sentence(rng: &mut Rng, words: usize) -> String {
    let mut out = Vec::with_capacity(words);
    for _ in 0..words {
        out.push(VOCAB[rng.below(VOCAB.len())]);
    }
    out.join(" ")
}

/// Realistic dictation lengths: mostly a sentence or two, a tail of
/// paragraphs, and ONE 4,000-word long-form take (the Y3 long-take shape).
fn take_len(rng: &mut Rng) -> usize {
    match rng.below(100) {
        0..=59 => 4 + rng.below(30),
        60..=89 => 30 + rng.below(90),
        _ => 120 + rng.below(400),
    }
}

fn rollup(path: &Path) -> Vec<(String, i64, i64, i64, i64, i64)> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT day, words, sessions, asr_ms, speech_ms, pipeline_ms
             FROM daily_stats ORDER BY day",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    rows
}

fn seeded() -> &'static Seeded {
    static SEEDED: OnceLock<Seeded> = OnceLock::new();
    SEEDED.get_or_init(|| {
        let dir = temp_dir("dbc-template");
        let path = dir.join(DB_FILE);
        let db = Database::open(path.clone()).expect("open template db");
        let mut rng = Rng(0xDBC0_2026);
        let base = Utc.with_ymd_and_hms(2026, 3, 1, 8, 0, 0).unwrap();
        let mut ids = Vec::with_capacity(TAKES + 1);
        let mut first_100 = Duration::ZERO;
        let mut last_100 = Duration::ZERO;
        let started = Instant::now();
        for i in 0..TAKES {
            // Bursts of TIE takes share one instant; bursts are 37 minutes apart,
            // so the history spans ~85 days and the day rollup has real work.
            let at: DateTime<Utc> = base + chrono::Duration::minutes(37 * (i / TIE) as i64);
            let len = take_len(&mut rng);
            let mut text = sentence(&mut rng, len);
            if i % 2_000 == 17 {
                text.push(' ');
                text.push_str(NEEDLE);
            }
            let t = Instant::now();
            let e = db
                .insert_transcript_at(
                    text,
                    "native".into(),
                    0.4,
                    3.2,
                    180,
                    Some("com.apple.TextEdit".into()),
                    at,
                    None,
                )
                .expect("insert take");
            let dt = t.elapsed();
            if i < 100 {
                first_100 += dt;
            } else if i >= TAKES - 100 {
                last_100 += dt;
            }
            ids.push(e.id);
        }
        // The long-form take, newest, through the same path.
        let long = sentence(&mut rng, LONG_FORM_WORDS);
        let e = db
            .insert_transcript_at(
                long,
                "native".into(),
                95.0,
                1_500.0,
                2_400,
                Some("com.apple.Notes".into()),
                base + chrono::Duration::days(120),
                None,
            )
            .expect("insert long-form take");
        assert_eq!(e.word_count, LONG_FORM_WORDS as i64);
        ids.push(e.id);
        eprintln!(
            "seed: {} takes in {:?}; take-path insert mean first 100 = {:?}, last 100 = {:?}",
            ids.len(),
            started.elapsed(),
            first_100 / 100,
            last_100 / 100
        );
        db.checkpoint();
        drop(db);
        let incremental_rollup = rollup(&path);
        Seeded {
            template: path,
            ids,
            incremental_rollup,
        }
    })
}

/// A private copy of the seeded DB, so tests can run in parallel and mutate.
fn copy_of_seeded(tag: &str) -> (PathBuf, Database) {
    let s = seeded();
    let dir = temp_dir(tag);
    let path = dir.join(DB_FILE);
    std::fs::copy(&s.template, &path).expect("copy template");
    let db = Database::open(path.clone()).expect("open copy");
    (path, db)
}

fn plan(conn: &Connection, sql: &str, n_params: usize) -> String {
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let params: Vec<rusqlite::types::Value> = (0..n_params)
        .map(|_| rusqlite::types::Value::Text("x".into()))
        .collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params), |r| {
            r.get::<_, String>(3)
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect::<Vec<_>>();
    rows.join(" | ")
}

/// VM steps a statement takes to produce its rows — SQLite's own count of the
/// work done, independent of how fast this machine is.
fn vm_steps(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> (usize, i32) {
    let mut stmt = conn.prepare(sql).unwrap();
    let n = stmt.query_map(params, |_| Ok(())).unwrap().count();
    (n, stmt.get_status(StatementStatus::VmStep))
}

fn cleanup(path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn first_page_query_is_bounded() {
    let (path, db) = copy_of_seeded("dbc-bounded");
    let conn = Connection::open(&path).unwrap();
    let total: i64 = conn
        .query_row("SELECT count(*) FROM transcripts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total as usize, TAKES + 1);

    // The History view's first page: LIMIT-bounded, walked off the index.
    assert!(HISTORY_FIRST_PAGE_SQL.contains("LIMIT ?1"));
    let p = plan(&conn, HISTORY_FIRST_PAGE_SQL, 1);
    eprintln!("first page plan: {p}");
    assert!(p.contains("USING INDEX idx_transcripts_created"), "{p}");
    assert!(
        !p.contains("USE TEMP B-TREE FOR ORDER BY"),
        "full sort: {p}"
    );

    // Work done is a function of the page, not of the 10,001 rows.
    let (rows, steps) = vm_steps(&conn, HISTORY_FIRST_PAGE_SQL, [200i64]);
    // The same statement asked for every row: what an unbounded read costs.
    let (all, full_steps) = vm_steps(&conn, HISTORY_FIRST_PAGE_SQL, [i64::MAX]);
    assert_eq!(all, TAKES + 1);
    eprintln!("first page: {rows} rows in {steps} vm steps (all {all} rows take {full_steps})");
    assert_eq!(rows, 200);
    assert!(
        steps * 10 < full_steps,
        "first page took {steps} vm steps; a full read takes {full_steps}"
    );

    // Every later page seeks into the index rather than re-walking from the top.
    let p = plan(&conn, HISTORY_NEXT_PAGE_SQL, 3);
    eprintln!("next page plan: {p}");
    assert!(
        p.contains("SEARCH transcripts USING INDEX idx_transcripts_created (created_at<?)"),
        "{p}"
    );

    // The same through the API the command calls.
    let t = Instant::now();
    let page = db.list_transcripts_page(200, None, None).unwrap();
    eprintln!("first page via API: {:?}", t.elapsed());
    assert_eq!(page.len(), 200);
    assert_eq!(page[0].word_count, LONG_FORM_WORDS as i64, "newest first");
    drop(conn);
    drop(db);
    cleanup(&path);
}

#[test]
fn fts_search_of_a_common_word_is_bounded() {
    let (path, db) = copy_of_seeded("dbc-fts");
    let conn = Connection::open(&path).unwrap();
    let matches: i64 = conn
        .query_row(
            "SELECT count(*) FROM transcripts_fts WHERE transcripts_fts MATCH '\"the\"*'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    eprintln!("common word matches {matches} of {} takes", TAKES + 1);
    assert!(matches > (TAKES / 3) as i64, "the word must be common");

    for (sql, n) in [(SEARCH_FIRST_PAGE_SQL, 2), (SEARCH_NEXT_PAGE_SQL, 4)] {
        let p = plan(&conn, sql, n);
        eprintln!("search plan: {p}");
        assert!(p.contains("VIRTUAL TABLE INDEX"), "not an FTS lookup: {p}");
        assert!(p.contains("LIMIT") || sql.contains("LIMIT"));
        assert!(
            !p.contains("SCAN t ") && !p.contains("SCAN transcripts"),
            "search must not scan the content table: {p}"
        );
    }

    // Bounded wall time on a debug build, through the real API, for the first
    // page and for a deep page.
    let t = Instant::now();
    let first = db
        .list_transcripts_page(200, Some("the".into()), None)
        .unwrap();
    let first_t = t.elapsed();
    let last = first.last().unwrap();
    let cursor = HistoryCursor {
        created_at: last.created_at,
        id: last.id.clone(),
    };
    let t = Instant::now();
    let second = db
        .list_transcripts_page(200, Some("the".into()), Some(cursor))
        .unwrap();
    let second_t = t.elapsed();
    eprintln!("search 'the': first page {first_t:?}, second page {second_t:?}");
    assert_eq!(first.len(), 200);
    assert_eq!(second.len(), 200);
    assert!(first_t < Duration::from_secs(2), "{first_t:?}");
    assert!(second_t < Duration::from_secs(2), "{second_t:?}");
    drop(conn);
    drop(db);
    cleanup(&path);
}

#[test]
fn day_series_and_take_rollup_never_scan_the_whole_table() {
    let (path, db) = copy_of_seeded("dbc-dayseries");
    let conn = Connection::open(&path).unwrap();

    // The Insights day series reads the rollup through its primary key.
    let p = plan(&conn, DAILY_SERIES_SQL, 1);
    eprintln!("day series plan: {p}");
    assert!(p.contains("SEARCH daily_stats USING"), "{p}");
    assert!(!p.contains("transcripts"), "{p}");

    // The rollup refresh that runs after EVERY take reads one day's rows by a
    // created_at range — before DB-C it re-read the whole table per take.
    let p = plan(&conn, DAY_ROLLUP_SQL, 2);
    eprintln!("take rollup plan: {p}");
    assert!(
        p.contains("SEARCH transcripts USING INDEX idx_transcripts_created (created_at>? AND created_at<?)"),
        "{p}"
    );
    let lo = Utc
        .with_ymd_and_hms(2026, 3, 2, 0, 0, 0)
        .unwrap()
        .to_rfc3339();
    let hi = Utc
        .with_ymd_and_hms(2026, 3, 3, 0, 0, 0)
        .unwrap()
        .to_rfc3339();
    let (rows, steps) = vm_steps(&conn, DAY_ROLLUP_SQL, [lo, hi]);
    let (all, full_steps) = vm_steps(
        &conn,
        "SELECT word_count, asr_seconds, COALESCE(speech_seconds,0), COALESCE(pipeline_ms,0) FROM transcripts",
        [],
    );
    eprintln!("one day's rollup: {rows} rows in {steps} vm steps; the whole table: {all} rows in {full_steps}");
    assert!(rows > 0 && rows < 200);
    assert!(steps * 20 < full_steps, "{steps} vs {full_steps}");

    // And the day series itself answers.
    let series = db.daily_series(365).unwrap();
    assert_eq!(series.len(), 365);
    drop(conn);
    drop(db);
    cleanup(&path);
}

#[test]
fn incremental_day_rollup_matches_a_full_rebuild() {
    let s = seeded();
    // `copy_of_seeded` opens the DB, and open() runs the FULL rebuild — so the
    // copy's rollup is the full answer and the template snapshot is the
    // incremental one the take path wrote.
    let (path, db) = copy_of_seeded("dbc-rollup");
    let full = rollup(&path);
    assert!(full.len() > 60, "history should span many days");
    assert_eq!(
        s.incremental_rollup, full,
        "per-take rollup drifted from a full rebuild"
    );

    // Deleting a take re-derives only its day, and still agrees.
    db.delete_transcript(&s.ids[4_242]).unwrap();
    db.delete_transcript("no-such-take").unwrap();
    let incremental = rollup(&path);
    db.recompute_daily_stats().unwrap();
    assert_eq!(incremental, rollup(&path), "delete rollup drifted");
    drop(db);
    cleanup(&path);
}

#[test]
fn pagination_has_a_tiebreaker_and_never_duplicates_a_row() {
    // The tiebreak is in the SQL that ships, on the primary key.
    for sql in [HISTORY_FIRST_PAGE_SQL, HISTORY_NEXT_PAGE_SQL] {
        assert!(sql.contains("ORDER BY created_at DESC, id DESC"), "{sql}");
    }
    for sql in [SEARCH_FIRST_PAGE_SQL, SEARCH_NEXT_PAGE_SQL] {
        assert!(
            sql.contains("ORDER BY t.created_at DESC, t.id DESC"),
            "{sql}"
        );
    }

    let s = seeded();
    let (path, db) = copy_of_seeded("dbc-paging");
    let expected: HashSet<&str> = s.ids.iter().map(String::as_str).collect();

    // Walk the WHOLE history with a page size that is coprime to the tie size,
    // so boundaries keep landing inside a run of identical timestamps. The
    // cursor is rebuilt from the row's JSON — exactly what the frontend sends.
    let walk = |query: Option<&str>, page_size: i64| -> (Vec<TranscriptEntry>, usize) {
        let mut out: Vec<TranscriptEntry> = Vec::new();
        let mut before: Option<HistoryCursor> = None;
        let mut split_ties = 0usize;
        loop {
            let page = db
                .list_transcripts_page(page_size, query.map(str::to_string), before.clone())
                .unwrap();
            if let (Some(prev), Some(first)) = (out.last(), page.first()) {
                if prev.created_at == first.created_at {
                    split_ties += 1;
                }
            }
            let Some(last) = page.last() else { break };
            let json = serde_json::to_value(last).unwrap();
            before = Some(
                serde_json::from_value(serde_json::json!({
                    "createdAt": json["createdAt"],
                    "id": json["id"],
                }))
                .unwrap(),
            );
            let n = page.len();
            out.extend(page);
            if (n as i64) < page_size {
                break;
            }
        }
        (out, split_ties)
    };

    let (all, split_ties) = walk(None, 7);
    eprintln!(
        "history walk: {} rows, {split_ties} page boundaries inside a tie",
        all.len()
    );
    assert!(
        split_ties > 100,
        "the walk must actually split ties ({split_ties})"
    );
    let mut seen = HashSet::new();
    for e in &all {
        assert!(seen.insert(e.id.as_str()), "row {} returned twice", e.id);
    }
    assert_eq!(seen, expected, "rows skipped or invented");
    for w in all.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        assert!(
            (a.created_at, &a.id) > (b.created_at, &b.id),
            "not a strict (created_at, id) DESC order"
        );
    }

    // Search pages: same total order, same guarantee.
    let conn = Connection::open(&path).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT t.id FROM transcripts_fts f JOIN transcripts t ON t.rowid = f.rowid
             WHERE transcripts_fts MATCH '\"meeting\"*'",
        )
        .unwrap();
    let want: HashSet<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    let (hits, search_split) = walk(Some("meeting"), 97);
    eprintln!(
        "search walk: {} rows, {search_split} boundaries inside a tie",
        hits.len()
    );
    let mut got = HashSet::new();
    for e in &hits {
        assert!(
            got.insert(e.id.clone()),
            "search row {} returned twice",
            e.id
        );
    }
    assert_eq!(got, want, "search pages skipped or invented rows");
    drop(stmt);
    drop(conn);
    drop(db);
    cleanup(&path);
}

/// A writer that records the size of every write it is handed, and keeps the
/// bytes so the content can be checked.
#[derive(Default)]
struct Recording {
    bytes: Vec<u8>,
    writes: usize,
    largest_write: usize,
}
impl std::io::Write for Recording {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writes += 1;
        self.largest_write = self.largest_write.max(buf.len());
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn contains(hay: &[u8], needle: &str) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn export_streams_and_leaks_no_key_or_home_path() {
    let (path, db) = copy_of_seeded("dbc-export");
    const KEY: &str = "YAP-DBC-TEST-LICENSE-KEY-7f3a9c";
    db.license_state_set("license.key", KEY).unwrap();
    db.license_state_set("license.trial_started_at_ms", "1760000000000")
        .unwrap();
    let home = std::env::var("HOME").unwrap_or_default();
    let data_dir = path.parent().unwrap().display().to_string();

    // Positive control: the leak check fires on the shapes it exists to catch.
    let bait = format!("{{\"text\":\"{KEY} {home}/Library {data_dir}\"}}");
    assert!(contains(bait.as_bytes(), KEY));
    assert!(home.is_empty() || contains(bait.as_bytes(), &home));
    assert!(contains(bait.as_bytes(), &data_dir));

    let mut w = Recording::default();
    let n = db.write_transcripts_jsonl(&mut w).unwrap();
    assert_eq!(n, TAKES + 1);

    // Streams: one line per row handed over as it is read — never one buffer.
    let lines: Vec<&[u8]> = w
        .bytes
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(lines.len(), n);
    let longest_line = lines.iter().map(|l| l.len()).max().unwrap();
    eprintln!(
        "export: {n} rows, {} bytes, {} writes, largest single write {} bytes (longest row {longest_line})",
        w.bytes.len(),
        w.writes,
        w.largest_write
    );
    assert!(
        w.writes >= n,
        "rows were batched into fewer writes than rows"
    );
    assert!(
        w.largest_write <= longest_line + 1,
        "a write larger than one row: the export built a buffer"
    );
    assert!(w.bytes.len() > 20 * w.largest_write);
    // Each line is a complete transcript object.
    for l in lines.iter().take(3) {
        let v: serde_json::Value = serde_json::from_slice(l).unwrap();
        assert!(v.get("id").is_some() && v.get("text").is_some());
    }

    // Leaks nothing it should not.
    assert!(!contains(&w.bytes, KEY), "export contains the license key");
    assert!(
        !contains(&w.bytes, "license."),
        "export contains license state"
    );
    if !home.is_empty() {
        assert!(!contains(&w.bytes, &home), "export contains the home path");
    }
    assert!(
        !contains(&w.bytes, &data_dir),
        "export contains the data dir"
    );
    assert!(
        !contains(&w.bytes, "/Users/"),
        "export contains an absolute user path"
    );
    drop(db);
    cleanup(&path);
}

fn vocab_hits(conn: &Connection, term: &str) -> i64 {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS temp.dbc_vocab
           USING fts5vocab(main, 'transcripts_fts', 'row');",
    )
    .unwrap();
    conn.query_row(
        "SELECT COALESCE(SUM(doc), 0) FROM temp.dbc_vocab WHERE term = ?1",
        [term],
        |r| r.get(0),
    )
    .unwrap()
}

fn file_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

#[test]
fn clear_history_leaves_no_fts_residue_for_a_known_word() {
    let (path, db) = copy_of_seeded("dbc-clear");
    let wal = PathBuf::from(format!("{}-wal", path.display()));

    // Positive control: the word IS in the index, and in the file, before.
    let (before_pages, before_hits) = {
        let conn = Connection::open(&path).unwrap();
        let hits = vocab_hits(&conn, NEEDLE);
        let pages: i64 = conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap();
        (pages, hits)
    };
    assert_eq!(
        before_hits,
        (TAKES / 2_000) as i64,
        "needle should index in 5 takes"
    );
    db.checkpoint();
    assert!(
        contains(&file_bytes(&path), NEEDLE),
        "control: needle on disk before clear"
    );

    db.clear_transcripts().unwrap();

    let conn = Connection::open(&path).unwrap();
    // The WORD is gone from the index — not merely the rows from the table.
    assert_eq!(
        vocab_hits(&conn, NEEDLE),
        0,
        "FTS still indexes the cleared word"
    );
    let terms: i64 = conn
        .query_row("SELECT count(*) FROM temp.dbc_vocab", [], |r| r.get(0))
        .unwrap();
    assert_eq!(terms, 0, "FTS vocabulary not empty after Clear History");
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM transcripts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);

    // VACUUMed: no free pages left holding old content, and the file shrank.
    let free: i64 = conn
        .query_row("PRAGMA freelist_count", [], |r| r.get(0))
        .unwrap();
    let after_pages: i64 = conn
        .query_row("PRAGMA page_count", [], |r| r.get(0))
        .unwrap();
    eprintln!("clear: page_count {before_pages} -> {after_pages}, freelist {free}");
    assert_eq!(free, 0, "free pages remain: the file was not VACUUMed");
    assert!(
        after_pages * 10 < before_pages,
        "{before_pages} -> {after_pages}"
    );
    drop(conn);

    // And the bytes themselves: nowhere in the DB file or its WAL.
    assert!(
        !contains(&file_bytes(&path), NEEDLE),
        "needle bytes survive in the .db"
    );
    assert!(
        !contains(&file_bytes(&wal), NEEDLE),
        "needle bytes survive in the -wal"
    );
    drop(db);
    cleanup(&path);
}

#[test]
fn clear_history_keeps_scratchpad_notes_by_design() {
    // DB-C's documented choice (db.rs `clear_transcripts`): Scratchpad notes
    // are documents the user wrote, not dictation history, so Clear History
    // KEEPS them. This test holds that line in both directions.
    let (path, db) = copy_of_seeded("dbc-scratch");
    let note = db
        .save_scratch(None, "Groceries".into(), "eggs, oat milk, coffee".into())
        .unwrap();
    db.clear_transcripts().unwrap();
    let notes = db.list_scratch().unwrap();
    assert_eq!(
        notes.len(),
        1,
        "Clear History must not erase Scratchpad notes"
    );
    assert_eq!(notes[0].id, note.id);
    assert_eq!(notes[0].body, "eggs, oat milk, coffee");
    assert!(db.list_transcripts(10, None).unwrap().is_empty());

    // Its own Delete destroys it, with secure_delete scrubbing the bytes.
    db.delete_scratch(&note.id).unwrap();
    assert!(db.list_scratch().unwrap().is_empty());
    db.checkpoint();
    assert!(
        !contains(&file_bytes(&path), "oat milk"),
        "scratch bytes survive delete"
    );
    drop(db);
    cleanup(&path);
}
