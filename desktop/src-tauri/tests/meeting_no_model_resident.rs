//! YV91 acceptance — **capture never brings an inference model resident and
//! never defeats the idle sweepers** (plan finding #27).
//!
//! The finding: no memory/battery/thermal preflight exists (only disk), and
//! nothing asserts that inference models stay unloaded during meeting capture —
//! on a fanless 8 GB M1 Air, which the plan names as the target machine. Its
//! prescribed fix is precisely a unit test on the session state machine:
//! *"capture start must not touch `TranscriptionManager::load` and must not
//! defeat the existing idle sweepers (`IDLE_UNLOAD_AFTER`,
//! `SIDECAR_IDLE_UNLOAD`)"*.
//!
//! Two counters make that provable from outside the module: `model_load_calls`
//! (every entry into `TranscriptionManager::load`) and `engine_touches` (every
//! push of the ASR idle sweeper's deadline, i.e. `touch`). A meeting that runs
//! a full lifecycle — preflight, start, capture, watchdog, stop, finalize —
//! must move neither.
//!
//! The sidecar half is asserted structurally: `SIDECAR_IDLE_UNLOAD` lives in
//! `polish.rs` and its sweeper is driven by the polish pool. The capture path
//! must not reference the polish module at all, and this file checks that
//! against the source rather than trusting it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use wilson_voice_lib::meeting::{
    fan_out_block, session_turnstile, ExternalStream, MeetingSession, SessionConfig,
    MEETING_QUEUE_DEPTH, TARGET_RATE,
};
use wilson_voice_lib::rtring::CaptureAnchor;
use wilson_voice_lib::{engine_touches, model_load_calls, IDLE_UNLOAD_AFTER};

const CAPTURE_SOURCE: &str = include_str!("../src/meeting.rs");
const RING_SOURCE: &str = include_str!("../src/rtring.rs");
const POWER_SOURCE: &str = include_str!("../src/power.rs");

fn turn() -> std::sync::MutexGuard<'static, ()> {
    // The library's turnstile, not a private one: `MeetingSession::start`
    // refuses a second concurrent session, so every suite that starts a
    // meeting has to queue behind the same lock the others take.
    session_turnstile()
}

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yap-yv91-resident-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
fn a_whole_meeting_lifecycle_never_asks_for_a_model() {
    let _turn = turn();
    let loads_before = model_load_calls();
    let touches_before = engine_touches();

    let dir = tmpdir("lifecycle");
    let session = MeetingSession::start(SessionConfig {
        // The blocks come from this test, not from a microphone — so the
        // session holds no stream of its own (YV91's `ExternalStream`).
        stream: Arc::new(ExternalStream),
        watchdog_interval: Duration::from_millis(10),
        ..SessionConfig::new(&dir, TARGET_RATE, 1)
    })
    .expect("a synthetic meeting starts");

    for i in 0..30u64 {
        let block = vec![0.3f32; TARGET_RATE as usize / 10];
        let anchors = [CaptureAnchor {
            host_ns: i * 100_000_000,
            sample_index: i * (TARGET_RATE as u64 / 10),
            frames: TARGET_RATE / 10,
            sample_rate: TARGET_RATE,
            lost_frames: 0,
        }];
        fan_out_block(&block, &anchors, None, None);
    }
    // Several watchdog ticks, so the periodic path is exercised too.
    std::thread::sleep(Duration::from_millis(60));
    let finalized = session.stop().expect("finalized");
    assert!(finalized.seconds > 2.9);

    assert_eq!(
        model_load_calls(),
        loads_before,
        "capture called TranscriptionManager::load — on an 8 GB fanless Air, a \
         resident model during a 3h recording is the whole of finding #27"
    );
    assert_eq!(
        engine_touches(),
        touches_before,
        "capture pushed the ASR idle sweeper's deadline out, which defeats \
         IDLE_UNLOAD_AFTER for as long as the meeting runs"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_idle_sweepers_are_still_the_ones_the_finding_names() {
    // A sweeper that got quietly disabled would make the test above pass for
    // the wrong reason.
    assert_eq!(IDLE_UNLOAD_AFTER, Duration::from_secs(15 * 60));
    let polish = include_str!("../src/polish.rs");
    assert!(
        polish.contains("const SIDECAR_IDLE_UNLOAD: Duration = Duration::from_secs(10 * 60)"),
        "SIDECAR_IDLE_UNLOAD is no longer a live 10-minute sweeper"
    );
}

#[test]
fn the_capture_path_does_not_reference_the_inference_stack_at_all() {
    // The structural half. `IDLE_UNLOAD_AFTER` / `SIDECAR_IDLE_UNLOAD` cannot be
    // defeated by code that never names the modules that own them.
    for (name, source) in [
        ("meeting.rs", CAPTURE_SOURCE),
        ("rtring.rs", RING_SOURCE),
        ("power.rs", POWER_SOURCE),
    ] {
        for forbidden in [
            "TranscriptionManager",
            "crate::transcription",
            "crate::polish",
            "crate::asr_engine",
            "IDLE_UNLOAD",
        ] {
            assert!(
                !source.contains(forbidden),
                "{name} references `{forbidden}` — the capture path must not be able to \
                 load a model or hold a sweeper open"
            );
        }
    }
}

#[test]
fn the_meeting_queue_is_deeper_than_a_dictation_s_but_still_bounded() {
    // Finding #12 asked for a deeper queue for meetings; finding #1 says
    // "bounded" is the point. Both, or the queue is just a slower leak.
    // YV63's dictation journal queue is 64 deep.
    assert!(
        (65..=4_096).contains(&MEETING_QUEUE_DEPTH),
        "meeting queue depth {MEETING_QUEUE_DEPTH} must be deeper than a dictation's 64          (finding #12) and still a bound rather than a buffer pool (finding #1)"
    );
}
