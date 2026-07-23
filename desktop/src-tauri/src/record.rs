//! In-process microphone capture via cpal (cpal 0.18).
//!
//! Recording inside the Wilson Voice process (not external ffmpeg) so macOS TCC
//! attributes Microphone permission to com.wilsonguenther.wilson-voice.
//! Live RMS level is exposed for the floating HUD waveform.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{WavSpec, WavWriter};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use uuid::Uuid;

/// Shared peak level 0..=1000 for HUD (updated every audio callback window).
pub type LevelHandle = Arc<AtomicU32>;

pub struct ActiveRecording {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<Result<(), String>>>,
    pub wav_path: PathBuf,
    started: std::time::Instant,
    pub level: LevelHandle,
}

impl ActiveRecording {
    /// Shared stop flag — false while recording, true after stop.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }
}

pub struct RecordingResult {
    pub wav_path: PathBuf,
    /// VOICED seconds (energy VAD) — the denominator for WPM. This is real
    /// speaking time: leading/trailing silence and long thinking pauses are
    /// excluded, natural inter-word gaps are kept. NOT the raw clip length
    /// (that inflates the denominator and makes WPM read low + jittery).
    pub speech_seconds: f64,
    /// Wall-clock hold (press→release), for latency telemetry only.
    pub hold_wall_seconds: f64,
}

pub fn start_recording(dir: PathBuf) -> Result<ActiveRecording, String> {
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let wav_path = dir.join(format!("{}.wav", Uuid::new_v4()));
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = stop.clone();
    let path_t = wav_path.clone();
    let level: LevelHandle = Arc::new(AtomicU32::new(0));
    let level_t = level.clone();

    let join = thread::Builder::new()
        .name("wv-record".into())
        .spawn(move || record_loop(path_t, stop_t, level_t))
        .map_err(|e| format!("spawn record thread: {e}"))?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(350);
    while !join.is_finished() && std::time::Instant::now() < deadline {
        thread::sleep(std::time::Duration::from_millis(20));
    }
    if join.is_finished() {
        return match join.join() {
            Ok(Ok(())) => Err("Recording stopped immediately".into()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Recording thread panicked".into()),
        };
    }

    Ok(ActiveRecording {
        stop,
        join: Some(join),
        wav_path,
        started: std::time::Instant::now(),
        level,
    })
}

pub fn stop_recording(mut active: ActiveRecording) -> Result<RecordingResult, String> {
    let hold_wall_seconds = active.started.elapsed().as_secs_f64().max(0.01);
    active.stop.store(true, Ordering::SeqCst);
    active.level.store(0, Ordering::Relaxed);
    if let Some(j) = active.join.take() {
        match j.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("Recording thread panicked".into()),
        }
    }

    if !active.wav_path.exists() {
        return Err(
            "No audio file — click Allow once for Microphone (Yap) in the system dialog, then enable it under System Settings → Privacy → Microphone."
                .into(),
        );
    }
    let meta = std::fs::metadata(&active.wav_path).map_err(|e| e.to_string())?;
    if meta.len() < 1000 {
        return Err(format!(
            "Audio too short ({} bytes) — hold longer, or allow Microphone for Yap.",
            meta.len()
        ));
    }
    // WPM denominator = real voiced time. Fall back to the raw clip length only
    // when VAD can't find speech (near-silent capture) so WPM never divides by ~0.
    let clip_seconds = wav_duration_seconds(&active.wav_path).unwrap_or(hold_wall_seconds);
    let voiced = voiced_seconds_from_wav(&active.wav_path).unwrap_or(0.0);
    let speech_seconds = if voiced >= 0.1 { voiced } else { clip_seconds };
    Ok(RecordingResult {
        wav_path: active.wav_path,
        speech_seconds: speech_seconds.max(0.05),
        hold_wall_seconds,
    })
}

fn wav_duration_seconds(path: &PathBuf) -> Option<f64> {
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let samples = reader.duration() as f64;
    let rate = spec.sample_rate as f64;
    if rate <= 0.0 {
        return None;
    }
    Some(samples / rate)
}

/// Read the (mono, i16) WAV back and estimate voiced seconds from its samples.
fn voiced_seconds_from_wav(path: &PathBuf) -> Option<f64> {
    let mut reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            // i64 shift: 1i32 << 31 overflows the sign bit for 32-bit WAV. (Prod
            // only ever writes 16-bit, but keep this correct for foreign inputs.)
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 * scale)
                .collect()
        }
        hound::SampleFormat::Float => {
            reader.samples::<f32>().filter_map(|s| s.ok()).collect()
        }
    };
    Some(voiced_seconds(&samples, spec.sample_rate))
}

/// Energy-based VAD → seconds of actual speech. Frame the signal (20 ms), take
/// per-frame RMS, estimate the noise floor from a low percentile (robust to a
/// few loud frames), and mark frames that clear `floor·3` (with an absolute and
/// a peak-relative minimum so pure silence never registers). Short gaps between
/// voiced runs (≤300 ms — natural inter-word pauses) are bridged so we count
/// continuous speaking; leading/trailing silence and long pauses stay excluded.
/// Deterministic and pure → unit-tested below.
fn voiced_seconds(samples: &[f32], sample_rate: u32) -> f64 {
    if sample_rate == 0 || samples.is_empty() {
        return 0.0;
    }
    let frame = (sample_rate as usize / 50).max(1); // 20 ms
    let frame_secs = frame as f64 / sample_rate as f64;
    let mut rms: Vec<f32> = Vec::with_capacity(samples.len() / frame + 1);
    let mut i = 0;
    while i + frame <= samples.len() {
        let mut sum = 0.0f32;
        for &s in &samples[i..i + frame] {
            sum += s * s;
        }
        rms.push((sum / frame as f32).sqrt());
        i += frame;
    }
    if rms.is_empty() {
        return 0.0;
    }
    let mut sorted = rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor = sorted[((sorted.len() as f64) * 0.10) as usize];
    let peak = *sorted.last().unwrap();
    // Dynamic range decides the gate. When the low-percentile "floor" sits close
    // to the peak, the clip has NO real silence to anchor on — it's continuous
    // speech (or steady input). A floor·3 gate would then reject every frame and
    // collapse voiced→0 (which silently reverts WPM to the old clip-length
    // denominator). Detect that and use a peak-relative gate that keeps the whole
    // utterance. Otherwise anchor on the silence floor to carve speech out of the
    // surrounding quiet (the common push-to-talk case).
    let thresh = if floor > peak * 0.4 {
        (peak * 0.3).max(1e-4)
    } else {
        (floor * 3.0).max(peak * 0.06).max(1e-4)
    };

    let mask: Vec<bool> = rms.iter().map(|&r| r > thresh).collect();
    // Bridge short inter-word gaps between voiced runs.
    let max_gap = (0.30 / frame_secs).round() as usize;
    let mut bridged = mask.clone();
    let mut last_voiced: Option<usize> = None;
    for idx in 0..mask.len() {
        if mask[idx] {
            if let Some(lv) = last_voiced {
                if idx - lv <= max_gap + 1 {
                    for slot in bridged.iter_mut().take(idx).skip(lv + 1) {
                        *slot = true;
                    }
                }
            }
            last_voiced = Some(idx);
        }
    }
    let voiced = bridged.iter().filter(|&&b| b).count();
    voiced as f64 * frame_secs
}

fn push_level(level: &LevelHandle, chunk: &[f32]) {
    if chunk.is_empty() {
        return;
    }
    // RMS of chunk, lightly compressed for pretty bars
    let mut sum = 0.0f32;
    let mut peak = 0.0f32;
    for &s in chunk {
        let a = s.abs();
        sum += a * a;
        if a > peak {
            peak = a;
        }
    }
    let rms = (sum / chunk.len() as f32).sqrt();
    let mix = (rms * 2.2 + peak * 0.35).clamp(0.0, 1.0);
    let v = (mix * 1000.0) as u32;
    // decay-friendly: keep max of previous (poller will decay)
    let prev = level.load(Ordering::Relaxed);
    if v > prev {
        level.store(v, Ordering::Relaxed);
    } else {
        // soft decay so bars fall smoothly even between callbacks
        level.store(prev.saturating_sub(40).max(v), Ordering::Relaxed);
    }
}

fn record_loop(
    wav_path: PathBuf,
    stop: Arc<AtomicBool>,
    level: LevelHandle,
) -> Result<(), String> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or_else(|| {
        "No microphone found. Click Dictate once so macOS prompts, then enable Yap under System Settings → Privacy → Microphone.".to_string()
    })?;

    let supported = device.default_input_config().map_err(|e| {
        format!(
            "Mic config failed ({e}). Enable Microphone for Yap (not Python) in System Settings."
        )
    })?;

    let sample_rate = supported.sample_rate();
    let channels = supported.channels();
    let sample_format = supported.sample_format();
    let conf: cpal::StreamConfig = supported.into();

    let dev_name = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "default".into());
    log::info!("mic device={dev_name} format={sample_format:?} rate={sample_rate} ch={channels}");

    let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let samples_cb = samples.clone();
    let level_cb = level.clone();
    let err_fn = |e| log::error!("cpal stream error: {e}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device
            .build_input_stream(
                conf.clone(),
                move |data: &[f32], _| {
                    push_level(&level_cb, data);
                    if let Ok(mut v) = samples_cb.lock() {
                        v.extend_from_slice(data);
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("mic stream f32: {e}"))?,
        cpal::SampleFormat::I16 => {
            let level_cb = level.clone();
            let samples_cb = samples.clone();
            device
                .build_input_stream(
                    conf.clone(),
                    move |data: &[i16], _| {
                        let f: Vec<f32> = data
                            .iter()
                            .map(|&s| s as f32 / i16::MAX as f32)
                            .collect();
                        push_level(&level_cb, &f);
                        if let Ok(mut v) = samples_cb.lock() {
                            v.extend(f);
                        }
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("mic stream i16: {e}"))?
        }
        cpal::SampleFormat::U16 => {
            let level_cb = level.clone();
            let samples_cb = samples.clone();
            device
                .build_input_stream(
                    conf,
                    move |data: &[u16], _| {
                        let f: Vec<f32> = data
                            .iter()
                            .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                            .collect();
                        push_level(&level_cb, &f);
                        if let Ok(mut v) = samples_cb.lock() {
                            v.extend(f);
                        }
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("mic stream u16: {e}"))?
        }
        other => {
            return Err(format!(
                "Unsupported sample format {other:?}. Try a different input device."
            ));
        }
    };

    stream
        .play()
        .map_err(|e| format!("mic start failed ({e}). Allow Microphone for Yap."))?;

    while !stop.load(Ordering::SeqCst) {
        thread::sleep(std::time::Duration::from_millis(50));
    }
    drop(stream);
    level.store(0, Ordering::Relaxed);

    let raw = samples.lock().map_err(|e| e.to_string())?.clone();
    if raw.is_empty() {
        return Err(
            "No samples captured. Enable Microphone for Yap in System Settings.".into(),
        );
    }

    let mono = if channels <= 1 {
        raw
    } else {
        let ch = channels as usize;
        raw.chunks(ch)
            .map(|c| c.iter().sum::<f32>() / c.len() as f32)
            .collect()
    };
    let target_rate = 16_000u32;
    let resampled = if sample_rate == target_rate {
        mono
    } else {
        resample_linear(&mono, sample_rate, target_rate)
    };

    write_wav_i16(&wav_path, target_rate, &resampled)?;
    log::info!(
        "wrote {} samples → {}",
        resampled.len(),
        wav_path.display()
    );
    Ok(())
}

fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if input.is_empty() || from == 0 {
        return Vec::new();
    }
    let ratio = from as f64 / to as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 * ratio;
        let i0 = src.floor() as usize;
        let i1 = (i0 + 1).min(input.len() - 1);
        let t = (src - i0 as f64) as f32;
        out.push(input[i0] * (1.0 - t) + input[i1] * t);
    }
    out
}

fn write_wav_i16(path: &PathBuf, sample_rate: u32, samples: &[f32]) -> Result<(), String> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for &s in samples {
        let clipped = s.clamp(-1.0, 1.0);
        let i = (clipped * i16::MAX as f32) as i16;
        writer.write_sample(i).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const SR: u32 = 16_000;

    fn silence(secs: f64) -> Vec<f32> {
        vec![0.0; (secs * SR as f64) as usize]
    }
    fn tone(secs: f64, freq: f32, amp: f32) -> Vec<f32> {
        let n = (secs * SR as f64) as usize;
        (0..n)
            .map(|i| amp * (2.0 * PI * freq * i as f32 / SR as f32).sin())
            .collect()
    }

    #[test]
    fn silence_is_zero_voiced() {
        assert_eq!(voiced_seconds(&silence(2.0), SR), 0.0);
    }

    #[test]
    fn empty_and_zero_rate_are_safe() {
        assert_eq!(voiced_seconds(&[], SR), 0.0);
        assert_eq!(voiced_seconds(&tone(1.0, 200.0, 0.3), 0), 0.0);
    }

    #[test]
    fn tone_in_silence_measures_only_the_tone() {
        // 0.5s silence | 1.0s tone | 0.5s silence  → voiced ≈ 1.0s, NOT 2.0s.
        let mut sig = silence(0.5);
        sig.extend(tone(1.0, 220.0, 0.4));
        sig.extend(silence(0.5));
        let v = voiced_seconds(&sig, SR);
        assert!(
            (v - 1.0).abs() < 0.12,
            "voiced {v:.3}s should be ~1.0s (clip is 2.0s)"
        );
        assert!(v < 1.5, "must exclude the leading/trailing silence");
    }

    #[test]
    fn short_interword_gap_is_bridged() {
        // word | 150ms gap | word  → one continuous voiced span (gap counted).
        let mut sig = tone(0.4, 200.0, 0.4);
        sig.extend(silence(0.15));
        sig.extend(tone(0.4, 200.0, 0.4));
        let v = voiced_seconds(&sig, SR);
        // ~0.95s (0.4 + 0.15 bridged + 0.4), definitely more than the 0.8s of pure tone.
        assert!(v > 0.85, "short gap should be bridged, got {v:.3}s");
        assert!(v < 1.1, "should not over-count, got {v:.3}s");
    }

    #[test]
    fn steady_signal_without_silence_is_not_collapsed_to_zero() {
        // Continuous speech / steady input has low dynamic range (no silence to
        // anchor a noise floor). A naive floor·3 gate rejects every frame → 0s,
        // which silently reverts WPM to the clip-length denominator. The
        // dynamic-range guard must keep (most of) a 2.0s steady tone as voiced.
        let sig = tone(2.0, 200.0, 0.4);
        let v = voiced_seconds(&sig, SR);
        assert!(
            v > 1.5,
            "steady tone must count as voiced (dynamic-range guard), got {v:.3}s"
        );
    }

    #[test]
    fn long_pause_is_not_bridged() {
        // word | 1.0s pause | word → the long thinking pause is excluded.
        let mut sig = tone(0.4, 200.0, 0.4);
        sig.extend(silence(1.0));
        sig.extend(tone(0.4, 200.0, 0.4));
        let v = voiced_seconds(&sig, SR);
        assert!(v < 1.0, "long pause must NOT be counted as speech, got {v:.3}s");
        assert!(v > 0.6, "both words should still count, got {v:.3}s");
    }
}
