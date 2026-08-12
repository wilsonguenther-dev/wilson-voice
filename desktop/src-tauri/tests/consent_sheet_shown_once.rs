//! YV96 — "the sheet renders on the first meeting-record attempt and never
//! again after the `settings_kv` ack key is set" (the item's own acceptance
//! line), plus the half of the closed O1 decision that is easy to lose in a
//! later refactor: this notice is a **nudge, not a gate**.
//!
//! Both claims are claims about a file on disk, so — like the rest of the YV94
//! suite — every test here opens a REAL SQLite database and, for the once-ness
//! claim, closes and reopens it. An in-memory DB would pass a "shown once" test
//! that a relaunch immediately falsifies.

mod support;

use support::{open_db, temp_dir};
use wilson_voice_lib::meetings::CONSENT_NOTICE_KEY;

/// The whole feature: shown on the first record attempt, never again.
#[test]
fn consent_sheet_shown_once() {
    let dir = temp_dir("consent-once");
    let db = open_db(&dir);

    // First record attempt on a fresh install: the sheet renders.
    let before = db.meeting_consent();
    assert!(
        before.should_show,
        "a fresh install has never shown the notice"
    );
    assert!(before.acknowledged_at.is_none());

    // The user closes it — by acknowledging or by dismissing; a one-time notice
    // is about display, not assent, so both paths land here.
    let after = db.acknowledge_meeting_consent().unwrap();
    assert!(!after.should_show, "the notice is one-time");
    let first_seen = after
        .acknowledged_at
        .clone()
        .expect("the close is timestamped");

    // Every subsequent record attempt in this session: nothing renders.
    for attempt in 0..5 {
        assert!(
            !db.meeting_consent().should_show,
            "attempt {attempt} re-showed a one-time notice"
        );
    }

    // Closing it again (a manual re-read from Settings → Privacy) must not move
    // the date the user first saw this text.
    let again = db.acknowledge_meeting_consent().unwrap();
    assert_eq!(
        again.acknowledged_at.as_deref(),
        Some(first_seen.as_str()),
        "the first close wins; a later one cannot overwrite it"
    );

    // And it survives a relaunch, which is the only version of "once" that
    // means anything to a user.
    drop(db);
    let reopened = open_db(&dir);
    let persisted = reopened.meeting_consent();
    assert!(!persisted.should_show, "the ack did not survive a reopen");
    assert_eq!(
        persisted.acknowledged_at.as_deref(),
        Some(first_seen.as_str())
    );
}

/// The mechanism the backlog asks the PR to document: ONE row in the existing
/// `settings_kv` table, not a `meetings.consent_ack` column (finding #13).
#[test]
fn the_ack_is_one_settings_kv_row_and_no_meeting_column() {
    let dir = temp_dir("consent-mechanism");
    let db = open_db(&dir);
    db.acknowledge_meeting_consent().unwrap();

    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).expect("reopen");

    let rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM settings_kv WHERE key = ?1",
            [CONSENT_NOTICE_KEY],
            |r| r.get(0),
        )
        .expect("count ack rows");
    assert_eq!(
        rows, 1,
        "the acknowledgement is exactly one settings_kv row"
    );

    // Nothing else was written to the table: this item introduced one key, and
    // a future settings migration should have to notice if that changes.
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM settings_kv", [], |r| r.get(0))
        .expect("count settings");
    assert_eq!(total, 1);

    // The dropped column stays dropped. `consent_ack` on `meetings` had zero
    // readers by design; if a later migration adds it back, this fails loudly.
    let has_column: bool = conn
        .prepare("SELECT * FROM meetings")
        .expect("prepare")
        .column_names()
        .iter()
        .any(|c| *c == "consent_ack");
    assert!(
        !has_column,
        "finding #13: the per-meeting consent_ack column must not come back"
    );
}

/// The closed O1 decision, as a test: dismissing or acknowledging the sheet does
/// not block starting the recording.
#[test]
fn consent_is_a_nudge_not_a_gate() {
    let dir = temp_dir("consent-nudge");
    let db = open_db(&dir);

    // Un-acknowledged — the state a first-ever recording starts in.
    let pending = db.meeting_consent();
    assert!(pending.should_show);
    assert!(
        !pending.blocks_recording,
        "O1 closed as a nudge: the notice never gates capture"
    );

    // A meeting starts and finishes with the notice still un-acknowledged. This
    // is the load-bearing assertion: no consent check sits on the write path.
    let meeting = db
        .create_meeting("Recorded before the notice was ever closed", "manual")
        .expect("a meeting starts without an acknowledgement");
    db.finish_meeting(&meeting.id, 42.0, None)
        .expect("and it finishes");
    assert_eq!(db.list_meetings(10, None).unwrap().len(), 1);
    assert!(
        db.meeting_consent().should_show,
        "recording must not silently acknowledge the notice on the user's behalf"
    );

    // After the close, still not a gate — nothing about acknowledging changes
    // what capture is allowed to do.
    let closed = db.acknowledge_meeting_consent().unwrap();
    assert!(!closed.blocks_recording);
    let second = db
        .create_meeting("And one after", "manual")
        .expect("still starts");
    assert_ne!(second.id, meeting.id);
}
