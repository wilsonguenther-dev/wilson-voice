//! YV103 / plan finding OS-3, part (c) — "idempotent teardown that is safe to
//! re-enter".
//!
//! The two halves of the loop OS-3 documents are the debounce (proved in
//! `aggregate_rebuild_debounce.rs`) and *re-entrancy*: a rebuild takes on the
//! order of a second, notifications keep arriving throughout it, and the naive
//! handler starts a second rebuild on top of the first. What that costs is not
//! an extra second of silence — it is a **double destroy** (the second teardown
//! calls `AudioHardwareDestroyAggregateDevice` on an ID the first one already
//! freed) and a **leaked tap** (the second create overwrites the first's ID
//! before anyone stops it). Both are HAL-level, and CoreAudio's own answer to
//! being asked to destroy something twice is not a friendly error code.
//!
//! This file drives the state machine against a fake that *asserts* on the
//! double-destroy and double-create rather than counting them after the fact,
//! so a regression fails at the offending call instead of at a total at the end.
//!
//! The real create/destroy sequence itself is not re-declared here: the 7-step
//! order is `syscapture`'s (YV100), asserted by its own teardown-order test, and
//! YV104's watchdog *calls* that path rather than restating it. What YV103 owns
//! — and what this file proves — is the guard that decides whether the sequence
//! may run at all.

use wilson_voice_lib::input_format::{
    selectors, FormatChangeAction, FormatEventSource, InputFormat, InputFormatWatch,
    OutputObservation, DEFAULT_TARGET_RATE,
};

const BUILT_IN: &str = "BuiltInSpeakerDevice";
const AIRPODS: &str = "AirPodsPro-1a2b";
const STUDIO: &str = "StudioDisplaySpeakers";

fn speakers() -> InputFormat {
    InputFormat::new(48_000, 2)
}

fn tapping() -> InputFormatWatch {
    let mut watch = InputFormatWatch::new(
        "MacBook Pro Microphone",
        InputFormat::new(48_000, 1),
        DEFAULT_TARGET_RATE,
    );
    watch.watch_output(BUILT_IN, speakers());
    watch
}

fn ev(uid: &str, at_ms: u64, selector: u32) -> OutputObservation {
    OutputObservation {
        device_uid: uid.to_string(),
        format: speakers(),
        host_time: 7_310_000_000_000 + at_ms * 1_000_000,
        output_sample_index: at_ms * 16,
        source: FormatEventSource::from_selector(selector),
        at_ms,
    }
}

/// Stands in for the process tap + aggregate device pair `syscapture` owns. It
/// does not model the 7 steps (that is YV100's test); it models the only thing
/// the guard has to protect: that each one exists exactly zero or one times.
#[derive(Debug, Default)]
struct FakeTapDevices {
    tap_live: bool,
    aggregate_live: bool,
    destroys: usize,
    creates: usize,
}

impl FakeTapDevices {
    fn destroy(&mut self) {
        assert!(
            self.tap_live && self.aggregate_live,
            "double destroy: AudioHardwareDestroyAggregateDevice/ProcessTap called on \
             an ID that was already freed"
        );
        self.tap_live = false;
        self.aggregate_live = false;
        self.destroys += 1;
    }

    fn create(&mut self) {
        assert!(
            !self.tap_live && !self.aggregate_live,
            "double create: a second AudioHardwareCreateProcessTap/CreateAggregateDevice \
             overwrote a live ID, leaking the first"
        );
        self.tap_live = true;
        self.aggregate_live = true;
        self.creates += 1;
    }
}

/// The session-start path: take the guard, create, release it. This is exactly
/// Hyprnote's `initialization_complete` (PR #1654) — the FIRST aggregate create
/// of a meeting is the one whose own notifications used to restart the stream
/// before it had finished coming up.
fn start_session(watch: &mut InputFormatWatch, devices: &mut FakeTapDevices, output_uid: &str) {
    assert!(
        watch.begin_aggregate_work(),
        "nothing is in flight at session start"
    );
    devices.create();
    watch.finish_aggregate_work(output_uid, speakers());
}

#[test]
fn events_during_the_initial_aggregate_create_are_dropped_not_replayed() {
    let mut watch = tapping();
    let mut devices = FakeTapDevices::default();

    assert!(watch.begin_aggregate_work());
    devices.create();
    // Everything below is a notification our own create fired: the private
    // aggregate appearing in the device list, and the default-output selection
    // being re-announced. Before the guard, the monitor answered each of these
    // by restarting the stream, which fired more of them.
    for (n, (uid, selector)) in [
        ("com.wilsonguenther.yap.tap-agg", selectors::DEVICES),
        (BUILT_IN, selectors::DEFAULT_OUTPUT_DEVICE),
        ("com.wilsonguenther.yap.tap-agg", selectors::DEVICES),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            watch.observe_output(ev(uid, n as u64 * 10, selector)),
            FormatChangeAction::Ignored(InputFormatWatch::GUARDED),
        );
    }
    watch.finish_aggregate_work(BUILT_IN, speakers());

    assert_eq!(devices.creates, 1, "one create for one session start");
    assert_eq!(devices.destroys, 0);
    assert_eq!(watch.rebuilds_issued(), 0, "setup is not a rebuild");
    assert!(
        !watch.has_pending_rebuild(),
        "our own events left nothing owed"
    );
}

#[test]
fn a_rebuild_issued_while_a_rebuild_is_in_flight_is_a_no_op() {
    // The re-entrancy case proper: the rebuild is slow, the HAL keeps talking,
    // and a second genuine-looking device change lands in the middle of it.
    let mut watch = tapping();
    let mut devices = FakeTapDevices::default();
    start_session(&mut watch, &mut devices, BUILT_IN);

    watch.observe_output(ev(AIRPODS, 0, selectors::DEFAULT_OUTPUT_DEVICE));
    let action = watch.tick_output(600);
    assert!(action.is_rebuild_aggregate());
    assert!(
        watch.is_aggregate_work_in_flight(),
        "issuing the action takes the guard, so no caller has to remember to"
    );

    // The rebuild starts. Halfway through, a THIRD device shows up — a Studio
    // Display waking, say. The fake asserts on the double-destroy if the state
    // machine lets this through.
    devices.destroy();
    assert!(
        !watch.begin_aggregate_work(),
        "an explicit rebuild request must be refused while one is in flight — \
         this return value IS the caller's re-entrancy guard"
    );
    for at_ms in [610, 620, 700, 1_400] {
        assert_eq!(
            watch.observe_output(ev(STUDIO, at_ms, selectors::DEFAULT_OUTPUT_DEVICE)),
            FormatChangeAction::Ignored(InputFormatWatch::GUARDED),
        );
        assert_eq!(
            watch.tick_output(at_ms),
            FormatChangeAction::Ignored(InputFormatWatch::GUARDED),
        );
    }
    devices.create();
    watch.finish_aggregate_work(AIRPODS, speakers());

    assert_eq!(devices.destroys, 1, "exactly one destroy");
    assert_eq!(devices.creates, 2, "the session start plus one rebuild");
    assert_eq!(watch.rebuilds_issued(), 1);
    assert!(devices.tap_live && devices.aggregate_live);
}

#[test]
fn releasing_the_guard_twice_is_safe_and_the_watch_still_works_afterwards() {
    // A rebuild that fails partway has to release the guard too — a watch left
    // guarded is a watch that is deaf for the rest of the meeting, which is the
    // failure mode the guard was supposed to prevent, arriving by the other
    // door. Callers therefore release on every path, and belt-and-braces
    // callers release twice.
    let mut watch = tapping();
    let mut devices = FakeTapDevices::default();
    start_session(&mut watch, &mut devices, BUILT_IN);

    watch.observe_output(ev(AIRPODS, 0, selectors::DEVICES));
    assert!(watch.tick_output(500).is_rebuild_aggregate());
    devices.destroy();
    // …and the create fails. The caller still releases, adopting whatever the
    // OS says the output is now.
    watch.finish_aggregate_work(AIRPODS, speakers());
    watch.finish_aggregate_work(AIRPODS, speakers());
    assert!(!watch.is_aggregate_work_in_flight());

    // The watch is not deaf: the next genuine change is still heard, and the
    // caller can still take the guard.
    watch.observe_output(ev(STUDIO, 2_000, selectors::DEFAULT_OUTPUT_DEVICE));
    let action = watch.tick_output(2_600);
    assert!(action.is_rebuild_aggregate(), "got {action:?}");
    devices.create();
    watch.finish_aggregate_work(STUDIO, speakers());

    assert_eq!(devices.destroys, 1);
    assert_eq!(devices.creates, 2);
    assert_eq!(watch.rebuilds_issued(), 2);
    assert_eq!(watch.output_segment_index(), 2, "two tap-track segments");
}

#[test]
fn a_failed_device_read_on_release_keeps_the_last_known_good_device_not_a_blank_one() {
    // The third door into the same "deaf for the rest of the meeting" failure.
    // `finish_aggregate_work` must be called on the FAILURE path, and the
    // natural UID on that path is the empty string — CoreAudio was asked for the
    // current default output and did not answer. Adopting it unconditionally
    // empties `output_device_uid`, after which `observe_output` short-circuits
    // on NO_OUTPUT_WATCHED before it ever compares devices: no notification and
    // no watchdog re-read can reach the rebuild path again, so the tap stays
    // pointed at the wrong device for the rest of the meeting. Releasing the
    // guard correctly would not save it, because the guard is not what is shut.
    //
    // So a blank reading is treated as "no reading" — the same rule the format
    // half already follows via `is_usable()` — and the last known-good device
    // stands until a real one replaces it.
    let mut watch = tapping();
    let mut devices = FakeTapDevices::default();
    start_session(&mut watch, &mut devices, BUILT_IN);

    watch.observe_output(ev(AIRPODS, 0, selectors::DEFAULT_OUTPUT_DEVICE));
    assert!(watch.tick_output(500).is_rebuild_aggregate());
    devices.destroy();
    devices.create();
    // The post-rebuild device read fails. The caller still releases — it must —
    // and hands over what it got: nothing. Same for the format, which is
    // already guarded and stays here as the control.
    watch.finish_aggregate_work("", InputFormat::new(0, 0));
    assert!(!watch.is_aggregate_work_in_flight());
    assert_eq!(
        watch.output_device_uid(),
        AIRPODS,
        "a failed read must not blank the watched device"
    );
    assert_eq!(watch.output_format(), speakers());

    // An hour of genuine changes afterwards. Every one of these returned
    // Ignored(NO_OUTPUT_WATCHED) before the fix, and `rebuilds_issued` never
    // moved past 1.
    let mut now = 60_000_u64;
    for round in 0..4_u32 {
        let target = if round % 2 == 0 { STUDIO } else { AIRPODS };
        watch.observe_output(ev(target, now, selectors::DEFAULT_OUTPUT_DEVICE));
        now += 600;
        let action = watch.tick_output(now);
        assert!(
            action.is_rebuild_aggregate(),
            "round {round}: the watch must still hear genuine changes, got {action:?}"
        );
        assert_eq!(action.marker().unwrap().to_device, target);
        devices.destroy();
        devices.create();
        watch.finish_aggregate_work(target, speakers());
        now += 900_000;
    }

    assert_eq!(
        watch.rebuilds_issued(),
        5,
        "one before the failure, four after"
    );
    assert_eq!(devices.destroys, 5);
    assert_eq!(devices.creates, 6, "one session start plus five rebuilds");
    assert!(devices.tap_live && devices.aggregate_live);
}

#[test]
fn a_user_switch_that_lands_inside_the_guard_window_is_recovered_by_the_watchdog() {
    // The honest cost of the guard, stated rather than hidden: it cannot tell a
    // genuine user-initiated switch that happens DURING our create/destroy from
    // one of our own notifications, so it drops both. That is the right trade
    // (a dropped switch costs one watchdog interval; a missed guard costs the
    // capture), and this is the mechanism that makes it recoverable:
    // `finish_aggregate_work` adopts what the OS actually reports, so if the
    // user moved the output mid-rebuild the adopted UID disagrees with reality
    // and the next re-read sees it.
    let mut watch = tapping();
    let mut devices = FakeTapDevices::default();
    start_session(&mut watch, &mut devices, BUILT_IN);

    watch.observe_output(ev(AIRPODS, 0, selectors::DEFAULT_OUTPUT_DEVICE));
    assert!(watch.tick_output(500).is_rebuild_aggregate());
    devices.destroy();
    // Mid-rebuild the user plugs in a Studio Display and macOS switches to it.
    // Dropped, along with our own noise.
    assert_eq!(
        watch.observe_output(ev(STUDIO, 700, selectors::DEFAULT_OUTPUT_DEVICE)),
        FormatChangeAction::Ignored(InputFormatWatch::GUARDED),
    );
    devices.create();
    // The rebuild landed on AirPods, which is what it was aimed at.
    watch.finish_aggregate_work(AIRPODS, speakers());
    assert_eq!(watch.rebuilds_issued(), 1);

    // The watchdog's own periodic re-read — source `Watchdog`, no notification
    // involved, which is the belt-and-braces path that exists precisely because
    // notifications can be missed.
    let recheck = OutputObservation {
        device_uid: STUDIO.to_string(),
        format: speakers(),
        host_time: 7_311_000_000_000,
        output_sample_index: 96_000,
        source: FormatEventSource::Watchdog,
        at_ms: 60_000,
    };
    assert_eq!(
        watch.observe_output(recheck),
        FormatChangeAction::Ignored(InputFormatWatch::COALESCING),
    );
    let action = watch.tick_output(60_500);
    assert!(
        action.is_rebuild_aggregate(),
        "the dropped switch is recovered on the next re-read, got {action:?}"
    );
    assert_eq!(action.marker().unwrap().to_device, STUDIO);
    assert_eq!(
        action.marker().unwrap().source,
        FormatEventSource::Watchdog,
        "and the marker says the watchdog found it, not a notification"
    );
    devices.destroy();
    devices.create();
    watch.finish_aggregate_work(STUDIO, speakers());
    assert_eq!(devices.destroys, 2);
    assert_eq!(devices.creates, 3);
}

#[test]
fn the_guard_survives_a_hundred_interleaved_rebuild_and_notification_rounds() {
    // A soak over the exact interleaving the loop needs — rebuild, notification
    // storm during it, release, repeat — because the bug OS-3 documents is not a
    // single wrong decision but a decision that feeds itself. The fake asserts
    // on every double-destroy and double-create along the way, so this fails at
    // the first offending call rather than at a total.
    let mut watch = tapping();
    let mut devices = FakeTapDevices::default();
    start_session(&mut watch, &mut devices, BUILT_IN);

    let mut now = 0_u64;
    for round in 0..100_u64 {
        let target = if round % 2 == 0 { AIRPODS } else { BUILT_IN };
        watch.observe_output(ev(target, now, selectors::DEFAULT_OUTPUT_DEVICE));
        now += 600;
        let action = watch.tick_output(now);
        assert!(action.is_rebuild_aggregate(), "round {round}: {action:?}");
        assert_eq!(action.marker().unwrap().segment_index, round as u32 + 1);
        devices.destroy();
        // The storm our own teardown/create emits, plus the settling
        // notifications that arrive just after the guard opens.
        for k in 0..8 {
            now += 3;
            watch.observe_output(ev(target, now, selectors::DEVICES));
            if k == 4 {
                watch.observe_output(ev(target, now, selectors::DEFAULT_OUTPUT_DEVICE));
            }
        }
        devices.create();
        watch.finish_aggregate_work(target, speakers());
        for _ in 0..3 {
            now += 3;
            assert_eq!(
                watch.observe_output(ev(target, now, selectors::DEVICES)),
                FormatChangeAction::Unchanged,
                "round {round}: settling notifications for the device we are ON"
            );
        }
    }
    assert_eq!(watch.rebuilds_issued(), 100);
    assert_eq!(devices.destroys, 100);
    assert_eq!(devices.creates, 101, "one session start plus 100 rebuilds");
}
