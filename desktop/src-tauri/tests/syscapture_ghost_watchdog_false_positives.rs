//! YV104, review round 2 — **the two ways this watchdog fired on a healthy
//! meeting, at the interval the product actually ticks at.**
//!
//! Every test in this file ticks at [`WATCHDOG_INTERVAL`] — 60 s, the value the
//! shipping watchdog thread sleeps for — and not at a convenient one. That is
//! the whole methodology: both defects below were invisible to suites that
//! ticked every second or every 30 s, and both were the *default* path at 60 s.
//!
//! **Defect one — a quiet meeting badged "permission is off".** The tap's
//! `ever_nonzero` bit tells a TCC denial apart from OS-4's ghost, which is what
//! the plan claims for it and all it can do. It does **not** tell a denial apart
//! from a granted tap that had nothing to record: an in-person meeting, a call
//! where the far side is quiet, a lecture before the lecturer starts. The first
//! cut read the bit as if it did, so 30 s of ordinary quiet — with the module's
//! own `EveryoneMuted` cause computed and attached to the very same action —
//! produced three full tap+aggregate teardown/recreate cycles in the first four
//! minutes of a healthy meeting and then a false privacy accusation. Rule (d)
//! enumerated the innocent causes and then rebuilt anyway; here the enumeration
//! is load-bearing.
//!
//! **Defect two — a tap that is actively delivering, rebuilt and then
//! degraded.** The in-flight branch honoured its timeout without ever reading
//! liveness, and the timeout was hard-coded to 20 s while the watchdog ticks
//! every 60 s — so an in-flight rebuild was always already expired by the next
//! tick, the "wait for it" branch was unreachable in the shipping
//! configuration, and a tap delivering audio on every callback got rebuilt
//! twice more and then banner-degraded with "system audio stopped coming
//! through" while it was recording fine.
//!
//! Both are asserted here through the **pure tick** and through
//! `meeting::watchdog_tick`, the function the shipping thread actually calls,
//! so neither can come back through the wiring.

use std::time::Duration;

use wilson_voice_lib::meeting::{
    watchdog_tick, ThermalState, WatchdogAction, WatchdogInputs, DISK_FLOOR_BYTES,
    WATCHDOG_INTERVAL,
};
use wilson_voice_lib::syscapture::{
    ghost_tick, is_unexplained_silence, GhostState, GhostWatchdog, TapEnvironment, TapLiveness,
    TapRebuildOutcome, TapVerdict, TapWatchdogAction, TapWatchdogInputs,
    MAX_TAP_REBUILDS_PER_MEETING, TAP_REBUILD_IN_FLIGHT_TIMEOUT,
};

/// Twenty minutes of the shipping tick.
const MEETING_TICKS: u32 = 20;

/// A tap whose IOProc is firing on cadence and whose samples are all zero,
/// having been that way for `silent`.
fn quiet_tap(silent: Duration, ever_nonzero: bool) -> TapLiveness {
    TapLiveness {
        ever_nonzero,
        since_nonzero: silent,
        // Callbacks every ~21 ms. The tap is alive; there is simply nothing in
        // it, which is what "everyone is quiet" looks like from in here and is
        // indistinguishable from OS-4's ghost on this field alone.
        since_last_block: Duration::from_millis(21),
        frames_delivered: silent.as_secs() * 48_000,
        lost_frames: 0,
    }
}

/// A tap delivering audio on every callback.
fn delivering_tap() -> TapLiveness {
    TapLiveness {
        ever_nonzero: true,
        since_nonzero: Duration::from_millis(20),
        since_last_block: Duration::from_millis(21),
        frames_delivered: 48_000 * 600,
        lost_frames: 0,
    }
}

/// Nothing on the machine is producing output. The module's OWN observation —
/// this is the reading that makes `plausible_silence_causes` compute
/// `EveryoneMuted`.
fn nothing_playing() -> TapEnvironment {
    TapEnvironment {
        system_output_active: Some(false),
        ..TapEnvironment::default()
    }
}

/// Something IS producing output while the tap stays quiet. The only reading
/// that can support a permission verdict.
fn something_playing() -> TapEnvironment {
    TapEnvironment {
        system_output_active: Some(true),
        ..TapEnvironment::default()
    }
}

/// The shipping watchdog's own inputs, healthy in every respect except the tap.
fn shipping_inputs(elapsed: Duration, tap: TapWatchdogInputs) -> WatchdogInputs {
    WatchdogInputs {
        elapsed,
        free_bytes: DISK_FLOOR_BYTES * 8,
        device_failed: false,
        since_last_block: Duration::from_millis(100),
        thermal: ThermalState::Nominal,
        cap_warned: false,
        tap: Some(tap),
    }
}

/// Drive a whole meeting at the shipping interval and hand back every action.
///
/// The caller reports every rebuild back immediately, which is the *generous*
/// configuration — a watchdog that still misbehaves when its caller is perfect
/// is misbehaving on its own account.
fn run_meeting(
    ticks: u32,
    env: TapEnvironment,
    liveness: impl Fn(Duration) -> TapLiveness,
) -> (GhostWatchdog, Vec<TapWatchdogAction>) {
    let mut dog = GhostWatchdog::new();
    let mut actions = Vec::new();
    for tick in 1..=ticks {
        let elapsed = WATCHDOG_INTERVAL * tick;
        let action = dog.tick(elapsed, liveness(elapsed), env, 4_242 + tick as u64);
        actions.push(action);
        if action.is_rebuild() {
            dog.finish_rebuild(
                TapRebuildOutcome::Succeeded,
                elapsed + Duration::from_secs(1),
            );
        }
    }
    (dog, actions)
}

// ── Defect one: the quiet, granted meeting ──────────────────────────────────

/// **The headline for defect one.** Twenty minutes of an ordinary in-person
/// meeting — permission granted, tap healthy, nothing playing — costs zero
/// rebuilds and earns no verdict at all.
///
/// The numbers in the assertions are the ones the review reproduced: three full
/// 7-step rebuilds at t=60/120/180 s and a degrade at t=240 s reading "System
/// audio was never captured — it looks like permission is off."
#[test]
fn a_quiet_but_granted_meeting_is_never_rebuilt_and_never_badged_denied() {
    let (dog, actions) = run_meeting(MEETING_TICKS, nothing_playing(), |elapsed| {
        quiet_tap(elapsed, false)
    });

    assert!(
        actions.iter().all(|a| *a == TapWatchdogAction::Continue),
        "a meeting with nothing playing must produce no tap action at all: {actions:?}"
    );
    assert_eq!(
        dog.log().count(),
        0,
        "each of these is a real CoreAudio tap + aggregate teardown/recreate on a working tap"
    );
    assert!(dog.log().is_empty());
    assert_eq!(dog.log().verdict(), None);
    assert!(!dog.is_degraded());

    // The first four minutes specifically — where the review saw t=60/120/180
    // rebuild and t=240 degrade.
    for tick in 1..=4u32 {
        assert_eq!(
            actions[tick as usize - 1],
            TapWatchdogAction::Continue,
            "t={}s",
            (WATCHDOG_INTERVAL * tick).as_secs()
        );
    }
}

/// The same meeting, but the tap DID deliver audio earlier — a call where
/// everyone then muted. Also zero rebuilds: the silence has the same
/// explanation whichever way the discriminator reads.
#[test]
fn a_call_that_goes_quiet_after_real_audio_is_not_rebuilt_either() {
    let (dog, actions) = run_meeting(MEETING_TICKS, nothing_playing(), |elapsed| {
        quiet_tap(elapsed, true)
    });
    assert!(actions.iter().all(|a| *a == TapWatchdogAction::Continue));
    assert_eq!(dog.log().count(), 0);
    assert!(!dog.is_degraded());
}

/// The same twenty minutes through `meeting::watchdog_tick` — the function the
/// shipping watchdog thread calls, tap wired in exactly as `MeetingSession`
/// wires it. The pure tick being right is not worth much if the wiring is not.
#[test]
fn the_shipping_watchdog_also_leaves_a_quiet_meeting_alone() {
    let mut ghost = GhostWatchdog::new();
    for tick in 1..=MEETING_TICKS {
        let elapsed = WATCHDOG_INTERVAL * tick;
        let liveness = quiet_tap(elapsed, false);
        let env = nothing_playing();
        let action = watchdog_tick(&shipping_inputs(
            elapsed,
            TapWatchdogInputs {
                elapsed,
                liveness,
                env,
                state: ghost.state(),
            },
        ));
        assert_eq!(
            action,
            WatchdogAction::Continue,
            "t={}s produced {action:?}",
            elapsed.as_secs()
        );
        ghost.apply(TapWatchdogAction::Continue, elapsed, liveness, env, 1);
    }
    assert!(ghost.log().is_empty());
}

/// Silence with nothing playing is *explained*, and that is what suppresses it —
/// the enumerated cause doing work rather than decorating a log line.
#[test]
fn silence_with_nothing_playing_is_explained_and_silence_with_audio_playing_is_not() {
    let tap = quiet_tap(Duration::from_secs(600), false);
    assert!(!is_unexplained_silence(&tap, &nothing_playing()));
    assert!(is_unexplained_silence(&tap, &something_playing()));
    // An unreadable probe is not an explanation either — `None` must never
    // become evidence in either direction, so the silence stays actionable.
    assert!(is_unexplained_silence(&tap, &TapEnvironment::default()));
}

/// A permission verdict requires evidence that there was something to capture.
/// Without it the degrade says what was actually observed, and its banner never
/// mentions permission.
#[test]
fn the_permission_verdict_requires_evidence_that_something_was_playing() {
    // Nothing was ever seen playing (the probe could not be read at all) and the
    // tap never delivered: honest verdict, no accusation.
    let (unknown_dog, unknown_actions) =
        run_meeting(MEETING_TICKS, TapEnvironment::default(), |elapsed| {
            quiet_tap(elapsed, false)
        });
    let degrade = unknown_actions
        .iter()
        .rev()
        .find(|a| a.is_degrade())
        .expect("an unexplained silent tap still degrades once the budget is spent");
    assert_eq!(
        degrade.verdict(),
        Some(TapVerdict::NoSystemAudioObserved),
        "nothing was ever observed playing, so nothing here says 'denied'"
    );
    assert_eq!(
        unknown_dog.log().count(),
        MAX_TAP_REBUILDS_PER_MEETING,
        "and it still spent its rebuilds first — rule (a) is untouched"
    );
    let banner = TapVerdict::NoSystemAudioObserved.banner();
    assert!(
        !banner.to_lowercase().contains("permission"),
        "banner accuses permission without evidence: {banner}"
    );
    assert!(!TapVerdict::NoSystemAudioObserved.blames_permission());
    assert!(banner.contains("microphone track is still recording"));

    // Something WAS playing, and the tap never delivered a sample through three
    // rebuilds. That is the denial, and it is allowed to say so.
    let (denied_dog, denied_actions) = run_meeting(MEETING_TICKS, something_playing(), |elapsed| {
        quiet_tap(elapsed, false)
    });
    let denied = denied_actions
        .iter()
        .rev()
        .find(|a| a.is_degrade())
        .expect("a silent tap with audio playing degrades");
    assert_eq!(denied.verdict(), Some(TapVerdict::PermissionLikelyDenied));
    assert!(TapVerdict::PermissionLikelyDenied.blames_permission());
    assert_eq!(
        denied_dog.log().verdict(),
        Some(TapVerdict::PermissionLikelyDenied)
    );

    // The two sessions differ ONLY in the output probe, and that is the whole
    // difference between an observation and an accusation.
    assert_ne!(degrade, denied);
}

/// Evidence belongs to the stretch of silence it was collected in. A tap that
/// went quiet while music played, recovered, and then went quiet again in a
/// silent room does not carry the old observation into the new verdict.
#[test]
fn evidence_that_audio_was_playing_does_not_outlive_the_silence_it_belonged_to() {
    let mut dog = GhostWatchdog::new();
    // Quiet while something plays: the observation lands.
    dog.tick(
        WATCHDOG_INTERVAL,
        quiet_tap(Duration::from_secs(60), false),
        something_playing(),
        1,
    );
    assert!(dog.state().output_active_observed);
    // The tap delivers again: the stretch is over and so is its evidence.
    dog.tick(
        WATCHDOG_INTERVAL * 2,
        delivering_tap(),
        nothing_playing(),
        2,
    );
    assert!(!dog.state().output_active_observed);
}

// ── Defect two: the stale in-flight flag ────────────────────────────────────

/// **The headline for defect two.** One rebuild is issued and its caller never
/// reports back. The tap starts delivering audio on the very next tick. For the
/// remaining nineteen minutes the watchdog must never touch it again — no
/// second rebuild, no third, and above all no "system audio stopped coming
/// through" banner on a tap that is recording.
#[test]
fn a_delivering_tap_is_never_rebuilt_or_degraded_by_a_stale_in_flight_flag() {
    let mut dog = GhostWatchdog::new();
    let mut actions = Vec::new();

    // t=60 s: genuine silence with audio playing ⇒ the first rebuild. Its caller
    // never calls `finish_rebuild` — which is the SHIPPING configuration, since
    // no call site in `src/` calls it at this commit.
    let first = dog.tick(
        WATCHDOG_INTERVAL,
        quiet_tap(Duration::from_secs(60), true),
        something_playing(),
        7,
    );
    assert!(first.is_rebuild(), "got {first:?}");

    // …and from t=120 s on, audio is arriving on every callback.
    for tick in 2..=MEETING_TICKS {
        let elapsed = WATCHDOG_INTERVAL * tick;
        actions.push(dog.tick(
            elapsed,
            delivering_tap(),
            something_playing(),
            7 + tick as u64,
        ));
    }

    assert!(
        !actions.iter().any(|a| a.is_rebuild()),
        "a tap delivering audio was rebuilt again: {actions:?}"
    );
    assert!(
        !actions.iter().any(|a| a.is_degrade()),
        "a tap delivering audio was degraded: {actions:?}"
    );
    assert!(!dog.is_degraded());
    assert_eq!(dog.log().count(), 1, "exactly the one genuine attempt");
    assert_eq!(dog.log().verdict(), None);

    // The open attempt is closed rather than left dangling — the log says the
    // rebuild worked, which is what the audio says too.
    assert_eq!(
        dog.log().attempts()[0].outcome,
        Some(TapRebuildOutcome::Succeeded)
    );
    assert_eq!(
        actions[0],
        TapWatchdogAction::RebuildSettled {
            outcome: TapRebuildOutcome::Succeeded
        },
        "the first tick after recovery closes the attempt and nothing else"
    );
}

/// The narrow version of the same rule, stated against the pure function: a
/// stale in-flight flag, two whole timeouts old, on a tap that is delivering.
/// The only correct answer is "close it".
#[test]
fn liveness_is_read_before_the_in_flight_timeout_is_honoured() {
    for rebuilds_issued in 0..=MAX_TAP_REBUILDS_PER_MEETING {
        let action = ghost_tick(&TapWatchdogInputs {
            elapsed: WATCHDOG_INTERVAL + TAP_REBUILD_IN_FLIGHT_TIMEOUT * 2,
            liveness: delivering_tap(),
            env: something_playing(),
            state: GhostState {
                rebuilds_issued,
                rebuild_issued_at: Some(WATCHDOG_INTERVAL),
                ..GhostState::default()
            },
        });
        assert_eq!(
            action,
            TapWatchdogAction::RebuildSettled {
                outcome: TapRebuildOutcome::Succeeded
            },
            "with {rebuilds_issued} rebuilds spent, a delivering tap still must not be touched"
        );
        assert_eq!(action.verdict(), None);
    }
}

/// The "wait for it" branch must be reachable **in the shipping
/// configuration**. A timeout shorter than the interval that reads it is not a
/// timeout, it is an immediate expiry dressed as one: every in-flight rebuild
/// is already stale by the next tick and the branch is dead code.
#[test]
fn an_in_flight_rebuild_is_actually_waited_for_at_the_shipping_interval() {
    assert!(
        TAP_REBUILD_IN_FLIGHT_TIMEOUT >= WATCHDOG_INTERVAL * 2,
        "in-flight timeout {TAP_REBUILD_IN_FLIGHT_TIMEOUT:?} is under two \
         {WATCHDOG_INTERVAL:?} ticks, so the wait branch cannot be reached"
    );

    // Issued at t=60 s, still genuinely silent: the tick at t=120 s waits.
    let waiting = ghost_tick(&TapWatchdogInputs {
        elapsed: WATCHDOG_INTERVAL * 2,
        liveness: quiet_tap(Duration::from_secs(120), true),
        env: something_playing(),
        state: GhostState {
            rebuilds_issued: 1,
            rebuild_issued_at: Some(WATCHDOG_INTERVAL),
            ..GhostState::default()
        },
    });
    assert_eq!(
        waiting,
        TapWatchdogAction::Continue,
        "one tick after issuing, a rebuild is still in flight — not expired"
    );

    // …and it does eventually expire, so nothing hangs forever.
    let expired = ghost_tick(&TapWatchdogInputs {
        elapsed: WATCHDOG_INTERVAL + TAP_REBUILD_IN_FLIGHT_TIMEOUT,
        liveness: quiet_tap(Duration::from_secs(180), true),
        env: something_playing(),
        state: GhostState {
            rebuilds_issued: 1,
            rebuild_issued_at: Some(WATCHDOG_INTERVAL),
            ..GhostState::default()
        },
    });
    assert!(expired.is_rebuild(), "got {expired:?}");
}

/// A stale in-flight flag on a tap whose silence turns out to be explained is
/// released rather than spent: the attempt closes as `unknown` — nobody here
/// knows whether the rebuild worked — and no new teardown is issued.
#[test]
fn an_explained_silence_releases_a_stale_flag_without_spending_the_budget() {
    let mut dog = GhostWatchdog::new();
    let first = dog.tick(
        WATCHDOG_INTERVAL,
        quiet_tap(Duration::from_secs(60), true),
        something_playing(),
        3,
    );
    assert!(first.is_rebuild());

    // Everyone mutes. The tap stays quiet, and nobody ever reported back.
    let mut actions = Vec::new();
    for tick in 2..=MEETING_TICKS {
        let elapsed = WATCHDOG_INTERVAL * tick;
        actions.push(dog.tick(
            elapsed,
            quiet_tap(elapsed, true),
            nothing_playing(),
            3 + tick as u64,
        ));
    }

    assert!(!actions.iter().any(|a| a.is_rebuild()));
    assert!(!actions.iter().any(|a| a.is_degrade()));
    assert_eq!(dog.log().count(), 1);
    assert_eq!(
        dog.log().attempts()[0].outcome,
        Some(TapRebuildOutcome::Unknown),
        "the attempt is closed honestly, not counted as a timeout it never proved"
    );
    assert_eq!(dog.state().rebuild_issued_at, None);
}
