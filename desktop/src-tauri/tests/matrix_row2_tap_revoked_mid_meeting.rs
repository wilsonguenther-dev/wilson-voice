//! Matrix row 2 — **tap permission revoked, or the tap dies mid-meeting**.
//!
//! Required behaviour (plan §6): log a `track_lost` marker into the meeting,
//! keep Track A, banner in the pill. **Do not stop the meeting.**
//!
//! This row is the far end of YV104's ghost watchdog, and the three things it
//! has to prove are:
//!
//!   1. the degrade **happens** — a tap that delivered real audio and then went
//!      to zeros gets its whole rebuild budget and then a `track_lost` verdict
//!      that says the tap stopped, *not* that permission is off;
//!   2. the degrade **stays a degrade** — nothing on this path can end the
//!      meeting, and that is driven through `meeting::watchdog_tick` itself,
//!      the 60 s rule the session actually runs, with the mic track healthy
//!      throughout;
//!   3. the ladder is **reachable in the app** — which is what this row spent
//!      four merges waiting for.
//!
//! **Published as `Test` since YV110, and (3) is the whole of what changed.**
//! `WatchdogInputs::tap` comes from `CaptureEnv::tap_liveness`, whose default is
//! `None`; until YV110 no environment in the tree overrode it, so the branch was
//! unreachable and a tap that cannot exist cannot die. `syscapture::TappedEnv`
//! — this row's published subject — is the first shipping `CaptureEnv` that
//! answers `Some`, and it answers with the fold the meeting's own drain
//! computes. The last two tests here drive that env directly, so the row's
//! promotion rests on the same kind of evidence its demotion used to.
//!
//! One thing this file does NOT claim, and YV110 says so in its own module
//! docs: the seven CoreAudio calls of a mid-meeting rebuild are decided and
//! logged, not executed. The behaviour row 2 publishes — marker, mic keeps
//! recording, banner, never stopped — is what ships.

use std::time::Duration;

use std::sync::Arc;

use wilson_voice_lib::meeting::{
    watchdog_tick, CaptureEnv, RtCapture, ThermalState, WatchdogAction, WatchdogInputs,
    WATCHDOG_INTERVAL,
};
use wilson_voice_lib::meeting_matrix::{Coverage, ROWS};
use wilson_voice_lib::rtring::CaptureAnchor;
use wilson_voice_lib::syscapture::{
    fold_block, open_tap, teardown, GhostWatchdog, MeetingTap, TapEnvironment, TapLiveness,
    TapResources, TapVerdict, TapWatchdogAction, TapWatchdogInputs, TappedEnv,
    MAX_TAP_REBUILDS_PER_MEETING,
};

#[path = "support/callsite.rs"]
mod callsite;
#[path = "support/tap.rs"]
mod fake;

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

/// **The presence half**, which is this file's old absence half pointed the
/// other way the day the wiring landed.
///
/// `start_system_tap` is called from `src/` now (`meeting_control`'s
/// `open_meeting_tap`, from `SessionEngine::start`), so a meeting really does
/// have a tap to lose. Deleting the check when it fired would have been the
/// failure mode `matrix_coverage.rs` warns about; keeping it inverted catches
/// the regression that matters now — a refactor that drops the call would leave
/// this row published as `Test` about a meeting with no second track.
#[test]
fn a_meeting_really_does_open_a_tap_now() {
    let callers = callsite::call_sites("start_system_tap", &["syscapture.rs"]);
    assert!(
        !callers.is_empty(),
        "nothing in `src/` calls `start_system_tap` any more, so no meeting has a system-audio \
         track to lose. Row 2 must go back to `Coverage::PolicyOnly` rather than stay green."
    );

    // The exclusion is not a blanket amnesty for that file: the definition is
    // skipped, a call from anywhere else is not.
    let syscapture = std::fs::read_to_string(callsite::src_dir().join("syscapture.rs"))
        .expect("read syscapture.rs");
    assert!(
        callsite::mentions_as_code(&syscapture, "start_system_tap"),
        "the symbol must really be defined there, or this exclusion is hiding nothing"
    );
}

/// A tap the meeting can lose, with no CoreAudio: YV100's real setup state
/// machine over its fake platform, wrapped in the shipping `MeetingTap`.
fn attached_tap() -> (Arc<MeetingTap>, Arc<RtCapture>) {
    let mut platform = fake::FakePlatform {
        format: wilson_voice_lib::syscapture::TapFormat {
            sample_rate: RATE,
            channels: 1,
        },
        ..fake::FakePlatform::default()
    };
    let open = open_tap(&mut platform, Some(7), "row2-uid", "Yap meeting capture").expect("opens");
    let ring = platform.bound_capture.clone().expect("bound ring");
    let platform = std::sync::Mutex::new(platform);
    let mut resources: TapResources = open.resources;
    let tap = MeetingTap::new(Arc::clone(&ring), open.format, move || {
        teardown(
            &mut *platform.lock().expect("fake platform"),
            &mut resources,
        );
    });
    (tap, ring)
}

/// **The row's reachability, which is what YV110 changed.**
///
/// The liveness the ladder runs on is no longer assembled by a test: it comes
/// out of the shipping `CaptureEnv` a two-track meeting is configured with,
/// folded from the blocks that meeting's own drain handed over. A tap that
/// delivered a minute of real audio and then went quiet reaches
/// `meeting::watchdog_tick` as a `Tap` action — never a `Stop`.
#[test]
fn the_shipping_environment_supplies_the_liveness_this_row_runs_on() {
    let (tap, ring) = attached_tap();
    let env = TappedEnv::new(std::path::PathBuf::from("."), Arc::clone(&tap));

    // A minute of the call, captured properly.
    for i in 0..100u64 {
        let block: Vec<f32> = (0..BLOCK_FRAMES)
            .map(|n| (n as f32 / BLOCK_FRAMES as f32) - 0.5)
            .collect();
        wilson_voice_lib::meeting::rt_capture_callback(&ring, &block, |s| s, i * 10_000_000);
    }
    tap.pump(WATCHDOG_INTERVAL);
    let (live, _, host_ns) = env.tap_liveness().expect("a two-track meeting has a tap");
    assert!(
        live.ever_nonzero,
        "the discriminator that separates this row from row 1 has to be fed by the real drain"
    );
    assert!(
        host_ns > 0,
        "the anchor's host time reaches the rebuild log"
    );

    // …and then it dies. Every subsequent drain finds an empty ring, which is
    // what a dead IOProc looks like from the consumer side.
    let mut ghost = GhostWatchdog::new();
    let mut saw_degrade = false;
    for minute in 2..=20 {
        let elapsed = WATCHDOG_INTERVAL * minute;
        tap.pump(elapsed);
        let (liveness, tap_env, host_ns) = env.tap_liveness().expect("still attached");
        let action = watchdog_tick(&WatchdogInputs {
            elapsed,
            free_bytes: ROOMY_DISK,
            device_failed: false,
            // Track A is delivering perfectly throughout — the mic is fine, only
            // the call's audio is gone.
            since_last_block: Duration::ZERO,
            thermal: ThermalState::Nominal,
            cap_warned: true,
            tap: Some(TapWatchdogInputs {
                elapsed,
                liveness,
                env: tap_env,
                state: ghost.state(),
            }),
        });
        match action {
            WatchdogAction::Stop(reason) => panic!(
                "matrix row 2: the meeting was STOPPED over a system-audio tap at {elapsed:?} \
                 ({reason}) — with liveness read from the SHIPPING environment"
            ),
            WatchdogAction::Tap(tap_action) => {
                ghost.apply(tap_action, elapsed, liveness, tap_env, host_ns);
                saw_degrade |= tap_action.is_degrade();
            }
            WatchdogAction::Continue | WatchdogAction::WarnApproachingCap => {}
        }
    }
    assert!(
        saw_degrade,
        "a tap that stopped delivering must reach `track_lost` through the shipping env, or this \
         row is published as covered by a path the app cannot walk"
    );
    assert_eq!(
        ghost.log().verdict(),
        Some(TapVerdict::GhostTapUnrecovered),
        "it delivered real audio first, so this is OS-4's ghost and NOT a permission problem"
    );
    assert!(!ghost.log().verdict().unwrap().blames_permission());
    assert!(ghost
        .log()
        .verdict()
        .unwrap()
        .banner()
        .contains("microphone"));
}

#[test]
fn the_published_cell_names_the_environment_that_made_this_row_reachable() {
    let row = ROWS.iter().find(|r| r.id == "2").expect("row 2");
    assert_eq!(
        row.coverage,
        Coverage::Test {
            test: "matrix_row2_tap_revoked_mid_meeting.rs",
            subject: "TappedEnv",
            subject_module: "syscapture.rs",
        }
    );
    let cell = row.coverage.cell();
    assert!(!cell.contains("NOT WIRED"), "{cell}");
}
