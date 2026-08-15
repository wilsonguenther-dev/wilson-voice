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

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use support::{open_db, temp_dir};
use wilson_voice_lib::meeting_control::{
    MeetingController, MeetingStatus, StatusSink, NO_ENGINE_MESSAGE, TICK_INTERVAL,
};
use wilson_voice_lib::meetings::MeetingState;

/// A short tick so the cadence is testable in a second instead of a minute. The
/// production value is asserted separately, below.
const TEST_TICK: Duration = Duration::from_millis(60);

/// How long anything here waits on the controller's ticker thread before calling
/// it stuck. A HANG guard, never a measurement: it is an order of magnitude
/// longer than the work it waits for, so reaching it means the ticker died — not
/// that the runner was busy. See [`support::wait_until`].
const TICK_DEADLINE: Duration = Duration::from_secs(30);

/// One status as it reached the UI, stamped when the sink received it.
///
/// The receipt time is what makes the clock checkable against something: an
/// `elapsed_seconds` of 2 is only correct if roughly two seconds of real time
/// had passed when it arrived, and only the sink can say when that was.
#[derive(Clone)]
struct Emit {
    at: Instant,
    status: MeetingStatus,
}

/// Collects every status the controller emits — the same sink `lib.rs` fills
/// with "emit to both windows and flip the pill's energy flag".
#[derive(Default)]
struct Sink {
    seen: Mutex<Vec<Emit>>,
}

impl Sink {
    fn statuses(&self) -> Vec<MeetingStatus> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.status.clone())
            .collect()
    }

    /// Every emit that said "recording", in arrival order.
    fn recording_emits(&self) -> Vec<Emit> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.status.recording)
            .cloned()
            .collect()
    }

    /// The highest elapsed value the payload has carried so far — the ONLY
    /// progress signal a cadence test should wait on, because it is the thing
    /// under test rather than a proxy for how often the scheduler ran.
    fn payload_seconds(&self) -> u64 {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.status.recording)
            .map(|e| e.status.elapsed_seconds)
            .max()
            .unwrap_or(0)
    }
}

fn sink() -> (Arc<Sink>, StatusSink) {
    let collected = Arc::new(Sink::default());
    let c = Arc::clone(&collected);
    let f: StatusSink = Arc::new(move |s: &MeetingStatus| {
        c.seen.lock().unwrap().push(Emit {
            at: Instant::now(),
            status: s.clone(),
        });
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

    // Let the clock run — waiting on the ticker's own output rather than on a
    // sleep, so this is "the meeting was live long enough to tick" on any box.
    support::wait_until(
        "the ticker to emit while the meeting is live",
        TICK_DEADLINE,
        || seen.recording_emits().len() >= 2,
    );

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
    let last = seen.statuses().last().cloned().unwrap();
    assert!(!last.recording);
    assert_eq!(last.elapsed_label, "00:00:00");
}

/// OS-12 fix (1): elapsed time is a 1 Hz emit from Rust, not a timer in the
/// webview.
///
/// **Why this is not `sleep(10 * TEST_TICK)` + "5 to 16 notifications arrived"
/// (YV111).** It was, and that shape asserts on the SCHEDULER: how many times a
/// background thread got to run inside a 600 ms wall-clock window is a property
/// of how busy the box is, and a loaded runner falls out of any window narrow
/// enough to still catch the defect. It failed three separate CI runs across the
/// yap22a/yap22b reviews (`expected ~11 recording statuses over 10 intervals,
/// got 4`) without the controller ever being wrong. Widening the window trades a
/// flake for a test that no longer fails when the clock breaks.
///
/// What OS-12 actually claims is about the PAYLOAD, and the payload can be
/// checked directly:
///
/// * **The clock never runs fast.** `elapsed_seconds` is `floor` of a monotonic
///   `Instant`, so it can never exceed the real time observed at the sink. This
///   direction is arithmetic — no amount of load can break it — and it is the
///   half that a webview `setInterval`/rAF clock (which counts frames, not
///   seconds) gets wrong.
/// * **The clock never stalls.** By the time the payload says `n`, no more than
///   `n + 1 s + slack` of real time has passed, so a clock counting at half rate
///   or wedged behind a stuck ticker fails.
/// * **It advances monotonically, in whole seconds**, starting from a zeroed
///   clock, with the label the UI renders derived from that same number. (Not
///   "every second appears": a thread that wakes late may skip one, and that is
///   the scheduler's business, not the clock's.)
/// * **Emits are spaced by the interval** — the "and not more" half. Asserted as
///   a per-emit MINIMUM gap, which load can only ever lengthen; a 60 Hz webview
///   clock lands at ~16 ms and fails by name.
///
/// The run is driven by the payload's own progress ([`Sink::payload_seconds`])
/// with a generous hang deadline, so a slow box makes this test slower and never
/// makes it fail.
#[test]
fn the_elapsed_clock_advances_one_second_per_second_and_emits_on_the_interval() {
    /// Whole seconds of payload clock to watch. Three is enough for a half-rate
    /// clock to fall outside [`LAG_CEILING`] and short enough to keep the file's
    /// exclusive lock brief.
    const WATCH_SECONDS: u64 = 3;
    /// How far the payload may lag real time: one second of `floor` truncation
    /// plus a second of scheduling slack. A ticker starved for a full second
    /// between emits still passes; a clock counting at half rate passes 2 s of
    /// lag just after the third second and cannot — which is what fixes
    /// [`WATCH_SECONDS`] at three.
    const LAG_CEILING: f64 = 1.0 + 1.0;

    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = temp_dir("yv95-tick");
    let db = Arc::new(open_db(&dir));
    let (seen, f) = sink();
    let c = MeetingController::new(Arc::clone(&db), f).with_tick_interval(TEST_TICK);

    // Taken BEFORE `start`, so this reference is never later than the
    // controller's own — which is what makes "the clock never runs fast"
    // unfalsifiable by scheduling.
    let before_start = Instant::now();
    c.start(&dir, Some("Ticker".into())).expect("start");
    support::wait_until(
        &format!("the elapsed payload to reach {WATCH_SECONDS} s"),
        TICK_DEADLINE,
        || seen.payload_seconds() >= WATCH_SECONDS,
    );
    c.stop("test").expect("stop");

    let emits = seen.recording_emits();
    assert!(
        emits.len() >= 2,
        "a recording meeting emitted {} statuses — there is no cadence to check",
        emits.len()
    );

    for (i, e) in emits.iter().enumerate() {
        let observed = e.at.duration_since(before_start).as_secs_f64();
        let payload = e.status.elapsed_seconds as f64;

        assert!(
            payload <= observed,
            "the clock reported {payload}s at {observed:.3}s of real time — a clock \
             that runs FAST is a rendered timer, not the meeting's elapsed time"
        );
        assert!(
            observed - payload <= LAG_CEILING,
            "the clock reported {payload}s at {observed:.3}s of real time — it is \
             stalled or counting slow (lag ceiling {LAG_CEILING}s)"
        );
        assert_eq!(
            e.status.elapsed_label,
            wilson_voice_lib::meetings::format_offset(payload),
            "the pill and the window render one number, formatted once in Rust"
        );
        assert_eq!(e.status.elapsed_label.len(), 8, "hh:mm:ss");

        let Some(p) = i.checked_sub(1).map(|j| &emits[j]) else {
            continue;
        };
        assert!(
            e.status.elapsed_seconds >= p.status.elapsed_seconds,
            "the clock went backwards: {}s after {}s",
            e.status.elapsed_seconds,
            p.status.elapsed_seconds
        );
        // The cadence itself. `emits[0]` is the synchronous status `start()`
        // returns with, not a tick, so the interval between it and the first
        // tick is not a sample of the ticker's cadence — every pair after it is.
        if i >= 2 {
            let gap = e.at.duration_since(p.at);
            assert!(
                gap >= TEST_TICK / 2,
                "two statuses {gap:?} apart on a {TEST_TICK:?} tick — the emit is \
                 free-running rather than interval-driven, which is the webview \
                 timer OS-12 removed"
            );
        }
    }

    // Non-vacuous: the UI's first sight of the clock is zero, and the clock
    // really did advance from there. (The values BETWEEN the ends are not
    // asserted — a late-scheduled ticker is allowed to skip a second, which is
    // the whole reason this test no longer counts anything.)
    let path: Vec<u64> = {
        let mut v: Vec<u64> = emits.iter().map(|e| e.status.elapsed_seconds).collect();
        v.dedup();
        v
    };
    assert_eq!(
        path.first().copied(),
        Some(0),
        "the first thing the UI hears is a zeroed clock: {path:?}"
    );
    assert!(
        path.last().copied().unwrap_or(0) >= WATCH_SECONDS,
        "the payload never reached {WATCH_SECONDS}s: {path:?}"
    );
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
