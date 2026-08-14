//! YV104 / plan finding OS-4 — **the first answer to tap silence is a rebuild,
//! never a permission verdict.**
//!
//! The bug this file exists to make impossible is not in CoreAudio; it is in the
//! plan. §2.1's original denial inference reads *"digital silence for N seconds
//! while a target is producing audio ⇒ permission looks denied"*, and Apple
//! Developer Forums thread 825780 describes a tap that delivers all-zero buffers
//! for **sixteen minutes at a stretch** while system audio is plainly audible.
//! Those two sentences describe the same observation. A watchdog wired the
//! original way badges a perfectly healthy meeting "permission revoked",
//! misapplies matrix row #2, and stops trying — and the sixteen-minute hole in
//! the middle of the lecture is still there afterwards.
//!
//! So the property under test is not "the watchdog eventually rebuilds". It is
//! the stronger, order-of-operations one: **there is no input, and no amount of
//! elapsed silence, that makes the FIRST action anything other than a full
//! rebuild** — including the input a naive heuristic finds most incriminating
//! (system output demonstrably active, the tap dead for an hour, and no non-zero
//! sample ever seen).
//!
//! Zero audio hardware. The state machine is a pure function of two clocks and
//! four booleans, which is the entire reason it was built as one.

use std::time::Duration;

use wilson_voice_lib::syscapture::{
    full_rebuild_sequence, ghost_tick, GhostState, SilenceCause, TapEnvironment, TapLiveness,
    TapSilenceKind, TapStep, TapWatchdogAction, TapWatchdogInputs, MAX_TAP_REBUILDS_PER_MEETING,
    TAP_SILENCE_REBUILD_AFTER,
};

/// A tap that has been producing real audio and has just gone to zeros.
fn ghost_after(silent: Duration) -> TapLiveness {
    TapLiveness {
        ever_nonzero: true,
        since_nonzero: silent,
        // The IOProc is still firing on cadence. That is what makes this bug
        // invisible to every liveness check that is not this one.
        since_last_block: Duration::from_millis(21),
        frames_delivered: 3_600 * 48_000,
        lost_frames: 0,
    }
}

/// A tap that has never produced a non-zero sample — what a TCC denial looks
/// like, and the input the original heuristic was most confident about.
fn never_delivered(silent: Duration) -> TapLiveness {
    TapLiveness {
        ever_nonzero: false,
        since_nonzero: silent,
        since_last_block: Duration::from_millis(21),
        frames_delivered: silent.as_secs() * 48_000,
        lost_frames: 0,
    }
}

fn tick(liveness: TapLiveness, env: TapEnvironment, state: GhostState) -> TapWatchdogAction {
    ghost_tick(&TapWatchdogInputs {
        elapsed: Duration::from_secs(3_600),
        liveness,
        env,
        state,
    })
}

/// The headline. Ten silence durations from the threshold to well past OS-4's
/// worst reported hole, against a fresh watchdog: every one is a rebuild.
#[test]
fn the_first_action_is_always_a_full_rebuild_however_long_the_silence_has_run() {
    let durations = [
        TAP_SILENCE_REBUILD_AFTER,
        Duration::from_secs(53),  // the shortest hole thread 825780 reports
        Duration::from_secs(60),  // …and the next
        Duration::from_secs(141), // …and the next
        Duration::from_secs(188), // 3 min 8 s
        Duration::from_secs(963), // 16 min 3 s — the worst one reported
        Duration::from_secs(1_800),
        Duration::from_secs(3_599),
        Duration::from_secs(6 * 3_600),
        Duration::from_secs(24 * 3_600),
    ];
    for silent in durations {
        let action = tick(
            ghost_after(silent),
            TapEnvironment::default(),
            GhostState::default(),
        );
        assert_eq!(
            action,
            TapWatchdogAction::RebuildFull {
                attempt: 1,
                kind: TapSilenceKind::AllZeroBuffers,
                causes: Default::default(),
            },
            "silence of {silent:?} must still answer with a rebuild, not a verdict"
        );
        assert_eq!(
            action.verdict(),
            None,
            "a rebuild can never carry a permission verdict — {silent:?}"
        );
    }
}

/// The same, for the input the plan's heuristic was written about: the tap is
/// silent AND the machine is demonstrably producing output. That conjunction is
/// exactly what §2.1 proposed to read as "permission looks denied". It is a
/// rebuild.
#[test]
fn silence_while_the_system_is_audibly_playing_is_still_a_rebuild_not_a_denial() {
    let env = TapEnvironment {
        system_output_active: Some(true),
        ..TapEnvironment::default()
    };
    let action = tick(
        ghost_after(Duration::from_secs(963)),
        env,
        GhostState::default(),
    );
    assert!(action.is_rebuild(), "got {action:?}");
    assert_eq!(action.verdict(), None);
}

/// And for the strongest possible denial signal — never a non-zero sample, ever.
/// The discriminator says "this looks like a denial"; the FIRST action is still
/// a rebuild, because OS-4's remedy is cheap, bounded and reversible, and being
/// wrong about a permission badge is not.
#[test]
fn even_a_tap_that_never_delivered_anything_gets_its_rebuilds_before_any_verdict() {
    let mut state = GhostState::default();
    for attempt in 1..=MAX_TAP_REBUILDS_PER_MEETING {
        let action = tick(
            never_delivered(Duration::from_secs(120 * attempt as u64)),
            TapEnvironment::default(),
            state,
        );
        assert!(
            action.is_rebuild(),
            "attempt {attempt} must be a rebuild, got {action:?}"
        );
        assert_eq!(action.verdict(), None);
        state.rebuilds_issued = attempt;
        state.grace_until = None;
        state.rebuild_issued_at = None;
    }
}

/// A tap that is delivering is never touched, however long the meeting runs.
/// The complement of the headline, and the thing that keeps the rebuild from
/// being free: three teardowns of a working tap is three real gaps in Track B.
#[test]
fn a_tap_that_is_delivering_audio_is_never_rebuilt() {
    for elapsed_s in [1u64, 60, 3_600, 3 * 3_600] {
        let live = TapLiveness {
            ever_nonzero: true,
            since_nonzero: TAP_SILENCE_REBUILD_AFTER - Duration::from_millis(1),
            since_last_block: Duration::from_millis(21),
            frames_delivered: elapsed_s * 48_000,
            lost_frames: 0,
        };
        let action = ghost_tick(&TapWatchdogInputs {
            elapsed: Duration::from_secs(elapsed_s),
            liveness: live,
            env: TapEnvironment::default(),
            state: GhostState::default(),
        });
        assert_eq!(
            action,
            TapWatchdogAction::Continue,
            "one millisecond short of the threshold is not silence"
        );
    }
}

/// An IOProc that has stopped firing altogether is the *other* silence, and it
/// gets the same remedy with a different name in the log — the tap and the
/// aggregate are both suspect either way, and OS-4 is explicit that a partial
/// recovery does not work.
#[test]
fn a_dead_ioproc_rebuilds_too_and_says_so() {
    let live = TapLiveness {
        ever_nonzero: true,
        since_nonzero: Duration::from_secs(120),
        since_last_block: Duration::from_secs(120),
        frames_delivered: 48_000 * 600,
        lost_frames: 0,
    };
    let action = tick(live, TapEnvironment::default(), GhostState::default());
    let TapWatchdogAction::RebuildFull { kind, causes, .. } = action else {
        panic!("expected a rebuild, got {action:?}");
    };
    assert_eq!(kind, TapSilenceKind::IoProcSilent);
    assert!(causes.contains(SilenceCause::IoProcNotFiring));
}

/// Rule (d): the innocent explanations ride along with the rebuild instead of
/// being silently ruled out. This is what keeps the log line an enumeration
/// rather than an accusation.
#[test]
fn the_other_silence_causes_are_carried_on_the_rebuild() {
    let env = TapEnvironment {
        aggregate_matches_output_device: false,
        system_output_active: Some(false),
        exclusive_flag_mutated_post_init: true,
        dispatch_queue_installed: false,
    };
    let action = tick(
        ghost_after(Duration::from_secs(90)),
        env,
        GhostState::default(),
    );
    let TapWatchdogAction::RebuildFull { causes, .. } = action else {
        panic!("expected a rebuild, got {action:?}");
    };
    assert!(causes.contains(SilenceCause::OutputRoutedElsewhere));
    assert!(causes.contains(SilenceCause::EveryoneMuted));
    assert!(causes.contains(SilenceCause::ExclusiveFlagInverted));
    assert!(causes.contains(SilenceCause::NilDispatchQueue));
    assert_eq!(causes.len(), 4);
    assert_eq!(
        causes.as_log_string(),
        "output_routed_elsewhere|everyone_muted|exclusive_flag_inverted|nil_dispatch_queue"
    );
}

/// The rebuild the action names is the SEVEN-step one, in OS-4's order, and it
/// is declared once — the backlog's rule for this item is that it calls that
/// sequence rather than duplicating it. `syscapture.rs` holding the only copy is
/// what makes "calls it" enforceable when YV100's FFI lands on top.
#[test]
fn the_rebuild_is_the_full_seven_step_sequence_in_apples_order() {
    assert_eq!(
        full_rebuild_sequence(),
        [
            TapStep::AudioDeviceStop,
            TapStep::AudioDeviceDestroyIOProcID,
            TapStep::AudioHardwareDestroyAggregateDevice,
            TapStep::AudioHardwareDestroyProcessTap,
            TapStep::AudioHardwareCreateProcessTap,
            TapStep::AudioHardwareCreateAggregateDevice,
            TapStep::CreateAndStartIOProc,
        ],
        "thread 825780 is explicit: restarting the IOProc alone or recreating only the \
         aggregate device is not reliable — both the tap and the aggregate must go"
    );
    // Every destroy precedes every create. Destroying the tap while the
    // aggregate still references it is the ordering bug the list prevents.
    let steps = full_rebuild_sequence();
    let last_teardown = steps.iter().rposition(|s| s.is_teardown()).unwrap();
    let first_create = steps.iter().position(|s| !s.is_teardown()).unwrap();
    assert!(last_teardown < first_create);
}

/// Type-level, and the reason it is worth a test: matrix row #2 says a lost
/// system-audio track NEVER ends the meeting, because Track A is still
/// recording the person holding the Mac. `TapWatchdogAction` has three variants
/// and none of them is a stop, so no future caller can wire one by accident.
#[test]
fn no_tap_outcome_can_ever_stop_a_meeting() {
    let outcomes = [
        tick(
            ghost_after(Duration::from_millis(0)),
            TapEnvironment::default(),
            GhostState::default(),
        ),
        tick(
            ghost_after(Duration::from_secs(963)),
            TapEnvironment::default(),
            GhostState::default(),
        ),
        tick(
            never_delivered(Duration::from_secs(963)),
            TapEnvironment::default(),
            GhostState {
                rebuilds_issued: MAX_TAP_REBUILDS_PER_MEETING,
                ..GhostState::default()
            },
        ),
    ];
    for action in outcomes {
        match action {
            TapWatchdogAction::Continue
            | TapWatchdogAction::RebuildFull { .. }
            | TapWatchdogAction::DegradeTrackLost { .. } => {}
        }
    }
}
