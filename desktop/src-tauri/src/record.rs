//! In-process microphone capture via cpal (cpal 0.18).
//!
//! Recording inside the Wilson Voice process (not external ffmpeg) so macOS TCC
//! attributes Microphone permission to com.wilsonguenther.wilson-voice.
//! Live RMS level is exposed for the floating HUD waveform.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{WavSpec, WavWriter};
use nnnoiseless::DenoiseState;
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

pub fn start_recording(dir: PathBuf, denoise: bool) -> Result<ActiveRecording, String> {
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let wav_path = dir.join(format!("{}.wav", Uuid::new_v4()));
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = stop.clone();
    let path_t = wav_path.clone();
    let level: LevelHandle = Arc::new(AtomicU32::new(0));
    let level_t = level.clone();

    let join = thread::Builder::new()
        .name("wv-record".into())
        .spawn(move || record_loop(path_t, stop_t, level_t, denoise))
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
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(|s| s.ok()).collect(),
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
    denoise: bool,
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
                        let f: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
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
        return Err("No samples captured. Enable Microphone for Yap in System Settings.".into());
    }

    let mono = if channels <= 1 {
        raw
    } else {
        let ch = channels as usize;
        raw.chunks(ch)
            .map(|c| c.iter().sum::<f32>() / c.len() as f32)
            .collect()
    };
    // Signal hygiene (Tier 0, docs/research/voice-isolation.md) — cheap pure DSP
    // applied at the NATIVE rate BEFORE the 16 kHz downsample: kill sub-80 Hz
    // rumble/hum, lift a quiet voice toward a consistent level (peak-limited, no
    // clipping), then de-click the PTT press/release edges. Each stage returns
    // its input unchanged on degenerate audio, so a bad clip never NaNs the path.
    let mono = high_pass(&mono, sample_rate, HIGH_PASS_HZ);
    let mono = normalize_rms(&mono, NORMALIZE_TARGET_DBFS);
    let mono = edge_fade(&mono, sample_rate, EDGE_FADE_MS);
    // Denoise (Tier 1, docs/research/voice-isolation.md) — RNNoise over the
    // native-rate buffer AFTER hygiene, still BEFORE the 16 kHz downsample.
    // Gated by the user's `denoise` setting; the function itself falls back to
    // its input on any degeneracy so a bad clip never loses the utterance.
    let mono = if denoise {
        denoise_rnnoise(&mono, sample_rate)
    } else {
        mono
    };
    let target_rate = 16_000u32;
    let resampled = if sample_rate == target_rate {
        mono
    } else {
        resample_linear(&mono, sample_rate, target_rate)
    };

    write_wav_i16(&wav_path, target_rate, &resampled)?;
    log::info!("wrote {} samples → {}", resampled.len(), wav_path.display());
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

// ── Signal hygiene (Tier 0) ─────────────────────────────────────────────────
// Pure, deterministic DSP applied to the mono buffer before the 16 kHz resample.
// Kept as standalone `&[f32]`-in / `Vec<f32>`-out functions so each is unit
// testable in isolation. Every one is defensive: on empty/zero-rate/degenerate
// input (or if it would produce a non-finite sample) it returns the input
// unchanged rather than corrupting the pipeline.

/// High-pass corner — Whisper needs nothing below ~80 Hz for speech, and this
/// kills AC hum, HVAC rumble, and desk/handling thumps before they smear the
/// low bands.
const HIGH_PASS_HZ: f32 = 80.0;
/// RMS-normalize / soft-AGC target level. −20 dBFS is a comfortable speech level
/// that leaves headroom below full-scale so a boosted quiet voice never clips.
const NORMALIZE_TARGET_DBFS: f32 = -20.0;
/// Edge de-click fade length (ms) at clip start/end.
const EDGE_FADE_MS: f32 = 5.0;

/// Second-order (biquad) Butterworth high-pass at `cutoff_hz`. Removes DC offset,
/// mains hum, and low rumble. Returns the input unchanged on degenerate input or
/// if the filter ever goes non-finite.
fn high_pass(samples: &[f32], sample_rate: u32, cutoff_hz: f32) -> Vec<f32> {
    if samples.is_empty()
        || sample_rate == 0
        || cutoff_hz <= 0.0
        || cutoff_hz >= sample_rate as f32 / 2.0
    {
        return samples.to_vec();
    }
    // RBJ cookbook high-pass coefficients (Q = 1/√2 → maximally flat passband).
    let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate as f32;
    let (sin_w0, cos_w0) = w0.sin_cos();
    let q = std::f32::consts::FRAC_1_SQRT_2;
    let alpha = sin_w0 / (2.0 * q);
    let a0 = 1.0 + alpha;
    if a0 == 0.0 || !a0.is_finite() {
        return samples.to_vec();
    }
    let b0 = ((1.0 + cos_w0) / 2.0) / a0;
    let b1 = (-(1.0 + cos_w0)) / a0;
    let b2 = b0;
    let a1 = (-2.0 * cos_w0) / a0;
    let a2 = (1.0 - alpha) / a0;

    let mut out = Vec::with_capacity(samples.len());
    // Direct Form I state.
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for &x0 in samples {
        let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        if !y0.is_finite() {
            return samples.to_vec();
        }
        out.push(y0);
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
    }
    out
}

/// RMS-normalize toward `target_dbfs` (soft AGC), peak-limited so the boost never
/// clips, with a final hard limit to [-1, 1] as a safety net. Silence guard: a
/// clip whose RMS is at/below the noise floor is left untouched so we never
/// divide by ~0 and blow up the gain on a silent capture.
fn normalize_rms(samples: &[f32], target_dbfs: f32) -> Vec<f32> {
    if samples.is_empty() {
        return samples.to_vec();
    }
    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f32;
    for &s in samples {
        if !s.is_finite() {
            return samples.to_vec();
        }
        sum_sq += (s as f64) * (s as f64);
        let a = s.abs();
        if a > peak {
            peak = a;
        }
    }
    let rms = (sum_sq / samples.len() as f64).sqrt() as f32;
    // Silence / near-silence guard (~-80 dBFS): nothing to lift, avoid blowup.
    if !rms.is_finite() || rms < 1e-4 || peak <= 0.0 {
        return samples.to_vec();
    }
    let target_rms = 10.0f32.powf(target_dbfs / 20.0);
    // Gain toward target, but never enough to push the loudest sample past ~full
    // scale (peak-limit) → boost stays clip-free. Cap runaway gain on very quiet
    // input. Also refuses to invent signal (>0 always).
    let peak_ceiling = 0.99f32;
    let gain = (target_rms / rms)
        .min(peak_ceiling / peak)
        .min(64.0)
        .max(0.0);
    if !gain.is_finite() {
        return samples.to_vec();
    }
    let out: Vec<f32> = samples
        .iter()
        .map(|&s| (s * gain).clamp(-1.0, 1.0))
        .collect();
    if out.iter().any(|v| !v.is_finite()) {
        return samples.to_vec();
    }
    out
}

/// Short linear fade-in/out over `fade_ms` at each edge to de-click the
/// push-to-talk key press and release pop. Returns input unchanged on degenerate
/// input.
fn edge_fade(samples: &[f32], sample_rate: u32, fade_ms: f32) -> Vec<f32> {
    if samples.is_empty() || sample_rate == 0 || fade_ms <= 0.0 {
        return samples.to_vec();
    }
    let mut out = samples.to_vec();
    let n = out.len();
    let mut fade = (sample_rate as f32 * fade_ms / 1000.0) as usize;
    if fade == 0 {
        return out;
    }
    // Don't let the two fades overlap on a very short clip.
    if fade * 2 > n {
        fade = n / 2;
    }
    for i in 0..fade {
        let g = i as f32 / fade as f32;
        out[i] *= g;
        out[n - 1 - i] *= g;
    }
    if out.iter().any(|v| !v.is_finite()) {
        return samples.to_vec();
    }
    out
}

// ── Denoise (Tier 1) ────────────────────────────────────────────────────────
// RNNoise (nnnoiseless) over the whole clip. RNNoise is a 48 kHz model that
// consumes 480-sample (10 ms) frames of i16-range floats and keeps recurrent
// state, so for our BATCH path we resample to 48 kHz (if the mic isn't already
// there), stream every frame through in order, then resample back. Conservative
// by construction: on any degenerate input — or if the model would gut the
// signal (output energy collapses vs input) — it returns the input UNCHANGED so
// a bad clip never loses the utterance.

/// RNNoise's fixed operating rate.
const RNNOISE_SR: u32 = 48_000;

/// Plain RMS of a buffer (linear amplitude). Helper for the denoise collapse
/// guard and its unit tests.
fn rms_energy(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&x| (x as f64) * (x as f64)).sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// Suppress steady background noise (fans, hum, keyboard hiss) with RNNoise.
/// Input/output are mono `[-1, 1]` floats at `sample_rate`. Falls back to the
/// input unchanged on any degeneracy or over-suppression — never returns silence
/// for a real utterance.
fn denoise_rnnoise(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    const FRAME: usize = DenoiseState::FRAME_SIZE; // 480 samples @ 48 kHz
    if samples.len() < FRAME || sample_rate == 0 || samples.iter().any(|s| !s.is_finite()) {
        return samples.to_vec();
    }
    // RNNoise only speaks 48 kHz.
    let need_resample = sample_rate != RNNOISE_SR;
    let at48 = if need_resample {
        resample_linear(samples, sample_rate, RNNOISE_SR)
    } else {
        samples.to_vec()
    };
    if at48.len() < FRAME {
        return samples.to_vec();
    }

    let mut state = DenoiseState::new();
    let mut in_frame = [0.0f32; FRAME];
    let mut out_frame = [0.0f32; FRAME];
    // Process one extra silent frame past the end to flush RNNoise's one-frame
    // algorithmic (overlap-add) delay, then drop that leading frame so the
    // denoised stream re-aligns sample-for-sample with the input.
    let total = at48.len() + FRAME;
    let mut denoised = Vec::with_capacity(total);
    let mut idx = 0;
    while idx < total {
        for (k, slot) in in_frame.iter_mut().enumerate() {
            let src = idx + k;
            // RNNoise wants i16-range floats, not [-1, 1].
            *slot = if src < at48.len() {
                at48[src] * i16::MAX as f32
            } else {
                0.0
            };
        }
        state.process_frame(&mut out_frame, &in_frame);
        for &o in out_frame.iter() {
            denoised.push(o / i16::MAX as f32);
        }
        idx += FRAME;
    }
    let aligned: Vec<f32> = denoised.into_iter().skip(FRAME).take(at48.len()).collect();

    let restored = if need_resample {
        resample_linear(&aligned, RNNOISE_SR, sample_rate)
    } else {
        aligned
    };

    // Never lose the utterance: bail to the raw input on empty / non-finite
    // output, or if denoise would collapse the signal to near-nothing.
    if restored.is_empty() || restored.iter().any(|s| !s.is_finite()) {
        return samples.to_vec();
    }
    let in_rms = rms_energy(samples);
    let out_rms = rms_energy(&restored);
    if in_rms > 1e-5 && out_rms < in_rms * 0.05 {
        return samples.to_vec();
    }
    restored
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
        assert!(
            v < 1.0,
            "long pause must NOT be counted as speech, got {v:.3}s"
        );
        assert!(v > 0.6, "both words should still count, got {v:.3}s");
    }

    // ── Signal hygiene (Tier 0) ─────────────────────────────────────────────
    // Native capture is ~48 kHz, so exercise the hygiene fns at that rate.
    const NATIVE_SR: u32 = 48_000;

    /// Sine at an arbitrary sample rate.
    fn tone_at(sr: u32, secs: f64, freq: f32, amp: f32) -> Vec<f32> {
        let n = (secs * sr as f64) as usize;
        (0..n)
            .map(|i| amp * (2.0 * PI * freq * i as f32 / sr as f32).sin())
            .collect()
    }

    fn rms(s: &[f32]) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        let sum: f64 = s.iter().map(|&x| (x as f64) * (x as f64)).sum();
        (sum / s.len() as f64).sqrt() as f32
    }
    fn to_dbfs(x: f32) -> f32 {
        20.0 * x.max(1e-12).log10()
    }
    fn max_abs(s: &[f32]) -> f32 {
        s.iter().fold(0.0f32, |m, &x| m.max(x.abs()))
    }

    #[test]
    fn high_pass_attenuates_low_freq_and_preserves_speech_band() {
        // A 25 Hz rumble tone should be knocked down hard; a 300 Hz speech-band
        // tone should pass through essentially untouched.
        let low = tone_at(NATIVE_SR, 1.0, 25.0, 0.5);
        let mid = tone_at(NATIVE_SR, 1.0, 300.0, 0.5);
        let low_hp = high_pass(&low, NATIVE_SR, HIGH_PASS_HZ);
        let mid_hp = high_pass(&mid, NATIVE_SR, HIGH_PASS_HZ);
        // Measure over the back half to skip the filter's start-up transient.
        let half = |v: &[f32]| v[v.len() / 2..].to_vec();

        let low_atten_db = to_dbfs(rms(&half(&low))) - to_dbfs(rms(&half(&low_hp)));
        assert!(
            low_atten_db > 12.0,
            "25 Hz rumble should be attenuated (got {low_atten_db:.1} dB)"
        );
        let mid_atten_db = to_dbfs(rms(&half(&mid))) - to_dbfs(rms(&half(&mid_hp)));
        assert!(
            mid_atten_db.abs() < 1.5,
            "300 Hz speech band should be preserved (got {mid_atten_db:.2} dB change)"
        );
        assert!(low_hp.len() == low.len() && !low_hp.iter().any(|v| !v.is_finite()));
    }

    #[test]
    fn normalize_lifts_quiet_sine_to_target_without_clipping() {
        // A very quiet (~-40 dBFS RMS) sine must be lifted toward the -20 dBFS
        // target within a few dB, with NO sample exceeding full scale.
        let quiet = tone_at(NATIVE_SR, 1.0, 220.0, 0.01414); // RMS ≈ 0.01 → -40 dBFS
        assert!(
            (to_dbfs(rms(&quiet)) - (-40.0)).abs() < 1.0,
            "fixture should sit near -40 dBFS, got {:.1}",
            to_dbfs(rms(&quiet))
        );
        let norm = normalize_rms(&quiet, NORMALIZE_TARGET_DBFS);
        let out_dbfs = to_dbfs(rms(&norm));
        assert!(
            (out_dbfs - NORMALIZE_TARGET_DBFS).abs() < 3.0,
            "normalized RMS {out_dbfs:.1} dBFS should be within a few dB of {NORMALIZE_TARGET_DBFS}"
        );
        assert!(
            max_abs(&norm) <= 1.0,
            "no sample may exceed full scale, got peak {}",
            max_abs(&norm)
        );
        assert!(norm.len() == quiet.len());
    }

    #[test]
    fn normalize_hard_limits_and_never_exceeds_full_scale() {
        // Even a hot / already-loud signal stays within [-1, 1] after normalize.
        let loud = tone_at(NATIVE_SR, 0.5, 300.0, 0.9);
        let norm = normalize_rms(&loud, NORMALIZE_TARGET_DBFS);
        assert!(
            max_abs(&norm) <= 1.0,
            "peak {} must be ≤ 1.0",
            max_abs(&norm)
        );
        assert!(!norm.iter().any(|v| !v.is_finite()));
    }

    #[test]
    fn normalize_silence_is_left_unchanged() {
        // Pure silence has no level to lift → guard must return it untouched
        // (no divide-by-noise blowup, no NaN).
        let sil = vec![0.0f32; NATIVE_SR as usize / 2];
        let out = normalize_rms(&sil, NORMALIZE_TARGET_DBFS);
        assert_eq!(out, sil);
    }

    #[test]
    fn edge_fade_zeroes_the_edges_and_keeps_the_body() {
        let mut sig = tone_at(NATIVE_SR, 0.2, 300.0, 0.8);
        // Force non-zero endpoints so a fade is observable.
        *sig.first_mut().unwrap() = 0.8;
        *sig.last_mut().unwrap() = -0.8;
        let faded = edge_fade(&sig, NATIVE_SR, EDGE_FADE_MS);
        assert_eq!(faded.len(), sig.len());
        assert!(faded[0].abs() < 1e-6, "start must fade in from 0");
        assert!(
            faded[faded.len() - 1].abs() < 1e-6,
            "end must fade out to 0"
        );
        // The middle is untouched.
        let mid = faded.len() / 2;
        assert!((faded[mid] - sig[mid]).abs() < 1e-6);
    }

    #[test]
    fn hygiene_never_yields_nan_or_empty() {
        // A realistic noisy, quiet capture run through the full chain must stay
        // finite and non-empty, at the natural length.
        let mut sig = tone_at(NATIVE_SR, 0.5, 180.0, 0.02);
        for (i, s) in sig.iter_mut().enumerate() {
            // add rumble + a little broadband texture
            *s += 0.03 * (2.0 * PI * 30.0 * i as f32 / NATIVE_SR as f32).sin();
            *s += if i % 7 == 0 { 0.01 } else { -0.005 };
        }
        let chained = edge_fade(
            &normalize_rms(
                &high_pass(&sig, NATIVE_SR, HIGH_PASS_HZ),
                NORMALIZE_TARGET_DBFS,
            ),
            NATIVE_SR,
            EDGE_FADE_MS,
        );
        assert_eq!(chained.len(), sig.len(), "length must be preserved");
        assert!(!chained.is_empty(), "must not be emptied");
        assert!(
            !chained.iter().any(|v| !v.is_finite()),
            "no NaN/Inf may be produced"
        );
        assert!(
            max_abs(&chained) <= 1.0,
            "output must stay within full scale"
        );

        // Degenerate inputs are handled without panicking or producing NaN.
        assert!(high_pass(&[], NATIVE_SR, HIGH_PASS_HZ).is_empty());
        assert!(normalize_rms(&[], NORMALIZE_TARGET_DBFS).is_empty());
        assert!(edge_fade(&[], NATIVE_SR, EDGE_FADE_MS).is_empty());
        assert_eq!(high_pass(&[0.5], 0, HIGH_PASS_HZ), vec![0.5]); // zero rate → unchanged
                                                                   // Single sample doesn't panic on the edge-fade halving.
        let one = edge_fade(&[0.7], NATIVE_SR, EDGE_FADE_MS);
        assert_eq!(one.len(), 1);
        assert!(one[0].is_finite());
    }

    // ── Denoise (Tier 1) ────────────────────────────────────────────────────
    // Exercise RNNoise at its native 48 kHz so no resample distortion clouds the
    // measurement. A voiced, speech-like fixture (fundamental + harmonics) is
    // what RNNoise is trained to keep; broadband noise is what it removes.

    /// Deterministic broadband noise in [-amp, amp] via a small xorshift PRNG —
    /// no `rand` dependency, fully reproducible so the test never flakes.
    fn broadband_noise(len: usize, amp: f32, seed: u64) -> Vec<f32> {
        let mut s = seed | 1;
        (0..len)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                // Map to [-1, 1) then scale.
                let u = (s >> 11) as f32 / (1u64 << 53) as f32; // [0, 1)
                (u * 2.0 - 1.0) * amp
            })
            .collect()
    }

    /// A voiced, speech-like tone: a ~140 Hz fundamental plus a few harmonics
    /// with a falling (formant-ish) envelope. RNNoise recognizes this as voiced
    /// and should preserve it.
    fn voiced(secs: f64, sr: u32, amp: f32) -> Vec<f32> {
        let n = (secs * sr as f64) as usize;
        let f0 = 140.0f32;
        let harmonics = [(1.0, 1.0), (2.0, 0.6), (3.0, 0.35), (4.0, 0.2), (5.0, 0.12)];
        (0..n)
            .map(|i| {
                let t = i as f32 / sr as f32;
                let mut v = 0.0f32;
                for (mult, gain) in harmonics {
                    v += gain * (2.0 * PI * f0 * mult * t).sin();
                }
                amp * v
            })
            .collect()
    }

    /// Best-aligned residual (noise) power between a clean reference and a
    /// processed signal, searching a small delay window so any leftover
    /// algorithmic offset can't masquerade as distortion.
    fn residual_power(clean: &[f32], processed: &[f32], max_shift: usize) -> f64 {
        let n = clean.len().min(processed.len());
        let mut best = f64::INFINITY;
        for shift in 0..=max_shift {
            let mut sum = 0.0f64;
            let mut cnt = 0usize;
            for i in 0..n {
                if i + shift >= processed.len() {
                    break;
                }
                let e = clean[i] as f64 - processed[i + shift] as f64;
                sum += e * e;
                cnt += 1;
            }
            if cnt > 0 {
                let p = sum / cnt as f64;
                if p < best {
                    best = p;
                }
            }
        }
        best
    }

    fn power(s: &[f32]) -> f64 {
        if s.is_empty() {
            return 0.0;
        }
        s.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / s.len() as f64
    }

    #[test]
    fn denoise_improves_snr_on_a_noisy_fixture() {
        // Voiced speech-like signal + broadband noise at ~equal power (~0 dB in).
        let clean = voiced(2.0, NATIVE_SR, 0.10);
        let noise = broadband_noise(clean.len(), 0.06, 0x9E37_79B9_7F4A_7C15);
        let noisy: Vec<f32> = clean.iter().zip(&noise).map(|(&c, &n)| c + n).collect();

        let denoised = denoise_rnnoise(&noisy, NATIVE_SR);
        assert_eq!(denoised.len(), noisy.len(), "length must be preserved");

        // SNR = clean power / residual (vs clean) power. RNNoise knocks the noise
        // floor down while keeping the voice, so the residual shrinks → SNR up.
        let shift = DenoiseState::FRAME_SIZE; // tolerate up to one frame of offset
        let sig_power = power(&clean);
        let snr_in = 10.0 * (sig_power / residual_power(&clean, &noisy, shift)).log10();
        let snr_out = 10.0 * (sig_power / residual_power(&clean, &denoised, shift)).log10();
        let improvement = snr_out - snr_in;
        assert!(
            improvement >= 6.0,
            "denoise should raise SNR ≥ 6 dB (in {snr_in:.1} → out {snr_out:.1}, +{improvement:.1} dB)"
        );
    }

    #[test]
    fn denoise_preserves_clean_speech_within_1db() {
        // A clean voiced clip must survive denoise with ≤ ~1 dB energy loss —
        // no over-suppression of the user's own words.
        let clean = voiced(2.0, NATIVE_SR, 0.12);
        let denoised = denoise_rnnoise(&clean, NATIVE_SR);
        assert_eq!(denoised.len(), clean.len());
        let loss_db = 10.0 * (power(&clean) / power(&denoised)).log10();
        assert!(
            loss_db <= 1.0,
            "clean speech should lose ≤ 1 dB of energy, lost {loss_db:.2} dB"
        );
        assert!(!denoised.iter().any(|v| !v.is_finite()));
    }

    #[test]
    fn denoise_falls_back_to_input_unchanged() {
        // Degenerate inputs must return the input UNCHANGED (never lose audio):
        //   • empty buffer
        //   • zero sample rate
        //   • too short to fill a single 480-sample frame
        //   • non-finite samples
        assert!(denoise_rnnoise(&[], NATIVE_SR).is_empty());

        let short = voiced(0.005, NATIVE_SR, 0.2); // < FRAME_SIZE samples
        assert!(short.len() < DenoiseState::FRAME_SIZE);
        assert_eq!(denoise_rnnoise(&short, NATIVE_SR), short);

        let some = voiced(0.05, NATIVE_SR, 0.2);
        assert_eq!(denoise_rnnoise(&some, 0), some, "zero rate → unchanged");

        let mut nan = voiced(0.05, NATIVE_SR, 0.2);
        nan[10] = f32::NAN;
        assert!(
            denoise_rnnoise(&nan, NATIVE_SR)
                .iter()
                .zip(&nan)
                .all(|(&a, &b)| a.to_bits() == b.to_bits()),
            "non-finite input must be returned byte-identical (unchanged)"
        );
    }
}
