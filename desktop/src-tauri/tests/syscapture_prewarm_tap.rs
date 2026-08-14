//! YV102 acceptance — **the pre-warm tears down cleanly, whatever it heard.**
//!
//! The item's own acceptance line: *"asserts the pre-warm function tears down
//! cleanly (all four teardown calls fire, same order as YV100's
//! `syscapture_teardown_order`) regardless of whether the tap ever delivered a
//! sample."*
//!
//! Why that clause matters more here than anywhere else in the tap code. The
//! pre-warm is the ONE tap Yap opens that it fully expects to hear nothing
//! from: it runs for 200 ms, from a Settings screen, on a Mac that is probably
//! silent, and its entire purpose is the system alert it provokes on the way in
//! (OS-10 — there is no permission-request API; creating and starting a tap
//! *is* the request). A teardown path that quietly depended on "we got audio,
//! so the IOProc is live" would leak a process tap and an aggregate device
//! every time a user opened Settings and did nothing — and those objects belong
//! to `coreaudiod`, not to this address space, so the leak survives Yap and
//! holds the user's output device inside a private aggregate until they log
//! out.
//!
//! It is also the surface that exercises YV100's ordering contract most often —
//! a Settings button, pressable any number of times, versus a real meeting. So
//! it calls [`open_tap`] and [`teardown`] rather than re-deriving either: the
//! assertions below are against the SAME state machine
//! `tests/syscapture_teardown_order.rs` locks down.
//!
//! Zero audio hardware, zero macOS 14.4, zero TCC grant — all of it rides
//! YV100's [`TapPlatform`] seam.

use std::sync::atomic::{AtomicBool, Ordering};

use wilson_voice_lib::syscapture::{
    prewarm_tap, TapError, TapStage, TeardownStep, PREWARM_DWELL, TEARDOWN_ORDER,
};

#[path = "support/tap.rs"]
mod tap;
use tap::{Call, FakePlatform};

const SELF_OBJECT: Option<u32> = Some(4242);

fn prewarm(platform: &mut FakePlatform, dwell: impl FnOnce()) -> wilson_voice_lib::syscapture::Prewarm
{
    prewarm_tap(platform, SELF_OBJECT, "yap.prewarm.test", "Yap pre-warm", dwell)
}

/// The headline: a tap that heard NOTHING is torn down exactly as completely as
/// one that heard everything, and in the same order.
#[test]
fn the_prewarm_tears_down_completely_whether_or_not_it_heard_a_sample() {
    for (label, heard_a_sample) in [("silent", false), ("audible", true)] {
        let mut platform = FakePlatform::default();
        let observed = AtomicBool::new(false);
        let run = prewarm(&mut platform, || {
            // The dwell is where the 200 ms of (discarded) audio would arrive.
            // Whether it did is the ONLY thing that differs between these two
            // runs, and it must change nothing about the teardown.
            observed.store(heard_a_sample, Ordering::Relaxed);
        });

        assert!(run.opened, "{label}: the tap opened");
        assert_eq!(run.error, None, "{label}");
        assert_eq!(
            run.teardown_steps,
            TEARDOWN_ORDER.to_vec(),
            "{label}: all four teardown calls, in the canonical order"
        );
        assert!(run.tore_down_completely(), "{label}");
        assert_eq!(
            platform.teardown_calls(),
            vec![
                Call::Stop,
                Call::DestroyIoProc,
                Call::DestroyAggregate,
                Call::DestroyTap
            ],
            "{label}: the platform saw the same four calls in the same order"
        );
    }
}

/// The order is not merely "all four" — it is `AudioDeviceStop` →
/// `AudioDeviceDestroyIOProcID` → `AudioHardwareDestroyAggregateDevice` →
/// `AudioHardwareDestroyProcessTap`, the sequence YV104's 7-step rebuild is the
/// first half of. Asserted against the constant, so a reordering of
/// `TEARDOWN_ORDER` cannot make this test agree with itself.
#[test]
fn the_teardown_order_is_yv100s_canonical_order() {
    let mut platform = FakePlatform::default();
    let run = prewarm(&mut platform, || {});
    assert_eq!(
        run.teardown_steps,
        vec![
            TeardownStep::AudioDeviceStop,
            TeardownStep::AudioDeviceDestroyIOProcID,
            TeardownStep::AudioHardwareDestroyAggregateDevice,
            TeardownStep::AudioHardwareDestroyProcessTap,
        ]
    );
    assert_eq!(run.teardown_steps, TEARDOWN_ORDER.to_vec());
}

/// A panic inside the dwell must not leak the tap.
///
/// This is not hypothetical: the dwell is the only place caller code runs while
/// a real process tap is open, and a `SystemTap` leaked by an unwinding Settings
/// button is indistinguishable — from the user's side — from Yap having taken
/// their speakers away.
#[test]
fn a_panic_during_the_dwell_still_tears_the_tap_down() {
    let mut platform = FakePlatform::default();
    let run = prewarm(&mut platform, || panic!("injected panic mid-dwell"));

    assert!(run.opened);
    assert_eq!(
        run.teardown_steps,
        TEARDOWN_ORDER.to_vec(),
        "the unwind was caught and the four teardown calls still fired"
    );
}

/// The failure path. `prewarm_tap` reports no steps of its own — `open_tap`
/// already tore down whatever existed before it returned the error — so the
/// proof has to come from the platform's call log, not from the return value.
/// Every setup step is walked, because "we tear down on failure" is only true
/// if it is true at each of them.
#[test]
fn a_failure_at_any_setup_step_leaves_nothing_behind() {
    let expected: &[(&str, TapStage, &[Call])] = &[
        ("create_tap", TapStage::CreateTap, &[]),
        ("tap_uid", TapStage::ReadTapUid, &[Call::DestroyTap]),
        ("tap_format", TapStage::ReadTapFormat, &[Call::DestroyTap]),
        (
            "default_output_uid",
            TapStage::ResolveDefaultOutput,
            &[Call::DestroyTap],
        ),
        (
            "create_aggregate",
            TapStage::CreateAggregate,
            &[Call::DestroyTap],
        ),
        (
            "create_ioproc",
            TapStage::CreateIoProc,
            &[Call::DestroyAggregate, Call::DestroyTap],
        ),
        (
            "start",
            TapStage::Start,
            &[
                Call::DestroyIoProc,
                Call::DestroyAggregate,
                Call::DestroyTap,
            ],
        ),
    ];

    for (step, stage, teardown) in expected {
        let mut platform = FakePlatform::failing_with_status(step);
        let dwelled = AtomicBool::new(false);
        let run = prewarm(&mut platform, || dwelled.store(true, Ordering::Relaxed));

        assert!(!run.opened, "{step}");
        assert_eq!(
            run.error,
            Some(TapError::Os {
                stage: *stage,
                status: -4
            }),
            "{step}: the error names the step that failed"
        );
        assert!(
            run.teardown_steps.is_empty(),
            "{step}: open_tap owns the failure-path teardown, not prewarm_tap"
        );
        assert!(
            !dwelled.load(Ordering::Relaxed),
            "{step}: there is nothing to dwell on when the tap never opened"
        );
        assert_eq!(
            platform.teardown_calls(),
            teardown.to_vec(),
            "{step}: exactly the resources that existed were destroyed, in order"
        );
    }
}

/// Repeating the step — a user pressing "Set up meeting recording" four times —
/// creates and destroys four taps and accumulates nothing. TCC shows its alert
/// at most once regardless; the leak, if there were one, would be per press.
#[test]
fn pressing_the_setup_button_repeatedly_accumulates_nothing() {
    for _ in 0..4 {
        let mut platform = FakePlatform::default();
        let run = prewarm(&mut platform, || {});
        assert!(run.tore_down_completely());
        assert_eq!(
            platform
                .calls
                .iter()
                .filter(|c| matches!(c, Call::CreateTap { .. }))
                .count(),
            1
        );
        assert_eq!(
            platform
                .calls
                .iter()
                .filter(|c| matches!(c, Call::DestroyTap))
                .count(),
            1,
            "one tap created, one tap destroyed"
        );
    }
}

/// The exclusion list still holds here. The pre-warm is a *global* tap for the
/// 200 ms it exists, and a global tap that does not exclude Yap records Yap —
/// including, on the very next press, itself.
#[test]
fn the_prewarm_excludes_yap_like_every_other_tap() {
    let mut platform = FakePlatform::default();
    let _ = prewarm(&mut platform, || {});
    assert_eq!(
        platform.calls.first(),
        Some(&Call::CreateTap {
            excluded: vec![4242]
        })
    );

    // And with no resolvable self object it refuses outright rather than
    // opening a tap that includes us.
    let mut platform = FakePlatform::default();
    let run = prewarm_tap(&mut platform, None, "uid", "name", || {});
    assert_eq!(run.error, Some(TapError::SelfProcessObjectUnavailable));
    assert!(platform.calls.is_empty(), "nothing was created to leak");
}

/// The dwell is 200 ms — long enough for `AudioDeviceStart` to have started and
/// the IOProc to have fired, short enough that the Settings step is a button
/// rather than a wait. It is also, deliberately, far below
/// `syscapture::DENIAL_GRACE`, which is what stops this step manufacturing a
/// denial verdict out of a quiet Mac (see
/// `tests/syscapture_never_delivered_denied_state.rs`).
#[test]
fn the_dwell_is_two_hundred_milliseconds() {
    assert_eq!(PREWARM_DWELL.as_millis(), 200);
    assert!(PREWARM_DWELL < wilson_voice_lib::syscapture::DENIAL_GRACE);
}
