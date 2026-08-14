//! Shared scaffolding for the YV94 meeting-schema tests and YV95's control
//! plane.
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

// ── YV95 · a synthetic capture engine ────────────────────────────────────────
//
// The control plane's dependency on audio is one trait
// (`meeting_control::CaptureEngine`), installed once at startup. YV91's
// `MeetingSession` is the production implementation; these tests install this
// one, which does everything the controller can observe — hands back a WAV path,
// reports captured seconds, can fail, can come back empty — without a
// microphone, a permission prompt, or three hours.
//
// That is the point of the seam rather than a compromise of it: "a user can
// start and stop a meeting" is a claim about the control plane, and a test that
// needed a real device could only ever be run by hand.

use std::path::Path;
use std::sync::atomic::{AtomicU8, AtomicUsize};
use std::sync::Arc;
use std::time::Instant;

use wilson_voice_lib::meeting_control::{ActiveCapture, CaptureEngine, CaptureOutcome};

/// What the next `start`/`stop` pair should do.
pub const FAKE_OK: u8 = 0;
/// `stop` returns no WAV — capture ran but nothing landed (a `partial` meeting).
pub const FAKE_NO_AUDIO: u8 = 1;
/// `start` refuses — the machine cannot record right now.
pub const FAKE_START_FAILS: u8 = 2;
/// `stop` errors out after a successful start.
pub const FAKE_STOP_FAILS: u8 = 3;

static FAKE_MODE: AtomicU8 = AtomicU8::new(FAKE_OK);
static FAKE_STARTS: AtomicUsize = AtomicUsize::new(0);
static FAKE_STOPS: AtomicUsize = AtomicUsize::new(0);

pub fn set_fake_mode(mode: u8) {
    FAKE_MODE.store(mode, Ordering::SeqCst);
}

pub fn fake_starts() -> usize {
    FAKE_STARTS.load(Ordering::SeqCst)
}

pub fn fake_stops() -> usize {
    FAKE_STOPS.load(Ordering::SeqCst)
}

pub struct FakeEngine;

impl CaptureEngine for FakeEngine {
    fn start(&self, dir: &Path) -> Result<Box<dyn ActiveCapture>, String> {
        FAKE_STARTS.fetch_add(1, Ordering::SeqCst);
        if FAKE_MODE.load(Ordering::SeqCst) == FAKE_START_FAILS {
            return Err("no input device".into());
        }
        let path = dir.join(format!("fake-{}.wav", FAKE_STARTS.load(Ordering::SeqCst)));
        Ok(Box::new(FakeCapture {
            path,
            started: Instant::now(),
        }))
    }
}

struct FakeCapture {
    path: PathBuf,
    started: Instant,
}

impl ActiveCapture for FakeCapture {
    fn seconds(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn stop(self: Box<Self>) -> Result<CaptureOutcome, String> {
        FAKE_STOPS.fetch_add(1, Ordering::SeqCst);
        let seconds = self.started.elapsed().as_secs_f64();
        match FAKE_MODE.load(Ordering::SeqCst) {
            FAKE_STOP_FAILS => Err("device disappeared mid-meeting".into()),
            FAKE_NO_AUDIO => Ok(CaptureOutcome {
                wav_path: None,
                seconds,
                note: Some("no audio reached the disk".into()),
            }),
            _ => {
                // A real file, because `finish_meeting` stores a path the
                // retention sweep later has to find and delete.
                std::fs::write(&self.path, b"RIFF....WAVEfake").map_err(|e| e.to_string())?;
                Ok(CaptureOutcome {
                    wav_path: Some(self.path.clone()),
                    seconds,
                    note: None,
                })
            }
        }
    }
}

/// Install the fake engine for this test PROCESS. Idempotent: the registry is a
/// `OnceLock`, so a second call is a no-op rather than a second recorder.
pub fn install_fake_engine() {
    wilson_voice_lib::meeting_control::install_capture_engine(Arc::new(FakeEngine));
}

/// Serializes the tests that reach for [`set_fake_mode`].
///
/// `cargo test` runs the tests in a file concurrently, and the engine's mode is
/// process-global (it has to be: the registry behind `CaptureEngine` is a
/// `OnceLock`, one recorder per process, exactly like production). Without this
/// the "start refuses" test would flip the mode under the "start succeeds" one.
/// Poisoning is recovered from rather than propagated, so ONE failing test does
/// not turn into every test failing with a lock error and hiding the real one.
pub fn exclusive() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
