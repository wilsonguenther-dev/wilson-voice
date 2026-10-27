//! yap24-NT1 (R2, R3) — audio that was never turned into words is kept
//! longer than transcribed audio, but not forever: 30 days, and a 5 GiB
//! ceiling past which the OLDEST of it goes first. The 7-day class (complete
//! AND at least one segment) is neither counted nor purged by the ceiling.
//! Every purge and every delete takes the meeting's ledger and anchor sidecars
//! with its WAVs.

mod support;

use std::path::{Path, PathBuf};

use support::{open_db, temp_dir};
use wilson_voice_lib::db::Database;
use wilson_voice_lib::meeting_pipeline::{meeting_sidecar_paths, sweep_meeting_audio};
use wilson_voice_lib::meetings::{
    MeetingState, NewMeetingSegment, AUDIO_RETENTION_DAYS, FAILED_AUDIO_CEILING_BYTES,
    FAILED_AUDIO_RETENTION_DAYS,
};

const NO_CEILING: u64 = u64::MAX;

fn set_started(dir: &Path, id: &str, days_ago: i64) {
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    let at = (chrono::Utc::now() - chrono::Duration::days(days_ago) + chrono::Duration::minutes(5))
        .to_rfc3339();
    conn.execute(
        "UPDATE meetings SET started_at = ?2 WHERE id = ?1",
        rusqlite::params![id, at],
    )
    .unwrap();
}

/// A meeting with two tracks of `bytes_per_track` each, `segments` transcript
/// rows, in `state`, that started `days_ago` days ago.
fn meeting(
    db: &Database,
    dir: &Path,
    tag: &str,
    state: MeetingState,
    segments: usize,
    days_ago: i64,
    bytes_per_track: usize,
) -> (String, PathBuf, PathBuf) {
    let mic = dir.join(format!("{tag}.t0.wav"));
    let sys = dir.join(format!("{tag}.t1.wav"));
    std::fs::write(&mic, vec![0u8; bytes_per_track]).unwrap();
    std::fs::write(&sys, vec![0u8; bytes_per_track]).unwrap();
    let m = db.create_meeting(tag, "manual").unwrap();
    db.finish_meeting(&m.id, 10.0, Some(&mic)).unwrap();
    db.set_meeting_sys_wav_path(&m.id, Some(&sys)).unwrap();
    let segs: Vec<NewMeetingSegment> = (0..segments)
        .map(|i| NewMeetingSegment::new(i as f64, i as f64 + 1.0, format!("words {i}")))
        .collect();
    db.append_meeting_segments(&m.id, &segs).unwrap();
    db.set_meeting_state(&m.id, state, None).unwrap();
    set_started(dir, &m.id, days_ago);
    (m.id, mic, sys)
}

fn has_audio(db: &Database, id: &str) -> bool {
    let m = db.get_meeting(id).unwrap().expect("row is never deleted");
    let kept = m.mic_wav_path.is_some() || m.sys_wav_path.is_some();
    assert_eq!(kept, m.audio_kept, "audio_kept must agree with the paths");
    kept
}

fn sidecars(dir: &Path, id: &str) -> Vec<PathBuf> {
    let files = vec![
        dir.join(format!("{id}.t0.asr-progress.json")),
        dir.join(format!("{id}.t1.asr-progress.json")),
        dir.join(format!("{id}.t0.index.jsonl")),
        dir.join(format!("{id}.t1.index.jsonl")),
        dir.join(format!("{id}.index.jsonl")),
        // Rebase onto yap24-NT2: the R2 attempt marker and the R3 capture
        // carry live beside the ledgers too (`ledger_paths`), so they go with
        // the audio as well.
        dir.join(format!("{id}.attempts.json")),
        dir.join(format!("{id}.capture.json")),
    ];
    for f in &files {
        std::fs::write(f, b"{}").unwrap();
    }
    files
}

#[test]
fn the_windows_are_the_ones_the_panel_set() {
    assert_eq!(
        AUDIO_RETENTION_DAYS, 7,
        "the 7-day class is not this item's to change"
    );
    assert_eq!(FAILED_AUDIO_RETENTION_DAYS, 30);
    assert_eq!(FAILED_AUDIO_CEILING_BYTES, 5 * 1024 * 1024 * 1024);
}

#[test]
fn untranscribed_audio_is_kept_thirty_days_not_seven() {
    let dir = temp_dir("nt1-30d");
    let db = open_db(&dir);
    let (failed_10, f10m, f10s) = meeting(&db, &dir, "failed10", MeetingState::Failed, 0, 10, 8);
    let (failed_31, f31m, f31s) = meeting(&db, &dir, "failed31", MeetingState::Failed, 0, 31, 8);
    let (partial_31, ..) = meeting(&db, &dir, "partial31", MeetingState::Partial, 2, 31, 8);
    let (transcribing_31, ..) = meeting(
        &db,
        &dir,
        "transcribing31",
        MeetingState::Transcribing,
        0,
        31,
        8,
    );
    let (silent_10, ..) = meeting(&db, &dir, "silent10", MeetingState::Complete, 0, 10, 8);
    let (silent_31, ..) = meeting(&db, &dir, "silent31", MeetingState::Complete, 0, 31, 8);
    let (done_8, d8m, d8s) = meeting(&db, &dir, "done8", MeetingState::Complete, 3, 8, 8);
    let (done_3, ..) = meeting(&db, &dir, "done3", MeetingState::Complete, 3, 3, 8);

    let sweep = sweep_meeting_audio(&db, chrono::Utc::now(), NO_CEILING).unwrap();

    // Inside 30 days, whatever the 7-day class would have said.
    assert!(has_audio(&db, &failed_10));
    assert!(f10m.exists() && f10s.exists());
    assert!(
        has_audio(&db, &silent_10),
        "complete with ZERO segments is not the 7-day class"
    );
    // Past 30 days, every untranscribed state, both tracks.
    for id in [&failed_31, &partial_31, &transcribing_31, &silent_31] {
        assert!(!has_audio(&db, id), "{id} is past 30 days and keeps audio");
    }
    assert!(!f31m.exists() && !f31s.exists(), "both tracks go together");
    // The 7-day class, unchanged.
    assert!(!has_audio(&db, &done_8));
    assert!(!d8m.exists() && !d8s.exists());
    assert!(has_audio(&db, &done_3));
    assert_eq!(sweep.transcribed.len(), 1);
    assert_eq!(sweep.untranscribed.len(), 4);
    // Rows are never deleted.
    assert_eq!(db.list_meetings(100, None).unwrap().len(), 8);
}

#[test]
fn past_the_ceiling_the_oldest_untranscribed_audio_goes_first() {
    let dir = temp_dir("nt1-ceiling");
    let db = open_db(&dir);
    // Four untranscribed meetings of 1000 bytes each (500 per track), all well
    // inside 30 days, so only the ceiling can purge them.
    let (oldest, ..) = meeting(&db, &dir, "a-oldest", MeetingState::Failed, 0, 9, 500);
    let (second, ..) = meeting(&db, &dir, "b-second", MeetingState::Partial, 1, 6, 500);
    let (third, ..) = meeting(&db, &dir, "c-third", MeetingState::Transcribing, 0, 3, 500);
    let (newest, ..) = meeting(&db, &dir, "d-newest", MeetingState::Complete, 0, 1, 500);
    // A huge, fresh, transcribed meeting: if the ceiling counted it, every
    // untranscribed meeting above would be purged.
    let (transcribed, ..) = meeting(&db, &dir, "e-done", MeetingState::Complete, 2, 2, 50_000);

    // 4000 bytes held, ceiling 2500: the two oldest go, in order, and the
    // class lands at 2000 <= 2500.
    let sweep = sweep_meeting_audio(&db, chrono::Utc::now(), 2_500).unwrap();
    let ids: Vec<&str> = sweep
        .untranscribed
        .iter()
        .map(|p| p.meeting_id.as_str())
        .collect();
    assert_eq!(ids, vec![oldest.as_str(), second.as_str()], "oldest first");
    assert!(!has_audio(&db, &oldest));
    assert!(!has_audio(&db, &second));
    assert!(has_audio(&db, &third));
    assert!(has_audio(&db, &newest));
    assert!(
        has_audio(&db, &transcribed),
        "the 7-day class is not the ceiling's"
    );
    assert!(sweep.transcribed.is_empty());

    // Under the ceiling now: a second sweep is a no-op.
    let again = sweep_meeting_audio(&db, chrono::Utc::now(), 2_500).unwrap();
    assert_eq!(again.meetings(), 0);
}

#[test]
fn a_zero_ceiling_still_never_touches_the_seven_day_class_or_a_live_recording() {
    let dir = temp_dir("nt1-zero");
    let db = open_db(&dir);
    let (done, dm, ds) = meeting(&db, &dir, "done", MeetingState::Complete, 1, 1, 100);
    let (failed, ..) = meeting(&db, &dir, "failed", MeetingState::Failed, 0, 1, 100);
    let (live, ..) = meeting(&db, &dir, "live", MeetingState::Recording, 0, 40, 100);

    let sweep = sweep_meeting_audio(&db, chrono::Utc::now(), 0).unwrap();
    assert!(has_audio(&db, &done));
    assert!(dm.exists() && ds.exists());
    assert!(!has_audio(&db, &failed));
    assert!(
        has_audio(&db, &live),
        "a row in `recording` is live and never purged"
    );
    assert_eq!(sweep.untranscribed.len(), 1);
}

#[test]
fn purge_removes_the_ledgers_and_anchors_with_the_wavs() {
    let dir = temp_dir("nt1-sidecars-purge");
    let db = open_db(&dir);
    let (old, ..) = meeting(&db, &dir, "old", MeetingState::Failed, 0, 31, 10);
    let (done, ..) = meeting(&db, &dir, "done", MeetingState::Complete, 1, 8, 10);
    let (kept, ..) = meeting(&db, &dir, "kept", MeetingState::Failed, 0, 2, 10);
    let old_files = sidecars(&dir, &old);
    let done_files = sidecars(&dir, &done);
    let kept_files = sidecars(&dir, &kept);
    assert_eq!(meeting_sidecar_paths(&dir, &old).len(), old_files.len());

    let sweep = sweep_meeting_audio(&db, chrono::Utc::now(), NO_CEILING).unwrap();
    for f in old_files.iter().chain(done_files.iter()) {
        assert!(
            !f.exists(),
            "{} outlived the audio it describes",
            f.display()
        );
        assert!(sweep.removed.contains(f));
    }
    for f in &kept_files {
        assert!(
            f.exists(),
            "{} belongs to a meeting that kept its audio",
            f.display()
        );
    }
}

#[test]
fn delete_removes_the_ledgers_and_anchors_with_the_wavs() {
    let dir = temp_dir("nt1-sidecars-delete");
    let db = open_db(&dir);
    let (gone, gm, gs) = meeting(&db, &dir, "gone", MeetingState::Failed, 0, 1, 10);
    let (other, om, _) = meeting(&db, &dir, "other", MeetingState::Failed, 0, 1, 10);
    let gone_files = sidecars(&dir, &gone);
    let other_files = sidecars(&dir, &other);

    db.delete_meeting_with_audio(&gone).unwrap();
    assert!(!gm.exists() && !gs.exists());
    for f in &gone_files {
        assert!(!f.exists(), "{} outlived its deleted meeting", f.display());
    }
    assert!(om.exists());
    for f in &other_files {
        assert!(f.exists(), "{} belongs to another meeting", f.display());
    }
}

/// yap24-NT1 fix round 1 (review BLOCKING 2) — the sweep commits the 7-day
/// class FIRST. If the untranscribed purge then fails (SQLITE_BUSY, a full
/// disk), the 7-day class's rows already say "audio expired", so its WAVs and
/// ledgers must still leave the disk in that same sweep — otherwise they sit
/// there forever, outside every later sweep, after the user was told they are
/// gone. The failed class keeps its audio (its transaction rolled back) and is
/// retried on the next sweep.
#[test]
fn a_failed_untranscribed_purge_still_deletes_the_transcribed_audio_it_expired() {
    let dir = temp_dir("nt1-second-purge-fails");
    let db = open_db(&dir);
    let (done_8, d8m, d8s) = meeting(&db, &dir, "done8", MeetingState::Complete, 3, 8, 8);
    let done_8_sidecars = sidecars(&dir, &done_8);
    let (failed_31, f31m, f31s) = meeting(&db, &dir, "failed31", MeetingState::Failed, 0, 31, 8);

    // Make the SECOND purge's UPDATE fail, and only it: a row that is not
    // `complete` cannot be updated while this trigger exists.
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER nt1_inject_second_purge_failure BEFORE UPDATE ON meetings
         WHEN OLD.state <> 'complete'
         BEGIN SELECT RAISE(ABORT, 'injected: untranscribed purge fails'); END;",
    )
    .unwrap();
    assert!(
        db.purge_untranscribed_meeting_audio(chrono::Utc::now(), NO_CEILING)
            .is_err(),
        "the injection must actually make the second purge fail"
    );

    let sweep = sweep_meeting_audio(&db, chrono::Utc::now(), NO_CEILING)
        .expect("a failure in one class never discards the other class's sweep");

    // The 7-day class: row cleared AND files gone, in the same sweep.
    assert_eq!(sweep.transcribed.len(), 1);
    assert!(!has_audio(&db, &done_8));
    assert!(
        !d8m.exists() && !d8s.exists(),
        "the row says the audio expired, so the WAVs must be gone too"
    );
    for f in &done_8_sidecars {
        assert!(!f.exists(), "{} survived its meeting's purge", f.display());
    }
    // The failed class: rolled back, row and files both kept, nothing lies.
    assert!(sweep.untranscribed.is_empty());
    assert!(has_audio(&db, &failed_31));
    assert!(f31m.exists() && f31s.exists());

    // Once the fault clears, the next sweep takes it.
    conn.execute_batch("DROP TRIGGER nt1_inject_second_purge_failure;")
        .unwrap();
    let sweep = sweep_meeting_audio(&db, chrono::Utc::now(), NO_CEILING).unwrap();
    assert_eq!(sweep.untranscribed.len(), 1);
    assert!(!has_audio(&db, &failed_31));
    assert!(!f31m.exists() && !f31s.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Settings → Privacy sentence, whitespace collapsed (JSX wraps it).
fn privacy_copy() -> String {
    include_str!("../../src/views/settings/Privacy.tsx")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// yap24-NT1 fix round 2 — the Privacy sentence is the retention promise, so it
/// must say what the sweep does: the 7 days count from when the meeting
/// STARTED (not from when its transcript was done), and an interrupted
/// (`partial`) meeting keeps its audio 30 days even when it has a transcript,
/// because Meetings still offers Retry on it.
#[test]
fn the_privacy_sentence_states_the_rule_the_sweep_runs() {
    let copy = privacy_copy();
    assert!(
        !copy.contains("7 days after its transcript is done"),
        "the 7 days are counted from started_at, not from the transcript"
    );
    assert!(
        copy.contains(
            "Audio from a meeting that finished and was fully transcribed is deleted 7 days after the meeting started."
        ),
        "the 7-day class, counted from the start: {copy}"
    );
    assert!(
        copy.contains(
            "A meeting that was interrupted, failed, has no transcript, or is only partly or not yet transcribed keeps its audio 30 days after it started, and past 5 GB of that audio the oldest is deleted first."
        ),
        "the 30-day class names interrupted meetings: {copy}"
    );

    // The behaviour the sentence promises, on the shapes the reviewers probed.
    let dir = temp_dir("nt1-copy");
    let db = open_db(&dir);
    // An interrupted meeting with a full transcript (capture_was_partial, a
    // recovered crash, a lid close): 10 days old, so 30-day class, kept.
    let (interrupted_10, ..) = meeting(&db, &dir, "int10", MeetingState::Partial, 4, 10, 8);
    // The same shape past 30 days goes.
    let (interrupted_31, ..) = meeting(&db, &dir, "int31", MeetingState::Partial, 4, 31, 8);
    // A meeting that STARTED 20 days ago and whose transcript finished just
    // now (a late Retry): 7 days after it started has passed, so it goes.
    let (late_retry, ..) = meeting(&db, &dir, "late", MeetingState::Complete, 2, 20, 8);
    // Fully transcribed, started 6 days ago: kept.
    let (fresh, ..) = meeting(&db, &dir, "fresh", MeetingState::Complete, 2, 6, 8);

    let sweep = sweep_meeting_audio(&db, chrono::Utc::now(), NO_CEILING).unwrap();
    assert!(
        has_audio(&db, &interrupted_10),
        "an interrupted meeting keeps audio 30 days, transcript or not"
    );
    assert!(!has_audio(&db, &interrupted_31));
    assert!(
        !has_audio(&db, &late_retry),
        "7 days after the meeting STARTED, once fully transcribed"
    );
    assert!(has_audio(&db, &fresh));
    assert_eq!(sweep.transcribed.len(), 1);
    assert_eq!(sweep.untranscribed.len(), 1);
}
