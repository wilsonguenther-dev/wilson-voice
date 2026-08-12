//! YV94 — "delete removes rows + wavs + FTS entries" (the plan's own line).
//!
//! A privacy feature that half-deletes is worse than none: the user believes the
//! meeting is gone while its words are still legible in an FTS5 shadow table and
//! its audio is still on disk. So this asserts all three, by name:
//!
//!   * zero rows in `meeting_segments`,
//!   * zero rows in the FTS shadow table (`meeting_segments_fts_docsize`, one
//!     row per indexed document — a MATCH miss could also just mean a bad query),
//!   * the WAV no longer exists on disk.

mod support;

use support::{fts_docsize_rows, open_db, seed_meeting, temp_dir};

fn count(dir: &std::path::Path, sql: &str) -> i64 {
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn delete_removes_rows_wavs_and_fts_entries() {
    let dir = temp_dir("cascade");
    let db = open_db(&dir);

    let doomed = seed_meeting(
        &db,
        &dir,
        "Confidential 1:1",
        &[
            "please keep this between us",
            "the salary number is not final",
            "we will revisit in the autumn",
        ],
    );
    let survivor = seed_meeting(&db, &dir, "Team sync", &["ship it on friday"]);

    let wav = db
        .get_meeting(&doomed)
        .unwrap()
        .unwrap()
        .mic_wav_path
        .expect("seeded meeting keeps its wav");
    let wav = std::path::PathBuf::from(wav);
    assert!(wav.is_file(), "precondition: the wav is on disk");
    assert_eq!(count(&dir, "SELECT COUNT(*) FROM meeting_segments"), 4);
    assert_eq!(fts_docsize_rows(&dir), 4);

    db.delete_meeting_with_audio(&doomed).unwrap();

    assert!(
        db.get_meeting(&doomed).unwrap().is_none(),
        "the row is gone"
    );
    assert_eq!(
        count(
            &dir,
            "SELECT COUNT(*) FROM meeting_segments WHERE meeting_id NOT IN (SELECT id FROM meetings)"
        ),
        0,
        "zero orphaned segments"
    );
    assert_eq!(
        count(&dir, "SELECT COUNT(*) FROM meeting_segments"),
        1,
        "only the survivor's segment remains"
    );
    assert_eq!(
        fts_docsize_rows(&dir),
        1,
        "zero FTS shadow rows for the deleted meeting"
    );
    assert!(
        db.list_meetings(50, Some("salary".into()))
            .unwrap()
            .is_empty(),
        "the words must not be findable afterwards"
    );
    assert!(!wav.exists(), "the wav must be gone from disk");

    // The neighbour is untouched — a delete that takes the whole table with it
    // would pass every assertion above.
    let alive = db.get_meeting(&survivor).unwrap().expect("survivor");
    assert_eq!(alive.segment_count, 1);
    assert_eq!(
        db.list_meetings(50, Some("friday".into())).unwrap().len(),
        1
    );
    assert!(std::path::Path::new(&alive.mic_wav_path.unwrap()).is_file());
}

/// The `ON DELETE CASCADE` net, exercised on its own: deleting only the parent
/// row (bypassing `delete_meeting`) must still leave no orphaned segments and no
/// stale index entries. This is what protects against a future code path that
/// deletes a meeting some other way.
#[test]
fn the_foreign_key_cascade_is_a_real_net() {
    let dir = temp_dir("cascade-fk");
    let db = open_db(&dir);
    let id = seed_meeting(&db, &dir, "Raw delete", &["alpha bravo", "charlie delta"]);
    drop(db);

    {
        let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute("DELETE FROM meetings WHERE id = ?1", rusqlite::params![id])
            .unwrap();
    }

    assert_eq!(count(&dir, "SELECT COUNT(*) FROM meeting_segments"), 0);
    assert_eq!(fts_docsize_rows(&dir), 0);
}

/// Deleting something that is already gone is a no-op, not an error — the UI
/// can double-fire a confirm without producing a scary toast.
#[test]
fn deleting_a_missing_meeting_is_a_no_op() {
    let dir = temp_dir("cascade-missing");
    let db = open_db(&dir);
    assert!(db.delete_meeting_with_audio("does-not-exist").is_ok());
    assert!(db.delete_meeting_with_audio("does-not-exist").is_ok());
}

/// A row pointing at a WAV that some other sweep already removed must still
/// delete cleanly — the rows are the part that must never survive.
#[test]
fn a_missing_wav_does_not_block_the_delete() {
    let dir = temp_dir("cascade-nowav");
    let db = open_db(&dir);
    let id = seed_meeting(&db, &dir, "Gone audio", &["something was said"]);
    let wav = db.get_meeting(&id).unwrap().unwrap().mic_wav_path.unwrap();
    std::fs::remove_file(&wav).unwrap();

    db.delete_meeting_with_audio(&id)
        .expect("delete still succeeds");
    assert!(db.get_meeting(&id).unwrap().is_none());
    assert_eq!(fts_docsize_rows(&dir), 0);
}
