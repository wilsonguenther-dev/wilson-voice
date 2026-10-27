//! yap-meetings-fix (B / NT1) — retention never purges audio that has not been
//! turned into words, and launch reconcile un-strands rows.

mod support;

use std::path::Path;
use std::sync::Mutex;

use support::{open_db, temp_dir};
use wilson_voice_lib::db::Database;
use wilson_voice_lib::meeting_control::reconcile_stranded_meetings;
use wilson_voice_lib::meeting_pipeline::AUDIO_REMOVED_MESSAGE;
use wilson_voice_lib::meetings::{MeetingState, NewMeetingSegment};

fn age(dir: &Path, id: &str) {
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    conn.execute(
        "UPDATE meetings SET started_at = ?2 WHERE id = ?1",
        rusqlite::params![id, "2020-01-01T00:00:00+00:00"],
    )
    .unwrap();
}

fn meeting(db: &Database, dir: &Path, tag: &str, state: MeetingState, segments: usize) -> String {
    let mic = dir.join(format!("{tag}.t0.wav"));
    let sys = dir.join(format!("{tag}.t1.wav"));
    std::fs::write(&mic, b"RIFF").unwrap();
    std::fs::write(&sys, b"RIFF").unwrap();
    let m = db.create_meeting(tag, "manual").unwrap();
    db.finish_meeting(&m.id, 10.0, Some(&mic)).unwrap();
    db.set_meeting_sys_wav_path(&m.id, Some(&sys)).unwrap();
    let segs: Vec<NewMeetingSegment> = (0..segments)
        .map(|i| NewMeetingSegment::new(i as f64, i as f64 + 1.0, format!("words {i}")))
        .collect();
    db.append_meeting_segments(&m.id, &segs).unwrap();
    db.set_meeting_state(&m.id, state, None).unwrap();
    age(dir, &m.id);
    m.id
}

#[test]
fn retention_is_state_aware() {
    let dir = temp_dir("nt1-retention");
    let db = open_db(&dir);
    let transcribing = meeting(&db, &dir, "transcribing", MeetingState::Transcribing, 0);
    let partial = meeting(&db, &dir, "partial", MeetingState::Partial, 2);
    let failed = meeting(&db, &dir, "failed", MeetingState::Failed, 0);
    let complete = meeting(&db, &dir, "complete", MeetingState::Complete, 3);
    let silent = meeting(&db, &dir, "silent", MeetingState::Complete, 0);

    let purged = db.purge_meeting_audio(chrono::Utc::now()).unwrap();
    assert_eq!(
        purged.len(),
        2,
        "only the complete meeting WITH segments, both tracks: {purged:?}"
    );
    assert!(purged
        .iter()
        .all(|p| p.to_string_lossy().contains("complete.t")));

    for id in [&transcribing, &partial, &failed, &silent] {
        let m = db.get_meeting(id).unwrap().unwrap();
        assert!(
            m.mic_wav_path.is_some() && m.sys_wav_path.is_some(),
            "kept: {m:?}"
        );
        assert!(m.audio_kept);
    }
    let gone = db.get_meeting(&complete).unwrap().unwrap();
    assert!(gone.mic_wav_path.is_none() && gone.sys_wav_path.is_none());
    assert_eq!(
        db.list_meeting_segments(&complete).unwrap().len(),
        3,
        "the transcript stays"
    );
}

#[test]
fn reconcile_fails_a_row_whose_audio_is_gone_and_requeues_one_whose_audio_is_there() {
    let dir = temp_dir("nt1-reconcile");
    let db = open_db(&dir);
    let kept = meeting(&db, &dir, "kept", MeetingState::Transcribing, 0);
    let lost = meeting(&db, &dir, "lost", MeetingState::Transcribing, 0);
    let lost_row = db.get_meeting(&lost).unwrap().unwrap();
    std::fs::remove_file(lost_row.mic_wav_path.unwrap()).unwrap();
    std::fs::remove_file(lost_row.sys_wav_path.unwrap()).unwrap();
    let done = meeting(&db, &dir, "done", MeetingState::Complete, 1);

    let queued = Mutex::new(Vec::<String>::new());
    let out = reconcile_stranded_meetings(&db, &dir, &|id| queued.lock().unwrap().push(id.to_string()));
    println!("reconciled: {out:?}");

    assert_eq!(
        *queued.lock().unwrap(),
        vec![kept.clone()],
        "only the row with audio is queued"
    );
    let k = db.get_meeting(&kept).unwrap().unwrap();
    assert_eq!(
        k.state, "transcribing",
        "left as is, handed to the pipeline"
    );
    let l = db.get_meeting(&lost).unwrap().unwrap();
    assert_eq!(l.state, "failed");
    assert_eq!(l.error.as_deref(), Some(AUDIO_REMOVED_MESSAGE));
    assert_eq!(db.get_meeting(&done).unwrap().unwrap().state, "complete");
    assert!(
        out.iter().all(|(id, _, _)| id != &done),
        "complete rows are not touched"
    );
}

// ── yap24-NT1 (R1) — launch recovers a crashed meeting INTO ITS ROW ─────────

use wilson_voice_lib::meeting::{
    recover_orphaned_meetings, IndexRecord, MeetingJournal, TARGET_RATE,
};
use wilson_voice_lib::meeting_control::{adopt_recovered_meetings, RECOVERED_MEETING_NOTE};

/// Record `seconds` on the given tracks of a `tracks`-track journal bound to
/// `row`, then die without stopping — what a crash leaves on disk.
fn crash(dir: &Path, tracks: usize, delivering: &[usize], row: Option<&str>, seconds: u64) {
    let journal = MeetingJournal::start(dir, tracks).expect("journal opens");
    if let Some(row) = row {
        journal.bind_row(row).expect("row binding written");
    }
    for second in 0..seconds {
        for &track in delivering {
            let block = vec![0.25f32; TARGET_RATE as usize];
            assert_eq!(journal.append(track, &block), block.len());
        }
        journal.index(IndexRecord {
            host_ns: (second + 1) * 1_000_000_000,
            captured_samples: (second + 1) * TARGET_RATE as u64,
            spilled_samples: (second + 1) * TARGET_RATE as u64,
        });
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    journal.abandon();
}

#[test]
fn a_crashed_meeting_is_recovered_into_its_own_row_by_track_and_queued() {
    let dir = temp_dir("nt1-recover");
    let db = open_db(&dir);
    // Two rows stuck where a crash leaves them.
    let two_track = db.create_meeting("call", "manual").unwrap();
    let sys_only = db.create_meeting("mic died", "manual").unwrap();
    db.set_meeting_state(&two_track.id, MeetingState::Recording, None)
        .unwrap();
    db.set_meeting_state(&sys_only.id, MeetingState::Recording, None)
        .unwrap();
    crash(&dir, 2, &[0, 1], Some(&two_track.id), 2);
    // Matrix row #3's shape: a two-track meeting whose MIC never delivered.
    // Positional filing would put the room's audio in mic_wav_path.
    crash(&dir, 2, &[1], Some(&sys_only.id), 2);

    let adopted = adopt_recovered_meetings(&db, recover_orphaned_meetings(&dir));
    assert_eq!(adopted.len(), 2, "{adopted:?}");
    assert!(
        recover_orphaned_meetings(&dir).is_empty(),
        "a recovered journal is consumed, not recovered twice"
    );
    assert_eq!(
        std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".meeting.row"))
            .count(),
        0,
        "the row binding goes with the marker"
    );

    let m = db.get_meeting(&two_track.id).unwrap().unwrap();
    assert_eq!(m.state, "partial", "a crash is never a whole meeting");
    assert!(m.mic_wav_path.as_deref().unwrap().ends_with(".t0.wav"));
    assert!(m.sys_wav_path.as_deref().unwrap().ends_with(".t1.wav"));
    assert!(Path::new(m.mic_wav_path.as_deref().unwrap()).exists());
    assert!(m.audio_kept);
    assert!(m.duration_seconds > 1.5, "{}", m.duration_seconds);
    assert!(m.error.as_deref().unwrap().contains(RECOVERED_MEETING_NOTE));

    let s = db.get_meeting(&sys_only.id).unwrap().unwrap();
    assert_eq!(s.mic_wav_path, None, "the room is never filed as the user");
    assert!(s.sys_wav_path.as_deref().unwrap().ends_with(".t1.wav"));
    assert_eq!(s.state, "partial");

    // Reconcile — the next launch step — hands both to the transcription queue
    // instead of failing them for missing audio.
    let queued = Mutex::new(Vec::<String>::new());
    reconcile_stranded_meetings(&db, &dir, &|id| queued.lock().unwrap().push(id.to_string()));
    let queued = queued.into_inner().unwrap();
    assert!(
        queued.contains(&two_track.id) && queued.contains(&sys_only.id),
        "{queued:?}"
    );
    assert_eq!(
        db.get_meeting(&two_track.id).unwrap().unwrap().state,
        "partial"
    );
}

#[test]
fn a_journal_with_no_row_binding_gets_a_row_rather_than_being_lost() {
    let dir = temp_dir("nt1-recover-unbound");
    let db = open_db(&dir);
    crash(&dir, 1, &[0], None, 1);
    let adopted = adopt_recovered_meetings(&db, recover_orphaned_meetings(&dir));
    assert_eq!(adopted.len(), 1);
    let m = db.get_meeting(&adopted[0].0).unwrap().unwrap();
    assert_eq!(m.source, "recovered");
    assert!(m.mic_wav_path.is_some());
    assert_eq!(m.state, "partial");
}

// ── yap24-NT1 review BLOCKING 2 — a deleted meeting never comes back ────────

use wilson_voice_lib::meeting::remove_journals_bound_to;

fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !n.ends_with(".db") && !n.contains(".db-"))
        .collect();
    v.sort();
    v
}

#[test]
fn a_meeting_deleted_while_recording_is_not_resurrected_and_its_audio_goes() {
    let dir = temp_dir("nt1-deleted-row");
    let db = open_db(&dir);
    let row = db.create_meeting("deleted by the user", "manual").unwrap();
    // The meeting is LIVE (its lock held) when the user deletes it, so the
    // delete command cannot remove the journal — it tombstones it. Then the
    // app crashes before the recording stops.
    let journal = MeetingJournal::start(&dir, 1).expect("journal opens");
    journal.bind_row(&row.id).unwrap();
    let block = vec![0.25f32; TARGET_RATE as usize];
    assert_eq!(journal.append(0, &block), block.len());
    journal.index(IndexRecord {
        host_ns: 1_000_000_000,
        captured_samples: TARGET_RATE as u64,
        spilled_samples: TARGET_RATE as u64,
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    db.delete_meeting_with_audio(&row.id).unwrap();
    assert!(
        remove_journals_bound_to(&dir, &row.id).is_empty(),
        "a live journal is never removed out from under its writer"
    );
    assert!(
        files_in(&dir)
            .iter()
            .any(|n| n.ends_with(".meeting.deleted")),
        "the live journal is tombstoned: {:?}",
        files_in(&dir)
    );
    journal.abandon();

    let recovered = recover_orphaned_meetings(&dir);
    assert_eq!(
        recovered.len(),
        1,
        "the journal itself is still recoverable"
    );
    assert!(recovered[0].deleted, "the tombstone reaches the adopt step");
    let wavs = recovered[0].tracks.clone();
    assert!(wavs.iter().all(|w| w.exists()));
    let adopted = adopt_recovered_meetings(&db, recovered);
    assert!(
        adopted.is_empty(),
        "a deleted meeting is never re-filed: {adopted:?}"
    );
    assert!(
        db.list_meetings(100, None).unwrap().is_empty(),
        "no 'Recovered' row is made for a meeting the user deleted"
    );
    assert!(
        wavs.iter().all(|w| !w.exists()),
        "the deleted meeting's recovered audio is deleted, not kept"
    );
    assert!(
        files_in(&dir)
            .iter()
            .all(|n| !n.ends_with(".meeting.deleted")),
        "the tombstone is consumed with the journal: {:?}",
        files_in(&dir)
    );
}

// ── yap24-NT1 re-review BLOCKING — absence from THIS DB is not a deletion ──

#[test]
fn a_bound_journal_whose_row_is_absent_from_a_substitute_db_keeps_its_audio() {
    // The real DB has the row in `recording`; this launch opened a DIFFERENT,
    // empty DB (corruption quarantine recreated it). No tombstone exists.
    let dir = temp_dir("nt1-absent-row");
    let real = open_db(&dir);
    let row = real.create_meeting("only copy", "manual").unwrap();
    real.set_meeting_state(&row.id, MeetingState::Recording, None)
        .unwrap();
    crash(&dir, 1, &[0], Some(&row.id), 1);
    let fresh_dir = temp_dir("nt1-absent-row-freshdb");
    let fresh = open_db(&fresh_dir);
    assert!(fresh.get_meeting(&row.id).unwrap().is_none());

    let recovered = recover_orphaned_meetings(&dir);
    assert_eq!(recovered.len(), 1);
    assert!(!recovered[0].deleted, "no tombstone was written");
    let wavs = recovered[0].tracks.clone();
    let adopted = adopt_recovered_meetings(&fresh, recovered);
    assert!(
        wavs.iter().all(|w| w.exists()),
        "the only copy of the audio is never unlinked on absence alone"
    );
    assert_eq!(adopted.len(), 1, "{adopted:?}");
    let m = fresh.get_meeting(&adopted[0].0).unwrap().unwrap();
    assert_eq!(m.source, "recovered", "filed where the user can find it");
    assert_eq!(
        m.mic_wav_path.as_deref().map(Path::new),
        Some(wavs[0].as_path())
    );
}

#[test]
fn launch_recovery_skips_the_in_memory_fallback_and_leaves_journals_whole() {
    use wilson_voice_lib::meeting_control::recover_crashed_meetings_at_launch;
    let dir = temp_dir("nt1-inmemory");
    let disk = open_db(&dir);
    let row = disk.create_meeting("on disk", "manual").unwrap();
    disk.set_meeting_state(&row.id, MeetingState::Recording, None)
        .unwrap();
    crash(&dir, 1, &[0], Some(&row.id), 1);
    crash(&dir, 1, &[0], None, 1);
    let before = files_in(&dir);

    let mem = Database::open_in_memory().unwrap();
    assert!(!mem.is_persistent());
    assert!(disk.is_persistent());
    let adopted = recover_crashed_meetings_at_launch(&mem, &dir);
    assert!(adopted.is_empty(), "{adopted:?}");
    assert!(mem.list_meetings(100, None).unwrap().is_empty());
    assert_eq!(
        files_in(&dir),
        before,
        "neither journal is consumed on the in-memory fallback"
    );

    // The next launch, with the real DB, recovers both: the bound one into
    // its own row, the unbound one into a Recovered row.
    let adopted = recover_crashed_meetings_at_launch(&disk, &dir);
    assert_eq!(adopted.len(), 2, "{adopted:?}");
    assert!(adopted.iter().any(|(id, _)| id == &row.id));
    let m = disk.get_meeting(&row.id).unwrap().unwrap();
    assert!(Path::new(m.mic_wav_path.as_deref().unwrap()).exists());
}

#[test]
fn deleting_a_meeting_removes_the_crash_journal_bound_to_it_and_nothing_else() {
    let dir = temp_dir("nt1-delete-journal");
    let db = open_db(&dir);
    let gone = db.create_meeting("delete me", "manual").unwrap();
    let kept = db.create_meeting("keep me", "manual").unwrap();
    crash(&dir, 1, &[0], Some(&gone.id), 1);
    crash(&dir, 1, &[0], Some(&kept.id), 1);
    // A LIVE journal bound to the same row: its lock is held, so it is left.
    let live = MeetingJournal::start(&dir, 1).expect("live journal opens");
    live.bind_row(&gone.id).unwrap();
    let live_id = live.id().to_string();

    // What the delete_meeting command does: the row and its audio, then every
    // crash journal bound to it.
    db.delete_meeting_with_audio(&gone.id).unwrap();
    let removed = remove_journals_bound_to(&dir, &gone.id);
    assert_eq!(
        removed.len(),
        1,
        "only the crashed journal, not the live one: {removed:?}"
    );
    let left = files_in(&dir);
    assert!(
        left.iter()
            .all(|n| !n.starts_with(&format!("{}.", removed[0]))),
        "every file of the deleted meeting's journal is gone: {left:?}"
    );
    assert!(
        left.iter().any(|n| n.starts_with(&format!("{live_id}."))),
        "a live journal is never removed out from under its writer: {left:?}"
    );
    drop(live);

    // Next launch: only the other meeting is recovered; the deleted one never
    // comes back.
    let adopted = adopt_recovered_meetings(&db, recover_orphaned_meetings(&dir));
    assert_eq!(adopted.len(), 1, "{adopted:?}");
    assert_eq!(adopted[0].0, kept.id);
    let rows = db.list_meetings(100, None).unwrap();
    assert_eq!(rows.len(), 1, "no Recovered row for the deleted meeting");
    assert_eq!(rows[0].id, kept.id);
}
