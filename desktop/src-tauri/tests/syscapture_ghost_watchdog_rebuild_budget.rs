//! YV104 / plan finding OS-4 — **exactly three rebuilds, then an honest
//! degrade, and every attempt is on the record.**
//!
//! Rule (a) — rebuild first, never a verdict — is only half of the fix. Left
//! alone it is an infinite loop: a tap that is permanently broken (a real TCC
//! denial, a nil dispatch queue, an `exclusive` flag inverted at init) is silent
//! forever, so "rebuild on silence" tears down and recreates a tap and a private
//! aggregate device every watchdog tick for three hours. Each teardown emits its
//! own device-list notifications — the loop YV103's guard exists to break — and
//! the user gets nothing but a machine that is busy.
//!
//! So the budget is the other half, and this file is its test. Three attempts,
//! spread over the meeting rather than spent inside one minute, each one written
//! into the structure YV106's migration persists, and then a degrade that says
//! what actually happened.
//!
//! Two things this file deliberately proves about the *shape* of the budget:
//!
//! * A grace window after each rebuild. Without it the budget is spent in three
//!   consecutive ticks, because the silence duration on the tick after a rebuild
//!   is still whatever it was on the tick before — a tap that was recreated one
//!   second ago has not had time to deliver anything yet.
//! * A rebuild that is never reported back does not hang the watchdog forever.
//!   That failure — the caller panicking between two of the seven steps, or
//!   missing `finish_aggregate_work` — is the same "deaf for the rest of the
//!   meeting" outcome as the bug this whole module is about, arriving by a
//!   different road.

use std::time::Duration;

use wilson_voice_lib::syscapture::{
    GhostWatchdog, SilenceCause, TapEnvironment, TapLiveness, TapRebuildOutcome, TapSilenceKind,
    TapStep, TapVerdict, TapWatchdogAction, MAX_TAP_REBUILDS_PER_MEETING, TAP_REBUILD_GRACE,
    TAP_REBUILD_IN_FLIGHT_TIMEOUT, TAP_SILENCE_REBUILD_AFTER,
};

/// A tap that produced real audio and is now all zeros — OS-4's ghost.
fn ghost(silent_for: Duration) -> TapLiveness {
    TapLiveness {
        ever_nonzero: true,
        since_nonzero: silent_for,
        since_last_block: Duration::from_millis(21),
        frames_delivered: 48_000 * 600,
        lost_frames: 0,
    }
}

/// The watchdog interval a real meeting ticks at.
const TICK: Duration = Duration::from_secs(60);

/// Drive a meeting that stays silent forever, one 60 s tick at a time, with a
/// caller that dutifully runs the seven steps and reports success every time.
///
/// Returns every action the watchdog produced, in order.
fn silent_meeting(minutes: u64) -> (GhostWatchdog, Vec<TapWatchdogAction>) {
    let mut dog = GhostWatchdog::new();
    let mut actions = Vec::new();
    for minute in 1..=minutes {
        let elapsed = TICK * minute as u32;
        let action = dog.tick(
            elapsed,
            ghost(elapsed),
            TapEnvironment::default(),
            9_000 + minute,
        );
        actions.push(action);
        if action.is_rebuild() {
            // The caller runs `full_rebuild_sequence()` and reports back. A real
            // rebuild takes about a second; the report lands well inside the
            // same tick.
            dog.finish_rebuild(
                TapRebuildOutcome::Succeeded,
                elapsed + Duration::from_secs(1),
            );
        }
    }
    (dog, actions)
}

/// **The headline.** Exactly three rebuilds, then exactly one degrade, and never
/// another action for the rest of the meeting however long it runs.
#[test]
fn exactly_three_rebuilds_then_a_degrade_and_then_silence_from_the_watchdog() {
    let (dog, actions) = silent_meeting(60);

    let rebuilds: Vec<_> = actions.iter().filter(|a| a.is_rebuild()).collect();
    assert_eq!(
        rebuilds.len(),
        MAX_TAP_REBUILDS_PER_MEETING as usize,
        "the budget is {MAX_TAP_REBUILDS_PER_MEETING} — got {rebuilds:?}"
    );

    let degrades: Vec<_> = actions.iter().filter(|a| a.is_degrade()).collect();
    assert_eq!(
        degrades.len(),
        1,
        "the meeting degrades once and is never re-badged"
    );

    // Attempts are numbered 1, 2, 3 in the order they fired.
    let numbers: Vec<u32> = actions
        .iter()
        .filter_map(|a| match a {
            TapWatchdogAction::RebuildFull { attempt, .. } => Some(*attempt),
            _ => None,
        })
        .collect();
    assert_eq!(numbers, vec![1, 2, 3]);

    // The rebuilds all precede the degrade. Ordering, not just counting: a
    // degrade that fired before the budget was spent would be OS-4's original
    // bug wearing this item's clothes.
    let last_rebuild = actions.iter().rposition(|a| a.is_rebuild()).unwrap();
    let degrade_at = actions.iter().position(|a| a.is_degrade()).unwrap();
    assert!(last_rebuild < degrade_at);

    // …and nothing at all after it, for the remaining ~50 minutes.
    assert!(
        actions[degrade_at + 1..]
            .iter()
            .all(|a| *a == TapWatchdogAction::Continue),
        "the watchdog is done once it has degraded"
    );
    assert!(dog.is_degraded());
}

/// The budget is spread over the meeting, not spent in three consecutive ticks.
/// This is the grace window doing its job, and it is what makes three attempts a
/// real recovery strategy instead of a formality.
#[test]
fn the_three_rebuilds_are_spread_out_rather_than_burned_in_one_minute() {
    // A deliberately impatient watchdog: one tick per second, and a caller that
    // completes each rebuild instantly. Nothing but the grace window stands
    // between this and three rebuilds inside three seconds.
    let mut dog = GhostWatchdog::new();
    for second in 1..=600u32 {
        let elapsed = Duration::from_secs(second as u64);
        let action = dog.tick(elapsed, ghost(elapsed), TapEnvironment::default(), 55);
        if action.is_rebuild() {
            dog.finish_rebuild(TapRebuildOutcome::Succeeded, elapsed);
        }
    }

    let stamps: Vec<u64> = dog.log().attempts().iter().map(|a| a.at_ms).collect();
    assert_eq!(
        stamps.len(),
        3,
        "the budget holds at one tick per second too"
    );
    for pair in stamps.windows(2) {
        let gap = Duration::from_millis(pair[1] - pair[0]);
        assert!(
            gap >= TAP_REBUILD_GRACE,
            "consecutive rebuilds {pair:?} are {gap:?} apart, inside the \
             {TAP_REBUILD_GRACE:?} grace window — the budget was burned in a few seconds"
        );
    }
}

/// **The structure YV106's migration persists.** The count and every attempt's
/// timestamp are on the record, alongside enough context to diagnose the
/// session months later without the logs.
#[test]
fn every_attempt_is_recorded_with_its_count_and_its_timestamp() {
    let (dog, _) = silent_meeting(60);
    let log = dog.log();

    assert_eq!(log.count(), MAX_TAP_REBUILDS_PER_MEETING);
    assert!(log.budget_exhausted());
    assert_eq!(log.verdict(), Some(TapVerdict::GhostTapUnrecovered));

    for (index, attempt) in log.attempts().iter().enumerate() {
        assert_eq!(attempt.attempt, index as u32 + 1);
        assert!(attempt.at_ms > 0, "attempt {index} has no timestamp");
        assert!(
            attempt.host_ns > 0,
            "attempt {index} lost its host-time anchor, so it cannot be located in the audio"
        );
        assert!(attempt.silent_ms >= TAP_SILENCE_REBUILD_AFTER.as_millis() as u64);
        assert_eq!(attempt.kind, TapSilenceKind::AllZeroBuffers);
        assert_eq!(attempt.outcome, Some(TapRebuildOutcome::Succeeded));
    }

    // The JSON is the persisted shape: versioned, self-describing, and it names
    // the budget it was measured against so a later change to the budget cannot
    // make an old row unreadable.
    let json = log.to_json();
    assert_eq!(json["version"], 1);
    assert_eq!(json["budget"], MAX_TAP_REBUILDS_PER_MEETING);
    assert_eq!(json["count"], MAX_TAP_REBUILDS_PER_MEETING);
    assert_eq!(json["verdict"], "ghost_tap_unrecovered");
    let attempts = json["attempts"].as_array().expect("attempts is an array");
    assert_eq!(attempts.len(), 3);
    assert_eq!(attempts[0]["attempt"], 1);
    assert!(attempts[0]["at_ms"].as_u64().unwrap() > 0);
    assert_eq!(attempts[0]["kind"], "all_zero_buffers");
    assert_eq!(attempts[0]["outcome"], "succeeded");
    assert!(attempts[0]["causes"].as_array().unwrap().is_empty());

    // The column the blob lands in is named once, by the module that produces
    // it. YV106 adds it; nothing here invents a second name for it.
    assert_eq!(
        wilson_voice_lib::meetings::TAP_REBUILDS_COLUMN,
        wilson_voice_lib::syscapture::TAP_REBUILDS_COLUMN
    );
}

/// A meeting where nothing ever went wrong stores nothing. The common case must
/// not cost the row a blob.
#[test]
fn a_healthy_meeting_records_no_rebuilds_at_all() {
    let mut dog = GhostWatchdog::new();
    let healthy = TapLiveness {
        ever_nonzero: true,
        since_nonzero: Duration::from_millis(30),
        since_last_block: Duration::from_millis(21),
        frames_delivered: 48_000 * 10_800,
        lost_frames: 0,
    };
    for minute in 1..=180u32 {
        let action = dog.tick(TICK * minute, healthy, TapEnvironment::default(), 1);
        assert_eq!(action, TapWatchdogAction::Continue);
    }
    assert!(dog.log().is_empty());
    assert_eq!(dog.log().count(), 0);
    assert!(!dog.is_degraded());
}

/// A tap that recovers after one rebuild spends one attempt and keeps the other
/// two. The budget is per meeting, not per incident, and this is what makes the
/// stored count meaningful: "this session needed one rebuild" is a real fact
/// about the machine it ran on.
#[test]
fn a_tap_that_recovers_spends_one_attempt_and_keeps_the_rest() {
    let mut dog = GhostWatchdog::new();

    let action = dog.tick(
        Duration::from_secs(600),
        ghost(Duration::from_secs(45)),
        TapEnvironment::default(),
        4_242,
    );
    assert!(action.is_rebuild());
    dog.finish_rebuild(TapRebuildOutcome::Succeeded, Duration::from_secs(601));

    // The tap comes back. Every subsequent tick is quiet.
    let recovered = TapLiveness {
        ever_nonzero: true,
        since_nonzero: Duration::from_millis(25),
        since_last_block: Duration::from_millis(21),
        frames_delivered: 48_000 * 1_200,
        lost_frames: 0,
    };
    for minute in 11..=120u32 {
        assert_eq!(
            dog.tick(TICK * minute, recovered, TapEnvironment::default(), 5_000),
            TapWatchdogAction::Continue
        );
    }

    assert_eq!(dog.log().count(), 1);
    assert_eq!(dog.log().verdict(), None, "a recovered tap is not degraded");
    assert!(!dog.is_degraded());
    assert!(!dog.log().budget_exhausted());
}

/// A rebuild that FAILS still costs an attempt, and the step it failed at is on
/// the record. "The rebuild failed" is not a diagnosis;
/// "`AudioHardwareCreateProcessTap` returned -4" is.
#[test]
fn a_failed_rebuild_costs_an_attempt_and_records_where_it_broke() {
    let mut dog = GhostWatchdog::new();
    let action = dog.tick(
        Duration::from_secs(300),
        ghost(Duration::from_secs(120)),
        TapEnvironment::default(),
        77,
    );
    assert!(action.is_rebuild());
    dog.finish_rebuild(
        TapRebuildOutcome::Failed {
            at: TapStep::AudioHardwareCreateProcessTap,
            status: -4,
        },
        Duration::from_secs(302),
    );

    assert_eq!(dog.log().count(), 1);
    let json = dog.log().to_json();
    assert_eq!(json["attempts"][0]["outcome"], "failed");
    assert_eq!(
        json["attempts"][0]["failed_at"],
        "AudioHardwareCreateProcessTap"
    );
    assert_eq!(json["attempts"][0]["os_status"], -4);
}

/// A rebuild that is never reported back must not make the watchdog wait
/// forever. It times out, counts as the attempt it was, and the log says so —
/// so a caller that dies between two of the seven steps degrades the meeting
/// honestly instead of leaving Track B dead and unmentioned.
#[test]
fn a_rebuild_that_never_reports_back_times_out_instead_of_hanging_the_budget() {
    let mut dog = GhostWatchdog::new();
    let mut actions = Vec::new();
    // Ticks every 30 s so several land inside the in-flight window.
    for step in 1..=40u32 {
        let elapsed = Duration::from_secs(30) * step;
        actions.push(dog.tick(elapsed, ghost(elapsed), TapEnvironment::default(), 3));
        // The caller never reports back. Not once.
    }

    assert_eq!(
        actions.iter().filter(|a| a.is_rebuild()).count(),
        MAX_TAP_REBUILDS_PER_MEETING as usize,
        "the budget still holds when nobody reports back"
    );
    assert!(dog.is_degraded());

    // Attempts 1 and 2 were superseded by the timeout; attempt 3 was open when
    // the degrade closed it. All three are closed, none is left dangling.
    let log = dog.log();
    assert_eq!(log.count(), 3);
    assert!(
        log.attempts()
            .iter()
            .all(|a| a.outcome == Some(TapRebuildOutcome::TimedOut)),
        "every unreported attempt reads as timed out: {:?}",
        log.attempts()
    );
    assert_eq!(
        log.attempts()[1].kind,
        TapSilenceKind::PreviousRebuildTimedOut,
        "the second attempt fired because the first never came back, and says so"
    );

    // …and it did wait before giving up, rather than firing three rebuilds in
    // three consecutive ticks.
    let first = Duration::from_millis(log.attempts()[0].at_ms);
    let second = Duration::from_millis(log.attempts()[1].at_ms);
    assert!(second - first >= TAP_REBUILD_IN_FLIGHT_TIMEOUT);
}

/// The degrade carries the causes that were plausible at the time, so the
/// banner and the stored row can both stay honest about what else it might have
/// been.
#[test]
fn the_degrade_carries_the_other_plausible_causes_too() {
    let mut dog = GhostWatchdog::new();
    let env = TapEnvironment {
        system_output_active: Some(false),
        ..TapEnvironment::default()
    };
    let mut last = TapWatchdogAction::Continue;
    for minute in 1..=20u32 {
        let elapsed = TICK * minute;
        last = dog.tick(elapsed, ghost(elapsed), env, 12);
        if last.is_rebuild() {
            dog.finish_rebuild(
                TapRebuildOutcome::Succeeded,
                elapsed + Duration::from_secs(1),
            );
        }
        if last.is_degrade() {
            break;
        }
    }
    let TapWatchdogAction::DegradeTrackLost {
        verdict,
        after_rebuilds,
        causes,
    } = last
    else {
        panic!("expected a degrade, got {last:?}");
    };
    assert_eq!(verdict, TapVerdict::GhostTapUnrecovered);
    assert_eq!(after_rebuilds, MAX_TAP_REBUILDS_PER_MEETING);
    assert!(causes.contains(SilenceCause::EveryoneMuted));
    // …and the banner never says "permission" for a tap that delivered audio.
    assert!(!verdict.banner().to_lowercase().contains("permission"));
    assert!(verdict
        .banner()
        .contains("microphone track is still recording"));
}

/// Not an assertion — a **transcript**. `#[ignore]`d so it never costs CI a
/// second, and run by hand into `docs/pr-screenshots/YV104/` so the PR carries
/// what a human would actually see during OS-4's own reported failure: the
/// 51-minute M2 Air session that lost 60 s, 53 s, 141 s, 16 min 3 s and 3 min
/// 8 s of system audio.
///
/// The manual repro the backlog asks for needs a tap, and there is no tap at
/// this commit (YV100 is unmerged). This is the honest substitute: the same
/// silence pattern, driven through the shipping state machine, printing the log
/// lines and the stored blob it produces.
///
/// `cargo test --test syscapture_ghost_watchdog_rebuild_budget -- --ignored --nocapture`
#[test]
#[ignore]
fn transcript_of_the_51_minute_m2_air_session_from_thread_825780() {
    // (start second, length) of each reported hole, from the forum thread.
    let holes: [(u64, u64); 5] = [(300, 60), (600, 53), (900, 141), (1_500, 963), (2_700, 188)];
    let silent_at = |second: u64| -> Option<u64> {
        holes
            .iter()
            .find(|(start, len)| second > *start && second <= start + len)
            .map(|(start, _)| second - start)
    };

    let mut dog = GhostWatchdog::new();
    println!("second  since_nonzero  action");
    for second in 1..=3_060u64 {
        if second % 60 != 0 {
            continue;
        }
        let silent = silent_at(second).unwrap_or(0);
        let live = TapLiveness {
            ever_nonzero: true,
            since_nonzero: Duration::from_secs(silent),
            since_last_block: Duration::from_millis(21),
            frames_delivered: second * 48_000,
            lost_frames: 0,
        };
        let elapsed = Duration::from_secs(second);
        let action = dog.tick(elapsed, live, TapEnvironment::default(), 7_310 + second);
        if action != TapWatchdogAction::Continue {
            println!("{second:>6}  {silent:>13}  {action:?}");
        }
        if action.is_rebuild() {
            dog.finish_rebuild(
                TapRebuildOutcome::Succeeded,
                elapsed + Duration::from_secs(1),
            );
        }
    }
    println!();
    println!("stored on the meeting row as `{}`:", "tap_rebuilds");
    println!(
        "{}",
        serde_json::to_string_pretty(&dog.log().to_json()).unwrap()
    );
    println!();
    println!("degraded: {}", dog.is_degraded());
    if let Some(verdict) = dog.log().verdict() {
        println!("banner:   {}", verdict.banner());
    }
}
