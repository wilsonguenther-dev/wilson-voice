//! YV104 / plan finding OS-4, rule (c) — **the denial discriminator the plan
//! said did not exist.**
//!
//! §2.1 concluded there was no reliable way to tell "the user denied system-audio
//! recording" from "the tap is delivering digital silence", and proposed a
//! heuristic built on elapsed silence plus `kAudioProcessPropertyIsRunningOutput`.
//! OS-4 shows that heuristic fires on Apple's own all-zero-buffer bug, which is
//! far more common than a mid-session TCC revocation, and then names the bit that
//! actually separates them:
//!
//! > record a single boolean, *did this tap ever deliver a non-zero sample?* A
//! > TCC denial is silent from sample zero; this bug is silent only after minutes
//! > of good audio. That one bit separates them with no private API and no
//! > guessing.
//!
//! This file drives both sessions through the identical rebuild ladder and
//! asserts three things: they are **indistinguishable** at every step before the
//! budget is spent (rule (a) — no premature verdict), they produce **different**
//! `TapWatchdogAction`s at the end, and the bit is computed only from data
//! `CaptureAnchor` and the drained blocks already carry — no new anchor field,
//! no private API, no probe that can fail.
//!
//! **What the bit does not do, added after review round 2.** `ever_nonzero`
//! separates a denial from OS-4's ghost, which is what OS-4 claims for it. It
//! does not separate a denial from a granted tap that had nothing to record, so
//! both sessions below are replayed with the machine **audibly producing
//! output** (`system_output_active: Some(true)`) — the only reading under which
//! "this tap heard nothing" is evidence of anything at all. The same denial
//! session replayed with nothing playing is not a denial and is asserted to
//! come out as `NoSystemAudioObserved` instead.
//!
//! No audio hardware, and no ASR: the two sessions are made of `f32` blocks.

use std::time::Duration;

use wilson_voice_lib::rtring::CaptureAnchor;
use wilson_voice_lib::syscapture::{
    fold_block, fold_idle, GhostWatchdog, TapEnvironment, TapLiveness, TapRebuildOutcome,
    TapVerdict, TapWatchdogAction, MAX_TAP_REBUILDS_PER_MEETING,
};

const RATE: u32 = 48_000;
const FRAMES: u32 = 512;

fn anchor(sample_index: u64) -> CaptureAnchor {
    CaptureAnchor {
        host_ns: 7_310_000_000_000 + sample_index * 20_833,
        sample_index,
        frames: FRAMES,
        sample_rate: RATE,
        lost_frames: 0,
    }
}

/// Replay one tap's whole life, block by block, and hand back the liveness it
/// folds to plus the actions the watchdog produced on 60 s ticks.
///
/// `good_seconds` is how long the tap delivered real audio before going to
/// zeros; 0 is the denial case (silent from sample zero) and anything else is
/// OS-4's ghost.
fn replay(
    good_seconds: u64,
    total_seconds: u64,
) -> (TapLiveness, Vec<TapWatchdogAction>, GhostWatchdog) {
    replay_in(good_seconds, total_seconds, audibly_playing())
}

/// Something is producing output on this machine for the whole session, so a
/// tap that hears nothing is hearing nothing *about something*.
fn audibly_playing() -> TapEnvironment {
    TapEnvironment {
        system_output_active: Some(true),
        ..TapEnvironment::default()
    }
}

fn replay_in(
    good_seconds: u64,
    total_seconds: u64,
    env: TapEnvironment,
) -> (TapLiveness, Vec<TapWatchdogAction>, GhostWatchdog) {
    let mut live = TapLiveness::started();
    let mut last_nonzero = Duration::ZERO;
    let mut dog = GhostWatchdog::new();
    let mut actions = Vec::new();

    // Real room tone, then exact zeros. OS-4's signature is "every sample in
    // every buffer is exactly 0.0f", so the quiet audio below is deliberately
    // tiny but non-zero — a tap delivering -90 dBFS room tone is a tap that
    // WORKS, and a discriminator that used an amplitude threshold would call it
    // denied.
    let mut room_tone = [0.0f32; FRAMES as usize];
    for (i, sample) in room_tone.iter_mut().enumerate() {
        *sample = if i % 7 == 0 { 0.000_015 } else { -0.000_015 };
    }
    let zeros = [0.0f32; FRAMES as usize];

    let mut sample_index = 0u64;
    for second in 1..=total_seconds {
        // ~94 callbacks a second at 512 frames / 48 kHz.
        for _ in 0..94 {
            let block: &[f32] = if second <= good_seconds {
                &room_tone
            } else {
                &zeros
            };
            fold_block(
                &mut live,
                block,
                &[anchor(sample_index)],
                Duration::from_secs(second),
                &mut last_nonzero,
            );
            sample_index += FRAMES as u64;
        }
        // The watchdog ticks once a minute, on the same liveness the drain folded.
        if second % 60 == 0 {
            let elapsed = Duration::from_secs(second);
            let action = dog.tick(elapsed, live, env, live_host_ns(&live));
            actions.push(action);
            if action.is_rebuild() {
                // A rebuild does NOT reset the discriminator. That is the whole
                // point of it being a property of the meeting rather than of the
                // tap object: a denial survives three new taps, and so does the
                // memory of the audio that arrived before the ghost.
                dog.finish_rebuild(
                    TapRebuildOutcome::Succeeded,
                    elapsed + Duration::from_secs(1),
                );
            }
        }
    }
    (live, actions, dog)
}

fn live_host_ns(live: &TapLiveness) -> u64 {
    // Stand-in for "the most recent anchor's host time"; a tap that delivered
    // nothing still has anchors here because the IOProc is firing, which is
    // exactly the case that makes the bit non-trivial.
    7_310_000_000_000 + live.frames_delivered
}

/// **The headline.** Identical silence, identical rebuild ladder, different
/// verdicts — and the only input that differs is one boolean folded from the
/// samples themselves.
#[test]
fn a_tap_that_delivered_audio_and_one_that_never_did_end_in_different_actions() {
    let (ghost_live, ghost_actions, ghost_dog) = replay(300, 900);
    let (denied_live, denied_actions, denied_dog) = replay(0, 900);

    // The bit itself.
    assert!(ghost_live.ever_nonzero, "five minutes of audio did arrive");
    assert!(!denied_live.ever_nonzero, "nothing ever arrived");

    // Both taps look identical to every OTHER measure a watchdog has: the same
    // number of frames delivered, the same live IOProc cadence, the same
    // silence. This is the assertion that makes the discriminator load-bearing
    // rather than decorative.
    assert_eq!(ghost_live.frames_delivered, denied_live.frames_delivered);
    assert_eq!(ghost_live.since_last_block, denied_live.since_last_block);
    assert_eq!(ghost_live.lost_frames, denied_live.lost_frames);

    // Both degraded, and the actions differ.
    let ghost_final = ghost_actions.iter().rev().find(|a| a.is_degrade()).unwrap();
    let denied_final = denied_actions
        .iter()
        .rev()
        .find(|a| a.is_degrade())
        .unwrap();
    assert_ne!(
        ghost_final, denied_final,
        "the two sessions must not produce the same WatchdogAction"
    );
    assert_eq!(
        ghost_final.verdict(),
        Some(TapVerdict::GhostTapUnrecovered),
        "a tap that produced five minutes of audio is not a permission problem"
    );
    assert_eq!(
        denied_final.verdict(),
        Some(TapVerdict::PermissionLikelyDenied),
        "a tap that was silent from sample zero is what a TCC denial looks like"
    );
    assert_eq!(
        ghost_dog.log().verdict(),
        Some(TapVerdict::GhostTapUnrecovered)
    );
    assert_eq!(
        denied_dog.log().verdict(),
        Some(TapVerdict::PermissionLikelyDenied)
    );
}

/// **The third session the first cut of this module could not see.** Identical
/// to the denied one — silent from sample zero, all-zero buffers, IOProc firing
/// on cadence — except that nothing was ever playing into it. That is not a
/// denial, it is an in-person meeting, and it is the common case rather than an
/// edge case.
///
/// It must never produce a permission verdict, and it must never spend a
/// rebuild: three teardown/recreate cycles of a healthy tap in the first four
/// minutes is what the first cut did here.
#[test]
fn a_silent_tap_with_nothing_playing_is_not_a_denial_and_is_not_rebuilt() {
    let (live, actions, dog) = replay_in(
        0,
        900,
        TapEnvironment {
            system_output_active: Some(false),
            ..TapEnvironment::default()
        },
    );
    assert!(!live.ever_nonzero, "the bit reads the same as the denial");
    assert!(
        actions.iter().all(|a| *a == TapWatchdogAction::Continue),
        "a granted tap with nothing to record must not be torn down: {actions:?}"
    );
    assert_eq!(dog.log().count(), 0);
    assert_eq!(dog.log().verdict(), None);
    assert!(!dog.is_degraded());

    // And when the probe cannot be read at all, the silence IS acted on — an
    // unreadable probe is not an explanation — but the verdict at the end says
    // only what was seen.
    let (_, unknown_actions, _) = replay_in(0, 900, TapEnvironment::default());
    let degrade = unknown_actions
        .iter()
        .rev()
        .find(|a| a.is_degrade())
        .expect("an unexplained silent tap still degrades");
    assert_eq!(degrade.verdict(), Some(TapVerdict::NoSystemAudioObserved));
    assert!(!TapVerdict::NoSystemAudioObserved.blames_permission());
}

/// Rule (a) survives rule (c): the two sessions are **indistinguishable** until
/// the budget is spent. The discriminator changes the verdict at the end, never
/// the response at the start.
#[test]
fn the_two_sessions_are_indistinguishable_until_every_rebuild_is_spent() {
    let (_, ghost_actions, _) = replay(300, 900);
    let (_, denied_actions, _) = replay(0, 900);

    // The two taps go silent at different wall-clock times — one after five
    // minutes of audio, the other immediately — so the tick indices do not line
    // up and comparing them tick-for-tick would be comparing clocks, not
    // policy. What must match is the LADDER: every action each session took
    // that was not "carry on", in order.
    let ladder = |actions: &[TapWatchdogAction]| {
        actions
            .iter()
            .copied()
            .filter(|a| *a != TapWatchdogAction::Continue)
            .collect::<Vec<_>>()
    };
    let ghost_ladder = ladder(&ghost_actions);
    let denied_ladder = ladder(&denied_actions);

    assert_eq!(
        ghost_ladder.len(),
        denied_ladder.len(),
        "the two sessions must take the same NUMBER of steps"
    );
    let last = ghost_ladder.len() - 1;
    for (index, (a, b)) in ghost_ladder.iter().zip(denied_ladder.iter()).enumerate() {
        if index == last {
            // The degrade — the ONE step where the discriminator is allowed to
            // matter.
            assert!(a.is_degrade() && b.is_degrade());
            assert_ne!(a, b, "the final step is where the two must part company");
            continue;
        }
        assert_eq!(
            a, b,
            "step {index} differs before the budget is spent — a premature verdict"
        );
        assert!(
            a.is_rebuild(),
            "step {index} should be a rebuild, got {a:?}"
        );
        assert_eq!(
            a.verdict(),
            None,
            "step {index} carries a verdict too early"
        );
    }

    // …and the ladder is exactly the budget, in both sessions.
    assert_eq!(last, MAX_TAP_REBUILDS_PER_MEETING as usize);
}

/// The bit is a property of the **meeting**, not of the tap object: three full
/// rebuilds create three brand-new taps, and none of them can make the app
/// forget that real audio once arrived.
#[test]
fn a_rebuild_cannot_erase_the_memory_that_audio_once_arrived() {
    let (live, _, dog) = replay(120, 900);
    assert!(live.ever_nonzero);
    assert_eq!(dog.log().count(), MAX_TAP_REBUILDS_PER_MEETING);
    assert_eq!(dog.log().verdict(), Some(TapVerdict::GhostTapUnrecovered));
    // Whatever the output probe said: one real sample rules permission out for
    // the rest of the meeting, and no environment reading can put it back.
    assert_eq!(live.verdict(true), TapVerdict::GhostTapUnrecovered);
    assert_eq!(live.verdict(false), TapVerdict::GhostTapUnrecovered);
}

/// The discriminator is a fold over data YV100's ring already ships. This test
/// is the standing proof of "no new `CaptureAnchor` field required": it builds
/// the whole answer from the five fields the anchor has carried since 22-A.
#[test]
fn the_discriminator_needs_no_new_anchor_field() {
    let mut live = TapLiveness::started();
    let mut last_nonzero = Duration::ZERO;
    let a = CaptureAnchor {
        host_ns: 1_000,
        sample_index: 0,
        frames: FRAMES,
        sample_rate: RATE,
        lost_frames: 0,
    };
    fold_block(
        &mut live,
        &[0.0; 512],
        &[a],
        Duration::from_secs(1),
        &mut last_nonzero,
    );
    assert_eq!(live.frames_delivered, FRAMES as u64);
    assert!(!live.ever_nonzero);

    // `lost_frames` is cumulative on the anchor, so the fold takes the maximum
    // rather than a sum — an anchor the anchor ring itself dropped loses
    // nothing, which is the property the field was documented for in 22-A.
    let b = CaptureAnchor {
        sample_index: FRAMES as u64,
        lost_frames: 4_096,
        ..a
    };
    let c = CaptureAnchor {
        sample_index: 2 * FRAMES as u64,
        lost_frames: 4_096,
        ..a
    };
    fold_block(
        &mut live,
        &[0.0; 512],
        &[b, c],
        Duration::from_secs(2),
        &mut last_nonzero,
    );
    assert_eq!(live.lost_frames, 4_096, "cumulative, not summed");
    assert_eq!(live.frames_delivered, 3 * FRAMES as u64);
}

/// A tick where nothing was drained still advances the clocks — otherwise a tap
/// whose IOProc stops firing entirely looks frozen in time rather than silent,
/// and the watchdog never fires at all.
#[test]
fn an_idle_tick_advances_the_silence_clock_rather_than_freezing_it() {
    let mut live = TapLiveness::started();
    let mut last_nonzero = Duration::ZERO;
    let mut block = [0.0f32; 512];
    block[9] = 0.5;
    fold_block(
        &mut live,
        &block,
        &[],
        Duration::from_secs(10),
        &mut last_nonzero,
    );
    assert!(live.ever_nonzero);
    assert_eq!(live.since_nonzero, Duration::ZERO);

    for second in 11..=90u64 {
        fold_idle(
            &mut live,
            Duration::from_secs(second),
            last_nonzero,
            Duration::from_secs(1),
        );
    }
    assert_eq!(live.since_nonzero, Duration::from_secs(80));
    assert_eq!(live.since_last_block, Duration::from_secs(80));

    // …and the watchdog sees it, as a dead IOProc rather than as a ghost.
    let mut dog = GhostWatchdog::new();
    let action = dog.tick(Duration::from_secs(90), live, TapEnvironment::default(), 1);
    assert!(action.is_rebuild(), "got {action:?}");
}
