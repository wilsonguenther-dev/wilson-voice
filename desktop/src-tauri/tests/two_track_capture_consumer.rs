//! YV106 acceptance — **the CONSUMER, generalized to two tracks**.
//!
//! The journal was N-track from the day it shipped. `MeetingCaptureInner` was
//! not: it carried ONE set of DSP and epoch fields — resampler, high-pass,
//! `captured`/`spilled`, `captured_base`, `native_rate`, `native_frames`,
//! `lost_frames`, `anchor_lost`, the index cadence — because 22-A only ever had
//! one producer. This file drives two synthetic block streams (mic-shaped and
//! tap-shaped, at DIFFERENT native rates, which is the normal case: the mic is
//! clocked by the input device and the tap by the aggregate's main sub-device)
//! and asserts each track's state is genuinely its own.
//!
//! Every invariant asserted here is one `MeetingCapture` already held for the
//! single mic track. The item is that they now hold PER TRACK.

use std::path::PathBuf;
use std::time::Duration;

use wilson_voice_lib::meeting::{
    fan_out_block, fan_out_tap_block, session_turnstile, ExternalStream, MeetingCapture,
    MeetingJournal, MeetingSession, MeetingState, SessionConfig, MIC_TRACK, SYSTEM_TRACK,
    TARGET_RATE,
};
use wilson_voice_lib::rtring::CaptureAnchor;

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yap-yv106-consumer-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// One callback's worth of tone at `rate`, with the anchor that would have
/// carried it. `lost` is the anchor's CUMULATIVE lost-frame count, which is how
/// the real ring stamps it.
fn block_at(
    rate: u32,
    index: u64,
    frames: usize,
    phase: f32,
    lost: u64,
) -> (Vec<f32>, [CaptureAnchor; 1]) {
    let start = index as usize * frames;
    let block: Vec<f32> = (0..frames)
        .map(|i| (((start + i) as f32) * phase).sin() * 0.6)
        .collect();
    (
        block,
        [CaptureAnchor {
            host_ns: index * 1_000_000_000 * frames as u64 / rate as u64,
            sample_index: index * frames as u64,
            frames: frames as u32,
            sample_rate: rate,
            lost_frames: lost,
        }],
    )
}

#[test]
fn each_track_keeps_its_own_dsp_epoch_and_index_state() {
    // Two producers at two rates, which is the configuration that makes a
    // SHARED `native_rate` visibly wrong: `captured` is
    // `frames * 16000 / native_rate`, so one divisor for two devices mis-times
    // whichever track is not the one the divisor belongs to.
    const MIC_RATE: u32 = 48_000;
    const SYS_RATE: u32 = 44_100;
    const MIC_FRAMES: usize = 4_800; // 100 ms
    const SYS_FRAMES: usize = 4_410; // 100 ms

    let dir = tmpdir("epochs");
    let journal = MeetingJournal::start(&dir, 2).expect("journal");
    let capture = MeetingCapture::with_tracks(MIC_RATE, 1, 2, Some(journal));
    // The tap announces the format it actually got, exactly as YV100's tap does
    // once `kAudioTapPropertyFormat` is readable.
    capture.retune_track(SYSTEM_TRACK, SYS_RATE, 1);
    assert_eq!(capture.track_native_rate(MIC_TRACK), MIC_RATE);
    assert_eq!(
        capture.track_native_rate(SYSTEM_TRACK),
        SYS_RATE,
        "retuning track 1 must not have retuned track 0"
    );

    // Two seconds on each track. The MIC loses 4800 frames to a full ring
    // partway through (the anchor's cumulative counter is the only witness);
    // the TAP loses nothing.
    for i in 0..20u64 {
        let lost = if i >= 10 { MIC_FRAMES as u64 } else { 0 };
        let (mic, a) = block_at(MIC_RATE, i, MIC_FRAMES, 0.05, lost);
        capture.accept_track(MIC_TRACK, &mic, &a);
        let (sys, b) = block_at(SYS_RATE, i, SYS_FRAMES, 0.31, 0);
        capture.accept_track(SYSTEM_TRACK, &sys, &b);
        std::thread::sleep(Duration::from_millis(2));
    }

    // `captured` is 16 kHz-EQUIVALENT, so both tracks report ~2 s of delivery —
    // computed through each track's own divisor. The mic's includes the frames
    // the ring refused, which is the whole point of counting delivery rather
    // than arrival.
    let mic_captured = capture.track_captured_samples(MIC_TRACK);
    let sys_captured = capture.track_captured_samples(SYSTEM_TRACK);
    let two_seconds = 2 * TARGET_RATE as u64;
    assert!(
        mic_captured.abs_diff(two_seconds + TARGET_RATE as u64 / 10) <= 64,
        "the mic delivered 2 s plus the 100 ms the ring refused: {mic_captured}"
    );
    assert!(
        sys_captured.abs_diff(two_seconds) <= 64,
        "the tap delivered 2 s at ITS OWN rate: {sys_captured}"
    );

    // The loss is the mic's alone: `spilled` tracks what reached disk, and the
    // tap's two numbers still agree.
    assert!(
        mic_captured > capture.track_spilled_samples(MIC_TRACK),
        "the mic's ring overrun must show as a divergence"
    );
    assert!(
        sys_captured.abs_diff(capture.track_spilled_samples(SYSTEM_TRACK)) < 64,
        "the tap lost nothing, so its counters agree: captured {sys_captured}, \
         spilled {}",
        capture.track_spilled_samples(SYSTEM_TRACK)
    );

    // Each track wrote its own index records, on its own cadence.
    assert!(capture.track_index_records_written(MIC_TRACK) >= 2);
    assert!(capture.track_index_records_written(SYSTEM_TRACK) >= 2);

    let journal = capture.close().expect("journal");
    let finalized = journal.finalize(MeetingState::Partial).expect("finalized");
    assert_eq!(finalized.tracks.len(), 2);
    // The mic's 100 ms hole is spliced; the tap's track is untouched by it.
    assert!(
        finalized.spliced_silence_samples >= TARGET_RATE as u64 / 20,
        "the mic's ring overrun was not spliced: {}",
        finalized.spliced_silence_samples
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_retune_on_one_track_leaves_the_other_track_alone() {
    // YV92's epoch banking, per track. An AirPods swap retunes the MIC; YV103's
    // aggregate rebuild retunes the TAP. Neither may re-time the other's
    // already-recorded audio.
    let dir = tmpdir("retune");
    let journal = MeetingJournal::start(&dir, 2).expect("journal");
    let capture = MeetingCapture::with_tracks(48_000, 1, 2, Some(journal));

    for i in 0..10u64 {
        let (mic, a) = block_at(48_000, i, 4_800, 0.05, 0);
        capture.accept_track(MIC_TRACK, &mic, &a);
        let (sys, b) = block_at(48_000, i, 4_800, 0.31, 0);
        capture.accept_track(SYSTEM_TRACK, &sys, &b);
    }
    let sys_before = capture.track_captured_samples(SYSTEM_TRACK);
    let mic_before = capture.track_captured_samples(MIC_TRACK);

    // The mic's device changes to 16 kHz mid-meeting.
    capture.retune(16_000, 1);
    assert_eq!(capture.track_native_rate(MIC_TRACK), 16_000);
    assert_eq!(
        capture.track_native_rate(SYSTEM_TRACK),
        48_000,
        "the tap's device did not change"
    );
    assert_eq!(
        capture.track_captured_samples(SYSTEM_TRACK),
        sys_before,
        "retuning the mic must not move a single sample of the tap's history"
    );
    assert!(
        capture.track_captured_samples(MIC_TRACK) >= mic_before,
        "the mic's own history is banked, not discarded"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_tap_block_never_reaches_a_dictation_take() {
    // The fan-out asymmetry, as a test rather than as a comment. A mic block is
    // shared with an armed dictation take; a tap block is not, and there is no
    // parameter that makes it so — the other people on the call must never be
    // pasteable into the user's document.
    let _turn = session_turnstile();
    let dir = tmpdir("fanout");
    let session = MeetingSession::start(SessionConfig {
        stream: std::sync::Arc::new(ExternalStream),
        watchdog_interval: Duration::from_secs(3_600),
        ..SessionConfig::virtual_meeting(&dir, TARGET_RATE, 1)
    })
    .expect("a synthetic virtual meeting starts");
    assert_eq!(
        session.capture().tracks(),
        2,
        "virtual meetings are 2-track"
    );

    let mut take: Vec<f32> = Vec::new();
    let (mic, a) = block_at(TARGET_RATE, 0, 1_600, 0.05, 0);
    fan_out_block(&mic, &a, Some(&mut take), None);
    assert_eq!(take.len(), mic.len(), "the mic block reached the take");

    let (sys, b) = block_at(TARGET_RATE, 0, 1_600, 0.31, 0);
    fan_out_tap_block(&sys, &b);
    assert_eq!(
        take.len(),
        mic.len(),
        "the tap block must not be reachable from any dictation sink"
    );
    assert!(session.capture().track_samples(SYSTEM_TRACK) > 0);

    let finalized = session.stop().expect("finalizes");
    assert_eq!(finalized.tracks.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_tap_block_into_a_mic_only_meeting_is_dropped_not_folded_into_track_zero() {
    // A 22-A (one-track) meeting with a tap somehow running — a rebuild landing
    // a beat after the user stopped a virtual meeting and started an in-person
    // one. Mixing the room into "Me" would be a silent, unfixable corruption of
    // the one track the user is sure about.
    let dir = tmpdir("mic-only");
    let journal = MeetingJournal::start(&dir, 1).expect("journal");
    let capture = MeetingCapture::new(TARGET_RATE, 1, Some(journal));
    let (mic, a) = block_at(TARGET_RATE, 0, 1_600, 0.05, 0);
    capture.accept(&mic, &a);
    let mic_samples = capture.track_samples(MIC_TRACK);

    let (sys, b) = block_at(TARGET_RATE, 0, 1_600, 0.31, 0);
    capture.accept_track(SYSTEM_TRACK, &sys, &b);
    assert_eq!(
        capture.track_samples(MIC_TRACK),
        mic_samples,
        "a block for a track this meeting does not have was folded into track 0"
    );
    assert_eq!(capture.track_samples(SYSTEM_TRACK), 0);

    let journal = capture.close().expect("journal");
    let finalized = journal.finalize(MeetingState::Complete).expect("finalized");
    assert_eq!(finalized.tracks.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
