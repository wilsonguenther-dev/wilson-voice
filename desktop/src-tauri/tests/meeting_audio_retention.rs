//! YV94 retention (finding #28) — **time-based, 7 days**, not
//! delete-after-summarize.
//!
//! Delete-after-summarize is incoherent for 22-A: there is no summarize stage
//! until YV97, so it would mean either "audio is never deleted" or "the
//! transcript is the only artifact and the room can never be re-heard". The
//! default flips once YV97 has shipped and proven itself.
//!
//! The invariant that must hold either way: the sweep takes AUDIO, never words.

mod support;

use support::{open_db, seed_meeting, temp_dir};
use wilson_voice_lib::meetings::AUDIO_RETENTION_DAYS;

#[test]
fn the_retention_window_is_seven_days() {
    assert_eq!(
        AUDIO_RETENTION_DAYS, 7,
        "changing this is a product decision (finding #28), not a tweak"
    );
}

#[test]
fn the_sweep_purges_audio_and_keeps_the_transcript() {
    let dir = temp_dir("retention");
    let db = open_db(&dir);

    let old = seed_meeting(&db, &dir, "Old lecture", &["entropy always increases"]);
    let fresh = seed_meeting(&db, &dir, "Today's standup", &["ship it on friday"]);

    // Backdate the old meeting past the window.
    {
        let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
        let long_ago =
            (chrono::Utc::now() - chrono::Duration::days(AUDIO_RETENTION_DAYS + 1)).to_rfc3339();
        conn.execute(
            "UPDATE meetings SET started_at = ?2 WHERE id = ?1",
            rusqlite::params![old, long_ago],
        )
        .unwrap();
    }

    let old_wav =
        std::path::PathBuf::from(db.get_meeting(&old).unwrap().unwrap().mic_wav_path.unwrap());
    let fresh_wav = std::path::PathBuf::from(
        db.get_meeting(&fresh)
            .unwrap()
            .unwrap()
            .mic_wav_path
            .unwrap(),
    );

    let cutoff = chrono::Utc::now() - chrono::Duration::days(AUDIO_RETENTION_DAYS);
    let purged = db.purge_meeting_audio(cutoff).unwrap();
    assert_eq!(
        purged.len(),
        1,
        "only the expired meeting's audio: {purged:?}"
    );
    for p in &purged {
        std::fs::remove_file(p).unwrap();
    }

    let old_row = db.get_meeting(&old).unwrap().unwrap();
    assert!(
        old_row.mic_wav_path.is_none(),
        "the path is cleared, not dangling"
    );
    assert!(
        !old_row.audio_kept,
        "the UI must be able to say 'audio expired'"
    );
    assert!(!old_wav.exists());
    assert_eq!(
        old_row.segment_count, 1,
        "the TRANSCRIPT is kept forever — only the audio expires"
    );
    assert_eq!(
        db.list_meetings(50, Some("entropy".into())).unwrap().len(),
        1,
        "an expired meeting stays searchable"
    );

    let fresh_row = db.get_meeting(&fresh).unwrap().unwrap();
    assert!(fresh_row.audio_kept);
    assert!(fresh_wav.is_file(), "audio inside the window is untouched");

    // Idempotent: a second sweep finds nothing left to purge.
    assert!(db.purge_meeting_audio(cutoff).unwrap().is_empty());
}
