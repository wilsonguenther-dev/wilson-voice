//! #250 fix round 1 (B3) — a notes job never costs a partial transcript its
//! `partial` state. The job no longer writes `summarizing` over the meeting's
//! transcript state, and a legacy `summarizing` row is recovered to `partial`
//! when it carries a pipeline note, so the card keeps its Retry.

mod support;

use std::path::Path;

use support::{open_db, temp_dir};
use wilson_voice_lib::db::Database;
use wilson_voice_lib::meeting_control::reconcile_stranded_meetings;
use wilson_voice_lib::meeting_pipeline::SOME_WINDOWS_FAILED_MESSAGE;
use wilson_voice_lib::meetings::{MeetingState, NewMeetingSegment};
use wilson_voice_lib::summary_jobs::state_after_stale_summarizing;

fn meeting(db: &Database, dir: &Path, tag: &str, error: Option<&str>) -> String {
    let mic = dir.join(format!("{tag}.t0.wav"));
    std::fs::write(&mic, b"RIFF").unwrap();
    let m = db.create_meeting(tag, "manual").unwrap();
    db.finish_meeting(&m.id, 10.0, Some(&mic)).unwrap();
    db.append_meeting_segments(
        &m.id,
        &[NewMeetingSegment::new(0.0, 1.0, "words".to_string())],
    )
    .unwrap();
    // A crash (in an older build) left the row parked in `summarizing`.
    db.set_meeting_state(&m.id, MeetingState::Summarizing, error)
        .unwrap();
    m.id
}

#[test]
fn a_partial_meeting_left_summarizing_is_recovered_as_partial_not_complete() {
    let dir = temp_dir("r1-b3-reconcile");
    let db = open_db(&dir);
    let partial = meeting(&db, &dir, "partial", Some(SOME_WINDOWS_FAILED_MESSAGE));
    let whole = meeting(&db, &dir, "whole", None);
    let out = reconcile_stranded_meetings(&db, &|_| {});
    println!("reconciled: {out:?}");
    let p = db.get_meeting(&partial).unwrap().unwrap();
    assert_eq!(
        p.state, "partial",
        "a transcript with failed windows keeps Retry: {p:?}"
    );
    assert_eq!(
        p.error.as_deref(),
        Some(SOME_WINDOWS_FAILED_MESSAGE),
        "the note is kept"
    );
    let w = db.get_meeting(&whole).unwrap().unwrap();
    assert_eq!(w.state, "complete", "{w:?}");
}

#[test]
fn stale_summarizing_rows_go_back_to_partial_when_they_carry_a_note() {
    assert_eq!(
        state_after_stale_summarizing(Some(SOME_WINDOWS_FAILED_MESSAGE)),
        MeetingState::Partial
    );
    assert_eq!(
        state_after_stale_summarizing(Some("capture cut short")),
        MeetingState::Partial
    );
    assert_eq!(state_after_stale_summarizing(None), MeetingState::Complete);
    assert_eq!(
        state_after_stale_summarizing(Some("  ")),
        MeetingState::Complete
    );
}

/// The notes job itself never writes the transcript state: no
/// `MeetingState::Summarizing` and no `set_meeting_state` inside
/// `summarize_meeting_blocking`, and the launch reset never hard-codes
/// `complete`.
#[test]
fn the_notes_job_never_writes_over_the_transcript_state() {
    let lib = include_str!("../src/lib.rs");
    let start = lib
        .find("fn summarize_meeting_blocking(")
        .expect("the notes job body");
    let end = start
        + lib[start..]
            .find("\n}\n")
            .expect("end of the notes job body");
    let body = &lib[start..end];
    assert!(
        !body.contains("MeetingState::Summarizing"),
        "the job parks the row in summarizing"
    );
    assert!(
        !body.contains("set_meeting_state"),
        "the job writes the transcript state"
    );
    let start = lib
        .find("fn resume_pending_summaries(")
        .expect("the launch resume");
    let end = start
        + lib[start..]
            .find("\n}\n")
            .expect("end of the launch resume");
    assert!(
        !lib[start..end].contains("MeetingState::Complete"),
        "the launch reset hard-codes complete"
    );
}
