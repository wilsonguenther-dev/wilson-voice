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
