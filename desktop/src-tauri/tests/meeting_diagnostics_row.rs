//! YV95 / OS-12 fix (3) — "log `thermalState` transitions and a battery
//! preflight reading into the meeting row so a bad session is diagnosable after
//! the fact, not guessed at."
//!
//! The backlog lists this as a manual check ("thermal-state and battery-preflight
//! values appear in a completed meeting's diagnostic row"). A manual check of a
//! thermal RAMP would need a hot Mac and a stopwatch, so the probe is an
//! injectable trait and the ramp is synthetic — which also makes the interesting
//! half testable at all: that a machine which starts nominal, goes serious in the
//! middle and settles back records THREE transitions rather than 10,800 samples.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use support::{open_db, temp_dir};
use wilson_voice_lib::meeting_control::{
    EnergyProbe, MeetingController, MeetingStatus, StatusSink,
};
use wilson_voice_lib::meeting_energy::{
    battery_from_raw, BatteryReading, MeetingDiagnostics, ThermalState, TIME_REMAINING_UNLIMITED,
};

const TEST_TICK: Duration = Duration::from_millis(40);

/// How long a ramp assertion will wait for the controller's ticker thread. Long
/// enough that a runner starved for a full second per tick still passes, short
/// enough that a genuinely stuck ticker fails the suite instead of hanging it.
const RAMP_DEADLINE: Duration = Duration::from_secs(30);

fn quiet_sink() -> StatusSink {
    Arc::new(|_: &MeetingStatus| {})
}

/// Block until `cond` holds, or fail with `what` after [`RAMP_DEADLINE`].
///
/// The thermal ramp below advances one step per read on the CONTROLLER's ticker
/// thread, so "sleep `n * TEST_TICK`, then assert the exact ramp path" is a bet
/// that the scheduler ran that thread `n` times — which a loaded CI box loses
/// (observed: 2 samples where 6 were needed, `[Fair]` vs `[Fair, Serious,
/// Fair]`). Widening the sleep does not fix a starvable thread; waiting on the
/// thread's own progress counter does, and turns the wall-clock race into a
/// timeout that names what never happened.
///
/// YV111 lifted the body into [`support::wait_until`] when the same bet lost
/// again next door, in `meeting_manual_start_stop`. One idiom, one place to
/// argue with.
fn wait_for(what: &str, cond: impl FnMut() -> bool) {
    support::wait_until(what, RAMP_DEADLINE, cond);
}

/// Walks a fixed thermal ramp, one step per read, then holds the last value —
/// a machine that heats up during a long meeting and stays hot.
struct RampProbe {
    ramp: Vec<ThermalState>,
    reads: AtomicUsize,
    battery: BatteryReading,
    pill_visible: bool,
}

impl EnergyProbe for RampProbe {
    fn thermal(&self) -> ThermalState {
        let i = self.reads.fetch_add(1, Ordering::SeqCst);
        *self
            .ramp
            .get(i)
            .unwrap_or_else(|| self.ramp.last().unwrap())
    }
    fn battery(&self) -> BatteryReading {
        self.battery
    }
    fn pill_visible(&self) -> bool {
        self.pill_visible
    }
}

impl RampProbe {
    /// How many steps of the ramp the ticker has consumed so far. The one
    /// observable that makes "the whole ramp was sampled" a fact rather than a
    /// hope about the scheduler.
    fn steps_read(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    /// True once every ramp value has been *observed into the diagnostics*, not
    /// merely read.
    ///
    /// The ticker body is strictly sequential — read, observe, emit, sleep — so
    /// the `(n+1)`-th read can only start after the `n`-th sample was folded in
    /// under the diagnostics lock. Waiting for one read past the end of the ramp
    /// is therefore the cheapest happens-before proof available from outside the
    /// controller.
    fn ramp_fully_observed(&self) -> bool {
        self.steps_read() > self.ramp.len()
    }
}

#[test]
fn a_completed_meeting_carries_its_thermal_and_battery_readings() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = temp_dir("yv95-diag");
    let db = Arc::new(open_db(&dir));

    // Started on battery at 62% with 96 minutes left — the reading OS-12 wants
    // in the row so "why did this meeting stop at 40 minutes" is answerable.
    let battery = battery_from_raw(Some("Battery Power"), Some(6200), Some(10000), 96.0 * 60.0);
    let probe = Arc::new(RampProbe {
        ramp: vec![
            ThermalState::Nominal,
            ThermalState::Nominal,
            ThermalState::Fair,
            ThermalState::Fair,
            ThermalState::Serious,
            ThermalState::Fair,
        ],
        reads: AtomicUsize::new(0),
        battery,
        pill_visible: true,
    });
    let c = MeetingController::new(Arc::clone(&db), quiet_sink())
        .with_probe(Arc::clone(&probe) as Arc<dyn EnergyProbe>)
        .with_tick_interval(TEST_TICK);

    let id = c.start(&dir, Some("Hot laptop".into())).expect("start");
    // NOT `sleep(TEST_TICK * 8)`: the assertions below are exact equality on the
    // ramp path, which is only true once the ticker has walked the whole ramp.
    // Wait for the ramp itself, so the test is a statement about the controller
    // and not about how busy the runner was.
    wait_for("the whole thermal ramp to be sampled", || {
        probe.ramp_fully_observed()
    });
    c.stop("user stopped").expect("stop");

    let row = db.get_meeting(&id).unwrap().unwrap();
    let blob = row.diagnostics.expect("the row carries a diagnostics blob");
    let d: MeetingDiagnostics = serde_json::from_str(&blob).expect("valid JSON");

    assert_eq!(d.version, MeetingDiagnostics::VERSION);
    assert_eq!(d.thermal_at_start, ThermalState::Nominal);
    // nominal → fair → serious → fair. Three transitions out of the six samples
    // we waited for, because identical samples are not events.
    let path: Vec<ThermalState> = d.thermal_transitions.iter().map(|t| t.to).collect();
    assert_eq!(
        path,
        vec![
            ThermalState::Fair,
            ThermalState::Serious,
            ThermalState::Fair
        ],
        "every real change is recorded, and nothing else is"
    );
    assert!(
        d.thermal_transitions.iter().all(|t| t.at_seconds >= 0.0),
        "each transition is stamped with where in the meeting it happened"
    );
    assert_eq!(d.thermal_at_end, Some(ThermalState::Fair));

    // The battery preflight.
    assert!(!d.battery_at_start.on_ac);
    assert_eq!(d.battery_at_start.percent, Some(62));
    assert_eq!(d.battery_at_start.minutes_remaining, Some(96));
    assert!(d.battery_at_end.is_some(), "and a closing sample");

    // OS-12's whole argument is about the PILL, so a power number without this
    // bit is uninterpretable.
    assert!(d.pill_visible);
    assert_eq!(d.stop_reason.as_deref(), Some("user stopped"));
}

/// The preflight is written BEFORE the first sample is captured, so a meeting
/// the app never got to close cleanly still says what the machine looked like
/// when it began.
#[test]
fn the_preflight_is_on_the_row_while_the_meeting_is_still_recording() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = temp_dir("yv95-preflight");
    let db = Arc::new(open_db(&dir));
    let probe = Arc::new(RampProbe {
        ramp: vec![ThermalState::Fair],
        reads: AtomicUsize::new(0),
        battery: battery_from_raw(
            Some("AC Power"),
            Some(100),
            Some(100),
            TIME_REMAINING_UNLIMITED,
        ),
        pill_visible: false,
    });
    let c = MeetingController::new(Arc::clone(&db), quiet_sink()).with_probe(probe);

    let id = c.start(&dir, None).expect("start");
    let blob = db
        .get_meeting(&id)
        .unwrap()
        .unwrap()
        .diagnostics
        .expect("preflight is written at start, not only at stop");
    let d: MeetingDiagnostics = serde_json::from_str(&blob).unwrap();
    assert_eq!(d.thermal_at_start, ThermalState::Fair);
    assert!(d.battery_at_start.on_ac);
    assert_eq!(d.battery_at_start.percent, Some(100));
    assert!(d.thermal_at_end.is_none(), "it has not ended yet");
    assert!(d.stop_reason.is_none());
    c.stop("test").unwrap();
}

/// The column arrived as migration 2 and is nullable, so every meeting recorded
/// by an older build reads as "never measured" rather than as a broken row.
#[test]
fn meetings_from_before_this_build_have_no_diagnostics_and_that_is_fine() {
    let dir = temp_dir("yv95-null-diag");
    let db = open_db(&dir);
    assert_eq!(
        db.schema_version().unwrap(),
        wilson_voice_lib::meetings::SCHEMA_VERSION
    );
    let m = db.create_meeting("Older meeting", "manual").unwrap();
    assert!(db
        .get_meeting(&m.id)
        .unwrap()
        .unwrap()
        .diagnostics
        .is_none());
    // And the column is writable on its own, which is what the controller does
    // twice per meeting.
    db.set_meeting_diagnostics(&m.id, r#"{"version":1}"#)
        .unwrap();
    assert_eq!(
        db.get_meeting(&m.id)
            .unwrap()
            .unwrap()
            .diagnostics
            .as_deref(),
        Some(r#"{"version":1}"#)
    );
}

/// The real probes must answer SOMETHING on the machine running the tests —
/// this is the only place the IOKit/objc bridge itself is exercised, and a
/// silent wrong-symbol regression there would otherwise only show up as an
/// empty diagnostics blob on a user's machine.
#[test]
fn the_real_probes_answer_on_this_machine() {
    let thermal = wilson_voice_lib::meeting_energy::thermal();
    assert_ne!(
        thermal,
        ThermalState::Unknown,
        "ProcessInfo.thermalState should read on macOS"
    );
    let b = wilson_voice_lib::meeting_energy::battery();
    // A Mac mini legitimately has no battery, so the assertion is on
    // CONSISTENCY, not on there being one.
    if b.has_battery {
        assert!(b.percent.is_some(), "a battery that exists has a charge");
        assert!(b.percent.unwrap() <= 100);
    } else {
        assert!(b.on_ac, "no battery means wall power");
    }
    if let Some(mins) = b.minutes_remaining {
        assert!(mins > 0, "an estimate is minutes, never a sentinel");
    }
}
