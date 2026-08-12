//! YV94 — the migration ladder.
//!
//! The plan's own stated acceptance line for this item is "opening the DB twice
//! is a no-op". That is worth a test because of what `db.rs` did before
//! (finding #26): schema changes were `CREATE TABLE IF NOT EXISTS` plus
//! `let _ = conn.execute("ALTER TABLE …")`, which discards the error, so a
//! migration that failed was indistinguishable from one that succeeded and
//! nothing recorded which had run. `PRAGMA user_version` arrives with the
//! meeting tables as migration 1, before any meeting row exists in the wild.

mod support;

use support::{open_db, temp_dir};

/// The version this build's meeting schema lives at.
const EXPECTED_VERSION: i64 = 1;

#[test]
fn opening_the_db_twice_is_a_no_op() {
    let dir = temp_dir("migrate");

    let db = open_db(&dir);
    assert_eq!(
        db.schema_version().unwrap(),
        EXPECTED_VERSION,
        "a fresh DB must land on the current schema version"
    );
    // Write something the ladder must not disturb.
    let id = support::seed_meeting(&db, &dir, "First meeting", &["hello there"]);
    drop(db);

    // Second open: the ladder sees version 1 and does nothing.
    let db = open_db(&dir);
    assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);
    assert_eq!(
        db.list_meetings(50, None).unwrap().len(),
        1,
        "reopening must not recreate, wipe or duplicate the meeting tables"
    );
    assert_eq!(db.get_meeting(&id).unwrap().unwrap().segment_count, 1);
    drop(db);

    // And a third, because "idempotent" that only holds twice is a coincidence.
    let db = open_db(&dir);
    assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);
    assert_eq!(db.list_meetings(50, None).unwrap().len(), 1);
}

/// A DB written before the ladder existed (user_version 0, no meeting tables,
/// rows already in it) must climb to 1 without losing anything. This is the
/// upgrade every shipped v0.8.0 install performs.
#[test]
fn a_pre_ladder_database_upgrades_without_losing_rows() {
    let dir = temp_dir("upgrade");
    let path = dir.join("wilson_voice.db");

    {
        // Stand up the v0.8.0 baseline by hand: the transcripts table with one
        // row, no user_version, no meeting tables.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE transcripts (
               id TEXT PRIMARY KEY, text TEXT NOT NULL,
               backend TEXT NOT NULL DEFAULT 'native',
               asr_seconds REAL NOT NULL DEFAULT 0,
               word_count INTEGER NOT NULL DEFAULT 0,
               source_app TEXT, created_at TEXT NOT NULL);
             INSERT INTO transcripts (id, text, word_count, created_at)
               VALUES ('t1', 'words from before the ladder', 5, '2026-01-01T00:00:00+00:00');",
        )
        .unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 0, "the pre-ladder baseline is version 0 by definition");
    }

    let db = wilson_voice_lib::db::Database::open(path.clone()).expect("open upgrades in place");
    assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);
    assert_eq!(
        db.list_transcripts(10, None).unwrap().len(),
        1,
        "the upgrade must not touch existing rows"
    );
    // The meeting tables now exist and are usable.
    let id = support::seed_meeting(&db, &dir, "After the upgrade", &["first meeting ever"]);
    assert!(db.get_meeting(&id).unwrap().is_some());
}

/// A DB stamped with a FUTURE version (a newer build ran, the user downgraded)
/// must open read-write and be left alone, not "migrated" backwards.
#[test]
fn a_newer_database_is_left_alone() {
    let dir = temp_dir("newer");
    let path = dir.join("wilson_voice.db");

    {
        let db = wilson_voice_lib::db::Database::open(path.clone()).unwrap();
        assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA user_version = 99;").unwrap();
    }

    let db = wilson_voice_lib::db::Database::open(path).expect("a newer DB still opens");
    assert_eq!(
        db.schema_version().unwrap(),
        99,
        "the version must not be rewritten downwards"
    );
    assert!(db.list_meetings(10, None).is_ok());
}

/// The tables, indexes and triggers migration 1 claims to create are all there.
#[test]
fn migration_one_creates_the_whole_meeting_schema() {
    let dir = temp_dir("objects");
    let _db = open_db(&dir);

    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    for (kind, name) in [
        ("table", "meetings"),
        ("table", "meeting_segments"),
        ("index", "idx_meetings_started"),
        ("index", "idx_segments_meeting"),
        ("table", "meeting_segments_fts"), // fts5 virtual tables are type 'table'
        ("trigger", "meeting_segments_ai"),
        ("trigger", "meeting_segments_ad"),
        ("trigger", "meeting_segments_au"),
    ] {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = ?1 AND name = ?2",
                rusqlite::params![kind, name],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "migration 1 must create {kind} {name}");
    }

    // The columns 22-A deliberately does NOT carry: consent_ack (finding #13 —
    // one settings_kv key instead), the diarization columns (yap23), and the
    // 22-B two-track columns. A future item ADDS them as migration 2; if one
    // shows up here, someone shipped it early and this is the tripwire.
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info('meetings')")
        .unwrap();
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert!(!cols.contains(&"consent_ack".to_string()), "{cols:?}");
    assert!(!cols.contains(&"sys_wav_path".to_string()), "{cols:?}");

    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info('meeting_segments')")
        .unwrap();
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    for banned in [
        "speaker_id",
        "speaker_label",
        "cluster_index",
        "overlapped",
        "track",
    ] {
        assert!(
            !cols.contains(&banned.to_string()),
            "{banned} is a later phase's column, not 22-A's: {cols:?}"
        );
    }
}
