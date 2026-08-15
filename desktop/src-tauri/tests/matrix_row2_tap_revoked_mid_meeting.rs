//! Matrix row 2 — **tap permission revoked, or the tap dies mid-meeting**.
//!
//! Required behaviour (plan §6): log a `track_lost` marker into the meeting,
//! keep Track A, banner in the pill. **Do not stop the meeting.**
//!
//! This row is the far end of YV104's ghost watchdog, and the two things it has
//! to prove are opposite in shape:
//!
//!   1. the degrade **happens** — a tap that delivered real audio and then went
//!      to zeros gets its whole rebuild budget and then a `track_lost` verdict
//!      that says the tap stopped, *not* that permission is off;
//!   2. the degrade **stays a degrade** — nothing on this path can end the
//!      meeting, and that is driven through `meeting::watchdog_tick` itself,
//!      the 60 s rule the session actually runs, with the mic track healthy
//!      throughout.
//!
//! Published as `PolicyOnly` with **#123 (YV100)** owning the wiring, because
//! the branch is unreachable in the shipping app: `WatchdogInputs::tap` comes
//! from `CaptureEnv::tap_liveness`, whose default is `None` and which no
//! environment in the tree overrides. A tap that cannot exist cannot die. The
//! last test here is what keeps that publication honest as the tree changes.

use std::time::Duration;

use wilson_voice_lib::meeting::{
    watchdog_tick, ThermalState, WatchdogAction, WatchdogInputs, WATCHDOG_INTERVAL,
};
use wilson_voice_lib::meeting_matrix::{Coverage, ROWS};
use wilson_voice_lib::rtring::CaptureAnchor;
use wilson_voice_lib::syscapture::{
    fold_block, GhostWatchdog, TapEnvironment, TapLiveness, TapVerdict, TapWatchdogAction,
    TapWatchdogInputs, MAX_TAP_REBUILDS_PER_MEETING,
};

#[path = "support/callsite.rs"]
mod callsite;

const RATE: u32 = 48_000;
const BLOCK_FRAMES: usize = 480;
/// Plenty of disk, so nothing else in the 60 s rule can be the reason a tick
/// answers what it answers.
const ROOMY_DISK: u64 = 500 * 1024 * 1024 * 1024;

fn anchor(index: u64) -> CaptureAnchor {
    CaptureAnchor {
        host_ns: index * 10_000_000,
        sample_index: index * BLOCK_FRAMES as u64,
        frames: BLOCK_FRAMES as u32,
        sample_rate: RATE,
        lost_frames: 0,
    }
}

/// A meeting that recorded the call properly for `good_minutes` and then lost
/// the tap: every block after that is exact zeros, delivered on cadence,
/// forever. That is what both a mid-meeting revocation and OS-4's ghost look
/// like from the drain side, and it is the case row 2 owns.
struct LostTap {
    live: TapLiveness,
    last_nonzero: Duration,
    index: u64,
}

impl LostTap {
    fn after_good_audio(good_minutes: u32) -> Self {
        let mut this = Self {
            live: TapLiveness::started(),
            last_nonzero: Duration::ZERO,
            index: 0,
        };
        for minute in 1..=good_minutes {
            this.minute(WATCHDOG_INTERVAL * minute, true);
        }
        this
    }

    /// Fold one 60 s stretch of 10 ms blocks in, all speech or all zeros.
    fn minute(&mut self, elapsed: Duration, audible: bool) {
        let block: Vec<f32> = if audible {
            (0..BLOCK_FRAMES)
                .map(|i| ((i as f32) / BLOCK_FRAMES as f32) - 0.5)
                .collect()
        } else {
            vec![0.0; BLOCK_FRAMES]
        };
        for _ in 0..(WATCHDOG_INTERVAL.as_millis() as u64 / 10) {
            fold_block(
                &mut self.live,
                &block,
                &[anchor(self.index)],
                elapsed,
                &mut self.last_nonzero,
            );
            self.index += 1;
        }
    }
}

/// The environment during the loss: the aggregate still points at the right
/// device, a real dispatch queue is installed, and the call is audibly still
/// going — which is what makes the silence a fault rather than a quiet room.
fn call_still_audible() -> TapEnvironment {
    TapEnvironment {
        system_output_active: Some(true),
        ..TapEnvironment::default()
    }
}

#[test]
fn a_tap_that_worked_and_then_died_degrades_to_track_lost_after_its_whole_budget() {
    let mut tap = LostTap::after_good_audio(5);
    let mut ghost = GhostWatchdog::new();
    let env = call_still_audible();
    let mut actions = Vec::new();

    for minute in 6..=20 {
        let elapsed = WATCHDOG_INTERVAL * minute;
        tap.minute(elapsed, false);
        let action = ghost.tick(elapsed, tap.live, env, tap.live.frames_delivered);
        actions.push(action);
        if action.is_rebuild() {
            // The rebuild ran and reported back; the tap is still dead.
            ghost.finish_rebuild(
                wilson_voice_lib::syscapture::TapRebuildOutcome::Succeeded,
                elapsed,
            );
        }
    }

    assert_eq!(
        actions.iter().filter(|a| a.is_rebuild()).count(),
        MAX_TAP_REBUILDS_PER_MEETING as usize,
        "the budget is spent exactly once, not per tick: {actions:#?}"
    );
    let degrade = actions.iter().find(|a| a.is_degrade()).expect("degrade");
    match degrade {
        TapWatchdogAction::DegradeTrackLost {
            verdict,
            after_rebuilds,
            ..
        } => {
            assert_eq!(*after_rebuilds, MAX_TAP_REBUILDS_PER_MEETING);
            assert_eq!(
                *verdict,
                TapVerdict::GhostTapUnrecovered,
                "this tap DID deliver real audio before it died, which is the one bit that \
                 separates it from a denial — it must not be badged as a permission problem"
            );
            assert!(!verdict.blames_permission());
        }
        other => panic!("expected a degrade, got {other:?}"),
    }
    assert!(ghost.is_degraded());
    assert_eq!(ghost.log().verdict(), Some(TapVerdict::GhostTapUnrecovered));
    assert!(
        ghost.log().budget_exhausted(),
        "the meeting row has to be able to say afterwards that the budget was spent"
    );
}

/// Once degraded, the meeting is left alone. No second badge, no fourth
/// rebuild, no re-litigating the verdict every 60 s for the next two hours.
#[test]
fn a_degraded_meeting_is_never_rebuilt_or_re_badged() {
    let mut tap = LostTap::after_good_audio(5);
    let mut ghost = GhostWatchdog::new();
    let env = call_still_audible();

    let mut after_degrade = Vec::new();
    let mut degraded_at = None;
    for minute in 6..=40 {
        let elapsed = WATCHDOG_INTERVAL * minute;
        tap.minute(elapsed, false);
        let action = ghost.tick(elapsed, tap.live, env, tap.live.frames_delivered);
        if degraded_at.is_some() {
            after_degrade.push(action);
        }
        if action.is_degrade() && degraded_at.is_none() {
            degraded_at = Some(minute);
        }
        if action.is_rebuild() {
            ghost.finish_rebuild(
                wilson_voice_lib::syscapture::TapRebuildOutcome::Succeeded,
                elapsed,
            );
        }
    }

    assert!(degraded_at.is_some(), "never degraded");
    assert!(!after_degrade.is_empty(), "nothing ran after the degrade");
    assert!(
        after_degrade
            .iter()
            .all(|a| *a == TapWatchdogAction::Continue),
        "after the degrade the watchdog is done: {after_degrade:#?}"
    );
    assert_eq!(
        ghost.log().count(),
        MAX_TAP_REBUILDS_PER_MEETING,
        "the rebuild log is the meeting's record of what happened and must not keep growing"
    );
}

/// **The row's actual promise, driven through the rule the session runs.**
///
/// `meeting::watchdog_tick` is the 60 s decision the capture session takes, and
/// a dead tap has to come out of it as a `Tap` action and never as a `Stop` —
/// with the mic healthy the whole time. The mic's own stall limit is a
/// *separate* input (`since_last_block`, Track A only) for exactly this reason;
/// sharing one field between the two would end a meeting because the far end of
/// a call went quiet.
#[test]
fn the_shipping_watchdog_never_stops_a_meeting_over_a_dead_tap() {
    let mut tap = LostTap::after_good_audio(5);
    let mut ghost = GhostWatchdog::new();
    let env = call_still_audible();
    let mut saw_degrade = false;

    for minute in 6..=40 {
        let elapsed = WATCHDOG_INTERVAL * minute;
        tap.minute(elapsed, false);
        let action = watchdog_tick(&WatchdogInputs {
            elapsed,
            free_bytes: ROOMY_DISK,
            device_failed: false,
            // Track A is delivering perfectly throughout. This is the whole
            // point: the mic is fine, only the call's audio is gone.
            since_last_block: Duration::ZERO,
            thermal: ThermalState::Nominal,
            cap_warned: true,
            tap: Some(TapWatchdogInputs {
                elapsed,
                liveness: tap.live,
                env,
                state: ghost.state(),
            }),
        });

        match action {
            WatchdogAction::Stop(reason) => panic!(
                "matrix row 2: the meeting was STOPPED over a system-audio tap at {elapsed:?} \
                 ({reason}). Losing Track B is a banner and a marker; Track A is still recording \
                 the person holding the Mac."
            ),
            WatchdogAction::Tap(tap_action) => {
                ghost.apply(
                    tap_action,
                    elapsed,
                    tap.live,
                    env,
                    tap.live.frames_delivered,
                );
                if tap_action.is_rebuild() {
                    ghost.finish_rebuild(
                        wilson_voice_lib::syscapture::TapRebuildOutcome::Succeeded,
                        elapsed,
                    );
                }
                saw_degrade |= tap_action.is_degrade();
            }
            WatchdogAction::Continue | WatchdogAction::WarnApproachingCap => {}
        }
    }

    assert!(
        saw_degrade,
        "the meeting survived, but it never actually degraded — this test has to reach the \
         `track_lost` path to be evidence for anything"
    );
}

/// **The absence half.** Row 2's behaviour needs a tap to lose, and nothing in
/// `src/` starts one.
///
/// `start_system_tap` itself now exists — YV100 (#123) merged the tap module —
/// so the exclusion this check has always been entitled to is finally load
/// bearing: `syscapture.rs` is where the symbol is *defined*, and
/// `callsite::call_sites`'s own doc comment is that *"a symbol's own definition
/// is not a call site"*. Rows 3 and 14 have passed their defining module here
/// since YV105 wrote them; row 2 passed `&[]` only because at that commit there
/// was nothing to exclude.
///
/// What this asserts is therefore unchanged and is the thing that matters: no
/// file in the shipping tree **calls** it. The day one does, this goes red with
/// the promote-the-row instruction, exactly as designed.
#[test]
fn start_system_tap_is_still_absent_so_row_2_is_not_wired() {
    let found = callsite::call_sites("start_system_tap", &["syscapture.rs"]);
    assert!(
        found.is_empty(),
        "{}",
        callsite::promote_the_row("2", "start_system_tap", &found)
    );

    // And the exclusion is not a blanket amnesty for that file: the definition
    // is skipped, a call from anywhere else is not. Proved rather than trusted,
    // because "we excluded the module the tap lives in" would otherwise be
    // indistinguishable from "we stopped checking".
    let syscapture = std::fs::read_to_string(callsite::src_dir().join("syscapture.rs"))
        .expect("read syscapture.rs");
    assert!(
        callsite::mentions_as_code(&syscapture, "start_system_tap"),
        "the symbol must really be defined there, or this exclusion is hiding nothing          and the assertion above is vacuous"
    );
}

/// The same fact from the other side, and the more important one, because it is
/// the reason the branch above is dead code in the app: no `CaptureEnv` in the
/// shipping tree ever returns a tap liveness, so `WatchdogInputs::tap` is
/// `None` on every real tick.
#[test]
fn no_shipping_capture_env_supplies_a_tap_liveness_yet() {
    let meeting = std::fs::read_to_string(callsite::src_dir().join("meeting.rs")).expect("read");
    let overrides: Vec<&str> = meeting
        .lines()
        .filter(|l| callsite::code_only(l).contains("fn tap_liveness"))
        .collect();
    assert_eq!(
        overrides.len(),
        1,
        "`tap_liveness` is declared more than once in meeting.rs — if that is an impl on a real \
         environment rather than the trait's own default, row 2 has a producer and must be \
         promoted: {overrides:?}"
    );
    let others = callsite::call_sites("tap_liveness", &["meeting.rs"]);
    assert!(
        others.is_empty(),
        "{}",
        callsite::promote_the_row("2", "tap_liveness", &others)
    );
}

#[test]
fn the_published_cell_names_the_owner_of_the_missing_wiring() {
    let row = ROWS.iter().find(|r| r.id == "2").expect("row 2");
    assert_eq!(
        row.coverage,
        Coverage::PolicyOnly {
            test: "matrix_row2_tap_revoked_mid_meeting.rs",
            // No PR owns this any more: #123 landed the tap module and nothing
            // in the 22-B backlog calls it. `None` is the honest owner, and it
            // is also the value that puts this row under `matrix_coverage`'s
            // standing unowned-row tripwire.
            wiring_pr: None,
            absent_call_site: "start_system_tap",
        }
    );
    let cell = row.coverage.cell();
    assert!(cell.contains("NOT WIRED"), "{cell}");
    assert!(cell.contains("start_system_tap"), "{cell}");
    // The cell must not go on naming a merged PR as the thing still to come.
    assert!(!cell.contains("#123"), "{cell}");
}
