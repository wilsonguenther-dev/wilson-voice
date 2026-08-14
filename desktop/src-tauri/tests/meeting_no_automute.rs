//! YV91 acceptance — **starting a meeting does not change output mute/volume,
//! and neither does a dictation taken during one** (plan finding #9).
//!
//! YV28 mutes the Mac's whole default output while a take records. That is
//! right for a five-second dictation and damaging for a meeting: it silences
//! the Zoom call the user is recording, and a hotkey pressed mid-meeting
//! silences the call they are LISTENING to, with nothing on screen saying why.
//! The plan never mentioned it, which is how a shipped behaviour becomes a
//! shipped bug.
//!
//! The fix makes auto-mute a property of the take's CONTEXT and puts one gate
//! (`sysaudio::mute_for_take`) in front of the only call site. These tests
//! drive that gate against a fake output device that records every write, so
//! "output mute/volume is unchanged" is an assertion about the device rather
//! than about the code that was supposed to leave it alone.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use wilson_voice_lib::meeting::{
    auto_mute_allowed, session_turnstile, take_context, ExternalStream, MeetingSession,
    SessionConfig, TakeContext,
};
use wilson_voice_lib::sysaudio::{mute_for_take, OutputAudioState, OutputControl};

/// A stand-in for the Mac's default output device. It starts unmuted at 60%,
/// and every mute/restore it is asked to perform is recorded.
struct FakeOutput {
    state: Mutex<OutputAudioState>,
    mutes: AtomicUsize,
    restores: AtomicUsize,
}

impl FakeOutput {
    fn new() -> Self {
        Self {
            state: Mutex::new(OutputAudioState {
                muted: false,
                volume: Some(0.6),
            }),
            mutes: AtomicUsize::new(0),
            restores: AtomicUsize::new(0),
        }
    }

    fn snapshot(&self) -> OutputAudioState {
        *self.state.lock().unwrap()
    }

    fn writes(&self) -> (usize, usize) {
        (
            self.mutes.load(Ordering::Relaxed),
            self.restores.load(Ordering::Relaxed),
        )
    }
}

impl OutputControl for FakeOutput {
    fn mute_and_save(&self) -> Option<OutputAudioState> {
        self.mutes.fetch_add(1, Ordering::Relaxed);
        let mut state = self.state.lock().unwrap();
        let saved = *state;
        state.muted = true;
        Some(saved)
    }

    fn restore(&self, saved: OutputAudioState) {
        self.restores.fetch_add(1, Ordering::Relaxed);
        *self.state.lock().unwrap() = saved;
    }
}

fn turn() -> std::sync::MutexGuard<'static, ()> {
    // The library's turnstile, not a private one: `MeetingSession::start`
    // refuses a second concurrent session, so every suite that starts a
    // meeting has to queue behind the same lock the others take.
    session_turnstile()
}

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yap-yv91-mute-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
fn starting_and_stopping_a_meeting_leaves_the_output_untouched() {
    let _turn = turn();
    let output = FakeOutput::new();
    let before = output.snapshot();

    let dir = tmpdir("session");
    let session = MeetingSession::start(SessionConfig {
        // This test never feeds audio at all — it watches the OUTPUT device —
        // so the session holds no input stream (YV91's `ExternalStream`).
        stream: Arc::new(ExternalStream),
        watchdog_interval: Duration::from_secs(3_600),
        ..SessionConfig::new(&dir, 16_000, 1)
    })
    .expect("a synthetic meeting starts");

    // The meeting itself asks the gate — and is refused, without the device
    // ever being touched.
    assert!(mute_for_take(TakeContext::Meeting, &output).is_none());
    assert_eq!(output.snapshot(), before, "output changed at meeting start");

    session.stop();

    assert_eq!(output.snapshot(), before, "output changed at meeting stop");
    assert_eq!(
        output.writes(),
        (0, 0),
        "the meeting made zero writes to the output device"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dictation_during_a_meeting_does_not_mute_the_call_the_user_is_listening_to() {
    let _turn = turn();
    let output = FakeOutput::new();
    let before = output.snapshot();
    let dir = tmpdir("mid-meeting");
    let session = MeetingSession::start(SessionConfig {
        // This test never feeds audio at all — it watches the OUTPUT device —
        // so the session holds no input stream (YV91's `ExternalStream`).
        stream: Arc::new(ExternalStream),
        watchdog_interval: Duration::from_secs(3_600),
        ..SessionConfig::new(&dir, 16_000, 1)
    })
    .expect("a synthetic meeting starts");

    // The dictation path does not pick its own context — it asks.
    let context = take_context();
    assert_eq!(context, TakeContext::DictationDuringMeeting);
    assert!(mute_for_take(context, &output).is_none());
    assert_eq!(output.snapshot(), before);
    assert_eq!(output.writes(), (0, 0));

    session.stop();

    // …and the moment the meeting ends, the ordinary dictation behaviour is
    // back. This is the half that proves the gate is a POLICY and not a
    // regression that deleted auto-mute outright.
    let context = take_context();
    assert_eq!(context, TakeContext::Dictation);
    let saved = mute_for_take(context, &output).expect("an ordinary dictation still mutes");
    assert!(output.snapshot().muted, "YV28's behaviour is intact");
    output.restore(saved);
    assert_eq!(output.snapshot(), before, "and it restores verbatim");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_policy_itself_is_explicit() {
    // The rule, without any machinery around it.
    assert!(auto_mute_allowed(TakeContext::Dictation));
    assert!(!auto_mute_allowed(TakeContext::Meeting));
    assert!(!auto_mute_allowed(TakeContext::DictationDuringMeeting));
}
