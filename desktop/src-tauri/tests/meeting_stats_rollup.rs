//! YV94 — "a fresh meeting shows in the Insights strip" (the plan's own line,
//! finding #29).
//!
//! Yap has no telemetry by design, so this local rollup is the ONLY way Wilson
//! can tell whether the Notetaker is used, retained or trusted. The test reaches
//! it through `insights()` — the exact query the Insights screen renders — not
//! through a private helper, so a rollup that works but is never wired to the
//! screen still fails.

mod support;

use support::{open_db, seed_meeting, temp_dir};
use wilson_voice_lib::meetings::{MeetingState, AUDIO_RETENTION_DAYS};

#[test]
fn a_fresh_meeting_shows_up_in_the_insights_strip() {
    let dir = temp_dir("stats");
    let db = open_db(&dir);

    let empty = db.insights().unwrap().meetings;
    assert_eq!(empty.total_meetings, 0);
    assert_eq!(empty.segments_indexed, 0);
    assert!(empty.first_meeting_at.is_none());
    assert_eq!(
        empty.audio_retention_days, AUDIO_RETENTION_DAYS,
        "the UI must never hardcode the retention number"
    );

    let id = seed_meeting(&db, &dir, "First ever", &["one", "two", "three"]);

    let s = db.insights().unwrap().meetings;
    assert_eq!(s.total_meetings, 1);
    assert_eq!(s.meetings_last_7, 1);
    assert_eq!(s.segments_indexed, 3);
    assert_eq!(s.complete_meetings, 1);
    assert_eq!(s.partial_meetings, 0);
    assert_eq!(s.failed_meetings, 0);
    assert_eq!(s.meetings_with_audio, 1);
    assert!((s.total_seconds - 12.0).abs() < 1e-6, "{}", s.total_seconds);
    assert!(s.first_meeting_at.is_some());
    assert_eq!(s.first_meeting_at, s.last_meeting_at, "one meeting so far");

    // Reliability is a split, not a single number: a partial meeting must be
    // visible as partial rather than folded into "complete".
    let partial = seed_meeting(&db, &dir, "Half a meeting", &["only this landed"]);
    db.set_meeting_state(&partial, MeetingState::Partial, Some("chunk 3 timed out"))
        .unwrap();
    let failed = seed_meeting(&db, &dir, "Lost one", &["nothing usable"]);
    db.set_meeting_state(&failed, MeetingState::Failed, Some("mic disappeared"))
        .unwrap();

    let s = db.insights().unwrap().meetings;
    assert_eq!(s.total_meetings, 3);
    assert_eq!(s.complete_meetings, 1);
    assert_eq!(s.partial_meetings, 1);
    assert_eq!(s.failed_meetings, 1);
    assert_eq!(s.segments_indexed, 5);

    // Deleting a meeting takes it back out of the rollup — the strip is a live
    // query, not an append-only counter.
    db.delete_meeting_with_audio(&id).unwrap();
    let s = db.insights().unwrap().meetings;
    assert_eq!(s.total_meetings, 2);
    assert_eq!(s.complete_meetings, 0);
    assert_eq!(s.segments_indexed, 2);
}

/// Activation (finding #29's first metric): days from the user's first
/// dictation to their first meeting. `None` — not 0 — when there is nothing to
/// measure from, because 0 reads as "the same day".
#[test]
fn activation_is_none_until_both_ends_exist() {
    let dir = temp_dir("stats-activation");
    let db = open_db(&dir);

    seed_meeting(&db, &dir, "Meeting with no dictation history", &["hello"]);
    let s = db.meeting_stats().unwrap();
    assert!(
        s.days_to_first_meeting.is_none(),
        "no dictation to measure from"
    );

    // A dictation dated a week before the meeting.
    let then = chrono::Utc::now() - chrono::Duration::days(7);
    db.insert_transcript_at(
        "an older dictation".to_string(),
        "native".to_string(),
        0.4,
        0.9,
        0,
        None,
        then,
        None,
    )
    .unwrap();
    let s = db.meeting_stats().unwrap();
    assert_eq!(s.days_to_first_meeting, Some(7));
}

/// The rollup must not be able to blank the dictation numbers next to it: even
/// with no meeting tables' worth of data, `insights()` answers normally.
#[test]
fn insights_still_reports_dictation_when_there_are_no_meetings() {
    let dir = temp_dir("stats-mixed");
    let db = open_db(&dir);
    db.insert_transcript(
        "hello world from a dictation".to_string(),
        "native".to_string(),
        0.3,
        1.2,
        0,
        None,
    )
    .unwrap();

    let i = db.insights().unwrap();
    assert_eq!(i.total_sessions, 1);
    assert_eq!(i.meetings.total_meetings, 0);
}
