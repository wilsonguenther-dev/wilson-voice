//! YV91 acceptance — **a dictation hotkey mid-meeting produces a normal
//! dictation clip and leaves no gap in the meeting audio** (plan finding #2a).
//!
//! The finding, verified against the repo before the panel merged it: there is
//! exactly one capture stream, and `LiveStream::begin` calls `dsp.reset()`,
//! which is `*self = Self::new(...)` — destructive. Reuse that path for a
//! meeting and a dictation hotkey pressed at minute 40 silently wipes the forty
//! minutes recorded so far.
//!
//! The fix is not "guard the reset". It is that meeting capture became a
//! long-lived FAN-OUT on the shared stream rather than a second exclusive Arm:
//! one drained block is served to the meeting and to the dictation take, and
//! the meeting's audio never lives in `StreamDsp` at all — so there is nothing
//! left for a reset to destroy. These tests assert the observable consequences
//! of that shape, which is what "no gap" means concretely:
//!
//! * the meeting's sample count grows by exactly the same amount per block
//!   whether a dictation is armed or not;
//! * the meeting's on-disk index records show no divergence between what the
//!   device delivered and what reached the spill across the dictation;
//! * the dictation take gets its own audio, and only the frames inside its
//!   window.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use wilson_voice_lib::meeting::{
    fan_out_block, meeting_capture_active, session_turnstile, take_context, ExternalStream,
    MeetingSession, MeetingState, SessionConfig, TakeContext,
};
use wilson_voice_lib::rtring::CaptureAnchor;

const RATE: u32 = 16_000;
/// 100 ms callbacks, so "minute 40" is expressible in a fast test.
const BLOCK: usize = RATE as usize / 10;

/// The active-meeting slot is process-global, so these tests take turns.
fn turn() -> std::sync::MutexGuard<'static, ()> {
    // The library's turnstile, not a private one: `MeetingSession::start`
    // refuses a second concurrent session, so every suite that starts a
    // meeting has to queue behind the same lock the others take.
    session_turnstile()
}

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yap-yv91-fanout-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn anchor(block_index: u64) -> Vec<CaptureAnchor> {
    vec![CaptureAnchor {
        host_ns: block_index * 100_000_000,
        sample_index: block_index * BLOCK as u64,
        frames: BLOCK as u32,
        sample_rate: RATE,
        lost_frames: 0,
    }]
}

/// A block whose sample value identifies which block it is, so the dictation
/// take's contents can be checked against the window it was armed for.
fn block(block_index: u64) -> Vec<f32> {
    vec![block_index as f32 / 1_000.0; BLOCK]
}

#[test]
fn a_dictation_taken_mid_meeting_leaves_the_meeting_without_a_gap() {
    let _turn = turn();
    let dir = tmpdir("mid-meeting");
    let session = MeetingSession::start(SessionConfig {
        // The blocks come from this test, not from a microphone — so the
        // session holds no stream of its own (YV91's `ExternalStream`).
        stream: Arc::new(ExternalStream),
        watchdog_interval: Duration::from_secs(3_600),
        ..SessionConfig::new(&dir, RATE, 1)
    })
    .expect("a synthetic meeting starts");

    assert!(meeting_capture_active());
    assert_eq!(
        take_context(),
        TakeContext::DictationDuringMeeting,
        "a hotkey pressed now is a MID-MEETING dictation, which is what decides \
         both the mute policy and the fan-out"
    );

    let mut index = 0u64;
    // Thirty seconds of meeting before anybody touches the hotkey.
    for _ in 0..300 {
        fan_out_block(&block(index), &anchor(index), None, None);
        index += 1;
    }
    let before = session.capture().samples();
    let blocks_before = session.capture().blocks();

    // The hotkey: a dictation take runs for five seconds, on the same stream.
    let mut take: Vec<f32> = Vec::new();
    let first_dictation_block = index;
    for _ in 0..50 {
        fan_out_block(&block(index), &anchor(index), Some(&mut take), None);
        index += 1;
    }
    let during = session.capture().samples();

    // …and thirty more seconds of meeting after it ends.
    for _ in 0..300 {
        fan_out_block(&block(index), &anchor(index), None, None);
        index += 1;
    }
    let after = session.capture().samples();

    // The meeting kept every sample, at the same rate, throughout.
    assert_eq!(
        during - before,
        50 * BLOCK as u64,
        "the meeting lost samples while a dictation was armed"
    );
    assert_eq!(
        after - during,
        300 * BLOCK as u64,
        "the meeting lost samples after the dictation ended (a reset would show here)"
    );
    assert_eq!(session.capture().blocks() - blocks_before, 350);

    // The dictation take got its own audio — the frames from its window, and
    // only those.
    assert_eq!(take.len(), 50 * BLOCK);
    assert!(
        (take[0] - first_dictation_block as f32 / 1_000.0).abs() < 1e-6,
        "the take starts at the block the hotkey was pressed on"
    );

    // And the meeting's own paper trail says there was no gap.
    let finalized = session.stop().expect("the meeting finalizes");
    assert_eq!(finalized.state, MeetingState::Complete);
    assert_eq!(
        finalized.spliced_silence_samples, 0,
        "the index records show no divergence between captured and spilled audio, \
         i.e. no gap at the dictation's timestamp"
    );
    assert!(finalized.tracks[0].exists());
    // 65 seconds of meeting, not 60 (the dictation's five seconds are IN it).
    assert!(
        (finalized.seconds - 65.0).abs() < 0.5,
        "meeting length was {:.2}s",
        finalized.seconds
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dictation_with_no_meeting_running_is_an_ordinary_dictation() {
    let _turn = turn();
    // The other half of the fan-out contract: with nothing registered, a block
    // goes only where it always went. Nothing about the dictation path changed
    // for the user who never records a meeting.
    assert!(!meeting_capture_active());
    assert_eq!(take_context(), TakeContext::Dictation);
    let mut take: Vec<f32> = Vec::new();
    for i in 0..10 {
        fan_out_block(&block(i), &anchor(i), Some(&mut take), None);
    }
    assert_eq!(take.len(), 10 * BLOCK);
}

#[test]
fn the_take_receives_only_the_frames_inside_its_arm_window() {
    let _turn = turn();
    // The consumer hands the dictation sink a RANGE, not the whole block: a
    // take that arms halfway through a callback must not be given the audio
    // from before the key went down (and, at the other end, must not lose the
    // tail still in the ring when it comes up).
    let mut take: Vec<f32> = Vec::new();
    let whole: Vec<f32> = (0..8).map(|i| i as f32).collect();
    fan_out_block(&whole, &[], Some(&mut take), Some(3..8));
    assert_eq!(take, vec![3.0, 4.0, 5.0, 6.0, 7.0]);

    // An out-of-bounds range is a bug upstream, and it degrades to the whole
    // block rather than panicking on the capture path.
    let mut take: Vec<f32> = Vec::new();
    fan_out_block(&whole, &[], Some(&mut take), Some(0..99));
    assert_eq!(take.len(), whole.len());
}
