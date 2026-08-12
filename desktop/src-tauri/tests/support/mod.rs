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
/// A wav lands, but the engine says the recording is short (YV91's watchdog
/// stop, or a session that stopped being the registered capture). The control
/// plane must file this as `partial`, NOT as `transcribing`.
pub const FAKE_PARTIAL: u8 = 4;

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
                partial: true,
            }),
            _ => {
                // A real file, because `finish_meeting` stores a path the
                // retention sweep later has to find and delete.
                std::fs::write(&self.path, b"RIFF....WAVEfake").map_err(|e| e.to_string())?;
                let partial = FAKE_MODE.load(Ordering::SeqCst) == FAKE_PARTIAL;
                Ok(CaptureOutcome {
                    wav_path: Some(self.path.clone()),
                    seconds,
                    note: if partial {
                        Some("stopped early: the Mac ran out of disk".into())
                    } else {
                        None
                    },
                    partial,
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

// ── YV97 · the summary stubs ────────────────────────────────────────────────
//
// The summarizer's five acceptance criteria are all properties of its PURE
// stages — chunking, grammar generation, defensive parsing, the ported gate, the
// merge. A stub model is what makes them testable at all: the real 1.5B is
// nondeterministic, slow, and not installed in CI, and none of the properties
// under test are properties of the model.

/// A meeting-shaped token count, standing in for a real BPE vocabulary.
///
/// One token per word, plus the three things that actually split a word into
/// several: an interior capital (`Delacroix-Bell`, `AirPods`), a punctuation or
/// separator character (BPE rarely merges `-`, `_`, `:` or `—` into a
/// neighbouring word), and length past a common-subword ceiling. Digits split
/// too, which is why a `seg_0001:` tag costs five tokens and one word.
///
/// The point of modelling it this way rather than as a flat multiplier: on
/// name-dense, tagged, disfluent meeting text this runs WELL ABOVE the
/// rewriter's 1.3-tokens/word proxy, which is finding #35 in one function. On
/// plain lowercase prose it stays near 1.0, so a chunker measured against it is
/// not simply being handed a bigger number everywhere.
pub fn meeting_token_count(text: &str) -> usize {
    text.split_whitespace()
        .map(|word| {
            let chars = word.chars().count();
            1 + word.chars().filter(|c| c.is_uppercase()).count()
                + word.chars().filter(|c| !c.is_alphanumeric()).count()
                + word.chars().filter(char::is_ascii_digit).count() / 2
                + chars.saturating_sub(6) / 4
        })
        .sum()
}

/// A scripted stand-in for the sidecar: answers come from a closure over the
/// request, and every request is recorded so a test can assert what was ASKED,
/// not only what came back.
pub struct StubModel {
    responder: Box<
        dyn Fn(&wilson_voice_lib::polish_protocol::PolishRequest) -> Result<Generated, SummaryError>
            + Send
            + Sync,
    >,
    requests: std::sync::Mutex<Vec<wilson_voice_lib::polish_protocol::PolishRequest>>,
    counts: AtomicU64,
}

use wilson_voice_lib::summarize::{Generated, SummaryClient, SummaryError, TokenCounter};

impl StubModel {
    /// A stub whose answers all fitted the model's context.
    pub fn new(
        responder: impl Fn(&wilson_voice_lib::polish_protocol::PolishRequest) -> Result<String, SummaryError>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self::truncating(move |req| responder(req).map(Generated::whole))
    }

    /// A stub that can also answer "…and I had to cut your input to fit" — the
    /// sidecar's `truncated:true`, which must reach the reader and not only the
    /// log.
    pub fn truncating(
        responder: impl Fn(&wilson_voice_lib::polish_protocol::PolishRequest) -> Result<Generated, SummaryError>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            responder: Box::new(responder),
            requests: std::sync::Mutex::new(Vec::new()),
            counts: AtomicU64::new(0),
        }
    }

    /// Every request the pipeline sent, in order.
    pub fn requests(&self) -> Vec<wilson_voice_lib::polish_protocol::PolishRequest> {
        self.requests.lock().expect("stub lock").clone()
    }

    /// Requests of one summarize stage (`map` / `reduce`).
    pub fn stage(&self, stage: &str) -> Vec<wilson_voice_lib::polish_protocol::PolishRequest> {
        self.requests()
            .into_iter()
            .filter(|r| r.mode == stage)
            .collect()
    }

    /// How many times the pipeline asked the tokenizer anything.
    pub fn token_count_calls(&self) -> u64 {
        self.counts.load(Ordering::Relaxed)
    }
}

impl TokenCounter for StubModel {
    fn count_tokens(&self, text: &str) -> Result<usize, SummaryError> {
        self.counts.fetch_add(1, Ordering::Relaxed);
        Ok(meeting_token_count(text))
    }
}

impl SummaryClient for StubModel {
    fn generate(
        &self,
        req: &wilson_voice_lib::polish_protocol::PolishRequest,
    ) -> Result<Generated, SummaryError> {
        self.requests.lock().expect("stub lock").push(req.clone());
        (self.responder)(req)
    }

    fn model_id(&self) -> String {
        "stub-1.5b".to_string()
    }
}

/// The `seg_NNNN` labels present in a rendered chunk — how a stub answers with
/// ids that really are in the chunk it was handed.
pub fn labels_in(chunk_text: &str) -> Vec<String> {
    chunk_text
        .lines()
        .filter_map(|l| {
            l.split_once(": ")
                .map(|(label, _)| label.trim().to_string())
        })
        .collect()
}

/// The body of one rendered chunk line, without its label.
pub fn bodies_in(chunk_text: &str) -> Vec<String> {
    chunk_text
        .lines()
        .filter_map(|l| l.split_once(": ").map(|(_, body)| body.trim().to_string()))
        .collect()
}

/// A well-formed MAP answer of exactly the shape the grammar admits.
pub fn map_answer(
    narrative: &str,
    actions: &[(&str, &str)],
    decisions: &[(&str, &str)],
    questions: &[(&str, &str)],
) -> String {
    let items = |items: &[(&str, &str)]| {
        serde_json::Value::Array(
            items
                .iter()
                .map(|(text, segment)| serde_json::json!({ "text": text, "segment": segment }))
                .collect(),
        )
    };
    serde_json::json!({
        "narrative": narrative,
        "actions": items(actions),
        "decisions": items(decisions),
        "questions": items(questions),
    })
    .to_string()
}

/// A tiny meeting whose words are the vocabulary the summary tests draw from —
/// so a "grounded" item in a test really is grounded, and the V3 floor is being
/// exercised rather than accidentally satisfied.
pub const SEGMENT_TEXTS: [&str; 3] = [
    "we should move the onboarding review before the release goes out",
    "we are shipping without the calendar work this cycle and that is settled",
    "nobody owns the escalation path yet and someone should fix the actions counter",
];

/// [`SEGMENT_TEXTS`] as YV94 rows, four seconds apart.
pub fn segments() -> Vec<wilson_voice_lib::meetings::MeetingSegment> {
    segments_from(&SEGMENT_TEXTS)
}

/// Arbitrary texts as YV94 rows.
pub fn segments_from(texts: &[&str]) -> Vec<wilson_voice_lib::meetings::MeetingSegment> {
    texts
        .iter()
        .enumerate()
        .map(|(i, text)| wilson_voice_lib::meetings::MeetingSegment {
            id: format!("segment-{i}"),
            meeting_id: "meeting-under-test".to_string(),
            start_seconds: i as f64 * 4.0,
            end_seconds: i as f64 * 4.0 + 3.5,
            text: (*text).to_string(),
            confidence: None,
            created_at: chrono::Utc::now(),
        })
        .collect()
}
