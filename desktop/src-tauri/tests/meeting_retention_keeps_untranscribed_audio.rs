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
