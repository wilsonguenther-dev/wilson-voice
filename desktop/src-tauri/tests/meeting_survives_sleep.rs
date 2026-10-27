//! yap24-NT10 — a meeting survives a lid close or sleep.
//!
//! `NSWorkspaceWillSleepNotification` (PR #239's observer) does not delay
//! sleep: macOS posts it and goes down whether or not the meeting finished
//! finalizing. The root power domain does wait. `power.rs` now registers with
//! `IORegisterForSystemPower`, and on `kIOMessageSystemWillSleep` publishes
//! `WillSleep` to the same `SleepWakeHub` FIRST and calls `IOAllowPowerChange`
//! only AFTER every consumer has returned.
//!
//! These tests inject `kIOMessageSystemWillSleep` through
//! `power::on_power_message` — the one function the registered C callback
//! calls — with a REAL `MeetingController` subscribed exactly the way `lib.rs`
//! subscribes it, a real SQLite file, a fake capture engine that lands a real
//! two-track WAV pair, and the REAL transcription pipeline worker behind a stub
//! decoder. They assert:
//!   - mid-recording: the row ends `partial` with "Stopped when the Mac went to
//!     sleep.", both WAV paths are on the row and on disk, the pipeline picked
//!     it up (segments on both tracks, still `partial` with the sentence), and
//!     the acknowledgement ran only after the meeting had stopped;
//!   - no meeting: a no-op that still acknowledges;
//!   - the messages IOPMLib.h says must not be acknowledged are not.
//!
//! What this does NOT prove: that macOS delivers the message on a real lid
//! close (IOKit's contract, not reproducible in-process), or that the main run
//! loop of the running app services the notification port — the source is
//! added to `CFRunLoopGetMain()` in `power.rs`, and the registration itself is
//! checked live below.

mod support;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use support::{open_db, temp_dir};
use wilson_voice_lib::asr_engine::{TimedKind, TimedSpan, TimedTranscript};
use wilson_voice_lib::db::Database;
use wilson_voice_lib::meeting_asr::{ChunkAsr, ChunkConfig, MeetingAsrConfig, NoDemand};
use wilson_voice_lib::meeting_control::{
    install_capture_engine, ActiveCapture, CaptureEngine, CaptureOutcome, MeetingController,
    MeetingStatus, SleepStep, StatusSink,
};
use wilson_voice_lib::meeting_energy::MeetingDiagnostics;
use wilson_voice_lib::meeting_matrix::{
    SleepEvent, SleepPolicy, SLEEP_STOP_NOTE, SLEEP_STOP_REASON,
};
use wilson_voice_lib::meeting_pipeline::{
    self, MeetingEngine, MeetingProgressEvent, PipelineDeps, MIC_TRACK, SYSTEM_TRACK,
};
use wilson_voice_lib::meetings::{MeetingKind, MeetingState};
use wilson_voice_lib::power::{
    self, PowerReply, SleepWakeHub, CONSUMER_MEETINGS, IO_MESSAGE_CAN_SYSTEM_SLEEP,
    IO_MESSAGE_SYSTEM_HAS_POWERED_ON, IO_MESSAGE_SYSTEM_WILL_NOT_SLEEP,
    IO_MESSAGE_SYSTEM_WILL_POWER_ON, IO_MESSAGE_SYSTEM_WILL_SLEEP,
};

const RATE: u32 = 16_000;
const SECONDS: f64 = 6.0;

fn write_wav(path: &Path, hz: f32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("create wav");
    let n = (SECONDS * RATE as f64) as usize;
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let v = (t * hz * std::f32::consts::TAU).sin() * 0.3;
        w.write_sample((v * i16::MAX as f32) as i16).unwrap();
    }
    w.finalize().unwrap();
}

/// A word every 0.5 s of whatever window it is handed.
struct StubAsr;

impl ChunkAsr for StubAsr {
    fn transcribe_timed(&self, samples: &[f32]) -> Result<TimedTranscript, String> {
        let len = samples.len() as f64 / RATE as f64;
        let mut words = Vec::new();
        let mut t = 0.05;
        while t + 0.3 < len {
            words.push(TimedSpan {
                start_seconds: t,
                end_seconds: t + 0.3,
                text: "word".into(),
            });
            t += 0.5;
        }
        let text = words
            .iter()
            .map(|w| w.text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        Ok(TimedTranscript {
            text,
            kind: TimedKind::Word,
            segments: Vec::new(),
            words,
        })
    }
}

fn deps(db: Arc<Database>, dir: &Path) -> PipelineDeps {
    PipelineDeps {
        db,
        progress_dir: dir.to_path_buf(),
        asr: Arc::new(|| Ok(MeetingEngine::new("stub-asr", Box::new(StubAsr)))),
        demand: Arc::new(NoDemand),
        quit: Arc::new(|| false),
        sink: Arc::new(|_: &MeetingProgressEvent| {}),
        after_transcript: None,
        config: MeetingAsrConfig {
            chunk: ChunkConfig {
                target_seconds: 2.0,
                min_seconds: 1.5,
                max_seconds: 2.5,
                overlap_seconds: 0.5,
                min_silence_seconds: 0.2,
            },
            yield_poll: Duration::from_millis(1),
            ..MeetingAsrConfig::default()
        },
        start_delay: Duration::ZERO,
    }
}

// ── a capture engine that lands a WHOLE two-track recording ─────────────────
// `partial: false` on purpose: the engine says the audio is fine, so the only
// thing that can file this row `partial` is the sleep.

static PARKED: OnceLock<Mutex<Option<(PathBuf, PathBuf)>>> = OnceLock::new();

struct FakeEngine;
struct FakeCapture {
    wavs: (PathBuf, PathBuf),
}

impl CaptureEngine for FakeEngine {
    fn start(&self, _dir: &Path, _kind: MeetingKind) -> Result<Box<dyn ActiveCapture>, String> {
        let wavs = PARKED
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap()
            .clone()
            .ok_or("no wav parked")?;
        Ok(Box::new(FakeCapture { wavs }))
    }
}

impl ActiveCapture for FakeCapture {
    fn seconds(&self) -> f64 {
        SECONDS
    }
    fn stop(self: Box<Self>) -> Result<CaptureOutcome, String> {
        Ok(CaptureOutcome {
            wav_path: Some(self.wavs.0.clone()),
            sys_wav_path: Some(self.wavs.1.clone()),
            tap_rebuilds: None,
            track_rates: Vec::new(),
            seconds: SECONDS,
            note: None,
            partial: false,
        })
    }
}

/// The meeting consumer, subscribed exactly as `lib.rs` subscribes it: the
/// closure calls `MeetingController::on_sleep_wake` and nothing else.
fn hub_with_meetings(
    controller: &Arc<MeetingController>,
    log: &Arc<Mutex<Vec<String>>>,
) -> Arc<SleepWakeHub> {
    let hub = Arc::new(SleepWakeHub::new());
    let c = Arc::clone(controller);
    let l = Arc::clone(log);
    let policy = Mutex::new(SleepPolicy::new());
    hub.subscribe(CONSUMER_MEETINGS, move |event| {
        let mut policy = policy.lock().unwrap();
        let step = c.on_sleep_wake(&mut policy, event);
        l.lock().unwrap().push(format!("{event:?}:{step:?}"));
    });
    hub
}

#[test]
fn will_sleep_mid_recording_finalizes_partial_then_acknowledges() {
    let _guard = support::exclusive();
    let dir = temp_dir("nt10-sleep-mid-meeting");
    let db = Arc::new(open_db(&dir));
    let mic = dir.join("m.t0.wav");
    let sys = dir.join("m.t1.wav");
    write_wav(&mic, 220.0);
    write_wav(&sys, 330.0);
    *PARKED.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some((mic.clone(), sys.clone()));
    install_capture_engine(Arc::new(FakeEngine));
    let pipeline = meeting_pipeline::spawn(deps(Arc::clone(&db), &dir));
    assert!(
        meeting_pipeline::install(pipeline.clone()),
        "pipeline installed"
    );

    let sink: StatusSink = Arc::new(|_: &MeetingStatus| {});
    let controller = Arc::new(MeetingController::new(Arc::clone(&db), sink));
    let id = controller
        .start(&dir, Some("Walking between rooms".into()))
        .expect("start");
    assert!(controller.is_recording());

    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let hub = hub_with_meetings(&controller, &log);

    // Inject kIOMessageSystemWillSleep through the callback's one handler.
    let acks = AtomicUsize::new(0);
    let reply = power::on_power_message(&hub, IO_MESSAGE_SYSTEM_WILL_SLEEP, || {
        acks.fetch_add(1, Ordering::SeqCst);
        // Read the state AT acknowledgement time: this is the moment the OS is
        // released to power down.
        let row = db.get_meeting(&id).unwrap().expect("row at ack");
        log.lock().unwrap().push(format!(
            "ack recording={} state={}",
            controller.is_recording(),
            row.state
        ));
    });
    assert_eq!(reply, PowerReply::Acknowledged);
    assert_eq!(acks.load(Ordering::SeqCst), 1, "acknowledged exactly once");
    assert_eq!(
        *log.lock().unwrap(),
        vec![
            "WillSleep:Finalized".to_string(),
            "ack recording=false state=partial".to_string(),
        ],
        "the meeting is finalized and written BEFORE the sleep is acknowledged"
    );

    // The row: partial, the one sentence, both WAV paths intact.
    let row = db.get_meeting(&id).unwrap().expect("row");
    assert_eq!(row.state, MeetingState::Partial.as_str());
    assert_eq!(row.error.as_deref(), Some(SLEEP_STOP_NOTE));
    assert_eq!(row.mic_wav_path.as_deref(), Some(mic.to_str().unwrap()));
    assert_eq!(row.sys_wav_path.as_deref(), Some(sys.to_str().unwrap()));
    assert!(mic.exists() && sys.exists(), "both WAVs still on disk");
    let d: MeetingDiagnostics =
        serde_json::from_str(row.diagnostics.as_deref().expect("diagnostics")).unwrap();
    assert_eq!(d.stop_reason.as_deref(), Some(SLEEP_STOP_REASON));

    // The existing pipeline enqueued it and transcribed both tracks; the row
    // stays partial and keeps its sentence.
    assert!(
        pipeline.wait_idle(Duration::from_secs(30)),
        "pipeline drained"
    );
    let row = db.get_meeting(&id).unwrap().expect("row after pipeline");
    let segs = db.list_meeting_segments(&id).unwrap();
    let t0 = segs.iter().filter(|s| s.track == MIC_TRACK).count();
    let t1 = segs.iter().filter(|s| s.track == SYSTEM_TRACK).count();
    println!(
        "after pipeline: state={} track0={t0} track1={t1}",
        row.state
    );
    assert!(t0 > 0 && t1 > 0, "the pipeline transcribed both tracks");
    assert_eq!(row.state, "partial", "transcribed, still honest: {row:?}");
    assert_eq!(row.error.as_deref(), Some(SLEEP_STOP_NOTE));

    // Wake never silently resumes the old row.
    hub.publish(SleepEvent::DidWake);
    assert!(!controller.is_recording(), "wake must not resume capture");
    assert_eq!(db.get_meeting(&id).unwrap().unwrap().state, "partial");

    // A second will-sleep (the NSWorkspace notification for the same sleep)
    // changes nothing and is still acknowledged.
    let reply = power::on_power_message(&hub, IO_MESSAGE_SYSTEM_WILL_SLEEP, || {
        acks.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(reply, PowerReply::Acknowledged);
    assert_eq!(acks.load(Ordering::SeqCst), 2);
    assert_eq!(db.get_meeting(&id).unwrap().unwrap().state, "partial");
}

#[test]
fn will_sleep_with_no_meeting_is_a_noop_that_still_acknowledges() {
    let _guard = support::exclusive();
    let dir = temp_dir("nt10-sleep-idle");
    let db = Arc::new(open_db(&dir));
    let sink: StatusSink = Arc::new(|_: &MeetingStatus| {});
    let controller = Arc::new(MeetingController::new(Arc::clone(&db), sink));
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let hub = hub_with_meetings(&controller, &log);

    let acks = AtomicUsize::new(0);
    let reply = power::on_power_message(&hub, IO_MESSAGE_SYSTEM_WILL_SLEEP, || {
        acks.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(reply, PowerReply::Acknowledged);
    assert_eq!(
        acks.load(Ordering::SeqCst),
        1,
        "a no-op still releases the sleep"
    );
    assert_eq!(
        *log.lock().unwrap(),
        vec![format!("WillSleep:{:?}", SleepStep::Nothing)]
    );
    assert!(!controller.is_recording());
}

/// IOPMLib.h: acknowledge can-sleep and will-sleep; NEVER acknowledge
/// will-not-sleep, will-power-on or has-powered-on. Only will-sleep reaches
/// the hub.
#[test]
fn only_the_two_sleep_messages_are_acknowledged() {
    let heard: Arc<Mutex<Vec<SleepEvent>>> = Arc::default();
    let hub = SleepWakeHub::new();
    let h = Arc::clone(&heard);
    hub.subscribe(CONSUMER_MEETINGS, move |e| h.lock().unwrap().push(e));

    for (msg, want) in [
        (IO_MESSAGE_CAN_SYSTEM_SLEEP, PowerReply::Acknowledged),
        (
            IO_MESSAGE_SYSTEM_WILL_NOT_SLEEP,
            PowerReply::NotAcknowledged,
        ),
        (IO_MESSAGE_SYSTEM_WILL_POWER_ON, PowerReply::NotAcknowledged),
        (
            IO_MESSAGE_SYSTEM_HAS_POWERED_ON,
            PowerReply::NotAcknowledged,
        ),
        (0xE000_0010, PowerReply::NotAcknowledged),
    ] {
        let acked = AtomicUsize::new(0);
        let reply = power::on_power_message(&hub, msg, || {
            acked.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(reply, want, "message {msg:#x}");
        let expected = usize::from(want == PowerReply::Acknowledged);
        assert_eq!(acked.load(Ordering::SeqCst), expected, "message {msg:#x}");
    }
    assert!(
        heard.lock().unwrap().is_empty(),
        "only will-sleep is published"
    );
    // The constants are IOMessage.h's iokit_common_msg values.
    assert_eq!(IO_MESSAGE_CAN_SYSTEM_SLEEP, 0xE000_0270);
    assert_eq!(IO_MESSAGE_SYSTEM_WILL_SLEEP, 0xE000_0280);
}

/// The call site exists as CODE (not a comment), the callback routes through
/// `on_power_message`, the acknowledgement is `IOAllowPowerChange`, and the
/// shipping observer actually obtains a live IOKit registration.
#[test]
fn power_rs_registers_the_iokit_hold_on_the_shipping_observer() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/power.rs");
    let body = std::fs::read_to_string(&path).expect("read power.rs");
    let code: String = body
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for needle in [
        "IORegisterForSystemPower(ctx.cast()",
        "IOAllowPowerChange(port, id)",
        "IODeregisterForSystemPower(&mut self.notifier)",
        "on_power_message(&ctx.hub, message_type",
        "CFRunLoopAddSource(CFRunLoopGetMain()",
        "super::hold::PowerHold::register(Arc::clone(&hub))",
    ] {
        assert!(code.contains(needle), "power.rs lacks `{needle}` as code");
    }

    #[cfg(target_os = "macos")]
    {
        let observer = power::SleepWakeObserver::register(Arc::new(SleepWakeHub::new()));
        assert!(
            observer.holds_sleep(),
            "IORegisterForSystemPower returned no connection"
        );
        observer.unregister();
    }
}
