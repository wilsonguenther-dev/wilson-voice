//! YV95 / finding #6 — "yap22 has no way to start a meeting… as scoped, 22 ships
//! a feature nobody can reach."
//!
//! The plan's acceptance line for this item is a UX one ("a user who has never
//! read a changelog can start and stop a meeting within 10 seconds"), and the
//! timed repro is in the PR. This file holds the half of that claim a machine
//! can check: pressing the thing produces a meeting row, the elapsed clock ticks
//! at 1 Hz while it runs, pressing it again closes the row out with a duration
//! and a WAV, and every failure mode says so instead of leaving a half-open
//! meeting behind.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use support::{open_db, temp_dir};
use wilson_voice_lib::meeting_control::{
    MeetingController, MeetingStatus, StatusSink, NO_ENGINE_MESSAGE, TICK_INTERVAL,
};
use wilson_voice_lib::meetings::MeetingState;

/// A short tick so the cadence is testable in a second instead of a minute. The
/// production value is asserted separately, below.
const TEST_TICK: Duration = Duration::from_millis(60);

/// Collects every status the controller emits — the same sink `lib.rs` fills
/// with "emit to both windows and flip the pill's energy flag".
#[derive(Default)]
struct Sink {
    seen: Mutex<Vec<MeetingStatus>>,
    ticks: AtomicUsize,
}

fn sink() -> (Arc<Sink>, StatusSink) {
    let collected = Arc::new(Sink::default());
    let c = Arc::clone(&collected);
    let f: StatusSink = Arc::new(move |s: &MeetingStatus| {
        if s.recording {
            c.ticks.fetch_add(1, Ordering::SeqCst);
        }
        c.seen.lock().unwrap().push(s.clone());
    });
    (collected, f)
}

#[test]
fn a_meeting_starts_and_stops_from_one_toggle() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = temp_dir("yv95-toggle");
    let db = Arc::new(open_db(&dir));
    let (seen, f) = sink();
    let c = MeetingController::new(Arc::clone(&db), f).with_tick_interval(TEST_TICK);

    let starts_before = support::fake_starts();
    let stops_before = support::fake_stops();
    assert!(!c.is_recording());
    assert!(!c.status().recording, "idle before anything is pressed");
    assert!(
        c.status().capture_available,
        "a capture engine is installed in this process"
    );

    // Press once — the same call the tray item, ⌃⌘M, the pill and the empty
    // state all make.
    let started = c.toggle(&dir, None).expect("start");
    assert!(started.recording);
    let id = started
        .id
        .clone()
        .expect("a recording status carries its id");
    assert!(
        started.title.unwrap().starts_with("Meeting "),
        "a manual meeting is named for when it started"
    );

    let row = db
        .get_meeting(&id)
        .unwrap()
        .expect("the row exists at once");
    assert_eq!(
        row.state,
        MeetingState::Recording.as_str(),
        "the row is open while capture runs"
    );
    assert!(
        row.diagnostics.is_some(),
        "OS-12: the preflight readings are written before the first sample"
    );

    // Let the 1 Hz clock run.
    std::thread::sleep(TEST_TICK * 5);

    // Press again.
    let stopped = c.toggle(&dir, None).expect("stop");
    assert!(!stopped.recording);
    assert!(!c.is_recording());

    let row = db.get_meeting(&id).unwrap().unwrap();
    assert_eq!(
        row.state,
        MeetingState::Transcribing.as_str(),
        "capture landed, so the honest next state is transcribing (YV93 finishes it)"
    );
    assert!(row.ended_at.is_some(), "the row is closed out");
    assert!(
        row.duration_seconds > 0.0,
        "a stopped meeting has a real duration"
    );
    let wav = row.mic_wav_path.expect("the WAV path is recorded");
    assert!(
        std::path::Path::new(&wav).exists(),
        "the file the row points at is on disk"
    );
    assert!(row.audio_kept, "audio is kept until the retention sweep");

    // The list the Meetings tab renders now has something in it — which is the
    // whole of finding #6.
    assert_eq!(db.list_meetings(10, None).unwrap().len(), 1);
    assert_eq!(support::fake_starts() - starts_before, 1);
    assert_eq!(support::fake_stops() - stops_before, 1);

    // The status stream ended on an idle status, so a UI that only listens to
    // the event (and never polls) still lands on the right state.
    let last = seen.seen.lock().unwrap().last().cloned().unwrap();
    assert!(!last.recording);
    assert_eq!(last.elapsed_label, "00:00:00");
}

/// OS-12 fix (1): elapsed time is a 1 Hz emit from Rust, not a timer in the
/// webview. Asserted as a RATE so the test does not depend on the scheduler
/// delivering exactly n ticks.
#[test]
fn the_elapsed_clock_ticks_once_per_interval_and_not_more() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = temp_dir("yv95-tick");
    let db = Arc::new(open_db(&dir));
    let (seen, f) = sink();
    let c = MeetingController::new(Arc::clone(&db), f).with_tick_interval(TEST_TICK);

    c.start(&dir, Some("Ticker".into())).expect("start");
    std::thread::sleep(TEST_TICK * 10);
    c.stop("test").expect("stop");

    let ticks = seen.ticks.load(Ordering::SeqCst);
    // One `Started` status plus roughly ten ticks. The window is wide because a
    // loaded CI box schedules late; the point is the ORDER of magnitude — a
    // rAF-driven clock would be in the hundreds.
    assert!(
        (5..=16).contains(&ticks),
        "expected ~11 recording statuses over 10 intervals, got {ticks}"
    );

    // And every one of them carried a rendered clock, so the pill and the window
    // cannot format it differently.
    for s in seen.seen.lock().unwrap().iter().filter(|s| s.recording) {
        assert_eq!(s.elapsed_label.len(), 8, "hh:mm:ss");
        assert_eq!(
            s.elapsed_label,
            wilson_voice_lib::meetings::format_offset(s.elapsed_seconds as f64)
        );
    }
}

/// The production cadence is one second. Named so a future "make the pill
/// smoother" change has to argue with OS-12 explicitly.
#[test]
fn the_shipped_tick_is_one_hertz() {
    assert_eq!(TICK_INTERVAL, Duration::from_secs(1));
}

/// A second press while recording must not open a second meeting.
#[test]
fn a_meeting_cannot_be_started_twice() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = temp_dir("yv95-double");
    let db = Arc::new(open_db(&dir));
    let (_seen, f) = sink();
    let c = MeetingController::new(Arc::clone(&db), f).with_tick_interval(TEST_TICK);

    c.start(&dir, None).expect("first start");
    let err = c.start(&dir, None).expect_err("second start must refuse");
    assert!(
        err.contains("already recording"),
        "unhelpful message: {err}"
    );
    c.stop("test").unwrap();
    assert_eq!(db.list_meetings(10, None).unwrap().len(), 1);
}

/// Stopping when nothing is running is an error to the caller, never a panic and
/// never a phantom row.
#[test]
fn stopping_nothing_is_not_a_meeting() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    let dir = temp_dir("yv95-nostop");
    let db = Arc::new(open_db(&dir));
    let (_seen, f) = sink();
    let c = MeetingController::new(Arc::clone(&db), f);
    assert!(c.stop("test").is_err());
    assert_eq!(db.list_meetings(10, None).unwrap().len(), 0);
}

/// A capture engine that refuses must not leave an empty `failed` meeting in the
/// user's list every time they press the hotkey by accident.
#[test]
fn a_refused_start_leaves_no_row_behind() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_START_FAILS);
    let dir = temp_dir("yv95-refuse");
    let db = Arc::new(open_db(&dir));
    let (_seen, f) = sink();
    let c = MeetingController::new(Arc::clone(&db), f);

    let err = c.start(&dir, None).expect_err("start must fail");
    assert!(err.contains("no input device"), "message lost: {err}");
    assert!(!c.is_recording());
    assert_eq!(
        db.list_meetings(10, None).unwrap().len(),
        0,
        "a failed start must not litter the Meetings list"
    );
    support::set_fake_mode(support::FAKE_OK);
}

/// Capture that ran but landed nothing is `partial` with the reason attached —
/// not `complete`, and not a silent success.
#[test]
fn capture_that_lands_no_audio_is_partial_and_says_why() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_NO_AUDIO);
    let dir = temp_dir("yv95-partial");
    let db = Arc::new(open_db(&dir));
    let (_seen, f) = sink();
    let c = MeetingController::new(Arc::clone(&db), f);

    let id = c.start(&dir, None).expect("start");
    let stopped = c.stop("user stopped").expect("stop");
    assert_eq!(stopped.state, MeetingState::Partial);

    let row = db.get_meeting(&id).unwrap().unwrap();
    assert_eq!(row.state, MeetingState::Partial.as_str());
    assert_eq!(row.error.as_deref(), Some("no audio reached the disk"));
    assert!(row.mic_wav_path.is_none());
    assert!(!row.audio_kept, "there is no audio to keep");
    support::set_fake_mode(support::FAKE_OK);
}

/// A capture that errors on the way out still closes its row: the meeting is
/// `partial` with the device's own message, and the wall clock stands in for the
/// duration the audio clock could not report.
#[test]
fn a_capture_that_fails_on_stop_still_closes_its_row() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_STOP_FAILS);
    let dir = temp_dir("yv95-stopfail");
    let db = Arc::new(open_db(&dir));
    let (_seen, f) = sink();
    let c = MeetingController::new(Arc::clone(&db), f);

    let id = c.start(&dir, None).expect("start");
    std::thread::sleep(Duration::from_millis(20));
    let stopped = c
        .stop("user stopped")
        .expect("stop reports the failure, not a panic");
    assert_eq!(stopped.state, MeetingState::Partial);

    let row = db.get_meeting(&id).unwrap().unwrap();
    assert!(row.ended_at.is_some(), "the row must not stay open forever");
    assert!(row.duration_seconds > 0.0, "wall clock stands in");
    assert_eq!(
        row.error.as_deref(),
        Some("device disappeared mid-meeting"),
        "the device's own words reach the meeting detail"
    );
    assert!(!c.is_recording(), "a failed stop still ends the session");
    support::set_fake_mode(support::FAKE_OK);
}

/// Quitting with a meeting live finalizes it. The alternative — an
/// `in_progress` row nothing will ever close — is the failure this phase exists
/// to prevent.
#[test]
fn quitting_finalizes_a_live_meeting() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = temp_dir("yv95-quit");
    let db = Arc::new(open_db(&dir));
    let (_seen, f) = sink();
    let c = MeetingController::new(Arc::clone(&db), f);

    let id = c.start(&dir, None).expect("start");
    c.stop_if_running("app quit");
    assert!(!c.is_recording());
    let row = db.get_meeting(&id).unwrap().unwrap();
    assert!(row.ended_at.is_some());
    let diag = row.diagnostics.expect("diagnostics");
    assert!(
        diag.contains("app quit"),
        "the stop reason is recorded so a short meeting is explainable later: {diag}"
    );
    // Idempotent — a second shutdown hook must not error or double-close.
    c.stop_if_running("app quit");
}

/// The message shown when nothing is installed behind the button. Asserted so
/// the UI copy and the backend cannot drift into "" or "error".
#[test]
fn the_unavailable_message_says_what_is_missing() {
    assert!(NO_ENGINE_MESSAGE.contains("capture engine"));
    assert!(!NO_ENGINE_MESSAGE.is_empty());
}

// ───────────── regression guards for the final review's BLOCKING findings ────

/// **BLOCKING finding 2 — `stop()` joined the 1 Hz ticker on the main thread.**
///
/// `stop()` runs on the macOS MAIN THREAD whenever the user reaches for the tray
/// item or ⌃⌘M. It used to set an `AtomicBool` and then `join()` the ticker —
/// which was, at that moment, in the middle of a `thread::sleep(TICK_INTERVAL)`.
/// The join could not return until that sleep expired, so every stop from a menu
/// or a hotkey froze the menu bar for up to a full second before any of the real
/// finalize work had even started.
///
/// The ticker now parks on a condvar, so stopping WAKES it instead of waiting
/// for it to look. This test states that as a bound: with a deliberately long
/// tick, `stop()` must return in a small fraction of it.
///
/// Falsify by putting `thread::sleep(tick)` back in `spawn_ticker`'s loop: the
/// elapsed time becomes ~`SLOW_TICK` and this fails by name.
#[test]
fn stopping_a_meeting_does_not_wait_out_a_tick() {
    /// Far longer than production's 1 s, so a fixed-cost stop and a
    /// waits-for-the-sleep stop cannot be confused on any machine.
    const SLOW_TICK: Duration = Duration::from_secs(5);
    /// Generous enough to absorb a loaded CI runner and the real finalize work
    /// (a wav write and three SQLite statements), and still nowhere near
    /// `SLOW_TICK`. The defect this guards produces ~5 s here, not 1.5 s.
    const CEILING: Duration = Duration::from_millis(1_500);

    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = temp_dir("yv95-stop-latency");
    let db = Arc::new(open_db(&dir));
    let (_seen, f) = sink();
    let c = MeetingController::new(Arc::clone(&db), f).with_tick_interval(SLOW_TICK);

    c.start(&dir, None).expect("start");
    // Land INSIDE the ticker's first park, which is where the old join blocked.
    std::thread::sleep(Duration::from_millis(150));

    let began = std::time::Instant::now();
    c.stop("user stopped").expect("stop");
    let took = began.elapsed();

    assert!(
        took < CEILING,
        "stop() took {took:?} — it is waiting out the {SLOW_TICK:?} tick instead of \
         waking the ticker, and this call runs on the macOS main thread for the \
         tray item and ⌃⌘M"
    );
    assert!(!c.is_recording());
}

/// A meeting whose ticker never got to run at all still stops promptly: the
/// condvar's notify has to be safe against a thread that has not reached its
/// first `wait` yet, which is why the flag is set under the same lock.
#[test]
fn stopping_immediately_after_starting_is_not_a_race() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = temp_dir("yv95-stop-immediate");
    let db = Arc::new(open_db(&dir));
    let (_seen, f) = sink();
    let c = MeetingController::new(Arc::clone(&db), f).with_tick_interval(Duration::from_secs(30));

    for _ in 0..5 {
        c.start(&dir, None).expect("start");
        let began = std::time::Instant::now();
        c.stop("user stopped").expect("stop");
        assert!(
            began.elapsed() < Duration::from_millis(1_500),
            "an immediate stop waited on the 30 s tick"
        );
    }
}

/// **The `partial` seam with YV91 (#108), which merged while this branch was
/// open.** A capture that lands a wav but reports the recording as short must
/// close as `partial`, not as `transcribing` — otherwise YV93 walks it on to
/// `complete` and nothing is left saying the meeting is missing a piece.
#[test]
fn a_short_recording_that_still_wrote_a_wav_is_partial() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_PARTIAL);
    let dir = temp_dir("yv95-partial");
    let db = Arc::new(open_db(&dir));
    let (_seen, f) = sink();
    let c = MeetingController::new(Arc::clone(&db), f).with_tick_interval(TEST_TICK);

    c.start(&dir, None).expect("start");
    let stopped = c.stop("user stopped").expect("stop");
    support::set_fake_mode(support::FAKE_OK);

    assert_eq!(
        stopped.state,
        MeetingState::Partial,
        "a wav landed, but the engine said the recording is short — \
         `transcribing` would file it as whole"
    );
    assert!(
        stopped.note.is_some(),
        "a partial meeting must say why in the row, not only in the log"
    );
    let row = db.get_meeting(&stopped.id).expect("row").expect("row");
    assert_eq!(row.state, MeetingState::Partial.as_str());
    assert!(
        row.mic_wav_path.is_some(),
        "the audio that WAS captured is still findable"
    );
}
