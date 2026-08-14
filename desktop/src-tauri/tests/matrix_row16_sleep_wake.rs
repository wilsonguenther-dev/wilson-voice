//! YV99 · error-matrix row 16 — "sleep / lid close mid-meeting".
//!
//! Required behaviour: `NSWorkspaceWillSleepNotification` finalizes the journal
//! and marks the meeting `paused_by_sleep`; on wake, Yap OFFERS resume as a new
//! segment rather than pretending the gap did not happen.
//!
//! This row is why YV91's power assertion is not enough on its own:
//! `kIOPMAssertionTypePreventUserIdleSystemSleep` cannot stop a lid close or a
//! user-initiated sleep. The two are complements. The assertion buys the idle
//! case; this state machine has to be honest about the case it cannot buy.
//!
//! Everything below is a sequence of notifications fed to a pure state machine,
//! because the alternative instrument is closing a laptop lid by hand and the
//! interesting cases (a duplicate notification, a wake with no preceding sleep,
//! a sleep after the user already stopped) are ones a lid cannot produce on
//! demand.
//!
//! **What these tests do NOT prove, stated first so nobody has to infer it.**
//! Nothing in this repository registers `NSWorkspaceWillSleepNotification`, so
//! nothing calls [`SleepPolicy::observe`] and closing a lid mid-meeting still
//! does whatever it did before. No open PR owns that call site — #108 mentions
//! the notification in a comment in `power.rs` and registers nothing. That is
//! why row 16 is published as `Coverage::PolicyOnly` and not `Coverage::Test`,
//! and `the_row_is_published_as_unwired` below is the assertion that keeps the
//! published table honest while this stays true.

use wilson_voice_lib::meeting_matrix::{
    Coverage, SleepAction, SleepEvent, SleepPolicy, PAUSED_BY_SLEEP, ROWS,
};

/// The claim this whole file is allowed to make, checked rather than trusted.
///
/// A tested policy with no caller reads exactly like a shipped feature to
/// anyone skimming `docs/yap22a-error-matrix.md`. The state machine below is
/// complete and correct; the app does not consult it yet; the row has to say
/// so. `matrix_coverage.rs` holds the other end — it fails the day the call
/// site appears, which is the day this row becomes a real `Test` row.
#[test]
fn the_row_is_published_as_unwired_because_nothing_registers_the_notification() {
    let row = ROWS.iter().find(|r| r.id == "16").expect("row 16");
    assert!(
        matches!(
            row.coverage,
            Coverage::PolicyOnly {
                wiring_pr: None,
                absent_call_site: "NSWorkspaceWillSleepNotification",
                ..
            }
        ),
        "row 16 must not claim end-to-end coverage while no call site exists: {:?}",
        row.coverage
    );
    let cell = row.coverage.cell();
    assert!(cell.contains("NOT WIRED"), "{cell}");
    assert!(cell.contains("no open PR"), "{cell}");
}

fn recording() -> SleepPolicy {
    let mut p = SleepPolicy::new();
    p.meeting_started();
    p
}

#[test]
fn sleep_finalizes_and_marks_paused_by_sleep() {
    let mut p = recording();
    assert!(p.is_recording());

    assert_eq!(
        p.observe(SleepEvent::WillSleep),
        SleepAction::FinalizeAndPause
    );
    assert!(
        !p.is_recording(),
        "capture must stop before the machine goes down"
    );
    assert!(
        p.paused_by_sleep(),
        "the meeting row records why it has a seam"
    );
    assert_eq!(PAUSED_BY_SLEEP, "paused_by_sleep");
}

#[test]
fn wake_offers_resume_as_a_new_segment_never_a_pretended_continuity() {
    let mut p = recording();
    p.observe(SleepEvent::WillSleep);

    assert_eq!(
        p.observe(SleepEvent::DidWake),
        SleepAction::OfferResumeAsNewSegment,
        "the plan's own words: offer resume-as-new-segment, not a pretended-continuous gap"
    );
    assert!(p.awaiting_resume());
    assert!(
        !p.is_recording(),
        "waking must not silently resume capture — it is an offer, not a decision"
    );

    assert!(p.resume_accepted());
    assert!(p.is_recording());
    assert!(
        !p.paused_by_sleep(),
        "the seam is closed once the user resumes"
    );
}

#[test]
fn declining_the_offer_ends_the_meeting_cleanly() {
    let mut p = recording();
    p.observe(SleepEvent::WillSleep);
    p.observe(SleepEvent::DidWake);

    assert!(p.resume_declined());
    assert!(!p.is_recording());
    assert!(!p.awaiting_resume());
    // A second answer to a prompt that is gone changes nothing.
    assert!(!p.resume_accepted());
    assert!(!p.is_recording());
}

/// Dark wakes and repeated notifications are normal macOS behaviour. A second
/// prompt stacked on the first — or a second finalize of a journal that is
/// already closed — is the bug this asserts against.
#[test]
fn duplicate_notifications_are_idempotent() {
    let mut p = recording();

    assert_eq!(
        p.observe(SleepEvent::WillSleep),
        SleepAction::FinalizeAndPause
    );
    assert_eq!(
        p.observe(SleepEvent::WillSleep),
        SleepAction::Ignore,
        "the journal is already finalized; finalizing twice is a bug"
    );

    assert_eq!(
        p.observe(SleepEvent::DidWake),
        SleepAction::OfferResumeAsNewSegment
    );
    assert_eq!(
        p.observe(SleepEvent::DidWake),
        SleepAction::Ignore,
        "one outstanding prompt, however many wakes"
    );
    assert_eq!(
        p.observe(SleepEvent::WillSleep),
        SleepAction::Ignore,
        "sleeping again with a prompt outstanding must not stack a second one"
    );
}

/// The case that must not become a silent continuous recording: we woke still
/// believing we were capturing, so the will-sleep notification never arrived or
/// arrived too late to act on. There IS a gap in the audio either way.
#[test]
fn a_wake_with_no_preceding_sleep_never_pretends_the_gap_did_not_happen() {
    let mut p = recording();

    assert_eq!(
        p.observe(SleepEvent::DidWake),
        SleepAction::FinalizeMissedSleepAndOfferResume,
        "finalize what is on disk AND offer resume — never treat the two sides as continuous"
    );
    assert!(!p.is_recording());
    assert!(p.awaiting_resume());
}

#[test]
fn sleep_while_idle_does_nothing_at_all() {
    let mut p = SleepPolicy::new();
    assert_eq!(p.observe(SleepEvent::WillSleep), SleepAction::Ignore);
    assert_eq!(p.observe(SleepEvent::DidWake), SleepAction::Ignore);
    assert!(!p.paused_by_sleep());
    assert!(!p.awaiting_resume());
}

/// A meeting the user already stopped must never come back from a resume
/// prompt — that would be the app arguing with them.
#[test]
fn a_user_stop_cancels_any_pending_resume() {
    let mut p = recording();
    p.observe(SleepEvent::WillSleep);
    p.observe(SleepEvent::DidWake);
    assert!(p.awaiting_resume());

    p.meeting_stopped();
    assert!(!p.awaiting_resume());
    assert!(!p.resume_accepted());
    assert_eq!(p.observe(SleepEvent::DidWake), SleepAction::Ignore);
}

/// The full lid-close arc, as one sequence, because the row is about the arc and
/// not about any single transition: record → close the lid → open it → resume →
/// close it again → open it again → stop.
#[test]
fn two_lid_closes_in_one_meeting_produce_two_offers_and_no_lost_state() {
    let mut p = recording();

    for round in 1..=2 {
        assert_eq!(
            p.observe(SleepEvent::WillSleep),
            SleepAction::FinalizeAndPause,
            "round {round}"
        );
        assert_eq!(
            p.observe(SleepEvent::DidWake),
            SleepAction::OfferResumeAsNewSegment,
            "round {round}"
        );
        assert!(p.resume_accepted(), "round {round}");
        assert!(p.is_recording(), "round {round}");
    }

    p.meeting_stopped();
    assert!(!p.is_recording());
}
