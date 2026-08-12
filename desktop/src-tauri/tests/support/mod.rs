//! Shared scaffolding for the YV94 meeting-schema tests.
//!
//! Every test here opens a REAL SQLite file, not `:memory:`. The claims under
//! test — `PRAGMA user_version` surviving a reopen, `secure_delete` cascading
//! through FTS5 shadow tables, a WAV disappearing from disk — are claims about
//! a file on a filesystem, and an in-memory DB would quietly make three of them
//! untestable.

#![allow(dead_code)] // each test binary uses a different subset

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use wilson_voice_lib::db::Database;
use wilson_voice_lib::meetings::NewMeetingSegment;

static SEQ: AtomicU64 = AtomicU64::new(0);

/// A fresh directory under Cargo's own per-target temp dir, so parallel test
/// binaries never share a DB and `cargo clean` takes the leftovers with it.
pub fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("yv94-{tag}-{pid}-{n}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

pub fn open_db(dir: &std::path::Path) -> Database {
    Database::open(dir.join("wilson_voice.db")).expect("open db")
}

/// A meeting with `texts.len()` segments, four seconds apart, plus a WAV file on
/// disk that the row points at — the shape a finished YV91+YV93 capture leaves.
pub fn seed_meeting(db: &Database, dir: &std::path::Path, title: &str, texts: &[&str]) -> String {
    let meeting = db.create_meeting(title, "manual").expect("create meeting");
    let segments: Vec<NewMeetingSegment> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| NewMeetingSegment::new(i as f64 * 4.0, i as f64 * 4.0 + 3.5, *t))
        .collect();
    db.append_meeting_segments(&meeting.id, &segments)
        .expect("append segments");
    let wav = dir.join(format!("{}.wav", meeting.id));
    std::fs::write(&wav, b"RIFF....WAVEfake").expect("write wav");
    db.finish_meeting(&meeting.id, texts.len() as f64 * 4.0, Some(&wav))
        .expect("finish meeting");
    db.set_meeting_state(
        &meeting.id,
        wilson_voice_lib::meetings::MeetingState::Complete,
        None,
    )
    .expect("set state");
    meeting.id
}

/// Count rows in one of FTS5's shadow tables. `_docsize` holds exactly one row
/// per indexed document, which makes it the honest answer to "is this phrase
/// still in the index" — a MATCH returning nothing could also mean the query
/// was wrong.
pub fn fts_docsize_rows(dir: &std::path::Path) -> i64 {
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).expect("reopen");
    conn.query_row(
        "SELECT COUNT(*) FROM meeting_segments_fts_docsize",
        [],
        |r| r.get(0),
    )
    .expect("docsize count")
}
