//! YV125 — "a meeting started without touching the picker records
//! `kind = 'unknown'` and is NOT blocked from recording."
//!
//! The picker exists because nothing in this backlog can infer the kind: there
//! is no calendar (yap24), and the audio cannot say whether the people it
//! carries were in the room. So it is asked — and a question asked at the
//! moment somebody is trying to start a recording is a question that has to be
//! skippable, or it becomes a gate in front of the one button finding #6 says
//! must not be gated.
//!
//! Skipping is therefore a real answer with a real branch behind it
//! (`unknown` ⇒ cluster Track A, the general case), and this file holds both
//! halves of that open: the value that gets stored, and the fact that storing
//! it costs the user nothing.

mod support;

use std::sync::Arc;

use support::{open_db, temp_dir};
use wilson_voice_lib::meeting_control::{MeetingController, MeetingStatus, StatusSink};
use wilson_voice_lib::meetings::{MeetingKind, MeetingState, DEFAULT_MEETING_KIND};

fn silent_sink() -> StatusSink {
    Arc::new(|_: &MeetingStatus| {})
}

/// The storage layer's own default. `Database::create_meeting` is the call
/// every pre-YV125 caller still makes, and it must not have quietly acquired an
/// opinion.
#[test]
fn a_row_created_without_a_kind_is_unknown() {
    let dir = temp_dir("yv125-row-default");
    let db = open_db(&dir);

    let m = db.create_meeting("No kind was named", "manual").unwrap();
    assert_eq!(m.kind, DEFAULT_MEETING_KIND);
    assert_eq!(m.kind(), MeetingKind::Unknown);

    // And it survives the round trip as itself, rather than as an empty string
    // or a NULL the reader has to guess at.
    let stored = db.get_meeting(&m.id).unwrap().expect("exists");
    assert_eq!(stored.kind, DEFAULT_MEETING_KIND);
    assert_eq!(stored.kind(), MeetingKind::Unknown);

    // Every explicit kind round-trips too, so "unknown" is the DEFAULT and not
    // the only thing the column can hold.
    for kind in [
        MeetingKind::Virtual,
        MeetingKind::InPerson,
        MeetingKind::Unknown,
    ] {
        let m = db
            .create_meeting_with_kind("Named", "manual", kind)
            .unwrap();
        assert_eq!(db.get_meeting(&m.id).unwrap().unwrap().kind(), kind);
    }
}

/// The control plane's skip path: `start`/`toggle`, which is what ⌃⌘M and the
/// tray's primary item call.
#[test]
fn a_meeting_started_without_the_picker_records_unknown_and_still_records() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = temp_dir("yv125-skip");
    let db = Arc::new(open_db(&dir));
    let c = MeetingController::new(Arc::clone(&db), silent_sink());

    let status = c.toggle(&dir, None).expect("the picker is not a gate");
    assert!(
        status.recording,
        "skipping the kind question must never stop a recording from starting"
    );
    let id = status
        .id
        .clone()
        .expect("a recording status carries its id");

    let row = db
        .get_meeting(&id)
        .unwrap()
        .expect("the row exists at once");
    assert_eq!(row.state, MeetingState::Recording.as_str());
    assert_eq!(
        row.kind, DEFAULT_MEETING_KIND,
        "the answer nobody gave is `unknown`, written at INSERT rather than \
         patched afterwards"
    );
    assert_eq!(row.kind(), MeetingKind::Unknown);

    // The engine was handed the same value the row got. Two writes from one
    // decision, and a test that only checked the row would not notice them
    // coming apart.
    assert_eq!(
        support::fake_started_kind(),
        Some(MeetingKind::Unknown),
        "the capture session carries the kind too (SessionConfig::kind)"
    );

    c.stop("test").expect("stop");
}

/// The picked path, for the same two properties: the choice reaches the row AND
/// the engine, and choosing does not gate the recording either.
#[test]
fn a_picked_kind_reaches_both_the_row_and_the_capture_session() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = temp_dir("yv125-picked");
    let db = Arc::new(open_db(&dir));
    let c = MeetingController::new(Arc::clone(&db), silent_sink());

    for kind in [
        MeetingKind::InPerson,
        MeetingKind::Virtual,
        MeetingKind::Unknown,
    ] {
        let status = c
            .toggle_with_kind(&dir, None, kind)
            .expect("a picked kind starts a meeting");
        assert!(status.recording, "kind={} did not start", kind.as_str());
        let id = status.id.clone().expect("id");
        assert_eq!(
            db.get_meeting(&id).unwrap().unwrap().kind(),
            kind,
            "the row must hold what was picked"
        );
        assert_eq!(support::fake_started_kind(), Some(kind));
        c.stop("test").expect("stop");
    }
}

/// A meeting that is already running ignores a second press's kind — it stops.
///
/// The toggle is one control with two meanings, and the kind belongs to only
/// one of them: a press that ENDS a recording cannot retroactively change what
/// was in the room while it ran.
#[test]
fn a_kind_on_the_stopping_press_does_not_rewrite_the_running_meeting() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = temp_dir("yv125-stop-press");
    let db = Arc::new(open_db(&dir));
    let c = MeetingController::new(Arc::clone(&db), silent_sink());

    let started = c
        .toggle_with_kind(&dir, None, MeetingKind::InPerson)
        .expect("start");
    let id = started.id.clone().expect("id");

    let stopped = c
        .toggle_with_kind(&dir, None, MeetingKind::Virtual)
        .expect("stop");
    assert!(
        !stopped.recording,
        "the second press stops, it does not start"
    );
    assert_eq!(
        db.get_meeting(&id).unwrap().unwrap().kind(),
        MeetingKind::InPerson,
        "the meeting keeps the kind it was recorded under"
    );
}
