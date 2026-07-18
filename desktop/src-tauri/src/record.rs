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
    /// Audio duration from mono 16 kHz samples — source of truth for WPM.
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
            "No audio file — click Allow once for Microphone (Wilson Voice) in the system dialog, then enable it under System Settings → Privacy → Microphone."
                .into(),
        );
    }
    let meta = std::fs::metadata(&active.wav_path).map_err(|e| e.to_string())?;
    if meta.len() < 1000 {
        return Err(format!(
            "Audio too short ({} bytes) — hold longer, or allow Microphone for Wilson Voice.",
            meta.len()
        ));
    }
    let speech_seconds = wav_duration_seconds(&active.wav_path).unwrap_or(hold_wall_seconds);
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
        "No microphone found. Click Dictate once so macOS prompts, then enable Wilson Voice under System Settings → Privacy → Microphone.".to_string()
    })?;

    let supported = device.default_input_config().map_err(|e| {
        format!(
            "Mic config failed ({e}). Enable Microphone for Wilson Voice (not Python) in System Settings."
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
        .map_err(|e| format!("mic start failed ({e}). Allow Microphone for Wilson Voice."))?;

    while !stop.load(Ordering::SeqCst) {
        thread::sleep(std::time::Duration::from_millis(50));
    }
    drop(stream);
    level.store(0, Ordering::Relaxed);

    let raw = samples.lock().map_err(|e| e.to_string())?.clone();
    if raw.is_empty() {
        return Err(
            "No samples captured. Enable Microphone for Wilson Voice in System Settings.".into(),
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
