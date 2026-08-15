//! Matrix row 14 — **output device changes mid-meeting (AirPods connect)**.
//!
//! Required behaviour (plan §6): the aggregate device references the *old*
//! output UID, so listen for default-output changes, tear down and rebuild the
//! aggregate, splice the spill, log a `device_change` marker. The plan is
//! emphatic that this happens constantly in real use and is not an edge case.
//!
//! YV103 merged the decision as the output half of the **same**
//! `InputFormatWatch` the mic path already uses — one guarded, debounced state
//! machine rather than two — and this file drives it as the row's evidence.
//!
//! Published as `PolicyOnly` with **#123 (YV100)** owning the wiring, because
//! the producer is missing rather than the decision: nothing in `src/` calls
//! `InputFormatWatch::watch_output`, so `output_device_uid` is empty on every
//! watch the app builds and `observe_output` short-circuits on
//! `NO_OUTPUT_WATCHED` before it can compare a device to anything. `record.rs`
//! does handle a `RebuildAggregate` action — by logging that one arrived on the
//! mic path, where it means nothing — and a handler for an action nobody emits
//! is not the behaviour this row publishes.

use wilson_voice_lib::input_format::{
    selectors, FormatChangeAction, FormatEventSource, InputFormat, InputFormatWatch,
    OutputObservation, RebuildReason, AGGREGATE_REBUILD_DEBOUNCE_MS, DEFAULT_TARGET_RATE,
};
use wilson_voice_lib::meeting_matrix::{Coverage, ROWS};

#[path = "support/callsite.rs"]
mod callsite;

const SPEAKERS: &str = "BuiltInSpeakerDevice";
const AIRPODS: &str = "AirPodsPro-1a2b";

fn format() -> InputFormat {
    InputFormat::new(48_000, 2)
}

/// A watch built the way a tap session builds one: mic side on the built-in
/// mic, output side armed at the device the aggregate was created around.
fn armed() -> InputFormatWatch {
    let mut watch = unarmed();
    watch.watch_output(SPEAKERS, format());
    watch
}

/// The only kind of watch the shipping app builds today — the output half never
/// turned on.
fn unarmed() -> InputFormatWatch {
    InputFormatWatch::new(
        "MacBook Pro Microphone",
        InputFormat::new(48_000, 1),
        DEFAULT_TARGET_RATE,
    )
}

fn observed(uid: &str, at_ms: u64) -> OutputObservation {
    OutputObservation {
        device_uid: uid.to_string(),
        format: format(),
        host_time: 7_310_000_000_000 + at_ms * 1_000_000,
        output_sample_index: at_ms * 48,
        source: FormatEventSource::from_selector(selectors::DEFAULT_OUTPUT_DEVICE),
        at_ms,
    }
}

/// **The row.** AirPods connect mid-meeting; once the burst settles, the watch
/// asks for the aggregate to be rebuilt around the new device.
#[test]
fn an_output_device_change_asks_for_the_aggregate_to_be_rebuilt() {
    let mut watch = armed();

    let first = watch.observe_output(observed(AIRPODS, 1_000));
    assert!(
        !first.is_rebuild_aggregate(),
        "the burst has not settled yet — one AirPods connect is four or five notifications, and \
         rebuilding on the first is how you rebuild five times: {first:?}"
    );

    let settled = watch.tick_output(1_000 + AGGREGATE_REBUILD_DEBOUNCE_MS);
    let FormatChangeAction::RebuildAggregate { marker, reason } = settled else {
        panic!("the output device moved and the aggregate was never rebuilt: {settled:?}");
    };
    assert_eq!(marker.from_device, SPEAKERS);
    assert_eq!(marker.to_device, AIRPODS);
    assert_eq!(reason, RebuildReason::DefaultOutputDeviceChanged);
    assert_eq!(
        watch.output_device_uid(),
        AIRPODS,
        "the watch adopts the device it just asked to be rebuilt around"
    );
    assert_eq!(watch.rebuilds_issued(), 1);
}

/// The plan's "do not treat it as an edge case", as a property: a burst of
/// notifications from ONE connect is one rebuild, not one per selector.
#[test]
fn one_connect_is_one_rebuild_however_many_notifications_it_emits() {
    let mut watch = armed();
    // `kAudioHardwarePropertyDefaultOutputDevice`, `…Devices`, and the stream
    // format all fire within a few milliseconds of each other.
    for at in [1_000, 1_005, 1_040, 1_120] {
        assert!(!watch
            .observe_output(observed(AIRPODS, at))
            .is_rebuild_aggregate());
    }
    let settled = watch.tick_output(1_120 + AGGREGATE_REBUILD_DEBOUNCE_MS);
    assert!(settled.is_rebuild_aggregate(), "{settled:?}");
    assert_eq!(watch.rebuilds_issued(), 1);

    // …and the storm the rebuild itself sets off is not a second connect.
    let quiet = watch.tick_output(5_000);
    assert!(!quiet.is_rebuild_aggregate(), "{quiet:?}");
    assert_eq!(watch.rebuilds_issued(), 1);
}

/// A device that connects and drops inside the same window is correctly no
/// rebuild at all — the window fires on the LATEST reading, not the first.
#[test]
fn a_device_that_comes_and_goes_inside_the_window_costs_nothing() {
    let mut watch = armed();
    watch.observe_output(observed(AIRPODS, 1_000));
    watch.observe_output(observed(SPEAKERS, 1_200));
    let settled = watch.tick_output(1_200 + AGGREGATE_REBUILD_DEBOUNCE_MS);
    assert!(
        !settled.is_rebuild_aggregate(),
        "the output ended up where it started; tearing the aggregate down for that is a gap in \
         Track B bought for nothing: {settled:?}"
    );
    assert_eq!(watch.rebuilds_issued(), 0);
}

/// The guard, which is the difference between this row and the documented
/// infinite loop it is modelled on: while our own create/destroy is in flight,
/// every notification it generates is discarded.
#[test]
fn the_rebuilds_own_notifications_cannot_start_another_rebuild() {
    let mut watch = armed();
    assert!(watch.begin_aggregate_work());
    assert!(
        !watch.begin_aggregate_work(),
        "a rebuild asked for while one is running must be refused, not run twice"
    );

    // Discarded, and discarded *for the guard's reason* — not merely swallowed
    // by the debounce window, which would look identical from the outside on
    // the tick they arrive and start a rebuild 500 ms later. The reason string
    // is the observable difference between "ignored because it was ours" and
    // "queued because the burst has not settled", so the assertion names it.
    for at in [2_000, 2_050, 2_100] {
        assert_eq!(
            watch.observe_output(observed(AIRPODS, at)),
            FormatChangeAction::Ignored(InputFormatWatch::GUARDED),
            "a notification emitted BY our own create/destroy must be discarded as ours"
        );
        assert!(
            !watch.has_pending_rebuild(),
            "…and must not be queued either: a queued event fires a rebuild the moment the \
             window closes, which is the loop by a 500 ms detour"
        );
    }
    // The watchdog's own settle tick is guarded too. Without this half, the
    // burst is merely deferred rather than discarded.
    assert_eq!(
        watch.tick_output(2_100 + AGGREGATE_REBUILD_DEBOUNCE_MS),
        FormatChangeAction::Ignored(InputFormatWatch::GUARDED)
    );

    watch.finish_aggregate_work(AIRPODS, format());
    assert!(!watch.is_aggregate_work_in_flight());
    assert!(
        !watch
            .tick_output(9_000 + AGGREGATE_REBUILD_DEBOUNCE_MS)
            .is_rebuild_aggregate(),
        "and nothing survives the window to fire after the guard is released"
    );
    assert_eq!(
        watch.rebuilds_issued(),
        0,
        "nothing the rebuild itself emitted may be counted as a device change"
    );
}

/// **The absence half.** The watch's output side is inert in the shipping app:
/// nothing arms it. `input_format.rs` defines `watch_output` and `record.rs`
/// names it in a comment saying the mic path never calls it — a comment naming
/// a call site is evidence the wiring is absent, which is why the scan ignores
/// both.
#[test]
fn nothing_arms_the_output_watch_yet_so_row_14_is_not_wired() {
    let found = callsite::call_sites("watch_output", &["input_format.rs"]);
    assert!(
        found.is_empty(),
        "{}",
        callsite::promote_the_row("14", "watch_output", &found)
    );

    // And the consequence, asserted rather than assumed: an unarmed watch — the
    // only kind the app builds today — cannot reach the rebuild path at all.
    let mut watch = unarmed();
    let action = watch.observe_output(observed(AIRPODS, 1_000));
    assert_eq!(
        action,
        FormatChangeAction::Ignored(InputFormatWatch::NO_OUTPUT_WATCHED),
        "an unarmed watch must ignore output readings rather than rebuild an aggregate that does \
         not exist"
    );
    assert!(!watch
        .tick_output(1_000 + AGGREGATE_REBUILD_DEBOUNCE_MS)
        .is_rebuild_aggregate());
}

#[test]
fn the_published_cell_names_the_owner_of_the_missing_wiring() {
    let row = ROWS.iter().find(|r| r.id == "14").expect("row 14");
    assert_eq!(
        row.coverage,
        Coverage::PolicyOnly {
            test: "matrix_row14_output_device_change.rs",
            wiring_pr: Some("#123 (YV100)"),
            absent_call_site: "watch_output",
        }
    );
    let cell = row.coverage.cell();
    assert!(cell.contains("#123 (YV100)"), "{cell}");
    assert!(cell.contains("watch_output"), "{cell}");
}
