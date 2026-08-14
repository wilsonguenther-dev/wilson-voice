//! YV99 · error-matrix row 17 — "meeting exceeds the 3 h cap".
//!
//! Required behaviour: warn at 2 h 45 m, hard-stop at 3 h cleanly finalized,
//! with a continuation meeting linked to the first.
//!
//! **Read the split before reading the tests.** Row 17 has two halves and only
//! one of them belongs to YV99:
//!
//! * **The cap and the warning are not here.** `meeting::MEETING_HARD_CAP`,
//!   `meeting::MEETING_CAP_WARN_AT` and `meeting::watchdog_tick` in PR #108 are
//!   the thresholds and the rule that actually run — the 60 s watchdog reads
//!   them and latches its own `cap_warned`. An earlier revision of this file
//!   asserted YV99's own copy of those numbers, which is worse than not
//!   testing them: it is a green test standing next to a published matrix row,
//!   both of which would keep agreeing with each other while the shipping cap
//!   drifted away from both. So this file tests that the copy is *gone*, and
//!   `matrix_coverage.rs` tripwires the row so that when #108 lands, the tests
//!   below are replaced by ones that import #108's constants and drive
//!   `watchdog_tick` directly.
//! * **The continuation link is here**, because #108 stops the session and
//!   does not build the meeting that carries on. That rule is pure, has one
//!   genuinely awkward case (applying it to its own output), and is testable
//!   now.

use std::fs;
use std::path::PathBuf;

use wilson_voice_lib::meeting_matrix::continuation_title;

fn src(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(file);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
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
             `meeting.rs` (PR #108) and must have exactly one home. If #108 has merged, \
             this file should be importing `meeting::MEETING_HARD_CAP` and driving \
             `meeting::watchdog_tick`, not re-testing a local copy."
        );
    }
}

/// And the row does not quietly pretend otherwise: its coverage cell says
/// "policy only", names the call site that is missing, and names #108.
#[test]
fn the_published_row_says_the_cap_is_enforced_elsewhere() {
    use wilson_voice_lib::meeting_matrix::ROWS;
    let row = ROWS.iter().find(|r| r.id == "17").expect("row 17");
    let cell = row.coverage.cell();
    assert!(cell.contains("Policy only"), "{cell}");
    assert!(cell.contains("MEETING_HARD_CAP"), "{cell}");
    assert!(cell.contains("#108 (YV91)"), "{cell}");
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
/// chain reads correctly all the way down. The stops that produce it are
/// #108's; what this asserts is that the naming rule survives being applied to
/// its own output twice, which is the only part of the chain YV99 decides.
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
