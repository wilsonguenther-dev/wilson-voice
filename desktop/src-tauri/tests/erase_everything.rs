//! PRIV-B — Clear History erases the audio and the partial words, not only the
//! SQLite rows.
//!
//! These tests assert on the FILESYSTEM, never on the query layer: a row that
//! is merely unqueryable, or a WAV the app has merely forgotten the path of,
//! is still a user's voice on disk. Each test plants a sentinel, runs the real
//! erase the `clear_history` / `delete_all_recordings` commands run, and then
//! looks at the bytes.
//!
//! Everything runs under `YAP_DATA_DIR` (Y0-D) pointed at a scratch root, so
//! no test ever reads or writes the real Application Support data dir. The
//! first assertion in every test is that the resolved paths really ARE under
//! the scratch root.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use wilson_voice_lib::{app_paths, db::Database, erase, meetings_dir, recovery_dir};

/// The environment is process-global; one test at a time.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A scratch state root exported as `YAP_DATA_DIR` exactly once — the LITERAL
/// variable name, so renaming the override breaks this suite instead of
/// letting it follow the rename into the real data dir.
fn scratch_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("yap-priv-b-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create scratch root");
        std::env::set_var("YAP_DATA_DIR", &root);
        root
    })
}

/// Paths resolved through the real override, proven to be under the scratch
/// root before anything is written.
fn paths() -> &'static app_paths::AppPaths {
    let root = scratch_root();
    let p = app_paths::paths();
    assert!(p.overridden, "YAP_DATA_DIR was not honoured");
    for dir in [&p.root, &p.recovery, &p.recordings, &p.meetings, &p.db] {
        assert!(
            dir.starts_with(root),
            "{} is outside the YAP_DATA_DIR scratch root {} — refusing to touch the real data dir",
            dir.display(),
            root.display()
        );
    }
    std::fs::create_dir_all(&p.recovery).unwrap();
    std::fs::create_dir_all(&p.recordings).unwrap();
    std::fs::create_dir_all(&p.meetings).unwrap();
    p
}

/// A fresh history DB under the scratch root, unique per test.
fn fresh_db(name: &str) -> (Database, PathBuf) {
    let path = paths().root.join(format!("{name}.db"));
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    (Database::open(path.clone()).expect("open db"), path)
}

/// A minimal valid 16 kHz mono WAV header + payload carrying `marker`.
fn plant_wav(path: &Path, marker: &str) {
    let mut bytes = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    bytes.extend_from_slice(&[
        16, 0, 0, 0, 1, 0, 1, 0, 0x80, 0x3e, 0, 0, 0, 0x7d, 0, 0, 2, 0, 16, 0,
    ]);
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(marker.len() as u32).to_le_bytes());
    bytes.extend_from_slice(marker.as_bytes());
    std::fs::write(path, bytes).expect("plant sentinel wav");
    assert!(
        path.exists(),
        "sentinel was not planted at {}",
        path.display()
    );
}

fn file_contains(path: &Path, needle: &[u8]) -> bool {
    std::fs::read(path)
        .map(|b| b.windows(needle.len()).any(|w| w == needle))
        .unwrap_or(false)
}

#[test]
fn a_sentinel_wav_in_recovery_is_gone_from_the_filesystem() {
    let _g = serial();
    let p = paths();
    // The crate's own accessor, not a path this test made up.
    assert_eq!(recovery_dir(), p.recovery);
    let (db, _) = fresh_db("recovery");

    // A parked take a row points at (Y3-D cancel / YV52 failure) ...
    let parked = p.recovery.join("priv-b-parked.wav");
    plant_wav(&parked, "PRIVB-PARKED");
    db.record_failed_dictation_for_take(&parked, 2.0, "cancelled", None, Some("take-1".into()))
        .unwrap();
    // ... a clip NO row points at, a raw Y3-A/YV63 spill, and a scratch WAV.
    let orphan = p.recovery.join("priv-b-orphan.wav");
    plant_wav(&orphan, "PRIVB-ORPHAN");
    let spill = p.recovery.join("take-2.spill.pcm");
    std::fs::write(&spill, b"PRIVB-SPILL-PCM").unwrap();
    let capture = p.recovery.join("take-3.capture.wav");
    plant_wav(&capture, "PRIVB-CAPTURE");
    let scratch = p.recordings.join("in-flight.wav");
    plant_wav(&scratch, "PRIVB-SCRATCH");

    let report = erase::clear_history_everywhere(&db, p).expect("clear history");

    for f in [&parked, &orphan, &spill, &capture, &scratch] {
        assert!(
            !f.exists(),
            "{} is still on disk after Clear History",
            f.display()
        );
    }
    assert_eq!(report.failed, 0);
    assert!(report.files_removed >= 5, "report undercounts: {report:?}");
    assert!(
        db.list_failed_dictations().unwrap().is_empty(),
        "the parked take's row survived Clear History"
    );
    // Nothing left in recovery/ at all.
    let left: Vec<_> = std::fs::read_dir(&p.recovery).unwrap().flatten().collect();
    assert!(left.is_empty(), "recovery/ not empty: {left:?}");
}

#[test]
fn take_chunks_rows_are_gone_not_just_unqueryable() {
    let _g = serial();
    let p = paths();
    let (db, db_path) = fresh_db("chunks");
    let sentinel = "PRIVB-CHUNK-SENTINEL-regrettable-words";
    let wav = p.recovery.join("take-9.wav");
    plant_wav(&wav, "PRIVB-TAKE9");
    db.record_failed_dictation_for_take(&wav, 9.0, "unfinished", None, Some("take-9".into()))
        .unwrap();
    db.record_take_chunk("take-9", 0, sentinel, 16_000).unwrap();
    db.record_take_chunk("take-9", 1, sentinel, 32_000).unwrap();

    erase::clear_history_everywhere(&db, p).expect("clear history");

    // Not "the app's query returns nothing": a SEPARATE connection counts rows.
    let raw = rusqlite::Connection::open(&db_path).unwrap();
    let n: i64 = raw
        .query_row("SELECT COUNT(*) FROM take_chunks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "take_chunks rows survived Clear History");
    drop(raw);
    // And the words are not byte-readable in the DB, its WAL or its SHM.
    for suffix in ["", "-wal", "-shm"] {
        let f = PathBuf::from(format!("{}{suffix}", db_path.display()));
        assert!(
            !file_contains(&f, sentinel.as_bytes()),
            "partial transcript text is still byte-readable in {}",
            f.display()
        );
    }
    assert!(!wav.exists(), "the take's audio survived its chunk text");
}

#[test]
fn orphaned_meeting_audio_is_swept() {
    let _g = serial();
    let p = paths();
    assert_eq!(meetings_dir(), p.meetings);
    let (db, _) = fresh_db("meetings");

    // A meeting a row still owns: Clear History is not "delete my meetings".
    let m = db.create_meeting("Standup", "manual").unwrap();
    let owned = p.meetings.join(format!("{}.wav", m.id));
    plant_wav(&owned, "PRIVB-OWNED-MEETING");
    db.finish_meeting(&m.id, 60.0, Some(&owned)).unwrap();

    // Audio no row owns: a WAV whose row is gone, and an abandoned journal.
    let orphan = p.meetings.join("deleted-meeting.wav");
    plant_wav(&orphan, "PRIVB-ORPHAN-MEETING");
    let spill = p.meetings.join("crashed.t0.spill.pcm");
    std::fs::write(&spill, b"PRIVB-MEETING-SPILL").unwrap();
    let marker = p.meetings.join("crashed.meeting.in_progress.json");
    std::fs::write(&marker, b"{}").unwrap();

    erase::clear_history_everywhere(&db, p).expect("clear history");

    for f in [&orphan, &spill, &marker] {
        assert!(
            !f.exists(),
            "orphaned meeting audio {} survived",
            f.display()
        );
    }
    assert!(
        owned.exists(),
        "Clear History deleted a meeting a row still owns"
    );

    // The separate control DOES take every meeting's audio, and says so in the row.
    let report = erase::delete_all_recordings(&db, p).expect("delete all recordings");
    assert!(
        !owned.exists(),
        "Delete all recordings left meeting audio on disk"
    );
    assert_eq!(report.failed, 0);
    assert!(
        db.meeting_audio_paths().unwrap().is_empty(),
        "a meeting row still points at deleted audio"
    );
}

#[test]
fn delete_all_recordings_keeps_the_transcripts() {
    let _g = serial();
    let p = paths();
    let (db, _) = fresh_db("keep");
    db.insert_transcript(
        "keep these words".into(),
        "native".into(),
        0.5,
        1.0,
        0,
        None,
    )
    .unwrap();
    let parked = p.recovery.join("keep-parked.wav");
    plant_wav(&parked, "PRIVB-KEEP");
    db.record_failed_dictation_for_take(&parked, 1.0, "cancelled", None, Some("take-k".into()))
        .unwrap();
    db.record_take_chunk("take-k", 0, "PRIVB-KEEP-CHUNK", 16_000)
        .unwrap();

    erase::delete_all_recordings(&db, p).expect("delete all recordings");

    assert!(
        !parked.exists(),
        "parked audio survived Delete all recordings"
    );
    assert!(db.list_failed_dictations().unwrap().is_empty());
    assert_eq!(
        db.list_transcripts(10, None).unwrap().len(),
        1,
        "Delete all recordings deleted transcript history"
    );
}
