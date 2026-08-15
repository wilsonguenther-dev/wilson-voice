//! Shared scaffolding for the YV94 meeting-schema tests and YV95's control
//! plane.
//!
//! Every test here opens a REAL SQLite file, not `:memory:`. The claims under
//! test — `PRAGMA user_version` surviving a reopen, `secure_delete` cascading
//! through FTS5 shadow tables, a WAV disappearing from disk — are claims about
//! a file on a filesystem, and an in-memory DB would quietly make three of them
//! untestable.

#![allow(dead_code)] // each test binary uses a different subset

/// YV109 — the two-track host-time reference the eval fixture and the
/// phase-closing E2E both score against. Its own module because it is the one
/// piece of scaffolding here that is a MEASUREMENT rather than a fixture: see
/// its header for why an eval harness computes the answer independently of the
/// code it is scoring.
pub mod two_track;

/// YV124 — the shipped `yap-diarize` sidecar behind the shipped `DiarizePool`,
/// plus the three-state answer to "can this machine produce embeddings?". Here
/// rather than in one test file because the anti-alias EER arm is the first
/// caller and YV126/YV129 are the next two.
pub mod diarize;

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

/// YV108 — a finished TWO-track meeting: the mic's turns and the tap's turns,
/// each `(start_seconds, track, text)`, appended in **two separate batches** the
/// way the real pipeline does it (one transcription pass per recorded wav).
///
/// The batching is the point, not an implementation detail: appending them
/// interleaved in one call would give every row the same `created_at` and hide
/// the ordering question this item exists to answer. Here the tap's rows are
/// written second and therefore carry a LATER `created_at` than mic rows that
/// were spoken after them — so a transcript that comes back in the right order
/// came back that way because of `start_seconds`, not by accident of insert
/// order.
///
/// YV125 — the row is created as [`MeetingKind::Virtual`], which is what a
/// two-track meeting IS: a call whose other participants arrived on their own
/// track. That is also the one kind under which the mic track is labelled "Me",
/// so a seeder that left it `unknown` would quietly make every Me/Them
/// expectation in this suite unreachable.
pub fn seed_two_track_meeting(
    db: &Database,
    dir: &std::path::Path,
    title: &str,
    turns: &[(f64, i64, &str)],
) -> String {
    let meeting = db
        .create_meeting_with_kind(
            title,
            "manual",
            wilson_voice_lib::meetings::MeetingKind::Virtual,
        )
        .expect("create meeting");
    let batch = |track: i64| -> Vec<NewMeetingSegment> {
        turns
            .iter()
            .filter(|(_, t, _)| *t == track)
            .map(|(start, t, text)| {
                NewMeetingSegment::new(*start, *start + 3.5, *text).on_track(*t)
            })
            .collect()
    };
    for track in [
        wilson_voice_lib::meetings::MIC_TRACK,
        wilson_voice_lib::meetings::SYSTEM_TRACK,
    ] {
        let segments = batch(track);
        if !segments.is_empty() {
            db.append_meeting_segments(&meeting.id, &segments)
                .expect("append segments");
        }
    }
    let mic_wav = dir.join(format!("{}.t0.wav", meeting.id));
    let sys_wav = dir.join(format!("{}.t1.wav", meeting.id));
    std::fs::write(&mic_wav, b"RIFF....WAVEfake").expect("write mic wav");
    std::fs::write(&sys_wav, b"RIFF....WAVEfake").expect("write sys wav");
    let seconds = turns
        .iter()
        .map(|(start, _, _)| *start + 4.0)
        .fold(0.0f64, f64::max);
    db.finish_meeting(&meeting.id, seconds, Some(&mic_wav))
        .expect("finish meeting");
    db.set_meeting_sys_wav_path(&meeting.id, Some(&sys_wav))
        .expect("set sys wav path");
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
/// YV107 / OS-2 — a two-track meeting whose finalize measured both tracks'
/// true rates, with the SYSTEM track running far enough off its nominal rate to
/// be flagged. Proves the measurement survives the whole stop path into the
/// `diagnostics` column, which is the only place a later escalation decision
/// could read it from.
pub const FAKE_TWO_TRACK_DRIFT: u8 = 5;

/// The rates [`FAKE_TWO_TRACK_DRIFT`] reports, as the finalize would have
/// measured them: a mic sitting inside its crystal tolerance and a tap 100 ppm
/// fast — the fixture offset OS-2's acceptance criterion names.
pub fn fake_track_rates() -> Vec<wilson_voice_lib::meeting::TrackRate> {
    vec![
        fake_track_rate(wilson_voice_lib::meeting::MIC_TRACK, 12.0),
        fake_track_rate(wilson_voice_lib::meeting::SYSTEM_TRACK, 100.0),
    ]
}

fn fake_track_rate(track: usize, ppm: f64) -> wilson_voice_lib::meeting::TrackRate {
    let nominal_rate = wilson_voice_lib::meeting::TARGET_RATE;
    let span_seconds = 7200.0;
    wilson_voice_lib::meeting::TrackRate {
        track,
        nominal_rate,
        measured_rate: nominal_rate as f64 * (1.0 + ppm / 1e6),
        ppm,
        intervals: span_seconds as usize,
        intervals_skipped: 0,
        span_seconds,
        ppm_uncertainty: 0.0,
        drift_at_cap_ms: ppm.abs() / 1e6 * 3.0 * 3600.0 * 1000.0,
        flagged: ppm.abs() > wilson_voice_lib::meeting::TRUE_RATE_PPM_LIMIT,
        segments: 1,
    }
}

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

/// YV125 — the kind the fake engine was last asked to start under, so a test
/// can assert what the control plane HANDED THE ENGINE and not only what it
/// wrote on the row. Those are two writes from one value, and a test that only
/// checks the row would not notice them coming apart.
static FAKE_KIND: AtomicU8 = AtomicU8::new(FAKE_KIND_NONE);
const FAKE_KIND_NONE: u8 = 0;
const FAKE_KIND_VIRTUAL: u8 = 1;
const FAKE_KIND_IN_PERSON: u8 = 2;
const FAKE_KIND_UNKNOWN: u8 = 3;

/// The kind of the most recent `FakeEngine::start`, or `None` before the first.
pub fn fake_started_kind() -> Option<wilson_voice_lib::meetings::MeetingKind> {
    use wilson_voice_lib::meetings::MeetingKind;
    match FAKE_KIND.load(Ordering::SeqCst) {
        FAKE_KIND_VIRTUAL => Some(MeetingKind::Virtual),
        FAKE_KIND_IN_PERSON => Some(MeetingKind::InPerson),
        FAKE_KIND_UNKNOWN => Some(MeetingKind::Unknown),
        _ => None,
    }
}

pub struct FakeEngine;

impl CaptureEngine for FakeEngine {
    fn start(
        &self,
        dir: &Path,
        kind: wilson_voice_lib::meetings::MeetingKind,
    ) -> Result<Box<dyn ActiveCapture>, String> {
        use wilson_voice_lib::meetings::MeetingKind;
        FAKE_KIND.store(
            match kind {
                MeetingKind::Virtual => FAKE_KIND_VIRTUAL,
                MeetingKind::InPerson => FAKE_KIND_IN_PERSON,
                MeetingKind::Unknown => FAKE_KIND_UNKNOWN,
            },
            Ordering::SeqCst,
        );
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
                sys_wav_path: None,
                tap_rebuilds: None,
                track_rates: Vec::new(),
                seconds,
                note: Some("no audio reached the disk".into()),
                partial: true,
            }),
            mode => {
                // A real file, because `finish_meeting` stores a path the
                // retention sweep later has to find and delete.
                std::fs::write(&self.path, b"RIFF....WAVEfake").map_err(|e| e.to_string())?;
                let partial = mode == FAKE_PARTIAL;
                Ok(CaptureOutcome {
                    wav_path: Some(self.path.clone()),
                    sys_wav_path: None,
                    tap_rebuilds: None,
                    track_rates: if mode == FAKE_TWO_TRACK_DRIFT {
                        fake_track_rates()
                    } else {
                        Vec::new()
                    },
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

/// Block until `cond` holds, or fail with `what` after `deadline`.
///
/// **The one idiom in this suite for "the other thread has got far enough".**
/// Anything the controller's ticker thread produces — a thermal sample, an
/// elapsed-clock emit — arrives when the SCHEDULER runs that thread, so
/// `sleep(n * TICK)` followed by an assertion about `n` things having happened
/// is a bet on the runner's load, not a statement about the code. Those bets
/// lose: `meeting_diagnostics_row` observed 2 ramp samples where 6 were needed,
/// and `meeting_manual_start_stop`'s tick-count window (YV95/#112) failed three
/// separate CI runs before YV111 reshaped it.
///
/// Widening the sleep does not fix a starvable thread — it only makes the same
/// bet with worse odds and a slower suite. Waiting on the thread's own progress
/// does, and turns a wall-clock race into a timeout that names what never
/// happened.
///
/// `deadline` is a HANG guard, never a measurement: it should be far longer than
/// any plausible scheduling delay, so that reaching it means the work genuinely
/// stopped rather than that the box was busy.
pub fn wait_until(what: &str, deadline: std::time::Duration, mut cond: impl FnMut() -> bool) {
    let start = std::time::Instant::now();
    while !cond() {
        assert!(
            start.elapsed() < deadline,
            "timed out after {deadline:?} waiting for {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
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
///
/// YV133 — a rendered line may now be `seg_0001 (Them): …`, so the speaker tag
/// is stripped back off here. That is what the model has to do too: the id is
/// what the grammar enumerates and what the citation must echo, and a stub that
/// answered `"seg_0001 (Them)"` would be dropped by the allowlist exactly as a
/// real model's would be.
pub fn labels_in(chunk_text: &str) -> Vec<String> {
    chunk_text
        .lines()
        .filter_map(|l| l.split_once(": ").map(|(label, _)| label))
        .map(|label| {
            label
                .split_once(" (")
                .map(|(id, _)| id)
                .unwrap_or(label)
                .trim()
                .to_string()
        })
        .collect()
}

/// The speaker tag on each rendered line, or `None` where the line carries
/// none — what a MAP pass was actually SHOWN about who was speaking.
pub fn speakers_in(chunk_text: &str) -> Vec<Option<String>> {
    chunk_text
        .lines()
        .filter_map(|l| l.split_once(": ").map(|(prefix, _)| prefix))
        .map(|prefix| {
            prefix
                .split_once(" (")
                .and_then(|(_, rest)| rest.strip_suffix(')'))
                .map(|s| s.to_string())
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

// ── YV133 · the speaker source YV128–130 will build ──────────────────────────

/// A [`SpeakerSource`] that answers with enrolled NAMES, keyed by segment id.
///
/// This stands in for the `speaker_profiles` lookup YV128 schemas and YV129
/// matches against — the seam, filled by hand, because the table does not exist
/// on `main` yet. That is the honest shape of it: `summarize.rs` takes any
/// source, the app today wires the only one it can build
/// (`summarize::TrackSpeakers`, which never returns a name), and this is what a
/// named one looks like from the summarizer's side. Nothing in `summarize.rs`
/// changes when the real one lands.
pub struct EnrolledSpeakers {
    by_segment_id: std::collections::HashMap<String, String>,
}

impl EnrolledSpeakers {
    /// `(segment id, enrolled name)` pairs. Segments not listed are unenrolled
    /// — the "new voice" case, which must come back as no attribution at all
    /// rather than as a guess.
    pub fn new(pairs: &[(&str, &str)]) -> Self {
        Self {
            by_segment_id: pairs
                .iter()
                .map(|(id, name)| ((*id).to_string(), (*name).to_string()))
                .collect(),
        }
    }
}

impl wilson_voice_lib::summarize::SpeakerSource for EnrolledSpeakers {
    fn speaker_for(
        &self,
        segment: &wilson_voice_lib::meetings::MeetingSegment,
    ) -> Option<wilson_voice_lib::summarize::SegmentSpeaker> {
        self.by_segment_id
            .get(&segment.id)
            .cloned()
            .map(wilson_voice_lib::summarize::SegmentSpeaker::Named)
    }
}

/// YV133 — arbitrary `(start_seconds, track, text)` turns as YV94 rows, in the
/// order given, so a test can build a two-track meeting's transcript without a
/// database.
pub fn segments_on_tracks(
    turns: &[(f64, i64, &str)],
) -> Vec<wilson_voice_lib::meetings::MeetingSegment> {
    turns
        .iter()
        .enumerate()
        .map(
            |(i, (start, track, text))| wilson_voice_lib::meetings::MeetingSegment {
                id: format!("segment-{i}"),
                meeting_id: "meeting-under-test".to_string(),
                start_seconds: *start,
                end_seconds: *start + 3.5,
                text: (*text).to_string(),
                confidence: None,
                created_at: chrono::Utc::now(),
                track: *track,
            },
        )
        .collect()
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
            track: wilson_voice_lib::meetings::MIC_TRACK,
        })
        .collect()
}
