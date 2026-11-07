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
//!   [`PARTIAL_MIN_INTERVAL`].
//! * **The tentative tail is display only** (yap-1106-partial-transcript-
//!   stream). A committed window lands 20–30 s into a take, far too late for
//!   the pill, so while the uncommitted audio grows the stream also decodes
//!   its newest [`TentativeConfig::max_seconds`] about once a
//!   [`TentativeConfig::interval`] and sends that guess as `tentative`. The
//!   guess never reaches [`StreamedTake::text`], history or the polish feed;
//!   it is dropped the moment a window commits over its audio, it never runs
//!   while a committed window is queued (committed work always goes first),
//!   it runs only while the resource governor is at full (the pill hides the
//!   words at any other level), and it is held to
//!   [`TentativeConfig::max_duty`] (10 %) of the take's wall time
//!   ([`tentative_gate`]), so it adds at most that much decode over a take
//!   that streams without it.
//!
//! Telemetry: one `stream_decode window …` line per decoded window and one
//! `stream_decode finish …` line per take, carrying the decode time spent
//! during capture and its duty cycle (`duty_pct`, decode wall time over capture
//! wall time) — the CPU+GPU overhead proxy the energy bench checks against —
//! plus the tentative tail's own count, decode time, duty and lag (the time
//! from the newest sample a guess covers to its emit), and one
//! `stream_decode tentative_summary …` line per take whatever its outcome.
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

/// yap-1106-partial-transcript-stream — how the live guess at the
/// uncommitted words is paced. `None` in [`StreamConfig::tentative`] turns it
/// off (the SP-05 unit tests run without it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TentativeConfig {
    /// The fastest a new guess starts. The pill's lag behind speech is about
    /// this plus one guess's decode time.
    pub interval: Duration,
    /// Uncommitted audio shorter than this is not guessed at (the model only
    /// invents words on a sliver).
    pub min_seconds: f64,
    /// Only the newest this-many seconds of uncommitted audio are decoded:
    /// the pill shows the newest words, and a short slice keeps a guess cheap.
    pub max_seconds: f64,
    /// Ceiling on guess decode time over the take's wall time (0.10 = 10 %).
    pub max_duty: f64,
}

impl Default for TentativeConfig {
    fn default() -> Self {
        TentativeConfig {
            interval: Duration::from_millis(1000),
            min_seconds: 1.0,
            max_seconds: 5.0,
            max_duty: 0.10,
        }
    }
}

/// Why [`tentative_gate`] did or did not start a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TentativeGate {
    Go,
    /// Inside the interval since the last guess (or since the take began).
    Wait,
    /// A guess or a committed window is still being decoded.
    Busy,
    /// Not enough uncommitted audio yet.
    Short,
    /// Another guess now would push guess decode past `max_duty`.
    Budget,
}

/// What [`tentative_gate`] decides on — a plain snapshot, so the pacing is
/// testable on a simulated clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TentativeClock {
    /// Take wall time so far.
    pub since_start: Duration,
    /// Since the last guess started, `None` before the first.
    pub since_last: Option<Duration>,
    /// Guess decode time spent this take.
    pub spent: Duration,
    /// The last guess's decode time: the estimate of what the next one costs.
    pub last_cost: Duration,
    /// Uncommitted samples.
    pub pending_samples: usize,
    /// A guess or a committed window is in flight.
    pub busy: bool,
}

/// Start a guess now? Pure: the whole tentative pacing is this function.
///
/// Two budget rules keep guess decode at `max_duty` of the take: each guess's
/// cost is spread over the gap after it (the next one waits at least
/// `last_cost / max_duty`, so a 150 ms guess repeats every 1.5 s and a 50 ms
/// one every `interval`), and the take's total guess time may not run ahead of
/// `max_duty × since_start`. Together the take's guess duty never exceeds
/// `max_duty` by more than one guess.
pub fn tentative_gate(t: &TentativeConfig, c: &TentativeClock, sample_rate: u32) -> TentativeGate {
    if c.busy {
        return TentativeGate::Busy;
    }
    let waited = c.since_last.unwrap_or(c.since_start);
    if waited < t.interval {
        return TentativeGate::Wait;
    }
    let rate = f64::from(sample_rate.max(1));
    if (c.pending_samples as f64) / rate < t.min_seconds {
        return TentativeGate::Short;
    }
    if t.max_duty <= 0.0 {
        return TentativeGate::Budget;
    }
    let pace = c.last_cost.as_secs_f64() / t.max_duty;
    let ahead = c.spent.as_secs_f64() > c.since_start.as_secs_f64() * t.max_duty;
    if waited.as_secs_f64() < pace || ahead {
        return TentativeGate::Budget;
    }
    TentativeGate::Go
}

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
    /// The live guess at the uncommitted words (`None` = off).
    pub tentative: Option<TentativeConfig>,
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
            tentative: Some(TentativeConfig::default()),
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
    /// yap-1106-partial-transcript-stream — may a display-only guess run now?
    /// The app allows one only while the governor runs at full (the pill hides
    /// live words at any other level, so a guess then would be wasted work).
    fn tentative_allowed(&self) -> bool {
        !self.constrained()
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
    /// Tentative guesses that reached the pill this take (display only).
    pub tentatives: usize,
    /// Decode time the guesses spent during capture.
    pub tentative_ms: u64,
    /// The worst speech → pill lag of an emitted guess.
    pub tentative_lag_max_ms: u64,
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

enum Job {
    /// A committed window: its text becomes part of the take.
    Window { index: usize, samples: Vec<f32> },
    /// The newest uncommitted audio, guessed at for the pill only.
    Tentative {
        samples: Vec<f32>,
        /// Windows committed when the guess was cut; any newer commit makes
        /// the guess stale.
        base_windows: usize,
        /// When its newest sample arrived (the lag clock).
        spoken_at: Instant,
    },
}

#[derive(Default)]
struct Progress {
    committed: Vec<String>,
    degraded: Option<String>,
    windows_done: usize,
    decode_ms: u64,
    audio_seconds: f64,
    throttle: PartialThrottle,
    tentatives: usize,
    tentative_failed: usize,
    tentative_stale: usize,
    tentative_lag_sum_ms: u64,
    tentative_lag_max_ms: u64,
    /// Set (to the last reason) while a guess was refused by the governor.
    tentative_off: Option<&'static str>,
}

struct Shared {
    progress: Mutex<Progress>,
    /// Committed windows not yet finished (in flight included).
    queued: AtomicUsize,
    /// Cheap mirror of `progress.degraded.is_some()` for the capture thread.
    degraded: AtomicBool,
    /// Windows committed so far (a guess cut before the newest is stale).
    sent: AtomicUsize,
    /// The take was sealed or released: queued guesses are skipped.
    released: AtomicBool,
    /// A guess is queued or decoding.
    tentative_busy: AtomicBool,
    /// Guess decode time this take, and the last guess's.
    tentative_spent_ms: AtomicU64,
    tentative_last_ms: AtomicU64,
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
    /// When the last guess started (or was last refused as quiet/governed).
    last_tentative: Option<Instant>,
    /// `fed` when the last guess was cut: the new audio since must hold speech.
    tentative_fed_at: usize,
    tentative_skipped_budget: usize,
    last_budget_skip: Option<Instant>,
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
            sent: AtomicUsize::new(0),
            released: AtomicBool::new(false),
            tentative_busy: AtomicBool::new(false),
            tentative_spent_ms: AtomicU64::new(0),
            tentative_last_ms: AtomicU64::new(0),
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
            last_tentative: None,
            tentative_fed_at: 0,
            tentative_skipped_budget: 0,
            last_budget_skip: None,
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
        self.maybe_tentative();
    }

    /// yap-1106-partial-transcript-stream — cut a guess from the newest
    /// uncommitted audio when [`tentative_gate`] allows one. Runs on the
    /// capture thread, so it only snapshots atomics and copies at most
    /// [`TentativeConfig::max_seconds`] of samples.
    fn maybe_tentative(&mut self) {
        let Some(t) = self.cfg.tentative else {
            return;
        };
        if self.is_degraded() || self.sealed.is_some() || self.tx.is_none() {
            return;
        }
        let now = Instant::now();
        let busy = self.shared.tentative_busy.load(Ordering::Acquire)
            || self.shared.queued.load(Ordering::Acquire) > 0;
        let clock = TentativeClock {
            since_start: now.saturating_duration_since(self.started),
            since_last: self
                .last_tentative
                .map(|at| now.saturating_duration_since(at)),
            spent: Duration::from_millis(self.shared.tentative_spent_ms.load(Ordering::Acquire)),
            last_cost: Duration::from_millis(self.shared.tentative_last_ms.load(Ordering::Acquire)),
            pending_samples: self.pending.len(),
            busy,
        };
        match tentative_gate(&t, &clock, self.cfg.sample_rate) {
            TentativeGate::Go => {}
            TentativeGate::Budget => {
                // Counted once per interval, not once per 30 ms frame.
                let due = self
                    .last_budget_skip
                    .is_none_or(|at| now.saturating_duration_since(at) >= t.interval);
                if due {
                    self.tentative_skipped_budget += 1;
                    self.last_budget_skip = Some(now);
                }
                return;
            }
            TentativeGate::Wait | TentativeGate::Busy | TentativeGate::Short => return,
        }
        // The audio since the last guess must hold speech, or the last guess
        // still stands and the model is not asked again.
        let fresh = self
            .fed
            .saturating_sub(self.tentative_fed_at)
            .min(self.pending.len());
        let fresh_quiet =
            fresh == 0 || rms(&self.pending[self.pending.len() - fresh..]) < QUIET_RMS_FLOOR;
        if fresh_quiet {
            self.last_tentative = Some(now);
            return;
        }
        if !self.decoder.tentative_allowed() {
            let first = {
                let mut p = lock(&self.shared.progress);
                let first = p.tentative_off.is_none();
                p.tentative_off = Some(REASON_GOVERNOR);
                first
            };
            if first {
                log::info!(
                    "stream_decode tentative_off take={} reason={REASON_GOVERNOR}",
                    self.take
                );
            }
            self.last_tentative = Some(now);
            return;
        }
        let rate = f64::from(self.cfg.sample_rate.max(1));
        let max = ((t.max_seconds.max(0.0) * rate) as usize).max(1);
        let start = self.pending.len().saturating_sub(max);
        let samples = self.pending[start..].to_vec();
        self.shared.tentative_busy.store(true, Ordering::Release);
        let sent = self
            .tx
            .as_ref()
            .map(|tx| {
                tx.send(Job::Tentative {
                    samples,
                    base_windows: self.windows_sent,
                    spoken_at: now,
                })
                .is_ok()
            })
            .unwrap_or(false);
        if !sent {
            self.shared.tentative_busy.store(false, Ordering::Release);
            return;
        }
        self.last_tentative = Some(now);
        self.tentative_fed_at = self.fed;
    }

    fn commit_window(&mut self, cut_frame: usize) {
        let n = (cut_frame * self.frame_samples).min(self.pending.len());
        let samples: Vec<f32> = self.pending.drain(..n).collect();
        let drop_frames = cut_frame.min(self.frames.len());
        self.frames.drain(..drop_frames);
        self.committed_samples += n;
        let index = self.windows_sent;
        self.windows_sent += 1;
        self.shared.sent.store(self.windows_sent, Ordering::Release);
        let backlog = self.shared.queued.fetch_add(1, Ordering::AcqRel) + 1;
        if backlog > self.cfg.max_backlog_windows {
            self.shared.degrade(self.take, REASON_BACKLOG);
        }
        let sent = self
            .tx
            .as_ref()
            .map(|tx| tx.send(Job::Window { index, samples }).is_ok())
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
        // No guess after the key comes up: release time belongs to the tail.
        self.shared.released.store(true, Ordering::Release);
    }

    /// Release: wait for the committed windows, decode ONLY the tail, and
    /// hand back the take's text — or say why the batch path must run.
    pub fn finish(mut self) -> StreamOutcome {
        let released = Instant::now();
        let capture_wall_ms = released.duration_since(self.started).as_millis() as u64;
        self.shared.released.store(true, Ordering::Release);
        drop(self.tx.take());
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                self.shared.degrade(self.take, REASON_WORKER_GONE);
            }
        }
        let (tentatives, tentative_ms, tentative_lag_max_ms) =
            self.log_tentative_summary(capture_wall_ms);
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
            tentatives,
            tentative_ms,
            tentative_lag_max_ms,
        })
    }

    /// One `stream_decode tentative_summary …` line per take that guessed
    /// (any outcome): count, failures, stale drops, budget skips, decode time,
    /// duty and lag. Counts and timings only, never text.
    fn log_tentative_summary(&self, capture_wall_ms: u64) -> (usize, u64, u64) {
        let (count, failed, stale, lag_sum, lag_max, off) = {
            let p = lock(&self.shared.progress);
            (
                p.tentatives,
                p.tentative_failed,
                p.tentative_stale,
                p.tentative_lag_sum_ms,
                p.tentative_lag_max_ms,
                p.tentative_off,
            )
        };
        let spent_ms = self.shared.tentative_spent_ms.load(Ordering::Acquire);
        if self.cfg.tentative.is_some()
            && (count + failed + stale + self.tentative_skipped_budget > 0 || off.is_some())
        {
            let duty_pct = if capture_wall_ms == 0 {
                0.0
            } else {
                spent_ms as f64 * 100.0 / capture_wall_ms as f64
            };
            let lag_avg_ms = if count == 0 {
                0
            } else {
                lag_sum / count as u64
            };
            log::info!(
                "stream_decode tentative_summary take={} count={count} failed={failed} stale={stale} skipped_budget={} spent_ms={spent_ms} capture_ms={capture_wall_ms} duty_pct={duty_pct:.1} lag_avg_ms={lag_avg_ms} lag_max_ms={lag_max} off={}",
                self.take,
                self.tentative_skipped_budget,
                off.unwrap_or("none")
            );
        }
        (count, spent_ms, lag_max)
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
        let (index, samples) = match job {
            Job::Window { index, samples } => (index, samples),
            Job::Tentative {
                samples,
                base_windows,
                spoken_at,
            } => {
                run_tentative(
                    &shared,
                    &*decoder,
                    sink.as_ref(),
                    take,
                    &cfg,
                    samples,
                    base_windows,
                    spoken_at,
                );
                continue;
            }
        };
        if shared.degraded.load(Ordering::Acquire) {
            shared.queued.fetch_sub(1, Ordering::AcqRel);
            continue;
        }
        if decoder.constrained() {
            shared.degrade(take, REASON_GOVERNOR);
            shared.queued.fetch_sub(1, Ordering::AcqRel);
            continue;
        }
        let audio_seconds = samples.len() as f64 / rate;
        let started = Instant::now();
        // A window of pure silence (the user held the key and said nothing)
        // commits as no words instead of asking the model to invent some.
        let result = if rms(&samples) < QUIET_RMS_FLOOR {
            Ok(String::new())
        } else {
            decoder.decode(samples)
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
                    "stream_decode window take={take} index={index} audio_ms={} decode_ms={ms} backlog={backlog}",
                    (audio_seconds * 1000.0) as u64
                );
                if let (Some(sink), Some(partial)) = (sink.as_ref(), partial) {
                    sink(&partial);
                }
            }
            Err(e) => {
                log::warn!("stream_decode window_failed take={take} index={index} error={e}");
                shared.degrade(take, REASON_WINDOW_FAILED);
            }
        }
        shared.queued.fetch_sub(1, Ordering::AcqRel);
    }
}

/// yap-1106-partial-transcript-stream — decode one guess and, unless a window
/// committed over its audio meanwhile, send it to the pill as `tentative`
/// beside the committed text. A failed guess is only counted: committed
/// windows and the release path never depend on one.
#[allow(clippy::too_many_arguments)]
fn run_tentative(
    shared: &Shared,
    decoder: &dyn WindowDecoder,
    sink: Option<&PartialSink>,
    take: u64,
    cfg: &StreamConfig,
    samples: Vec<f32>,
    base_windows: usize,
    spoken_at: Instant,
) {
    let stale = |shared: &Shared| {
        shared.degraded.load(Ordering::Acquire)
            || shared.released.load(Ordering::Acquire)
            || shared.sent.load(Ordering::Acquire) != base_windows
    };
    if stale(shared) {
        lock(&shared.progress).tentative_stale += 1;
        shared.tentative_busy.store(false, Ordering::Release);
        return;
    }
    if !decoder.tentative_allowed() {
        lock(&shared.progress).tentative_off = Some(REASON_GOVERNOR);
        shared.tentative_busy.store(false, Ordering::Release);
        return;
    }
    let audio_ms = samples.len() as u64 * 1000 / u64::from(cfg.sample_rate.max(1));
    let started = Instant::now();
    let result = decoder.decode(samples);
    let ms = started.elapsed().as_millis() as u64;
    shared.tentative_spent_ms.fetch_add(ms, Ordering::AcqRel);
    shared.tentative_last_ms.store(ms, Ordering::Release);
    let partial = match result {
        Err(e) => {
            log::warn!("stream_decode tentative_failed take={take} error={e}");
            lock(&shared.progress).tentative_failed += 1;
            None
        }
        Ok(text) => {
            let mut p = lock(&shared.progress);
            if stale(shared) {
                p.tentative_stale += 1;
                None
            } else {
                let lag_ms = spoken_at.elapsed().as_millis() as u64;
                let tentative = text.trim().to_string();
                let admitted =
                    !tentative.is_empty() && p.throttle.admit(Instant::now(), cfg.partial_interval);
                if admitted {
                    p.tentatives += 1;
                    p.tentative_lag_sum_ms += lag_ms;
                    p.tentative_lag_max_ms = p.tentative_lag_max_ms.max(lag_ms);
                }
                log::debug!(
                    "stream_decode tentative take={take} audio_ms={audio_ms} decode_ms={ms} lag_ms={lag_ms} emitted={admitted}"
                );
                admitted.then(|| TranscriptPartial {
                    take,
                    committed: join_texts(&p.committed),
                    tentative,
                })
            }
        }
    };
    shared.tentative_busy.store(false, Ordering::Release);
    if let (Some(sink), Some(partial)) = (sink, partial) {
        sink(&partial);
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
            // The SP-05 tests pin exact decode calls; the tentative tests
            // below switch the guess on explicitly.
            tentative: None,
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
        let quiet: Vec<f32> = std::iter::repeat_n(0.0f32, min - 1).collect();
        assert_eq!(find_cut(&quiet, &cfg), None);
        // …and steady speech under the maximum keeps listening.
        let speech: Vec<f32> = std::iter::repeat_n(0.2f32, min).collect();
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

    // ---- yap-1106-partial-transcript-stream: the tentative tail ----------

    /// A guess every frame it is allowed: no interval, no budget, no windows.
    fn guessing() -> StreamConfig {
        StreamConfig {
            min_window_seconds: 100.0,
            max_window_seconds: 101.0,
            tentative: Some(TentativeConfig {
                interval: Duration::from_millis(0),
                min_seconds: 0.5,
                max_seconds: 1.0,
                max_duty: 1_000.0,
            }),
            ..small()
        }
    }

    fn collect() -> (PartialSink, Arc<Mutex<Vec<TranscriptPartial>>>) {
        let seen: Arc<Mutex<Vec<TranscriptPartial>>> = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let sink: PartialSink = Arc::new(move |p: &TranscriptPartial| {
            lock(&seen2).push(p.clone());
        });
        (sink, seen)
    }

    fn wait_for(mut done: impl FnMut() -> bool) {
        let until = Instant::now() + Duration::from_secs(5);
        while !done() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn clock(
        since_start: f64,
        since_last: Option<f64>,
        spent: f64,
        last_cost: f64,
    ) -> TentativeClock {
        TentativeClock {
            since_start: Duration::from_secs_f64(since_start),
            since_last: since_last.map(Duration::from_secs_f64),
            spent: Duration::from_secs_f64(spent),
            last_cost: Duration::from_secs_f64(last_cost),
            pending_samples: 10 * RATE,
            busy: false,
        }
    }

    #[test]
    fn tentative_gate_waits_for_the_interval_and_for_audio() {
        let t = TentativeConfig::default();
        let r = STREAM_SAMPLE_RATE;
        assert_eq!(
            tentative_gate(&t, &clock(0.5, None, 0.0, 0.0), r),
            TentativeGate::Wait
        );
        assert_eq!(
            tentative_gate(&t, &clock(1.2, None, 0.0, 0.0), r),
            TentativeGate::Go
        );
        assert_eq!(
            tentative_gate(&t, &clock(9.0, Some(0.4), 0.0, 0.0), r),
            TentativeGate::Wait
        );
        let short = TentativeClock {
            pending_samples: RATE / 2,
            ..clock(9.0, Some(2.0), 0.0, 0.0)
        };
        assert_eq!(tentative_gate(&t, &short, r), TentativeGate::Short);
        let busy = TentativeClock {
            busy: true,
            ..clock(9.0, Some(2.0), 0.0, 0.0)
        };
        assert_eq!(tentative_gate(&t, &busy, r), TentativeGate::Busy);
    }

    #[test]
    fn tentative_gate_holds_guess_decode_to_ten_percent_of_the_take() {
        let t = TentativeConfig::default();
        let r = STREAM_SAMPLE_RATE;
        // A 0.2 s guess is spread over 2 s: 1.5 s after it is too soon.
        assert_eq!(
            tentative_gate(&t, &clock(10.0, Some(1.5), 0.5, 0.2), r),
            TentativeGate::Budget
        );
        // 1.1 s spent in a 10 s take is ahead of 10 %.
        assert_eq!(
            tentative_gate(&t, &clock(10.0, Some(3.0), 1.1, 0.2), r),
            TentativeGate::Budget
        );
        assert_eq!(
            tentative_gate(&t, &clock(10.0, Some(2.0), 0.7, 0.2), r),
            TentativeGate::Go
        );
        // A zero budget never guesses.
        let none = TentativeConfig { max_duty: 0.0, ..t };
        assert_eq!(
            tentative_gate(&none, &clock(10.0, Some(3.0), 0.0, 0.0), r),
            TentativeGate::Budget
        );
    }

    /// A 30 s take on a simulated clock: with a guess costing 150 ms the gate
    /// starts one about every 1.5 s, so a word is on the pill at most ~1.65 s
    /// after it is spoken, and guess decode stays at or under 10 % of the take.
    #[test]
    fn a_cheap_guess_keeps_the_pill_under_two_seconds_behind_on_a_30s_take() {
        let t = TentativeConfig::default();
        let cost = Duration::from_millis(150);
        let step = Duration::from_millis(30);
        let mut now = Duration::ZERO;
        let mut last: Option<Duration> = None;
        let mut spent = Duration::ZERO;
        let mut busy_until = Duration::ZERO;
        let mut worst_lag = Duration::ZERO;
        let mut guesses = 0;
        while now <= Duration::from_secs(30) {
            let c = TentativeClock {
                since_start: now,
                since_last: last.map(|at| now - at),
                spent,
                last_cost: if guesses == 0 { Duration::ZERO } else { cost },
                pending_samples: 10 * RATE,
                busy: now < busy_until,
            };
            if tentative_gate(&t, &c, STREAM_SAMPLE_RATE) == TentativeGate::Go {
                // A word spoken just after the previous guess was cut waits
                // for this guess to be cut and decoded.
                let gap = now - last.unwrap_or(Duration::ZERO);
                worst_lag = worst_lag.max(gap + cost);
                last = Some(now);
                spent += cost;
                busy_until = now + cost;
                guesses += 1;
            }
            now += step;
        }
        assert!(guesses >= 15, "guesses={guesses}");
        assert!(
            worst_lag <= Duration::from_secs(2),
            "worst lag {worst_lag:?}"
        );
        let duty = spent.as_secs_f64() / 30.0;
        assert!(duty <= 0.10 + cost.as_secs_f64() / 30.0, "duty={duty}");
    }

    #[test]
    fn a_tentative_guess_reaches_the_pill_before_any_window_commits() {
        let (sink, seen) = collect();
        let stub = Stub::new();
        let mut s = StreamDecoder::start(7, guessing(), stub.clone(), Some(sink));
        let audio = tone(2.0);
        for chunk in audio.chunks(160) {
            s.push(chunk, 1.0);
        }
        wait_for(|| !lock(&seen).is_empty());
        let first = lock(&seen).first().cloned().expect("a tentative partial");
        assert_eq!(first.take, 7);
        assert_eq!(first.committed, "");
        assert!(first.tentative.starts_with('w'), "{first:?}");
        // Only the newest max_seconds were decoded.
        assert!(stub.lens().iter().all(|n| *n <= RATE), "{:?}", stub.lens());
        s.seal(audio.len());
        // No window ever committed: the take still decodes the batch way, so
        // the guess is nowhere in the take's text.
        assert_eq!(s.finish(), StreamOutcome::NotUsed(REASON_SHORT));
    }

    #[test]
    fn a_tentative_guess_is_never_part_of_the_take_text() {
        let (sink, seen) = collect();
        let cfg = StreamConfig {
            tentative: guessing().tentative,
            ..small()
        };
        let out = run(cfg, Stub::new(), &tone(5.0), Some(sink));
        let StreamOutcome::Streamed(take) = out else {
            panic!("expected a streamed take, got {out:?}");
        };
        // The text is exactly one word per committed window plus the tail;
        // no guess's word slipped in.
        assert_eq!(
            take.text.split_whitespace().count(),
            take.windows,
            "{take:?}"
        );
        for p in lock(&seen).iter().filter(|p| !p.tentative.is_empty()) {
            assert!(
                !take.text.split_whitespace().any(|w| w == p.tentative),
                "{p:?} in {take:?}"
            );
        }
    }

    #[test]
    fn a_guess_overtaken_by_a_committed_window_is_dropped() {
        let (sink, seen) = collect();
        let stub = Arc::new(Stub {
            calls: Mutex::new(Vec::new()),
            delay: Duration::from_millis(200),
            fail_on: None,
            constrained: false,
        });
        let cfg = StreamConfig {
            tentative: guessing().tentative,
            ..small()
        };
        let mut s = StreamDecoder::start(3, cfg, stub, Some(sink));
        let audio = tone(3.5);
        // The first guess is cut at 0.5 s and decodes for 200 ms; the window
        // at 2–3 s commits long before it returns.
        for chunk in audio.chunks(160) {
            s.push(chunk, 1.0);
        }
        wait_for(|| lock(&s.shared.progress).tentative_stale > 0);
        assert!(lock(&s.shared.progress).tentative_stale > 0);
        assert!(
            lock(&seen)
                .iter()
                .all(|p| p.tentative.is_empty() || !p.committed.is_empty()),
            "a guess cut before the window was shown after it"
        );
        s.seal(audio.len());
        let _ = s.finish();
    }

    #[test]
    fn a_constrained_governor_turns_the_guess_off() {
        let (sink, seen) = collect();
        let stub = Arc::new(Stub {
            calls: Mutex::new(Vec::new()),
            delay: Duration::from_millis(0),
            fail_on: None,
            constrained: true,
        });
        let mut s = StreamDecoder::start(5, guessing(), stub.clone(), Some(sink));
        let audio = tone(2.0);
        for chunk in audio.chunks(160) {
            s.push(chunk, 1.0);
        }
        assert!(stub.lens().is_empty(), "no guess reached the engine");
        assert!(lock(&seen).is_empty());
        assert_eq!(
            lock(&s.shared.progress).tentative_off,
            Some(REASON_GOVERNOR)
        );
        s.seal(audio.len());
        let _ = s.finish();
    }

    #[test]
    fn no_guess_runs_after_the_take_is_sealed() {
        let stub = Stub::new();
        let mut s = StreamDecoder::start(9, guessing(), stub.clone(), None);
        s.seal(0);
        s.push(&tone(2.0), 1.0);
        assert!(stub.lens().is_empty());
        let _ = s.finish();
    }

    #[test]
    fn the_shipped_config_guesses_and_the_unit_config_does_not() {
        let t = StreamConfig::default().tentative.expect("on by default");
        assert_eq!(t.max_duty, 0.10);
        assert!(t.interval <= Duration::from_millis(1000));
        assert!(small().tentative.is_none());
    }
}
