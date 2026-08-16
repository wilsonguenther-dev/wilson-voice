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
///
/// 1 = YV94's meeting tables. 2 = YV95's `diagnostics` column (OS-12's thermal +
/// battery instrumentation), which is what makes the ladder worth having: the
/// second step is the first one that had to run against databases the first step
/// already wrote. 3 = YV106's two-track columns (`sys_wav_path`,
/// `tap_rebuilds`, `meeting_segments.track`). 4 = YV125's `meetings.kind`, the
/// column the diarization branch reads. 5 = YV130's segment-attribution columns
/// (`cluster_index`, `speaker_id`, `speaker_locked`, `embedding`) and the
/// `speaker_relabel_history` table one undo reads back.
const EXPECTED_VERSION: i64 = 5;

/// Put a database BACK on an older rung, columns and all, so the step under
/// test is a real upgrade of a real older database rather than a fresh install
/// that happened to walk past it.
///
/// Drops are in reverse ladder order, and each rung's drop list is exactly the
/// columns that rung's migration adds — which is what makes this rewind, and not
/// a second, divergent statement of the schema.
fn rewind_to(path: &std::path::Path, version: i64) {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut sql = String::from("BEGIN IMMEDIATE;\n");
    if version < 5 {
        sql.push_str(
            "DROP TABLE IF EXISTS speaker_relabel_history;\n\
             DROP INDEX IF EXISTS idx_segments_cluster;\n\
             DROP INDEX IF EXISTS idx_segments_speaker;\n\
             ALTER TABLE meeting_segments DROP COLUMN cluster_index;\n\
             ALTER TABLE meeting_segments DROP COLUMN speaker_id;\n\
             ALTER TABLE meeting_segments DROP COLUMN speaker_locked;\n\
             ALTER TABLE meeting_segments DROP COLUMN embedding;\n",
        );
    }
    if version < 4 {
        sql.push_str("ALTER TABLE meetings DROP COLUMN kind;\n");
    }
    if version < 3 {
        sql.push_str(
            "ALTER TABLE meetings DROP COLUMN sys_wav_path;\n\
             ALTER TABLE meetings DROP COLUMN tap_rebuilds;\n\
             ALTER TABLE meeting_segments DROP COLUMN track;\n",
        );
    }
    if version < 2 {
        sql.push_str("ALTER TABLE meetings DROP COLUMN diagnostics;\n");
    }
    sql.push_str(&format!("PRAGMA user_version = {version};\nCOMMIT;"));
    conn.execute_batch(&sql)
        .unwrap_or_else(|e| panic!("rewind to schema {version}: {e}"));
}

/// The columns on `meetings`, as SQLite reports them.
fn columns(path: &std::path::Path, table: &str) -> Vec<String> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap();
    let cols = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    cols
}

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

    // The columns NO phase has shipped yet: consent_ack (finding #13 — one
    // settings_kv key instead) and the diarization names still outstanding.
    //
    // `sys_wav_path` and `meeting_segments.track` USED to be on this list. They
    // are migration 3's now (YV106), which is the step this list was waiting
    // for, so they moved to
    // `migration_three_adds_the_two_track_columns_without_touching_existing_rows`
    // — asserted present rather than asserted absent.
    //
    // `speaker_id` and `cluster_index` moved the same way in YV130 (migration
    // 5), to
    // `migration_five_adds_the_correction_columns_without_touching_existing_rows`.
    // `speaker_label` and `overlapped` did NOT move and are still refused:
    // there is no denormalised label column (the label is
    // `speaker_id` → `speaker_profiles.display_name`), and YV127's whole
    // finding is that a per-segment `overlapped` flag is a column the
    // segmentation model cannot honestly fill.
    let path = dir.join("wilson_voice.db");
    let cols = columns(&path, "meetings");
    assert!(!cols.contains(&"consent_ack".to_string()), "{cols:?}");

    let cols = columns(&path, "meeting_segments");
    for banned in ["speaker_label", "overlapped"] {
        assert!(
            !cols.contains(&banned.to_string()),
            "{banned} is a column this schema deliberately does not have: {cols:?}"
        );
    }
}

/// YV95 — migration 2 adds ONE nullable column, and every row written before it
/// survives with a NULL there rather than being rewritten.
///
/// The interesting case is not a fresh install (which never sees step 2 as a
/// separate event) but an EXISTING database full of YV94 meetings: this walks
/// one up the ladder and checks the old rows are still exactly what they were.
#[test]
fn migration_two_adds_diagnostics_without_touching_existing_rows() {
    let dir = temp_dir("diagnostics-column");
    let path = dir.join("wilson_voice.db");

    // A database that stopped at version 1, with a meeting already in it.
    let id = {
        let db = open_db(&dir);
        let id = support::seed_meeting(&db, &dir, "Recorded before YV95", &["hello there"]);
        drop(db);
        rewind_to(&path, 1);
        id
    };

    let db = wilson_voice_lib::db::Database::open(path.clone()).expect("upgrade");
    assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);

    let m = db
        .get_meeting(&id)
        .unwrap()
        .expect("the old meeting survived");
    assert_eq!(m.title, "Recorded before YV95");
    assert_eq!(m.segment_count, 1, "its segments are untouched");
    assert!(
        m.diagnostics.is_none(),
        "a meeting recorded before the column existed reads as 'never measured'"
    );

    // The column is real, nullable, and writable.
    db.set_meeting_diagnostics(&id, r#"{"version":1,"pillVisible":true}"#)
        .unwrap();
    assert!(db
        .get_meeting(&id)
        .unwrap()
        .unwrap()
        .diagnostics
        .unwrap()
        .contains("pillVisible"));

    // Re-opening does not re-run the step.
    drop(db);
    let db = wilson_voice_lib::db::Database::open(path).unwrap();
    assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);
    assert!(db.get_meeting(&id).unwrap().unwrap().diagnostics.is_some());
}

// ── YV106 · migration 3 ──────────────────────────────────────────────────────

/// The step this item owns: three additive columns, applied to a database that
/// already has 22-A meetings in it, with nothing backfilled and nothing moved.
///
/// The interesting case is again the EXISTING database, not the fresh install:
/// `meeting_segments.track` is `NOT NULL DEFAULT 0`, and the whole argument for
/// that default is that every row already in the wild came from the mic. If that
/// is true, an upgraded row reads back as track 0 with no migration code having
/// touched it — and if it is not, this fails.
#[test]
fn migration_three_adds_the_two_track_columns_without_touching_existing_rows() {
    let dir = temp_dir("two-track-columns");
    let path = dir.join("wilson_voice.db");

    let id = {
        let db = open_db(&dir);
        let id = support::seed_meeting(&db, &dir, "Recorded before YV106", &["hello there"]);
        drop(db);
        rewind_to(&path, 2);
        id
    };

    let db = wilson_voice_lib::db::Database::open(path.clone()).expect("upgrade");
    assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);

    let m = db
        .get_meeting(&id)
        .unwrap()
        .expect("the old meeting survived");
    assert_eq!(m.title, "Recorded before YV106");
    assert_eq!(m.segment_count, 1, "its segments are untouched");
    assert!(
        m.mic_wav_path.is_some(),
        "the mic track it already had is still there"
    );
    assert!(
        m.sys_wav_path.is_none() && m.tap_rebuilds.is_none(),
        "a mic-only meeting has no second track and no rebuild log"
    );
    let segments = db.list_meeting_segments(&id).unwrap();
    assert_eq!(segments.len(), 1);
    assert_eq!(
        segments[0].track,
        wilson_voice_lib::meetings::MIC_TRACK,
        "every segment recorded before this migration IS a mic segment — that \
         is why the default needs no backfill"
    );

    // The columns are real, and both of the meeting-level ones are writable.
    let sys = dir.join("sys.wav");
    std::fs::write(&sys, b"RIFF....WAVEfake").unwrap();
    db.set_meeting_sys_wav_path(&id, Some(&sys)).unwrap();
    db.set_meeting_tap_rebuilds(&id, r#"{"version":1,"count":2}"#)
        .unwrap();
    let m = db.get_meeting(&id).unwrap().unwrap();
    assert_eq!(m.sys_wav_path.as_deref(), Some(sys.to_str().unwrap()));
    assert!(m.tap_rebuilds.unwrap().contains("\"count\":2"));

    // Re-opening does not re-run the step (an `ALTER TABLE … ADD COLUMN` that
    // ran twice would be an error, not a no-op — which is the whole reason the
    // ladder writes `user_version` inside the step's transaction).
    drop(db);
    let db = wilson_voice_lib::db::Database::open(path).unwrap();
    assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);
    assert!(db.get_meeting(&id).unwrap().unwrap().sys_wav_path.is_some());
}

/// A FRESH database reaches the current version in one pass, with every column
/// present — the install every new user gets, which never sees the steps as
/// separate events.
#[test]
fn a_fresh_database_lands_on_the_current_version_with_every_column_present() {
    let dir = temp_dir("fresh-three");
    let db = open_db(&dir);
    assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);
    drop(db);

    let path = dir.join("wilson_voice.db");
    let meetings = columns(&path, "meetings");
    for col in [
        "mic_wav_path",
        "diagnostics",
        "sys_wav_path",
        "tap_rebuilds",
        "kind",
    ] {
        assert!(meetings.contains(&col.to_string()), "{col}: {meetings:?}");
    }
    let segments = columns(&path, "meeting_segments");
    for col in [
        "track",
        "cluster_index",
        "speaker_id",
        "speaker_locked",
        "embedding",
    ] {
        assert!(segments.contains(&col.to_string()), "{col}: {segments:?}");
    }
}

// ── YV125 · migration 4 ──────────────────────────────────────────────────────

/// The step this item owns, against a database that already has meetings in it.
///
/// `kind` is `NOT NULL DEFAULT 'unknown'`, and unlike migration 3's `track`
/// default the value here is NOT a fact about the old rows — nobody was ever
/// asked. That is exactly why it is `unknown` and why no backfill exists:
/// `unknown` is the branch that CLUSTERS Track A, so a meeting whose kind was
/// never asked is never mistaken for one whose microphone held a single
/// speaker. Guessing `virtual` to preserve those rows' "Me" labels would be
/// inventing an answer on the one question finding #4 says was being answered
/// wrongly by assumption.
#[test]
fn migration_four_adds_kind_as_unknown_without_touching_existing_rows() {
    use wilson_voice_lib::meetings::{MeetingKind, DEFAULT_MEETING_KIND};

    let dir = temp_dir("kind-column");
    let path = dir.join("wilson_voice.db");

    let id = {
        let db = open_db(&dir);
        let id = support::seed_meeting(&db, &dir, "Recorded before YV125", &["hello there"]);
        drop(db);
        rewind_to(&path, 3);
        id
    };
    assert!(
        !columns(&path, "meetings").contains(&"kind".to_string()),
        "the rewind must actually remove the column, or this proves nothing"
    );

    let db = wilson_voice_lib::db::Database::open(path.clone()).expect("upgrade");
    assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);

    let m = db
        .get_meeting(&id)
        .unwrap()
        .expect("the old meeting survived");
    assert_eq!(m.title, "Recorded before YV125");
    assert_eq!(m.segment_count, 1, "its segments are untouched");
    assert!(m.mic_wav_path.is_some(), "its audio is still there");
    assert_eq!(
        m.kind, DEFAULT_MEETING_KIND,
        "a meeting recorded before the column existed was never asked"
    );
    assert_eq!(
        m.kind(),
        MeetingKind::Unknown,
        "…and `unknown` is the branch that clusters Track A rather than the \
         one that files the whole room as this user"
    );

    // The column is real and holds every value the picker can produce.
    for kind in [
        MeetingKind::Virtual,
        MeetingKind::InPerson,
        MeetingKind::Unknown,
    ] {
        let fresh = db
            .create_meeting_with_kind("Picked", "manual", kind)
            .unwrap();
        assert_eq!(db.get_meeting(&fresh.id).unwrap().unwrap().kind(), kind);
    }

    // Re-opening does not re-run the step (an `ALTER TABLE … ADD COLUMN` that
    // ran twice would be an error, not a no-op).
    drop(db);
    let db = wilson_voice_lib::db::Database::open(path).unwrap();
    assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);
    assert_eq!(
        db.get_meeting(&id).unwrap().unwrap().kind(),
        MeetingKind::Unknown
    );
}

/// The acceptance line in full: a synthetic VIRTUAL meeting — two recorded
/// tracks, transcript segments from both — round-trips through the tables,
/// keeps its two wav paths apart, and keeps its segments attributed.
///
/// This is the row shape YV107's merge reads and YV108 renders, proved at the
/// storage layer before either exists.
#[test]
fn a_two_track_meeting_round_trips_with_both_wavs_and_attributed_segments() {
    use wilson_voice_lib::meetings::{NewMeetingSegment, MIC_TRACK, SYSTEM_TRACK};

    let dir = temp_dir("virtual-meeting");
    let db = open_db(&dir);

    let meeting = db.create_meeting("Virtual standup", "manual").unwrap();
    let mic = dir.join("t0.wav");
    let sys = dir.join("t1.wav");
    std::fs::write(&mic, b"RIFF....WAVEfake").unwrap();
    std::fs::write(&sys, b"RIFF....WAVEfake").unwrap();

    db.append_meeting_segments(
        &meeting.id,
        &[
            NewMeetingSegment::new(0.0, 2.0, "morning everyone"),
            NewMeetingSegment::new(2.5, 5.0, "morning, can you hear me").on_track(SYSTEM_TRACK),
            NewMeetingSegment::new(5.5, 7.0, "loud and clear"),
        ],
    )
    .unwrap();
    db.finish_meeting(&meeting.id, 7.0, Some(&mic)).unwrap();
    db.set_meeting_sys_wav_path(&meeting.id, Some(&sys))
        .unwrap();

    let m = db.get_meeting(&meeting.id).unwrap().unwrap();
    assert_eq!(m.mic_wav_path.as_deref(), Some(mic.to_str().unwrap()));
    assert_eq!(m.sys_wav_path.as_deref(), Some(sys.to_str().unwrap()));
    assert!(m.audio_kept);

    let segments = db.list_meeting_segments(&meeting.id).unwrap();
    let tracks: Vec<i64> = segments.iter().map(|s| s.track).collect();
    assert_eq!(
        tracks,
        vec![MIC_TRACK, SYSTEM_TRACK, MIC_TRACK],
        "in start-time order, and each segment still knows which track it came \
         from — this is what makes Me/Them possible at all"
    );

    // Deleting the meeting hands back BOTH wavs to unlink. A delete that
    // returned only the mic track would leave the other people on the call on
    // disk after the user asked for the meeting to be gone.
    let removed = db.delete_meeting(&meeting.id).unwrap();
    assert!(
        removed.contains(&mic) && removed.contains(&sys),
        "{removed:?}"
    );

    // And so does the retention sweep, on the same rule.
    let db2 = {
        let dir2 = temp_dir("virtual-retention");
        let db2 = open_db(&dir2);
        let m2 = db2.create_meeting("Old virtual meeting", "manual").unwrap();
        let mic2 = dir2.join("t0.wav");
        let sys2 = dir2.join("t1.wav");
        std::fs::write(&mic2, b"x").unwrap();
        std::fs::write(&sys2, b"x").unwrap();
        db2.finish_meeting(&m2.id, 7.0, Some(&mic2)).unwrap();
        db2.set_meeting_sys_wav_path(&m2.id, Some(&sys2)).unwrap();
        let purged = db2
            .purge_meeting_audio(chrono::Utc::now() + chrono::Duration::days(1))
            .unwrap();
        assert!(
            purged.contains(&mic2) && purged.contains(&sys2),
            "both tracks age out together: {purged:?}"
        );
        let after = db2.get_meeting(&m2.id).unwrap().unwrap();
        assert!(after.mic_wav_path.is_none() && after.sys_wav_path.is_none());
        db2
    };
    drop(db2);
}

// ── YV130 · migration 5 ──────────────────────────────────────────────────────

/// The step this item owns, against a database that already has meetings in it.
///
/// Three of the four new columns are nullable with no default, and that is the
/// claim worth testing: a segment recorded before this build was never
/// clustered, never attributed and never embedded, and the schema has to be able
/// to SAY that rather than defaulting it to cluster 0 / speaker "" / a zero
/// vector — every one of which is a value the correction surface would then
/// treat as real. `speaker_locked` is the exception and takes `DEFAULT 0`
/// because "nobody has confirmed this segment" is a fact about every row that
/// existed before there was a way to confirm one.
#[test]
fn migration_five_adds_the_correction_columns_without_touching_existing_rows() {
    let dir = temp_dir("correction-columns");
    let path = dir.join("wilson_voice.db");

    let id = {
        let db = open_db(&dir);
        let id = support::seed_meeting(&db, &dir, "Recorded before YV130", &["hello there"]);
        drop(db);
        rewind_to(&path, 4);
        id
    };
    let before = columns(&path, "meeting_segments");
    for col in ["cluster_index", "speaker_id", "speaker_locked", "embedding"] {
        assert!(
            !before.contains(&col.to_string()),
            "the rewind must actually remove {col}, or this proves nothing: {before:?}"
        );
    }

    let db = wilson_voice_lib::db::Database::open(path.clone()).expect("upgrade");
    assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);

    let m = db
        .get_meeting(&id)
        .unwrap()
        .expect("the old meeting survived");
    assert_eq!(m.title, "Recorded before YV130");
    assert_eq!(m.segment_count, 1, "its segments are untouched");

    let segments = db.list_meeting_segments(&id).unwrap();
    assert_eq!(segments.len(), 1);
    let seg = &segments[0];
    assert_eq!(seg.text, "hello there", "the transcript itself is untouched");

    // Read the new columns raw: a pre-YV130 row is unclustered, unattributed,
    // unconfirmed and un-embedded, and every one of those is a NULL rather than
    // a stand-in value.
    let conn = rusqlite::Connection::open(&path).unwrap();
    let (cluster, speaker, locked, embedding): (
        Option<i64>,
        Option<String>,
        i64,
        Option<Vec<u8>>,
    ) = conn
        .query_row(
            "SELECT cluster_index, speaker_id, speaker_locked, embedding
             FROM meeting_segments WHERE id = ?1",
            rusqlite::params![seg.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert!(cluster.is_none(), "never clustered is not cluster 0");
    assert!(speaker.is_none(), "never attributed is not speaker ''");
    assert_eq!(locked, 0, "nobody confirmed a segment before YV130 existed");
    assert!(embedding.is_none(), "never embedded is not a zero vector");
    drop(conn);

    // The columns are real and writable through the accessor the clustering
    // pass will use.
    db.record_segment_clustering(&seg.id, 0, &[0.5, 0.25, 0.125])
        .unwrap();
    let members = db.cluster_members(&m.id, 0).unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(
        members[0].embedding.as_deref(),
        Some(&[0.5f32, 0.25, 0.125][..]),
        "the retained vector round-trips through the BLOB byte for byte"
    );

    // The history table exists, is empty, and is keyed the way one undo needs.
    let conn = rusqlite::Connection::open(&path).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM speaker_relabel_history", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(n, 0);
    drop(conn);

    // Re-opening does not re-run the step (an `ALTER TABLE … ADD COLUMN` that
    // ran twice would be an error, not a no-op).
    drop(db);
    let db = wilson_voice_lib::db::Database::open(path).unwrap();
    assert_eq!(db.schema_version().unwrap(), EXPECTED_VERSION);
    assert_eq!(db.cluster_members(&m.id, 0).unwrap().len(), 1);
}

/// The objects migration 5 claims to create are all there, by name.
#[test]
fn migration_five_creates_the_history_table_and_its_indexes() {
    let dir = temp_dir("correction-objects");
    let _db = open_db(&dir);

    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    for (kind, name) in [
        ("table", "speaker_relabel_history"),
        ("index", "idx_segments_cluster"),
        ("index", "idx_segments_speaker"),
        ("index", "idx_relabel_history_segment"),
    ] {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = ?1 AND name = ?2",
                rusqlite::params![kind, name],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "migration 5 must create {kind} {name}");
    }

    // There is no `speaker_profiles` foreign key, on purpose: that table is
    // YV128's and is not in this build, and a REFERENCES clause naming a table
    // SQLite cannot see fails at WRITE time rather than at migration time.
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE name = 'speaker_relabel_history'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !sql.contains("REFERENCES"),
        "migration 5 must not reference a table this build does not create: {sql}"
    );
}
