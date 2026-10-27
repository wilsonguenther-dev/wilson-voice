//! yap24-NT11 — a call on built-in speakers is transcribed once.
//!
//! Tier A (no ASR model): the AEC3 pre-pass and its coupling gate on
//! synthetic two-track audio with a known acoustic path, on a headphones
//! control (no path), and on the NT0 `call-bleed-2voice` corpus fixture when
//! it is on disk; plus the step-4 fallbacks through the real library
//! functions (speaker_label, the suppressed_as_echo column, the purge/delete
//! of the derived file). Numbers only — no transcript text is printed.

mod support;

use std::path::{Path, PathBuf};

use support::{open_db, seed_meeting, temp_dir};
use wilson_voice_lib::meeting_aec::{aec_coupling, aec_path_for, aec_prepass, mic_source_for};
use wilson_voice_lib::meetings::{speaker_label, speaker_label_for_span, DiarizationTarget, MIC_TRACK, SYSTEM_TRACK};

const RATE: u32 = 16_000;

/// Deterministic speech-like signal: noise shaped by a syllabic envelope with
/// pauses, so it has talk spurts and silences like a voice.
fn voice(seed: u64, seconds: f64, gain: f32) -> Vec<f32> {
    let n = (seconds * RATE as f64) as usize;
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut prev = 0.0f32;
    (0..n)
        .map(|i| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let white = ((state >> 33) as f32 / (1u64 << 31) as f32) - 0.5;
            // Mild pre-emphasis keeps it out of the high-pass filter's band.
            let x = white - 0.6 * prev;
            prev = white;
            let t = i as f32 / RATE as f32;
            let syllable = (0.5 + 0.5 * (t * 2.0 * std::f32::consts::PI * 4.0 + seed as f32).sin()).powi(2);
            let spurt = if ((t + seed as f32 * 0.37) % 3.0) < 2.2 { 1.0 } else { 0.0 };
            x * syllable * spurt * gain
        })
        .collect()
}

fn write_wav(path: &Path, samples: &[f32]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

/// mic = local talker in some windows + the far side through a small room
/// (delay + two reflections), or with `coupled = false` no far side at all.
fn fixture(dir: &Path, coupled: bool) -> (PathBuf, PathBuf) {
    let secs = 40.0;
    let far = voice(7, secs, 0.5);
    let mut local = voice(11, secs, 0.3);
    // The local talker is silent in the first 20 s (pure call windows), and the
    // far side is silent in the last 10 s (pure local windows).
    for (i, s) in local.iter_mut().enumerate() {
        if (i as f64) < 20.0 * RATE as f64 {
            *s = 0.0;
        }
    }
    let mut sys = far.clone();
    for (i, s) in sys.iter_mut().enumerate() {
        if (i as f64) >= 30.0 * RATE as f64 {
            *s = 0.0;
        }
    }
    let delay = 48; // 3 ms
    let mic: Vec<f32> = (0..local.len())
        .map(|i| {
            let mut v = local[i];
            if coupled {
                let tap = |d: usize, g: f32| if i >= d { sys[i - d] * g } else { 0.0 };
                v += tap(delay, 0.45) + tap(delay + 37, 0.2) + tap(delay + 113, 0.08);
            }
            v
        })
        .collect();
    let (m, s) = (dir.join("call.t0.wav"), dir.join("call.t1.wav"));
    write_wav(&m, &mic);
    write_wav(&s, &sys);
    (m, s)
}

#[test]
fn synthetic_call_bleed_is_cancelled_and_local_speech_is_untouched() {
    let dir = temp_dir("nt11-synth");
    let (mic, sys) = fixture(&dir, true);
    let lag = aec_coupling(&mic, &sys).expect("a real acoustic path must be detected");
    assert!((0.0..=20.0).contains(&lag), "lag_ms={lag}");
    let out = aec_path_for(&mic);
    let r = aec_prepass(&mic, &sys, &out).expect("pre-pass");
    println!(
        "synthetic call_windows={} local_windows={} call_atten_p50_db={:.1} call_atten_p25_db={:.1} local_loss_p90_db={:.2} lag_ms={lag:.1} wall_ms={}",
        r.call_windows, r.local_windows, r.call_atten_p50_db, r.call_atten_p25_db, r.local_loss_p90_db, r.wall_ms
    );
    assert!(r.call_windows >= 10 && r.local_windows >= 5, "{r:?}");
    assert!(r.call_atten_p50_db >= 15.0, "call windows p50 {:.1} dB < 15 dB", r.call_atten_p50_db);
    assert!(r.local_loss_p90_db <= 1.0, "local-only p90 loss {:.2} dB > 1 dB", r.local_loss_p90_db);
    // Idempotent: the second run is a no-op and the file is unchanged.
    let before = std::fs::read(&out).unwrap();
    let again = aec_prepass(&mic, &sys, &out).unwrap();
    assert!(again.skipped_existing);
    assert_eq!(before, std::fs::read(&out).unwrap());
    // Raw WAV untouched, no temp file left behind.
    let raw = hound::WavReader::open(&mic).unwrap().len();
    assert_eq!(hound::WavReader::open(&out).unwrap().len(), raw);
    assert!(std::fs::read_dir(&dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".partial")));
    // The pipeline's choice: virtual + coupled -> the cleaned file.
    assert_eq!(mic_source_for("m", true, &mic, Some(&sys)), out);
}

#[test]
fn headphones_control_has_no_coupling_and_reads_the_raw_mic() {
    let dir = temp_dir("nt11-phones");
    let (mic, sys) = fixture(&dir, false);
    assert_eq!(aec_coupling(&mic, &sys), None, "no acoustic path, no coupling");
    assert_eq!(mic_source_for("m", true, &mic, Some(&sys)), mic);
    assert!(!aec_path_for(&mic).exists(), "nothing derived for a headphones call");
    // In-person (not virtual) and mic-only never derive anything either.
    let (mic2, sys2) = fixture(&temp_dir("nt11-inperson"), true);
    assert_eq!(mic_source_for("m", false, &mic2, Some(&sys2)), mic2);
    assert_eq!(mic_source_for("m", true, &mic2, None), mic2);
    assert!(!aec_path_for(&mic2).exists());
}

#[test]
fn corpus_call_bleed_2voice_fixture_is_cancelled() {
    let root = dirs::home_dir().unwrap().join("yap-eval-corpus/meetings/call-bleed-2voice");
    let (mic, sys) = (root.join("track0.wav"), root.join("track1.wav"));
    if !mic.is_file() || !sys.is_file() {
        assert!(std::env::var("YAP_EVAL_REQUIRE").as_deref() != Ok("1"), "corpus fixture missing");
        println!("SKIP call-bleed-2voice not on disk");
        return;
    }
    let dir = temp_dir("nt11-corpus");
    let out = dir.join("track0.aec.wav");
    let lag = aec_coupling(&mic, &sys);
    let r = aec_prepass(&mic, &sys, &out).expect("pre-pass");
    println!(
        "call-bleed-2voice call_windows={} local_windows={} call_atten_p50_db={:.1} call_atten_p90_db={:.1} local_loss_p90_db={:.2} lag_ms={lag:?} wall_ms={}",
        r.call_windows, r.local_windows, r.call_atten_p50_db, r.call_atten_p90_db, r.local_loss_p90_db, r.wall_ms
    );
    assert!(lag.is_some(), "the bleed fixture has a real path into the mic");
    assert!(r.call_atten_p50_db >= 15.0, "p50 {:.1} dB", r.call_atten_p50_db);
}

#[test]
fn tap_hole_span_is_labelled_through_speaker_label_never_me() {
    let me = speaker_label(MIC_TRACK, DiarizationTarget::MicIsMe);
    let unattributed = speaker_label(MIC_TRACK, DiarizationTarget::ClusterTrackA);
    assert_ne!(me, unattributed);
    assert_eq!(speaker_label_for_span(MIC_TRACK, DiarizationTarget::MicIsMe, false), unattributed);
    assert_eq!(speaker_label_for_span(MIC_TRACK, DiarizationTarget::MicIsMe, true), me);
    assert_eq!(
        speaker_label_for_span(SYSTEM_TRACK, DiarizationTarget::MicIsMe, false),
        speaker_label(SYSTEM_TRACK, DiarizationTarget::MicIsMe)
    );
}

#[test]
fn suppressed_echo_rows_are_kept_but_leave_the_transcript() {
    let dir = temp_dir("nt11-suppress");
    let db = open_db(&dir);
    let id = seed_meeting(&db, &dir, "call", &["first line here", "echo of the far side"]);
    let segs = db.list_meeting_segments(&id).unwrap();
    assert_eq!(segs.len(), 2);
    assert!(db.mark_segment_suppressed_as_echo(&id, &segs[1].id).unwrap());
    assert_eq!(db.list_meeting_segments(&id).unwrap().len(), 1);
    assert_eq!(db.count_suppressed_echo_segments(&id).unwrap(), 1);
}

#[test]
fn delete_cascade_removes_the_derived_aec_file_and_anchors() {
    let dir = temp_dir("nt11-delete");
    let db = open_db(&dir);
    let id = seed_meeting(&db, &dir, "call", &["hello"]);
    let wav = PathBuf::from(db.get_meeting(&id).unwrap().unwrap().mic_wav_path.unwrap());
    let derived = wilson_voice_lib::meeting_aec::derived_paths_for_wav(&wav);
    assert_eq!(derived.len(), 3);
    for d in &derived {
        std::fs::write(d, b"x").unwrap();
    }
    db.delete_meeting_with_audio(&id).unwrap();
    assert!(!wav.exists());
    for d in &derived {
        assert!(!d.exists(), "{} survived the delete", d.file_name().unwrap().to_string_lossy());
    }
}
