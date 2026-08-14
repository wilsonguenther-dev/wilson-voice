//! YV99 · error-matrix row 17 — "meeting exceeds the 3 h cap".
//!
//! Required behaviour: warn at 2 h 45 m, hard-stop at 3 h cleanly finalized,
//! with a continuation meeting linked to the first.
//!
//! **Read the split before reading the tests.** Row 17 is two claims with two
//! different truth values, so the matrix publishes it as two cells:
//!
//! * **The cap runs, and this file drives the code that runs it.**
//!   `meeting::MEETING_HARD_CAP`, `meeting::MEETING_CAP_WARN_AT` and
//!   `meeting::watchdog_tick` are the thresholds and the rule the 60 s watchdog
//!   reads. They are *imported* here, never re-declared: an earlier revision of
//!   this item shipped its own `MEETING_HARD_CAP` and its own warn/stop latch,
//!   which is a green test standing next to a published matrix row while the
//!   shipping cap drifts away from both. Row `17` is `Coverage::Test` naming
//!   `watchdog_tick` as its subject, and `matrix_coverage.rs` fails if this file
//!   ever stops touching it.
//! * **The continuation meeting does not exist**, and row `17b` says so. The
//!   watchdog stops the session at the cap and stops there — nothing starts the
//!   meeting that carries on, and `continuation_title` has no callers. The
//!   naming rule is decided and tested here as policy, published as
//!   `PolicyOnly, NOT WIRED`, and tripwired so the day something calls it the
//!   row has to be promoted.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use wilson_voice_lib::meeting::{
    watchdog_tick, StopReason, ThermalState, WatchdogAction, WatchdogInputs, DISK_FLOOR_BYTES,
    MEETING_CAP_WARN_AT, MEETING_HARD_CAP,
};
use wilson_voice_lib::meeting_matrix::{continuation_title, Coverage, ROWS};

fn src(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(file);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// A meeting that is entirely healthy except for how long it has been running.
fn healthy_at(elapsed: Duration, cap_warned: bool) -> WatchdogInputs {
    WatchdogInputs {
        elapsed,
        free_bytes: DISK_FLOOR_BYTES * 8,
        device_failed: false,
        since_last_block: Duration::from_millis(100),
        thermal: ThermalState::Nominal,
        cap_warned,
    }
}

/// The warning fires at the published threshold, and exactly once.
///
/// The "once" half is the part worth a test: the watchdog ticks every 60 s, so a
/// rule without the `cap_warned` latch would warn the user fifteen times in the
/// quarter of an hour before the cap.
#[test]
fn the_cap_warning_fires_at_2h45m_and_only_once() {
    let one_tick_early = MEETING_CAP_WARN_AT - Duration::from_secs(60);
    assert_eq!(
        watchdog_tick(&healthy_at(one_tick_early, false)),
        WatchdogAction::Continue,
        "nothing to say a minute before the warning threshold"
    );

    assert_eq!(
        watchdog_tick(&healthy_at(MEETING_CAP_WARN_AT, false)),
        WatchdogAction::WarnApproachingCap
    );

    // Every later tick, with the warning already delivered, must be silent.
    let mut elapsed = MEETING_CAP_WARN_AT;
    while elapsed < MEETING_HARD_CAP {
        assert_eq!(
            watchdog_tick(&healthy_at(elapsed, true)),
            WatchdogAction::Continue,
            "the warning repeated at {elapsed:?} — the latch is not being honoured"
        );
        elapsed += Duration::from_secs(60);
    }
}

/// The hard stop fires at the published cap, and stays fired.
#[test]
fn the_meeting_stops_at_the_three_hour_cap() {
    assert_eq!(
        watchdog_tick(&healthy_at(
            MEETING_HARD_CAP - Duration::from_secs(60),
            true
        )),
        WatchdogAction::Continue,
        "still recording a minute before the cap"
    );

    for elapsed in [
        MEETING_HARD_CAP,
        MEETING_HARD_CAP + Duration::from_secs(60),
        MEETING_HARD_CAP * 3,
    ] {
        assert_eq!(
            watchdog_tick(&healthy_at(elapsed, true)),
            WatchdogAction::Stop(StopReason::HardCap),
            "at {elapsed:?}"
        );
    }
}

/// The cap is three hours, and the warning is fifteen minutes before it.
///
/// Asserted against the imported constants rather than restated as literals:
/// this is the row's *published* behaviour, so the numbers in
/// `docs/yap22a-error-matrix.md` and the numbers the watchdog enforces have to
/// be the same numbers, and the only way to keep that true is to read them.
#[test]
fn the_published_thresholds_are_the_ones_the_watchdog_enforces() {
    assert_eq!(MEETING_HARD_CAP, Duration::from_secs(3 * 60 * 60));
    assert_eq!(
        MEETING_HARD_CAP - MEETING_CAP_WARN_AT,
        Duration::from_secs(15 * 60),
        "the warning must leave the user fifteen minutes to do something about it"
    );
}

/// A meeting that hits the cap while something worse is happening is stopped
/// for the worse thing.
///
/// Not pedantry: `StopReason` is what the user is shown and what the meeting row
/// records, so a device that died at 2 h 59 m must not be reported as a meeting
/// that simply ran long.
#[test]
fn a_dying_meeting_at_the_cap_is_stopped_for_the_reason_that_matters() {
    let mut dying = healthy_at(MEETING_HARD_CAP, true);
    dying.device_failed = true;
    assert_eq!(
        watchdog_tick(&dying),
        WatchdogAction::Stop(StopReason::DeviceFailed)
    );

    let mut starving = healthy_at(MEETING_HARD_CAP, true);
    starving.free_bytes = DISK_FLOOR_BYTES / 2;
    assert_eq!(
        watchdog_tick(&starving),
        WatchdogAction::Stop(StopReason::LowDisk {
            free_bytes: DISK_FLOOR_BYTES / 2
        })
    );
}

/// The duplication this row's own reviewer caught, kept caught.
///
/// Two definitions of a three-hour cap in one binary is not a redundancy, it is
/// a fork: the watchdog enforces one, the matrix test asserts the other, and
/// nothing makes them agree. `meeting_matrix` gets no cap of its own.
#[test]
fn the_cap_thresholds_are_not_re_declared_alongside_the_matrix() {
    let matrix = src("meeting_matrix.rs");
    for forbidden in [
        "const MEETING_HARD_CAP",
        "const MEETING_WARN_AT",
        "const MEETING_CAP_WARN_AT",
        "struct CapPolicy",
        "enum CapAction",
    ] {
        assert!(
            !matrix.contains(forbidden),
            "src/meeting_matrix.rs contains `{forbidden}` — row 17's cap lives in \
             `meeting.rs` and must have exactly one home. This file imports it and drives \
             `watchdog_tick`; a local copy would let the published row and the running rule \
             disagree with nothing to catch it."
        );
    }
}

/// The published cells: the cap is claimed as shipping behaviour, the
/// continuation is not.
#[test]
fn the_published_rows_separate_what_ships_from_what_does_not() {
    let cap = ROWS.iter().find(|r| r.id == "17").expect("row 17");
    assert_eq!(
        cap.coverage,
        Coverage::Test {
            test: "matrix_row17_meeting_cap.rs",
            subject: "watchdog_tick",
            subject_module: "meeting.rs",
        }
    );

    let carry_on = ROWS.iter().find(|r| r.id == "17b").expect("row 17b");
    let cell = carry_on.coverage.cell();
    assert!(cell.contains("NOT WIRED"), "{cell}");
    assert!(cell.contains("continuation_title"), "{cell}");
}

/// The continuation link, including the case a 9-hour session actually produces:
/// applying the rule to its own output.
#[test]
fn the_continuation_meeting_is_linked_and_never_stacks_its_own_suffix() {
    assert_eq!(continuation_title("Lecture"), "Lecture (continued)");
    assert_eq!(
        continuation_title("Lecture (continued)"),
        "Lecture (continued 2)"
    );
    assert_eq!(
        continuation_title("Lecture (continued 2)"),
        "Lecture (continued 3)"
    );
    assert_eq!(
        continuation_title("Lecture (continued 9)"),
        "Lecture (continued 10)"
    );

    // A title that merely ends in parentheses is not a continuation marker.
    assert_eq!(
        continuation_title("Standup (Tuesday)"),
        "Standup (Tuesday) (continued)"
    );
    // Trailing whitespace and an empty title do not produce ragged output.
    assert_eq!(continuation_title("Meeting  "), "Meeting (continued)");
    assert_eq!(continuation_title(""), "(continued)");
}

/// An all-day recording is three meetings, each linked to the last, and the
/// chain reads correctly all the way down. The stops that produce it are the
/// watchdog's; what this asserts is that the naming rule survives being applied
/// to its own output twice — and it is still policy, because nothing calls it.
#[test]
fn an_all_day_recording_is_three_linked_meetings() {
    let mut titles = vec!["Constitutional Law".to_string()];
    for _ in 0..2 {
        titles.push(continuation_title(titles.last().unwrap()));
    }

    assert_eq!(
        titles,
        vec![
            "Constitutional Law",
            "Constitutional Law (continued)",
            "Constitutional Law (continued 2)",
        ]
    );
}
