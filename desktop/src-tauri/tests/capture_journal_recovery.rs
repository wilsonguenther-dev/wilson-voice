//! YV91 acceptance — **N-track journal round-trip and orphan recovery** (matrix
//! rows #4 and #6, plan finding #12), plus the preflight refusal that carries
//! the exact free-space number.
//!
//! YV63 gave a dictation take a crash journal: frames spill to disk as they
//! arrive, next to an `.in_progress.json` marker whose presence at the next
//! startup IS the crash signal. A meeting needs the same thing generalized in
//! two directions:
//!
//! * **N tracks.** One for 22-A; the two-track shape is what 22-B's system-audio
//!   tap needs, and building it now costs nothing.
//! * **Index records.** A meeting's bounded queue can drop a chunk under a disk
//!   hiccup, and in a five-second dictation that is invisible and harmless.
//!   Over three hours it silently shifts every timestamp after it — and with
//!   them every speaker label and every evidence reference downstream. So the
//!   journal writes a periodic record of "what the device delivered" against
//!   "what reached the spill", and a recovery splices explicit silence for the
//!   difference instead of producing a shorter, wrongly-timed track.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use wilson_voice_lib::meeting::{
    fan_out_block, plan_silence_splices, preflight, recover_orphaned_meetings, required_free_bytes,
    rt_capture_callback, session_turnstile, BatterySnapshot, ExternalStream, IndexRecord,
    MeetingCapture, MeetingJournal, MeetingSession, MeetingState, PreflightError, PreflightInputs,
    RtCapture, SessionConfig, Splice, ThermalState, INDEX_INTERVAL, MEETING_HARD_CAP, RING_FRAMES,
    TARGET_RATE,
};
use wilson_voice_lib::rtring::CaptureAnchor;

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yap-yv91-journal-{tag}-{}-{}",
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

#[test]
fn one_track_round_trips_from_spill_to_wav() {
    let dir = tmpdir("round-trip");
    let journal = MeetingJournal::start(&dir, 1).expect("journal opens");
    let tone: Vec<f32> = (0..TARGET_RATE as usize * 2)
        .map(|i| ((i as f32) * 0.01).sin() * 0.5)
        .collect();
    for chunk in tone.chunks(1_600) {
        assert_eq!(journal.append(0, chunk), chunk.len());
    }
    journal.index(IndexRecord {
        host_ns: 1_000_000_000,
        captured_samples: tone.len() as u64,
        spilled_samples: tone.len() as u64,
    });

    let finalized = journal.finalize(MeetingState::Complete).expect("finalized");
    assert_eq!(finalized.state, MeetingState::Complete);
    assert_eq!(finalized.tracks.len(), 1);
    assert_eq!(finalized.spliced_silence_samples, 0);
    let samples = wav_samples(&finalized.tracks[0]);
    assert_eq!(
        samples.len(),
        tone.len(),
        "every sample survived the journal"
    );
    assert!((finalized.seconds - 2.0).abs() < 0.01);
    // Nothing in-progress is left to be "recovered" on the next launch.
    assert!(recover_orphaned_meetings(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn two_tracks_round_trip_because_22b_reuses_this_journal() {
    let dir = tmpdir("n-track");
    let journal = MeetingJournal::start(&dir, 2).expect("journal opens");
    let a: Vec<f32> = vec![0.5; TARGET_RATE as usize];
    let b: Vec<f32> = vec![-0.25; TARGET_RATE as usize];
    assert_eq!(journal.append(0, &a), a.len());
    assert_eq!(journal.append(1, &b), b.len());
    let finalized = journal.finalize(MeetingState::Complete).expect("finalized");
    assert_eq!(
        finalized.tracks.len(),
        2,
        "one wav per track, in track order"
    );
    let track_a = wav_samples(&finalized.tracks[0]);
    let track_b = wav_samples(&finalized.tracks[1]);
    assert_eq!(track_a.len(), a.len());
    assert_eq!(track_b.len(), b.len());
    assert!(track_a[0] > 0 && track_b[0] < 0, "the tracks did not cross");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_app_that_dies_mid_meeting_is_recovered_as_partial() {
    let dir = tmpdir("orphan");
    let journal = MeetingJournal::start(&dir, 1).expect("journal opens");
    let audio: Vec<f32> = vec![0.4; TARGET_RATE as usize];
    assert_eq!(journal.append(0, &audio), audio.len());
    journal.index(IndexRecord {
        host_ns: 1_000_000_000,
        captured_samples: audio.len() as u64,
        spilled_samples: audio.len() as u64,
    });
    // The writer flushes on a 250 ms timer (finding #12: a timer, not every
    // callback), so give it one before simulating the crash.
    std::thread::sleep(Duration::from_millis(400));
    journal.abandon();

    let recovered = recover_orphaned_meetings(&dir);
    assert_eq!(recovered.len(), 1);
    assert_eq!(
        recovered[0].state,
        MeetingState::Partial,
        "a meeting nobody stopped is partial — that is the state YV94's row stores"
    );
    assert_eq!(wav_samples(&recovered[0].tracks[0]).len(), audio.len());
    assert!(
        recover_orphaned_meetings(&dir).is_empty(),
        "recovery is not repeatable — the marker is gone"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A block of tone, and the anchor the capture callback would have written for
/// it. Constant-amplitude audio is useless here: the capture path high-passes,
/// which decays DC to zero and makes "audio" indistinguishable from a splice.
fn tone_block(index: u64, frames: usize) -> (Vec<f32>, [CaptureAnchor; 1]) {
    let start = index as usize * frames;
    let block: Vec<f32> = (0..frames)
        .map(|i| (((start + i) as f32) * 0.05).sin() * 0.6)
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
fn a_dropped_chunk_is_spliced_as_silence_not_swallowed() {
    // Finding #12's failure, end to end: the journal loses audio, and the
    // finalize must produce a track of the RIGHT LENGTH with an explicit hole,
    // not a shorter track whose every later timestamp is wrong.
    //
    // The drop is produced by the LIVE path — a journal with a depth-1 queue
    // whose writer is parked, so `MeetingJournal::append` really is rejected
    // and `MeetingCapture::accept` really does have to notice. Writing the two
    // index records by hand instead would assert only that this test can do
    // arithmetic: the capture path used to advance `spilled` by what it HANDED
    // the journal rather than by what the journal took, so the live numbers
    // always agreed, no splice was ever planned, and a hand-written test passed
    // over the top of the bug it was written to catch.
    const BLOCK: usize = 1_600; // 100 ms at 16 kHz
    let dir = tmpdir("dropped");
    let journal = MeetingJournal::start_with_depth(&dir, 1, 1).expect("journal opens");
    let pause = journal.pause_handle();
    let capture = MeetingCapture::new(TARGET_RATE, 1, Some(journal));

    // A second of audio that lands. Paced like a real callback (100 ms of
    // audio per block) so a depth-1 queue keeps up with it.
    for i in 0..10 {
        let (block, anchors) = tone_block(i, BLOCK);
        capture.accept(&block, &anchors);
        std::thread::sleep(Duration::from_millis(5));
    }
    // …a second the parked writer's bounded queue refuses…
    pause.store(true, Ordering::SeqCst);
    // The queue still holds `depth` messages, so let it fill before the drops.
    for i in 10..20 {
        let (block, anchors) = tone_block(i, BLOCK);
        capture.accept(&block, &anchors);
    }
    pause.store(false, Ordering::SeqCst);
    // …and a second that lands again, whose timestamps must not shift.
    std::thread::sleep(Duration::from_millis(20));
    for i in 20..30 {
        let (block, anchors) = tone_block(i, BLOCK);
        capture.accept(&block, &anchors);
        std::thread::sleep(Duration::from_millis(5));
    }

    let journal = capture
        .close()
        .expect("the journal comes back to be finalized");
    let dropped = journal.dropped_samples();
    assert!(
        dropped > 0,
        "the depth-1 queue was supposed to reject something — nothing to detect otherwise"
    );
    let finalized = journal.finalize(MeetingState::Partial).expect("finalized");

    assert!(
        finalized.spliced_silence_samples >= dropped,
        "{} sample(s) were dropped but only {} were spliced — the gap detector \
         is reading numbers that hid the loss",
        dropped,
        finalized.spliced_silence_samples
    );
    let samples = wav_samples(&finalized.tracks[0]);
    let expected = 30 * BLOCK;
    assert!(
        samples.len().abs_diff(expected) <= 16,
        "three seconds of wall clock must produce three seconds of track: got {} samples, want ~{expected}",
        samples.len()
    );
    let zeros = samples.iter().filter(|&&s| s == 0).count();
    assert!(
        zeros >= dropped as usize,
        "{dropped} dropped sample(s) but only {zeros} silent ones — the track was \
         shortened somewhere instead of held open"
    );
    assert!(
        longest_silence(&samples) >= BLOCK,
        "the hole is one explicit stretch of silence where the queue refused"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_ring_overrun_is_spliced_as_silence_too() {
    // The second, independent route to the same invisible loss: the RT ring
    // refuses frames the device really delivered. The anchor is the only
    // witness — its `sample_index` counts DELIVERED frames and its
    // `lost_frames` counts what did not fit — so a full ring has to reach the
    // index records exactly as a full journal queue does.
    //
    // Before this, `RtCapture` advanced its frame counter by the ACCEPTED
    // count, so every anchor after an overrun was shifted down by precisely the
    // amount lost, `captured` collapsed onto `spilled`, and the loss vanished.
    let dir = tmpdir("overrun");
    let rt = RtCapture::new(TARGET_RATE, 1);
    // One callback larger than the whole ring: the tail cannot fit.
    let over = RING_FRAMES + 8_000;
    let block: Vec<f32> = (0..over).map(|i| ((i as f32) * 0.05).sin() * 0.6).collect();
    rt_capture_callback(&rt, &block, |s| s, 1_000);
    assert_eq!(
        rt.frames_delivered(),
        over as u64,
        "the device delivered all of it"
    );
    assert_eq!(rt.frames_lost(), 8_000, "and 8000 frames did not fit");

    let journal = MeetingJournal::start(&dir, 1).expect("journal opens");
    let capture = MeetingCapture::new(TARGET_RATE, 1, Some(journal));
    let mut drained: Vec<f32> = Vec::with_capacity(RING_FRAMES);
    let mut anchors: Vec<CaptureAnchor> = Vec::new();
    rt.samples.drain_into(&mut drained);
    rt.anchors.drain_into(&mut anchors);
    capture.accept(&drained, &anchors);

    let journal = capture.close().expect("journal");
    assert_eq!(journal.dropped_samples(), 0, "the JOURNAL took everything");
    let finalized = journal.finalize(MeetingState::Partial).expect("finalized");
    assert!(
        finalized.spliced_silence_samples >= 8_000,
        "a ring overrun of 8000 frames spliced only {} samples of silence",
        finalized.spliced_silence_samples
    );
    let samples = wav_samples(&finalized.tracks[0]);
    assert!(
        samples.len().abs_diff(over) <= 16,
        "the finalized track is as long as the device's audio: got {}, want ~{over}",
        samples.len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_device_that_stalls_mid_meeting_is_spliced_not_shifted() {
    // One meeting at a time, process-wide: `MeetingSession::start` refuses a
    // second concurrent session, so the suites take the same turnstile.
    let _turn = session_turnstile();
    // The third route to lost audio, and the one the other two are blind to:
    // the device stops delivering. Nothing errors, nothing overflows — the
    // callback simply stops firing, so `captured` and `spilled` freeze together
    // and AGREE. A gap detector reading only those two numbers reports a
    // perfect recording of a meeting with a five-second hole in it, and every
    // timestamp after the hole is wrong.
    //
    // Before the host-clock rule, this exact feed finalized as "6.00s,
    // complete, spliced 0". The meeting really lasted eleven seconds.
    const BLOCK: usize = 1_600; // 100 ms at 16 kHz
    const STALL_NS: u64 = 5_000_000_000;
    let dir = tmpdir("stall");
    let session = MeetingSession::start(SessionConfig {
        // Synthetic blocks from this test, so there is no microphone to hold.
        stream: Arc::new(ExternalStream),
        // The watchdog's own stall rule is unit-tested; this test is about what
        // the FINALIZED track says, so keep the timer out of the way.
        watchdog_interval: Duration::from_secs(3_600),
        ..SessionConfig::new(&dir, TARGET_RATE, 1)
    })
    .expect("a synthetic meeting starts");

    // A tone, because the capture path high-passes: constant-amplitude "audio"
    // decays to zero and becomes indistinguishable from a spliced hole.
    let feed = |index: u64, host_offset_ns: u64| {
        let start = index as usize * BLOCK;
        let block: Vec<f32> = (0..BLOCK)
            .map(|i| (((start + i) as f32) * 0.05).sin() * 0.6)
            .collect();
        let anchors = [CaptureAnchor {
            host_ns: index * 100_000_000 + host_offset_ns,
            sample_index: index * BLOCK as u64,
            frames: BLOCK as u32,
            sample_rate: TARGET_RATE,
            lost_frames: 0,
        }];
        fan_out_block(&block, &anchors, None, None);
    };

    // Three seconds of audio…
    for i in 0..30u64 {
        feed(i, 0);
    }
    // …five seconds where the device delivers nothing at all (no blocks, no
    // anchors, no callbacks — the host clock is the only thing still moving)…
    // …and three more seconds, which the user experienced at t=8s, not t=3s.
    for i in 30..60u64 {
        feed(i, STALL_NS);
    }

    let finalized = session.stop().expect("the meeting finalizes");

    let stall_samples = STALL_NS * TARGET_RATE as u64 / 1_000_000_000;
    assert_eq!(
        finalized.spliced_silence_samples, stall_samples,
        "five seconds of dead device must become five seconds of explicit \
         silence — {} sample(s) were spliced",
        finalized.spliced_silence_samples
    );
    assert_eq!(
        finalized.state,
        MeetingState::Partial,
        "a meeting with a hole in it is not `complete`, however cleanly it was \
         stopped — `complete` has to mean nothing was lost"
    );

    let samples = wav_samples(&finalized.tracks[0]);
    let expected = 60 * BLOCK + stall_samples as usize;
    assert!(
        samples.len().abs_diff(expected) <= 16,
        "eleven seconds of wall clock must produce an eleven-second track: got \
         {} samples, want ~{expected}",
        samples.len()
    );
    assert!(
        (finalized.seconds - 11.0).abs() < 0.05,
        "the meeting's duration was {:.2}s",
        finalized.seconds
    );

    // The hole is one explicit stretch, and it sits where the device died —
    // within one index record of it, which is the resolution the index cadence
    // buys (a record is written about once a second, so a stall is placed to
    // the record boundary before it, never later than the stall itself).
    let hole = longest_silence(&samples);
    assert!(
        hole >= stall_samples as usize,
        "the silence is one run of {hole} sample(s), want at least {stall_samples}"
    );
    let start = samples
        .windows(stall_samples as usize)
        .position(|w| w.iter().all(|&s| s == 0))
        .expect("the spliced hole is in the track");
    let truth = 30 * BLOCK;
    let slack = INDEX_INTERVAL.as_secs() as usize * TARGET_RATE as usize;
    assert!(
        start <= truth && truth - start <= slack,
        "the hole starts at sample {start}; the device died at {truth}, and the \
         index cadence bounds the error to {slack}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_gap_planner_is_pure_and_ignores_sub_millisecond_resampler_phase() {
    // A gap of a few samples is the streaming resampler withholding an output
    // whose right-hand neighbour has not arrived. Splicing for that would make
    // the detector cry wolf on every healthy recording.
    let phase = [
        IndexRecord {
            host_ns: 1,
            captured_samples: 16_000,
            spilled_samples: 15_999,
        },
        IndexRecord {
            host_ns: 2,
            captured_samples: 32_000,
            spilled_samples: 31_999,
        },
    ];
    assert!(plan_silence_splices(&phase).is_empty());

    let real = [
        IndexRecord {
            host_ns: 1,
            captured_samples: 16_000,
            spilled_samples: 16_000,
        },
        IndexRecord {
            host_ns: 2,
            captured_samples: 32_000,
            spilled_samples: 24_000,
        },
    ];
    assert_eq!(
        plan_silence_splices(&real),
        vec![Splice {
            at_sample: 16_000,
            silence_samples: 8_000
        }]
    );
}

#[test]
fn a_synthetic_low_disk_value_refuses_with_the_exact_free_space_number() {
    // Matrix row #4's first half. The number is the point: "not enough disk
    // space" with no figure is a refusal the user cannot act on.
    let free: u64 = 1_500_000_000;
    let err = preflight(&PreflightInputs {
        free_bytes: free,
        duration_estimate: None,
        battery: BatterySnapshot::default(),
        thermal: ThermalState::Nominal,
    })
    .expect_err("1.5 GB is under the 2 GB headroom alone");
    let PreflightError::NotEnoughDisk {
        free_bytes,
        required_bytes,
        budgeted,
    } = err.clone()
    else {
        panic!("expected a disk refusal, got {err:?}");
    };
    assert_eq!(free_bytes, free);
    assert_eq!(budgeted, MEETING_HARD_CAP);
    assert_eq!(required_bytes, required_free_bytes(MEETING_HARD_CAP));

    let message = err.to_string();
    assert!(
        message.contains(&free.to_string()),
        "the exact free-space number is missing from: {message}"
    );
    assert!(
        message.contains(&required_bytes.to_string()),
        "the exact requirement is missing from: {message}"
    );
    assert!(
        message.contains("3h 00m"),
        "the budgeted duration is missing from: {message}"
    );
}
