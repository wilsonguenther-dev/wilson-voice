//! YV106 acceptance — **the journal, with two producers**.
//!
//! `tests/capture_journal_recovery.rs` already proves the single-track case, and
//! this file deliberately extends its assumptions rather than restating them: a
//! two-track meeting is the same journal, the same `finalize`, the same
//! recovery, with a second `append(track, …)` stream going into it.
//!
//! What is genuinely new, and what each test here is for:
//!
//! * **Two wavs come out, and they did not cross.** The 22-A suite already
//!   asserts this at the journal level (`two_tracks_round_trip_because_22b_
//!   reuses_this_journal`). What it could not assert is the same thing through
//!   the CONSUMER, which is what this item built.
//! * **Each track is spliced from its OWN index sequence.** This is the half of
//!   the journal that was not N-track and had to become it. `plan_silence_
//!   splices` turns a record sequence into "audio is missing HERE" — so one
//!   shared sequence means a drop on the TAP punches a hole in the MIC's
//!   finalized track, and both tracks' timestamps end up wrong in opposite
//!   directions. The test drives a real, live drop on one track only.
//! * **A recovered crash recovers both tracks**, because the marker names both
//!   spills and both index sidecars.
//! * **A journal the SHIPPED build left behind still recovers.** Track 0's
//!   sidecar keeps its 22-A filename and the marker keeps its top-level `index`
//!   key precisely so a v0.8.0 crash finalizes correctly on this build; the
//!   marker in that test is hand-written, because one produced by the current
//!   writer would prove nothing about the one the shipped build writes.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use wilson_voice_lib::meeting::{
    recover_orphaned_meetings, IndexRecord, MeetingCapture, MeetingJournal, MeetingState,
    MIC_TRACK, SYSTEM_TRACK, TARGET_RATE,
};
use wilson_voice_lib::rtring::CaptureAnchor;

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yap-yv106-journal-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn wav_samples(path: &Path) -> Vec<i16> {
    let mut reader = hound::WavReader::open(path).expect("finalized wav opens");
    reader.samples::<i16>().filter_map(|s| s.ok()).collect()
}

/// A block of tone and the anchor a capture callback would have written for it.
/// Constant-amplitude audio is useless: the capture path high-passes, which
/// decays DC to zero and makes "audio" indistinguishable from a splice.
fn tone_block(index: u64, frames: usize, phase: f32) -> (Vec<f32>, [CaptureAnchor; 1]) {
    let start = index as usize * frames;
    let block: Vec<f32> = (0..frames)
        .map(|i| (((start + i) as f32) * phase).sin() * 0.6)
        .collect();
    (
        block,
        [CaptureAnchor {
            host_ns: index * 100_000_000,
            sample_index: index * frames as u64,
            frames: frames as u32,
            sample_rate: TARGET_RATE,
            lost_frames: 0,
        }],
    )
}

/// The longest run of exact zeros — the spliced hole, since a tone crosses zero
/// but never sits on it.
fn longest_silence(samples: &[i16]) -> usize {
    let (mut best, mut run) = (0usize, 0usize);
    for &s in samples {
        run = if s == 0 { run + 1 } else { 0 };
        best = best.max(run);
    }
    best
}

#[test]
fn two_tracks_round_trip_through_the_consumer_into_two_playable_wavs() {
    let dir = tmpdir("round-trip");
    let journal = MeetingJournal::start(&dir, 2).expect("journal opens");
    assert_eq!(journal.tracks(), 2);
    let capture = MeetingCapture::with_tracks(TARGET_RATE, 1, 2, Some(journal));
    assert_eq!(capture.tracks(), 2);

    // One second into each track, from two independent block streams — the mic
    // at one frequency, the tap at another, so a crossed track is visible in the
    // samples rather than only in the counters.
    const BLOCK: usize = 1_600; // 100 ms at 16 kHz
    for i in 0..10u64 {
        let (mic, mic_anchors) = tone_block(i, BLOCK, 0.05);
        capture.accept_track(MIC_TRACK, &mic, &mic_anchors);
        let (sys, sys_anchors) = tone_block(i, BLOCK, 0.31);
        capture.accept_track(SYSTEM_TRACK, &sys, &sys_anchors);
        std::thread::sleep(Duration::from_millis(2));
    }

    // Each track kept its own books.
    assert_eq!(capture.track_blocks(MIC_TRACK), 10);
    assert_eq!(capture.track_blocks(SYSTEM_TRACK), 10);
    assert!(capture.track_samples(MIC_TRACK) > 0);
    assert!(capture.track_samples(SYSTEM_TRACK) > 0);
    assert_eq!(
        capture.samples(),
        capture.track_samples(MIC_TRACK),
        "the MEETING's clock is the mic's — a tap must never be able to make a \
         dead microphone look like a running meeting"
    );

    let journal = capture.close().expect("the journal comes back");
    let finalized = journal.finalize(MeetingState::Complete).expect("finalized");
    assert_eq!(finalized.state, MeetingState::Complete);
    assert_eq!(finalized.spliced_silence_samples, 0);
    assert_eq!(finalized.tracks.len(), 2, "one wav per track");
    assert_eq!(finalized.track_numbers, vec![MIC_TRACK, SYSTEM_TRACK]);

    let mic_wav = finalized.wav_for_track(MIC_TRACK).expect("mic wav");
    let sys_wav = finalized.wav_for_track(SYSTEM_TRACK).expect("system wav");
    assert_ne!(mic_wav, sys_wav);
    let mic = wav_samples(mic_wav);
    let sys = wav_samples(sys_wav);
    assert!(
        mic.len().abs_diff(10 * BLOCK) <= 16,
        "the mic track is a second long: {}",
        mic.len()
    );
    assert!(
        sys.len().abs_diff(10 * BLOCK) <= 16,
        "the system track is a second long: {}",
        sys.len()
    );
    assert_ne!(
        mic, sys,
        "the two tracks carry DIFFERENT audio — identical content here would \
         mean one producer was written to both spills"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_loss_on_one_track_splices_that_track_only() {
    // THE reason the index had to become per-track, and the test that fails on
    // the shared-sidecar version of this code.
    //
    // Both producers run for the same three seconds, block for block, on the
    // same host clock. The TAP's ring refuses a second of audio partway through
    // (the anchors' cumulative `lost_frames` is the only witness, exactly as a
    // real overrun leaves it); the MIC loses nothing. With one shared index
    // sequence the finalize plans the tap's hole against the mic's counters as
    // well, and a track that lost nothing comes back a second longer with
    // silence punched into it — every timestamp after that point wrong, on the
    // one track the user is sure about.
    const BLOCK: usize = 1_600; // 100 ms at 16 kHz
    const LOST: u64 = TARGET_RATE as u64; // one second the tap's ring refused
    let dir = tmpdir("one-track-loss");
    let journal = MeetingJournal::start(&dir, 2).expect("journal opens");
    let capture = MeetingCapture::with_tracks(TARGET_RATE, 1, 2, Some(journal));

    for i in 0..30u64 {
        let (mic, a) = tone_block(i, BLOCK, 0.05);
        capture.accept_track(MIC_TRACK, &mic, &a);
        let (sys, mut b) = tone_block(i, BLOCK, 0.31);
        // Cumulative, and it stays cumulative after the loss — the consumer
        // advances its epoch total by the DELTA, which is what makes a single
        // overrun a single splice rather than one per block that follows it.
        b[0].lost_frames = if i >= 10 { LOST } else { 0 };
        capture.accept_track(SYSTEM_TRACK, &sys, &b);
        std::thread::sleep(Duration::from_millis(2));
    }

    // The divergence is the tap's alone, in the live counters, before any
    // finalize has run.
    // Sub-millisecond slack, because a streaming resampler withholds an output
    // whose right-hand neighbour has not arrived — the same phase the splice
    // planner ignores below `SPLICE_MIN_SAMPLES` (16).
    assert!(
        capture
            .track_captured_samples(MIC_TRACK)
            .abs_diff(capture.track_spilled_samples(MIC_TRACK))
            < 16,
        "the mic delivered what reached the disk: captured {}, spilled {}",
        capture.track_captured_samples(MIC_TRACK),
        capture.track_spilled_samples(MIC_TRACK)
    );
    assert!(
        capture
            .track_captured_samples(SYSTEM_TRACK)
            .saturating_sub(capture.track_spilled_samples(SYSTEM_TRACK))
            .abs_diff(LOST)
            < 16,
        "the tap's ring refused a second, and only the tap's: captured {}, spilled {}",
        capture.track_captured_samples(SYSTEM_TRACK),
        capture.track_spilled_samples(SYSTEM_TRACK)
    );

    let journal = capture.close().expect("journal");
    assert_eq!(
        journal.dropped_samples(),
        0,
        "the JOURNAL took everything — this loss is the ring's, on one track"
    );
    let finalized = journal.finalize(MeetingState::Partial).expect("finalized");
    assert!(
        finalized.spliced_silence_samples.abs_diff(LOST) < 16,
        "exactly the tap's loss was spliced — no more (the mic's records read \
         as the tap's, or the tap's read twice) and no less: {}",
        finalized.spliced_silence_samples
    );

    let mic = wav_samples(finalized.wav_for_track(MIC_TRACK).expect("mic wav"));
    let sys = wav_samples(finalized.wav_for_track(SYSTEM_TRACK).expect("system wav"));

    // The mic recorded 30 blocks and lost nothing, so its track is exactly 30
    // blocks long with no hole in it.
    assert!(
        mic.len().abs_diff(30 * BLOCK) <= 16,
        "the mic track was resized by the OTHER track's loss: got {}, want ~{}",
        mic.len(),
        30 * BLOCK
    );
    assert!(
        longest_silence(&mic) < BLOCK,
        "a hole was spliced into the mic track, which lost nothing: {} zero \
         samples in a row",
        longest_silence(&mic)
    );

    // The system track is held open at its true length, with the hole explicit.
    assert!(
        sys.len().abs_diff(30 * BLOCK + LOST as usize) <= 16,
        "the system track must be held open for what the device delivered: got {}",
        sys.len()
    );
    assert!(
        longest_silence(&sys) >= LOST as usize,
        "the tap's hole is one explicit stretch of silence: {}",
        longest_silence(&sys)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_journal_queue_drop_on_one_track_is_spliced_into_that_track() {
    // The other route to the same loss, through the LIVE bounded queue rather
    // than the ring — the failure `capture_journal_recovery.rs` produces for one
    // track, here with two tracks open. A depth-1 queue with a parked writer is
    // what makes the rejection deterministic; the pause window is short in wall
    // time, and only the tap is feeding during it, so the mic's own host clock
    // has no gap in it to splice.
    const BLOCK: usize = 1_600;
    let dir = tmpdir("queue-drop");
    let journal = MeetingJournal::start_with_depth(&dir, 2, 1).expect("journal opens");
    let pause = journal.pause_handle();
    let capture = MeetingCapture::with_tracks(TARGET_RATE, 1, 2, Some(journal));

    // The mic's blocks are contiguous in ITS OWN clock across the whole test —
    // it delivers 20 blocks, 100 ms apart, and never stops.
    let mut mic_i = 0u64;
    let mut feed_mic = |capture: &MeetingCapture| {
        let (mic, a) = tone_block(mic_i, BLOCK, 0.05);
        capture.accept_track(MIC_TRACK, &mic, &a);
        mic_i += 1;
    };

    for i in 0..10u64 {
        feed_mic(&capture);
        let (sys, b) = tone_block(i, BLOCK, 0.31);
        capture.accept_track(SYSTEM_TRACK, &sys, &b);
        std::thread::sleep(Duration::from_millis(5));
    }
    pause.store(true, std::sync::atomic::Ordering::SeqCst);
    for i in 10..20u64 {
        let (sys, b) = tone_block(i, BLOCK, 0.31);
        capture.accept_track(SYSTEM_TRACK, &sys, &b);
    }
    pause.store(false, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(20));
    for i in 20..30u64 {
        feed_mic(&capture);
        let (sys, b) = tone_block(i, BLOCK, 0.31);
        capture.accept_track(SYSTEM_TRACK, &sys, &b);
        std::thread::sleep(Duration::from_millis(5));
    }

    let journal = capture.close().expect("journal");
    let dropped = journal.dropped_samples();
    assert!(
        dropped > 0,
        "the depth-1 queue was supposed to reject something — nothing to detect otherwise"
    );
    let finalized = journal.finalize(MeetingState::Partial).expect("finalized");
    assert!(
        finalized.spliced_silence_samples >= dropped,
        "{dropped} sample(s) were dropped but only {} were spliced",
        finalized.spliced_silence_samples
    );

    let sys = wav_samples(finalized.wav_for_track(SYSTEM_TRACK).expect("system wav"));
    assert!(
        longest_silence(&sys) >= BLOCK,
        "the refused chunks are one explicit stretch of silence in the track \
         that lost them: {}",
        longest_silence(&sys)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_crash_mid_two_track_meeting_recovers_both_tracks() {
    let dir = tmpdir("orphan-two-track");
    let journal = MeetingJournal::start(&dir, 2).expect("journal opens");
    let mic: Vec<f32> = (0..TARGET_RATE as usize)
        .map(|i| ((i as f32) * 0.05).sin() * 0.6)
        .collect();
    let sys: Vec<f32> = (0..TARGET_RATE as usize)
        .map(|i| ((i as f32) * 0.31).sin() * 0.4)
        .collect();
    assert_eq!(journal.append(MIC_TRACK, &mic), mic.len());
    assert_eq!(journal.append(SYSTEM_TRACK, &sys), sys.len());
    journal.index_track(
        MIC_TRACK,
        IndexRecord {
            host_ns: 1_000_000_000,
            captured_samples: mic.len() as u64,
            spilled_samples: mic.len() as u64,
        },
    );
    journal.index_track(
        SYSTEM_TRACK,
        IndexRecord {
            host_ns: 1_000_000_000,
            captured_samples: sys.len() as u64,
            spilled_samples: sys.len() as u64,
        },
    );
    // The writer flushes on a 250 ms timer, so give it one before the "crash".
    std::thread::sleep(Duration::from_millis(400));
    journal.abandon();

    let recovered = recover_orphaned_meetings(&dir);
    assert_eq!(recovered.len(), 1);
    let meeting = &recovered[0];
    assert_eq!(
        meeting.state,
        MeetingState::Partial,
        "a meeting nobody stopped is partial"
    );
    assert_eq!(meeting.tracks.len(), 2, "both tracks were recovered");
    assert_eq!(meeting.track_numbers, vec![MIC_TRACK, SYSTEM_TRACK]);
    assert_eq!(
        meeting.spliced_silence_samples, 0,
        "neither track lost anything, so neither is spliced"
    );
    assert_eq!(
        wav_samples(meeting.wav_for_track(MIC_TRACK).unwrap()).len(),
        mic.len()
    );
    assert_eq!(
        wav_samples(meeting.wav_for_track(SYSTEM_TRACK).unwrap()).len(),
        sys.len()
    );
    assert!(
        recover_orphaned_meetings(&dir).is_empty(),
        "recovery is not repeatable — the marker is gone"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_journal_abandoned_by_the_shipped_22a_build_still_recovers() {
    // The upgrade path, and the reason track 0's sidecar keeps its old name.
    //
    // A user crashes on v0.8.0 and relaunches on the build this PR produces.
    // What is on disk is a 22-A marker: ONE track entry, no per-track `index`
    // key, and the index sidecar under the top-level `index` key. This test
    // writes that shape BY HAND — a marker produced by the current writer would
    // prove nothing about the one the shipped build left — and asserts the
    // finalize still splices it correctly.
    let dir = tmpdir("legacy-marker");
    let id = "22a-marker-fixture";
    let spill = dir.join(format!("{id}.t0.spill.pcm"));
    let index = dir.join(format!("{id}.index.jsonl"));
    let marker = dir.join(format!("{id}.meeting.in_progress.json"));

    // One second of tone as raw little-endian i16, exactly as the writer spills.
    let mut pcm: Vec<u8> = Vec::new();
    for i in 0..TARGET_RATE as usize {
        let s = (((i as f32) * 0.05).sin() * 0.6 * i16::MAX as f32) as i16;
        pcm.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(&spill, &pcm).unwrap();
    // Two records saying half a second of audio never reached the disk.
    std::fs::write(
        &index,
        format!(
            "{{\"host_ns\":1000000000,\"captured_samples\":8000,\"spilled_samples\":8000}}\n\
             {{\"host_ns\":2000000000,\"captured_samples\":24000,\"spilled_samples\":16000}}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        &marker,
        serde_json::json!({
            "version": 1,
            "kind": "meeting",
            "started_at": "2026-08-11T16:03:00+00:00",
            "sample_rate": TARGET_RATE,
            "state": "recording",
            "index": index.to_string_lossy(),
            "tracks": [{ "track": 0, "spill": spill.to_string_lossy() }],
        })
        .to_string(),
    )
    .unwrap();

    let recovered = recover_orphaned_meetings(&dir);
    assert_eq!(
        recovered.len(),
        1,
        "the 22-A marker was not recovered at all"
    );
    let meeting = &recovered[0];
    assert_eq!(meeting.state, MeetingState::Partial);
    assert_eq!(meeting.track_numbers, vec![MIC_TRACK]);
    assert_eq!(
        meeting.spliced_silence_samples, 8_000,
        "the top-level `index` key is still where track 0's records are read \
         from — a build that only looked for `{{id}}.t0.index.jsonl` would \
         recover this meeting half a second short and shift every timestamp \
         after it"
    );
    assert_eq!(
        wav_samples(meeting.wav_for_track(MIC_TRACK).unwrap()).len(),
        TARGET_RATE as usize + 8_000
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_mic_only_meeting_is_byte_for_byte_what_22a_produced() {
    // The regression guard. A one-track journal must still write ONE spill, ONE
    // index sidecar under its 22-A name, and finalize to ONE wav — this item
    // changed the journal's shape, and every recording already in the wild was
    // made by the version it changed.
    let dir = tmpdir("single-track-unchanged");
    let journal = MeetingJournal::start(&dir, 1).expect("journal opens");
    let id = journal.id().to_string();
    let capture = MeetingCapture::new(TARGET_RATE, 1, Some(journal));
    for i in 0..10u64 {
        let (block, anchors) = tone_block(i, 1_600, 0.05);
        capture.accept(&block, &anchors);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        dir.join(format!("{id}.index.jsonl")).exists(),
        "track 0's sidecar keeps 22-A's filename"
    );
    assert!(
        !dir.join(format!("{id}.t1.index.jsonl")).exists(),
        "a mic-only meeting has no second sidecar"
    );

    let journal = capture.close().expect("journal");
    let finalized = journal.finalize(MeetingState::Complete).expect("finalized");
    assert_eq!(finalized.tracks.len(), 1);
    assert_eq!(finalized.track_numbers, vec![MIC_TRACK]);
    assert!(finalized.wav_for_track(SYSTEM_TRACK).is_none());
    assert_eq!(finalized.spliced_silence_samples, 0);
    assert!(wav_samples(&finalized.tracks[0]).len().abs_diff(16_000) <= 16);
    let _ = std::fs::remove_dir_all(&dir);
}
