//! #250 fix round 1 (B3) — a notes job never costs a partial transcript its
//! `partial` state. The job no longer writes `summarizing` over the meeting's
//! transcript state, and a legacy `summarizing` row is recovered to `partial`
//! when its resume ledger records a failed window, so the card keeps its Retry.
//!
//! #250 fix round 1 (independent B1): every SHIPPED writer of `summarizing`
//! called `set_meeting_state(id, Summarizing, None)`, which clears `error`, so
//! the rows these tests build carry `error = NULL` — the shape a real upgrade
//! meets — and the failed window lives where the pipeline put it: the
//! per-track ledger `<id>.t<N>.asr-progress.json` beside the audio.

mod support;

use std::path::Path;

use support::{open_db, temp_dir};
use wilson_voice_lib::db::Database;
use wilson_voice_lib::meeting_asr::{
    BoundaryKind, ChunkOutcome, ChunkWindow, JsonProgressStore, ProgressStore,
};
use wilson_voice_lib::meeting_control::reconcile_stranded_meetings;
use wilson_voice_lib::meeting_pipeline::{
    ledger_key, ledger_records_failed_window, SOME_WINDOWS_FAILED_MESSAGE,
};
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

fn window(index: usize) -> ChunkWindow {
    ChunkWindow {
        index,
        audio_start_seconds: index as f64,
        audio_end_seconds: index as f64 + 1.0,
        content_start_seconds: index as f64,
        content_end_seconds: index as f64 + 1.0,
        start_boundary: BoundaryKind::default(),
        end_boundary: BoundaryKind::default(),
    }
}

/// What the transcription pipeline left on disk for one track: one decoded
/// window and, when `failed`, one `asr_failed` window.
fn ledger(dir: &Path, id: &str, track: i64, failed: bool) {
    let mut store = JsonProgressStore::new(dir);
    let key = ledger_key(id, track);
    let done = ChunkOutcome {
        status: wilson_voice_lib::meeting_asr::ChunkStatus::Done,
        error: None,
        ..ChunkOutcome::failed(&window(0), String::new())
    };
    store.record_chunk(&key, &done).unwrap();
    if failed {
        store
            .record_chunk(
                &key,
                &ChunkOutcome::failed(&window(1), "decode timed out".into()),
            )
            .unwrap();
    }
}

#[test]
fn a_partial_meeting_left_summarizing_is_recovered_as_partial_not_complete() {
    let dir = temp_dir("r1-b3-reconcile");
    let db = open_db(&dir);
    // The shape every shipped build left: `summarizing`, `error = NULL`.
    let partial = meeting(&db, &dir, "partial", None);
    ledger(&dir, &partial, 1, true);
    let whole = meeting(&db, &dir, "whole", None);
    ledger(&dir, &whole, 0, false);
    let out = reconcile_stranded_meetings(&db, &dir, &|_| {});
    println!("reconciled: {out:?}");
    let p = db.get_meeting(&partial).unwrap().unwrap();
    assert_eq!(
        p.state, "partial",
        "a transcript with failed windows keeps Retry: {p:?}"
    );
    assert_eq!(
        p.error.as_deref(),
        Some(SOME_WINDOWS_FAILED_MESSAGE),
        "the pipeline's note is restored"
    );
    let w = db.get_meeting(&whole).unwrap().unwrap();
    assert_eq!(w.state, "complete", "{w:?}");
    assert_eq!(w.error, None, "{w:?}");
}

#[test]
fn the_failed_window_signal_reads_the_pipelines_own_ledger() {
    let dir = temp_dir("r1-b1-ledger");
    assert!(
        !ledger_records_failed_window(&dir, "absent"),
        "no ledger, no claim"
    );
    ledger(&dir, "clean", 0, false);
    assert!(!ledger_records_failed_window(&dir, "clean"));
    ledger(&dir, "mic-hole", 0, true);
    assert!(ledger_records_failed_window(&dir, "mic-hole"));
    ledger(&dir, "sys-hole", 0, false);
    ledger(&dir, "sys-hole", 1, true);
    assert!(
        ledger_records_failed_window(&dir, "sys-hole"),
        "either track counts"
    );
    std::fs::write(dir.join("junk.t0.asr-progress.json"), b"{not json").unwrap();
    assert!(
        !ledger_records_failed_window(&dir, "junk"),
        "unreadable is no claim"
    );
}

#[test]
fn stale_summarizing_rows_go_back_to_partial_when_a_window_failed() {
    // The real legacy row: no note, a failed window in the ledger.
    assert_eq!(
        state_after_stale_summarizing(None, true),
        (
            MeetingState::Partial,
            Some(SOME_WINDOWS_FAILED_MESSAGE.to_string())
        )
    );
    // A note that survived is kept as it is.
    assert_eq!(
        state_after_stale_summarizing(Some("capture cut short"), false),
        (MeetingState::Partial, Some("capture cut short".to_string()))
    );
    assert_eq!(
        state_after_stale_summarizing(None, false),
        (MeetingState::Complete, None)
    );
    assert_eq!(
        state_after_stale_summarizing(Some("  "), false).0,
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
