//! Y8-C acceptance — **earcons**: four bundled sounds, off by default, and the
//! one ordering that makes them audible at all.
//!
//! YV28 mutes the Mac's whole output while a take records. A start earcon that
//! plays after the mute, or a stop earcon that plays before the restore, is
//! silent — the feature would "work" in every unit test and be inaudible to a
//! human. So the ordering is asserted against a fake output device and a fake
//! speaker that write into ONE shared event log, and `lib.rs` is scanned so the
//! take path cannot reach the mute any other way than through the functions
//! under test.
//!
//! The meeting rule follows `tests/meeting_no_automute.rs`'s shape: a real
//! synthetic `MeetingSession` is started behind the library's turnstile, and the
//! context the dictation path asks for (`take_context()`) is what gates sound.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use wilson_voice_lib::earcons::{
    after_take, earcon_allowed, end_take, milestone_crossed, play_event, start_take, Earcon,
    EarconSink,
};
use wilson_voice_lib::meeting::{
    session_turnstile, take_context, ExternalStream, MeetingSession, SessionConfig, TakeContext,
};
use wilson_voice_lib::sysaudio::{OutputAudioState, OutputControl};
use wilson_voice_lib::{load_settings, AppSettings};

/// One log both fakes write into, so "before" and "after" are literal.
type Log = Arc<Mutex<Vec<String>>>;

struct FakeOutput {
    log: Log,
    state: Mutex<OutputAudioState>,
}

impl FakeOutput {
    fn new(log: &Log) -> Self {
        Self {
            log: log.clone(),
            state: Mutex::new(OutputAudioState {
                muted: false,
                volume: Some(0.6),
                device: None,
            }),
        }
    }
    fn muted(&self) -> bool {
        self.state.lock().unwrap().muted
    }
}

impl OutputControl for FakeOutput {
    fn mute_and_save(&self) -> Option<OutputAudioState> {
        let mut state = self.state.lock().unwrap();
        self.log.lock().unwrap().push("mute".into());
        let saved = *state;
        state.muted = true;
        Some(saved)
    }
    fn restore(&self, saved: OutputAudioState) {
        self.log.lock().unwrap().push("restore".into());
        *self.state.lock().unwrap() = saved;
    }
}

/// A speaker that records what it was asked to play AND whether the output was
/// muted at that instant — a sound "played" into a muted device is the bug.
struct FakeSpeaker<'a> {
    log: Log,
    output: &'a FakeOutput,
}

impl EarconSink for FakeSpeaker<'_> {
    fn play_blocking(&self, earcon: Earcon) {
        let audible = if self.output.muted() {
            "MUTED"
        } else {
            "audible"
        };
        self.log
            .lock()
            .unwrap()
            .push(format!("play:{earcon:?}:{audible}"));
    }
    fn play(&self, earcon: Earcon) {
        self.play_blocking(earcon)
    }
}

fn rig() -> (Log, FakeOutput) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let output = FakeOutput::new(&log);
    (log, output)
}

fn events(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yap-y8c-earcons-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// (sample rate, channels, bits, data bytes) from a canonical PCM WAV.
fn wav_shape(bytes: &[u8]) -> (u32, u16, u16, usize) {
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    let mut i = 12;
    let (mut rate, mut ch, mut bits, mut data) = (0, 0, 0, 0);
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
        if id == b"fmt " {
            assert_eq!(u16::from_le_bytes([bytes[i + 8], bytes[i + 9]]), 1, "PCM");
            ch = u16::from_le_bytes([bytes[i + 10], bytes[i + 11]]);
            rate = u32::from_le_bytes(bytes[i + 12..i + 16].try_into().unwrap());
            bits = u16::from_le_bytes([bytes[i + 22], bytes[i + 23]]);
        } else if id == b"data" {
            data = len;
        }
        i += 8 + len + (len & 1);
    }
    (rate, ch, bits, data)
}

fn duration_ms(bytes: &[u8]) -> f64 {
    let (rate, ch, bits, data) = wav_shape(bytes);
    data as f64 / (rate as f64 * ch as f64 * (bits as f64 / 8.0)) * 1000.0
}

// ── assets ──────────────────────────────────────────────────────────────────

#[test]
fn the_four_assets_exist_are_bundled_and_are_short() {
    let dir = manifest().join("assets/sounds");
    let conf: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest().join("tauri.conf.json")).expect("tauri.conf.json"),
    )
    .expect("tauri.conf.json is JSON");
    let resources = conf["bundle"]["resources"]
        .as_object()
        .expect("bundle.resources is a map");

    assert_eq!(Earcon::ALL.len(), 4, "four earcons, no more");
    for earcon in Earcon::ALL {
        let path = dir.join(earcon.file_name());
        let on_disk = std::fs::read(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        // The embedded copy IS the file — a stale or missing asset cannot ship.
        assert_eq!(
            earcon.bytes(),
            on_disk.as_slice(),
            "{earcon:?} embedded != on disk"
        );
        let key = format!("assets/sounds/{}", earcon.file_name());
        assert!(
            resources.contains_key(&key),
            "{key} is not in tauri.conf.json bundle.resources"
        );
        let ms = duration_ms(&on_disk);
        assert!(ms > 30.0, "{earcon:?} is {ms:.0} ms — too short to hear");
        match earcon {
            Earcon::Start | Earcon::Stop => {
                assert!(
                    ms < 200.0,
                    "{earcon:?} is {ms:.0} ms; start/stop must be < 200 ms"
                )
            }
            _ => assert!(ms < 500.0, "{earcon:?} is {ms:.0} ms; earcons are short"),
        }
    }
    // Nothing is fetched at runtime: no URL anywhere in the module.
    let src = std::fs::read_to_string(manifest().join("src/earcons.rs")).unwrap();
    assert!(!src.contains("http://") && !src.contains("https://"));
}

// ── off by default ──────────────────────────────────────────────────────────

#[test]
fn earcons_are_off_by_default() {
    assert!(!AppSettings::default().earcons, "a fresh install is silent");

    // A store written before Y8-C (no `earcons` key at all) loads as OFF too.
    let dir = tmpdir("default");
    let path = dir.join("settings.json");
    std::fs::write(&path, r#"{"schemaVersion": 2, "muteWhileDictating": true}"#).unwrap();
    assert!(
        !load_settings(&path).earcons,
        "a pre-Y8-C store loads silent"
    );
    let _ = std::fs::remove_dir_all(&dir);

    // And with the default settings, a whole take plays nothing.
    let (log, output) = rig();
    let speaker = FakeSpeaker {
        log: log.clone(),
        output: &output,
    };
    let d = AppSettings::default();
    let saved = start_take(
        d.earcons,
        d.mute_while_dictating,
        TakeContext::Dictation,
        &speaker,
        &output,
    );
    end_take(d.earcons, saved, TakeContext::Dictation, &speaker, &output);
    assert_eq!(events(&log), vec!["mute", "restore"], "no sound by default");
}

#[test]
fn nothing_plays_when_the_setting_is_off() {
    let (log, output) = rig();
    let speaker = FakeSpeaker {
        log: log.clone(),
        output: &output,
    };
    let saved = start_take(false, true, TakeContext::Dictation, &speaker, &output);
    end_take(false, saved, TakeContext::Dictation, &speaker, &output);
    play_event(false, TakeContext::Dictation, &speaker, Earcon::Paste);
    play_event(false, TakeContext::Dictation, &speaker, Earcon::Achievement);
    assert_eq!(events(&log), vec!["mute", "restore"]);
    assert!(!earcon_allowed(false, TakeContext::Dictation));
}

// ── the ordering — this is the whole item ───────────────────────────────────

#[test]
fn start_plays_before_the_mute_and_stop_after_the_restore() {
    let (log, output) = rig();
    let speaker = FakeSpeaker {
        log: log.clone(),
        output: &output,
    };

    let saved = start_take(true, true, TakeContext::Dictation, &speaker, &output);
    assert!(output.muted(), "YV28's mute still happens");
    end_take(true, saved, TakeContext::Dictation, &speaker, &output);
    assert!(!output.muted(), "and is restored verbatim");

    assert_eq!(
        events(&log),
        vec!["play:Start:audible", "mute", "restore", "play:Stop:audible",],
        "start must sound BEFORE the mute and stop AFTER the restore"
    );

    // With auto-mute switched off the earcons still play, and nothing is muted.
    let (log, output) = rig();
    let speaker = FakeSpeaker {
        log: log.clone(),
        output: &output,
    };
    let saved = start_take(true, false, TakeContext::Dictation, &speaker, &output);
    assert!(saved.is_none());
    end_take(true, saved, TakeContext::Dictation, &speaker, &output);
    assert_eq!(
        events(&log),
        vec!["play:Start:audible", "play:Stop:audible"]
    );
}

/// The take path in `lib.rs` must reach the output device ONLY through the
/// functions asserted above — a second, direct `mute_for_take` call would mute
/// with no earcon ordering at all.
#[test]
fn the_take_path_mutes_and_restores_only_through_the_earcon_order() {
    let lib = std::fs::read_to_string(manifest().join("src/lib.rs")).unwrap();
    let count = |needle: &str| lib.matches(needle).count();
    assert_eq!(
        count("sysaudio::mute_for_take("),
        0,
        "lib.rs mutes directly"
    );
    assert_eq!(count("earcons::start_take("), 1);
    assert_eq!(count("earcons::end_take("), 1);
    // stop and cancel both end through end_take_audio; only exit restores bare.
    assert_eq!(count("end_take_audio(&state);"), 1, "the stop path");
    assert_eq!(count("end_take_audio(state);"), 1, "the cancel path");
    assert_eq!(
        count("sysaudio::restore("),
        1,
        "only the exit restore is bare"
    );
}

// ── meetings ────────────────────────────────────────────────────────────────

#[test]
fn nothing_plays_while_a_meeting_records() {
    let _turn = session_turnstile();
    let dir = tmpdir("meeting");
    let session = MeetingSession::start(SessionConfig {
        stream: Arc::new(ExternalStream),
        watchdog_interval: Duration::from_secs(3_600),
        ..SessionConfig::new(&dir, 16_000, 1)
    })
    .expect("a synthetic meeting starts");

    let (log, output) = rig();
    let speaker = FakeSpeaker {
        log: log.clone(),
        output: &output,
    };
    // The meeting itself, and a dictation taken during it — setting ON.
    for context in [TakeContext::Meeting, take_context()] {
        let saved = start_take(true, true, context, &speaker, &output);
        end_take(true, saved, context, &speaker, &output);
        play_event(true, context, &speaker, Earcon::Paste);
        play_event(true, context, &speaker, Earcon::Achievement);
    }
    assert_eq!(take_context(), TakeContext::DictationDuringMeeting);
    assert!(
        events(&log).is_empty(),
        "a meeting recording heard nothing and was muted by nothing: {:?}",
        events(&log)
    );

    session.stop();

    // The moment the meeting ends, the ordinary behaviour is back — proving the
    // gate is the meeting, not a feature that plays nothing at all.
    let context = take_context();
    assert_eq!(context, TakeContext::Dictation);
    let saved = start_take(true, true, context, &speaker, &output);
    end_take(true, saved, context, &speaker, &output);
    assert_eq!(
        events(&log),
        vec!["play:Start:audible", "mute", "restore", "play:Stop:audible"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ── paste / achievement ─────────────────────────────────────────────────────

#[test]
fn one_sound_per_landed_take_and_milestones_fire_once() {
    assert_eq!(milestone_crossed(990, 1_004), Some(1_000));
    assert_eq!(milestone_crossed(1_000, 1_020), None, "already past it");
    assert_eq!(
        milestone_crossed(4_990, 10_001),
        Some(10_000),
        "the highest"
    );
    assert_eq!(after_take(true, 10, 20), Some(Earcon::Paste));
    assert_eq!(
        after_take(false, 10, 20),
        None,
        "copied only: no paste sound"
    );
    assert_eq!(
        after_take(true, 999, 1_001),
        Some(Earcon::Achievement),
        "a milestone replaces the paste sound, never stacks on it"
    );
}
