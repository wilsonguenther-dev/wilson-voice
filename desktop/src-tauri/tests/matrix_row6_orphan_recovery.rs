//! YV91 acceptance — **error matrix row #6: the app is killed or crashes
//! mid-meeting.**
//!
//! Required behaviour: an orphan `.in_progress.json` marker plus its
//! `.spill.pcm` found at startup finalizes into a wav, and the meeting is
//! `state='partial'` with a Resume-processing entry point.
//!
//! This extends YV63, which did the same for a dictation take. What is new for
//! a meeting: N tracks, an index sidecar, and the rule that a marker whose
//! state was never stamped is a CRASH — so the recovered meeting is partial
//! even though nothing ever wrote the word "partial" anywhere.
//!
//! The recovery is also the same code a normal stop runs
//! (`finalize_meeting_marker`), on purpose: the crash path is exercised on
//! every clean stop, instead of being the path nobody runs until it matters.

use std::path::{Path, PathBuf};
use std::time::Duration;

use wilson_voice_lib::meeting::{
    recover_orphaned_meetings, IndexRecord, MeetingJournal, MeetingState, TARGET_RATE,
};

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yap-yv91-row6-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn entries(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.to_string_lossy().ends_with(suffix))
                .collect()
        })
        .unwrap_or_default()
}

/// Record a meeting and then die without stopping it.
fn crash_mid_meeting(dir: &Path, tracks: usize, seconds: usize) {
    let journal = MeetingJournal::start(dir, tracks).expect("journal opens");
    for second in 0..seconds {
        for track in 0..tracks {
            let level = 0.5 - (track as f32 * 0.2);
            let block = vec![level; TARGET_RATE as usize];
            assert_eq!(journal.append(track, &block), block.len());
        }
        journal.index(IndexRecord {
            host_ns: (second as u64 + 1) * 1_000_000_000,
            captured_samples: (second as u64 + 1) * TARGET_RATE as u64,
            spilled_samples: (second as u64 + 1) * TARGET_RATE as u64,
        });
    }
    // The writer batches its flushes on a 250 ms timer; let it land the bytes
    // the way a real crash would find them.
    std::thread::sleep(Duration::from_millis(500));
    journal.abandon();
}

#[test]
fn an_orphan_marker_and_spill_become_a_playable_wav_marked_partial() {
    let dir = tmpdir("crash");
    crash_mid_meeting(&dir, 1, 3);

    // This is what the next launch finds.
    assert_eq!(entries(&dir, "meeting.in_progress.json").len(), 1);
    assert_eq!(entries(&dir, "spill.pcm").len(), 1);
    assert_eq!(entries(&dir, "index.jsonl").len(), 1);

    let recovered = recover_orphaned_meetings(&dir);
    assert_eq!(recovered.len(), 1, "the startup scan found the orphan");
    let meeting = &recovered[0];
    assert_eq!(
        meeting.state,
        MeetingState::Partial,
        "a marker nobody stamped is a crash, and a crashed meeting is partial"
    );
    assert!(
        !meeting.id.is_empty(),
        "the id survives, so the row can be linked"
    );
    assert_eq!(meeting.tracks.len(), 1);
    let mut reader = hound::WavReader::open(&meeting.tracks[0]).expect("a real wav");
    assert_eq!(reader.spec().sample_rate, TARGET_RATE);
    assert_eq!(reader.spec().channels, 1);
    let samples: Vec<i16> = reader.samples::<i16>().filter_map(|s| s.ok()).collect();
    assert_eq!(samples.len(), TARGET_RATE as usize * 3, "all three seconds");
    assert!((meeting.seconds - 3.0).abs() < 0.01);
    assert_eq!(meeting.spliced_silence_samples, 0);

    // The in-progress state is gone; the wav is what is left.
    assert!(entries(&dir, "meeting.in_progress.json").is_empty());
    assert!(entries(&dir, "spill.pcm").is_empty());
    assert!(entries(&dir, "index.jsonl").is_empty());
    assert!(
        recover_orphaned_meetings(&dir).is_empty(),
        "a second launch does not recover the same meeting twice"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_crashed_two_track_meeting_recovers_both_tracks() {
    let dir = tmpdir("crash-2");
    crash_mid_meeting(&dir, 2, 1);
    let recovered = recover_orphaned_meetings(&dir);
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].tracks.len(), 2);
    for track in &recovered[0].tracks {
        assert!(track.exists());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn several_orphans_are_all_recovered_and_a_dictation_journal_is_left_alone() {
    let dir = tmpdir("many");
    crash_mid_meeting(&dir, 1, 1);
    crash_mid_meeting(&dir, 1, 1);
    // YV63's dictation marker uses a different suffix and belongs to a
    // different recovery path; the meeting scan must not touch it.
    let foreign = dir.join("11111111-2222-3333-4444-555555555555.in_progress.json");
    std::fs::write(&foreign, "{}").expect("write");

    let recovered = recover_orphaned_meetings(&dir);
    assert_eq!(recovered.len(), 2);
    assert!(foreign.exists(), "the dictation journal was not consumed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_truncated_index_line_does_not_cost_the_recording() {
    // A crash truncates whatever was mid-write. The index is a convenience;
    // losing its last line must never lose the audio.
    let dir = tmpdir("truncated");
    crash_mid_meeting(&dir, 1, 2);
    let index = entries(&dir, "index.jsonl").pop().expect("an index");
    let mut raw = std::fs::read_to_string(&index).expect("read");
    raw.push_str("{\"host_ns\": 999, \"captured_sam");
    std::fs::write(&index, raw).expect("write");

    let recovered = recover_orphaned_meetings(&dir);
    assert_eq!(recovered.len(), 1);
    assert!((recovered[0].seconds - 2.0).abs() < 0.01);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_marker_with_no_audio_is_cleaned_up_rather_than_becoming_a_row() {
    // A stray start — the user hit record and immediately stopped, or the app
    // died before a single callback. A "meeting" of zero samples in History
    // would be noise, so recovery retires it silently. Same rule as YV63's
    // MIN_CLIP_SAMPLES floor for a stray tap.
    let dir = tmpdir("empty");
    let journal = MeetingJournal::start(&dir, 1).expect("journal opens");
    std::thread::sleep(Duration::from_millis(300));
    journal.abandon();
    assert_eq!(entries(&dir, "meeting.in_progress.json").len(), 1);

    assert!(recover_orphaned_meetings(&dir).is_empty());
    assert!(
        entries(&dir, "meeting.in_progress.json").is_empty(),
        "the empty marker is cleaned up, not left to be rescanned forever"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
