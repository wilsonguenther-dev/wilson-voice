//! In-process microphone capture via cpal (cpal 0.18).
//!
//! Recording inside the Wilson Voice process (not external ffmpeg) so macOS TCC
//! attributes Microphone permission to com.wilsonguenther.wilson-voice.
//! Live RMS level is exposed for the floating HUD waveform.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{WavSpec, WavWriter};
use nnnoiseless::DenoiseState;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::vad;

/// Shared peak level 0..=1000 for HUD (updated every audio callback window).
pub type LevelHandle = Arc<AtomicU32>;

/// Every clip is written — and every ASR engine is fed — at 16 kHz mono.
const TARGET_RATE: u32 = 16_000;
/// Shortest take worth transcribing: 30 ms at the target rate. Below this it was
/// a stray tap, not speech (this used to be a "wav smaller than 1 kB" check —
/// same threshold, measured on the in-memory buffer now).
const MIN_CLIP_SAMPLES: usize = TARGET_RATE as usize * 30 / 1000;

pub struct ActiveRecording {
    stop: Arc<AtomicBool>,
    wav_path: PathBuf,
    started: Instant,
    pub level: LevelHandle,
    /// The user's denoise setting, captured at arm time and applied in
    /// `finalize_take` — the one DSP stage that genuinely cannot stream (YV37:
    /// high-pass, AGC accumulation and the 16 kHz resample all run inline on
    /// the capture frames, so only the finalize remains after release).
    denoise: bool,
    /// YV35 telemetry: key-press → capture-start, in ms.
    capture_start_ms: i64,
}

impl ActiveRecording {
    /// Shared stop flag — false while recording, true after stop.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }
}

pub struct RecordingResult {
    /// The take as 16 kHz mono `[-1, 1]` floats — DSP'd during capture (YV37)
    /// and handed STRAIGHT to the ASR engine. Nothing on the transcribe path
    /// touches the disk any more: the clip used to be written, read back,
    /// sometimes rewritten and re-decoded before a single sample reached
    /// Whisper.
    pub samples: Vec<f32>,
    /// History/recovery WAV for this take, written on a background thread while
    /// ASR runs (YV37) and unlinked when this drops — see `ClipWav`.
    pub clip: ClipWav,
    /// VOICED seconds (energy VAD) — the denominator for WPM. This is real
    /// speaking time: leading/trailing silence and long thinking pauses are
    /// excluded, natural inter-word gaps are kept. NOT the raw clip length
    /// (that inflates the denominator and makes WPM read low + jittery).
    pub speech_seconds: f64,
    /// TRUE energy-VAD voiced seconds — 0.0 on silence, with NO clip-length
    /// fallback (unlike `speech_seconds`, which floors to the raw clip so WPM
    /// never divides by ~0). This is the honest "did the user actually speak?"
    /// signal: the no-speech gate reads it to reject a near-silent tap before
    /// ASR, so Whisper never hallucinates repetitive garbage on silence.
    pub voiced_seconds: f64,
    /// Wall-clock hold (press→release), for latency telemetry only.
    pub hold_wall_seconds: f64,
    /// YV36 voice isolation — Silero VAD's verdict on whether the clip actually
    /// contains speech, used to STRENGTHEN the no-speech gate (complementing the
    /// energy VAD above). `true` whenever Silero is unavailable/errored (safe
    /// fallback: never reject on a missing model), `true` when Silero found a
    /// voiced region, and `false` ONLY when Silero ran successfully and detected
    /// no speech at all — the one case energy VAD can miss (e.g. a steady fan the
    /// RMS gate counts as "voiced"). The gate rejects a clip when this is false.
    pub speech_present: bool,
    /// YV35 telemetry: key-press → capture-start, in ms. Carried through from
    /// `ActiveRecording` so the single latency log line can also show the press
    /// side of the pipeline — it used to be invisible, and used to include a
    /// fixed start-poll wait paid on every take.
    pub capture_start_ms: i64,
}

/// Arm a take on the persistent capture worker (YV35).
///
/// The worker keeps the cpal input stream — and its cached device + stream
/// config — alive across dictations, so a keypress no longer pays device open +
/// stream build + `play()`. Arming is an event handshake: we block only until
/// the worker acknowledges the stream is live and buffering (microseconds on the
/// warm path), bounded by `ARM_TIMEOUT` so a wedged or denied device surfaces an
/// error instead of hanging the hotkey. This used to be a fixed third-of-a-second poll loop
/// paid on EVERY take, because a healthy capture thread never finishes.
///
/// `pressed_at` is the instant the PTT combo went down when the caller knows it
/// — the anchor for the press→capture_start span. `None` (tray/button starts)
/// measures from this call instead.
pub fn start_recording(
    dir: PathBuf,
    denoise: bool,
    pressed_at: Option<Instant>,
) -> Result<ActiveRecording, String> {
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let wav_path = dir.join(format!("{}.wav", Uuid::new_v4()));
    // The hold clock starts at the request, NOT after the handshake, so
    // `hold_wall_seconds` reports the real press→release wall time.
    let started = Instant::now();

    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    dispatch(CaptureCmd::Arm { reply: reply_tx })?;
    await_reply(&reply_rx, ARM_TIMEOUT, "arm")?;

    let capture_start_ms = pressed_at.unwrap_or(started).elapsed().as_millis() as i64;
    log::info!(
        "capture armed: press→capture_start={capture_start_ms}ms (arm_wait={}ms)",
        started.elapsed().as_millis()
    );

    Ok(ActiveRecording {
        stop: Arc::new(AtomicBool::new(false)),
        wav_path,
        started,
        level: capture_level().clone(),
        denoise,
        capture_start_ms,
    })
}

pub fn stop_recording(
    active: ActiveRecording,
    isolation_vad: Option<&vad::WarmVad>,
) -> Result<RecordingResult, String> {
    let hold_wall_seconds = active.started.elapsed().as_secs_f64().max(0.01);
    active.stop.store(true, Ordering::SeqCst);
    active.level.store(0, Ordering::Relaxed);

    // Stop buffering and take the raw capture off the persistent worker; the
    // stream itself stays open for the next take (see `capture_worker_loop`).
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    dispatch(CaptureCmd::Disarm { reply: reply_tx })?;
    let captured = await_reply(&reply_rx, DISARM_TIMEOUT, "stop")?;
    // Everything that CAN stream already ran on the capture frames (YV37); this
    // only finishes the take — apply the accumulated AGC gain, de-click the
    // edges, optionally denoise — and yields the 16 kHz mono buffer ASR reads.
    let mut samples = finalize_take(captured, active.denoise)?;

    if samples.len() < MIN_CLIP_SAMPLES {
        // Sub-second tap: no wav was ever written for it (the recovery write is
        // spawned below, after the take is known good), so nothing to clean up.
        return Err(format!(
            "Audio too short ({} ms) — hold longer, or allow Microphone for Yap.",
            samples.len() * 1000 / TARGET_RATE as usize
        ));
    }
    // WPM denominator = real voiced time. Fall back to the raw clip length only
    // when VAD can't find speech (near-silent capture) so WPM never divides by ~0.
    let clip_seconds = samples.len() as f64 / TARGET_RATE as f64;
    // Both the energy VAD (WPM denominator) and the Silero voice-isolation pass
    // below read the SAME in-memory buffer — no wav read-back (YV37).
    let voiced = voiced_seconds(&samples, TARGET_RATE);
    let speech_seconds = if voiced >= 0.1 { voiced } else { clip_seconds };

    // YV36 — Silero VAD voice isolation through the WARM engine loaded once at
    // startup (`vad::WarmVad`, held in the app state). `None` means no model on
    // disk / the load failed, and ANY inference failure falls back to the
    // energy-VAD path with `speech_present = true` so an utterance is NEVER lost.
    let mut speech_present = true;
    if let Some(warm) = isolation_vad {
        match warm.isolate(&samples, TARGET_RATE) {
            Ok(iso) => {
                speech_present = iso.has_speech;
                // (b) Trim the ASR buffer to the voiced span so Whisper decodes
                // only the speech (never trim to empty — if Silero found speech we
                // keep its padded span). Since YV37 this is a plain in-memory swap
                // instead of a wav rewrite. We do NOT touch `voiced`/`speech_seconds`:
                // WPM stays anchored to the energy VAD over the full utterance.
                let did_trim = iso.trimmed.is_some();
                if iso.has_speech {
                    if let Some(trimmed) = iso.trimmed {
                        if !trimmed.is_empty() {
                            samples = trimmed;
                        }
                    }
                }
                log::info!(
                    "YV36 silero: has_speech={} voiced={:.2}s trimmed={}",
                    iso.has_speech,
                    iso.voiced_seconds,
                    did_trim
                );
            }
            Err(e) => {
                // Safe fallback — energy VAD alone decides the gate.
                log::warn!("YV36 silero unavailable, using energy VAD: {e}");
            }
        }
    }

    // History/recovery wav — spawned AFTER the ASR buffer is final, so the write
    // runs alongside transcription + paste instead of sitting between key-release
    // and ASR (YV37). The guard unlinks it when the take is done.
    let clip = ClipWav::spawn(active.wav_path, samples.clone());

    Ok(RecordingResult {
        samples,
        clip,
        speech_seconds: speech_seconds.max(0.05),
        // The TRUE voiced value — no clip-length fallback, so a silent tap reads
        // 0.0 and the no-speech gate can reject it before ASR.
        voiced_seconds: voiced,
        hold_wall_seconds,
        speech_present,
        capture_start_ms: active.capture_start_ms,
    })
}

/// Pure sweep predicate (YV20/M3): does this dir-entry name look like a captured
/// voice clip we should remove at startup? Case-insensitive `*.wav`, non-empty
/// stem. Kept pure so it's unit-testable without touching the filesystem.
fn is_stale_wav(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".wav") && lower.len() > ".wav".len()
}

/// Remove every stale `*.wav` from the recordings dir. Called once at startup —
/// no capture is in flight then, so any leftover clip is orphaned (a prior crash
/// or hard-kill that couldn't run its own cleanup). Returns how many were removed.
pub fn sweep_stale_wavs(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        if is_stale_wav(&name.to_string_lossy()) && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// The take's history/recovery WAV. YV37 moved this write OFF the critical path:
/// `spawn` hands the finished 16 kHz samples to a background thread that writes
/// while ASR + paste run on the in-memory buffer, instead of the release→ASR gap
/// paying for it. Dropping the guard unlinks the file — YV20/M3: the clip is
/// disposable and never outlives the pipeline, on success or error alike — and
/// joins the writer first so the delete can never race it and leak a wav.
pub struct ClipWav {
    path: PathBuf,
    writer: Option<thread::JoinHandle<()>>,
}

impl ClipWav {
    fn spawn(path: PathBuf, samples: Vec<f32>) -> Self {
        let target = path.clone();
        let writer = thread::Builder::new()
            .name("wv-clip-wav".into())
            .spawn(
                move || match write_wav_i16(&target, TARGET_RATE, &samples) {
                    Ok(()) => log::info!("wrote {} samples → {}", samples.len(), target.display()),
                    // Non-fatal by design: the transcript comes from memory now, so a
                    // failed history write can never cost the user their words.
                    Err(e) => log::warn!("history wav write failed (transcript unaffected): {e}"),
                },
            )
            .ok();
        Self { path, writer }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ClipWav {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Read a WAV off disk as the 16 kHz mono `[-1, 1]` f32 buffer the embedded ASR
/// engine consumes (YV32). Our own clips are already mono/16 kHz (see
/// `record_loop`), so this is a straight read for the dictation path; a foreign
/// file (the CLI's `--transcribe-file`, a test fixture) is downmixed and
/// linearly resampled with the same helper the capture path uses.
pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let spec = reader.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 * scale)
                .collect()
        }
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(|s| s.ok()).collect(),
    };
    if interleaved.is_empty() || spec.sample_rate == 0 {
        return Err(format!("no audio samples in {}", path.display()));
    }
    let mono = if spec.channels <= 1 {
        interleaved
    } else {
        let ch = spec.channels as usize;
        interleaved
            .chunks(ch)
            .map(|c| c.iter().sum::<f32>() / c.len() as f32)
            .collect()
    };
    let out = if spec.sample_rate == TARGET_RATE {
        mono
    } else {
        resample_linear(&mono, spec.sample_rate, TARGET_RATE)
    };
    if out.is_empty() {
        return Err(format!("no audio samples in {}", path.display()));
    }
    Ok(out)
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

// ── Persistent capture worker (YV35) ────────────────────────────────────────
// One long-lived thread owns the cpal input stream and keeps it — plus the
// device + stream config it was opened with — alive ACROSS dictations, so a
// keypress pays no device open, no `build_input_stream` and no `play()`. Arm and
// stop are event handshakes over channels: the caller is released the moment the
// worker acknowledges, which replaces the fixed start poll (a third of a second,
// paid on EVERY take, because a healthy capture thread never finishes) and the
// 50 ms stop poll of the old per-take capture thread.
//
// The stream is closed after `IDLE_CLOSE` without a take so the macOS mic
// indicator never stays lit for a session Yap isn't recording in; the next take
// reopens it straight from the cached device + config. Any stream error (device
// unplugged, sample-rate change) invalidates the cache AND drops the stream, so
// the next arm re-queries the hardware instead of feeding off a dead device.

/// Bounded wait for the worker to confirm the stream is live and buffering.
/// Microseconds on the warm path; only a cold open (or a permission prompt) gets
/// anywhere near it. On expiry the caller surfaces the error (`transcript_error`).
const ARM_TIMEOUT: Duration = Duration::from_secs(3);
/// Bounded wait for the worker to hand the captured buffer back on release.
const DISARM_TIMEOUT: Duration = Duration::from_secs(5);
/// Close the persistent stream after this long without a take (mic indicator off).
const IDLE_CLOSE: Duration = Duration::from_secs(60);
/// How often the idle worker wakes to test the idle window.
const IDLE_TICK: Duration = Duration::from_secs(5);

const NO_MIC_ERR: &str = "No microphone found. Click Dictate once so macOS prompts, then enable Yap under System Settings → Privacy → Microphone.";
const NO_SAMPLES_ERR: &str = "No samples captured. Enable Microphone for Yap in System Settings.";
const NO_AUDIO_ERR: &str = "No audio captured — click Allow once for Microphone (Yap) in the system dialog, then enable it under System Settings → Privacy → Microphone.";

/// One take as it comes off the capture worker. Since YV37 the frames are
/// already downmixed, high-passed and resampled to 16 kHz *during* capture, so
/// this is nearly the finished ASR buffer — `finalize_take` only levels, fades
/// and (optionally) denoises it.
struct CapturedAudio {
    /// 16 kHz mono, high-passed — streamed frame by frame while the user spoke.
    samples: Vec<f32>,
    /// The same take as raw mono at `sample_rate`, untouched by any filter. The
    /// never-lose-audio fallback: if the streamed buffer is empty or went
    /// non-finite, `finalize_take` rebuilds the clip from this instead.
    raw: Vec<f32>,
    /// Native capture rate — the rate `raw` is at (`samples` is always 16 kHz).
    sample_rate: u32,
    /// Soft-AGC gain accumulated over the take, applied at finalize. `1.0` means
    /// "leave the level alone" (silence / degenerate input).
    gain: f32,
}

enum CaptureCmd {
    /// Begin buffering into the persistent stream (opening it if needed).
    Arm {
        reply: mpsc::SyncSender<Result<(), String>>,
    },
    /// Stop buffering and hand back the take. The stream STAYS open.
    Disarm {
        reply: mpsc::SyncSender<Result<CapturedAudio, String>>,
    },
}

/// The HUD level meter, shared by every take: only one capture is ever in
/// flight, and the persistent stream's callback outlives any single recording.
fn capture_level() -> &'static LevelHandle {
    static LEVEL: OnceLock<LevelHandle> = OnceLock::new();
    LEVEL.get_or_init(|| Arc::new(AtomicU32::new(0)))
}

/// Wait for a worker acknowledgement — the event signal that replaced YV35's
/// fixed poll loops. Returns the instant the worker answers; the bounded timeout
/// keeps a wedged or permission-denied device from hanging the hotkey forever.
/// Pure over the channel → unit tested.
fn await_reply<T>(
    rx: &mpsc::Receiver<Result<T, String>>,
    timeout: Duration,
    what: &str,
) -> Result<T, String> {
    match rx.recv_timeout(timeout) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(e),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "Microphone {what} timed out after {}ms — check Yap's Microphone permission under System Settings → Privacy.",
            timeout.as_millis()
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(format!("Microphone {what} failed — capture worker stopped."))
        }
    }
}

/// Name-keyed cache of the input device + the stream config it accepted. The HAL
/// property queries behind `default_input_config` cost tens of ms per open on
/// macOS (worse on USB/Bluetooth) and land straight on the keypress→capture path
/// whenever the persistent stream has to be reopened. Keyed by device name so
/// switching the system default misses naturally; `invalidate` is called on ANY
/// open or stream error so a stale rate/format self-heals on the next take.
/// Generic over the device/config types so the logic is unit-testable without
/// touching audio hardware.
struct DeviceConfigCache<D, C> {
    entry: Option<(String, D, C)>,
}

impl<D: Clone, C: Clone> DeviceConfigCache<D, C> {
    fn new() -> Self {
        Self { entry: None }
    }

    /// Cached device + config for `name`, or `None` on a miss. An empty name is
    /// NEVER a hit: cpal reports it when it cannot identify the device, and
    /// caching under it would pin an unknown device forever.
    fn get(&self, name: &str) -> Option<(D, C)> {
        match &self.entry {
            Some((cached, device, config)) if !name.is_empty() && cached == name => {
                Some((device.clone(), config.clone()))
            }
            _ => None,
        }
    }

    fn store(&mut self, name: String, device: D, config: C) {
        if name.is_empty() {
            self.entry = None;
            return;
        }
        self.entry = Some((name, device, config));
    }

    fn invalidate(&mut self) {
        self.entry = None;
    }

    /// Whether anything is cached at all — the invalidation assertions read it.
    #[cfg(test)]
    fn is_cached(&self) -> bool {
        self.entry.is_some()
    }
}

/// The live cpal input stream plus everything its callback writes into. Owned by
/// the worker thread (`cpal::Stream` is not `Send`) and kept across takes.
struct LiveStream {
    _stream: cpal::Stream,
    /// Streaming DSP + buffers, written by the audio callback (YV37).
    dsp: Arc<Mutex<StreamDsp>>,
    capturing: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
}

impl LiveStream {
    /// Begin a take: drop anything the callback saw while idle (and every bit of
    /// the previous take's DSP state), zero the meter, then open the gate.
    fn begin(&self) {
        if let Ok(mut dsp) = self.dsp.lock() {
            dsp.reset();
        }
        capture_level().store(0, Ordering::Relaxed);
        self.capturing.store(true, Ordering::SeqCst);
    }

    /// End a take: close the gate and take the streamed 16 kHz buffer (plus its
    /// raw fallback and AGC stats) off the DSP state.
    fn end(&self) -> CapturedAudio {
        self.capturing.store(false, Ordering::SeqCst);
        capture_level().store(0, Ordering::Relaxed);
        self.dsp
            .lock()
            .map(|mut dsp| dsp.take())
            .unwrap_or_else(|_| CapturedAudio {
                samples: Vec::new(),
                raw: Vec::new(),
                sample_rate: TARGET_RATE,
                gain: 1.0,
            })
    }

    fn is_capturing(&self) -> bool {
        self.capturing.load(Ordering::SeqCst)
    }

    fn has_failed(&self) -> bool {
        self.failed.load(Ordering::SeqCst)
    }
}

/// Send a command to the persistent worker, spawning (or respawning) it when it
/// is not running — a worker that died took its stream with it, so the next
/// command transparently gets a fresh one.
fn dispatch(cmd: CaptureCmd) -> Result<(), String> {
    static WORKER: OnceLock<Mutex<Option<mpsc::Sender<CaptureCmd>>>> = OnceLock::new();
    let slot = WORKER.get_or_init(|| Mutex::new(None));
    let mut guard = slot
        .lock()
        .map_err(|_| "capture worker lock poisoned".to_string())?;

    let mut cmd = cmd;
    if let Some(tx) = guard.as_ref() {
        match tx.send(cmd) {
            Ok(()) => return Ok(()),
            // Receiver gone → the worker thread is dead; respawn below.
            Err(mpsc::SendError(returned)) => {
                cmd = returned;
                *guard = None;
            }
        }
    }

    let (tx, rx) = mpsc::channel::<CaptureCmd>();
    thread::Builder::new()
        .name("wv-capture".into())
        .spawn(move || capture_worker_loop(rx))
        .map_err(|e| format!("spawn capture worker: {e}"))?;
    tx.send(cmd)
        .map_err(|_| "capture worker exited immediately".to_string())?;
    *guard = Some(tx);
    Ok(())
}

/// The worker thread: owns the persistent stream + the device/config cache and
/// answers every arm/stop with an explicit signal (never a poll).
fn capture_worker_loop(rx: mpsc::Receiver<CaptureCmd>) {
    let mut cache: DeviceConfigCache<cpal::Device, cpal::SupportedStreamConfig> =
        DeviceConfigCache::new();
    let mut live: Option<LiveStream> = None;
    let mut idle_since = Instant::now();

    loop {
        match rx.recv_timeout(IDLE_TICK) {
            Ok(CaptureCmd::Arm { reply }) => {
                // A stream whose device errored (unplugged, format changed) is
                // useless — drop it and re-query the hardware from scratch.
                if live.as_ref().is_some_and(LiveStream::has_failed) {
                    log::warn!("capture stream reported a device error — reopening cold");
                    live = None;
                    cache.invalidate();
                }
                if live.is_none() {
                    match open_stream(&mut cache) {
                        Ok(stream) => live = Some(stream),
                        Err(e) => {
                            let _ = reply.send(Err(e));
                            continue;
                        }
                    }
                }
                if let Some(stream) = live.as_ref() {
                    if stream.is_capturing() {
                        log::warn!("arm while already capturing — dropping the orphaned take");
                    }
                    stream.begin();
                    let _ = reply.send(Ok(()));
                }
            }
            Ok(CaptureCmd::Disarm { reply }) => {
                idle_since = Instant::now();
                let captured = match live.as_ref() {
                    Some(stream) => Ok(stream.end()),
                    None => Err(NO_SAMPLES_ERR.to_string()),
                };
                let _ = reply.send(captured);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let idle = live.as_ref().is_some_and(|s| !s.is_capturing());
                if idle && idle_since.elapsed() >= IDLE_CLOSE {
                    live = None;
                    log::info!(
                        "capture stream closed after {}s idle (mic indicator off)",
                        IDLE_CLOSE.as_secs()
                    );
                }
            }
            // Every sender is gone (process teardown) — release the stream.
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Open the input stream, reusing the cached device + config when the system
/// default is the same one we opened last time. Any failure invalidates the
/// cache so the next attempt re-queries the hardware.
fn open_stream(
    cache: &mut DeviceConfigCache<cpal::Device, cpal::SupportedStreamConfig>,
) -> Result<LiveStream, String> {
    let opened_at = Instant::now();
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| NO_MIC_ERR.to_string())?;
    let dev_name = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_default();

    let cached = cache.get(&dev_name);
    let was_cached = cached.is_some();
    let (device, supported) = match cached {
        Some((device, config)) => (device, config),
        None => {
            let config = device.default_input_config().map_err(|e| {
                cache.invalidate();
                format!("Mic config failed ({e}). Enable Microphone for Yap (not Python) in System Settings.")
            })?;
            (device, config)
        }
    };

    let sample_rate = supported.sample_rate();
    let channels = supported.channels();
    let sample_format = supported.sample_format();
    let conf: cpal::StreamConfig = supported.into();

    let dsp = Arc::new(Mutex::new(StreamDsp::new(sample_rate, channels)));
    let capturing = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));

    let stream = match sample_format {
        cpal::SampleFormat::F32 => {
            build_capture_stream::<f32>(&device, conf, &dsp, &capturing, &failed)
        }
        cpal::SampleFormat::I16 => {
            build_capture_stream::<i16>(&device, conf, &dsp, &capturing, &failed)
        }
        cpal::SampleFormat::U16 => {
            build_capture_stream::<u16>(&device, conf, &dsp, &capturing, &failed)
        }
        other => Err(format!(
            "Unsupported sample format {other:?}. Try a different input device."
        )),
    };
    let stream = match stream {
        Ok(stream) => stream,
        Err(e) => {
            cache.invalidate();
            return Err(e);
        }
    };
    if let Err(e) = stream.play() {
        cache.invalidate();
        return Err(format!("mic start failed ({e}). Allow Microphone for Yap."));
    }

    log::info!(
        "mic device={dev_name} format={sample_format:?} rate={sample_rate} ch={channels} cached_config={was_cached} open_ms={}",
        opened_at.elapsed().as_millis()
    );
    // The device accepted this config — remember it so the next reopen skips the
    // HAL property queries entirely.
    cache.store(dev_name, device, supported);

    Ok(LiveStream {
        _stream: stream,
        dsp,
        capturing,
        failed,
    })
}

/// Build the input stream for one sample format. The callback converts to f32,
/// feeds the HUD meter and pushes the frame through the streaming DSP (YV37:
/// downmix → high-pass → AGC accumulation → 16 kHz resample, all incremental)
/// — but ONLY while a take is armed, so the persistent stream costs nothing
/// (and retains nothing) between dictations. A stream error flips `failed`,
/// which makes the next arm reopen.
fn build_capture_stream<T>(
    device: &cpal::Device,
    conf: cpal::StreamConfig,
    dsp: &Arc<Mutex<StreamDsp>>,
    capturing: &Arc<AtomicBool>,
    failed: &Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let dsp = dsp.clone();
    let capturing = capturing.clone();
    let failed_cb = failed.clone();
    let level = capture_level().clone();
    let mut scratch: Vec<f32> = Vec::new();

    device
        .build_input_stream(
            conf,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                if !capturing.load(Ordering::SeqCst) {
                    return;
                }
                scratch.clear();
                scratch.extend(data.iter().map(|&s| s.to_sample::<f32>()));
                push_level(&level, &scratch);
                if let Ok(mut dsp) = dsp.lock() {
                    dsp.push(&scratch);
                }
            },
            move |e| {
                log::error!("cpal stream error: {e}");
                failed_cb.store(true, Ordering::SeqCst);
            },
            None,
        )
        .map_err(|e| format!("mic stream: {e}"))
}

// ── Streaming DSP (YV37) ────────────────────────────────────────────────────
// Everything that can run per-frame now does, INSIDE the capture path, so the
// release→ASR gap no longer pays for the whole clip's DSP: each callback's
// frames are downmixed, high-passed (biquad state carried across frames), fed
// to the AGC accumulator and resampled to 16 kHz incrementally. What is left at
// stop is only what genuinely needs the whole take: the AGC gain, the edge fade
// and the optional RNNoise pass.
//
// The raw mono capture is kept alongside the streamed buffer as the
// never-lose-audio fallback — if the streamed output is empty or ever went
// non-finite, `finalize_take` rebuilds the clip from the raw samples with the
// batch chain instead of losing the utterance.

/// One take's streaming DSP state + buffers. Lives behind the capture stream's
/// mutex and is written by the audio callback.
struct StreamDsp {
    sample_rate: u32,
    channels: u16,
    high_pass: Option<Biquad>,
    resampler: StreamResampler,
    /// AGC accumulation: running sum of squares / count / peak over the
    /// high-passed take, so the gain is known the instant the user releases.
    sum_sq: f64,
    counted: usize,
    peak: f32,
    /// Cleared the moment a non-finite sample is seen — AGC then leaves the
    /// level alone rather than acting on garbage stats.
    finite: bool,
    /// 16 kHz mono, high-passed — the ASR buffer, built during capture.
    out: Vec<f32>,
    /// Raw mono at the native rate — the fallback (see above).
    raw: Vec<f32>,
    /// Reused downmix scratch so a callback allocates nothing steady-state.
    mono: Vec<f32>,
}

impl StreamDsp {
    fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
            high_pass: Biquad::high_pass(sample_rate, HIGH_PASS_HZ),
            resampler: StreamResampler::new(sample_rate, TARGET_RATE),
            sum_sq: 0.0,
            counted: 0,
            peak: 0.0,
            finite: true,
            out: Vec::new(),
            raw: Vec::new(),
            mono: Vec::new(),
        }
    }

    /// Drop the previous take entirely (buffers AND filter/resampler state) so
    /// no audio — and no biquad tail — ever bleeds from one dictation into the
    /// next.
    fn reset(&mut self) {
        *self = Self::new(self.sample_rate, self.channels);
    }

    /// Push one callback's interleaved native-rate frames through the chain.
    fn push(&mut self, interleaved: &[f32]) {
        if interleaved.is_empty() {
            return;
        }
        self.mono.clear();
        let ch = self.channels.max(1) as usize;
        if ch <= 1 {
            self.mono.extend_from_slice(interleaved);
        } else {
            self.mono.extend(
                interleaved
                    .chunks(ch)
                    .map(|c| c.iter().sum::<f32>() / c.len() as f32),
            );
        }
        // Fallback copy first — kept exactly as captured.
        self.raw.extend_from_slice(&self.mono);

        // Signal hygiene (Tier 0, docs/research/voice-isolation.md) at the NATIVE
        // rate: kill sub-80 Hz rumble/hum before anything else looks at the frame.
        // A filter that goes non-finite leaves the frame unfiltered (and resets
        // its own state) rather than poisoning the buffer.
        if let Some(hp) = self.high_pass.as_mut() {
            hp.process(&mut self.mono);
        }
        for &s in self.mono.iter() {
            if !s.is_finite() {
                self.finite = false;
                continue;
            }
            self.sum_sq += (s as f64) * (s as f64);
            self.counted += 1;
            let a = s.abs();
            if a > self.peak {
                self.peak = a;
            }
        }
        // …and straight down to 16 kHz, frame by frame: the ASR buffer is
        // essentially ready the moment the key comes up.
        self.resampler.push(&self.mono, &mut self.out);
    }

    /// Close the take: flush the resampler's tail and hand the buffers over.
    fn take(&mut self) -> CapturedAudio {
        self.resampler.finish(&mut self.out);
        let rms = if self.finite && self.counted > 0 {
            (self.sum_sq / self.counted as f64).sqrt() as f32
        } else {
            0.0
        };
        let gain = if self.finite {
            agc_gain(rms, self.peak, NORMALIZE_TARGET_DBFS)
        } else {
            1.0
        };
        let captured = CapturedAudio {
            samples: std::mem::take(&mut self.out),
            raw: std::mem::take(&mut self.raw),
            sample_rate: self.sample_rate,
            gain,
        };
        self.reset();
        captured
    }
}

/// Finish a streamed take into the 16 kHz mono buffer the ASR engine consumes:
/// apply the accumulated AGC gain, de-click the PTT press/release edges, then
/// (optionally) denoise. Every stage returns its input unchanged on degenerate
/// audio, and an unusable streamed buffer falls back to the raw capture, so a
/// take is never lost to DSP.
fn finalize_take(captured: CapturedAudio, denoise: bool) -> Result<Vec<f32>, String> {
    let CapturedAudio {
        samples,
        raw,
        sample_rate,
        gain,
    } = captured;
    if samples.is_empty() && raw.is_empty() {
        return Err(NO_AUDIO_ERR.into());
    }
    let leveled = if samples.is_empty() || samples.iter().any(|s| !s.is_finite()) {
        log::warn!(
            "YV37 streamed DSP unusable ({} samples) — rebuilding the clip from the raw capture",
            samples.len()
        );
        fallback_chain(&raw, sample_rate)
    } else {
        apply_gain(&samples, gain)
    };
    let faded = edge_fade(&leveled, TARGET_RATE, EDGE_FADE_MS);
    // Denoise (Tier 1, docs/research/voice-isolation.md) — RNNoise, gated by the
    // user's `denoise` setting. It is the one stage that cannot stream cheaply
    // (48 kHz round-trip over the whole clip), and it falls back to its input on
    // any degeneracy so a bad clip never loses the utterance.
    let out = if denoise {
        denoise_rnnoise(&faded, TARGET_RATE)
    } else {
        faded
    };
    if out.is_empty() {
        return Err(NO_AUDIO_ERR.into());
    }
    Ok(out)
}

/// Never-lose-audio path: the pre-YV37 batch chain over the untouched raw mono
/// capture, used only when the streamed buffer came back unusable.
fn fallback_chain(raw: &[f32], sample_rate: u32) -> Vec<f32> {
    let hp = high_pass(raw, sample_rate, HIGH_PASS_HZ);
    let leveled = normalize_rms(&hp, NORMALIZE_TARGET_DBFS);
    if sample_rate == TARGET_RATE {
        leveled
    } else {
        resample_linear(&leveled, sample_rate, TARGET_RATE)
    }
}

/// Scale a buffer, hard-limited to `[-1, 1]` as a safety net.
fn apply_gain(samples: &[f32], gain: f32) -> Vec<f32> {
    samples
        .iter()
        .map(|&s| (s * gain).clamp(-1.0, 1.0))
        .collect()
}

/// Linear resampler that can be fed in arbitrary frames and produces EXACTLY
/// what `resample_linear` would over the concatenated input (asserted in the
/// tests) — the capture path streams through this, the fallback path still runs
/// the batch function.
///
/// It emits an output sample as soon as both of its input neighbours have
/// arrived, keeping only the couple of input samples the next output still needs
/// (so a long hold costs nothing extra), and `finish` flushes the tail with the
/// same clamped last-neighbour rule the batch version uses.
struct StreamResampler {
    /// Input samples per output sample (`from / to`).
    ratio: f64,
    /// Retained input, `pending[0]` being global input index `base`.
    pending: Vec<f32>,
    base: u64,
    /// Index of the next output sample to emit.
    next_out: u64,
    /// Total input samples seen so far.
    seen: u64,
}

impl StreamResampler {
    fn new(from: u32, to: u32) -> Self {
        let ratio = if from == 0 || to == 0 {
            1.0
        } else {
            from as f64 / to as f64
        };
        Self {
            ratio,
            pending: Vec::new(),
            base: 0,
            next_out: 0,
            seen: 0,
        }
    }

    /// Feed one frame, appending every output sample it completes.
    fn push(&mut self, frame: &[f32], out: &mut Vec<f32>) {
        if frame.is_empty() {
            return;
        }
        self.pending.extend_from_slice(frame);
        self.seen += frame.len() as u64;
        // Only emit outputs the batch version would also emit for the input seen
        // so far — the trailing partial sample is dropped there, and `finish`
        // decides it here.
        let ready = (self.seen as f64 / self.ratio).floor() as u64;
        while self.next_out < ready {
            let src = self.next_out as f64 * self.ratio;
            let i0 = src.floor() as u64;
            // Needs its right-hand neighbour; wait for the next frame otherwise.
            if i0 + 1 >= self.seen {
                break;
            }
            let k = (i0 - self.base) as usize;
            let t = (src - i0 as f64) as f32;
            out.push(self.pending[k] * (1.0 - t) + self.pending[k + 1] * t);
            self.next_out += 1;
        }
        // Release everything before the next output's left neighbour.
        let next_src = self.next_out as f64 * self.ratio;
        let keep_from = next_src.floor() as u64;
        let drop = keep_from.saturating_sub(self.base) as usize;
        let drop = drop.min(self.pending.len());
        if drop > 0 {
            self.pending.drain(..drop);
            self.base += drop as u64;
        }
    }

    /// Flush the tail (the last output sample clamps to the final input sample,
    /// exactly like `resample_linear`).
    fn finish(&mut self, out: &mut Vec<f32>) {
        let total = (self.seen as f64 / self.ratio).floor() as u64;
        while self.next_out < total {
            let src = self.next_out as f64 * self.ratio;
            let i0 = src.floor() as u64;
            let k = (i0 - self.base) as usize;
            if k >= self.pending.len() {
                break;
            }
            let k1 = (k + 1).min(self.pending.len() - 1);
            let t = (src - i0 as f64) as f32;
            out.push(self.pending[k] * (1.0 - t) + self.pending[k1] * t);
            self.next_out += 1;
        }
    }
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

/// Second-order (biquad) Butterworth high-pass, Direct Form I. The state lives
/// in the struct so the SAME filter can run over a whole clip at once (the
/// fallback chain) or across the capture frames as they arrive (YV37) with
/// identical output.
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    /// High-pass at `cutoff_hz`; `None` for a degenerate rate/cutoff, which
    /// callers treat as "leave the audio alone".
    fn high_pass(sample_rate: u32, cutoff_hz: f32) -> Option<Self> {
        if sample_rate == 0 || cutoff_hz <= 0.0 || cutoff_hz >= sample_rate as f32 / 2.0 {
            return None;
        }
        // RBJ cookbook high-pass coefficients (Q = 1/√2 → maximally flat passband).
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate as f32;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let q = std::f32::consts::FRAC_1_SQRT_2;
        let alpha = sin_w0 / (2.0 * q);
        let a0 = 1.0 + alpha;
        if a0 == 0.0 || !a0.is_finite() {
            return None;
        }
        Some(Self {
            b0: ((1.0 + cos_w0) / 2.0) / a0,
            b1: (-(1.0 + cos_w0)) / a0,
            b2: ((1.0 + cos_w0) / 2.0) / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        })
    }

    /// Filter a frame in place. Returns false if the biquad went non-finite: it
    /// then stops at that sample (the rest of the frame passes through
    /// unfiltered) and clears its state, so one bad sample can never poison the
    /// rest of the take — the batch caller falls back to the input outright.
    fn process(&mut self, frame: &mut [f32]) -> bool {
        for slot in frame.iter_mut() {
            let x0 = *slot;
            let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1
                - self.a2 * self.y2;
            if !y0.is_finite() {
                self.x1 = 0.0;
                self.x2 = 0.0;
                self.y1 = 0.0;
                self.y2 = 0.0;
                return false;
            }
            self.x2 = self.x1;
            self.x1 = x0;
            self.y2 = self.y1;
            self.y1 = y0;
            *slot = y0;
        }
        true
    }
}

/// Batch high-pass over a whole buffer (the fallback chain + the DSP tests).
/// Returns the input unchanged on degenerate input or if the filter ever goes
/// non-finite.
fn high_pass(samples: &[f32], sample_rate: u32, cutoff_hz: f32) -> Vec<f32> {
    let Some(mut filter) = Biquad::high_pass(sample_rate, cutoff_hz) else {
        return samples.to_vec();
    };
    let mut out = samples.to_vec();
    if !filter.process(&mut out) {
        return samples.to_vec();
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
    let gain = agc_gain(rms, peak, target_dbfs);
    if gain == 1.0 {
        return samples.to_vec();
    }
    let out = apply_gain(samples, gain);
    if out.iter().any(|v| !v.is_finite()) {
        return samples.to_vec();
    }
    out
}

/// The soft-AGC gain for a take, from its level stats. Shared so the streaming
/// path (stats accumulated frame by frame during capture, YV37) and the batch
/// fallback lift a quiet voice by exactly the same amount. `1.0` means "leave
/// the level alone" — silence, or anything degenerate.
fn agc_gain(rms: f32, peak: f32, target_dbfs: f32) -> f32 {
    // Silence / near-silence guard (~-80 dBFS): nothing to lift, avoid blowup.
    if !rms.is_finite() || rms < 1e-4 || !peak.is_finite() || peak <= 0.0 {
        return 1.0;
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
        return 1.0;
    }
    gain
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

fn write_wav_i16(path: &Path, sample_rate: u32, samples: &[f32]) -> Result<(), String> {
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

    // ── Persistent capture worker (YV35) ────────────────────────────────────
    // The cpal stream itself needs hardware, but the two pieces of logic that
    // decide how fast (and how safely) a keypress reaches capture are pure: the
    // device/config cache and the arm/stop signal wait that replaced the fixed
    // start poll. Both are exercised here with stand-in device/config types.

    /// Stand-ins for `cpal::Device` / `cpal::SupportedStreamConfig`.
    type TestCache = DeviceConfigCache<&'static str, u32>;

    #[test]
    fn config_cache_hits_only_the_same_named_device() {
        let mut cache: TestCache = DeviceConfigCache::new();
        assert!(!cache.is_cached());
        assert_eq!(
            cache.get("MacBook Pro Microphone"),
            None,
            "cold cache misses"
        );

        cache.store("MacBook Pro Microphone".into(), "builtin", 48_000);
        assert!(cache.is_cached());
        assert_eq!(
            cache.get("MacBook Pro Microphone"),
            Some(("builtin", 48_000)),
            "same device reuses the cached config (no HAL query on reopen)"
        );
        // A different default input device must NOT reuse another device's
        // rate/format — that would open the stream misconfigured.
        assert_eq!(cache.get("AirPods Pro"), None);
        // An unidentifiable device (cpal returns an empty name) never hits.
        assert_eq!(cache.get(""), None);
    }

    #[test]
    fn config_cache_invalidates_on_device_error_and_restores() {
        let mut cache: TestCache = DeviceConfigCache::new();
        cache.store("AirPods Pro".into(), "bt", 24_000);
        assert!(cache.get("AirPods Pro").is_some());

        // Stream/open error (device unplugged, rate changed) → drop everything
        // so the next arm re-queries the hardware.
        cache.invalidate();
        assert!(!cache.is_cached());
        assert_eq!(cache.get("AirPods Pro"), None);

        // …and the next successful open re-populates it.
        cache.store("AirPods Pro".into(), "bt", 16_000);
        assert_eq!(cache.get("AirPods Pro"), Some(("bt", 16_000)));

        // Re-storing under a NEW device replaces the entry (one device cached).
        cache.store("MacBook Pro Microphone".into(), "builtin", 48_000);
        assert_eq!(cache.get("AirPods Pro"), None);
        assert_eq!(
            cache.get("MacBook Pro Microphone"),
            Some(("builtin", 48_000))
        );
    }

    #[test]
    fn config_cache_never_stores_an_empty_device_name() {
        let mut cache: TestCache = DeviceConfigCache::new();
        cache.store("MacBook Pro Microphone".into(), "builtin", 48_000);
        // An unnamed device must clear the cache rather than pin an unknown
        // device under "" (which would then hit for every unnamed device).
        cache.store(String::new(), "mystery", 8_000);
        assert!(!cache.is_cached());
        assert_eq!(cache.get(""), None);
    }

    #[test]
    fn arm_signal_returns_as_soon_as_the_worker_answers() {
        let (tx, rx) = mpsc::sync_channel::<Result<u8, String>>(1);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            let _ = tx.send(Ok(7));
        });
        let started = Instant::now();
        assert_eq!(await_reply(&rx, Duration::from_secs(3), "arm"), Ok(7));
        // Signalled, not polled: nowhere near the old fixed start-poll wait.
        assert!(
            started.elapsed() < Duration::from_millis(300),
            "arm took {:?} — it should return on the worker's signal",
            started.elapsed()
        );
    }

    #[test]
    fn arm_signal_surfaces_worker_errors_verbatim() {
        let (tx, rx) = mpsc::sync_channel::<Result<u8, String>>(1);
        tx.send(Err(NO_MIC_ERR.to_string())).unwrap();
        assert_eq!(
            await_reply(&rx, Duration::from_secs(3), "arm"),
            Err(NO_MIC_ERR.to_string()),
            "a mic/permission failure must reach the user unchanged"
        );
    }

    #[test]
    fn arm_signal_is_bounded_and_reports_a_dead_worker() {
        // Nothing ever answers → bounded timeout, not a hung hotkey.
        let (keep_alive, rx) = mpsc::sync_channel::<Result<u8, String>>(1);
        let started = Instant::now();
        let timed_out = await_reply(&rx, Duration::from_millis(60), "arm").unwrap_err();
        assert!(timed_out.contains("timed out"), "got {timed_out}");
        assert!(started.elapsed() >= Duration::from_millis(60));
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(keep_alive);

        // Worker thread died before answering → distinct, actionable error.
        let (tx, rx) = mpsc::sync_channel::<Result<u8, String>>(1);
        drop(tx);
        let dead = await_reply(&rx, Duration::from_secs(3), "stop").unwrap_err();
        assert!(dead.contains("capture worker stopped"), "got {dead}");
    }

    #[test]
    fn stale_wav_predicate_matches_only_wavs() {
        assert!(is_stale_wav("f47ac10b-58cc-4372-a567-0e02b2c3d479.wav"));
        assert!(is_stale_wav("CLIP.WAV"));
        assert!(is_stale_wav("a.wav"));
        // Non-wavs and the bare extension must be left alone.
        assert!(!is_stale_wav(".wav"));
        assert!(!is_stale_wav("wilson_voice.db"));
        assert!(!is_stale_wav("notes.txt"));
        assert!(!is_stale_wav("recording.wav.tmp"));
        assert!(!is_stale_wav(""));
    }

    #[test]
    fn sweep_removes_wavs_only() {
        let dir =
            std::env::temp_dir().join(format!("yv20-sweep-{}-{:p}", std::process::id(), &0u8));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.wav"), b"x").unwrap();
        std::fs::write(dir.join("b.WAV"), b"x").unwrap();
        std::fs::write(dir.join("keep.db"), b"x").unwrap();
        let removed = sweep_stale_wavs(&dir);
        assert_eq!(removed, 2, "both wavs removed");
        assert!(dir.join("keep.db").exists(), "non-wav preserved");
        assert!(!dir.join("a.wav").exists());
        // Missing dir is a safe no-op.
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(sweep_stale_wavs(&dir), 0);
    }

    /// The committed E2E fixture (`--transcribe-file` gate): a `say`-generated
    /// clip of "The quick brown fox jumps over the lazy dog." — see
    /// tests/fixtures/README.md.
    #[test]
    fn fixture_wav_loads_as_16k_mono() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("quick-brown-fox-16k.wav");
        let samples = read_wav_16k_mono(&fixture).expect("fixture reads");
        let seconds = samples.len() as f64 / SR as f64;
        assert!(
            (1.0..10.0).contains(&seconds),
            "fixture should be a short phrase, got {seconds:.2}s"
        );
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        assert!(
            voiced_seconds(&samples, SR) > 0.5,
            "fixture must contain speech"
        );
    }

    #[test]
    fn wav_loader_downsamples_and_rejects_empty() {
        let dir = std::env::temp_dir().join(format!("yv32-wav-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // 1 s at 32 kHz (32k samples) → 16k samples at the 16 kHz target rate.
        let path = dir.join("tone32k.wav");
        write_wav_i16(&path, 32_000, &tone(2.0, 220.0, 0.4)).unwrap();
        let out = read_wav_16k_mono(&path).expect("32 kHz wav reads");
        let expected = SR as usize;
        assert!(
            out.len().abs_diff(expected) <= 2,
            "expected ~{expected} samples at 16 kHz, got {}",
            out.len()
        );
        // A zero-sample wav is an error, never a silent empty transcription.
        let empty = dir.join("empty.wav");
        write_wav_i16(&empty, SR, &[]).unwrap();
        assert!(read_wav_16k_mono(&empty).is_err());
        let _ = std::fs::remove_dir_all(&dir);
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

    // ── Streaming DSP (YV37) ────────────────────────────────────────────────
    // The capture path now filters + resamples frame by frame instead of running
    // the whole chain after key-release. The bar these tests hold it to: framing
    // must not change the audio at all — streaming an arbitrarily chopped-up
    // take must produce EXACTLY what the batch chain produced over the whole
    // buffer — and an unusable streamed buffer must still yield the utterance.

    /// Split `input` into frames of the given (repeating) sizes — a stand-in for
    /// the ragged buffer sizes cpal actually hands the callback.
    fn frames<'a>(input: &'a [f32], sizes: &[usize]) -> Vec<&'a [f32]> {
        let mut out = Vec::new();
        let mut rest = input;
        let mut i = 0;
        while !rest.is_empty() {
            let n = sizes[i % sizes.len()].min(rest.len()).max(1);
            let (head, tail) = rest.split_at(n);
            out.push(head);
            rest = tail;
            i += 1;
        }
        out
    }

    #[test]
    fn stream_resampler_matches_batch_for_every_framing() {
        // 48/44.1 kHz mics downsampling to 16 kHz, the already-16 kHz identity
        // case, and an upsample for good measure — each fed in even, ragged and
        // single-sample frames.
        for (from, to) in [
            (48_000u32, 16_000u32),
            (44_100, 16_000),
            (16_000, 16_000),
            (8_000, 16_000),
        ] {
            let input = tone_at(from, 0.05, 220.0, 0.5);
            let batch = resample_linear(&input, from, to);
            for sizes in [vec![512], vec![480, 137, 1, 999], vec![1], vec![7, 3]] {
                let mut stream = StreamResampler::new(from, to);
                let mut out = Vec::new();
                for frame in frames(&input, &sizes) {
                    stream.push(frame, &mut out);
                }
                stream.finish(&mut out);
                assert_eq!(
                    out.len(),
                    batch.len(),
                    "{from}→{to} in {sizes:?}-sized frames must yield the batch length"
                );
                assert!(
                    out.iter().zip(&batch).all(|(a, b)| a == b),
                    "{from}→{to} in {sizes:?}-sized frames must be sample-identical to the batch resample"
                );
            }
        }
    }

    #[test]
    fn stream_resampler_emits_during_capture_and_holds_no_backlog() {
        // The point of YV37: 16 kHz samples exist WHILE the user speaks, not
        // after release — and the retained input never grows with hold length.
        let input = tone_at(NATIVE_SR, 0.5, 220.0, 0.4);
        let mut stream = StreamResampler::new(NATIVE_SR, SR);
        let mut out = Vec::new();
        let mut after_first = 0usize;
        for (i, frame) in frames(&input, &[512]).into_iter().enumerate() {
            stream.push(frame, &mut out);
            if i == 0 {
                after_first = out.len();
            }
            assert!(
                stream.pending.len() <= 512 + 4,
                "retained input must stay bounded, got {}",
                stream.pending.len()
            );
        }
        assert!(
            after_first > 100,
            "the first 512-frame callback should already produce ~170 samples at 16 kHz, got {after_first}"
        );
        stream.finish(&mut out);
        let expected = input.len() / 3; // 48 kHz → 16 kHz
        assert!(
            out.len().abs_diff(expected) <= 2,
            "expected ~{expected} samples, got {}",
            out.len()
        );
    }

    #[test]
    fn streaming_high_pass_matches_the_batch_filter() {
        // The biquad carries its state across callbacks — otherwise every frame
        // boundary would restart the filter and click.
        let input = tone_at(NATIVE_SR, 0.2, 25.0, 0.5);
        let batch = high_pass(&input, NATIVE_SR, HIGH_PASS_HZ);
        let mut filter = Biquad::high_pass(NATIVE_SR, HIGH_PASS_HZ).expect("valid cutoff");
        let mut streamed = Vec::new();
        for frame in frames(&input, &[441, 64, 1, 2048]) {
            let mut chunk = frame.to_vec();
            assert!(filter.process(&mut chunk), "clean audio must filter");
            streamed.extend_from_slice(&chunk);
        }
        assert_eq!(streamed.len(), batch.len());
        assert!(
            streamed.iter().zip(&batch).all(|(a, b)| a == b),
            "per-frame filtering must equal filtering the whole take at once"
        );
        // A degenerate rate/cutoff has no filter at all (callers pass audio through).
        assert!(Biquad::high_pass(0, HIGH_PASS_HZ).is_none());
        assert!(Biquad::high_pass(NATIVE_SR, 0.0).is_none());
        assert!(Biquad::high_pass(NATIVE_SR, NATIVE_SR as f32).is_none());
    }

    #[test]
    fn stream_dsp_downmixes_and_resamples_frames_as_they_arrive() {
        // Stereo 48 kHz in (what a MacBook mic hands the callback) → mono 16 kHz
        // out, produced incrementally, with the raw mono fallback kept alongside.
        let mono_in = tone_at(NATIVE_SR, 0.3, 200.0, 0.02);
        let interleaved: Vec<f32> = mono_in
            .iter()
            .flat_map(|&s| [s * 0.5, s * 1.5]) // channel average == the mono input
            .collect();
        let mut dsp = StreamDsp::new(NATIVE_SR, 2);
        let mut mid_capture = 0usize;
        for (i, frame) in frames(&interleaved, &[1024, 512, 130])
            .into_iter()
            .enumerate()
        {
            dsp.push(frame);
            if i == 1 {
                mid_capture = dsp.out.len();
            }
        }
        assert!(
            mid_capture > 0,
            "16 kHz samples must exist mid-hold, not only after release"
        );
        let captured = dsp.take();
        assert_eq!(captured.sample_rate, NATIVE_SR);
        assert_eq!(
            captured.raw.len(),
            mono_in.len(),
            "the raw fallback keeps every captured mono sample"
        );
        assert!(
            captured
                .raw
                .iter()
                .zip(&mono_in)
                .all(|(a, b)| (a - b).abs() < 1e-6),
            "downmix must average the channels"
        );
        let expected = mono_in.len() / 3;
        assert!(
            captured.samples.len().abs_diff(expected) <= 2,
            "expected ~{expected} samples at 16 kHz, got {}",
            captured.samples.len()
        );
        assert!(captured.samples.iter().all(|s| s.is_finite()));
        // A quiet take is lifted, never attenuated to nothing.
        assert!(captured.gain > 1.0, "quiet take should get AGC gain");
        // …and the state is gone with the take: the next dictation starts clean.
        assert!(dsp.out.is_empty() && dsp.raw.is_empty());
    }

    #[test]
    fn finalize_levels_fades_and_never_loses_the_take() {
        let quiet = tone_at(SR, 0.5, 220.0, 0.02);
        let gain = agc_gain(rms(&quiet), max_abs(&quiet), NORMALIZE_TARGET_DBFS);
        let captured = CapturedAudio {
            samples: quiet.clone(),
            raw: Vec::new(),
            sample_rate: SR,
            gain,
        };
        let out = finalize_take(captured, false).expect("a normal take finalizes");
        assert_eq!(
            out.len(),
            quiet.len(),
            "finalize must not change the length"
        );
        assert!(out[0].abs() < 1e-6, "edges are de-clicked");
        assert!(
            (to_dbfs(rms(&out)) - NORMALIZE_TARGET_DBFS).abs() < 3.0,
            "AGC gain accumulated during capture must land near the target, got {:.1} dBFS",
            to_dbfs(rms(&out))
        );
        assert!(max_abs(&out) <= 1.0 && out.iter().all(|s| s.is_finite()));

        // Never lose audio: a streamed buffer that went non-finite falls back to
        // the raw capture (native rate, unfiltered) instead of losing the take.
        let raw = tone_at(NATIVE_SR, 0.5, 220.0, 0.3);
        let mut broken = tone_at(SR, 0.5, 220.0, 0.3);
        broken[100] = f32::NAN;
        let out = finalize_take(
            CapturedAudio {
                samples: broken,
                raw: raw.clone(),
                sample_rate: NATIVE_SR,
                gain: 1.0,
            },
            false,
        )
        .expect("the fallback still produces the utterance");
        let expected = raw.len() / 3;
        assert!(
            out.len().abs_diff(expected) <= 2,
            "fallback must resample the raw capture to 16 kHz, got {}",
            out.len()
        );
        assert!(out.iter().all(|s| s.is_finite()) && rms(&out) > 0.0);

        // An empty streamed buffer takes the same fallback…
        let out = finalize_take(
            CapturedAudio {
                samples: Vec::new(),
                raw,
                sample_rate: NATIVE_SR,
                gain: 1.0,
            },
            false,
        )
        .expect("empty streamed buffer falls back to raw");
        assert!(!out.is_empty());
        // …and a take with NO audio at all is an actionable error, never silence.
        let err = finalize_take(
            CapturedAudio {
                samples: Vec::new(),
                raw: Vec::new(),
                sample_rate: NATIVE_SR,
                gain: 1.0,
            },
            false,
        )
        .unwrap_err();
        assert!(err.contains("Microphone"), "got {err}");
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
