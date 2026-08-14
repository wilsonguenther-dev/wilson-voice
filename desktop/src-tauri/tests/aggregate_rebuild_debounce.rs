//! YV103 / plan finding OS-3 — the ≥500 ms coalescing debounce on the output
//! half of `InputFormatWatch`.
//!
//! The failure this exists for: *one* AirPods connect is not one notification.
//! OS-3 names the burst exactly — "a single AirPods connect emits a burst across
//! `kAudioHardwarePropertyDefaultOutputDevice`,
//! `kAudioHardwarePropertyDefaultInputDevice`, `kAudioHardwarePropertyDevices`
//! and `kAudioDevicePropertyStreamFormat`" — and the tap's aggregate device
//! takes roughly a second to destroy and recreate. Rebuilding once per
//! notification means six overlapping teardowns of the same device, each one
//! emitting its own notifications, which is the other half of the infinite loop
//! Hyprnote hit (PR #1654).
//!
//! Zero audio hardware, on purpose: OS-3's own instruction is to make the pure
//! `event sequence in → rebuild decisions out` function the acceptance criterion
//! "rather than the current 'device-change mid-meeting doesn't lose audio',
//! which is a manual repro nobody will run 40 times." The observation carries
//! its own millisecond stamp so the window is exercised without sleeping.
//!
//! **The debounce is coalescing, not restarting.** The window opens at the FIRST
//! event of a burst and is never extended by later ones — only the *reading* the
//! rebuild is issued against is refreshed. A restarting debounce would let a
//! device that chatters every 400 ms starve the rebuild forever, which is the
//! same lost meeting by a slower route.

use wilson_voice_lib::input_format::{
    selectors, FormatChangeAction, FormatEventSource, InputFormat, InputFormatWatch,
    OutputObservation, RebuildReason, AGGREGATE_REBUILD_DEBOUNCE_MS, DEFAULT_TARGET_RATE,
};

const BUILT_IN: &str = "BuiltInSpeakerDevice";
const AIRPODS: &str = "AirPodsPro-1a2b";

fn speakers() -> InputFormat {
    InputFormat::new(48_000, 2)
}

/// A tap session's watch: mic side on the built-in mic, output side pointed at
/// the device the aggregate was built around.
fn tapping() -> InputFormatWatch {
    let mut watch = InputFormatWatch::new(
        "MacBook Pro Microphone",
        InputFormat::new(48_000, 1),
        DEFAULT_TARGET_RATE,
    );
    watch.watch_output(BUILT_IN, speakers());
    watch
}

fn ev(uid: &str, format: InputFormat, at_ms: u64, selector: u32) -> OutputObservation {
    OutputObservation {
        device_uid: uid.to_string(),
        format,
        host_time: 7_310_000_000_000 + at_ms * 1_000_000,
        output_sample_index: at_ms * 16,
        source: FormatEventSource::from_selector(selector),
        at_ms,
    }
}

#[test]
fn the_debounce_window_is_at_least_the_500ms_os3_requires() {
    // OS-3 says "≥500 ms". Proved by behaviour rather than by reading the
    // constant back at itself: a burst that is 1 ms short of half a second has
    // not settled, whatever the constant happens to say. The rest of this file
    // then drives the window through `AGGREGATE_REBUILD_DEBOUNCE_MS` so a
    // deliberate widening does not have to be re-typed in eight places — the
    // `meeting_matrix` lesson about a threshold re-declared in two places
    // drifting in one of them.
    let mut watch = tapping();
    watch.observe_output(ev(AIRPODS, speakers(), 0, selectors::DEFAULT_OUTPUT_DEVICE));
    assert_eq!(
        watch.tick_output(499),
        FormatChangeAction::Ignored(InputFormatWatch::COALESCING),
        "499 ms is inside any window that honours OS-3's ≥500 ms floor"
    );
    assert!(
        watch
            .tick_output(AGGREGATE_REBUILD_DEBOUNCE_MS)
            .is_rebuild_aggregate(),
        "…and the window's own edge closes it"
    );
}

#[test]
fn a_six_event_airpods_burst_inside_the_window_is_exactly_one_rebuild() {
    // The acceptance criterion, literally: six selector events inside 500 ms,
    // one `RebuildAggregate`, not six.
    let mut watch = tapping();

    // A real connect burst: the default output moves, the device list churns as
    // the Bluetooth device registers, the rate is announced, and the whole thing
    // is re-announced as the link settles. All inside ~400 ms.
    let burst = [
        (0_u64, AIRPODS, selectors::DEFAULT_OUTPUT_DEVICE),
        (35, AIRPODS, selectors::DEVICES),
        (70, AIRPODS, selectors::NOMINAL_SAMPLE_RATE),
        (180, AIRPODS, selectors::DEVICES),
        (310, AIRPODS, selectors::DEFAULT_OUTPUT_DEVICE),
        (420, AIRPODS, selectors::DEVICES),
    ];
    // Every action the whole sequence produces, so "exactly one" is counted
    // rather than asserted a step at a time.
    let mut actions: Vec<(u64, FormatChangeAction)> = burst
        .into_iter()
        .map(|(at_ms, uid, selector)| {
            (
                at_ms,
                watch.observe_output(ev(uid, speakers(), at_ms, selector)),
            )
        })
        .collect();
    assert_eq!(actions.len(), 6, "six events went in");
    assert!(
        watch.has_pending_rebuild(),
        "the burst is still open — the rebuild is owed, not forgotten"
    );

    // The last event of a burst is by definition not followed by another, so
    // the watchdog's tick is what closes the window.
    actions.push((
        AGGREGATE_REBUILD_DEBOUNCE_MS,
        watch.tick_output(AGGREGATE_REBUILD_DEBOUNCE_MS),
    ));

    let rebuilds: Vec<u64> = actions
        .iter()
        .filter(|(_, action)| action.is_rebuild_aggregate())
        .map(|(at_ms, _)| *at_ms)
        .collect();
    assert_eq!(
        rebuilds,
        vec![AGGREGATE_REBUILD_DEBOUNCE_MS],
        "six events must produce exactly one rebuild, issued when the window \
         closed and not before — got rebuilds at {rebuilds:?}"
    );
    for (at_ms, action) in actions.iter().take(6) {
        assert_eq!(
            *action,
            FormatChangeAction::Ignored(InputFormatWatch::COALESCING),
            "event at {at_ms} ms must be coalesced, not acted on"
        );
    }
    let settled = &actions[6].1;
    assert!(
        settled.is_rebuild_aggregate(),
        "the window is inclusive at exactly {AGGREGATE_REBUILD_DEBOUNCE_MS} ms, got {settled:?}"
    );
    assert_eq!(watch.rebuilds_issued(), 1);
    assert_eq!(watch.output_device_uid(), AIRPODS);
    assert_eq!(
        settled.rebuild_reason(),
        Some(RebuildReason::DefaultOutputDeviceChanged)
    );
    // The rebuild is issued against the SETTLED reading, not the first event of
    // the burst: the marker's host time is the last one seen.
    let marker = settled.marker().expect("a rebuild carries a marker");
    assert_eq!(marker.host_time, ev(AIRPODS, speakers(), 420, 0).host_time);

    // And the burst does not fire twice: the watchdog keeps ticking.
    watch.finish_aggregate_work(AIRPODS, speakers());
    for now in [600, 1_200, 60_000] {
        assert_eq!(
            watch.tick_output(now),
            FormatChangeAction::Unchanged,
            "nothing is pending at {now} ms"
        );
    }
    assert_eq!(watch.rebuilds_issued(), 1);
}

#[test]
fn the_window_opens_at_the_first_event_and_is_never_restarted_by_later_ones() {
    // A restarting debounce is the tempting implementation and the wrong one: a
    // device that re-announces itself every 400 ms would hold the window open
    // forever and the aggregate would stay pointed at a device nobody is
    // listening to — a silent tap track for the rest of the meeting, which is
    // the exact class of failure OS-4 spends its length on.
    let mut watch = tapping();
    let mut rebuilds = 0;
    for at_ms in (0..2_000).step_by(400) {
        if watch
            .observe_output(ev(AIRPODS, speakers(), at_ms, selectors::DEVICES))
            .is_rebuild_aggregate()
        {
            rebuilds += 1;
            watch.finish_aggregate_work(AIRPODS, speakers());
        }
    }
    assert_eq!(
        rebuilds, 1,
        "a chattering device must still get its one rebuild, not starve"
    );
}

#[test]
fn a_burst_that_settles_back_where_it_started_is_no_rebuild_at_all() {
    // AirPods that connect and drop inside one window (a flaky pairing, or a
    // case lid opened and shut) leave the aggregate pointed at exactly the
    // device it was already pointed at. Rebuilding for that is a self-inflicted
    // ~1 s hole in the tap track for no reason.
    let mut watch = tapping();
    assert_eq!(
        watch.observe_output(ev(AIRPODS, speakers(), 0, selectors::DEFAULT_OUTPUT_DEVICE)),
        FormatChangeAction::Ignored(InputFormatWatch::COALESCING)
    );
    assert_eq!(
        watch.observe_output(ev(BUILT_IN, speakers(), 120, selectors::DEVICES)),
        FormatChangeAction::Ignored(InputFormatWatch::COALESCING)
    );
    assert_eq!(
        watch.tick_output(700),
        FormatChangeAction::Unchanged,
        "the settled state is the state we are already in"
    );
    assert_eq!(watch.rebuilds_issued(), 0);
    assert_eq!(watch.output_segment_index(), 0);
    assert!(!watch.is_aggregate_work_in_flight());
}

#[test]
fn an_event_arriving_after_the_window_flushes_the_burst_without_waiting_for_a_tick() {
    // The watchdog tick is not the only thing that can close a window — a later
    // event does too. That matters because the tap's tick interval is measured
    // in tens of seconds, and OS-4's all-zero holes start the moment the
    // aggregate is pointed at the wrong device.
    let mut watch = tapping();
    assert!(!watch
        .observe_output(ev(AIRPODS, speakers(), 0, selectors::DEFAULT_OUTPUT_DEVICE))
        .is_rebuild_aggregate());
    let action = watch.observe_output(ev(AIRPODS, speakers(), 512, selectors::DEVICES));
    assert!(
        action.is_rebuild_aggregate(),
        "an event past the window flushes it, got {action:?}"
    );
    assert_eq!(watch.rebuilds_issued(), 1);
}

#[test]
fn a_sample_rate_renegotiation_on_the_same_device_is_still_a_rebuild() {
    // OS-4's named trigger for the all-zero holes: "sample-rate renegotiation on
    // the output device (44.1 kHz ↔ 48 kHz) when another app changes output" —
    // and, critically, "Bluetooth device state changes (AirPods sleep/wake)
    // where UID stays the same". A UID-only comparison sees nothing there, so
    // the format is compared too.
    let mut watch = tapping();
    watch.observe_output(ev(
        BUILT_IN,
        InputFormat::new(44_100, 2),
        0,
        selectors::NOMINAL_SAMPLE_RATE,
    ));
    let action = watch.tick_output(600);
    assert_eq!(
        action.rebuild_reason(),
        Some(RebuildReason::OutputFormatRenegotiated),
        "same UID, new rate, got {action:?}"
    );
    let marker = action.marker().expect("a rebuild carries a marker");
    assert_eq!(
        marker.from_device, marker.to_device,
        "the device did not move"
    );
    assert_eq!(marker.from.sample_rate_hz, 48_000);
    assert_eq!(marker.to.sample_rate_hz, 44_100);
}

#[test]
fn an_unreadable_format_on_the_current_device_is_never_a_rebuild() {
    // CoreAudio reporting a zero rate means "I don't know yet", which is the
    // normal state mid-Bluetooth-handshake. Rebuilding on "I don't know" is how
    // the loop starts: the rebuild churns the device list, the next read is
    // still mid-handshake, and around it goes.
    let mut watch = tapping();
    for bad in [InputFormat::new(0, 2), InputFormat::new(48_000, 0)] {
        assert_eq!(
            watch.observe_output(ev(BUILT_IN, bad, 0, selectors::NOMINAL_SAMPLE_RATE)),
            FormatChangeAction::Unchanged
        );
    }
    assert_eq!(watch.tick_output(10_000), FormatChangeAction::Unchanged);
    assert_eq!(watch.rebuilds_issued(), 0);
    assert_eq!(
        watch.output_format(),
        speakers(),
        "and the known-good format is kept"
    );
}

#[test]
fn the_output_half_is_inert_until_a_tap_session_points_it_somewhere() {
    // 22-A's mic-only path shares this type. It never calls `watch_output`, so
    // an output notification that reaches it must be a no-op rather than a
    // rebuild of an aggregate device that does not exist.
    let mut watch = InputFormatWatch::new("Mic", InputFormat::new(48_000, 1), DEFAULT_TARGET_RATE);
    assert_eq!(
        watch.observe_output(ev(AIRPODS, speakers(), 0, selectors::DEFAULT_OUTPUT_DEVICE)),
        FormatChangeAction::Ignored(InputFormatWatch::NO_OUTPUT_WATCHED)
    );
    assert_eq!(watch.tick_output(10_000), FormatChangeAction::Unchanged);
    assert_eq!(watch.rebuilds_issued(), 0);
}
