//! YV102 acceptance — **two keys, two facts, no crosstalk.**
//!
//! The item's own acceptance line: *asserts the new setup-acknowledgement key
//! round-trips independently of `CONSENT_NOTICE_KEY` — acking one does not ack
//! the other.*
//!
//! The temptation this test exists to kill is real and cheap-looking: both rows
//! are "the user has dealt with the meeting-recording thing", both are one
//! `settings_kv` row, so why not one key? Because they are facts about
//! different mechanisms with different failure modes. Collapse them and you get
//! one of two bugs, neither recoverable from a single boolean:
//!
//! * a user who read the consent notice never gets the TCC pre-warm — so the
//!   system alert ambushes them at T-0 of their first Zoom join, which is the
//!   entire thing YV102 exists to prevent, and a dismissal there is terminal
//!   because TCC does not ask twice; or
//! * a user who ran the pre-warm never sees the notice, which is the one piece
//!   of copy standing between Yap and a recording-consent problem.
//!
//! Like the rest of the YV94/YV96 suite these open a REAL SQLite file and, for
//! the persistence claims, close and reopen it. An in-memory database passes
//! "it round-trips" tests that a relaunch immediately falsifies.

mod support;

use support::{open_db, temp_dir};
use wilson_voice_lib::meetings::{
    SetupVerdict, SystemAudioSetup, CONSENT_NOTICE_KEY, SYSTEM_AUDIO_SETUP_ACK_KEY,
};

/// The headline: the two keys are independent in both directions.
#[test]
fn the_setup_key_and_the_consent_key_do_not_ack_each_other() {
    let dir = temp_dir("settings-kv-independence");
    let db = open_db(&dir);

    // A fresh install: neither fact is true.
    assert!(db.meeting_consent().should_show, "notice not yet shown");
    assert!(!db.system_audio_setup().has_run, "setup step not yet run");

    // Acking the consent notice must not claim the pre-warm ran.
    db.acknowledge_meeting_consent().unwrap();
    assert!(!db.meeting_consent().should_show);
    assert!(
        !db.system_audio_setup().has_run,
        "acknowledging the notice silently claimed the TCC pre-warm had run"
    );
    assert_eq!(db.system_audio_setup().verdict, SetupVerdict::NotRun);

    // And the reverse, from a clean database, so neither ordering hides the
    // other's bug.
    let dir = temp_dir("settings-kv-independence-reverse");
    let db = open_db(&dir);
    db.record_system_audio_setup(SetupVerdict::Ran).unwrap();
    assert!(db.system_audio_setup().has_run);
    assert!(
        db.meeting_consent().should_show,
        "running the pre-warm silently acknowledged a legal notice nobody read"
    );
    assert!(db.meeting_consent().acknowledged_at.is_none());
}

/// The key names themselves. Both versioned, both distinct — a copy-paste that
/// left them equal would make every test above pass for the wrong reason only
/// if this one did not exist.
#[test]
fn the_two_keys_are_different_versioned_names() {
    assert_ne!(SYSTEM_AUDIO_SETUP_ACK_KEY, CONSENT_NOTICE_KEY);
    assert_eq!(
        SYSTEM_AUDIO_SETUP_ACK_KEY,
        "meeting_system_audio_setup_ack_v1"
    );
    assert!(
        SYSTEM_AUDIO_SETUP_ACK_KEY.ends_with("_v1"),
        "versioned, so materially new behaviour can re-run the step"
    );
}

/// One row, in the table that already exists — not a new column on `meetings`,
/// which every future meeting would carry and nothing would read (finding #13's
/// lesson, applied a second time).
#[test]
fn the_setup_state_is_exactly_one_settings_kv_row() {
    let dir = temp_dir("settings-kv-one-row");
    let db = open_db(&dir);
    db.record_system_audio_setup(SetupVerdict::Ran).unwrap();
    db.record_system_audio_setup(SetupVerdict::Granted).unwrap();
    db.record_system_audio_setup(SetupVerdict::LooksDenied)
        .unwrap();

    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).expect("reopen");
    let rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM settings_kv WHERE key = ?1",
            [SYSTEM_AUDIO_SETUP_ACK_KEY],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1, "three runs, one row");

    let columns: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('meetings') \
             WHERE name LIKE '%system_audio%' OR name LIKE '%setup%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(columns, 0, "no per-meeting column for an app-wide fact");
}

/// The LAST run wins, unlike the consent notice where the FIRST close wins.
///
/// Different questions: "when did this user first see this text" cannot change,
/// but "what does Yap currently believe about this Mac's permission" can — a
/// user who allows the tap after a denial must not be stuck looking at the
/// denied banner forever.
#[test]
fn the_latest_verdict_wins_and_survives_a_relaunch() {
    let dir = temp_dir("settings-kv-latest-wins");
    let db = open_db(&dir);

    db.record_system_audio_setup(SetupVerdict::LooksDenied)
        .unwrap();
    let denied = db.system_audio_setup();
    assert!(denied.looks_denied());
    let denied_at = denied.last_run_at.clone().expect("the run is timestamped");

    let fixed = db.record_system_audio_setup(SetupVerdict::Granted).unwrap();
    assert_eq!(fixed.verdict, SetupVerdict::Granted);
    assert!(
        !fixed.looks_denied(),
        "a user who fixed the permission is not still denied"
    );
    assert_ne!(
        fixed.last_run_at.as_deref(),
        Some(denied_at.as_str()),
        "the timestamp moved with the verdict"
    );

    drop(db);
    let reopened = open_db(&dir);
    let persisted = reopened.system_audio_setup();
    assert!(persisted.has_run);
    assert_eq!(persisted.verdict, SetupVerdict::Granted);
    assert_eq!(persisted.last_run_at, fixed.last_run_at);
}

/// Every verdict round-trips through the row encoding unchanged.
#[test]
fn every_verdict_round_trips() {
    let dir = temp_dir("settings-kv-verdicts");
    let db = open_db(&dir);
    for verdict in [
        SetupVerdict::Ran,
        SetupVerdict::Granted,
        SetupVerdict::LooksDenied,
        SetupVerdict::Unavailable,
        SetupVerdict::Failed,
    ] {
        let state = db.record_system_audio_setup(verdict).unwrap();
        assert_eq!(state.verdict, verdict);
        assert_eq!(db.system_audio_setup().verdict, verdict);
        assert!(state.has_run);
        assert!(state.last_run_at.is_some());
    }
}

/// A row this build cannot parse degrades to "it ran", never to a denial.
///
/// A wrong `LooksDenied` sends the user to System Settings to fix something
/// that is not broken, and — worse — teaches them to ignore the banner. A wrong
/// `Ran` costs one unnecessary press of a button that is idempotent anyway.
#[test]
fn an_unparseable_row_never_becomes_a_denial() {
    for row in [
        "",
        "   ",
        "some_verdict_from_the_future 2026-08-14T00:00:00Z",
        "granted",       // no timestamp
        "2026-08-14T00:00:00Z", // an older encoding: bare timestamp, no verdict
    ] {
        let state = SystemAudioSetup::from_row(Some(row.to_string()));
        assert!(state.has_run, "{row:?}");
        assert!(
            !state.looks_denied(),
            "{row:?} was read as a denial Yap cannot substantiate"
        );
    }

    // No row at all is the only "never run" state.
    let none = SystemAudioSetup::from_row(None);
    assert!(!none.has_run);
    assert_eq!(none.verdict, SetupVerdict::NotRun);
    assert!(none.last_run_at.is_none());
}

/// The nudge-not-gate decision (O1, finding #13) applies to this surface too:
/// mic-only meeting recording runs on the macOS 12 floor and is never gated on
/// a system-audio permission. The field is a value the UI reads rather than a
/// convention a future entry point can forget.
#[test]
fn the_setup_step_never_blocks_a_recording() {
    let dir = temp_dir("settings-kv-never-blocks");
    let db = open_db(&dir);
    assert!(!db.system_audio_setup().blocks_recording);
    for verdict in [
        SetupVerdict::Ran,
        SetupVerdict::LooksDenied,
        SetupVerdict::Unavailable,
        SetupVerdict::Failed,
    ] {
        let state = db.record_system_audio_setup(verdict).unwrap();
        assert!(
            !state.blocks_recording,
            "{verdict:?} must not stand in front of mic-only recording"
        );
    }
    // The consent notice's own promise is unchanged by any of it.
    assert!(!db.meeting_consent().blocks_recording);
}
