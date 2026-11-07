//! yap-1106-sp-05 — streaming decode DURING capture.
//!
//! A dictation take used to decode only after the key came up, so a four
//! minute take sat on "Transcribing…" for about three seconds. This module
//! decodes the take while it is still being spoken: the capture consumer feeds
//! every 16 kHz sample in ([`feed`]), and the take is cut into 20–30 s windows
//! at the speaker's own pauses (a simple energy VAD over 30 ms frames). Each
//! committed window is decoded on a background thread on the dictation's own
//! pinned engine (which, since yap-1106-engine-owner, runs inside the engine
//! helper process), so on release only the uncommitted TAIL — never more than
//! one window — is left to decode.
//!
//! Safety rules, in order of importance:
//!
//! * **No loss.** Every sample fed is either in a committed window or in the
//!   tail, and [`StreamDecoder::seal`] proves the stream saw exactly the
//!   samples the take captured. Anything short of that — a failed window, a
//!   backlog over [`StreamConfig::max_backlog_windows`], a constrained
//!   resource governor, a capture count that does not match — is
//!   [`StreamOutcome::Degraded`], and the caller decodes the WHOLE take the
//!   way it always has (SP-01 windowing for long takes).
//! * **Short takes are untouched.** A take that never filled one window is
//!   [`StreamOutcome::NotUsed`]: it goes down the unchanged batch path, byte
//!   for byte.
//! * **Partials are honest.** `transcript_partial {take, committed, tentative}`
//!   carries only text a window really decoded, at most once every
//!   [`PARTIAL_MIN_INTERVAL`]. Window cutting has no tentative hypothesis, so
//!   `tentative` is always empty here; the field exists so the pill item
//!   (yap-1106-partial-transcript-stream) renders one shape.
//!
//! Telemetry: one `stream_decode window …` line per decoded window and one
//! `stream_decode finish …` line per take, carrying the decode time spent
//! during capture and its duty cycle (`duty_pct`, decode wall time over capture
//! wall time) — the CPU+GPU overhead proxy the energy bench checks against.
//! Never transcript text, never audio.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;

/// The Tauri event the committed text rides on.
pub const TRANSCRIPT_PARTIAL_EVENT: &str = "transcript_partial";

/// The stream works at the dictation rate, like everything above capture.
pub const STREAM_SAMPLE_RATE: u32 = 16_000;

/// One VAD frame: 30 ms.
pub const FRAME_SECONDS: f64 = 0.030;

/// The fastest the pill is told about new committed text.
pub const PARTIAL_MIN_INTERVAL: Duration = Duration::from_millis(500);

/// A frame quieter than this (RMS, after the take's running gain) is a pause
/// whatever the speaker's level.
pub const QUIET_RMS_FLOOR: f32 = 0.004;

/// …and so is a frame under this fraction of the window's mean RMS, so a loud
/// room still has pauses.
pub const QUIET_FRACTION_OF_MEAN: f32 = 0.15;

/// A tail shorter than this that is also silent is not worth a decode (the
/// model only hallucinates on it).
const TAIL_SKIP_SECONDS: f64 = 0.25;

pub const REASON_BACKLOG: &str = "backlog";
pub const REASON_GOVERNOR: &str = "governor";
pub const REASON_WINDOW_FAILED: &str = "window_failed";
pub const REASON_TAIL_FAILED: &str = "tail_failed";
pub const REASON_CAPTURE_MISMATCH: &str = "capture_mismatch";
pub const REASON_UNSEALED: &str = "unsealed";
pub const REASON_WORKER_GONE: &str = "worker_gone";
pub const REASON_INCOMPLETE: &str = "incomplete";
pub const REASON_DROPPED: &str = "dropped";
pub const REASON_SHORT: &str = "short_take";

/// The stream's geometry. A struct so tests can shrink the windows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamConfig {
    /// A window is never committed shorter than this.
    pub min_window_seconds: f64,
    /// …and is cut at the quietest frame if no pause came by this length.
    pub max_window_seconds: f64,
    /// How long a run of quiet frames must be to count as a pause.
    pub pause_seconds: f64,
    /// Windows committed but not yet decoded (in flight included) above which
    /// the stream gives up and the take decodes in batch.
    pub max_backlog_windows: usize,
    pub partial_interval: Duration,
    pub sample_rate: u32,
}

impl Default for StreamConfig {
    fn default() -> Self {
        StreamConfig {
            min_window_seconds: 20.0,
            max_window_seconds: 30.0,
            pause_seconds: 0.3,
            max_backlog_windows: 2,
            partial_interval: PARTIAL_MIN_INTERVAL,
            sample_rate: STREAM_SAMPLE_RATE,
        }
    }
}

impl StreamConfig {
    pub fn frame_samples(&self) -> usize {
        ((self.sample_rate.max(1) as f64) * FRAME_SECONDS)
            .round()
            .max(1.0) as usize
    }

    pub fn frames_for(&self, seconds: f64) -> usize {
        let per_second = self.sample_rate.max(1) as f64 / self.frame_samples() as f64;
        (seconds * per_second).round().max(1.0) as usize
    }

    /// The longest tail a release can ever be left with, in samples.
    pub fn max_window_samples(&self) -> usize {
        let max = self
            .frames_for(self.max_window_seconds)
            .max(self.frames_for(self.min_window_seconds) + 1);
        max * self.frame_samples()
    }
}

/// Where to cut the uncommitted audio, as a FRAME index, or `None` to keep
/// listening. Pure: the whole VAD decision is this function.
///
/// * Below `min_window_seconds` of frames: never.
/// * The first run of `pause_seconds` of quiet frames whose midpoint lies at
///   or past the minimum, within the maximum: cut in the middle of the pause.
/// * No pause by `max_window_seconds`: cut at the quietest frame in
///   `[min, max)`, so a window never outgrows the maximum.
pub fn find_cut(frame_rms: &[f32], cfg: &StreamConfig) -> Option<usize> {
    let min = cfg.frames_for(cfg.min_window_seconds);
    let max = cfg.frames_for(cfg.max_window_seconds).max(min + 1);
    if frame_rms.len() < min {
        return None;
    }
    let hi = frame_rms.len().min(max);
    let scan = &frame_rms[..hi];
    let mean = scan.iter().map(|r| f64::from(*r)).sum::<f64>() / scan.len().max(1) as f64;
    let quiet = QUIET_RMS_FLOOR.max(mean as f32 * QUIET_FRACTION_OF_MEAN);
    let need = cfg.frames_for(cfg.pause_seconds);
    let mut run_start: Option<usize> = None;
    for (i, rms) in scan.iter().enumerate().skip(min.saturating_sub(need)) {
        if *rms < quiet {
            let start = *run_start.get_or_insert(i);
            let run = i + 1 - start;
            if run >= need {
                let cut = start + run / 2;
                if cut >= min {
                    return Some(cut);
                }
            }
        } else {
            run_start = None;
        }
    }
    if frame_rms.len() >= max {
        let mut best = min;
        let mut best_rms = f32::INFINITY;
        for (i, rms) in scan.iter().enumerate().take(hi).skip(min) {
            if *rms < best_rms {
                best_rms = *rms;
                best = i;
            }
        }
        return Some(best);
    }
    None
}

/// Rate limit for `transcript_partial`: at most one emit per interval.
#[derive(Debug, Clone, Copy, Default)]
pub struct PartialThrottle {
    last: Option<Instant>,
}

impl PartialThrottle {
    /// True (and the clock restarts) when an emit is allowed at `now`.
    pub fn admit(&mut self, now: Instant, interval: Duration) -> bool {
        match self.last {
            Some(at) if now.saturating_duration_since(at) < interval => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

/// The payload of [`TRANSCRIPT_PARTIAL_EVENT`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptPartial {
    pub take: u64,
    pub committed: String,
    pub tentative: String,
}

/// Decodes one window. In the app this is the dictation's pinned engine; in
/// tests a stub.
pub trait WindowDecoder: Send + Sync {
    fn decode(&self, samples: Vec<f32>) -> Result<String, String>;
    /// True while the resource governor wants background decode to stand down.
    fn constrained(&self) -> bool {
        false
    }
}

/// Who hears about committed text.
pub type PartialSink = Arc<dyn Fn(&TranscriptPartial) + Send + Sync>;

/// What a finished stream produced.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamedTake {
    pub text: String,
    /// Committed windows plus the tail.
    pub windows: usize,
    pub tail_seconds: f64,
    /// The tail's decode alone.
    pub tail_ms: u64,
    /// Release → text: waiting for an in-flight window plus the tail.
    pub release_ms: u64,
    /// Decode time spent while the take was still recording.
    pub capture_decode_ms: u64,
    pub committed_audio_seconds: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamOutcome {
    /// Use this text; only the tail was decoded at release.
    Streamed(StreamedTake),
    /// The take never filled a window — decode it the unchanged batch way.
    NotUsed(&'static str),
    /// Streaming fell behind or failed — decode the WHOLE take in batch.
    Degraded(String),
}

struct Job {
    index: usize,
    samples: Vec<f32>,
}

#[derive(Default)]
struct Progress {
    committed: Vec<String>,
    degraded: Option<String>,
    windows_done: usize,
    decode_ms: u64,
    audio_seconds: f64,
    throttle: PartialThrottle,
}

struct Shared {
    progress: Mutex<Progress>,
    /// Committed windows not yet finished (in flight included).
    queued: AtomicUsize,
    /// Cheap mirror of `progress.degraded.is_some()` for the capture thread.
    degraded: AtomicBool,
}

impl Shared {
    fn degrade(&self, take: u64, reason: &str) {
        let first = {
            let mut p = lock(&self.progress);
            if p.degraded.is_none() {
                p.degraded = Some(reason.to_string());
                true
            } else {
                false
            }
        };
        self.degraded.store(true, Ordering::Release);
        if first {
            log::warn!("stream_decode degraded take={take} reason={reason} fallback=batch");
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn join_texts(parts: &[String]) -> String {
    parts
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sq: f64 = samples.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (sq / samples.len() as f64).sqrt() as f32
}

/// How much quiet is kept after the last voiced frame when a tail is trimmed.
const TAIL_PAD_SECONDS: f64 = 0.2;

/// Drop the trailing quiet frames of a release tail (keeping a short pad):
/// the batch path trims a take's trailing silence before decoding, and a model
/// fed seconds of nothing is where "Thank you." comes from. Returns how many
/// samples were dropped (only ever silence below [`QUIET_RMS_FLOOR`]).
pub fn trim_trailing_quiet(samples: &mut Vec<f32>, cfg: &StreamConfig) -> usize {
    let frame = cfg.frame_samples();
    let mut end = samples.len();
    while end > 0 {
        let start = end.saturating_sub(frame);
        if rms(&samples[start..end]) >= QUIET_RMS_FLOOR {
            break;
        }
        end = start;
    }
    let pad = (TAIL_PAD_SECONDS * cfg.sample_rate.max(1) as f64) as usize;
    let keep = (end + pad).min(samples.len());
    let dropped = samples.len() - keep;
    samples.truncate(keep);
    dropped
}

/// One take's streaming decode.
pub struct StreamDecoder {
    take: u64,
    cfg: StreamConfig,
    frame_samples: usize,
    /// Uncommitted samples (gain applied).
    pending: Vec<f32>,
    /// RMS of each complete frame of `pending`.
    frames: Vec<f32>,
    acc_sq: f64,
    acc_n: usize,
    fed: usize,
    committed_samples: usize,
    windows_sent: usize,
    sealed: Option<usize>,
    tx: Option<mpsc::Sender<Job>>,
    worker: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
    decoder: Arc<dyn WindowDecoder>,
    sink: Option<PartialSink>,
    started: Instant,
}

impl StreamDecoder {
    /// Start a take's stream and its decode thread.
    pub fn start(
        take: u64,
        cfg: StreamConfig,
        decoder: Arc<dyn WindowDecoder>,
        sink: Option<PartialSink>,
    ) -> StreamDecoder {
        let shared = Arc::new(Shared {
            progress: Mutex::new(Progress::default()),
            queued: AtomicUsize::new(0),
            degraded: AtomicBool::new(false),
        });
        let (tx, rx) = mpsc::channel::<Job>();
        let worker = {
            let shared = shared.clone();
            let decoder = decoder.clone();
            let sink = sink.clone();
            std::thread::Builder::new()
                .name("yap-stream-decode".into())
                .spawn(move || worker_loop(rx, shared, decoder, sink, take, cfg))
        };
        let (tx, worker) = match worker {
            Ok(handle) => (Some(tx), Some(handle)),
            Err(e) => {
                log::warn!("stream_decode spawn_failed take={take} error={e}");
                shared.degrade(take, REASON_WORKER_GONE);
                (None, None)
            }
        };
        StreamDecoder {
            take,
            cfg,
            frame_samples: cfg.frame_samples(),
            pending: Vec::new(),
            frames: Vec::new(),
            acc_sq: 0.0,
            acc_n: 0,
            fed: 0,
            committed_samples: 0,
            windows_sent: 0,
            sealed: None,
            tx,
            worker,
            shared,
            decoder,
            sink,
            started: Instant::now(),
        }
    }

    pub fn take_id(&self) -> u64 {
        self.take
    }

    /// Samples fed so far (the seal compares this with the capture's count).
    pub fn fed(&self) -> usize {
        self.fed
    }

    /// Samples already handed to the decode thread in committed windows.
    pub fn committed_samples(&self) -> usize {
        self.committed_samples
    }

    /// Windows handed to the decode thread so far.
    pub fn windows_committed(&self) -> usize {
        self.windows_sent
    }

    /// Committed windows not yet decoded (in flight included). The bench
    /// paces its feed on this.
    pub fn backlog(&self) -> usize {
        self.shared.queued.load(Ordering::Acquire)
    }

    pub fn is_degraded(&self) -> bool {
        self.shared.degraded.load(Ordering::Acquire)
    }

    /// Feed 16 kHz mono samples as they land; `gain` is the take's running
    /// AGC gain, so a window decodes at the level the batch path would.
    pub fn push(&mut self, samples: &[f32], gain: f32) {
        if self.sealed.is_some() || samples.is_empty() {
            return;
        }
        self.fed += samples.len();
        if self.is_degraded() {
            // The batch path decodes the whole take; buffering is pointless.
            self.pending.clear();
            self.frames.clear();
            return;
        }
        let gain = if gain.is_finite() && gain > 0.0 {
            gain
        } else {
            1.0
        };
        let mut new_frame = false;
        for &s in samples {
            let v = if s.is_finite() {
                (s * gain).clamp(-1.0, 1.0)
            } else {
                0.0
            };
            self.pending.push(v);
            self.acc_sq += f64::from(v) * f64::from(v);
            self.acc_n += 1;
            if self.acc_n == self.frame_samples {
                self.frames
                    .push((self.acc_sq / self.acc_n as f64).sqrt() as f32);
                self.acc_sq = 0.0;
                self.acc_n = 0;
                new_frame = true;
            }
        }
        if !new_frame {
            return;
        }
        while let Some(cut) = find_cut(&self.frames, &self.cfg) {
            if cut == 0 {
                break;
            }
            self.commit_window(cut);
            if self.is_degraded() {
                self.pending.clear();
                self.frames.clear();
                break;
            }
        }
    }

    fn commit_window(&mut self, cut_frame: usize) {
        let n = (cut_frame * self.frame_samples).min(self.pending.len());
        let samples: Vec<f32> = self.pending.drain(..n).collect();
        let drop_frames = cut_frame.min(self.frames.len());
        self.frames.drain(..drop_frames);
        self.committed_samples += n;
        let index = self.windows_sent;
        self.windows_sent += 1;
        let backlog = self.shared.queued.fetch_add(1, Ordering::AcqRel) + 1;
        if backlog > self.cfg.max_backlog_windows {
            self.shared.degrade(self.take, REASON_BACKLOG);
        }
        let sent = self
            .tx
            .as_ref()
            .map(|tx| tx.send(Job { index, samples }).is_ok())
            .unwrap_or(false);
        if !sent {
            self.shared.queued.fetch_sub(1, Ordering::AcqRel);
            self.shared.degrade(self.take, REASON_WORKER_GONE);
        }
    }

    /// The capture closed with `captured_total` samples. Nothing is accepted
    /// after this, and [`finish`](Self::finish) refuses a stream whose count
    /// does not match.
    pub fn seal(&mut self, captured_total: usize) {
        if self.sealed.is_none() {
            self.sealed = Some(captured_total);
        }
    }

    /// Release: wait for the committed windows, decode ONLY the tail, and
    /// hand back the take's text — or say why the batch path must run.
    pub fn finish(mut self) -> StreamOutcome {
        let released = Instant::now();
        let capture_wall_ms = released.duration_since(self.started).as_millis() as u64;
        drop(self.tx.take());
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                self.shared.degrade(self.take, REASON_WORKER_GONE);
            }
        }
        let (degraded, windows_done, committed, capture_decode_ms, audio_seconds) = {
            let p = lock(&self.shared.progress);
            (
                p.degraded.clone(),
                p.windows_done,
                p.committed.clone(),
                p.decode_ms,
                p.audio_seconds,
            )
        };
        if let Some(reason) = degraded {
            return StreamOutcome::Degraded(reason);
        }
        match self.sealed {
            None => {
                self.shared.degrade(self.take, REASON_UNSEALED);
                return StreamOutcome::Degraded(REASON_UNSEALED.into());
            }
            Some(total) if total != self.fed => {
                log::warn!(
                    "stream_decode capture_mismatch take={} fed={} captured={total}",
                    self.take,
                    self.fed
                );
                self.shared.degrade(self.take, REASON_CAPTURE_MISMATCH);
                return StreamOutcome::Degraded(REASON_CAPTURE_MISMATCH.into());
            }
            Some(_) => {}
        }
        if windows_done == 0 {
            log::info!(
                "stream_decode not_used take={} reason={REASON_SHORT}",
                self.take
            );
            return StreamOutcome::NotUsed(REASON_SHORT);
        }
        if windows_done != self.windows_sent {
            self.shared.degrade(self.take, REASON_INCOMPLETE);
            return StreamOutcome::Degraded(REASON_INCOMPLETE.into());
        }
        let mut tail = std::mem::take(&mut self.pending);
        let rate = self.cfg.sample_rate.max(1) as f64;
        let trimmed = trim_trailing_quiet(&mut tail, &self.cfg);
        if trimmed > 0 {
            log::debug!(
                "stream_decode tail_trimmed take={} quiet_ms={}",
                self.take,
                (trimmed as f64 * 1000.0 / rate) as u64
            );
        }
        let tail_seconds = tail.len() as f64 / rate;
        let tail_started = Instant::now();
        let tail_text = if tail.is_empty()
            || rms(&tail) < QUIET_RMS_FLOOR
            || (tail_seconds < TAIL_SKIP_SECONDS && rms(&tail) < QUIET_RMS_FLOOR * 4.0)
        {
            String::new()
        } else {
            match self.decoder.decode(tail) {
                Ok(text) => text,
                Err(e) => {
                    log::warn!("stream_decode tail_failed take={} error={e}", self.take);
                    self.shared.degrade(self.take, REASON_TAIL_FAILED);
                    return StreamOutcome::Degraded(REASON_TAIL_FAILED.into());
                }
            }
        };
        let tail_ms = tail_started.elapsed().as_millis() as u64;
        let release_ms = released.elapsed().as_millis() as u64;
        let mut parts = committed;
        parts.push(tail_text);
        let text = join_texts(&parts);
        let partial = {
            let mut p = lock(&self.shared.progress);
            p.throttle
                .admit(Instant::now(), self.cfg.partial_interval)
                .then(|| TranscriptPartial {
                    take: self.take,
                    committed: text.clone(),
                    tentative: String::new(),
                })
        };
        if let (Some(sink), Some(partial)) = (self.sink.as_ref(), partial) {
            sink(&partial);
        }
        let duty_pct = if capture_wall_ms == 0 {
            0.0
        } else {
            capture_decode_ms as f64 * 100.0 / capture_wall_ms as f64
        };
        log::info!(
            "stream_decode finish take={} windows={} tail_s={tail_seconds:.2} tail_ms={tail_ms} release_ms={release_ms} capture_decode_ms={capture_decode_ms} capture_ms={capture_wall_ms} committed_audio_s={audio_seconds:.1} duty_pct={duty_pct:.1}",
            self.take,
            windows_done + 1
        );
        StreamOutcome::Streamed(StreamedTake {
            text,
            windows: windows_done + 1,
            tail_seconds,
            tail_ms,
            release_ms,
            capture_decode_ms,
            committed_audio_seconds: audio_seconds,
        })
    }
}

impl Drop for StreamDecoder {
    fn drop(&mut self) {
        // A stream nobody finished (replaced, cancelled, no-speech): its worker
        // skips whatever is still queued instead of holding the engine.
        if self.worker.is_some() {
            self.shared.degrade(self.take, REASON_DROPPED);
        }
    }
}

fn worker_loop(
    rx: mpsc::Receiver<Job>,
    shared: Arc<Shared>,
    decoder: Arc<dyn WindowDecoder>,
    sink: Option<PartialSink>,
    take: u64,
    cfg: StreamConfig,
) {
    let rate = cfg.sample_rate.max(1) as f64;
    for job in rx {
        if shared.degraded.load(Ordering::Acquire) {
            shared.queued.fetch_sub(1, Ordering::AcqRel);
            continue;
        }
        if decoder.constrained() {
            shared.degrade(take, REASON_GOVERNOR);
            shared.queued.fetch_sub(1, Ordering::AcqRel);
            continue;
        }
        let audio_seconds = job.samples.len() as f64 / rate;
        let started = Instant::now();
        // A window of pure silence (the user held the key and said nothing)
        // commits as no words instead of asking the model to invent some.
        let result = if rms(&job.samples) < QUIET_RMS_FLOOR {
            Ok(String::new())
        } else {
            decoder.decode(job.samples)
        };
        let ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(text) => {
                let partial = {
                    let mut p = lock(&shared.progress);
                    p.committed.push(text.trim().to_string());
                    p.windows_done += 1;
                    p.decode_ms += ms;
                    p.audio_seconds += audio_seconds;
                    let committed = join_texts(&p.committed);
                    p.throttle
                        .admit(Instant::now(), cfg.partial_interval)
                        .then(|| TranscriptPartial {
                            take,
                            committed,
                            tentative: String::new(),
                        })
                };
                let backlog = shared.queued.load(Ordering::Acquire).saturating_sub(1);
                log::info!(
                    "stream_decode window take={take} index={} audio_ms={} decode_ms={ms} backlog={backlog}",
                    job.index,
                    (audio_seconds * 1000.0) as u64
                );
                if let (Some(sink), Some(partial)) = (sink.as_ref(), partial) {
                    sink(&partial);
                }
            }
            Err(e) => {
                log::warn!(
                    "stream_decode window_failed take={take} index={} error={e}",
                    job.index
                );
                shared.degrade(take, REASON_WINDOW_FAILED);
            }
        }
        shared.queued.fetch_sub(1, Ordering::AcqRel);
    }
}

// ---------------------------------------------------------------------------
// The one live stream (the take being recorded right now).
// ---------------------------------------------------------------------------

static ACTIVE: Mutex<Option<StreamDecoder>> = Mutex::new(None);
static NEXT_TAKE: AtomicU64 = AtomicU64::new(1);

/// A fresh id for the next take's `transcript_partial` events.
pub fn next_take_id() -> u64 {
    NEXT_TAKE.fetch_add(1, Ordering::Relaxed)
}

/// The language a stream may decode in: one fixed language. An auto-detect
/// set (two or more languages, auto on) is decided per take after release, so
/// it never streams.
pub fn stream_language(primary: &str, set_len: usize, auto: bool) -> Option<String> {
    if (auto && set_len > 1) || primary.trim().is_empty() {
        None
    } else {
        Some(primary.to_string())
    }
}

/// Install the stream for the take that is about to record. A leftover stream
/// is dropped (its worker stands down).
pub fn arm(decoder: StreamDecoder) {
    let previous = lock(&ACTIVE).replace(decoder);
    if let Some(previous) = previous {
        log::info!(
            "stream_decode discarded take={} reason=replaced",
            previous.take_id()
        );
    }
}

/// Drop the live stream, if any.
pub fn disarm(reason: &str) {
    let previous = lock(&ACTIVE).take();
    if let Some(previous) = previous {
        log::info!(
            "stream_decode discarded take={} reason={reason}",
            previous.take_id()
        );
    }
}

/// Capture consumer → the live stream. A no-op with nothing armed.
pub fn feed(samples: &[f32], gain: f32) {
    if samples.is_empty() {
        return;
    }
    if let Some(stream) = lock(&ACTIVE).as_mut() {
        stream.push(samples, gain);
    }
}

/// The capture closed with `captured_total` 16 kHz samples.
pub fn seal(captured_total: usize) {
    if let Some(stream) = lock(&ACTIVE).as_mut() {
        stream.seal(captured_total);
    }
}

/// Take the live stream out for the release path to finish.
pub fn take_active() -> Option<StreamDecoder> {
    lock(&ACTIVE).take()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: usize = STREAM_SAMPLE_RATE as usize;

    fn tone(seconds: f64) -> Vec<f32> {
        let n = (seconds * RATE as f64) as usize;
        (0..n)
            .map(|i| 0.3 * (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / RATE as f32).sin())
            .collect()
    }

    fn silence(seconds: f64) -> Vec<f32> {
        vec![0.0; (seconds * RATE as f64) as usize]
    }

    fn small() -> StreamConfig {
        StreamConfig {
            min_window_seconds: 2.0,
            max_window_seconds: 3.0,
            pause_seconds: 0.3,
            // Generous here so a slow test thread never trips the backlog;
            // the backlog test sets the shipped 2 explicitly.
            max_backlog_windows: 100,
            partial_interval: Duration::from_millis(0),
            sample_rate: STREAM_SAMPLE_RATE,
        }
    }

    /// Records every call's length; answers "w<n>".
    struct Stub {
        calls: Mutex<Vec<usize>>,
        delay: Duration,
        fail_on: Option<usize>,
        constrained: bool,
    }

    impl Stub {
        fn new() -> Arc<Stub> {
            Arc::new(Stub {
                calls: Mutex::new(Vec::new()),
                delay: Duration::from_millis(0),
                fail_on: None,
                constrained: false,
            })
        }
        fn lens(&self) -> Vec<usize> {
            lock(&self.calls).clone()
        }
    }

    impl WindowDecoder for Stub {
        fn decode(&self, samples: Vec<f32>) -> Result<String, String> {
            std::thread::sleep(self.delay);
            let n = {
                let mut calls = lock(&self.calls);
                calls.push(samples.len());
                calls.len()
            };
            if self.fail_on == Some(n) {
                return Err("engine error".into());
            }
            Ok(format!("w{n}"))
        }
        fn constrained(&self) -> bool {
            self.constrained
        }
    }

    fn run(
        cfg: StreamConfig,
        stub: Arc<Stub>,
        audio: &[f32],
        sink: Option<PartialSink>,
    ) -> StreamOutcome {
        let mut s = StreamDecoder::start(7, cfg, stub, sink);
        for chunk in audio.chunks(160) {
            s.push(chunk, 1.0);
        }
        s.seal(audio.len());
        s.finish()
    }

    #[test]
    fn find_cut_waits_for_the_minimum_window() {
        let cfg = small();
        let min = cfg.frames_for(cfg.min_window_seconds);
        // Nothing below `min` frames ever cuts, however quiet…
        let quiet: Vec<f32> = std::iter::repeat(0.0f32).take(min - 1).collect();
        assert_eq!(find_cut(&quiet, &cfg), None);
        // …and steady speech under the maximum keeps listening.
        let speech: Vec<f32> = std::iter::repeat(0.2f32).take(min).collect();
        assert_eq!(find_cut(&speech, &cfg), None);
    }

    #[test]
    fn find_cut_prefers_a_pause_between_min_and_max() {
        let cfg = small();
        let min = cfg.frames_for(cfg.min_window_seconds);
        let need = cfg.frames_for(cfg.pause_seconds);
        let mut frames = vec![0.2f32; min + 5];
        frames.extend(vec![0.0f32; need + 2]);
        frames.extend(vec![0.2f32; 3]);
        let cut = find_cut(&frames, &cfg).expect("a pause past the minimum cuts");
        assert!(
            cut >= min + 5 && cut < min + 5 + need + 2,
            "cut {cut} is inside the pause"
        );
    }

    #[test]
    fn find_cut_forces_the_quietest_frame_at_max() {
        let cfg = small();
        let min = cfg.frames_for(cfg.min_window_seconds);
        let max = cfg.frames_for(cfg.max_window_seconds);
        let mut frames = vec![0.2f32; max + 4];
        frames[min + 3] = 0.15;
        assert_eq!(find_cut(&frames, &cfg), Some(min + 3));
        assert_eq!(find_cut(&frames[..max - 1], &cfg), None);
    }

    #[test]
    fn partial_throttle_admits_at_most_every_500ms() {
        let mut t = PartialThrottle::default();
        let t0 = Instant::now();
        assert!(t.admit(t0, PARTIAL_MIN_INTERVAL));
        assert!(!t.admit(t0 + Duration::from_millis(100), PARTIAL_MIN_INTERVAL));
        assert!(!t.admit(t0 + Duration::from_millis(499), PARTIAL_MIN_INTERVAL));
        assert!(t.admit(t0 + Duration::from_millis(500), PARTIAL_MIN_INTERVAL));
        assert!(!t.admit(t0 + Duration::from_millis(900), PARTIAL_MIN_INTERVAL));
    }

    #[test]
    fn release_decodes_only_the_uncommitted_tail_and_loses_nothing() {
        let cfg = small();
        let stub = Stub::new();
        let mut audio = tone(2.5);
        audio.extend(silence(0.5));
        audio.extend(tone(5.0));
        let out = run(cfg, stub.clone(), &audio, None);
        let take = match out {
            StreamOutcome::Streamed(take) => take,
            other => panic!("expected a streamed take, got {other:?}"),
        };
        let lens = stub.lens();
        assert!(
            lens.len() >= 2,
            "at least one committed window plus the tail"
        );
        assert_eq!(take.windows, lens.len());
        // The tail is the LAST call and never longer than one window.
        assert!(*lens.last().unwrap() <= cfg.max_window_samples());
        // No loss: every fed sample was decoded exactly once.
        assert_eq!(lens.iter().sum::<usize>(), audio.len());
        // The first window was cut inside the pause (2.5–3.0 s).
        let first = lens[0] as f64 / RATE as f64;
        assert!((2.5..=3.0).contains(&first), "first window {first}s");
        let expected: Vec<String> = (1..=lens.len()).map(|n| format!("w{n}")).collect();
        assert_eq!(take.text, expected.join(" "));
    }

    #[test]
    fn a_take_shorter_than_one_window_is_not_streamed() {
        let stub = Stub::new();
        let out = run(small(), stub.clone(), &tone(1.5), None);
        assert_eq!(out, StreamOutcome::NotUsed(REASON_SHORT));
        assert!(
            stub.lens().is_empty(),
            "the batch path decodes it, not the stream"
        );
    }

    #[test]
    fn a_backlog_over_two_windows_degrades_to_batch() {
        let stub = Arc::new(Stub {
            calls: Mutex::new(Vec::new()),
            delay: Duration::from_millis(300),
            fail_on: None,
            constrained: false,
        });
        let cfg = StreamConfig {
            max_backlog_windows: 2,
            ..small()
        };
        let mut s = StreamDecoder::start(1, cfg, stub, None);
        let audio = tone(12.0);
        s.push(&audio, 1.0);
        s.seal(audio.len());
        assert_eq!(s.finish(), StreamOutcome::Degraded(REASON_BACKLOG.into()));
    }

    #[test]
    fn a_constrained_governor_degrades_to_batch() {
        let stub = Arc::new(Stub {
            calls: Mutex::new(Vec::new()),
            delay: Duration::from_millis(0),
            fail_on: None,
            constrained: true,
        });
        let out = run(small(), stub.clone(), &tone(5.0), None);
        assert_eq!(out, StreamOutcome::Degraded(REASON_GOVERNOR.into()));
        assert!(stub.lens().is_empty());
    }

    #[test]
    fn a_failed_window_degrades_to_batch() {
        let stub = Arc::new(Stub {
            calls: Mutex::new(Vec::new()),
            delay: Duration::from_millis(0),
            fail_on: Some(1),
            constrained: false,
        });
        let out = run(small(), stub, &tone(5.0), None);
        assert_eq!(out, StreamOutcome::Degraded(REASON_WINDOW_FAILED.into()));
    }

    #[test]
    fn a_capture_count_mismatch_degrades_to_batch() {
        let stub = Stub::new();
        let audio = tone(5.0);
        let mut s = StreamDecoder::start(2, small(), stub, None);
        s.push(&audio, 1.0);
        s.seal(audio.len() + 1);
        assert_eq!(
            s.finish(),
            StreamOutcome::Degraded(REASON_CAPTURE_MISMATCH.into())
        );
    }

    #[test]
    fn committed_text_is_emitted_as_transcript_partial() {
        let seen: Arc<Mutex<Vec<TranscriptPartial>>> = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let sink: PartialSink = Arc::new(move |p: &TranscriptPartial| {
            lock(&seen2).push(p.clone());
        });
        let out = run(small(), Stub::new(), &tone(5.0), Some(sink));
        assert!(matches!(out, StreamOutcome::Streamed(_)));
        let seen = lock(&seen).clone();
        assert!(!seen.is_empty());
        assert_eq!(seen[0].take, 7);
        assert_eq!(seen[0].committed, "w1");
        assert_eq!(seen[0].tentative, "");
        let json = serde_json::to_value(&seen[0]).unwrap();
        assert!(json.get("take").is_some());
        assert!(json.get("committed").is_some());
        assert!(json.get("tentative").is_some());
    }

    #[test]
    fn partials_respect_the_interval() {
        let seen: Arc<Mutex<Vec<TranscriptPartial>>> = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let sink: PartialSink = Arc::new(move |p: &TranscriptPartial| {
            lock(&seen2).push(p.clone());
        });
        let cfg = StreamConfig {
            partial_interval: Duration::from_secs(3600),
            ..small()
        };
        let _ = run(cfg, Stub::new(), &tone(12.0), Some(sink));
        assert_eq!(lock(&seen).len(), 1, "one emit inside one interval");
    }

    #[test]
    fn a_release_tail_loses_only_trailing_silence() {
        let cfg = small();
        let mut tail = tone(1.0);
        tail.extend(silence(2.0));
        let dropped = trim_trailing_quiet(&mut tail, &cfg);
        let pad = (TAIL_PAD_SECONDS * RATE as f64) as usize;
        assert!(tail.len() >= RATE && tail.len() <= RATE + pad + cfg.frame_samples());
        assert_eq!(dropped + tail.len(), 3 * RATE);
        let mut voiced = tone(1.0);
        assert_eq!(trim_trailing_quiet(&mut voiced, &cfg), 0);
    }

    #[test]
    fn a_silent_window_is_never_sent_to_the_model() {
        let stub = Stub::new();
        let mut audio = silence(3.5);
        audio.extend(tone(1.0));
        let out = run(small(), stub.clone(), &audio, None);
        assert!(matches!(out, StreamOutcome::Streamed(_)), "{out:?}");
        // Only the voiced tail reached the model.
        let lens = stub.lens();
        assert_eq!(lens.len(), 1);
    }

    #[test]
    fn an_auto_detect_set_never_streams() {
        assert_eq!(stream_language("en", 2, true), None);
        assert_eq!(stream_language("en", 1, true), Some("en".into()));
        assert_eq!(stream_language("fr", 3, false), Some("fr".into()));
    }
}
