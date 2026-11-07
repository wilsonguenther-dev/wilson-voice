//! In-process microphone capture via cpal (cpal 0.18).
//!
//! Recording inside the Wilson Voice process (not external ffmpeg) so macOS TCC
//! attributes Microphone permission to com.wilsonguenther.wilson-voice.
//! Live RMS level is exposed for the floating HUD waveform.

use crate::dictation;
use cpal::traits::{DeviceTrait, StreamTrait};
use hound::{WavSpec, WavWriter};
use nnnoiseless::DenoiseState;
use parking_lot::{Condvar, Mutex as PLMutex};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::input_format::{
    self, FormatChangeAction, FormatEventSource, InputFormat, InputFormatWatch, InputObservation,
};
use crate::resample::{resample_decimate, resample_linear, Biquad, StreamResampler};
use crate::vad;

/// Shared peak level 0..=1000 for HUD (updated every audio callback window).
pub type LevelHandle = Arc<AtomicU32>;

/// Every clip is written — and every ASR engine is fed — at 16 kHz mono.
pub const TARGET_RATE: u32 = 16_000;
/// Shortest take worth transcribing: 30 ms at the target rate. Below this it was
/// a stray tap, not speech (this used to be a "wav smaller than 1 kB" check —
/// same threshold, measured on the in-memory buffer now).
const MIN_CLIP_SAMPLES: usize = TARGET_RATE as usize * 30 / 1000;

// ── Y3-F: the declared maximum session length ───────────────────────────────
//
// Before this item there was no maximum take length anywhere in the code:
// `git grep "MAX_TAKE\|max_take\|MAX_RECORD\|session_len" desktop` returned
// nothing functional. That is not generosity. It means the failure mode at some
// unknown length is a timeout or an OOM — a cliff the user discovers by falling
// off it — instead of a number we chose, said out loud, and tested.

/// The longest ONE dictation take may run, in seconds. **Thirty minutes.**
///
/// ### Why thirty, and not Wispr's twenty
///
/// Wispr declares 20 minutes (raised from 5). Twenty is not copied here,
/// because the point of a ceiling is that it is *measured*, and our own
/// measurement supports more than twenty:
///
/// * **Capture is flat in the length of the take.** Y3-A (merged, `2ef8f0c`)
///   moved the take's audio to a disk spill, and
///   `tests/dictation_capture_memory.rs` measures a synthetic **twenty-minute**
///   take holding under [`MAX_RESIDENT_CAPTURE_BYTES`] (4 MiB) resident — down
///   from ~153 MB across two unbounded `Vec`s on the pre-Y3 shape. Nothing in
///   that path grows with duration, so twenty minutes was a fixture length, not
///   a limit.
/// * **The binding cost is the ONE read-back at the end.** `finalize_take`
///   reads the spill back as 16 kHz mono `f32`, which is exactly
///   `TARGET_RATE * 4` = **64 KB per second of take**, deterministic and
///   resident once. At this ceiling that is `1800 * 64 KB` ≈ **115 MB**, held
///   for the length of one finalize.
/// * `tests/max_session.rs::the_ceiling_is_a_measurement_not_a_guess` re-runs
///   Y3-A's probe at **this** number rather than at twenty minutes, so the
///   declared ceiling and the measured ceiling can never drift apart: raising
///   `MAX_SESSION_SECONDS` re-measures capture at the new length or fails.
///
/// Thirty is therefore the larger number our own evidence supports, declared
/// because it is declared and tested — not because it beats a competitor. It is
/// deliberately NOT five: Wilson dictates long, and a low ceiling is the same
/// complaint in a new costume.
pub const MAX_SESSION_SECONDS: u64 = 30 * 60;

/// [`MAX_SESSION_SECONDS`] as a `Duration` — the form the watchdog compares.
pub const MAX_SESSION: Duration = Duration::from_secs(MAX_SESSION_SECONDS);

/// How far into the ceiling the one calm warning fires, in percent.
///
/// A percentage rather than a second count, so the warning cannot drift away
/// from the ceiling when the ceiling moves. At 80% of thirty minutes the user
/// gets six minutes of notice.
pub const SESSION_WARN_PERCENT: u64 = 80;

/// When the warning fires — **derived** from [`MAX_SESSION_SECONDS`] and
/// [`SESSION_WARN_PERCENT`], never hand-written.
pub const SESSION_WARN_AT: Duration =
    Duration::from_secs(MAX_SESSION_SECONDS * SESSION_WARN_PERCENT / 100);

/// How far back from the ceiling the cut may look for a silence to land on.
///
/// "A couple of seconds": long enough to contain a whole word plus the gap
/// after it at any speaking rate, short enough that what a capped take loses is
/// a trailing fragment rather than a sentence.
pub const CUT_GRACE: Duration = Duration::from_secs(2);

/// What the ceiling watchdog decides on one tick.
///
/// Same shape as `meeting::watchdog_tick`'s [`crate::meeting::WatchdogAction`]
/// on purpose — a pure function of elapsed time and a latch, so the rule is
/// testable without a microphone, an `AppState` or a running app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCapAction {
    /// Nothing to do; keep recording.
    Continue,
    /// Cross [`SESSION_WARN_AT`] for the first time — say so, once.
    Warn,
    /// At or past [`MAX_SESSION`] — stop cleanly and transcribe what we have.
    Stop,
}

/// The ceiling rule. `warned` is the caller's latch: the watchdog ticks many
/// times a second, and a rule without the latch would repeat the warning for
/// the whole last fifth of the take.
///
/// `Stop` outranks `Warn`: a take that somehow reached the ceiling without ever
/// being warned still stops rather than spending a tick on the notice.
pub fn session_cap_tick(elapsed: Duration, warned: bool) -> SessionCapAction {
    if elapsed >= MAX_SESSION {
        SessionCapAction::Stop
    } else if elapsed >= SESSION_WARN_AT && !warned {
        SessionCapAction::Warn
    } else {
        SessionCapAction::Continue
    }
}

/// The one calm line the pill shows when the warning fires.
///
/// Deliberately a plain status string on the SAME `status` event the pill
/// already renders (`lib::status_message` → `emit_status`) — no toast system,
/// no second channel, no new window. It states the ceiling and what happens at
/// it, because a warning that does not say what is coming is just an alarm.
pub fn session_cap_warning() -> String {
    format!(
        "{} minutes in — Yap stops and transcribes at {}.",
        SESSION_WARN_AT.as_secs() / 60,
        human_minutes(MAX_SESSION)
    )
}

/// `30:00`-style minutes:seconds, for the warning copy.
fn human_minutes(d: Duration) -> String {
    format!("{}:{:02}", d.as_secs() / 60, d.as_secs() % 60)
}

/// The ceiling in samples at [`TARGET_RATE`].
pub fn max_session_samples() -> usize {
    MAX_SESSION_SECONDS as usize * TARGET_RATE as usize
}

/// Where to cut a take that reached the ceiling.
///
/// Returns an index into `samples`. The contract, in order:
///
/// 1. A take at or under `ceiling` is never cut — the returned index is
///    `samples.len()`. Nothing is discarded for being long enough to fit.
/// 2. Otherwise the cut lands at or before `ceiling`, and PREFERS the start of
///    the last silence beginning inside `[ceiling - grace, ceiling]`, so the
///    take ends between words rather than through one. `vad::`-grade frame
///    energy is what decides "silence": this reuses the same bridged 20 ms
///    voiced mask [`pause_spans`] and `voiced_seconds` already compute on every
///    take ([`voiced_frames`]), rather than adding a second opinion about what
///    speech is.
/// 3. With no silence in the grace window — the user was mid-sentence at the
///    ceiling, which is the whole reason the grace window is small — the cut is
///    `ceiling` exactly. Truncating a trailing fragment is the honest outcome;
///    discarding the take is not, and never happens.
pub fn cap_cut_index(samples: &[f32], sample_rate: u32, ceiling: usize, grace: Duration) -> usize {
    if samples.len() <= ceiling {
        return samples.len();
    }
    let grace_samples = (grace.as_secs_f64() * sample_rate as f64) as usize;
    let window_start = ceiling.saturating_sub(grace_samples);
    let Some((mask, frame_secs)) = voiced_frames(&samples[..ceiling], sample_rate) else {
        return ceiling;
    };
    let frame = (frame_secs * sample_rate as f64).round() as usize;
    if frame == 0 {
        return ceiling;
    }
    // Walk back from the ceiling for the START of the last silent run — the
    // moment the speaker stopped — and cut there.
    let mut best: Option<usize> = None;
    let mut run_start: Option<usize> = None;
    for (i, &voiced) in mask.iter().enumerate() {
        if voiced {
            run_start = None;
        } else {
            let at = i * frame;
            if run_start.is_none() {
                run_start = Some(at);
            }
            if let Some(start) = run_start {
                if start >= window_start && start <= ceiling {
                    best = Some(start);
                }
            }
        }
    }
    best.unwrap_or(ceiling).min(ceiling)
}

/// The take's stop signal (YV38) — what the HUD level thread parks on instead of
/// re-reading a flag every 50 ms. `stop_recording` (and the cancel path through
/// it) flips it once and wakes every waiter, so a waiter learns the hold ended
/// within microseconds of key-release rather than up to a poll interval later.
/// `wait_stopped` still takes a tick because the one waiter has periodic work of
/// its own (the ~20 fps level emit) — the tick is that waiter's own cadence, it
/// is never a poll of the stop state.
#[derive(Default)]
pub struct StopSignal {
    stopped: PLMutex<bool>,
    woken: Condvar,
}

impl StopSignal {
    /// Wake every waiter — the take is over. Idempotent.
    fn stop(&self) {
        *self.stopped.lock() = true;
        self.woken.notify_all();
    }

    /// Park until the take stops or `tick` elapses, whichever comes first.
    /// `true` = the take has stopped and the caller should wind down.
    pub fn wait_stopped(&self, tick: Duration) -> bool {
        let mut stopped = self.stopped.lock();
        if *stopped {
            return true;
        }
        self.woken.wait_for(&mut stopped, tick);
        *stopped
    }

    /// Has the take already stopped? Never parks — the signal assertions read it.
    #[cfg(test)]
    fn is_stopped(&self) -> bool {
        *self.stopped.lock()
    }
}

pub struct ActiveRecording {
    stop: Arc<StopSignal>,
    wav_path: PathBuf,
    /// DB-B — the recovery dir (the journal dir), where the finished clip is
    /// also linked while it decodes; see [`ClipWav::spawn`].
    recovery_dir: PathBuf,
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
    /// Shared stop signal — waiters park on it and are woken when the take ends.
    pub fn stop_signal(&self) -> Arc<StopSignal> {
        self.stop.clone()
    }

    /// When this take started capturing — the clock the Y3-F ceiling watchdog
    /// measures against. An `Instant`, not a wall-clock time, so a system clock
    /// change mid-take can never move the ceiling.
    pub fn started_at(&self) -> Instant {
        self.started
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
    /// The silences the speaker left, as shares of this take's voiced time
    /// (Y4-C). The rules stage paragraphs on these — see
    /// [`dictation::insert_paragraphs`]. Empty when the clip has no measurable
    /// speech, which is exactly the "no timing" case the rule falls back from.
    pub pause_spans: Vec<dictation::PauseSpan>,
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
    /// YV67 — the capture device errored DURING this take (unplugged mic, format
    /// change). The take still comes back with every sample that made it, because
    /// a truncated recording is the user's audio; the pipeline turns this into a
    /// retryable failure instead of pasting a silently cut-off transcript. Until
    /// YV67 the flag was only read on the NEXT arm, so the take it truncated
    /// looked perfectly healthy.
    pub device_failed: bool,
}

/// Y3 test seam — drive the dictation capture consumer with synthetic frames,
/// with no microphone, no capture worker and no Tauri app, so
/// `tests/dictation_capture_memory.rs` can measure what a real take holds.
///
/// `#[doc(hidden)]`: this is a harness, not product API. It exposes exactly two
/// things — push frames in, ask how many bytes are resident — plus the take
/// hand-off, which is what makes the byte-identity assertion possible.
#[doc(hidden)]
pub mod capture_probe {
    use super::{CapturedAudio, SpillWriter, StreamDsp};
    use std::path::Path;

    /// One take's capture consumer, spilling to `recordings_dir` and moving an
    /// aborted spill to `recovery_dir`. `None` for `recordings_dir` builds the
    /// pre-Y3 unbounded-RAM path, which is what the byte-identity test compares
    /// against.
    pub struct CaptureProbe {
        dsp: StreamDsp,
    }

    impl CaptureProbe {
        pub fn new(
            sample_rate: u32,
            channels: u16,
            spill_wav: Option<(&Path, &Path)>,
        ) -> Result<Self, String> {
            let mut dsp = StreamDsp::new(sample_rate, channels);
            if let Some((wav_path, recovery_dir)) = spill_wav {
                dsp.spill = Some(
                    SpillWriter::open(wav_path, recovery_dir)
                        .ok_or_else(|| "capture spill would not open".to_string())?,
                );
            }
            Ok(Self { dsp })
        }

        /// One capture callback's interleaved native-rate frames.
        pub fn push(&mut self, interleaved: &[f32]) {
            self.dsp.push(interleaved);
        }

        /// Bytes of audio held in RAM right now (see `StreamDsp::resident_bytes`).
        pub fn resident_bytes(&self) -> usize {
            self.dsp.resident_bytes()
        }

        /// File name of this take's spill, so a test can look for it in the
        /// recovery dir after an abort.
        pub fn spill_file_name(&self) -> Option<String> {
            self.dsp
                .spill
                .as_ref()
                .and_then(|s| s.path.file_name())
                .map(|n| n.to_string_lossy().into_owned())
        }

        /// Abandon the take WITHOUT closing it — the mid-take abort. The spill
        /// is finalized as a playable prefix and moved to the recovery dir.
        pub fn abort(self) {
            drop(self.dsp);
        }

        /// Close the take and run it through the real finalize chain.
        pub fn finish(mut self, denoise: bool) -> Result<Vec<f32>, String> {
            let captured: CapturedAudio = self.dsp.take();
            super::finalize_take(captured, denoise)
        }
    }
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
///
/// `journal_dir` is `data_dir()/recovery/` — the take's crash journal (YV63) is
/// opened there before the stream is armed, so the frames start spilling to disk
/// with the first callback. A journal that cannot be opened is simply `None`:
/// nothing about capture depends on it.
pub fn start_recording(
    dir: PathBuf,
    journal_dir: &Path,
    denoise: bool,
    pressed_at: Option<Instant>,
    stream_vad: Option<Arc<vad::WarmVad>>,
) -> Result<ActiveRecording, String> {
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let wav_path = dir.join(format!("{}.wav", Uuid::new_v4()));
    // The hold clock starts at the request, NOT after the handshake, so
    // `hold_wall_seconds` reports the real press→release wall time.
    let started = Instant::now();
    let journal = CaptureJournal::start(journal_dir);
    // Y3 — the take's audio goes straight to `<id>.capture.wav` next to the
    // history wav as it is captured, so a long dictation costs a bounded window
    // of RAM instead of growing two Vecs for as long as the key is held.
    let spill = SpillWriter::open(&wav_path, journal_dir).map(Box::new);
    // SP-06 — with denoise on, the take's release DSP runs WHILE it is
    // captured: RNNoise frame by frame, and Silero on each frame through the
    // warm second session (when one is loaded and free).
    let analysis = denoise.then(|| {
        let vad = stream_vad.as_ref().and_then(vad::WarmVad::begin_stream);
        log::info!("record stream_dsp_armed vad={}", vad.is_some());
        Box::new(TakeAnalysis::new(true, vad))
    });

    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    dispatch(CaptureCmd::Arm {
        journal,
        spill,
        analysis,
        reply: reply_tx,
    })?;
    await_reply(&reply_rx, ARM_TIMEOUT, "arm")?;

    let capture_start_ms = pressed_at.unwrap_or(started).elapsed().as_millis() as i64;
    log::info!(
        "capture armed: press→capture_start={capture_start_ms}ms (arm_wait={}ms)",
        started.elapsed().as_millis()
    );

    Ok(ActiveRecording {
        stop: Arc::new(StopSignal::default()),
        wav_path,
        recovery_dir: journal_dir.to_path_buf(),
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
    finish_recording(end_capture(active)?, isolation_vad)
}

/// yap-ptt-capture-tail (D4) — the longest a take may keep capturing after its
/// stop edge. There is NO trailing pad: capture ends at the `Disarm`, one
/// channel round-trip to the capture worker, so the real tail is a thread
/// spawn plus the device callback buffer in flight (~10 ms at 48 kHz).
#[allow(dead_code)] // the D4 contract; held by capture_ends_within_budget_of_the_stop_edge_while_context_reads_stall
pub const STOP_EDGE_BUDGET: Duration = Duration::from_millis(300);

/// A take whose capture has ended (`end_capture`) but whose DSP has not run
/// (`finish_recording`).
pub struct EndedCapture {
    active: ActiveRecording,
    hold_wall_seconds: f64,
    captured: CapturedAudio,
}

/// The capture half of a stop: wake the waiters, stop buffering, take the raw
/// capture off the persistent worker (the stream itself stays open for the next
/// take — see `capture_worker_loop`). Bounded by `DISARM_TIMEOUT`.
pub fn end_capture(active: ActiveRecording) -> Result<EndedCapture, String> {
    let hold_wall_seconds = active.started.elapsed().as_secs_f64().max(0.01);
    // YV38: signal (not a flag another thread has to notice) — the HUD level
    // thread is woken here, on release, instead of up to 50 ms afterwards.
    active.stop.stop();
    active.level.store(0, Ordering::Relaxed);
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    dispatch(CaptureCmd::Disarm { reply: reply_tx })?;
    let captured = await_reply(&reply_rx, DISARM_TIMEOUT, "stop")?;
    Ok(EndedCapture {
        active,
        hold_wall_seconds,
        captured,
    })
}

/// yap-ptt-capture-tail (D4) — the stop edge's ordering, in one place: the
/// capture is ended FIRST, on its own thread, and only then does
/// `read_context` run (frontmost app, bundle id, the AX text before the caret).
/// Those reads go to the user's app over Accessibility with the system's
/// default messaging timeout (6 s); in the 2026-10-19 walk a busy frontmost app
/// stalled them ~3.3 s, and because the `Disarm` ran only after them, a 1.5 s
/// hold came back as 4.2 s of audio (`capture_ms=4226`) and the ambient speech
/// after release was transcribed and pasted.
pub fn end_at_stop_edge<T, C>(
    end: impl FnOnce() -> T + Send + 'static,
    read_context: impl FnOnce() -> C,
) -> (thread::JoinHandle<T>, C)
where
    T: Send + 'static,
{
    let capture = thread::Builder::new()
        .name("wv-capture-stop".into())
        .spawn(end)
        .expect("spawn the capture-stop thread");
    (capture, read_context())
}

/// The DSP half of a stop: finalize, cap, VAD, isolate, spawn the wav write.
pub fn finish_recording(
    ended: EndedCapture,
    isolation_vad: Option<&vad::WarmVad>,
) -> Result<RecordingResult, String> {
    let EndedCapture {
        active,
        hold_wall_seconds,
        mut captured,
    } = ended;
    // YV63: the take is off the capture worker, so its crash journal comes back
    // with it. Held here (not passed on to the DSP) so EVERY exit from this
    // function retires it — the early return below drops it, which cleans the
    // marker + spill exactly like the normal completion at the end does.
    let journal = captured.journal.take();
    // YV67: the device's health at Disarm belongs to THIS take — read it off
    // before `finalize_take` consumes the capture.
    let device_failed = captured.device_failed;
    // SP-06 — with denoise on, the edge fade, RNNoise and Silero already ran
    // over the capture frame by frame; what is left is their tails and the
    // gain. Anything that does not check out falls to the batch chain below.
    let t_release = Instant::now();
    let gain = captured.gain;
    let (mut samples, mut streamed_vad) = match captured.analysis.take().map(|a| (*a).finish(gain))
    {
        Some(Ok(done)) if !done.out.is_empty() => {
            let samples = if gain == 1.0 {
                done.out
            } else {
                apply_gain(&done.out, gain)
            };
            log::info!(
                "record stream_release ok=true ms={} samples={} vad_frames={}",
                t_release.elapsed().as_millis(),
                samples.len(),
                done.vad.as_ref().map_or(0, vad::StreamIsolation::frames)
            );
            (samples, done.vad)
        }
        other => {
            if let Some(Err(e)) = &other {
                log::warn!("record stream_release ok=false reason=\"{e}\" -> batch chain");
            }
            drop(other);
            // Everything that CAN stream already ran on the capture frames
            // (YV37); this only finishes the take — apply the accumulated AGC
            // gain, de-click the edges, optionally denoise — and yields the
            // 16 kHz mono buffer ASR reads.
            (finalize_take(captured, active.denoise)?, None)
        }
    };

    // Y3-F — the declared ceiling is enforced on the AUDIO, not only on the
    // watchdog that normally trips first. The watchdog stops the take within one
    // HUD frame of `MAX_SESSION`, so the overshoot here is milliseconds on the
    // normal path; this is what makes the ceiling true for every path, including
    // a take stopped by something that never consulted the clock.
    let ceiling = max_session_samples();
    if samples.len() > ceiling {
        let cut = cap_cut_index(&samples, TARGET_RATE, ceiling, CUT_GRACE);
        log::info!(
            "session ceiling: take ran to {:.1}s, cut at {:.1}s (ceiling {}s)",
            samples.len() as f64 / TARGET_RATE as f64,
            cut as f64 / TARGET_RATE as f64,
            MAX_SESSION_SECONDS
        );
        samples.truncate(cut);
    }

    if samples.len() < MIN_CLIP_SAMPLES {
        // Sub-second tap: no wav was ever written for it (the recovery write is
        // spawned below, after the take is known good), so nothing to clean up.
        return Err(crate::permissions::named(&format!(
            "Audio too short ({} ms) — hold longer, or allow Microphone for Yap.",
            samples.len() * 1000 / TARGET_RATE as usize
        )));
    }
    // WPM denominator = real voiced time. Fall back to the raw clip length only
    // when VAD can't find speech (near-silent capture) so WPM never divides by ~0.
    let clip_seconds = samples.len() as f64 / TARGET_RATE as f64;
    // Both the energy VAD (WPM denominator) and the Silero voice-isolation pass
    // below read the SAME in-memory buffer — no wav read-back (YV37).
    let voiced = voiced_seconds(&samples, TARGET_RATE);
    let speech_seconds = if voiced >= 0.1 { voiced } else { clip_seconds };
    // Y4-C — the pauses, measured on the UNTRIMMED buffer and before the ASR
    // swap below, from the same mask `voiced_seconds` just summed.
    let take_pauses = pause_spans(&samples, TARGET_RATE);

    // YV36 — Silero VAD voice isolation (see `silero_trim`). SP-03: the same
    // function is what `wilson-voice --bench` runs over a file. SP-06: when the
    // take streamed through Silero, the verdict is already made — only the
    // trim's copy is left.
    let (samples, speech_present) = match streamed_vad.take() {
        Some(stream) => streamed_trim(samples, stream, isolation_vad),
        None => silero_trim(samples, isolation_vad),
    };

    // History/recovery wav — spawned AFTER the ASR buffer is final, so the write
    // runs alongside transcription + paste instead of sitting between key-release
    // and ASR (YV37). The guard unlinks it when the take is done.
    //
    // DB-B: the same writer also links the finished wav into the recovery dir
    // as `<id>.decoding.wav` for as long as the take is in flight. That link is
    // what a SIGKILL, a crash or the tray's `app.exit(0)` mid-decode leaves
    // behind (none of them run `Drop`), in a directory `sweep_stale_wavs` never
    // touches, so the next launch's `recover_dictation` adopts it.
    let clip = ClipWav::spawn(active.wav_path, samples.clone(), Some(&active.recovery_dir));
    // YV63 normal completion: the take survived capture and its wav is being
    // written, so the spill is no longer the only copy of these words — the
    // in-progress marker goes with it, and startup finds no orphan to recover.
    if let Some(journal) = journal {
        journal.finish();
    }

    Ok(RecordingResult {
        samples,
        clip,
        speech_seconds: speech_seconds.max(0.05),
        // The TRUE voiced value — no clip-length fallback, so a silent tap reads
        // 0.0 and the no-speech gate can reject it before ASR.
        voiced_seconds: voiced,
        pause_spans: take_pauses,
        hold_wall_seconds,
        speech_present,
        capture_start_ms: active.capture_start_ms,
        device_failed,
    })
}

/// YV36 — Silero VAD voice isolation through the WARM engine loaded once at
/// startup (`vad::WarmVad`, held in the app state). `None` means no model on
/// disk / the load failed, and ANY inference failure falls back to the
/// energy-VAD path with `speech_present = true` so an utterance is NEVER lost.
/// Returns the ASR buffer (trimmed to the voiced span when Silero found speech)
/// and whether speech is present.
fn silero_trim(samples: Vec<f32>, isolation_vad: Option<&vad::WarmVad>) -> (Vec<f32>, bool) {
    if let Some(warm) = isolation_vad {
        match warm.isolate(&samples, TARGET_RATE) {
            Ok(iso) => return apply_isolation(samples, iso, false),
            Err(e) => {
                // Safe fallback — energy VAD alone decides the gate.
                log::warn!("YV36 silero unavailable, using energy VAD: {e}");
            }
        }
    }
    (samples, true)
}

/// SP-06 — the streamed take's Silero verdict over its final buffer. A stream
/// that cannot vouch for the buffer (inference failed mid-take, or the session
/// ceiling cut the clip after capture) hands over to the batch pass.
fn streamed_trim(
    samples: Vec<f32>,
    stream: vad::StreamIsolation<'_>,
    isolation_vad: Option<&vad::WarmVad>,
) -> (Vec<f32>, bool) {
    let frames = stream.frames();
    match stream.finish(&samples, TARGET_RATE) {
        Ok(iso) => apply_isolation(samples, iso, true),
        Err(e) => {
            log::warn!("record stream_vad ok=false frames={frames} reason=\"{e}\" -> batch silero");
            silero_trim(samples, isolation_vad)
        }
    }
}

/// Apply one isolation verdict (batch or streamed) to the ASR buffer.
fn apply_isolation(mut samples: Vec<f32>, iso: vad::Isolation, streamed: bool) -> (Vec<f32>, bool) {
    let speech_present = iso.has_speech;
    // (b) Trim the ASR buffer to the voiced span so Whisper decodes only the
    // speech (never trim to empty — if Silero found speech we keep its padded
    // span). Since YV37 this is a plain in-memory swap instead of a wav
    // rewrite. We do NOT touch `voiced`/`speech_seconds`: WPM stays anchored to
    // the energy VAD over the full utterance.
    let did_trim = iso.trimmed.is_some();
    if iso.has_speech {
        if let Some(trimmed) = iso.trimmed {
            if !trimmed.is_empty() {
                samples = trimmed;
            }
        }
    }
    log::info!(
        "YV36 silero: has_speech={} voiced={:.2}s trimmed={} streamed={streamed}",
        speech_present,
        iso.voiced_seconds,
        did_trim
    );
    (samples, speech_present)
}

/// SP-03 — `wilson-voice --bench`: the leveling a live take gets WHILE it is
/// being captured (the streamed high-pass + AGC, YV37), done over a buffer read
/// from a file. A file never streamed, so this is the batch chain the live path
/// itself falls back to (`fallback_chain`: high-pass + RMS normalize at 16 kHz).
/// It runs BEFORE the bench's release clock starts, exactly as capture-time DSP
/// is off the release path in the app.
pub(crate) fn bench_capture_chain(raw: &[f32]) -> Vec<f32> {
    fallback_chain(raw, TARGET_RATE)
}

/// SP-03 — one bench take after the release-side DSP + VAD.
pub(crate) struct BenchDsp {
    /// The buffer ASR decodes (trimmed to the voiced span when Silero ran).
    pub samples: Vec<f32>,
    /// Energy-VAD voiced seconds over the untrimmed buffer (the WPM denominator).
    pub voiced_seconds: f64,
    /// `false` only when Silero ran and heard no speech.
    pub speech_present: bool,
}

/// SP-03 — the release-side half of `finish_recording`, in its order, over a
/// leveled buffer: apply the (unit) gain, de-click the edges, optionally
/// denoise, measure voiced time and pauses, then the Silero trim. The session
/// ceiling and the history wav are left out — a bench file is not a take to
/// keep, and the ceiling only cuts audio longer than any bench fixture.
pub(crate) fn bench_release_dsp(
    leveled: &[f32],
    denoise: bool,
    isolation_vad: Option<&vad::WarmVad>,
) -> Result<BenchDsp, String> {
    if leveled.is_empty() {
        return Err(NO_AUDIO_ERR.into());
    }
    let gained = apply_gain(leveled, 1.0);
    let faded = edge_fade(&gained, TARGET_RATE, EDGE_FADE_MS);
    let out = if denoise {
        denoise_rnnoise(&faded, TARGET_RATE)
    } else {
        faded
    };
    if out.is_empty() {
        return Err(NO_AUDIO_ERR.into());
    }
    let voiced = voiced_seconds(&out, TARGET_RATE);
    // Same work the live take does on release; the bench only pays for it.
    let _pauses = pause_spans(&out, TARGET_RATE);
    let (samples, speech_present) = silero_trim(out, isolation_vad);
    Ok(BenchDsp {
        samples,
        voiced_seconds: voiced,
        speech_present,
    })
}

/// SP-06 — what a capture callback hands the DSP at 16 kHz (~10.6 ms), the
/// chunk the bench streams a file in.
const BENCH_CALLBACK_SAMPLES: usize = 170;

/// SP-06 — the bench's capture half of the streamed release DSP: feed a
/// leveled file through [`TakeAnalysis`] in callback-sized chunks, exactly as
/// the capture worker does while the key is held. Runs BEFORE the bench's
/// release clock. `None` when nothing would stream (denoise off, no VAD).
pub(crate) fn bench_capture_stream<'a>(
    leveled: &[f32],
    denoise: bool,
    isolation_vad: Option<&'a vad::WarmVad>,
) -> Option<TakeAnalysis<'a>> {
    let vad = isolation_vad.and_then(vad::WarmVad::begin_stream_local);
    if !denoise && vad.is_none() {
        return None;
    }
    let mut analysis = TakeAnalysis::new(denoise, vad);
    for chunk in leveled.chunks(BENCH_CALLBACK_SAMPLES) {
        analysis.push(chunk, 1.0);
    }
    Some(analysis)
}

/// SP-06 — [`bench_release_dsp`] for a take that streamed: only the tails of
/// the fade, RNNoise and Silero are left for the release clock. Byte-identical
/// to the batch function on the same input (test); any stream that cannot
/// vouch for its output runs the batch function instead.
pub(crate) fn bench_release_streamed(
    leveled: &[f32],
    denoise: bool,
    isolation_vad: Option<&vad::WarmVad>,
    streamed: Option<TakeAnalysis<'_>>,
) -> Result<BenchDsp, String> {
    let Some(analysis) = streamed else {
        return bench_release_dsp(leveled, denoise, isolation_vad);
    };
    let done = match analysis.finish(1.0) {
        Ok(done) if !done.out.is_empty() => done,
        Ok(_) => return Err(NO_AUDIO_ERR.into()),
        Err(e) => {
            log::warn!("bench stream_release ok=false reason=\"{e}\" -> batch chain");
            return bench_release_dsp(leveled, denoise, isolation_vad);
        }
    };
    let StreamedTake { out, vad } = done;
    // The energy VAD is unchanged: whole-buffer, over the untrimmed take.
    let voiced = voiced_seconds(&out, TARGET_RATE);
    let _pauses = pause_spans(&out, TARGET_RATE);
    let (samples, speech_present) = match vad {
        Some(stream) => streamed_trim(out, stream, isolation_vad),
        None => silero_trim(out, isolation_vad),
    };
    Ok(BenchDsp {
        samples,
        voiced_seconds: voiced,
        speech_present,
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
///
/// YV52 is the ONE exception: a take whose transcription failed hands its clip
/// to `keep_for_recovery`, which moves the wav aside so the user can retry ASR
/// on the same audio instead of re-speaking it.
pub struct ClipWav {
    path: PathBuf,
    writer: Option<thread::JoinHandle<()>>,
    /// Set by `keep_for_recovery` — the file has been moved out from under this
    /// guard, so `Drop` must not try to unlink it.
    kept: bool,
    /// DB-B — `<recovery>/<id>.decoding.wav`, a hard link to the finished clip
    /// that exists exactly while the take is in flight. Every ORDERLY end of
    /// the take removes it (`Drop`, `keep_for_recovery`); only a death that
    /// skips both — kill -9, a crash, `app.exit` — leaves it for the next
    /// launch to adopt. `None` for a guard with no recovery dir.
    in_flight: Option<PathBuf>,
}

/// DB-B — the suffix of an in-flight clip's recovery link. Read back by
/// [`recover_orphaned_spills`] alongside Y3-A's `.capture.wav`.
const IN_FLIGHT_SUFFIX: &str = ".decoding.wav";

/// Link `target` into `link` without ever leaving a partial file under the
/// final name: a hard link is atomic; the copy fallback (another volume) goes
/// through a temp name and a rename.
fn link_in_flight(target: &Path, link: &Path) -> Result<(), String> {
    if let Some(dir) = link.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(link);
    if std::fs::hard_link(target, link).is_ok() {
        return Ok(());
    }
    let tmp = link.with_extension("wav.partial");
    let copied = std::fs::copy(target, &tmp)
        .map_err(|e| e.to_string())
        .and_then(|_| std::fs::rename(&tmp, link).map_err(|e| e.to_string()));
    if copied.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    copied
}

impl ClipWav {
    /// Start writing the take's clip in the background. With `recovery`, the
    /// finished wav is also linked there as `<id>.decoding.wav` (DB-B) so a
    /// take that dies mid-decode leaves its audio somewhere the next launch
    /// looks. Public so `tests/long_take_recovery.rs` can drive the SAME
    /// writer a real take uses, then kill its own process for real.
    pub fn spawn(path: PathBuf, samples: Vec<f32>, recovery: Option<&Path>) -> Self {
        let in_flight = recovery.and_then(|dir| {
            let stem = path.file_stem()?.to_string_lossy().to_string();
            Some(dir.join(format!("{stem}{IN_FLIGHT_SUFFIX}")))
        });
        let target = path.clone();
        let link = in_flight.clone();
        let writer = thread::Builder::new()
            .name("wv-clip-wav".into())
            .spawn(
                move || match write_wav_i16(&target, TARGET_RATE, &samples) {
                    Ok(()) => {
                        log::info!("wrote {} samples → {}", samples.len(), target.display());
                        if let Some(link) = link {
                            if let Err(e) = link_in_flight(&target, &link) {
                                // Degrades to the pre-DB-B behaviour: a kill
                                // mid-decode loses this take; nothing else does.
                                log::warn!("DB-B in-flight recovery link failed: {e}");
                            }
                        }
                    }
                    // Non-fatal by design: the transcript comes from memory now, so a
                    // failed history write can never cost the user their words.
                    Err(e) => log::warn!("history wav write failed (transcript unaffected): {e}"),
                },
            )
            .ok();
        Self {
            path,
            writer,
            kept: false,
            in_flight,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// DB-B — the take's id: the clip's file stem, which is also the stem of
    /// its in-flight recovery link, so the `take_chunks` a decode persists under
    /// it come back with the audio after a crash.
    pub fn take_id(&self) -> Option<String> {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
    }

    /// Block until the background write (and its in-flight link) is done.
    /// Tests use this to reach the state a real take is in mid-decode.
    pub fn wait_written(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }

    /// A guard over a wav that is ALREADY on disk, so the recovery path
    /// (YV52/YV67/Y3-D) can be driven without a capture device. Same drop
    /// semantics as a real clip — it unlinks unless `keep_for_recovery` ran.
    ///
    /// Y3-D removed the `#[cfg(test)]` gate. `tests/cancel_long_take.rs` is an
    /// INTEGRATION test: it links the library the way the app does, without
    /// `cfg(test)`, so a unit-test-only seam is invisible to it. The choice was
    /// between exposing this constructor and writing a second, parallel
    /// implementation of "park a clip in the recovery dir" that the test could
    /// reach — and a parallel implementation is a test that stops describing
    /// the code the moment the two drift. This way the cancel test drives the
    /// SAME `keep_for_recovery` the app calls.
    pub fn adopt_existing(path: PathBuf) -> Self {
        Self {
            path,
            writer: None,
            kept: false,
            in_flight: None,
        }
    }

    /// YV52 — preserve this take's audio for a retry instead of unlinking it.
    ///
    /// The background write is joined first (so the wav is complete), then the
    /// file is MOVED into `dir` — deliberately not the recordings dir, which
    /// `sweep_stale_wavs` empties at every startup and would destroy exactly the
    /// clips a recovery needs. Returns the new path. On any failure the guard is
    /// left armed, so a clip that could not be preserved is still unlinked.
    pub fn keep_for_recovery(&mut self, dir: &Path) -> Result<PathBuf, String> {
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let name = self
            .path
            .file_name()
            .ok_or_else(|| format!("clip has no file name: {}", self.path.display()))?;
        let target = dir.join(name);
        std::fs::rename(&self.path, &target).map_err(|e| e.to_string())?;
        self.kept = true;
        // DB-B — the take now has its own row; retire the in-flight link AFTER
        // the move, so there is no instant at which neither copy exists.
        if let Some(link) = self.in_flight.take() {
            let _ = std::fs::remove_file(link);
        }
        Ok(target)
    }
}

impl Drop for ClipWav {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        // DB-B — an orderly end of the take: it is no longer in flight.
        if let Some(link) = self.in_flight.take() {
            let _ = std::fs::remove_file(link);
        }
        if self.kept {
            return;
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

// ── Crash-safe capture journal (YV63) ───────────────────────────────────────
// Everything above only writes a wav AFTER the take ends, so a take the app dies
// in the middle of used to leave NOTHING behind — a long dictation lost to one
// glitch, with no way to get the words back. The journal fixes that class: the
// 16 kHz frames are spilled to `data_dir()/recovery/` as they arrive, next to an
// in-progress marker naming the spill and the moment capture started. A normal
// stop removes both (see `stop_recording`); anything left at startup means the
// app died mid-take, and `recover_orphaned_journals` finalizes it into a real
// wav that gets the YV52 failed-dictation treatment — a Retry row in History.
//
// The one hard rule: the journal is ADDITIVE. It runs on its own writer thread
// behind a bounded queue, so a slow disk can never stall the audio callback, and
// every failure — open, marker write, spawn, queue full, write error — degrades
// to exactly the behaviour that existed before it. Frames are never dropped for
// it; journal writes are.

/// Marker file suffix: `<id>.in_progress.json`, written at capture start and
/// removed on normal completion. Its presence at startup IS the crash signal.
const JOURNAL_MARKER_EXT: &str = "in_progress.json";
/// Spill file suffix: `<id>.spill.pcm` — raw little-endian i16 mono at
/// `TARGET_RATE`, appended frame by frame (no container, so a truncated file is
/// still a valid prefix of the take).
const JOURNAL_SPILL_EXT: &str = "spill.pcm";
/// How many capture frames may be in flight to the journal writer. Deep enough
/// to ride out a disk hiccup of tens of callbacks, bounded so a wedged disk can
/// never grow memory without limit — past it, journal writes are dropped.
const JOURNAL_QUEUE_DEPTH: usize = 64;

/// One thing for the journal writer to put on disk. EVERYTHING the journal
/// records travels as one of these, over the one bounded queue, because the
/// queue is what keeps the disk off the capture path — a second kind of record
/// that "only happens a few times a session" is exactly how a blocking `open` +
/// `write` gets back in (YV92 review).
enum JournalWrite {
    /// 16 kHz frames, already converted to i16 by the capture path.
    Frames(Vec<i16>),
    /// YV92 — one `device_change` JSON line for the marker sidecar.
    Marker(String),
}

/// One take's spill queue: the bounded hand-off from the capture path to the
/// journal writer. Split out from `CaptureJournal` so the never-block rule is
/// unit-testable without a writer thread or a disk.
struct JournalQueue {
    tx: mpsc::SyncSender<JournalWrite>,
    dropped: AtomicU64,
}

impl JournalQueue {
    fn new(tx: mpsc::SyncSender<JournalWrite>) -> Self {
        Self {
            tx,
            dropped: AtomicU64::new(0),
        }
    }

    /// Hand a write to the writer if it has room. NEVER blocks and never errors:
    /// a full queue (writer behind) or a dead writer counts the write as dropped
    /// and returns immediately — the audio callback pays one `try_send`.
    fn offer(&self, write: JournalWrite) {
        if self.tx.try_send(write).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// The in-progress journal for ONE take. Created at capture start, owned by the
/// capture DSP while the user speaks, handed back with the take and retired by
/// `stop_recording`. Dropping it retires it too, so no exit path can leak a
/// marker that would make the next startup "recover" a take that never crashed.
pub struct CaptureJournal {
    marker: PathBuf,
    spill: PathBuf,
    /// `None` once retired — dropping the sender is what tells the writer to
    /// flush and exit.
    queue: Option<JournalQueue>,
    writer: Option<thread::JoinHandle<()>>,
    /// Set by `abandon` (tests): leave the marker + spill on disk exactly as a
    /// crash mid-take leaves them.
    keep: bool,
}

impl CaptureJournal {
    /// Open a journal for a take in `dir` (`data_dir()/recovery/`). `None` only
    /// if the writer thread cannot be spawned — a take is never blocked, delayed
    /// or failed by journalling.
    ///
    /// The take's paths are decided here, but creating the files (and writing
    /// the marker) is the WRITER's first job, not this caller's: `start` sits on
    /// the press→capture_start path that YV35 exists to keep short, so it must
    /// not pay for a `create` + a `write` on a cold disk.
    pub fn start(dir: &Path) -> Option<Self> {
        Self::start_with_depth(dir, JOURNAL_QUEUE_DEPTH)
    }

    fn start_with_depth(dir: &Path, depth: usize) -> Option<Self> {
        let id = Uuid::new_v4().to_string();
        let spill = dir.join(format!("{id}.{JOURNAL_SPILL_EXT}"));
        let marker = dir.join(format!("{id}.{JOURNAL_MARKER_EXT}"));
        let (tx, rx) = mpsc::sync_channel::<JournalWrite>(depth.max(1));
        let (dir, open_spill, open_marker) = (dir.to_path_buf(), spill.clone(), marker.clone());
        let Ok(writer) = thread::Builder::new()
            .name("wv-capture-journal".into())
            .spawn(move || journal_writer_loop(&dir, &open_spill, &open_marker, rx))
        else {
            log::warn!("YV63 journal off for this take (writer thread)");
            return None;
        };
        Some(Self {
            marker,
            spill,
            queue: Some(JournalQueue::new(tx)),
            writer: Some(writer),
            keep: false,
        })
    }

    /// Spill one capture frame (16 kHz mono floats). Called from the capture
    /// path — allocation + one `try_send`, no lock, no disk, no blocking.
    pub fn append(&self, frames: &[f32]) {
        if frames.is_empty() {
            return;
        }
        if let Some(queue) = self.queue.as_ref() {
            queue.offer(JournalWrite::Frames(
                frames.iter().map(|&s| to_i16(s)).collect(),
            ));
        }
    }

    /// YV92 — record a `device_change` at the point in the spill where the input
    /// format changed. The spill itself stays ONE continuous 16 kHz stream (that
    /// is what makes a truncated file a valid prefix of the take); the segment
    /// boundary is this record, and the output sample index it carries is where
    /// the previous segment ends and the new one begins.
    ///
    /// Handed to the writer thread over the SAME bounded queue the frames use —
    /// allocation + one `try_send`, no lock, no disk, no blocking (YV92 review).
    /// It used to `open` + `writeln!` inline, and its only caller reaches it
    /// through `LiveStream::mark_device_change`, which holds the DSP mutex the
    /// cpal input callback locks every buffer: a real `open()` on a cold or busy
    /// disk therefore parked the audio callback at exactly the device-change
    /// seam this marker exists to timestamp. Whose *thread* writes is not the
    /// question — whose *lock* it holds is.
    ///
    /// Queueing also makes the marker land in FIFO order behind the frames that
    /// preceded it, and every failure is still swallowed: a full queue drops the
    /// marker (counted, logged at retire) rather than costing the take.
    pub fn mark_device_change(&self, marker: &serde_json::Value) {
        if let Some(queue) = self.queue.as_ref() {
            queue.offer(JournalWrite::Marker(marker.to_string()));
        }
    }

    /// `<id>.spill.pcm` → `<id>.spill.markers.jsonl`, i.e. the sidecar lives
    /// next to the audio it annotates and is retired with it.
    fn markers_path(&self) -> PathBuf {
        markers_path_for(&self.spill)
    }

    /// Normal completion — the take made it out of capture, so the journal has
    /// done its job. Joins the writer and removes the marker + spill.
    fn finish(self) {
        // The work is in `Drop`, so every path (including an early error return
        // in `stop_recording`) retires the journal the same way.
    }

    /// Test-only: simulate the app dying mid-take. Flushes what the writer
    /// already has and leaves the marker + spill on disk for the startup scan.
    #[cfg(test)]
    pub fn abandon(mut self) {
        self.keep = true;
        self.retire();
    }

    #[cfg(test)]
    fn marker_path(&self) -> &Path {
        &self.marker
    }

    #[cfg(test)]
    fn spill_path(&self) -> &Path {
        &self.spill
    }

    #[cfg(test)]
    fn dropped_writes(&self) -> u64 {
        self.queue.as_ref().map(JournalQueue::dropped).unwrap_or(0)
    }

    /// Hang up on the writer, wait for it to flush, then clear the on-disk state
    /// unless this journal was deliberately abandoned. Idempotent.
    fn retire(&mut self) {
        let dropped = self.queue.as_ref().map(JournalQueue::dropped).unwrap_or(0);
        drop(self.queue.take());
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        if dropped > 0 {
            log::warn!("YV63 journal fell behind: {dropped} spill write(s) dropped (audio intact)");
        }
        if self.keep {
            return;
        }
        let _ = std::fs::remove_file(&self.marker);
        let _ = std::fs::remove_file(&self.spill);
        // YV92 sidecar — retired with the take it annotates (absent on the
        // overwhelmingly common take where no device change happened).
        let _ = std::fs::remove_file(self.markers_path());
    }
}

impl Drop for CaptureJournal {
    fn drop(&mut self) {
        self.retire();
    }
}

/// The journal writer thread. It opens the take's spill and writes the
/// in-progress marker FIRST — that pair is what a later startup reads as "the
/// app died mid-take" — then appends every chunk the capture path offers and
/// flushes it, so the bytes are on DISK before the crash rather than in our
/// buffer. Any error ends the journal for this take (the audio itself is
/// unaffected — the capture path never learns about it, its offers simply
/// become dropped writes).
fn journal_writer_loop(dir: &Path, spill: &Path, marker: &Path, rx: mpsc::Receiver<JournalWrite>) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!("YV63 journal off for this take (recovery dir): {e}");
        return;
    }
    let file = match std::fs::File::create(spill) {
        Ok(file) => file,
        Err(e) => {
            log::warn!("YV63 journal off for this take (spill): {e}");
            return;
        }
    };
    // The marker is what a later startup reads: when the take began and where
    // its audio went. Written after the spill exists so a marker can never point
    // at a file that was never created.
    let meta = serde_json::json!({
        "version": 1,
        "started_at": chrono::Utc::now().to_rfc3339(),
        "spill": spill.to_string_lossy(),
        "sample_rate": TARGET_RATE,
    });
    if let Err(e) = std::fs::write(marker, meta.to_string()) {
        log::warn!("YV63 journal off for this take (marker): {e}");
        let _ = std::fs::remove_file(spill);
        return;
    }

    let markers = markers_path_for(spill);
    let mut out = std::io::BufWriter::new(file);
    let mut bytes = 0usize;
    let mut buf: Vec<u8> = Vec::new();
    while let Ok(write) = rx.recv() {
        match write {
            JournalWrite::Frames(chunk) => {
                buf.clear();
                buf.reserve(chunk.len() * 2);
                for sample in chunk {
                    buf.extend_from_slice(&sample.to_le_bytes());
                }
                if let Err(e) = out.write_all(&buf).and_then(|()| out.flush()) {
                    log::warn!("YV63 journal write stopped after {bytes} bytes: {e}");
                    return;
                }
                bytes += buf.len();
            }
            // YV92 — the sidecar is opened lazily HERE, on the writer thread,
            // because a device change is rare and the overwhelmingly common
            // take never has one. A sidecar that cannot be written is logged
            // and the audio journal carries on: the marker is an annotation,
            // the spill is the promise.
            JournalWrite::Marker(line) => {
                if let Err(e) = append_marker_line(&markers, &line) {
                    log::warn!(
                        "YV92 device-change marker not written to {}: {e}",
                        markers.display()
                    );
                }
            }
        }
    }
    let _ = out.flush();
}

/// `<id>.spill.pcm` → `<id>.spill.markers.jsonl`. One function so the journal
/// and its writer thread can never disagree about where the sidecar lives.
fn markers_path_for(spill: &Path) -> PathBuf {
    spill.with_extension("markers.jsonl")
}

/// Append one JSON line to the marker sidecar. The only disk write in the
/// journal that is NOT the spill, and it runs on the writer thread like every
/// other one.
fn append_marker_line(path: &Path, line: &str) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")?;
    file.flush()
}

/// One take rebuilt from an orphaned journal (YV63).
pub struct RecoveredTake {
    /// The finalized wav, written next to the spill it came from — i.e. already
    /// inside the recovery dir the YV52 retry path and its 7-day purge use.
    pub wav_path: PathBuf,
    /// Length of the recovered audio, for the failed-dictation row.
    pub seconds: f64,
    /// DB-B — the take's own id, i.e. the `<id>` in `<id>.in_progress.json` /
    /// `<id>.capture.wav`. It is the ONLY thing that ties audio recovered off
    /// disk to the chunk text `take_chunks` persisted while the take was still
    /// being decoded; without it a recovered wav and its partial words are two
    /// unrelated artefacts in the same directory.
    pub take_id: String,
}

/// Pure predicate (mirrors `is_stale_wav`): does this dir entry name an
/// in-progress capture marker? Kept pure so the scan rule is unit-testable.
fn is_journal_marker(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(JOURNAL_MARKER_EXT) && lower.len() > JOURNAL_MARKER_EXT.len() + 1
}

/// Startup scan of `data_dir()/recovery/`: finalize every ORPHANED journal into
/// a playable wav. An orphan is a marker whose take never completed — the app
/// died mid-take — because a normal stop removes its own marker.
///
/// Returns one entry per take whose audio survived; a marker with no usable
/// audio (a stray tap, a zero-length spill) is simply cleaned up. Never fails:
/// anything unreadable is logged and left alone for the next launch to retry.
pub fn recover_orphaned_journals(dir: &Path) -> Vec<RecoveredTake> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut recovered = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        if !is_journal_marker(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let marker = entry.path();
        match finalize_orphaned_journal(&marker) {
            Ok(Some(take)) => recovered.push(take),
            Ok(None) => {}
            Err(e) => log::warn!(
                "YV63 orphaned take {} not recovered ({e}) — left for the next launch",
                marker.display()
            ),
        }
    }
    recovered
}

/// Turn ONE orphaned marker into a wav. `Ok(None)` means the marker was retired
/// with nothing worth recovering; `Err` leaves the marker in place so a
/// transient failure (full disk) does not throw the audio away.
fn finalize_orphaned_journal(marker: &Path) -> Result<Option<RecoveredTake>, String> {
    let raw = std::fs::read_to_string(marker).map_err(|e| e.to_string())?;
    let meta: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let spill = meta
        .get("spill")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| "marker has no spill path".to_string())?;
    let sample_rate = meta
        .get("sample_rate")
        .and_then(|v| v.as_u64())
        .filter(|r| *r > 0)
        .unwrap_or(TARGET_RATE as u64) as u32;

    let bytes = std::fs::read(&spill).unwrap_or_default();
    let samples: Vec<f32> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b) as f32 / i16::MAX as f32)
        .collect();
    // Same floor the live path uses: below it the "take" was a stray tap, and a
    // Retry row for it would be noise in History.
    if samples.len() < MIN_CLIP_SAMPLES {
        let _ = std::fs::remove_file(&spill);
        let _ = std::fs::remove_file(spill.with_extension("markers.jsonl"));
        let _ = std::fs::remove_file(marker);
        return Ok(None);
    }

    // `<id>.in_progress.json` → `<id>.wav`, i.e. the recovered clip lands right
    // where a kept failed take does (same dir, same retry + purge lifecycle).
    let name = marker.file_name().unwrap_or_default().to_string_lossy();
    let id = name
        .strip_suffix(&format!(".{JOURNAL_MARKER_EXT}"))
        .unwrap_or(&name);
    let wav_path = marker.with_file_name(format!("{id}.wav"));
    if let Err(e) = write_wav_i16(&wav_path, sample_rate, &samples) {
        // Never leave a half-written wav behind; the marker stays so the next
        // launch tries again rather than losing the utterance.
        let _ = std::fs::remove_file(&wav_path);
        return Err(e);
    }
    let _ = std::fs::remove_file(&spill);
    let _ = std::fs::remove_file(spill.with_extension("markers.jsonl"));
    let _ = std::fs::remove_file(marker);
    Ok(Some(RecoveredTake {
        wav_path,
        seconds: samples.len() as f64 / sample_rate as f64,
        take_id: id.to_string(),
    }))
}

/// DB-B — the OTHER half of what a dead long take leaves behind.
///
/// Y3-A spills a take's frames to `<recordings>/<id>.capture.wav` as they
/// arrive and, when the take dies mid-flight, [`SpillWriter::drop`] finalizes
/// the header and moves that file into the recovery dir. Its name is
/// deliberately not a journal marker, so [`recover_orphaned_journals`] walks
/// straight past it — which until now meant NOTHING read it back. The audio sat
/// in the recovery dir until the 7-day sweep deleted it, and the user's words
/// with it.
///
/// This turns each orphaned spill into `<id>.wav`, exactly the shape a recovered
/// journal produces, so it joins the ONE recovery lifecycle that already exists
/// (a `failed_dictations` row, the retry path's gates, the 7-day purge) instead
/// of growing a second one beside it.
///
/// Best-effort, like every other recovery step here: a spill that cannot be
/// finalized is LEFT ON DISK for the next launch rather than deleted.
pub fn recover_orphaned_spills(dir: &Path) -> Vec<RecoveredTake> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut recovered = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(id) = spill_take_id(&name) else {
            continue;
        };
        let spill = entry.path();
        match finalize_orphaned_spill(&spill, id) {
            Ok(Some(take)) => recovered.push(take),
            Ok(None) => {}
            Err(e) => log::warn!(
                "DB-B orphaned spill {} not recovered ({e}) — left for the next launch",
                spill.display()
            ),
        }
    }
    recovered
}

/// yap-fx-07 — the loudest one-second window an orphan may have and still be
/// called silent (RMS, full scale = 1.0). The 2026-11-06 forensics measured a
/// near-silent spill at ~0.002 and real speech at 0.06–0.10 in EVERY 10 s
/// window; 0.008 sits well clear of both, on the "keep it" side.
pub const ORPHAN_SILENCE_RMS: f32 = 0.008;

/// yap-fx-07 — is this recovered audio silent, i.e. safe to delete instead of
/// offering it as a failed dictation? Silent means NO one-second window gets to
/// [`ORPHAN_SILENCE_RMS`]: a single loud second anywhere keeps the clip, so a
/// long take that is mostly pauses is never thrown away. A non-finite sample
/// is treated as signal (keep), never as silence. Pure, so it is unit-tested.
pub fn is_silent_clip(samples: &[f32]) -> bool {
    if samples.iter().any(|s| !s.is_finite()) {
        return false;
    }
    let window = TARGET_RATE as usize;
    samples.chunks(window).all(|w| {
        let mean_sq = w.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / w.len() as f64;
        (mean_sq.sqrt() as f32) < ORPHAN_SILENCE_RMS
    })
}

/// yap-fx-07 — the IN-SESSION half of [`recover_orphaned_spills`].
///
/// [`recover_orphaned_spills`] only runs at launch, so a capture spill
/// `SpillWriter::drop` moves into the recovery dir mid-session (the 2026-11-06
/// "arm while already capturing — dropping the orphaned take" case) sat with no
/// `failed_dictations` row until the next launch — invisible in History, and a
/// candidate for the age sweep. This adopts those spills while the app runs.
///
/// Deliberately narrower than the launch scan, because takes are live now:
///
/// * only `<id>.capture.wav` — a `<id>.decoding.wav` in-flight link belongs to
///   a take that may be decoding RIGHT NOW, and only the launch scan (when
///   nothing is live) may read those back;
/// * only files at least `min_age` old (by mtime) — `SpillWriter::drop` can
///   fall back to copy + remove, and a half-copied file is not an orphan yet;
/// * never over an existing `<id>.wav` — that name may already be a kept take
///   with its own row, and the launch scan still sees the spill next time;
/// * never a spill a row already names (`keep`).
///
/// Same finalize as the launch scan (silent → deleted, else `<id>.wav`).
pub fn recover_aborted_capture_spills(
    dir: &Path,
    keep: &[String],
    now: std::time::SystemTime,
    min_age: Duration,
) -> Vec<RecoveredTake> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut recovered = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(id) = name
            .strip_suffix(const_spill_suffix())
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        let spill = entry.path();
        if keep.iter().any(|k| Path::new(k) == spill) {
            continue;
        }
        // An unreadable mtime counts as "just written": it can only defer.
        let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(now);
        if now.duration_since(modified).unwrap_or_default() < min_age {
            continue;
        }
        if spill.with_file_name(format!("{id}.wav")).exists() {
            log::info!("record orphan_spill_deferred reason=target_exists");
            continue;
        }
        match finalize_orphaned_spill(&spill, id) {
            Ok(Some(take)) => recovered.push(take),
            Ok(None) => {}
            Err(e) => log::warn!("record orphan_spill_failed error={e} action=left_for_next_pass"),
        }
    }
    recovered
}

/// Pure predicate (mirrors [`is_journal_marker`]): the take id inside
/// `<id>.capture.wav`, or `None` for anything else in the directory.
fn spill_take_id(name: &str) -> Option<&str> {
    // Y3-A's capture spill, or DB-B's in-flight decode link — both are a
    // complete 16 kHz wav named by its take id.
    let id = name
        .strip_suffix(const_spill_suffix())
        .or_else(|| name.strip_suffix(IN_FLIGHT_SUFFIX))?;
    (!id.is_empty()).then_some(id)
}

/// `".capture.wav"` — the dot belongs to the separator, not to the extension
/// constant, which is why this is spelled once here rather than at each use.
fn const_spill_suffix() -> &'static str {
    ".capture.wav"
}

/// Turn ONE orphaned spill into `<id>.wav`. `Ok(None)` means there was nothing
/// worth recovering (a stray tap under the same floor the journal path uses);
/// `Err` leaves the spill in place for the next launch.
///
/// The spill is deleted ONLY after a complete `<id>.wav` exists: the new wav is
/// written under a temp name and renamed into place, so a full disk or a crash
/// mid-write can never leave a truncated `<id>.wav` that a later launch takes
/// for the real thing. (An existing `<id>.wav` is NOT treated as proof the take
/// was already rebuilt — the journal and the spill never share an id, and a
/// partial wav from an interrupted write would have cost the only good copy.)
fn finalize_orphaned_spill(spill: &Path, id: &str) -> Result<Option<RecoveredTake>, String> {
    let wav_path = spill.with_file_name(format!("{id}.wav"));
    let samples = read_wav_16k_mono(spill)?;
    if samples.len() < MIN_CLIP_SAMPLES {
        let _ = std::fs::remove_file(spill);
        return Ok(None);
    }
    // yap-fx-07 — an orphan is DELETED only when it is silent. Everything else
    // becomes a row; a 6.7 s spill of room hiss (forensics 2026-11-06, loudness
    // 0.002) is not a dictation and a Retry for it would be noise in History.
    if is_silent_clip(&samples) {
        let _ = std::fs::remove_file(spill);
        log::info!(
            "record orphan_spill_silent seconds={:.1} action=deleted",
            samples.len() as f64 / TARGET_RATE as f64
        );
        return Ok(None);
    }
    let tmp = spill.with_file_name(format!("{id}.wav.partial"));
    if let Err(e) = write_wav_i16(&tmp, TARGET_RATE, &samples) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, &wav_path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    let _ = std::fs::remove_file(spill);
    Ok(Some(RecoveredTake {
        wav_path,
        seconds: samples.len() as f64 / TARGET_RATE as f64,
        take_id: id.to_string(),
    }))
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
        // YV92: anti-aliased on the way down (see `resample::resample_decimate`).
        // A 48 kHz file full of room noise used to fold its 8–24 kHz band
        // straight into the speech band on the way to 16 kHz, and the WER that
        // came back got blamed on the model.
        resample_decimate(&mono, spec.sample_rate, TARGET_RATE)
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
    let Some((mask, frame_secs)) = voiced_frames(samples, sample_rate) else {
        return 0.0;
    };
    mask.iter().filter(|&&b| b).count() as f64 * frame_secs
}

/// The SILENCES between voiced runs of a take (Y4-C), as
/// [`dictation::PauseSpan`]s the rules stage can paragraph on.
///
/// Same bridged energy-VAD mask [`voiced_seconds`] sums — the mask was already
/// being computed on every take and thrown away. Nothing new is measured here
/// and no model is required, which is the point: paragraphing has to work with
/// no polish model and no Silero model installed, which is the default.
///
/// Each span's position is carried as a share of the take's total VOICED time,
/// so it survives the Silero trim that may follow (trimming leading silence
/// changes wall-clock offsets and changes no voiced fraction at all).
/// Leading and trailing silence are not pauses BETWEEN anything and are dropped.
pub fn pause_spans(samples: &[f32], sample_rate: u32) -> Vec<dictation::PauseSpan> {
    let Some((mask, frame_secs)) = voiced_frames(samples, sample_rate) else {
        return Vec::new();
    };
    let total_voiced = mask.iter().filter(|&&b| b).count() as f64 * frame_secs;
    if total_voiced <= 0.0 {
        return Vec::new();
    }
    let mut spans = Vec::new();
    let mut voiced_before = 0.0f64;
    let mut gap = 0usize;
    let mut seen_voice = false;
    for &v in &mask {
        if v {
            if seen_voice && gap > 0 {
                spans.push(dictation::PauseSpan {
                    seconds: gap as f64 * frame_secs,
                    at_voiced_fraction: voiced_before / total_voiced,
                });
            }
            gap = 0;
            seen_voice = true;
            voiced_before += frame_secs;
        } else if seen_voice {
            gap += 1;
        }
    }
    spans
}

/// The bridged per-frame voiced mask, plus the frame duration in seconds.
/// `None` when there is nothing to analyse.
fn voiced_frames(samples: &[f32], sample_rate: u32) -> Option<(Vec<bool>, f64)> {
    if sample_rate == 0 || samples.is_empty() {
        return None;
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
        return None;
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
    for (idx, &voiced) in mask.iter().enumerate() {
        if voiced {
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
    Some((bridged, frame_secs))
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
/// Close the persistent stream after this long without a take (mic indicator
/// off) — ON BATTERY, and the floor everywhere. See [`idle_close_after`].
const IDLE_CLOSE: Duration = Duration::from_secs(60);
/// SP-09 — on wall power the warm stream stays open this long without a take.
///
/// THE MIC INDICATOR RULE (SP-09, yap24-OS1). macOS lights the orange
/// microphone indicator for as long as ANY input stream is open, whether or not
/// a single sample is kept. Yap keeps its stream open only (1) while a take is
/// armed, (2) while a meeting holds it, (3) from an fn key-down (the press
/// opens it, so a cold mic starts filling the pre-roll at the press instead of
/// after `HOLD_ARM_MS`), and (4) for the idle window after the last take or
/// press: [`IDLE_CLOSE_AC`] (5 min) on wall power, [`IDLE_CLOSE`] (60 s) on
/// battery. While open and idle, audio lives ONLY in the consumer's in-memory
/// pre-roll ring (the newest [`PRE_ROLL_MS`] + [`PRESS_GRACE_MS`]); nothing is
/// written, sent or kept unless a take arms, and a tap that was not a hold
/// empties the ring. When the indicator is lit and no take is running, it is
/// one of those four windows and nothing else.
const IDLE_CLOSE_AC: Duration = Duration::from_secs(5 * 60);
/// SP-09 — audio kept from BEFORE a take's anchor (the fn key-down, or the arm
/// itself for a start with no press), so a word spoken AT the press is whole.
pub const PRE_ROLL_MS: u64 = 500;
/// SP-09 — how long after the key-down a hold may still become a take and
/// claim the audio from the press on. Covers `HOLD_ARM_MS` (280 ms) plus the
/// Start→Arm handshake with room to spare; a press older than this is stale.
pub const PRESS_GRACE_MS: u64 = 1_000;
/// SP-09 — the press→first-sample budget on a warm stream (logged every take).
pub const PRESS_TO_FIRST_SAMPLE_BUDGET_MS: u64 = 20;
/// SP-09 — taps whose pre-armed audio was discarded (telemetry counter).
static PRE_ARM_DISCARDS: AtomicU64 = AtomicU64::new(0);

/// SP-09 — the idle window for the power source the Mac is on right now.
fn idle_close_after(on_battery: bool) -> Duration {
    if on_battery {
        IDLE_CLOSE
    } else {
        IDLE_CLOSE_AC
    }
}
/// How often the idle worker wakes to test the idle window — 5s puts the mic
/// indicator out within a tick of [`IDLE_CLOSE`] without splitting hairs over
/// it. YV81: this tick runs ONLY while a stream is actually open; with nothing
/// to close the worker blocks on its channel and never wakes at all.
const IDLE_TICK: Duration = Duration::from_secs(5);
/// YV92/OS-9 — how often the capture watchdog re-reads the input device's health
/// and format while a stream is open. Sixty seconds is the plan's own figure and
/// it is the CEILING, not the latency: the CoreAudio property listener
/// (`input_format::arm_listeners`) makes a real format change wake the worker on
/// the very next tick, and this interval is what covers a machine where the
/// listener could not be installed at all.
const WATCHDOG_TICK: Duration = Duration::from_secs(60);
/// How long `end` waits for the consumer to finish the take's tail (YV91).
const CONSUMER_DRAIN_TIMEOUT: Duration = Duration::from_millis(500);
/// WF-10 — the worker's tick while a take (or a meeting) is in flight. A mic
/// that vanishes mid-take is noticed within this, and the take carries on on
/// the next ranked mic; the idle stream keeps [`IDLE_TICK`] (YV81).
const ARMED_TICK: Duration = Duration::from_millis(250);
/// WF-10 — a stream whose device errored is retried no more often than this
/// (an unplugged mic with nothing to fall back to must not spin the HAL).
const FAILED_RETRY: Duration = Duration::from_secs(1);

// ── WF-10: "Mic disconnected — continuing on …" ─────────────────────────────
// Set by the capture worker when a take in flight is carried onto a DIFFERENT
// input device; read by `build_status` (lib.rs) so the pill says so, and
// cleared at the next Arm so the note belongs to exactly one take.

fn mic_switch_slot() -> &'static Mutex<Option<String>> {
    static NOTE: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    NOTE.get_or_init(|| Mutex::new(None))
}

static MIC_SWITCH_SEQ: AtomicU64 = AtomicU64::new(0);

/// The pill line for a take that moved to `to` — because its mic went away
/// (`lost`), or because the resolver now prefers another one (the pinned mic
/// came back). Never empty, never a path; the name is the one Settings shows.
pub fn mic_switch_note(to: &str, lost: bool) -> String {
    let to = to.trim();
    match (lost, to.is_empty()) {
        (true, true) => "Mic disconnected — continuing on another microphone".to_string(),
        (true, false) => format!("Mic disconnected — continuing on {to}"),
        (false, true) => "Switched microphones — still recording".to_string(),
        (false, false) => format!("Switched to {to} — still recording"),
    }
}

/// The note for the take in flight, if its mic was swapped mid-take.
pub fn mid_take_mic_note() -> Option<String> {
    mic_switch_slot()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

/// Bumped on every mid-take switch; the HUD loop re-emits status when it moves.
pub fn mic_switch_seq() -> u64 {
    MIC_SWITCH_SEQ.load(Ordering::SeqCst)
}

fn note_mid_take_switch(to: &str, lost: bool) {
    *mic_switch_slot().lock().unwrap_or_else(|p| p.into_inner()) = Some(mic_switch_note(to, lost));
    MIC_SWITCH_SEQ.fetch_add(1, Ordering::SeqCst);
}

fn clear_mid_take_note() {
    *mic_switch_slot().lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// Did a reopen move a take in flight onto a different device? (Pure.) A
/// format change on the SAME mic (AirPods dropping to HFP) is not a switch.
fn is_mid_take_switch(from: &str, to: &str, armed: bool) -> bool {
    armed && !to.is_empty() && from != to
}

// ── WF-13: "I can't hear <mic>" ─────────────────────────────────────────────
// A take whose first 1.5 s carries no sound at all — a muted interface, a
// headset with its hardware mute on, a virtual input with nothing routed into
// it — used to run to the end and come back as an empty dictation. The capture
// consumer now listens to that opening window, and if it is below the silence
// floor the pill says "I can't hear <mic>" with a Switch mic button while the
// take KEEPS recording (the warning never stops capture). It fires at most once
// per take, and it goes away the moment real sound arrives.

/// How much of the take's opening is judged.
pub const SILENT_MIC_WINDOW: Duration = Duration::from_millis(1500);

/// The silence floor, as an AMPLITUDE (half the peak-to-peak swing, full scale
/// = 1.0) — about -66 dBFS. A dead or muted input delivers digital zero or a
/// few LSBs of converter noise (~1e-5); a live mic in a quiet room idles at
/// -60 dBFS and up, and the quietest speech the `ORPHAN_SILENCE_RMS` forensics
/// measured sits near 0.002 RMS with peaks well above this. Peak-to-peak (not
/// raw peak) so an interface that idles at a DC offset still reads silent.
pub const SILENT_MIC_FLOOR: f32 = 0.0005;

/// What one block of the take's audio changed about the silent-mic verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SilentMicEvent {
    /// Nothing to report (still inside the window, or already decided).
    None,
    /// The opening window closed below the floor: warn — once.
    Silent,
    /// After a `Silent`, sound finally arrived: clear the warning — once.
    Recovered,
}

/// WF-13 — the level check over a take's opening window. Pure (no clock, no
/// device): it counts frames, so the unit tests feed it recorded fixtures.
#[derive(Debug, Clone)]
pub struct SilentMicCheck {
    channels: usize,
    window_frames: u64,
    seen_frames: u64,
    lo: f32,
    hi: f32,
    /// `Some(true)` = warned and still silent, `Some(false)` = decided (heard
    /// sound, or recovered). `None` = still inside the window.
    state: Option<bool>,
}

impl SilentMicCheck {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        let rate = u64::from(sample_rate.max(1));
        Self {
            channels: channels.max(1),
            window_frames: rate * SILENT_MIC_WINDOW.as_millis() as u64 / 1000,
            seen_frames: 0,
            lo: f32::INFINITY,
            hi: f32::NEG_INFINITY,
            state: None,
        }
    }

    /// Has the opening window been judged yet?
    #[cfg(test)]
    pub fn decided(&self) -> bool {
        self.state.is_some()
    }

    /// Feed the take's next INTERLEAVED block (native rate, `channels` wide).
    pub fn feed(&mut self, interleaved: &[f32]) -> SilentMicEvent {
        match self.state {
            Some(false) => SilentMicEvent::None,
            Some(true) => {
                if block_amplitude(interleaved) > SILENT_MIC_FLOOR {
                    self.state = Some(false);
                    SilentMicEvent::Recovered
                } else {
                    SilentMicEvent::None
                }
            }
            None => {
                let frames = (interleaved.len() / self.channels) as u64;
                let room = self.window_frames.saturating_sub(self.seen_frames);
                let take = frames.min(room) as usize * self.channels;
                for &s in &interleaved[..take.min(interleaved.len())] {
                    if s.is_finite() {
                        self.lo = self.lo.min(s);
                        self.hi = self.hi.max(s);
                    }
                }
                self.seen_frames += frames.min(room);
                if self.amplitude() > SILENT_MIC_FLOOR {
                    // Sound inside the window: decided early, never warns.
                    self.state = Some(false);
                    return SilentMicEvent::None;
                }
                if self.seen_frames >= self.window_frames {
                    self.state = Some(true);
                    return SilentMicEvent::Silent;
                }
                SilentMicEvent::None
            }
        }
    }

    fn amplitude(&self) -> f32 {
        if self.hi < self.lo {
            0.0
        } else {
            (self.hi - self.lo) * 0.5
        }
    }
}

/// Half the peak-to-peak swing of a block (0.0 for an empty one).
fn block_amplitude(block: &[f32]) -> f32 {
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &s in block.iter().filter(|s| s.is_finite()) {
        lo = lo.min(s);
        hi = hi.max(s);
    }
    if hi < lo {
        0.0
    } else {
        (hi - lo) * 0.5
    }
}

/// The pill line. The name is the one Settings shows; never empty, never a path.
pub fn silent_mic_note(mic: &str) -> String {
    let mic = mic.trim();
    if mic.is_empty() {
        "I can't hear your microphone".to_string()
    } else {
        format!("I can't hear {mic}")
    }
}

fn silent_mic_slot() -> &'static Mutex<Option<String>> {
    static NOTE: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    NOTE.get_or_init(|| Mutex::new(None))
}

fn armed_mic_slot() -> &'static Mutex<String> {
    static NAME: OnceLock<Mutex<String>> = OnceLock::new();
    NAME.get_or_init(|| Mutex::new(String::new()))
}

/// Bumped at every Arm: the take the silent-mic warning belongs to.
static SILENT_MIC_TAKE: AtomicU64 = AtomicU64::new(0);
/// The take the warning last fired for — the once-per-take gate. A mid-take
/// reopen (WF-10) starts a new consumer with a fresh check, and this is what
/// keeps the second device from warning a second time in the same take.
static SILENT_MIC_FIRED_FOR: AtomicU64 = AtomicU64::new(0);

/// The pure once-per-take gate: true only for the FIRST claim of `take`.
fn claim_once(fired_for: &AtomicU64, take: u64) -> bool {
    take != 0 && fired_for.swap(take, Ordering::SeqCst) != take
}

/// The warning for the take in flight, if its mic is silent.
pub fn silent_mic_warning() -> Option<String> {
    silent_mic_slot()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

/// A new take: forget the last take's warning and remember which mic it opened on.
fn begin_silent_mic_take(device_name: &str) {
    SILENT_MIC_TAKE.fetch_add(1, Ordering::SeqCst);
    *armed_mic_slot().lock().unwrap_or_else(|p| p.into_inner()) = device_name.to_string();
    *silent_mic_slot().lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// A mid-take reopen moved the take to `device_name`: the old mic's warning no
/// longer describes it (the WF-10 note replaces it).
fn retarget_silent_mic(device_name: &str) {
    *armed_mic_slot().lock().unwrap_or_else(|p| p.into_inner()) = device_name.to_string();
    let had = silent_mic_slot()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
        .is_some();
    if had {
        MIC_SWITCH_SEQ.fetch_add(1, Ordering::SeqCst);
    }
}

/// Apply one check event to the shared slot (the HUD loop redraws on the seq).
fn apply_silent_mic_event(event: SilentMicEvent) {
    match event {
        SilentMicEvent::None => {}
        SilentMicEvent::Silent => {
            let take = SILENT_MIC_TAKE.load(Ordering::SeqCst);
            if !claim_once(&SILENT_MIC_FIRED_FOR, take) {
                return;
            }
            let mic = armed_mic_slot()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone();
            *silent_mic_slot().lock().unwrap_or_else(|p| p.into_inner()) =
                Some(silent_mic_note(&mic));
            MIC_SWITCH_SEQ.fetch_add(1, Ordering::SeqCst);
            log::warn!(
                "record silent_mic_warned window_ms={} floor={} named={}",
                SILENT_MIC_WINDOW.as_millis(),
                SILENT_MIC_FLOOR,
                !mic.trim().is_empty()
            );
        }
        SilentMicEvent::Recovered => {
            let had = silent_mic_slot()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take()
                .is_some();
            if had {
                MIC_SWITCH_SEQ.fetch_add(1, Ordering::SeqCst);
                log::info!("record silent_mic_recovered");
            }
        }
    }
}

pub(crate) const NO_MIC_ERR: &str = "No microphone found. Click Dictate once so macOS prompts, then enable Yap under System Settings → Privacy → Microphone.";
pub(crate) const NO_SAMPLES_ERR: &str =
    "No samples captured. Enable Microphone for Yap in System Settings.";
pub(crate) const NO_AUDIO_ERR: &str = "No audio captured — click Allow once for Microphone (Yap) in the system dialog, then enable it under System Settings → Privacy → Microphone.";

/// One take as it comes off the capture worker. Since YV37 the frames are
/// already downmixed, high-passed and resampled to 16 kHz *during* capture, so
/// this is nearly the finished ASR buffer — `finalize_take` only levels, fades
/// and (optionally) denoises it.
struct CapturedAudio {
    /// 16 kHz mono, high-passed — streamed frame by frame while the user spoke.
    ///
    /// Since Y3 this is EMPTY whenever `spill` is `Some`: the take's audio
    /// lives in the spill and `finalize_take` reads it back. It carries the
    /// whole take only on the degraded path where no spill could be written.
    samples: Vec<f32>,
    /// Y3 — the take on disk, written incrementally during capture. This
    /// replaced the unbounded native-rate `raw` Vec that used to be the
    /// never-lose-audio fallback; a finished WAV is the stronger copy.
    spill: Option<CaptureSpill>,
    /// Native capture rate (`samples`/the spill are always 16 kHz).
    sample_rate: u32,
    /// Soft-AGC gain accumulated over the take, applied at finalize. `1.0` means
    /// "leave the level alone" (silence / degenerate input).
    gain: f32,
    /// YV63 crash journal, handed back with the take so `stop_recording` can
    /// retire it. `None` when journalling could not be started for this take.
    journal: Option<CaptureJournal>,
    /// YV67 — stamped at Disarm from `LiveStream::has_failed`: did the capture
    /// device error while this take was being held? The DSP never sets it; the
    /// worker does, because only the worker owns the stream.
    device_failed: bool,
    /// SP-06 — the release-side DSP (edge fade, denoise, Silero) already run
    /// over this take frame by frame during capture. `None` when the take was
    /// not analysed (denoise off) or the analysis fell out of step with the
    /// capture; `finish_recording` then runs the batch chain, as before.
    analysis: Option<Box<TakeAnalysis<'static>>>,
}

/// Stamp a finished take with the capture device's health (YV67).
///
/// Split out of the `Disarm` arm so the hand-off is testable without a live cpal
/// stream, and written as a pass-through on purpose: the samples ride out
/// UNTOUCHED. A mic that died mid-hold still recorded everything up to the
/// moment it died, and that partial audio is exactly what the pipeline needs to
/// keep so the take stays retryable instead of pasting a truncated transcript.
fn mark_device_failure(mut captured: CapturedAudio, failed: bool) -> CapturedAudio {
    if failed {
        log::warn!(
            "capture device errored mid-take — keeping the {} sample(s) it did capture",
            captured.samples.len()
        );
    }
    captured.device_failed = failed;
    captured
}

enum CaptureCmd {
    /// Begin buffering into the persistent stream (opening it if needed), with
    /// this take's crash journal (YV63) for the capture path to spill into.
    Arm {
        journal: Option<CaptureJournal>,
        /// Y3 — this take's capture spill, opened by `start_recording` in the
        /// recordings dir so the capture worker never has to know a path.
        ///
        /// Boxed: the writer is ~16x every other command, and every command
        /// on this channel would otherwise pay for its size.
        spill: Option<Box<SpillWriter>>,
        /// SP-06 — this take's streamed release DSP, installed with the take so
        /// it sees the first sample.
        analysis: Option<Box<TakeAnalysis<'static>>>,
        reply: mpsc::SyncSender<Result<(), String>>,
    },
    /// Stop buffering and hand back the take. The stream STAYS open.
    Disarm {
        reply: mpsc::SyncSender<Result<CapturedAudio, String>>,
    },
    /// YV91: open the stream (if it is not already) and HOLD it open for a
    /// meeting, which has no Arm of its own.
    ///
    /// Without this a meeting is a passenger on whatever stream a dictation
    /// happened to leave open — and the idle closer drops that stream after
    /// [`IDLE_CLOSE`], so a meeting with no concurrent dictation captures
    /// exactly 60 seconds and then finalizes a 60-second wav as `complete`.
    /// A meeting must own its own audio source.
    /// The reply carries the format the stream is actually running at. YV95's
    /// capture adapter builds the meeting's `SessionConfig` from it: the native
    /// rate is only knowable once a stream is open, and configuring a meeting's
    /// resampler from a guess is how a three-hour recording comes back at the
    /// wrong speed.
    Hold {
        reply: mpsc::SyncSender<Result<InputFormat, String>>,
    },
    /// The meeting ended: the stream may go idle again (and the idle clock
    /// starts now, so the mic indicator goes out a minute after the MEETING,
    /// not a minute after the last dictation).
    Release,
    /// SP-09 — the push-to-talk key went down. Open the stream if it is closed
    /// (a cold open now starts at the press, not `HOLD_ARM_MS` later) and stamp
    /// the press frame, so the take that may follow opens its window there.
    /// Fire-and-forget: the event tap thread never waits on the worker.
    PreArm { pressed_at: Instant },
    /// SP-09 — the press was a tap (or was cut short), not a hold: forget the
    /// press and empty the pre-roll ring so its audio never reaches a take.
    DiscardPreArm,
}

/// SP-09 — fn (or a held mouse button) went down: capture starts now. Never
/// blocks; a failure is logged and the Arm that follows surfaces it.
pub fn pre_arm(pressed_at: Instant) {
    if let Err(e) = dispatch(CaptureCmd::PreArm { pressed_at }) {
        log::warn!("record pre_arm_failed error={e}");
    }
}

/// SP-09 — the press turned out to be a tap: discard what it buffered.
pub fn discard_pre_arm() {
    if let Err(e) = dispatch(CaptureCmd::DiscardPreArm) {
        log::warn!("record pre_arm_discard_failed error={e}");
    }
}

/// SP-09 — a key-down the next take may anchor on, in the CURRENT ring's frame
/// space (a reopen forgets it).
#[derive(Debug, Clone, Copy)]
struct PendingPress {
    frame: u64,
    at: Instant,
    /// Was the stream already open at the press (the "warm" budget applies)?
    warm: bool,
}

/// SP-09 — the consumer's pre-roll: the ring of recent native frames plus the
/// bookkeeping that replays it into a take once and times the press.
///
/// Belongs to the DEVICE (like the rings and the window): a reopen adopts a
/// fresh one, because frame indices restart with the new ring.
struct PreRollState {
    ring: crate::rtring::PreRoll,
    /// Frames of pre-roll a take keeps before its anchor ([`PRE_ROLL_MS`]).
    lead_frames: u64,
    /// The window start the ring was last replayed for (`u64::MAX` = none).
    replayed_for: u64,
    scratch: Vec<f32>,
    /// The press being timed: (frame, key-down instant).
    press_watch: Option<(u64, Instant)>,
    /// Key-down → the device delivered a frame at/after it, for the last press.
    press_to_first_sample_ms: Option<u64>,
}

impl PreRollState {
    fn new(sample_rate_hz: u32, channels: u16) -> Self {
        let rate = sample_rate_hz as u64;
        let capacity = rate * (PRE_ROLL_MS + PRESS_GRACE_MS) / 1000;
        Self::with_frames(capacity as usize, rate * PRE_ROLL_MS / 1000, channels)
    }

    fn with_frames(capacity_frames: usize, lead_frames: u64, channels: u16) -> Self {
        Self {
            ring: crate::rtring::PreRoll::with_frames(capacity_frames, channels.max(1) as usize),
            lead_frames: lead_frames.min(capacity_frames as u64),
            replayed_for: u64::MAX,
            scratch: Vec::with_capacity(capacity_frames * channels.max(1) as usize),
            press_watch: None,
            press_to_first_sample_ms: None,
        }
    }
}

/// SP-09 — where a take's arm window opens: `lead` frames before its anchor
/// (the key-down frame when there is a live press, else the arm frame), never
/// before the previous take's close (no word is kept twice).
///
/// Pure: this is the arithmetic that decides whether the first syllable of a
/// dictation survives, and the only other way to exercise it is a microphone.
fn take_window_start(now_frame: u64, press_frame: Option<u64>, lead: u64, prev_to: u64) -> u64 {
    let anchor = press_frame.filter(|&p| p <= now_frame).unwrap_or(now_frame);
    let from = anchor.saturating_sub(lead);
    if prev_to != u64::MAX && prev_to <= now_frame {
        from.max(prev_to)
    } else {
        from
    }
}

/// SP-09 — before a drained block goes out: a take whose window opened BEHIND
/// what this consumer already drained gets those frames from the pre-roll ring
/// first, once per window. Returns the frames replayed.
fn replay_pre_roll(
    pre: &Mutex<PreRollState>,
    dsp: &Mutex<StreamDsp>,
    pos: u64,
    from: u64,
    to: u64,
) -> usize {
    let Ok(mut guard) = pre.lock() else {
        return 0;
    };
    let st = &mut *guard;
    if from == u64::MAX || from >= pos || st.replayed_for == from {
        return 0;
    }
    st.replayed_for = from;
    st.scratch.clear();
    let (_, n) = st.ring.copy_range(from, to.min(pos), &mut st.scratch);
    if n > 0 {
        if let Ok(mut d) = dsp.lock() {
            d.push(&st.scratch);
        }
    }
    n
}

/// SP-09 — after a drained block: keep it in the pre-roll ring and finish
/// timing a press the device has now delivered audio past.
fn remember_block(pre: &Mutex<PreRollState>, pos: u64, block: &[f32]) {
    if let Ok(mut st) = pre.lock() {
        st.ring.push(pos, block);
        let end = st.ring.end_frame();
        if let Some((frame, at)) = st.press_watch {
            if end > frame {
                st.press_to_first_sample_ms = Some(at.elapsed().as_millis() as u64);
                st.press_watch = None;
            }
        }
    }
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
        Err(mpsc::RecvTimeoutError::Timeout) => Err(crate::permissions::named(&format!(
            "Microphone {what} timed out after {}ms — check Yap's Microphone permission under System Settings → Privacy.",
            timeout.as_millis()
        ))),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(format!("Microphone {what} failed — capture worker stopped."))
        }
    }
}

/// Cache of the input device + the stream config it accepted. The HAL property
/// queries behind `default_input_config` cost tens of ms per open on macOS
/// (worse on USB/Bluetooth) and land straight on the keypress→capture path
/// whenever the persistent stream has to be reopened. `invalidate` is called on
/// ANY open or stream error so a stale rate/format self-heals on the next take.
/// Generic over the device/config types so the logic is unit-testable without
/// touching audio hardware.
///
/// YV92/OS-9 — an entry is keyed on the device name **and** the format it was
/// cached at. AirPods keep their name across a rate renegotiation (48000 →
/// 24000, or down to the HFP rate the moment their microphone is engaged), so a
/// name-only key handed the reopen a stale sample rate and the remainder of the
/// session came out time-stretched.
///
/// Be precise about who that key actually protects, because it is easy to
/// over-claim (YV92 review): asking with `Some(format)` requires already having
/// queried the format, which is the very HAL round-trip this cache exists to
/// skip. So the one production lookup — [`resolve_device_config`] — asks with
/// `None`, and what really keeps a renegotiated rate out of an open is the
/// INVALIDATION discipline around it:
///
/// * while a stream is live, the format watchdog invalidates on every real
///   change (see [`reopen_after_change`]);
/// * when the stream idle-closes, [`close_idle_capture`] invalidates, because
///   nothing is watching the HAL any more and the next Arm is a cold open that
///   must not inherit a minute-old format;
/// * a format-change edge that arrives with no stream open invalidates on the
///   spot (`capture_worker_loop`);
/// * any open or stream error invalidates, as it always did.
///
/// The format half of the key is then the belt to that braces: a caller that
/// *does* know the current format (the tests, and any future caller that has
/// already paid for the query) can never be handed a stale entry.
struct DeviceConfigCache<D, C> {
    entry: Option<CachedDevice<D, C>>,
}

struct CachedDevice<D, C> {
    name: String,
    format: InputFormat,
    device: D,
    config: C,
}

impl<D: Clone, C: Clone> DeviceConfigCache<D, C> {
    fn new() -> Self {
        Self { entry: None }
    }

    /// Cached device + config for `name`, or `None` on a miss. An empty name is
    /// NEVER a hit: cpal reports it when it cannot identify the device, and
    /// caching under it would pin an unknown device forever. When `format` is
    /// given, a cached entry at a DIFFERENT format is a miss too — that is the
    /// AirPods case above.
    fn get(&self, name: &str, format: Option<InputFormat>) -> Option<(D, C)> {
        match &self.entry {
            Some(entry)
                if !name.is_empty()
                    && entry.name == name
                    && format.is_none_or(|f| f == entry.format) =>
            {
                Some((entry.device.clone(), entry.config.clone()))
            }
            _ => None,
        }
    }

    fn store(&mut self, name: String, format: InputFormat, device: D, config: C) {
        if name.is_empty() {
            self.entry = None;
            return;
        }
        self.entry = Some(CachedDevice {
            name,
            format,
            device,
            config,
        });
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
    /// `None` for the brief moment a format change is swapping the device out,
    /// and after a swap that could not be completed. Optional rather than
    /// mandatory so the DSP buffers — the take's audio — outlive the device the
    /// take started on (OS-9: the recovery must not cost the recording).
    stream: Option<cpal::Stream>,
    /// Streaming DSP + buffers. Written by the CONSUMER thread since YV91 —
    /// the audio callback no longer takes this lock (OS-7).
    dsp: Arc<Mutex<StreamDsp>>,
    /// Which frame indices belong to the take that is armed (YV91).
    ///
    /// This replaced YV92's momentary `capturing` gate: the callback no longer
    /// decides anything (a meeting has no Arm at all), so the take's slice of
    /// each drained block is an index range, resolved by the consumer.
    window: Arc<ArmWindow>,
    /// The take's ARMED INTENT — "a take is in flight and wants audio" — as
    /// opposed to [`ArmWindow::is_open`], which is only whether the CURRENT
    /// stream's frames are being routed to it right now (YV92 review).
    ///
    /// The two differ for exactly as long as a reopen takes, and conflating
    /// them re-created OS-9's failure mode from the inside: a reopen builds a
    /// NEW ring whose frame counter restarts at zero, so the old window's
    /// `from` — a frame index in the CLOSED stream's space — can never be
    /// reached again and every remaining frame of a three-hour take is routed
    /// to nobody, with nothing logged. Armed intent is set by
    /// [`LiveStream::begin`] and cleared by [`LiveStream::end`] — by the take,
    /// never by the device — so a reopen has something durable to re-open the
    /// window *from* (see [`reopen_with`]).
    armed: AtomicBool,
    failed: Arc<AtomicBool>,
    /// The preallocated rings the callback writes into (YV91). Rebuilt by a
    /// reopen, because its geometry is the device's.
    rt: Arc<crate::meeting::RtCapture>,
    /// Stop flag + join handle for the thread draining [`Self::rt`] (YV91).
    /// Dropping it (including by ASSIGNING a new one over it, which is what a
    /// reopen does) stops and joins the old consumer.
    consumer: ConsumerHandle,
    /// What the stream is ACTUALLY running on, as opposed to what a cache
    /// believes. The format-change watchdog compares against these.
    device_name: String,
    format: InputFormat,
    /// The format-change state machine, owned for the LIFE OF THE STREAM.
    ///
    /// It has to live here rather than inside `watchdog_poll`: the watch is
    /// what numbers the spill segments, so a fresh one per tick would stamp
    /// every `device_change` marker of a take with `segment_index: 1` and the
    /// sidecar would no longer order. It rides through a reopen the same way
    /// the DSP does (see [`reopen_after_change`]) and restarts its numbering at
    /// each new take (see [`LiveStream::begin`]).
    watch: InputFormatWatch,
    /// SP-09 — the consumer's pre-roll ring (shared with the consumer thread).
    pre_roll: Arc<Mutex<PreRollState>>,
    /// SP-09 — the key-down the next take anchors on, if one is live.
    pending_press: Option<PendingPress>,
}

/// The consumer thread's stop flag and join handle, as ONE owned value.
///
/// YV91 put this `Drop` on `LiveStream` itself. That stopped compiling the
/// moment YV92's reopen needed to move a freshly-opened stream's parts onto the
/// live one (you cannot move out of a type that implements `Drop`), and the
/// obvious workaround — copying the handle out and letting the shell drop —
/// would have signalled stop on the Arc the NEW consumer is reading. Owning the
/// pair here makes "replace the consumer" a plain assignment whose semantics
/// are exactly right: the old thread is told to stop and joined, the new one is
/// left alone.
struct ConsumerHandle {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl Drop for ConsumerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl LiveStream {
    /// Begin a take: drop anything the callback saw while idle (and every bit of
    /// the previous take's DSP state), arm this take's crash journal, zero the
    /// meter, then open the gate. The journal is installed AFTER the reset — the
    /// reset is what retires a previous take's journal, and the segment
    /// numbering restarts with it because the marker sidecar is per-take.
    ///
    /// YV91 (finding #2a): `reset()` is still destructive, and that is now
    /// SAFE, because a meeting's audio does not live in `StreamDsp` — it is a
    /// separate consumer of the same fanned-out block. Wiping the take state
    /// here can no longer wipe a meeting that is recording.
    #[cfg(test)]
    fn begin(&mut self, journal: Option<CaptureJournal>, spill: Option<SpillWriter>) {
        self.begin_with(journal, spill, None);
    }

    /// [`Self::begin`] with the take's SP-06 streamed analysis, installed under
    /// the same lock as the reset so it sees the take's first sample.
    fn begin_with(
        &mut self,
        journal: Option<CaptureJournal>,
        spill: Option<SpillWriter>,
        analysis: Option<Box<TakeAnalysis<'static>>>,
    ) {
        if let Ok(mut dsp) = self.dsp.lock() {
            dsp.reset();
            dsp.journal = journal;
            dsp.spill = spill;
            dsp.analysis = analysis;
        }
        self.watch.restart_segments();
        capture_level().store(0, Ordering::Relaxed);
        // Intent first, window second: a watchdog tick that lands between the
        // two must never see an open window with nothing armed behind it.
        self.armed.store(true, Ordering::SeqCst);
        // SP-09 — the take owns everything from its window start on: the
        // key-down frame (capture starts at the press, not at the arm) less
        // `PRE_ROLL_MS`, replayed by the consumer from its pre-roll ring.
        let now_frame = self.rt.frames_accepted();
        let (_, prev_to) = self.window.bounds();
        let press = self.pending_press.take().filter(|p| {
            p.frame <= now_frame && p.at.elapsed() <= Duration::from_millis(PRESS_GRACE_MS)
        });
        let (lead, first_sample_ms) = self
            .pre_roll
            .lock()
            .map(|st| (st.lead_frames, st.press_to_first_sample_ms))
            .unwrap_or((0, None));
        let from = take_window_start(now_frame, press.map(|p| p.frame), lead, prev_to);
        let rate = (self.format.sample_rate_hz as u64).max(1);
        let kept_before_arm_ms = now_frame.saturating_sub(from) * 1000 / rate;
        match press {
            Some(p) => {
                let ms = first_sample_ms.unwrap_or_else(|| p.at.elapsed().as_millis() as u64);
                log::info!(
                    "record take_armed anchor=press kept_before_arm_ms={kept_before_arm_ms} \
                     press_to_first_sample_ms={ms} warm={} measured={}",
                    p.warm,
                    first_sample_ms.is_some()
                );
                if p.warm && ms > PRESS_TO_FIRST_SAMPLE_BUDGET_MS {
                    log::warn!(
                        "record press_to_first_sample_over_budget ms={ms} budget_ms={}",
                        PRESS_TO_FIRST_SAMPLE_BUDGET_MS
                    );
                }
            }
            None => log::info!(
                "record take_armed anchor=arm kept_before_arm_ms={kept_before_arm_ms} \
                 press_to_first_sample_ms=na"
            ),
        }
        self.window.open(from);
    }

    /// SP-09 — the push-to-talk key went down on this stream: stamp the frame
    /// the device is at, and start timing press→first-sample.
    fn note_press(&mut self, at: Instant, warm: bool) {
        let frame = self.rt.frames_accepted();
        self.pending_press = Some(PendingPress { frame, at, warm });
        if let Ok(mut st) = self.pre_roll.lock() {
            st.press_watch = Some((frame, at));
            st.press_to_first_sample_ms = None;
        }
    }

    /// SP-09 — the press was a tap: forget it and empty the pre-roll ring (a
    /// take already in flight keeps its audio). Returns whether a press was
    /// pending.
    fn discard_press(&mut self) -> bool {
        let had = self.pending_press.take().is_some();
        if !self.is_armed() {
            if let Ok(mut st) = self.pre_roll.lock() {
                st.ring.clear();
                st.press_watch = None;
            }
        }
        had
    }

    /// End a take: close the gate, close the take's spill and take the 16 kHz
    /// buffer (plus its AGC stats) off the DSP state.
    fn end(&self) -> CapturedAudio {
        // Disarm before closing the window — the take is over, so a reopen
        // racing this must not resurrect it.
        self.armed.store(false, Ordering::SeqCst);
        // Close the window at the frame the device is at, then WAIT for the
        // consumer to work through it: those frames are the tail of the take
        // (the ones still in the ring at key-up) and dropping them would
        // truncate every dictation by a few milliseconds.
        self.window.close(self.rt.frames_accepted());
        self.drain_consumer(CONSUMER_DRAIN_TIMEOUT);
        capture_level().store(0, Ordering::Relaxed);
        self.dsp
            .lock()
            .map(|mut dsp| dsp.take())
            .unwrap_or_else(|_| CapturedAudio {
                samples: Vec::new(),
                spill: None,
                sample_rate: TARGET_RATE,
                gain: 1.0,
                journal: None,
                device_failed: false,
                analysis: None,
            })
    }

    /// Wait (bounded) until the consumer has drained everything the callback
    /// has produced.
    fn drain_consumer(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while self.rt.samples.consumed() < self.rt.samples.written() {
            if Instant::now() >= deadline {
                log::warn!("capture consumer did not drain within {timeout:?}");
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn is_capturing(&self) -> bool {
        self.window.is_open()
    }

    /// Whether a take is in flight — what a reopen restores the gate to.
    fn is_armed(&self) -> bool {
        self.armed.load(Ordering::SeqCst)
    }

    fn has_failed(&self) -> bool {
        self.failed.load(Ordering::SeqCst)
    }

    /// The take's crash journal, if it has one — the watchdog writes the
    /// `device_change` marker through it.
    fn mark_device_change(&self, marker: &input_format::DeviceChangeMarker) {
        if let Ok(dsp) = self.dsp.lock() {
            if let Some(journal) = dsp.journal.as_ref() {
                journal.mark_device_change(&marker.to_json());
            }
        }
    }

    /// How many 16 kHz output samples the take has produced so far — the marker's
    /// segment boundary.
    fn output_samples(&self) -> u64 {
        self.dsp
            .lock()
            .map(|d| (d.spilled + d.out.len()) as u64)
            .unwrap_or(0)
    }

    /// YV92/OS-9 — fold one reading of "what does the OS say the input is now"
    /// into the stream's OWN watch, stamping it with this take's host time and
    /// output-sample boundary.
    ///
    /// This is the seam the watchdog runs on, and it is a method on the live
    /// stream precisely so the segment counter survives the tick: the second
    /// format change of a take must produce `segment_index: 2`, not another 1.
    /// `record::tests` drives it directly with a hardware-free `LiveStream`.
    fn observe_input(
        &mut self,
        device_name: String,
        format: InputFormat,
        source: FormatEventSource,
    ) -> FormatChangeAction {
        let observation = InputObservation {
            device_name,
            format,
            host_time: host_time_now(),
            output_sample_index: self.output_samples(),
            source,
        };
        self.watch.observe(observation)
    }

    /// The stream reopened: track what the HAL actually handed back, on both the
    /// stream and its watch, WITHOUT counting another segment (the change that
    /// caused the reopen was already counted, and a reopen after a plain stream
    /// error is not a segment boundary at all).
    fn adopt_reopened(&mut self, device_name: String, format: InputFormat) {
        self.watch.resync(device_name.clone(), format);
        self.device_name = device_name;
        self.format = format;
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

/// YV91 — hold the persistent input stream open for a meeting.
///
/// [`crate::meeting::MeetingSession::start`] calls this so a meeting owns its
/// audio source outright. Blocking (bounded by [`ARM_TIMEOUT`]) because a
/// meeting that cannot open the microphone must fail at start, loudly, rather
/// than record silence for an hour.
pub fn hold_stream_for_meeting() -> Result<InputFormat, String> {
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    dispatch(CaptureCmd::Hold { reply: reply_tx })?;
    reply_rx
        .recv_timeout(ARM_TIMEOUT)
        .map_err(|_| crate::permissions::named(NO_MIC_ERR))?
}

/// Release the meeting's hold. The stream stays open until the ordinary idle
/// close, so a dictation right after a meeting still pays nothing.
pub fn release_stream_for_meeting() {
    let _ = dispatch(CaptureCmd::Release);
}

/// Everything the idle-close decision depends on.
struct IdleInputs {
    stream_open: bool,
    /// Is a dictation take armed right now?
    capturing: bool,
    /// Has a meeting explicitly asked for the stream (YV91's `Hold`)?
    held_for_meeting: bool,
    /// Is a meeting registered at all? Belt and braces with `held_for_meeting`:
    /// the hold is the mechanism, this is the fact, and if they ever disagree
    /// the stream stays OPEN — losing a meeting's audio is unrecoverable and
    /// leaving the mic indicator lit for an extra minute is not.
    meeting_active: bool,
    idle_for: Duration,
    /// SP-09 — draining the battery: close at [`IDLE_CLOSE`] (60 s) instead of
    /// [`IDLE_CLOSE_AC`] (5 min).
    on_battery: bool,
}

/// Should the worker drop the persistent stream on this tick?
///
/// Pure, because this one boolean decides whether a three-hour meeting is a
/// three-hour recording or a sixty-second one, and the only other way to
/// exercise it is to sit in front of a microphone for a minute.
fn should_close_idle_stream(inputs: &IdleInputs) -> bool {
    inputs.stream_open
        && !inputs.capturing
        && !inputs.held_for_meeting
        && !inputs.meeting_active
        && inputs.idle_for >= idle_close_after(inputs.on_battery)
}

/// WF-09 — should Arm/Hold drop the warm stream because the user picked a
/// different microphone since it was opened?
///
/// The persistent stream outlives a take by up to [`IDLE_CLOSE`], so without
/// this a pick made in that window would only apply to the take AFTER the next
/// one. A stream that is mid-take or feeding a meeting is never dropped here:
/// losing a meeting's audio is unrecoverable, and the watchdog carries a live
/// capture across to the newly resolved device on its next tick anyway.
fn should_reopen_for_mic_pick(
    stream_open: bool,
    capturing: bool,
    held_for_meeting: bool,
    meeting_active: bool,
    opened_generation: u64,
    current_generation: u64,
) -> bool {
    stream_open
        && !capturing
        && !held_for_meeting
        && !meeting_active
        && opened_generation != current_generation
}

/// The Arm/Hold half of [`should_reopen_for_mic_pick`]: drop the warm stream
/// (and its cache entry) when the pick moved, logging the one decision.
fn drop_stream_if_mic_picked(
    live: &mut Option<LiveStream>,
    cache: &mut DeviceConfigCache<cpal::Device, cpal::SupportedStreamConfig>,
    held_for_meeting: bool,
    opened_generation: u64,
) {
    let current = crate::mic_select::generation();
    if should_reopen_for_mic_pick(
        live.is_some(),
        live.as_ref().is_some_and(LiveStream::is_capturing),
        held_for_meeting,
        crate::meeting::meeting_capture_active(),
        opened_generation,
        current,
    ) {
        log::info!("mic_select reopen reason=picked_since_open");
        *live = None;
        cache.invalidate();
    }
}

/// The worker thread: owns the persistent stream + the device/config cache and
/// answers every arm/stop with an explicit signal (never a poll).
fn capture_worker_loop(rx: mpsc::Receiver<CaptureCmd>) {
    let mut cache: DeviceConfigCache<cpal::Device, cpal::SupportedStreamConfig> =
        DeviceConfigCache::new();
    let mut live: Option<LiveStream> = None;
    let mut idle_since = Instant::now();
    let mut last_watchdog = Instant::now();
    // Is a meeting holding the stream open? (YV91.)
    let mut held_for_meeting = false;
    // WF-09 — the microphone-pick generation the open stream was opened at.
    let mut opened_generation = crate::mic_select::generation();

    loop {
        // YV81 — the tick exists ONLY to close an open stream that has gone
        // `IDLE_CLOSE` without a take. With no stream open there is nothing for
        // it to close, so the worker blocks on the channel instead of waking
        // every five seconds for the rest of the session: the next command
        // wakes it, which is the same signal-not-poll contract the arm/stop
        // path already keeps.
        let next = if let Some(stream) = live.as_ref() {
            // WF-10 — tick fast while audio is wanted, so a mic that vanishes
            // mid-take is replaced within [`ARMED_TICK`], not a minute later.
            let tick = if stream.is_armed() || held_for_meeting {
                ARMED_TICK
            } else {
                IDLE_TICK
            };
            rx.recv_timeout(tick)
        } else {
            rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        match next {
            Ok(CaptureCmd::Arm {
                journal,
                spill,
                analysis,
                reply,
            }) => {
                // A stream whose device errored (unplugged, format changed) is
                // useless — drop it and re-query the hardware from scratch.
                if live.as_ref().is_some_and(LiveStream::has_failed) {
                    log::warn!("capture stream reported a device error — reopening cold");
                    live = None;
                    cache.invalidate();
                }
                drop_stream_if_mic_picked(
                    &mut live,
                    &mut cache,
                    held_for_meeting,
                    opened_generation,
                );
                if live.is_none() {
                    // Read BEFORE resolving: a pick that lands mid-open makes
                    // the next Arm reopen rather than being silently missed.
                    let generation = crate::mic_select::generation();
                    match open_stream(&mut cache) {
                        Ok(stream) => {
                            live = Some(stream);
                            opened_generation = generation;
                        }
                        Err(e) => {
                            let _ = reply.send(Err(e));
                            continue;
                        }
                    }
                }
                if let Some(stream) = live.as_mut() {
                    if stream.is_capturing() {
                        log::warn!("arm while already capturing — dropping the orphaned take");
                    }
                    // WF-10 — a fresh take starts with no "continuing on" note.
                    clear_mid_take_note();
                    // WF-13 — a fresh take is judged on its own opening 1.5 s.
                    begin_silent_mic_take(&stream.device_name);
                    stream.begin_with(journal, spill.map(|s| *s), analysis);
                    let _ = reply.send(Ok(()));
                }
            }
            Ok(CaptureCmd::Disarm { reply }) => {
                idle_since = Instant::now();
                let captured = match live.as_ref() {
                    // YV67: the device-error flag used to be read only on the
                    // NEXT arm, so an unplugged mic produced a take that looked
                    // healthy and pasted a silently truncated transcript. Read
                    // it HERE, on the take it actually truncated — and still
                    // hand back every sample that was captured.
                    Some(stream) => Ok(mark_device_failure(stream.end(), stream.has_failed())),
                    None => Err(crate::permissions::named(NO_SAMPLES_ERR)),
                };
                let _ = reply.send(captured);
            }
            Ok(CaptureCmd::Hold { reply }) => {
                if live.as_ref().is_some_and(LiveStream::has_failed) {
                    log::warn!("capture stream reported a device error — reopening cold");
                    live = None;
                    cache.invalidate();
                }
                drop_stream_if_mic_picked(
                    &mut live,
                    &mut cache,
                    held_for_meeting,
                    opened_generation,
                );
                if live.is_none() {
                    // Read BEFORE resolving: a pick that lands mid-open makes
                    // the next Arm reopen rather than being silently missed.
                    let generation = crate::mic_select::generation();
                    match open_stream(&mut cache) {
                        Ok(stream) => {
                            live = Some(stream);
                            opened_generation = generation;
                        }
                        Err(e) => {
                            let _ = reply.send(Err(e));
                            continue;
                        }
                    }
                }
                held_for_meeting = true;
                let format = live
                    .as_ref()
                    .map(|s| s.format)
                    .unwrap_or_else(|| InputFormat::new(TARGET_RATE, 1));
                let _ = reply.send(Ok(format));
            }
            Ok(CaptureCmd::Release) => {
                held_for_meeting = false;
                idle_since = Instant::now();
            }
            Ok(CaptureCmd::PreArm { pressed_at }) => {
                if live.as_ref().is_some_and(LiveStream::has_failed) {
                    live = None;
                    cache.invalidate();
                }
                drop_stream_if_mic_picked(
                    &mut live,
                    &mut cache,
                    held_for_meeting,
                    opened_generation,
                );
                let warm = live.is_some();
                if live.is_none() {
                    let generation = crate::mic_select::generation();
                    match open_stream(&mut cache) {
                        Ok(stream) => {
                            live = Some(stream);
                            opened_generation = generation;
                        }
                        Err(e) => {
                            // The Arm that follows a real hold surfaces it.
                            log::warn!("record pre_arm_open_failed error={e}");
                            continue;
                        }
                    }
                }
                if let Some(stream) = live.as_mut() {
                    if !stream.is_armed() {
                        stream.note_press(pressed_at, warm);
                        log::debug!("record pre_armed warm={warm}");
                    }
                }
                // The press is activity: a cold-opened stream gets a full window.
                idle_since = Instant::now();
            }
            Ok(CaptureCmd::DiscardPreArm) => {
                if let Some(stream) = live.as_mut() {
                    if stream.discard_press() {
                        let n = PRE_ARM_DISCARDS.fetch_add(1, Ordering::Relaxed) + 1;
                        log::info!("record pre_arm_discarded reason=tap count={n}");
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // YV92 — the health watchdog. A dictation always has an Arm
                // coming to re-check the device; a long capture does not, so
                // this is the only thing standing between a mid-session format
                // change and three hours of half-speed audio nobody looks at
                // until stop. YV91 made the second half of that sentence
                // literal: a MEETING is the long capture, and it runs on this
                // same stream.
                let event = input_format::take_event();
                match live.as_mut() {
                    Some(stream) => {
                        // WF-10 — a dead stream (mic unplugged) is not left to
                        // wait for the 60 s tick: it is replaced at once, and
                        // retried at most every [`FAILED_RETRY`].
                        let failed_due =
                            stream.has_failed() && last_watchdog.elapsed() >= FAILED_RETRY;
                        if event.is_some() || failed_due || last_watchdog.elapsed() >= WATCHDOG_TICK
                        {
                            last_watchdog = Instant::now();
                            watchdog_poll(stream, &mut cache, event);
                        }
                    }
                    // No stream to reconfigure, but the HAL still said the input
                    // moved — and the cached config is exactly what the next
                    // cold Arm would open with. Drop it (YV92/OS-9).
                    None if event.is_some() => cache.invalidate(),
                    None => {}
                }
                let idle_for = idle_since.elapsed();
                // SP-09 — the power source decides the window; read (IOKit)
                // only once the 60 s battery floor has passed.
                let capturing = live.as_ref().is_some_and(LiveStream::is_capturing);
                let on_battery = live.is_some()
                    && !capturing
                    && !held_for_meeting
                    && idle_for >= IDLE_CLOSE
                    && {
                        let b = crate::meeting_energy::battery();
                        b.has_battery && !b.on_ac
                    };
                if should_close_idle_stream(&IdleInputs {
                    stream_open: live.is_some(),
                    capturing,
                    held_for_meeting,
                    meeting_active: crate::meeting::meeting_capture_active(),
                    idle_for,
                    on_battery,
                }) {
                    live = None;
                    close_idle_capture(&mut cache);
                    log::info!(
                        "capture stream closed after {}s idle on_battery={on_battery} (mic indicator off)",
                        idle_close_after(on_battery).as_secs()
                    );
                }
            }
            // Every sender is gone (process teardown) — release the stream.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                input_format::disarm_listeners();
                return;
            }
        }
    }
}

/// The idle close (YV81): the persistent stream has gone [`IDLE_CLOSE`] without
/// a take, so it is dropped to put the macOS mic indicator out.
///
/// YV92/OS-9 — dropping the stream also ends the ONLY thing watching the input
/// format: the listeners come off with it, and the worker goes back to blocking
/// on its channel, so nothing observes the HAL again until the next Arm. Anything
/// the cache still holds is therefore a snapshot of a world no one has looked at
/// since, which is precisely the AirPods hole: idle-close at 60 s, the user puts
/// the AirPods in and their mic renegotiates to HFP while nothing is open, and
/// the next Arm opens from a cached 48 kHz config the device no longer runs —
/// with no live stream, there is no watchdog to catch it afterwards either.
///
/// So the cache dies with the stream. That costs the next cold Arm one
/// `default_input_config()` round-trip; it does not touch YV35's warm path,
/// where a take inside the idle window never calls the opener at all.
fn close_idle_capture<D: Clone, C: Clone>(cache: &mut DeviceConfigCache<D, C>) {
    input_format::disarm_listeners();
    cache.invalidate();
}

/// The cache half of the open path: the cached device + config for `name`, or
/// the hardware's answer, plus whether it was a hit.
///
/// Split out — and generic over the device/config types — so the case the cache
/// exists to get wrong is testable without cpal: a same-named device that
/// renegotiated its rate while nothing was open must not be served from the
/// cache. Any failed query invalidates, so a device that has gone away can never
/// leave a live-looking entry behind.
fn resolve_device_config<D: Clone, C: Clone>(
    cache: &mut DeviceConfigCache<D, C>,
    name: &str,
    device: D,
    query: impl FnOnce(&D) -> Result<C, String>,
) -> Result<(D, C, bool), String> {
    if let Some(hit) = cache.get(name, None) {
        return Ok((hit.0, hit.1, true));
    }
    match query(&device) {
        Ok(config) => Ok((device, config, false)),
        Err(e) => {
            cache.invalidate();
            Err(e)
        }
    }
}

/// YV92/OS-9 — one pass of the capture health watchdog, on the
/// [`WATCHDOG_TICK`] cadence (or immediately, when the CoreAudio listener says
/// something moved).
///
/// Two questions, both of which used to be asked only at Arm and Disarm — i.e.
/// never, during a capture that runs for hours:
///
/// 1. **Did the stream die?** `LiveStream::has_failed` is the flag cpal's error
///    callback sets. Asking it here is fix (d) of OS-9: a dead stream is now
///    discovered within a tick instead of at stop.
/// 2. **Did the input format change?** The default input device and its nominal
///    rate/channels are re-read and fed to the pure state machine
///    (`input_format::InputFormatWatch`). On a real change the take is carried
///    across to a stream reopened at the NEW rate, and a `device_change` marker
///    goes into the journal at the exact output-sample boundary.
fn watchdog_poll(
    live: &mut LiveStream,
    cache: &mut DeviceConfigCache<cpal::Device, cpal::SupportedStreamConfig>,
    event: Option<FormatEventSource>,
) {
    let failed = live.has_failed();
    if failed {
        log::warn!(
            "YV92 watchdog: capture stream on {} reported a device error — reopening",
            live.device_name
        );
    }
    let observed = match chosen_input_device() {
        Ok((device, name)) => device.default_input_config().ok().map(|config| {
            (
                name,
                InputFormat::new(config.sample_rate(), config.channels()),
            )
        }),
        Err(e) => {
            log::warn!("YV92 watchdog: no input device ({e})");
            None
        }
    };
    let Some((name, format)) = observed else {
        return;
    };

    // The watch belongs to the STREAM, not to this tick: building one here
    // would restart the segment counter every 60 s and stamp every marker of a
    // take `segment_index: 1`.
    let action = live.observe_input(name, format, event.unwrap_or(FormatEventSource::Watchdog));
    match action {
        FormatChangeAction::Unchanged if !failed => {}
        FormatChangeAction::Ignored(why) => {
            log::debug!("YV92 watchdog: ignoring an input-format reading — {why}");
        }
        FormatChangeAction::Unchanged => {
            // The format is fine but the stream is dead: reopen at the same
            // format rather than leaving a silent take running.
            reopen_after_change(live, cache, None);
        }
        FormatChangeAction::Reconfigure { marker, ratio } => {
            log::warn!(
                "YV92 watchdog: input format changed ({} {}Hz/{}ch → {} {}Hz/{}ch, source={}) — \
                 segment {} opens at output sample {}, new resample ratio {:.4}",
                marker.from_device,
                marker.from.sample_rate_hz,
                marker.from.channels,
                marker.to_device,
                marker.to.sample_rate_hz,
                marker.to.channels,
                marker.source.as_str(),
                marker.segment_index,
                marker.output_sample_index,
                ratio.value(),
            );
            live.mark_device_change(&marker);
            reopen_after_change(live, cache, Some(marker.to));
        }
        // YV103 — the output half of the same watch. The mic path never calls
        // `watch_output`, so its watch's output half is inert and this arm is
        // unreachable from `observe_input`; the aggregate rebuild belongs to
        // 22-B's tap session, which drives `observe_output`/`tick_output` on its
        // own watch. Logged rather than asserted so a future wiring mistake
        // shows up in the support bundle instead of panicking a user's capture.
        FormatChangeAction::RebuildAggregate { marker, reason } => {
            log::warn!(
                "YV92 watchdog: ignoring an aggregate-rebuild action on the mic path \
                 ({} → {}, reason={}) — the mic stream has no aggregate device",
                marker.from_device,
                marker.to_device,
                reason.as_str(),
            );
        }
    }
}

/// Swap the live device out from under a take without losing the take.
///
/// Order matters and is the whole point: pause the gate, close the OLD device
/// (a cpal stream is bound to the format it was built with, so there is no
/// in-place reconfigure), invalidate the name-keyed cache so the reopen cannot
/// come back with the stale rate, then reopen handing the SAME DSP across. If
/// the reopen fails, the take's audio is still in that DSP — the stream slot is
/// simply left empty and the flag left set, so Disarm still returns every sample
/// captured before the change and the next Arm opens cold.
fn reopen_after_change(
    live: &mut LiveStream,
    cache: &mut DeviceConfigCache<cpal::Device, cpal::SupportedStreamConfig>,
    to: Option<InputFormat>,
) {
    reopen_with(live, to, |reuse| {
        cache.invalidate();
        open_stream_into(cache, Some(reuse))
    });
}

/// The device-independent half of [`reopen_after_change`]: everything that
/// decides what happens to the TAKE, with the cpal open passed in.
///
/// Split out because that is the half the fail-then-succeed path lives in and
/// the half a microphone is not needed for — see
/// `a_transient_reopen_failure_still_resumes_the_take`.
///
/// The take is resumed from its ARMED INTENT, never from the momentary
/// `is_capturing()` this function just closed. Reading the window to decide
/// what to restore the window to is only correct while every reopen succeeds:
/// after a failed attempt it is already closed, so the self-healing retry on
/// the next watchdog tick would "restore" closed onto a perfectly live stream
/// and silently drop the rest of the take (YV92 review).
///
/// **The YV91 rebase changed what "restore" means, and this is the seam.**
/// YV92's gate was one `AtomicBool` shared with the callback, so a reopen
/// restored a boolean. YV91 replaced it with an arm WINDOW over cumulative
/// frame indices, and those indices belong to the RING — which a reopen
/// rebuilds, restarting the count at zero. Copying the old window across (or
/// simply leaving it in place) leaves `from` at a frame index the new stream
/// will not reach for another N hours, so `armed_range` returns `None` for
/// every block and the rest of a three-hour take is routed to nobody with
/// nothing logged: OS-9's exact failure mode, re-created by the composition of
/// two fixes that are each correct alone. The window is therefore re-OPENED at
/// the new ring's origin, and
/// `a_reopen_mid_take_rearms_the_window_in_the_new_rings_frame_space` is the
/// regression guard.
fn reopen_with(
    live: &mut LiveStream,
    to: Option<InputFormat>,
    open: impl FnOnce(TakeInFlight) -> Result<LiveStream, String>,
) {
    // Close the window at the last frame this device delivered and let the
    // consumer work through what is still in the ring: those frames are audio
    // the take already paid for, and the device going away is no reason to
    // drop them.
    live.window.close(live.rt.frames_accepted());
    live.drain_consumer(CONSUMER_DRAIN_TIMEOUT);
    live.stream = None;
    let reuse = (live.dsp.clone(), live.failed.clone());
    let from_device = live.device_name.clone();
    // Read BEFORE the open: a successful reopen clears the shared flag.
    let device_lost = live.has_failed();
    match open(reuse) {
        Ok(reopened) => {
            // WF-10 — the take is moving to another mic: say so in the pill.
            let switched = is_mid_take_switch(&from_device, &reopened.device_name, live.is_armed());
            live.stream = reopened.stream;
            // The rings, the window and the thread draining them all belong to
            // the DEVICE, so they are adopted wholesale. Assigning the consumer
            // handle stops and joins the old thread (see [`ConsumerHandle`]).
            live.rt = reopened.rt;
            live.window = reopened.window;
            live.consumer = reopened.consumer;
            // SP-09 — the pre-roll is indexed in the old ring's frames: adopt
            // the new device's empty one, and forget a press stamped there.
            live.pre_roll = reopened.pre_roll;
            live.pending_press = None;
            // `reopened.watch` is discarded on purpose — the take's segment
            // numbering lives on `live`, and the reopen only tells it what
            // actually opened.
            live.adopt_reopened(reopened.device_name, reopened.format);
            if live.is_armed() {
                // Frame zero OF THE NEW RING: every frame this device delivers
                // from now on belongs to the take that is still running.
                live.window.open(0);
            }
            if switched {
                retarget_silent_mic(&live.device_name);
                note_mid_take_switch(&live.device_name, device_lost);
                log::warn!(
                    "mic_select switched_mid_take device_lost={device_lost} kept_samples={}",
                    live.output_samples()
                );
            }
            // A meeting is a SECOND consumer of this device, with its own
            // resampler, and YV92's rule — a ratio must never survive a format
            // change — applies to it too. It also has its own cumulative frame
            // accounting, which the new ring just reset; telling it about the
            // reopen is what keeps its gap detector from reading that reset as
            // an hour of lost audio.
            crate::meeting::retune_active_capture(live.format.sample_rate_hz, live.format.channels);
            log::info!(
                "YV92 watchdog: capture continues on {} at {}Hz/{}ch (armed={})",
                live.device_name,
                live.format.sample_rate_hz,
                live.format.channels,
                live.is_armed(),
            );
        }
        Err(e) => {
            // Keep the failure visible: Disarm stamps it onto the take (YV67),
            // the watchdog retries on its next tick — and because the intent
            // outlives this attempt, that retry reopens the gate for the take
            // that is still running.
            live.failed.store(true, Ordering::SeqCst);
            if let Some(to) = to {
                live.format = to;
            }
            log::error!(
                "YV92 watchdog: could not reopen the input after a format change ({e}) — \
                 the {} sample(s) already captured are kept, retrying on the next tick",
                live.output_samples()
            );
        }
    }
}

/// The mach host time the format change was observed at — YV91's capture anchor
/// as far as this item needs it: a monotonic tick that a later index record can
/// be lined up against. `mach_absolute_time` on macOS, a monotonic nanosecond
/// count elsewhere so the marker shape is identical on every platform.
fn host_time_now() -> u64 {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn mach_absolute_time() -> u64;
        }
        // SAFETY: no arguments, no pointers, always available on macOS.
        unsafe { mach_absolute_time() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        static EPOCH: OnceLock<Instant> = OnceLock::new();
        EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
    }
}

/// Open the input stream, reusing the cached device + config when the system
/// default is the same one we opened last time. Any failure invalidates the
/// cache so the next attempt re-queries the hardware.
fn open_stream(
    cache: &mut DeviceConfigCache<cpal::Device, cpal::SupportedStreamConfig>,
) -> Result<LiveStream, String> {
    open_stream_into(cache, None)
}

/// The input device the next take opens and the name it reports, or the "no
/// mic" error. Split out so the format watchdog can ask the same question the
/// opener asks without duplicating the device rules.
///
/// WF-09 — this is no longer always the system default: the microphone picker
/// (`mic_select`) may pin a device by its stable id, and a pinned device that
/// is not plugged in falls back to the system default
/// (`mic_select::resolve`). With nothing pinned it is the old default path.
fn chosen_input_device() -> Result<(cpal::Device, String), String> {
    crate::mic_select::input_device_for_next_take()
        .ok_or_else(|| crate::permissions::named(NO_MIC_ERR))
}

/// The three things a reopened stream inherits from the take that was already
/// running: the DSP (which owns the buffered audio, the AGC statistics and the
/// crash journal), the capture gate, and the error flag. Named so the reopen
/// signature reads as "carry the take across", not as a tuple of Arcs.
type TakeInFlight = (Arc<Mutex<StreamDsp>>, Arc<AtomicBool>);

/// Open the input stream, optionally HANDING IT AN EXISTING TAKE.
///
/// `reuse` is `Some` on exactly one path: YV92's format-change recovery, where
/// the device must be reopened at a new rate *without* interrupting the take in
/// flight. The DSP (and therefore the buffered audio, the AGC statistics and the
/// crash journal) is carried across and only its rate-dependent state is rebuilt
/// — see [`StreamDsp::reconfigure`]. On a cold open (`None`) everything is
/// fresh, which is exactly what it was before this item.
fn open_stream_into(
    cache: &mut DeviceConfigCache<cpal::Device, cpal::SupportedStreamConfig>,
    reuse: Option<TakeInFlight>,
) -> Result<LiveStream, String> {
    let opened_at = Instant::now();
    let (device, dev_name) = chosen_input_device()?;

    let (device, supported, was_cached) = resolve_device_config(
        cache,
        &dev_name,
        device,
        |device| {
            device.default_input_config().map_err(|e| {
                crate::permissions::named(&format!(
                    "Mic config failed ({e}). Enable Microphone for Yap (not Python) in System Settings."
                ))
            })
        },
    )?;

    let sample_rate = supported.sample_rate();
    let channels = supported.channels();
    let sample_format = supported.sample_format();
    let conf: cpal::StreamConfig = supported.into();

    let format = InputFormat::new(sample_rate, channels);
    let (dsp, failed) = match reuse {
        Some((dsp, failed)) => {
            // The take survives the device: only the rate-dependent state is
            // rebuilt, and the error flag is cleared because it belonged to the
            // stream we just closed.
            if let Ok(mut dsp) = dsp.lock() {
                dsp.reconfigure(sample_rate, channels);
            }
            failed.store(false, Ordering::SeqCst);
            (dsp, failed)
        }
        None => (
            Arc::new(Mutex::new(StreamDsp::new(sample_rate, channels))),
            Arc::new(AtomicBool::new(false)),
        ),
    };
    // YV91/OS-7: everything the callback touches is allocated HERE, on this
    // (non-real-time) thread, and never grows again. It is NEVER reused across
    // a reopen — the ring's geometry is the device's, and its frame counters
    // restart with the stream (which is what `reopen_with` re-opens the arm
    // window against).
    let rt = Arc::new(crate::meeting::RtCapture::new(sample_rate, channels));

    let stream = match sample_format {
        cpal::SampleFormat::F32 => build_capture_stream::<f32>(&device, conf, &rt, &failed),
        cpal::SampleFormat::I16 => build_capture_stream::<i16>(&device, conf, &rt, &failed),
        cpal::SampleFormat::U16 => build_capture_stream::<u16>(&device, conf, &rt, &failed),
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
        return Err(crate::permissions::named(&format!(
            "mic start failed ({e}). Allow Microphone for Yap."
        )));
    }

    log::info!(
        "mic device={dev_name} format={sample_format:?} rate={sample_rate} ch={channels} cached_config={was_cached} open_ms={}",
        opened_at.elapsed().as_millis()
    );
    // The device accepted this config — remember it (with the FORMAT it was
    // accepted at, YV92) so the next reopen skips the HAL property queries
    // entirely while a renegotiated rate still misses.
    cache.store(dev_name.clone(), format, device, supported);
    // YV92 — the HAL will now tell us the moment this device's format changes
    // or the default input moves, instead of the session discovering it at stop.
    if !input_format::arm_listeners() {
        log::debug!(
            "YV92 input-format listeners unavailable — falling back to the {}s watchdog re-read",
            WATCHDOG_TICK.as_secs()
        );
    }

    let window = Arc::new(ArmWindow::idle());
    let pre_roll = Arc::new(Mutex::new(PreRollState::new(
        format.sample_rate_hz,
        format.channels,
    )));
    let consumer_stop = Arc::new(AtomicBool::new(false));
    let consumer = {
        let (rt, dsp, window, pre_roll, stop, level) = (
            rt.clone(),
            dsp.clone(),
            window.clone(),
            pre_roll.clone(),
            consumer_stop.clone(),
            capture_level().clone(),
        );
        thread::Builder::new()
            .name("wv-capture-consumer".into())
            .spawn(move || capture_consumer_loop(rt, dsp, window, pre_roll, stop, level))
            .ok()
    };

    Ok(LiveStream {
        stream: Some(stream),
        dsp,
        window,
        // A freshly opened stream is not itself a take. On the reuse path the
        // caller (`reopen_with`) owns the armed intent and re-opens the window
        // from ITS copy — this one is discarded with the rest of the shell.
        armed: AtomicBool::new(false),
        failed,
        rt,
        consumer: ConsumerHandle {
            stop: consumer_stop,
            join: consumer,
        },
        watch: InputFormatWatch::new(dev_name.clone(), format, TARGET_RATE),
        device_name: dev_name,
        format,
        pre_roll,
        pending_press: None,
    })
}

/// Build the input stream for one sample format.
///
/// YV91 (plan finding OS-7) rewrote this callback. It used to `extend` a
/// `Vec`, take a `std::sync::Mutex` and run the whole DSP chain — allocation,
/// a blocking lock and two unbounded buffers — on the audio thread. A
/// five-second dictation never exposed that; three continuous hours does, and
/// 22-B runs the same body on a CoreAudio tap's IOProc where a missed deadline
/// is an audible glitch in the call the user is listening to.
///
/// So the callback now does exactly three things and returns: one host-time
/// anchor into the anchor ring, the frames into the sample ring, one counter
/// bump — see [`crate::meeting::rt_capture_callback`], which is where the body
/// lives so it can be driven directly by a test with a counting allocator
/// installed. Every bit of DSP moved to [`capture_consumer_loop`].
///
/// The frames enter the ring UNCONDITIONALLY now, not only while a take is
/// armed: a meeting recording has no Arm at all, and the consumer decides who
/// each block belongs to (see the arm window in [`LiveStream::begin`]).
///
/// A stream error flips `failed`, which makes the next arm reopen — and, per
/// OS-9, is what YV92 polls on the meeting watchdog rather than only at Arm.
fn build_capture_stream<T>(
    device: &cpal::Device,
    conf: cpal::StreamConfig,
    rt: &Arc<crate::meeting::RtCapture>,
    failed: &Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let rt = rt.clone();
    let failed_cb = failed.clone();
    let mut epoch: Option<cpal::StreamInstant> = None;

    device
        .build_input_stream(
            conf,
            move |data: &[T], info: &cpal::InputCallbackInfo| {
                // The parameter this callback used to bind to `_`. It is the
                // instant the ADC captured these frames, and it is the anchor
                // OS-2 (cross-track alignment), finding #12 (gap detection) and
                // finding #3 (resume) all need. Rebased to the first callback
                // so the number is a stream-relative nanosecond clock rather
                // than an absolute mach tick nobody downstream can interpret.
                let captured_at = info.timestamp().capture;
                let base = *epoch.get_or_insert(captured_at);
                let host_ns = captured_at.duration_since(base).as_nanos() as u64;
                crate::meeting::rt_capture_callback(&rt, data, |s| s.to_sample::<f32>(), host_ns);
            },
            move |e| {
                log::error!("cpal stream error: {e}");
                failed_cb.store(true, Ordering::SeqCst);
            },
            None,
        )
        .map_err(|e| format!("mic stream: {e}"))
}

/// The normal-priority consumer that drains the capture rings (YV91 / OS-7).
///
/// Everything the audio callback used to do lives here: the HUD meter, the
/// streaming DSP, and — new — the meeting fan-out. One drained block is served
/// to BOTH consumers ([`crate::meeting::fan_out_block`]), which is finding
/// #2a's fix: a dictation take started mid-meeting can neither reset nor gap
/// the meeting, because the meeting was never in `StreamDsp` to begin with.
///
/// Which frames belong to the dictation take is decided by the arm WINDOW
/// (frame indices), not by a flag read at drain time. A flag would lose the
/// tail of every take — the frames still sitting in the ring when the user
/// releases the key.
fn capture_consumer_loop(
    rt: Arc<crate::meeting::RtCapture>,
    dsp: Arc<Mutex<StreamDsp>>,
    window: Arc<ArmWindow>,
    pre_roll: Arc<Mutex<PreRollState>>,
    stop: Arc<AtomicBool>,
    level: LevelHandle,
) {
    let channels = rt.channels().max(1) as usize;
    // WF-13 — the silent-mic check for the take this consumer is serving,
    // keyed on the arm window's start so every new take gets a fresh one.
    let mut silent_check: Option<(u64, SilentMicCheck)> = None;
    let mut block: Vec<f32> = Vec::with_capacity(crate::meeting::RING_FRAMES);
    let mut anchors: Vec<crate::rtring::CaptureAnchor> =
        Vec::with_capacity(crate::meeting::ANCHOR_RING_LEN);
    // Frames (per-channel) this consumer has already taken.
    let mut pos: u64 = 0;
    loop {
        block.clear();
        anchors.clear();
        rt.anchors.drain_into(&mut anchors);
        let taken = rt.samples.drain_into(&mut block);
        if taken == 0 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(CONSUMER_IDLE_SLEEP);
            continue;
        }
        let frames = (taken / channels) as u64;
        // The dictation take's slice of this block, if any.
        let (from, to) = window.bounds();
        // SP-09 — a window opened behind `pos` (at the key-down, less the
        // pre-roll) first gets the frames this consumer already drained.
        replay_pre_roll(&pre_roll, &dsp, pos, from, to);
        let armed_slice = armed_range(pos, frames, channels, from, to);

        let mut guard = armed_slice
            .as_ref()
            .and_then(|_| dsp.lock().ok())
            .map(DictationDsp);
        crate::meeting::fan_out_block(
            &block,
            &anchors,
            guard.as_mut().map(|g| {
                let sink: &mut dyn crate::meeting::DictationSink = g;
                sink
            }),
            armed_slice.clone(),
        );
        drop(guard);

        if let Some(range) = armed_slice {
            if silent_check.as_ref().map(|(at, _)| *at) != Some(from) {
                silent_check = Some((from, SilentMicCheck::new(rt.sample_rate(), channels)));
            }
            if let Some((_, check)) = silent_check.as_mut() {
                apply_silent_mic_event(check.feed(&block[range.clone()]));
            }
            push_level(&level, &block[range]);
        } else if crate::meeting::meeting_capture_active() {
            push_level(&level, &block);
        }
        remember_block(&pre_roll, pos, &block);
        pos += frames;
    }
}

/// Which part of a drained block belongs to the armed take — the intersection
/// of the block's frames `[pos, pos + frames)` with the arm window
/// `[from, to)`, expressed as an index range into the INTERLEAVED block.
///
/// Pure, because this is the one piece of arithmetic in the new capture path
/// that can silently truncate or over-copy a dictation, and the only way to
/// exercise it otherwise is with a live microphone.
///
/// `u64::MAX` for either bound means "not armed" / "still open", which the
/// saturating comparisons below handle without a special case.
fn armed_range(
    pos: u64,
    frames: u64,
    channels: usize,
    from: u64,
    to: u64,
) -> Option<std::ops::Range<usize>> {
    let lo = from.max(pos);
    let hi = to.min(pos + frames);
    if lo >= hi {
        return None;
    }
    let a = ((lo - pos) as usize) * channels;
    let b = ((hi - pos) as usize) * channels;
    Some(a..b)
}

/// Adapter: the dictation DSP as a fan-out sink. The lock is taken by the
/// consumer, off the audio thread, which is the whole point of OS-7's fix.
struct DictationDsp<'a>(std::sync::MutexGuard<'a, StreamDsp>);

impl crate::meeting::DictationSink for DictationDsp<'_> {
    fn push_native(&mut self, interleaved: &[f32]) {
        self.0.push(interleaved);
    }
}

/// Which frames belong to the take that is currently armed, as a half-open
/// window of cumulative per-channel frame indices. `to == u64::MAX` means the
/// take is still open.
struct ArmWindow {
    from: AtomicU64,
    to: AtomicU64,
}

impl ArmWindow {
    fn idle() -> Self {
        Self {
            from: AtomicU64::new(u64::MAX),
            to: AtomicU64::new(u64::MAX),
        }
    }

    fn bounds(&self) -> (u64, u64) {
        (
            self.from.load(Ordering::Acquire),
            self.to.load(Ordering::Acquire),
        )
    }

    fn open(&self, at: u64) {
        self.to.store(u64::MAX, Ordering::Release);
        self.from.store(at, Ordering::Release);
    }

    fn close(&self, at: u64) {
        self.to.store(at, Ordering::Release);
    }

    fn is_open(&self) -> bool {
        let (from, to) = self.bounds();
        from != u64::MAX && to == u64::MAX
    }
}

/// How long the consumer naps when the ring is empty. Short enough that the HUD
/// meter and the release→ASR gap are unchanged; long enough that an idle-but-
/// open stream costs nothing measurable.
const CONSUMER_IDLE_SLEEP: Duration = Duration::from_millis(2);

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

// ── Bounded capture: the take lives on disk, not in RAM (Y3) ───────────────
//
// Before Y3 a take grew TWO unbounded `Vec<f32>`s for as long as the user held
// the key: `out` (16 kHz, 64 KB/s) and `raw` (native rate, 192 KB/s at 48 kHz).
// A twenty-minute dictation was ~307 MB of resident audio before a single DSP
// stage ran, and `finalize_take` then took the whole thing by value. Nothing
// bounded either buffer; the only ceiling was the user's patience.
//
// Now the consumer thread — NOT the audio callback, which still only copies
// into the ring (see `rtring.rs`) — drains `out` into the take's own WAV as it
// goes and keeps a bounded working window in RAM. `finalize_take` reads the
// take back with the `read_wav_16k_mono` that already existed. The raw
// native-rate fallback Vec is gone entirely: the spill IS the never-lose-audio
// copy, and a file on disk survives strictly more than a Vec does.

/// The ceiling on the audio buffers `StreamDsp` keeps resident for a take of
/// **any** length — the number `tests/dictation_capture_memory.rs` asserts
/// against, counted the same way `tests/meeting_capture_memory.rs` counts a
/// meeting's: the capacity of every `f32` buffer the take owns (`out`, the
/// downmix scratch `mono`, and the resampler's retained `pending` + `scratch`).
///
/// 4 MiB, which is ~8% of the ~32 MB the item budgets for capture and leaves
/// the rest of that budget to the read-back `finalize_take` does once at the
/// end. It is a ceiling, not a target: steady state sits near
/// [`SPILL_WINDOW_SAMPLES`] plus one callback of scratch, roughly 200 KB.
pub const MAX_RESIDENT_CAPTURE_BYTES: usize = 4 * 1024 * 1024;

/// How much finished 16 kHz audio stays in RAM behind the spill cursor.
///
/// Sized from the DSP's *actual* lookahead, not from a round number. Of the
/// three stages that run over a finished take — `high_pass` (a biquad, one
/// sample of state, carried across frames during capture), `normalize_rms`
/// (two passes over statistics this path already accumulates incrementally)
/// and `denoise_rnnoise` (RNNoise, which is strictly frame-local: 480 samples
/// at 48 kHz, i.e. **160 samples at 16 kHz**, 10 ms) — the largest true
/// dependency is one RNNoise frame, 160 samples.
///
/// 16 000 samples (one second, 64 000 bytes) is 100 whole RNNoise frames, so
/// the retained window can never split one, and it keeps the spill to one
/// write per second of speech instead of one per callback. That is a hundredfold
/// margin over the dependency and still 1/64th of [`MAX_RESIDENT_CAPTURE_BYTES`].
const SPILL_WINDOW_SAMPLES: usize = TARGET_RATE as usize;

/// Suffix of a take's capture spill: `<id>.capture.wav`.
///
/// A file of its own, deliberately NOT the history/recovery WAV (`ClipWav`,
/// still i16 and still written at the end) and NOT the YV63 journal spill
/// (`<id>.spill.pcm`, i16, lossy under backpressure by design). This one is
/// 32-bit float so the read-back is bit-exact — an i16 round-trip would quietly
/// requantise every take, which is the regression `dictation_capture_memory`'s
/// byte-identity test exists to catch.
const CAPTURE_SPILL_EXT: &str = "capture.wav";

/// The WAV spec every capture spill is written with. 16 kHz mono f32 — the
/// same rate and channel count `out` already holds, so spilling is a copy and
/// never a conversion.
fn capture_spill_spec() -> hound::WavSpec {
    hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    }
}

/// A finished capture spill: the take, complete, on disk.
pub struct CaptureSpill {
    path: PathBuf,
}

impl CaptureSpill {
    /// Read the whole take back as 16 kHz mono floats. This is the one place
    /// the take is fully resident, and it happens once, after the key is up —
    /// not for every callback while the user is still speaking.
    fn read_back(&self) -> Result<Vec<f32>, String> {
        read_wav_16k_mono(&self.path)
    }
}

impl Drop for CaptureSpill {
    fn drop(&mut self) {
        // The take made it out of capture and `finalize_take` has read it, so
        // the spill has done its job. Same disposability rule as `ClipWav`.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Writes one take's 16 kHz frames to `<recordings>/<id>.capture.wav` as they
/// arrive, on the capture CONSUMER thread (normal priority, already holding the
/// DSP mutex) — never on the audio callback, which still does nothing but copy
/// into the ring.
///
/// Failure is not fatal and is never silent: the first write error abandons the
/// spill and flips `failed`, and `StreamDsp` then stops draining `out`, which
/// degrades to exactly the pre-Y3 unbounded-RAM behaviour rather than losing
/// the utterance. A disk that has gone away costs memory, not words.
struct SpillWriter {
    writer: Option<hound::WavWriter<std::io::BufWriter<std::fs::File>>>,
    path: PathBuf,
    /// `data_dir()/recovery/` — where an ABORTED take's spill is moved so it
    /// sits alongside the YV63 journal artefacts the retry path already reads.
    recovery_dir: PathBuf,
    written: usize,
    failed: bool,
    finished: bool,
}

impl SpillWriter {
    /// Open the spill for the take whose history WAV will be `wav_path`.
    /// `None` on any failure — a take with no spill simply keeps its audio in
    /// RAM the old way.
    fn open(wav_path: &Path, recovery_dir: &Path) -> Option<Self> {
        let path = wav_path.with_extension(CAPTURE_SPILL_EXT);
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("Y3 capture spill off for this take (dir): {e}");
                return None;
            }
        }
        match hound::WavWriter::create(&path, capture_spill_spec()) {
            Ok(writer) => Some(Self {
                writer: Some(writer),
                path,
                recovery_dir: recovery_dir.to_path_buf(),
                written: 0,
                failed: false,
                finished: false,
            }),
            Err(e) => {
                log::warn!("Y3 capture spill off for this take: {e}");
                None
            }
        }
    }

    /// Append finished 16 kHz samples. Returns `false` once the spill has
    /// failed, which is the caller's signal to stop draining its buffer.
    fn append(&mut self, samples: &[f32]) -> bool {
        if self.failed {
            return false;
        }
        let Some(writer) = self.writer.as_mut() else {
            self.failed = true;
            return false;
        };
        for &s in samples {
            if let Err(e) = writer.write_sample(s) {
                log::warn!(
                    "Y3 capture spill failed after {} samples ({e}) — the take stays in RAM",
                    self.written
                );
                self.failed = true;
                return false;
            }
        }
        self.written += samples.len();
        true
    }

    /// Close the WAV and hand back the finished take. `None` when the spill
    /// failed or never received a sample, in which case the caller's in-memory
    /// buffer is still the take.
    fn finish(mut self) -> Option<CaptureSpill> {
        self.finished = true;
        let writer = self.writer.take()?;
        if self.failed || self.written == 0 {
            drop(writer);
            let _ = std::fs::remove_file(&self.path);
            return None;
        }
        match writer.finalize() {
            Ok(()) => Some(CaptureSpill {
                path: self.path.clone(),
            }),
            Err(e) => {
                log::warn!("Y3 capture spill could not be closed: {e}");
                let _ = std::fs::remove_file(&self.path);
                None
            }
        }
    }
}

impl Drop for SpillWriter {
    /// A take that died mid-spill — the stream was torn down, the take was
    /// abandoned, the process is unwinding. The WAV header is finalized so the
    /// prefix on disk is a PLAYABLE file, and it is moved into the recovery dir
    /// next to the YV63 journal's artefacts. Its name (`<id>.capture.wav`) is
    /// not a journal marker, so `recover_orphaned_journals` walks straight past
    /// it and the two recovery mechanisms cannot confuse each other.
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let Some(writer) = self.writer.take() else {
            return;
        };
        if self.failed || self.written == 0 || writer.finalize().is_err() {
            let _ = std::fs::remove_file(&self.path);
            return;
        }
        if std::fs::create_dir_all(&self.recovery_dir).is_err() {
            return;
        }
        let Some(name) = self.path.file_name() else {
            return;
        };
        let dest = self.recovery_dir.join(name);
        if std::fs::rename(&self.path, &dest).is_err() {
            if std::fs::copy(&self.path, &dest).is_ok() {
                let _ = std::fs::remove_file(&self.path);
            }
            return;
        }
        log::warn!(
            "Y3 take aborted mid-spill — {} samples kept at {}",
            self.written,
            dest.display()
        );
    }
}

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
    /// 16 kHz mono, high-passed — the bounded working window (Y3). Everything
    /// behind [`SPILL_WINDOW_SAMPLES`] has already gone to `spill`; what is
    /// here is the tail, plus whatever has accumulated since the last drain.
    /// With no spill (open failed, or a write failed mid-take) this is the
    /// whole take, exactly as it was before Y3.
    out: Vec<f32>,
    /// This take's on-disk spill (Y3). `None` means the take is riding in RAM.
    spill: Option<SpillWriter>,
    /// 16 kHz samples already drained out of `out` into `spill` — the offset
    /// that keeps `output_samples()` (and therefore every journal marker's
    /// segment boundary) counting the whole take and not just the window.
    spilled: usize,
    /// Reused downmix scratch so a callback allocates nothing steady-state.
    mono: Vec<f32>,
    /// YV63 crash journal for the take in flight, installed by `LiveStream::begin`
    /// and handed back by `take`. `None` between takes (and whenever journalling
    /// could not be started), which turns every spill below into a no-op.
    journal: Option<CaptureJournal>,
    /// SP-06 — the take's release DSP, fed every 16 kHz sample as it lands in
    /// `out`. Dropped by `reset` (its Silero session goes back to the warm VAD).
    analysis: Option<Box<TakeAnalysis<'static>>>,
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
            spill: None,
            spilled: 0,
            mono: Vec::new(),
            journal: None,
            analysis: None,
        }
    }

    /// Drop the previous take entirely (buffers AND filter/resampler state) so
    /// no audio — and no biquad tail — ever bleeds from one dictation into the
    /// next.
    fn reset(&mut self) {
        // An unfinished spill here belongs to a take nobody collected; dropping
        // it is what moves its audio to the recovery dir (see `SpillWriter`).
        *self = Self::new(self.sample_rate, self.channels);
    }

    /// YV92/OS-9 — the input format changed UNDER a live take (AirPods went in,
    /// the link renegotiated to HFP, the default input device moved). Unlike
    /// [`Self::reset`] this is deliberately NON-destructive: the take's buffers,
    /// its AGC statistics and its crash journal all ride through. What is thrown
    /// away is exactly what belongs to the old format — the resampler (a ratio
    /// must never survive a format change) and the high-pass state, both of
    /// which are rebuilt for the new rate.
    ///
    /// The resampler's tail is flushed at the OLD ratio first, so the last few
    /// milliseconds captured before the change are neither lost nor stretched.
    ///
    /// One honest wart: `raw` — the never-lose-audio fallback — now holds two
    /// native rates end to end. It is only consulted when the streamed buffer
    /// came back unusable, and preserving audio at a slightly wrong pitch beats
    /// discarding it, so it is kept and logged rather than truncated.
    fn reconfigure(&mut self, sample_rate: u32, channels: u16) {
        if sample_rate == 0 || (sample_rate == self.sample_rate && channels == self.channels) {
            return;
        }
        let before = self.out.len();
        self.resampler.finish(&mut self.out);
        if let Some(journal) = self.journal.as_ref() {
            journal.append(&self.out[before..]);
        }
        // SP-06: the flushed tail is part of the take — the analysis sees it too.
        self.feed_analysis(before);
        // SP-05: …and so does the streaming decode.
        self.feed_stream(before);
        // yap-audit-meeting-timings: while a meeting records, the number that
        // matters is the MEETING's — this take's counter is 0 whenever nobody
        // is dictating, which is what made a retune 30 minutes into a meeting
        // read "0 samples captured so far".
        let captured = match crate::meeting::active_capture_seconds() {
            Some(seconds) => crate::meeting::RetuneCapture::Meeting {
                seconds,
                dictation_samples: self.out.len(),
            },
            None => crate::meeting::RetuneCapture::Dictation {
                samples: self.out.len(),
            },
        };
        log::warn!(
            "{}",
            crate::meeting::input_format_change_line(
                self.sample_rate,
                self.channels,
                sample_rate,
                channels,
                captured,
            )
        );
        self.sample_rate = sample_rate;
        self.channels = channels;
        self.high_pass = Biquad::high_pass(sample_rate, HIGH_PASS_HZ);
        self.resampler.retune(sample_rate, TARGET_RATE);
    }

    /// The rate the resampler is converting FROM right now — the value OS-9 says
    /// must track the device, not the moment the stream happened to open.
    #[cfg(test)]
    fn resample_from_rate(&self) -> u32 {
        self.resampler.from_rate()
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
        let before = self.out.len();
        self.resampler.push(&self.mono, &mut self.out);
        // YV63: the same frames go to the crash journal on their way past, so a
        // take the app dies in the middle of is already on disk. Bounded
        // hand-off — if the writer falls behind the SPILL is dropped, never the
        // frame, and the callback never waits on the disk.
        if let Some(journal) = self.journal.as_ref() {
            journal.append(&self.out[before..]);
        }
        // SP-06: …and through the release DSP + Silero, frame by frame, so the
        // take is analysed by the time the key comes up.
        self.feed_analysis(before);
        // SP-05: …and into the streaming decode, so the take is mostly
        // transcribed by the time the key comes up.
        self.feed_stream(before);
        // …and Y3's drain: everything behind the working window goes to the
        // take's own WAV, so `out` stops growing with the length of the take.
        self.drain_to_spill(false);
    }

    /// Move finished samples out of `out` and into the spill, keeping
    /// [`SPILL_WINDOW_SAMPLES`] resident (or nothing at all when `all`).
    ///
    /// A no-op when the take has no spill, which is the degraded path: `out`
    /// then grows the way it did before Y3 rather than losing audio.
    fn drain_to_spill(&mut self, all: bool) {
        let keep = if all { 0 } else { SPILL_WINDOW_SAMPLES };
        if self.out.len() <= keep {
            return;
        }
        let Some(spill) = self.spill.as_mut() else {
            return;
        };
        let cut = self.out.len() - keep;
        if !spill.append(&self.out[..cut]) {
            // The disk gave up. Keep every sample in RAM and stop draining —
            // memory is the cost, never the words.
            self.spill = None;
            return;
        }
        self.out.drain(..cut);
        self.spilled += cut;
    }

    /// Bytes of audio this take is holding in RAM right now — the number
    /// `MAX_RESIDENT_CAPTURE_BYTES` bounds. Counted like
    /// `meeting_capture_memory.rs` counts a meeting's: CAPACITY (not length) of
    /// every retained `f32` buffer, including the resampler's.
    fn resident_bytes(&self) -> usize {
        (self.out.capacity() + self.mono.capacity()) * std::mem::size_of::<f32>()
            + self.resampler.resident_bytes()
    }

    /// The soft-AGC gain over the take so far — at `take` time, the take's
    /// gain. SP-06 scales what Silero sees by it during capture, so the
    /// detector hears roughly the level the batch pass used to.
    fn running_gain(&self) -> f32 {
        if !self.finite {
            return 1.0;
        }
        let rms = if self.counted > 0 {
            (self.sum_sq / self.counted as f64).sqrt() as f32
        } else {
            0.0
        };
        agc_gain(rms, self.peak, NORMALIZE_TARGET_DBFS)
    }

    /// SP-06 — hand `out[from..]` (just resampled) to the take's analysis.
    fn feed_analysis(&mut self, from: usize) {
        if self.analysis.is_none() || from >= self.out.len() {
            return;
        }
        let gain = self.running_gain();
        if let Some(analysis) = self.analysis.as_mut() {
            analysis.push(&self.out[from..], gain);
        }
    }

    /// SP-05 — hand the 16 kHz samples from `from` on to the take's streaming
    /// decode (`stream_decode`), at the take's running AGC gain. A no-op when
    /// no stream is armed (CLI, bench, tests).
    fn feed_stream(&self, from: usize) {
        if from >= self.out.len() {
            return;
        }
        crate::stream_decode::feed(&self.out[from..], self.running_gain());
    }

    /// Close the take: flush the resampler's tail and hand the buffers over.
    fn take(&mut self) -> CapturedAudio {
        let before_tail = self.out.len();
        self.resampler.finish(&mut self.out);
        let gain = self.running_gain();
        self.feed_analysis(before_tail);
        self.feed_stream(before_tail);
        // SP-06: the analysis is only worth anything if it saw EVERY sample of
        // the take; anything else and the batch chain decides.
        let captured_total = self.spilled + self.out.len();
        // SP-05: the stream must have seen exactly this many samples, or the
        // release path decodes the whole take in batch.
        crate::stream_decode::seal(captured_total);
        let analysis = match self.analysis.take() {
            Some(a) if a.fed() == captured_total => Some(a),
            Some(a) => {
                log::warn!(
                    "record stream_dsp_out_of_step analysed={} captured={captured_total}",
                    a.fed()
                );
                None
            }
            None => None,
        };
        // Y3: the resampler's tail has landed, so the take is complete — push
        // ALL of it to the spill and close the file. When there is a spill the
        // take now lives entirely on disk and `samples` goes out empty; when
        // there is not, `samples` carries the whole take as it always did.
        if let Some(journal) = self.journal.as_ref() {
            journal.append(&self.out[before_tail..]);
        }
        self.drain_to_spill(true);
        let spill = self.spill.take().and_then(SpillWriter::finish);
        let samples = if spill.is_some() {
            self.out.clear();
            Vec::new()
        } else {
            std::mem::take(&mut self.out)
        };
        let captured = CapturedAudio {
            samples,
            spill,
            sample_rate: self.sample_rate,
            gain,
            // Taken (not reset) so the journal rides out with its take — the
            // reset below must not retire a journal `stop_recording` still owns.
            journal: self.journal.take(),
            // A device error is stamped by the worker at Disarm (YV67), not here.
            device_failed: false,
            analysis,
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
        spill,
        sample_rate,
        gain,
        ..
    } = captured;
    // Y3 — the take comes off the disk, not out of a giant Vec. A spill that
    // cannot be read back is not allowed to lose the utterance: `samples` is
    // the in-RAM copy on the degraded path and is used as-is.
    let streamed = match spill.as_ref() {
        Some(spill) => match spill.read_back() {
            Ok(read) => read,
            Err(e) => {
                log::warn!("Y3 capture spill unreadable ({e}) — falling back to the RAM buffer");
                samples
            }
        },
        None => samples,
    };
    if streamed.is_empty() {
        return Err(crate::permissions::named(NO_AUDIO_ERR));
    }
    let leveled = if streamed.iter().any(|s| !s.is_finite()) {
        // The streamed chain produced garbage. Pre-Y3 this rebuilt the clip
        // from the untouched native-rate `raw` Vec; there is no such Vec any
        // more, so the salvage runs over the 16 kHz capture with the
        // non-finite samples zeroed. Same intent, same batch chain
        // (`fallback_chain` at TARGET_RATE is high-pass + normalize_rms, with
        // no resample), and it keeps every sample that was not garbage.
        log::warn!(
            "YV37 streamed DSP unusable ({} samples, captured at {sample_rate}Hz) — \
             salvaging the capture",
            streamed.len()
        );
        let sane: Vec<f32> = streamed
            .iter()
            .map(|&s| if s.is_finite() { s } else { 0.0 })
            .collect();
        fallback_chain(&sane, TARGET_RATE)
    } else {
        apply_gain(&streamed, gain)
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
        return Err(crate::permissions::named(NO_AUDIO_ERR));
    }
    Ok(out)
}

/// Never-lose-audio path: the pre-YV37 batch chain, used only when the streamed
/// buffer came back unusable. Since Y3 its input is the 16 kHz capture read
/// back from the spill rather than a native-rate `raw` Vec, so `sample_rate`
/// is `TARGET_RATE` on the live path and the resample below is skipped.
fn fallback_chain(raw: &[f32], sample_rate: u32) -> Vec<f32> {
    let hp = high_pass(raw, sample_rate, HIGH_PASS_HZ);
    let leveled = normalize_rms(&hp, NORMALIZE_TARGET_DBFS);
    if sample_rate == TARGET_RATE {
        leveled
    } else {
        // Same anti-alias filter the streaming path runs (YV92) — the fallback
        // must not be a quieter way to ship aliased audio.
        resample_decimate(&leveled, sample_rate, TARGET_RATE)
    }
}

/// Scale a buffer, hard-limited to `[-1, 1]` as a safety net.
fn apply_gain(samples: &[f32], gain: f32) -> Vec<f32> {
    samples
        .iter()
        .map(|&s| (s * gain).clamp(-1.0, 1.0))
        .collect()
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
pub(crate) const HIGH_PASS_HZ: f32 = 80.0;
/// RMS-normalize / soft-AGC target level. −20 dBFS is a comfortable speech level
/// that leaves headroom below full-scale so a boosted quiet voice never clips.
const NORMALIZE_TARGET_DBFS: f32 = -20.0;
/// Edge de-click fade length (ms) at clip start/end.
const EDGE_FADE_MS: f32 = 5.0;

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
    let gain = (target_rms / rms).min(peak_ceiling / peak).clamp(0.0, 64.0);
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

// ── SP-06: the release-side DSP, streamed during capture ───────────────────
// Before SP-06 a stop paid for the edge fade, RNNoise and Silero over the whole
// take (≈1.5 s on a 240 s take, 6.1 s on a 281 s one). Every one of them is a
// streaming computation with a short edge: the fade-in only needs the first
// 5 ms, RNNoise is a recurrent 10 ms-frame model, Silero a recurrent 30 ms-frame
// one. So they run as the capture ring drains, and the stop only pays for the
// last 5 ms of fade, RNNoise's one-frame flush and Silero's last frame.
//
// The arithmetic is the batch chain's, sample for sample (tests prove the
// stream byte-identical to `edge_fade` + `denoise_rnnoise` + `WarmVad::isolate`
// on the same input). The one live-path difference is the AGC gain, which is
// only known at stop: the stream runs on the un-gained take and the gain is
// applied to its output (and Silero hears the running gain), where the batch
// chain applied it first.

/// [`resample_linear`], one chunk at a time. Every output sample is the same
/// expression over the same two inputs as the batch function, so the stream is
/// byte-identical to it (test). What a stream cannot know early is the clip's
/// length, which decides the output count and clamps the last interpolation;
/// both are settled in [`Self::finish`].
struct LinearStream {
    ratio: f64,
    /// Input retained from global index `base` on.
    buf: Vec<f32>,
    base: usize,
    /// Next output index.
    next: usize,
}

impl LinearStream {
    fn new(from: u32, to: u32) -> Self {
        Self {
            ratio: from as f64 / to as f64,
            buf: Vec::new(),
            base: 0,
            next: 0,
        }
    }

    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.buf.extend_from_slice(input);
        let have = self.base + self.buf.len();
        loop {
            let src = self.next as f64 * self.ratio;
            let i0 = src.floor() as usize;
            // Both neighbours must be in hand; the last sample's clamp waits
            // for `finish`.
            if i0 + 1 >= have {
                break;
            }
            let t = (src - i0 as f64) as f32;
            out.push(self.buf[i0 - self.base] * (1.0 - t) + self.buf[i0 + 1 - self.base] * t);
            self.next += 1;
        }
        let keep_from = ((self.next as f64 * self.ratio).floor() as usize).min(have);
        if keep_from > self.base {
            self.buf.drain(..keep_from - self.base);
            self.base = keep_from;
        }
    }

    fn finish(&mut self, out: &mut Vec<f32>) {
        let len = self.base + self.buf.len();
        if len == 0 {
            return;
        }
        let out_len = ((len as f64) / self.ratio).floor() as usize;
        while self.next < out_len {
            let src = self.next as f64 * self.ratio;
            let i0 = src.floor() as usize;
            let i1 = (i0 + 1).min(len - 1);
            let t = (src - i0 as f64) as f32;
            out.push(self.buf[i0 - self.base] * (1.0 - t) + self.buf[i1 - self.base] * t);
            self.next += 1;
        }
    }

    /// Outputs produced so far (after `finish`: the batch output length).
    fn produced(&self) -> usize {
        self.next
    }
}

/// RNNoise's frame at 48 kHz (10 ms).
const DENOISE_FRAME: usize = DenoiseState::FRAME_SIZE;

/// [`denoise_rnnoise`] frame by frame: the same 16→48 kHz interpolation, the
/// same i16-range frames through one recurrent state, the same one-frame
/// delay dropped, the same 48→16 kHz return and the same never-gut-the-take
/// guard — the guard and the zero-padded flush in [`Self::finish`].
struct StreamDenoise {
    state: Box<DenoiseState<'static>>,
    up: LinearStream,
    down: LinearStream,
    /// The 48 kHz frame not yet whole, already i16-scaled.
    frame_in: Vec<f32>,
    frames: usize,
    /// Denoised 48 kHz samples produced, the delay frame included.
    produced48: usize,
    fed16: usize,
    /// Σx² over the 16 kHz input, summed in input order like `rms_energy`.
    in_sq: f64,
    finite: bool,
    at48: Vec<f32>,
    aligned: Vec<f32>,
}

impl StreamDenoise {
    fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            up: LinearStream::new(TARGET_RATE, RNNOISE_SR),
            down: LinearStream::new(RNNOISE_SR, TARGET_RATE),
            frame_in: Vec::with_capacity(DENOISE_FRAME),
            frames: 0,
            produced48: 0,
            fed16: 0,
            in_sq: 0.0,
            finite: true,
            at48: Vec::new(),
            aligned: Vec::new(),
        }
    }

    fn push(&mut self, x16: &[f32], out: &mut Vec<f32>) {
        self.fed16 += x16.len();
        for &x in x16 {
            if !x.is_finite() {
                self.finite = false;
            }
            self.in_sq += (x as f64) * (x as f64);
        }
        if !self.finite {
            return;
        }
        self.at48.clear();
        self.up.push(x16, &mut self.at48);
        self.run_frames(out);
    }

    /// Frame `at48`, denoise every whole frame, return the aligned output to 16 kHz.
    fn run_frames(&mut self, out: &mut Vec<f32>) {
        self.aligned.clear();
        let mut at = 0;
        while at < self.at48.len() {
            let take = (DENOISE_FRAME - self.frame_in.len()).min(self.at48.len() - at);
            self.frame_in.extend(
                self.at48[at..at + take]
                    .iter()
                    .map(|&s| s * i16::MAX as f32),
            );
            at += take;
            if self.frame_in.len() == DENOISE_FRAME {
                self.run_one();
            }
        }
        self.down.push(&self.aligned, out);
    }

    fn run_one(&mut self) {
        let mut out_frame = [0.0f32; DENOISE_FRAME];
        self.state.process_frame(&mut out_frame, &self.frame_in);
        for &o in out_frame.iter() {
            // The first frame out is RNNoise's algorithmic delay — dropped, so
            // the output re-aligns sample for sample with the input.
            if self.produced48 >= DENOISE_FRAME {
                self.aligned.push(o / i16::MAX as f32);
            }
            self.produced48 += 1;
        }
        self.frames += 1;
        self.frame_in.clear();
    }

    /// Flush the tail into `out` (which holds ONLY this stream's output).
    /// `false` = the batch function would have returned its input unchanged,
    /// so `out` must not be used.
    fn finish(&mut self, out: &mut Vec<f32>) -> bool {
        if !self.finite || self.fed16 < DENOISE_FRAME {
            return false;
        }
        self.at48.clear();
        self.up.finish(&mut self.at48);
        self.run_frames(out);
        let len48 = self.up.produced();
        if len48 < DENOISE_FRAME {
            return false;
        }
        // The batch pass runs one zero frame past the end (plus the zero-padded
        // partial frame) to flush the delay, then keeps exactly `len48`.
        let total = len48 + DENOISE_FRAME;
        self.aligned.clear();
        while self.frames * DENOISE_FRAME < total {
            self.frame_in.resize(DENOISE_FRAME, 0.0);
            self.run_one();
        }
        let excess = (self.produced48 - DENOISE_FRAME).saturating_sub(len48);
        let keep = self.aligned.len().saturating_sub(excess);
        self.aligned.truncate(keep);
        self.down.push(&self.aligned, out);
        self.down.finish(out);
        if out.is_empty() || out.iter().any(|s| !s.is_finite()) {
            return false;
        }
        let in_rms = (self.in_sq / self.fed16 as f64).sqrt() as f32;
        let out_rms = rms_energy(out);
        !(in_rms > 1e-5 && out_rms < in_rms * 0.05)
    }
}

/// The edge fade's length in samples at the take rate (`edge_fade`'s own sum).
fn edge_fade_samples() -> usize {
    (TARGET_RATE as f32 * EDGE_FADE_MS / 1000.0) as usize
}

/// SP-06 — one take's release DSP, fed the 16 kHz capture as it lands: the
/// edge fade (the fade-out tail is held back until the take's length is
/// known), RNNoise when `denoise`, and Silero through a warm streamed session.
///
/// It keeps the finished (pre-gain) buffer in RAM, 64 KB per second of take,
/// beside the Y3 spill: that buffer IS the ASR input, so the stop no longer
/// reads the spill back. The spill stays the crash/recovery copy.
pub(crate) struct TakeAnalysis<'a> {
    fade: usize,
    /// The last `fade` samples, not yet released (their fade-out factor
    /// depends on the take's final length).
    hold: Vec<f32>,
    fed: usize,
    released: usize,
    finite: bool,
    denoise: Option<StreamDenoise>,
    /// The finished buffer so far (faded, denoised, before the AGC gain).
    out: Vec<f32>,
    vad: Option<vad::StreamIsolation<'a>>,
    vad_fed: usize,
    scratch: Vec<f32>,
}

/// A finished [`TakeAnalysis`]: the ASR buffer before the AGC gain, and the
/// Silero pass that saw every frame of it.
pub(crate) struct StreamedTake<'a> {
    pub out: Vec<f32>,
    pub vad: Option<vad::StreamIsolation<'a>>,
}

impl<'a> TakeAnalysis<'a> {
    pub(crate) fn new(denoise: bool, vad: Option<vad::StreamIsolation<'a>>) -> Self {
        Self {
            fade: edge_fade_samples(),
            hold: Vec::new(),
            fed: 0,
            released: 0,
            finite: true,
            denoise: denoise.then(StreamDenoise::new),
            out: Vec::new(),
            vad,
            vad_fed: 0,
            scratch: Vec::new(),
        }
    }

    /// Samples of the take seen so far.
    fn fed(&self) -> usize {
        self.fed
    }

    /// Feed the next 16 kHz samples of the take. `vad_gain` scales what Silero
    /// hears (`1.0` = untouched; the bench, and the identity tests).
    pub(crate) fn push(&mut self, samples: &[f32], vad_gain: f32) {
        self.fed += samples.len();
        if !self.finite {
            return;
        }
        if samples.iter().any(|s| !s.is_finite()) {
            // The batch chain salvages a non-finite take; the stream does not try.
            self.finite = false;
            return;
        }
        // `apply_gain(x, 1.0)`: the batch chain's hard limit, gain applied later.
        self.hold
            .extend(samples.iter().map(|&s| s.clamp(-1.0, 1.0)));
        if self.hold.len() <= self.fade {
            return;
        }
        let release = self.hold.len() - self.fade;
        self.scratch.clear();
        for (k, &s) in self.hold[..release].iter().enumerate() {
            let idx = self.released + k;
            // `edge_fade`'s fade-in, the same factor expression.
            self.scratch.push(if idx < self.fade {
                s * (idx as f32 / self.fade as f32)
            } else {
                s
            });
        }
        self.hold.drain(..release);
        self.released += release;
        self.forward(vad_gain);
    }

    fn forward(&mut self, vad_gain: f32) {
        match self.denoise.as_mut() {
            Some(dn) => dn.push(&self.scratch, &mut self.out),
            None => self.out.extend_from_slice(&self.scratch),
        }
        self.feed_vad(vad_gain);
    }

    fn feed_vad(&mut self, gain: f32) {
        if let Some(vad) = self.vad.as_mut() {
            vad.push(&self.out[self.vad_fed..], gain);
        }
        self.vad_fed = self.out.len();
    }

    /// Close the take: fade out the held tail, flush RNNoise, decide Silero's
    /// last frames. `Err` = use the batch chain (non-finite capture, a take
    /// shorter than its two fades, or a denoise the batch guard would refuse).
    pub(crate) fn finish(mut self, vad_gain: f32) -> Result<StreamedTake<'a>, String> {
        if !self.finite {
            return Err("non-finite capture".into());
        }
        let n = self.fed;
        if n < 2 * self.fade {
            return Err(format!(
                "take of {n} samples is shorter than its edge fades"
            ));
        }
        self.scratch.clear();
        for (k, &s) in self.hold.iter().enumerate() {
            let idx = self.released + k;
            // `edge_fade`'s fade-out: sample n-1-i gets i/fade.
            let i = n - 1 - idx;
            self.scratch.push(if i < self.fade {
                s * (i as f32 / self.fade as f32)
            } else {
                s
            });
        }
        self.hold.clear();
        self.released = n;
        self.forward(vad_gain);
        if let Some(mut dn) = self.denoise.take() {
            if !dn.finish(&mut self.out) {
                return Err("denoise would gut the take; batch guard decides".into());
            }
        }
        self.feed_vad(vad_gain);
        Ok(StreamedTake {
            out: std::mem::take(&mut self.out),
            vad: self.vad.take(),
        })
    }
}

/// The one float→PCM conversion in the app: hard-limited, then scaled. Shared by
/// the wav writer and the YV63 journal so a recovered spill and a normal clip
/// hold bit-identical samples.
fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

pub(crate) fn write_wav_i16(path: &Path, sample_rate: u32, samples: &[f32]) -> Result<(), String> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for &s in samples {
        writer.write_sample(to_i16(s)).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// SP-03 — the bench's release-side DSP keeps a voiced buffer whole when
    /// no Silero engine is loaded (energy-VAD path), reports its voiced time,
    /// and refuses an empty buffer instead of timing nothing.
    #[test]
    fn bench_release_dsp_keeps_the_buffer_on_the_energy_vad_path() {
        let rate = super::TARGET_RATE as f32;
        // 2 s: 0.5 s silence, 1 s of a 220 Hz tone, 0.5 s silence.
        let raw: Vec<f32> = (0..(2 * super::TARGET_RATE as usize))
            .map(|i| {
                let t = i as f32 / rate;
                if (0.5..1.5).contains(&t) {
                    0.3 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                } else {
                    0.0
                }
            })
            .collect();
        let leveled = super::bench_capture_chain(&raw);
        assert_eq!(leveled.len(), raw.len());
        let out = super::bench_release_dsp(&leveled, false, None).unwrap();
        assert_eq!(out.samples.len(), raw.len(), "no Silero → no trim");
        assert!(out.speech_present);
        assert!(out.voiced_seconds > 0.0 && out.voiced_seconds <= 2.0);
        assert!(super::bench_release_dsp(&[], false, None).is_err());
    }

    use super::*;
    use std::f32::consts::PI;

    const SR: u32 = 16_000;

    // ── YV91: the arm window ───────────────────────────────────────────────
    // The capture callback no longer gates on a `capturing` flag — the frames
    // enter the ring unconditionally (a meeting has no Arm at all) and the
    // CONSUMER decides which of them belong to the dictation take, by frame
    // index. That arithmetic is the one place in the new path that can
    // silently truncate a take, and a live microphone is the only other way to
    // exercise it, so it is pure and tested here.

    #[test]
    fn nothing_is_armed_before_the_first_take() {
        let window = ArmWindow::idle();
        assert!(!window.is_open());
        let (from, to) = window.bounds();
        assert_eq!(armed_range(0, 1024, 1, from, to), None);
    }

    #[test]
    fn a_take_that_arms_mid_block_starts_at_the_frame_the_key_went_down() {
        // 512 stereo frames in the block; the key went down at frame 200 of it.
        assert_eq!(
            armed_range(1_000, 512, 2, 1_200, u64::MAX),
            Some(400..1_024),
            "the take must not be given the audio from before the keypress"
        );
    }

    #[test]
    fn a_take_that_ends_mid_block_keeps_its_tail_and_nothing_after_it() {
        // This is the case a `capturing` flag read at drain time gets wrong:
        // the frames between the last callback and key-up are still in the
        // ring, and they are the end of the utterance.
        assert_eq!(
            armed_range(1_000, 512, 1, 0, 1_300),
            Some(0..300),
            "the tail of the take is kept, and the frames after key-up are not"
        );
    }

    #[test]
    fn a_whole_block_inside_the_window_is_taken_whole() {
        assert_eq!(armed_range(1_000, 512, 1, 500, 5_000), Some(0..512));
        assert_eq!(armed_range(1_000, 512, 2, 500, 5_000), Some(0..1_024));
    }

    #[test]
    fn blocks_wholly_outside_the_window_are_the_meetings_alone() {
        // Before the take…
        assert_eq!(armed_range(0, 512, 1, 1_000, u64::MAX), None);
        // …and after it. Both still reach the meeting fan-out; neither reaches
        // the dictation DSP.
        assert_eq!(armed_range(2_000, 512, 1, 100, 1_500), None);
        // A window that closed exactly at this block's first frame is empty,
        // not a one-frame slice.
        assert_eq!(armed_range(1_000, 512, 1, 0, 1_000), None);
    }

    #[test]
    fn opening_a_window_clears_the_previous_takes_close() {
        // `begin` must reopen, not extend: a second take that inherited the
        // first take's `to` would be silently empty.
        let window = ArmWindow::idle();
        window.open(100);
        window.close(200);
        assert!(!window.is_open());
        window.open(300);
        assert!(window.is_open());
        let (from, to) = window.bounds();
        assert_eq!(from, 300);
        assert_eq!(to, u64::MAX);
        assert_eq!(armed_range(300, 64, 1, from, to), Some(0..64));
    }

    // ── The idle closer vs. a meeting (YV91) ────────────────────────────────

    fn idle(idle_for: Duration) -> IdleInputs {
        IdleInputs {
            stream_open: true,
            capturing: false,
            held_for_meeting: false,
            meeting_active: false,
            idle_for,
            // The pre-SP-09 tests are the battery case: 60 s.
            on_battery: true,
        }
    }

    #[test]
    fn on_wall_power_the_warm_stream_stays_open_five_minutes_but_not_on_battery() {
        let ac = |idle_for: Duration| IdleInputs {
            on_battery: false,
            ..idle(idle_for)
        };
        assert!(!should_close_idle_stream(&ac(IDLE_CLOSE)));
        assert!(!should_close_idle_stream(&ac(Duration::from_secs(299))));
        assert!(should_close_idle_stream(&ac(Duration::from_secs(300))));
        // Battery keeps the 60 s close.
        assert!(should_close_idle_stream(&idle(IDLE_CLOSE)));
        assert_eq!(idle_close_after(false), Duration::from_secs(300));
        assert_eq!(idle_close_after(true), Duration::from_secs(60));
    }

    #[test]
    fn an_idle_stream_with_no_meeting_still_closes_after_a_minute() {
        // The behaviour that must not regress: the mic indicator goes out when
        // nothing is recording.
        assert!(should_close_idle_stream(&idle(IDLE_CLOSE)));
        assert!(!should_close_idle_stream(&idle(
            IDLE_CLOSE - Duration::from_secs(1)
        )));
    }

    #[test]
    fn the_worker_never_closes_a_stream_while_a_meeting_is_registered() {
        // A meeting has no Arm, so `is_capturing()` — the dictation arm window
        // — is false for its entire length. Closing on that alone gave a
        // meeting with no concurrent dictation exactly 60 seconds of audio and
        // then three hours of a session that thought it was still recording.
        let held = IdleInputs {
            held_for_meeting: true,
            ..idle(Duration::from_secs(3 * 60 * 60))
        };
        assert!(
            !should_close_idle_stream(&held),
            "a three-hour meeting is 180 idle-close windows long"
        );
        // And the fact alone is enough, even if the hold were ever missed.
        let registered = IdleInputs {
            meeting_active: true,
            ..idle(Duration::from_secs(3 * 60 * 60))
        };
        assert!(!should_close_idle_stream(&registered));
    }

    #[test]
    fn releasing_the_hold_lets_the_stream_close_again() {
        // Otherwise a meeting would pin the microphone (and the indicator) for
        // the rest of the session.
        let after = IdleInputs {
            held_for_meeting: false,
            meeting_active: false,
            ..idle(IDLE_CLOSE)
        };
        assert!(should_close_idle_stream(&after));
    }

    #[test]
    fn an_armed_dictation_is_never_closed_out_from_under() {
        let armed = IdleInputs {
            capturing: true,
            ..idle(IDLE_CLOSE * 10)
        };
        assert!(!should_close_idle_stream(&armed));
        // Nothing to close.
        let shut = IdleInputs {
            stream_open: false,
            ..idle(IDLE_CLOSE * 10)
        };
        assert!(!should_close_idle_stream(&shut));
    }

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

    fn fmt(rate: u32) -> InputFormat {
        InputFormat::new(rate, 1)
    }

    #[test]
    fn config_cache_hits_only_the_same_named_device() {
        let mut cache: TestCache = DeviceConfigCache::new();
        assert!(!cache.is_cached());
        assert_eq!(
            cache.get("MacBook Pro Microphone", None),
            None,
            "cold cache misses"
        );

        cache.store(
            "MacBook Pro Microphone".into(),
            fmt(48_000),
            "builtin",
            48_000,
        );
        assert!(cache.is_cached());
        assert_eq!(
            cache.get("MacBook Pro Microphone", None),
            Some(("builtin", 48_000)),
            "same device reuses the cached config (no HAL query on reopen)"
        );
        // A different default input device must NOT reuse another device's
        // rate/format — that would open the stream misconfigured.
        assert_eq!(cache.get("AirPods Pro", None), None);
        // An unidentifiable device (cpal returns an empty name) never hits.
        assert_eq!(cache.get("", None), None);
    }

    /// YV92/OS-9 — the cache used to be keyed on the device NAME alone, so
    /// AirPods that renegotiated from 48 kHz to 24 kHz (or down to the HFP rate
    /// the moment their mic is engaged) kept their name and handed the reopen a
    /// stale sample rate: the rest of the session came out time-stretched. A
    /// caller that knows the current format now misses.
    #[test]
    fn config_cache_misses_when_the_same_device_reports_a_new_format() {
        let mut cache: TestCache = DeviceConfigCache::new();
        cache.store("AirPods Pro".into(), fmt(48_000), "bt", 48_000);
        assert_eq!(
            cache.get("AirPods Pro", Some(fmt(48_000))),
            Some(("bt", 48_000)),
            "the format it was cached at still hits"
        );
        assert_eq!(
            cache.get("AirPods Pro", Some(fmt(24_000))),
            None,
            "the SAME device at a new rate must not reuse the stale config"
        );
        assert_eq!(
            cache.get("AirPods Pro", Some(InputFormat::new(48_000, 2))),
            None,
            "a channel-count change is a format change too"
        );
    }

    #[test]
    fn config_cache_invalidates_on_device_error_and_restores() {
        let mut cache: TestCache = DeviceConfigCache::new();
        cache.store("AirPods Pro".into(), fmt(24_000), "bt", 24_000);
        assert!(cache.get("AirPods Pro", None).is_some());

        // Stream/open error (device unplugged, rate changed) → drop everything
        // so the next arm re-queries the hardware.
        cache.invalidate();
        assert!(!cache.is_cached());
        assert_eq!(cache.get("AirPods Pro", None), None);

        // …and the next successful open re-populates it.
        cache.store("AirPods Pro".into(), fmt(16_000), "bt", 16_000);
        assert_eq!(cache.get("AirPods Pro", None), Some(("bt", 16_000)));

        // Re-storing under a NEW device replaces the entry (one device cached).
        cache.store(
            "MacBook Pro Microphone".into(),
            fmt(48_000),
            "builtin",
            48_000,
        );
        assert_eq!(cache.get("AirPods Pro", None), None);
        assert_eq!(
            cache.get("MacBook Pro Microphone", None),
            Some(("builtin", 48_000))
        );
    }

    #[test]
    fn config_cache_never_stores_an_empty_device_name() {
        let mut cache: TestCache = DeviceConfigCache::new();
        cache.store(
            "MacBook Pro Microphone".into(),
            fmt(48_000),
            "builtin",
            48_000,
        );
        // An unnamed device must clear the cache rather than pin an unknown
        // device under "" (which would then hit for every unnamed device).
        cache.store(String::new(), fmt(8_000), "mystery", 8_000);
        assert!(!cache.is_cached());
        assert_eq!(cache.get("", None), None);
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
    fn stop_signal_wakes_waiters_immediately_on_release() {
        let stop = Arc::new(StopSignal::default());
        assert!(!stop.is_stopped());
        // A tick with no stop expires as a plain timeout — the waiter keeps its
        // own cadence and learns nothing has changed.
        assert!(!stop.wait_stopped(Duration::from_millis(10)));

        let signaller = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            signaller.stop();
        });
        let started = Instant::now();
        // A long tick must NOT be waited out: the condvar wakes on the signal,
        // which is the whole point of YV38 (the old flag was read every 50 ms).
        assert!(stop.wait_stopped(Duration::from_secs(5)));
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "stop took {:?} — it should wake on the signal, not on the tick",
            started.elapsed()
        );
        // Already stopped → returns without parking at all.
        assert!(stop.is_stopped());
        assert!(stop.wait_stopped(Duration::from_secs(30)));
    }

    #[test]
    fn arm_signal_surfaces_worker_errors_verbatim() {
        let (tx, rx) = mpsc::sync_channel::<Result<u8, String>>(1);
        tx.send(Err(crate::permissions::named(NO_MIC_ERR))).unwrap();
        assert_eq!(
            await_reply(&rx, Duration::from_secs(3), "arm"),
            Err(crate::permissions::named(NO_MIC_ERR)),
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

    // ── yap-fx-07 — every kept clip is listed ───────────────────────────────

    fn fx07_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-fx07-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The artefact `SpillWriter::drop` leaves in the recovery dir: a playable
    /// 16 kHz mono f32 `<id>.capture.wav`, at the given amplitude.
    fn fx07_spill(dir: &Path, name: &str, seconds: f32, amp: f32) -> PathBuf {
        let path = dir.join(name);
        let mut w = hound::WavWriter::create(&path, capture_spill_spec()).unwrap();
        let n = (TARGET_RATE as f32 * seconds) as usize;
        for i in 0..n {
            let t = i as f32 / TARGET_RATE as f32;
            w.write_sample(amp * (2.0 * PI * 220.0 * t).sin()).unwrap();
        }
        w.finalize().unwrap();
        path
    }

    #[test]
    fn silent_clip_needs_every_second_below_the_floor() {
        let rate = TARGET_RATE as usize;
        // The forensics spill: ~0.002 loudness for 6.7 s — silent.
        let hiss: Vec<f32> = (0..(rate * 67 / 10))
            .map(|i| if i % 2 == 0 { 0.002 } else { -0.002 })
            .collect();
        assert!(is_silent_clip(&hiss));
        assert!(is_silent_clip(&vec![0.0; rate * 3]));
        // Speech-level signal (0.06) is never silent.
        let speech: Vec<f32> = (0..rate * 2)
            .map(|i| if i % 2 == 0 { 0.06 } else { -0.06 })
            .collect();
        assert!(!is_silent_clip(&speech));
        // Nine silent seconds and ONE loud one: kept.
        let mut mostly_pause = vec![0.0f32; rate * 9];
        mostly_pause.extend((0..rate).map(|i| if i % 2 == 0 { 0.05 } else { -0.05 }));
        assert!(!is_silent_clip(&mostly_pause));
        // A non-finite sample is signal, never silence.
        let mut bad = vec![0.0f32; rate];
        bad[10] = f32::NAN;
        assert!(!is_silent_clip(&bad));
    }

    #[test]
    fn aborted_spill_gets_a_wav_or_is_deleted_only_if_silent() {
        let dir = fx07_dir("adopt");
        let voiced = fx07_spill(&dir, "voiced.capture.wav", 2.0, 0.3);
        let silent = fx07_spill(&dir, "quiet.capture.wav", 2.0, 0.001);
        // An in-flight decode link must never be touched mid-session.
        let in_flight = fx07_spill(&dir, "live.decoding.wav", 2.0, 0.3);
        let later = std::time::SystemTime::now() + Duration::from_secs(3600);

        let got = recover_aborted_capture_spills(&dir, &[], later, Duration::from_secs(10));
        assert_eq!(got.len(), 1, "only the voiced spill is offered");
        assert_eq!(got[0].take_id, "voiced");
        assert!(got[0].wav_path.ends_with("voiced.wav"));
        assert!(got[0].wav_path.exists());
        assert!((got[0].seconds - 2.0).abs() < 0.01);
        assert!(!voiced.exists(), "spill retired once its wav is complete");
        assert!(!silent.exists(), "silent orphan deleted");
        assert!(!dir.join("quiet.wav").exists(), "silent orphan gets no wav");
        assert!(in_flight.exists(), "decoding link left for the launch scan");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn aborted_spill_scan_defers_young_kept_and_shadowed_files() {
        let dir = fx07_dir("defer");
        let young = fx07_spill(&dir, "young.capture.wav", 2.0, 0.3);
        // Too new: a copy fallback may still be writing it.
        let got = recover_aborted_capture_spills(
            &dir,
            &[],
            std::time::SystemTime::now(),
            Duration::from_secs(3600),
        );
        assert!(got.is_empty());
        assert!(young.exists());

        let later = std::time::SystemTime::now() + Duration::from_secs(3600);
        // A row already names it: not an orphan.
        let keep = vec![young.to_string_lossy().into_owned()];
        assert!(recover_aborted_capture_spills(&dir, &keep, later, Duration::ZERO).is_empty());
        assert!(young.exists());

        // `<id>.wav` already exists (maybe a kept take): never overwritten.
        std::fs::write(dir.join("young.wav"), b"kept take").unwrap();
        assert!(recover_aborted_capture_spills(&dir, &[], later, Duration::ZERO).is_empty());
        assert_eq!(std::fs::read(dir.join("young.wav")).unwrap(), b"kept take");
        assert!(young.exists());

        // Missing dir: a safe no-op.
        let _ = std::fs::remove_dir_all(&dir);
        assert!(recover_aborted_capture_spills(&dir, &[], later, Duration::ZERO).is_empty());
    }

    // ── Crash-safe capture journal (YV63) ───────────────────────────────────
    // The promise is "a dictation is never lost, even if Yap dies mid-take", so
    // the tests check the three things that promise rests on: the audio is on
    // DISK while the user is still speaking, the spill can never slow capture
    // down, and a take that ends normally leaves nothing behind to recover.

    fn journal_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yv63-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Wait (bounded) for the background writer to have `min_bytes` on disk.
    /// The writer is asynchronous BY DESIGN — that is the whole point — so the
    /// test observes the file, it does not reach into the writer.
    fn wait_for_spill(path: &Path, min_bytes: u64) -> u64 {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            if len >= min_bytes || Instant::now() >= deadline {
                return len;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Wait (bounded) for a path the writer thread creates.
    fn wait_for_file(path: &Path) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        path.exists()
    }

    fn read_spill(path: &Path) -> Vec<i16> {
        std::fs::read(path)
            .unwrap()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect()
    }

    #[test]
    fn journal_appends_frames_incrementally() {
        let dir = journal_dir("append");
        let journal = CaptureJournal::start(&dir).expect("journal opens");
        assert!(
            wait_for_file(journal.marker_path()),
            "the in-progress marker must exist from capture START, not from the end"
        );

        // Frame one — as it would arrive from the capture worker, mid-hold.
        let first = tone(0.2, 220.0, 0.4);
        journal.append(&first);
        let on_disk = wait_for_spill(journal.spill_path(), (first.len() * 2) as u64);
        assert_eq!(
            on_disk,
            (first.len() * 2) as u64,
            "frames must be readable mid-stream — a crash now must still find audio"
        );
        assert_eq!(
            read_spill(journal.spill_path()),
            first.iter().map(|&s| to_i16(s)).collect::<Vec<_>>(),
            "the spilled samples are the captured ones, not a re-encoding"
        );

        // …and the file GROWS with the take instead of being rewritten at the end.
        let second = tone(0.1, 330.0, 0.3);
        journal.append(&second);
        let grown = wait_for_spill(
            journal.spill_path(),
            ((first.len() + second.len()) * 2) as u64,
        );
        assert_eq!(
            grown,
            ((first.len() + second.len()) * 2) as u64,
            "later frames append to the same spill"
        );
        assert_eq!(journal.dropped_writes(), 0, "an idle disk drops nothing");

        drop(journal);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_writer_never_blocks_capture() {
        // A wedged/slow writer is simulated by a receiver that never receives:
        // the queue fills after two chunks and every further offer must return
        // immediately, counted as a dropped JOURNAL write — never a dropped
        // frame, never an error the capture path has to handle.
        let (tx, rx) = mpsc::sync_channel::<JournalWrite>(2);
        let queue = JournalQueue::new(tx);
        let started = Instant::now();
        for i in 0..6i16 {
            queue.offer(JournalWrite::Frames(vec![i; 4]));
        }
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "offering into a full journal queue must not park the capture path"
        );
        assert_eq!(
            queue.dropped(),
            4,
            "the four chunks past the bound are dropped, not queued or awaited"
        );
        // What did fit is intact and in order — the overflow corrupts nothing.
        assert_eq!(queued_frames(rx.recv().unwrap()), vec![0i16; 4]);
        assert_eq!(queued_frames(rx.recv().unwrap()), vec![1i16; 4]);

        // A writer that died outright is the same story: a dropped write.
        drop(rx);
        queue.offer(JournalWrite::Frames(vec![9i16; 4]));
        assert_eq!(queue.dropped(), 5);
    }

    fn queued_frames(write: JournalWrite) -> Vec<i16> {
        match write {
            JournalWrite::Frames(chunk) => chunk,
            JournalWrite::Marker(line) => panic!("expected frames, got a marker: {line}"),
        }
    }

    /// YV92 review — the `device_change` marker used to `open()` + `writeln!()`
    /// inline. On the worker thread, yes, which is what the old doc-comment
    /// argued about; but its only caller reaches it through
    /// `LiveStream::mark_device_change`, which holds the DSP mutex the cpal
    /// input callback locks on EVERY buffer — and in `watchdog_poll` the marker
    /// is written while the old stream is still running and still contending for
    /// that lock. A cold-disk `open()` there costs a dropped callback buffer at
    /// exactly the seam the marker exists to timestamp.
    ///
    /// So the marker has to clear the same bar a frame does: hand off over the
    /// bounded queue, never touch the disk on the capture path.
    #[test]
    fn a_device_change_marker_uses_the_same_never_block_handoff_as_a_frame() {
        let (tx, rx) = mpsc::sync_channel::<JournalWrite>(1);
        let queue = JournalQueue::new(tx);
        // Fill the bound so every marker below hits a full queue — the wedged
        // writer / slow disk case.
        queue.offer(JournalWrite::Frames(vec![7i16; 4]));

        let line = serde_json::json!({"kind": "device_change", "segment_index": 1}).to_string();
        let started = Instant::now();
        for _ in 0..64 {
            queue.offer(JournalWrite::Marker(line.clone()));
        }
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "a marker must never park the path that holds the audio callback's lock"
        );
        assert_eq!(
            queue.dropped(),
            64,
            "a marker past the bound is DROPPED like a frame — never awaited, never an error"
        );
        assert_eq!(
            queued_frames(rx.recv().unwrap()),
            vec![7i16; 4],
            "and the overflow corrupts nothing that did fit"
        );
    }

    #[test]
    fn normal_completion_removes_marker() {
        let dir = journal_dir("complete");
        let journal = CaptureJournal::start(&dir).expect("journal opens");
        let marker = journal.marker_path().to_path_buf();
        let spill = journal.spill_path().to_path_buf();
        journal.append(&tone(0.2, 220.0, 0.4));
        wait_for_spill(&spill, 2);
        assert!(marker.exists() && spill.exists());

        journal.finish();

        assert!(
            !marker.exists(),
            "a take that ended normally must leave no in-progress marker"
        );
        assert!(!spill.exists(), "…and no spilled audio either");
        assert!(
            recover_orphaned_journals(&dir).is_empty(),
            "startup must find nothing to recover after a normal take"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn orphaned_spill_finalizes_into_a_playable_wav() {
        let dir = journal_dir("orphan");
        let journal = CaptureJournal::start(&dir).expect("journal opens");
        let marker = journal.marker_path().to_path_buf();
        let spoken = tone(0.5, 220.0, 0.4);
        journal.append(&spoken);
        wait_for_spill(journal.spill_path(), (spoken.len() * 2) as u64);
        // The app dies here: marker + spill survive, nothing retires them.
        journal.abandon();
        assert!(marker.exists());

        let recovered = recover_orphaned_journals(&dir);
        assert_eq!(recovered.len(), 1, "the orphaned take must be recovered");
        assert!((recovered[0].seconds - 0.5).abs() < 0.01);
        let samples = read_wav_16k_mono(&recovered[0].wav_path).expect("recovered wav parses");
        assert_eq!(samples.len(), spoken.len());
        assert!(!marker.exists(), "a recovered marker is retired");
        // Idempotent: a second startup has nothing left to do.
        assert!(recover_orphaned_journals(&dir).is_empty());

        // A sub-30 ms spill was a stray tap — retired, never surfaced as a take.
        let stray = CaptureJournal::start(&dir).expect("journal opens");
        let stray_marker = stray.marker_path().to_path_buf();
        stray.append(&tone(0.01, 220.0, 0.4));
        stray.abandon();
        assert!(recover_orphaned_journals(&dir).is_empty());
        assert!(!stray_marker.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_marker_predicate_matches_only_markers() {
        assert!(is_journal_marker("8b1f.in_progress.json"));
        assert!(is_journal_marker("8B1F.IN_PROGRESS.JSON"));
        // The spill, a recovered clip and a kept failed take all live in the same
        // dir — none of them is a marker.
        assert!(!is_journal_marker("8b1f.spill.pcm"));
        assert!(!is_journal_marker("8b1f.wav"));
        assert!(!is_journal_marker("in_progress.json"));
        assert!(!is_journal_marker(""));
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
        // Y3 — no spill was installed on this DSP, so the take comes back in
        // RAM exactly as it did before the spill existed.
        assert!(
            captured.spill.is_none(),
            "a probe with no spill keeps the take in RAM"
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
        assert!(dsp.out.is_empty() && dsp.spilled == 0 && dsp.spill.is_none());
    }

    /// YV92/OS-9 — the take must survive an input format change. Feed the DSP at
    /// 48 kHz, tell it the device is now AirPods at 24 kHz, keep feeding: the
    /// ratio must follow the device, the audio captured before the change must
    /// still be there, and the audio captured after it must be at the right
    /// speed (which is what a stale ratio silently destroys).
    #[test]
    fn stream_dsp_retunes_on_a_format_change_without_losing_the_take() {
        let mut dsp = StreamDsp::new(NATIVE_SR, 1);
        let first = tone_at(NATIVE_SR, 0.5, 300.0, 0.3);
        for frame in frames(&first, &[512]) {
            dsp.push(frame);
        }
        let after_first = dsp.out.len();
        assert!(after_first > 0);
        assert_eq!(dsp.resample_from_rate(), NATIVE_SR);

        // AirPods go in: same take, half the input rate.
        dsp.reconfigure(24_000, 1);
        assert_eq!(
            dsp.resample_from_rate(),
            24_000,
            "a ratio must never survive a format change"
        );
        assert!(
            dsp.out.len() >= after_first,
            "reconfigure must flush the old-rate tail, never drop it"
        );
        let kept = dsp.out.len();

        let second = tone_at(24_000, 0.5, 300.0, 0.3);
        for frame in frames(&second, &[256]) {
            dsp.push(frame);
        }
        // 0.5 s in at 24 kHz is 0.5 s out at 16 kHz — 8000 samples. At the STALE
        // 48 kHz ratio it would have been ~4000, i.e. the half-speed track OS-9
        // describes.
        let produced = dsp.out.len() - kept;
        assert!(
            produced.abs_diff(8_000) <= 4,
            "expected ~8000 samples from 0.5s at the NEW rate, got {produced}"
        );
        let captured = dsp.take();
        assert!(
            captured.samples.len() >= 8_000 + after_first - 4,
            "every sample from both formats rides out with the take"
        );
        assert_eq!(captured.sample_rate, 24_000);
        assert!(captured.samples.iter().all(|s| s.is_finite()));

        // A no-op reconfigure (same format) changes nothing at all.
        let mut same = StreamDsp::new(NATIVE_SR, 1);
        for frame in frames(&first, &[512]) {
            same.push(frame);
        }
        let before = same.out.len();
        same.reconfigure(NATIVE_SR, 1);
        assert_eq!(same.out.len(), before);
        same.reconfigure(0, 1);
        assert_eq!(
            same.resample_from_rate(),
            NATIVE_SR,
            "a zero rate is ignored"
        );
    }

    // ── The watchdog seam (YV92/OS-9) ───────────────────────────────────────
    // The pure state machine is proved in `tests/input_format_change_handler.rs`
    // by a test that holds ONE watch across a whole AirPods sequence. That is
    // only meaningful if production holds one too, so these tests drive the
    // production objects: a real `LiveStream` (its cpal handle is already an
    // `Option`, so a hardware-free one is the same struct with `stream: None`),
    // the real `observe_input` the watchdog calls, the real journal, and the
    // real marker sidecar on disk.

    /// A `LiveStream` with no cpal handle — everything the watchdog seam touches
    /// is present, and nothing that needs a microphone is.
    fn hardware_free_stream(device: &str, format: InputFormat) -> LiveStream {
        LiveStream {
            stream: None,
            dsp: Arc::new(Mutex::new(StreamDsp::new(
                format.sample_rate_hz,
                format.channels,
            ))),
            window: Arc::new(ArmWindow::idle()),
            armed: AtomicBool::new(false),
            failed: Arc::new(AtomicBool::new(false)),
            rt: Arc::new(crate::meeting::RtCapture::new(
                format.sample_rate_hz,
                format.channels,
            )),
            // No thread: these tests drive `drain_once` themselves, so the
            // consumer's work happens inline and deterministically.
            consumer: ConsumerHandle {
                stop: Arc::new(AtomicBool::new(false)),
                join: None,
            },
            watch: InputFormatWatch::new(device, format, TARGET_RATE),
            device_name: device.to_string(),
            format,
            // Pre-roll OFF by default, so every pre-SP-09 test keeps its exact
            // sample counts; the SP-09 tests install a real one.
            pre_roll: Arc::new(Mutex::new(PreRollState::with_frames(0, 0, format.channels))),
            pending_press: None,
        }
    }

    fn marker_lines(path: &Path) -> Vec<serde_json::Value> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("each marker line is JSON"))
            .collect()
    }

    /// Markers ride the journal's bounded queue to the writer thread (that is
    /// what keeps `open()` off the lock the audio callback takes), so the
    /// sidecar is observed, never assumed — same contract as `wait_for_spill`.
    fn wait_for_marker_lines(path: &Path, want: usize) -> Vec<serde_json::Value> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let lines = marker_lines(path);
            if lines.len() >= want || Instant::now() >= deadline {
                return lines;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Exactly what the cpal input callback does (see `build_capture_stream`),
    /// followed by exactly what the consumer does with it.
    ///
    /// YV91 moved the decision: the callback is now unconditional — a meeting
    /// has no Arm, so refusing frames at the callback would refuse the
    /// meeting's audio too — and the arm WINDOW decides, on the consumer side,
    /// which frames belong to the dictation take. These tests therefore have to
    /// go through both halves, because the half that a reopen can break is the
    /// second one.
    fn feed_one_callback(live: &LiveStream, frames: &[f32]) {
        crate::meeting::rt_capture_callback(&live.rt, frames, |s: f32| s, 0);
        drain_once(live);
    }

    /// One iteration of [`capture_consumer_loop`]'s body, inline. The `pos` the
    /// real loop keeps on its stack is recovered from the ring's own consumed
    /// count, which is the same number by construction.
    fn drain_once(live: &LiveStream) {
        let ch = live.rt.channels().max(1) as usize;
        let pos = live.rt.samples.consumed() / ch as u64;
        let mut block: Vec<f32> = Vec::new();
        let mut anchors: Vec<crate::rtring::CaptureAnchor> = Vec::new();
        live.rt.anchors.drain_into(&mut anchors);
        let taken = live.rt.samples.drain_into(&mut block);
        if taken == 0 {
            return;
        }
        let frames = (taken / ch) as u64;
        let (from, to) = live.window.bounds();
        replay_pre_roll(&live.pre_roll, &live.dsp, pos, from, to);
        let armed_slice = armed_range(pos, frames, ch, from, to);
        let mut guard = armed_slice
            .as_ref()
            .and_then(|_| live.dsp.lock().ok())
            .map(DictationDsp);
        crate::meeting::fan_out_block(
            &block,
            &anchors,
            guard.as_mut().map(|g| {
                let sink: &mut dyn crate::meeting::DictationSink = g;
                sink
            }),
            armed_slice,
        );
        drop(guard);
        remember_block(&live.pre_roll, pos, &block);
    }

    // ── SP-09: pre-roll and arm on key-down ─────────────────────────────────

    const SP09_SR: u32 = 48_000;
    /// One 10 ms callback at 48 kHz mono.
    const SP09_CB: usize = 480;

    fn sp09_stream() -> LiveStream {
        let mut live = hardware_free_stream("Built-in Microphone", fmt(SP09_SR));
        live.pre_roll = Arc::new(Mutex::new(PreRollState::new(SP09_SR, 1)));
        live
    }

    /// The acceptance test: a scripted press whose word starts AT the key-down
    /// (t = 0). The take does not arm until the hold gesture is confirmed
    /// (`HOLD_ARM_MS`) and the Start→Arm handshake has run — ~300 ms during
    /// which the arm window is SHUT — yet the WAV still starts with the word.
    #[test]
    fn a_scripted_press_that_speaks_at_t0_keeps_the_first_250ms_of_the_word() {
        let mut live = sp09_stream();
        // 200 ms of room silence before the press.
        for _ in 0..20 {
            feed_one_callback(&live, &vec![0.0; SP09_CB]);
        }
        // t = 0: fn goes down and the word starts in the same instant.
        live.note_press(Instant::now(), true);
        let word = tone_at(SP09_SR, 0.6, 440.0, 0.3);
        let mut chunks = word.chunks(SP09_CB);
        for c in chunks.by_ref().take(30) {
            feed_one_callback(&live, c);
        }
        assert!(!live.is_capturing(), "the hold has not armed yet");
        live.begin(None, None);
        assert!(live.pending_press.is_none(), "the take consumed the press");
        for c in chunks {
            feed_one_callback(&live, c);
        }
        let out = live.end().samples;
        // 16 kHz out: 200 ms of pre-roll + the whole 600 ms word ≈ 12 800.
        // Without the pre-roll the take would be the last 300 ms (≈ 4 800).
        assert!(out.len() >= 12_000, "take holds the pre-roll and the word: {}", out.len());
        let onset = out
            .iter()
            .position(|s| s.abs() > 0.05)
            .expect("the word is in the take");
        // The word starts at the press, 200 ms (3 200 samples) into the take.
        assert!(onset <= 3_200 + 320, "word onset at {onset}, want ≈3200");
        let first_250ms = &out[onset..onset + 4_000];
        assert!(
            rms(first_250ms) > 0.05,
            "the first 250 ms of the word survive: rms {}",
            rms(first_250ms)
        );
        assert!(out.len() - onset >= 9_000, "the word is whole from its onset");
    }

    #[test]
    fn a_tap_that_is_not_a_hold_discards_the_buffer() {
        let mut live = sp09_stream();
        live.note_press(Instant::now(), true);
        // The tap's audio (a click, a cough) — 100 ms of it.
        for _ in 0..10 {
            feed_one_callback(&live, &tone_at(SP09_SR, 0.01, 440.0, 0.3));
        }
        assert!(live.discard_press(), "a press was pending");
        assert!(live.pending_press.is_none());
        assert_eq!(live.pre_roll.lock().unwrap().ring.len_frames(), 0);
        // A later start (hands-free, no press) keeps none of the tap.
        for _ in 0..5 {
            feed_one_callback(&live, &vec![0.0; SP09_CB]);
        }
        live.begin(None, None);
        for _ in 0..5 {
            feed_one_callback(&live, &vec![0.0; SP09_CB]);
        }
        let out = live.end().samples;
        assert!(!out.is_empty());
        assert!(
            out.iter().all(|s| s.abs() < 0.01),
            "the tap's audio never reaches a take"
        );
    }

    #[test]
    fn a_start_with_no_press_still_keeps_500ms_of_pre_roll() {
        let mut live = sp09_stream();
        // 1 s of audio before an arm with no key-down (hands-free / button).
        for _ in 0..100 {
            feed_one_callback(&live, &tone_at(SP09_SR, 0.01, 440.0, 0.3));
        }
        live.begin(None, None);
        // The consumer replays the pre-roll on the first block after the arm.
        feed_one_callback(&live, &tone_at(SP09_SR, 0.01, 440.0, 0.3));
        let out = live.end().samples;
        // 500 ms of pre-roll + one 10 ms block at 16 kHz (≈ 8 160), give or
        // take the resampler's edge.
        assert!(
            (7_700..=8_600).contains(&out.len()),
            "pre-roll is 500 ms: {}",
            out.len()
        );
    }

    #[test]
    fn the_window_opens_at_the_press_less_the_pre_roll() {
        // Press 6 000 frames before the arm, 2 400 frames (50 ms @ 48 k) lead.
        assert_eq!(take_window_start(10_000, Some(4_000), 2_400, u64::MAX), 1_600);
        // No press: the arm itself is the anchor.
        assert_eq!(take_window_start(10_000, None, 2_400, u64::MAX), 7_600);
        // Never before the previous take's close — no word is kept twice.
        assert_eq!(take_window_start(10_000, Some(4_000), 2_400, 3_000), 3_000);
        // Clamped at the ring's origin.
        assert_eq!(take_window_start(1_000, Some(500), 2_400, u64::MAX), 0);
        // A press stamped in another frame space (ahead of now) is ignored.
        assert_eq!(take_window_start(1_000, Some(5_000), 0, u64::MAX), 1_000);
        // The ring holds the pre-roll plus the press grace, and the lead is
        // exactly PRE_ROLL_MS.
        let st = PreRollState::new(SP09_SR, 2);
        assert_eq!(st.lead_frames, 24_000);
        assert_eq!(st.ring.capacity_frames(), 72_000);
    }

    /// The whole point of the segment counter: it has to ORDER the markers of
    /// one take. Before the watch was owned by the stream, `watchdog_poll` built
    /// a fresh one per tick, so the full AirPods sequence (48 k → 24 k → 16 k
    /// HFP → 48 k) wrote three markers all claiming `segment_index: 1` and any
    /// rollover driven off it would collide on every file after the first.
    #[test]
    fn every_swap_of_one_take_opens_the_next_segment() {
        let dir = journal_dir("yv92-segments");
        let journal = CaptureJournal::start(&dir).expect("journal opens");
        let markers = journal.markers_path();
        let mut live = hardware_free_stream("MacBook Pro Microphone", fmt(48_000));
        live.begin(Some(journal), None);

        let mut indices = Vec::new();
        let mut boundaries = Vec::new();
        for (device, rate) in [
            ("AirPods Pro", 24_000),
            ("AirPods Pro", 16_000), // the mic engages: HFP/SCO
            ("MacBook Pro Microphone", 48_000),
        ] {
            // Audio keeps arriving between swaps, so the boundary each marker
            // carries has to move too.
            if let Ok(mut dsp) = live.dsp.lock() {
                let grown = dsp.out.len() + 16_000;
                dsp.out.resize(grown, 0.1);
            }
            // …the exact call `watchdog_poll` makes.
            let action = live.observe_input(
                device.to_string(),
                fmt(rate),
                FormatEventSource::StreamFormat,
            );
            let FormatChangeAction::Reconfigure { marker, ratio } = action else {
                panic!("a real format change must reconfigure, got {action:?}");
            };
            assert_eq!(
                ratio.from_hz, rate,
                "the ratio follows the new nominal rate"
            );
            live.mark_device_change(&marker);
            // Stand-in for the successful half of `reopen_after_change`: the
            // cpal reopen is the only part that needs hardware.
            live.adopt_reopened(device.to_string(), fmt(rate));
            indices.push(marker.segment_index);
            boundaries.push(marker.output_sample_index);
        }

        assert_eq!(
            indices,
            vec![1, 2, 3],
            "each swap opens the NEXT segment — a constant index cannot order a sidecar"
        );
        assert_eq!(
            boundaries,
            vec![16_000, 32_000, 48_000],
            "and each marker lands at the output sample where its segment starts"
        );

        let on_disk = wait_for_marker_lines(&markers, 3);
        assert_eq!(on_disk.len(), 3, "one line per swap");
        assert_eq!(
            on_disk
                .iter()
                .map(|m| m["segment_index"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2, 3],
            "the journal sidecar carries the same ordering the state machine decided"
        );
        assert_eq!(on_disk[0]["to_sample_rate"], 24_000);
        assert_eq!(on_disk[1]["to_sample_rate"], 16_000);
        assert_eq!(on_disk[2]["to_sample_rate"], 48_000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The two things that must NOT advance the counter: the burst of selectors
    /// one renegotiation fires, and the reopen that follows a change already
    /// counted. And the one thing that must reset it: a new take.
    #[test]
    fn a_reopen_is_not_a_segment_and_a_new_take_restarts_the_numbering() {
        let mut live = hardware_free_stream("MacBook Pro Microphone", fmt(48_000));
        live.begin(None, None);

        let action = live.observe_input(
            "AirPods Pro".into(),
            fmt(24_000),
            FormatEventSource::StreamFormat,
        );
        assert_eq!(action.marker().unwrap().segment_index, 1);
        live.adopt_reopened("AirPods Pro".into(), fmt(24_000));
        assert_eq!(live.watch.segment_index(), 1, "a reopen is not a boundary");
        assert_eq!(live.device_name, "AirPods Pro");
        assert_eq!(live.format, fmt(24_000));

        // The rest of the HAL's burst for the same renegotiation.
        for source in [
            FormatEventSource::NominalSampleRate,
            FormatEventSource::DefaultInputDevice,
            FormatEventSource::Watchdog,
        ] {
            assert_eq!(
                live.observe_input("AirPods Pro".into(), fmt(24_000), source),
                FormatChangeAction::Unchanged,
                "a repeat of an applied change is not another segment"
            );
        }
        assert_eq!(live.watch.segment_index(), 1);

        // A second swap in the SAME take is segment 2 — the regression this all
        // exists for.
        let second = live.observe_input(
            "AirPods Pro".into(),
            fmt(16_000),
            FormatEventSource::StreamFormat,
        );
        assert_eq!(second.marker().unwrap().segment_index, 2);

        // …and the next take starts its own numbering at 0.
        live.begin(None, None);
        assert_eq!(live.watch.segment_index(), 0);
        let fresh = live.observe_input(
            "MacBook Pro Microphone".into(),
            fmt(48_000),
            FormatEventSource::DefaultInputDevice,
        );
        assert_eq!(fresh.marker().unwrap().segment_index, 1);
    }

    /// Stands in for the successful half of `open_stream_into(cache, Some(reuse))`:
    /// the take is carried across (same DSP, same gate), only the rate-dependent
    /// state is rebuilt and the old stream's error flag is cleared. Everything
    /// it omits is the cpal handle, which is the only part needing a microphone.
    fn hardware_free_reopen(
        device: &'static str,
        format: InputFormat,
    ) -> impl FnOnce(TakeInFlight) -> Result<LiveStream, String> {
        move |(dsp, failed)| {
            if let Ok(mut dsp) = dsp.lock() {
                dsp.reconfigure(format.sample_rate_hz, format.channels);
            }
            failed.store(false, Ordering::SeqCst);
            let mut reopened = hardware_free_stream(device, format);
            reopened.dsp = dsp;
            reopened.failed = failed;
            Ok(reopened)
        }
    }

    /// THE REBASE SEAM (YV91 × YV92), guarded explicitly.
    ///
    /// YV92's reopen restored a boolean gate. YV91 replaced that gate with an
    /// arm WINDOW over cumulative frame indices — and those indices belong to
    /// the ring, which a reopen rebuilds from zero. Carrying the old window
    /// across a reopen therefore leaves `from` at a frame index the new stream
    /// reaches hours later (or never), so `armed_range` returns `None` for
    /// every block and the rest of the take is routed to nobody: OS-9's failure
    /// mode, re-created by composing two fixes that are each correct alone.
    ///
    /// This asserts the frame SPACE, not just "audio kept arriving", because
    /// that is the part a future refactor can silently get wrong.
    #[test]
    fn a_reopen_mid_take_rearms_the_window_in_the_new_rings_frame_space() {
        let mut live = hardware_free_stream("MacBook Pro Microphone", fmt(48_000));
        live.begin(None, None);
        // Enough frames that the old window's `from` is far from zero AND the
        // ring has advanced well past where the new one will start.
        for _ in 0..8 {
            feed_one_callback(&live, &tone_at(48_000, 0.2, 220.0, 0.3));
        }
        let frames_before = live.rt.frames_accepted();
        assert!(
            frames_before > 0,
            "the old ring is deep into its own frame space"
        );
        let before = live.output_samples();

        reopen_with(
            &mut live,
            Some(fmt(24_000)),
            hardware_free_reopen("AirPods Pro", fmt(24_000)),
        );

        assert_eq!(
            live.rt.frames_accepted(),
            0,
            "a reopen builds a NEW ring — its frame counter starts at zero"
        );
        let (from, to) = live.window.bounds();
        assert_eq!(
            (from, to),
            (0, u64::MAX),
            "so the take's window has to be re-opened at THIS ring's origin, \
             not left pointing into the closed stream's frame space"
        );
        assert!(live.is_capturing() && live.is_armed());

        feed_one_callback(&live, &tone_at(24_000, 0.2, 220.0, 0.3));
        assert!(
            live.output_samples() > before,
            "and the frames the new device delivers land in the take"
        );
    }

    /// A reopen while NOTHING is armed must not adopt a take that does not
    /// exist — the symmetric error, and the one that would hand the next
    /// dictation the seconds of room tone that preceded it.
    #[test]
    fn a_reopen_with_no_take_in_flight_leaves_the_window_shut() {
        let mut live = hardware_free_stream("MacBook Pro Microphone", fmt(48_000));
        assert!(!live.is_armed());
        reopen_with(
            &mut live,
            Some(fmt(24_000)),
            hardware_free_reopen("AirPods Pro", fmt(24_000)),
        );
        assert!(!live.is_capturing(), "no take, no window");
        let before = live.output_samples();
        feed_one_callback(&live, &tone_at(24_000, 0.2, 220.0, 0.3));
        assert_eq!(
            live.output_samples(),
            before,
            "and idle frames belong to nobody"
        );
    }

    /// YV92 review — the failure mode this whole item exists to remove, put back
    /// by the fix for it.
    ///
    /// `reopen_after_change` used to read the take's liveness off the momentary
    /// capture gate and then immediately close that gate. That is only correct
    /// while every reopen succeeds. Fail one — a transient AirPods/HFP error is
    /// ordinary — and the gate stays shut; the self-healing retry on the next
    /// watchdog tick (`Unchanged` + `failed` → `reopen_after_change(.., None)`)
    /// then re-read the gate as *already false* and, on a perfectly successful
    /// reopen, restored false. Live stream, intact DSP, and every remaining
    /// frame of a three-hour take dropped at the gate with nothing logged.
    #[test]
    fn a_transient_reopen_failure_still_resumes_the_take() {
        let mut live = hardware_free_stream("MacBook Pro Microphone", fmt(48_000));
        live.begin(None, None);
        feed_one_callback(&live, &tone_at(48_000, 0.2, 220.0, 0.3));
        let before = live.output_samples();
        assert!(before > 0, "the take is recording before the swap");

        // The AirPods go in and the reopen at the new rate fails.
        reopen_with(&mut live, Some(fmt(24_000)), |_| {
            Err("mic stream: device busy".to_string())
        });
        assert!(
            !live.is_capturing(),
            "with no stream open the gate is shut — nothing to gate"
        );
        assert!(
            live.is_armed(),
            "but the TAKE is still in flight; a device error does not end it"
        );
        assert!(live.has_failed(), "…and the watchdog is told to retry");
        assert_eq!(
            live.output_samples(),
            before,
            "the audio captured before the swap is kept (YV67)"
        );

        // The next watchdog tick: `Unchanged` + failed → reopen at the same
        // format. This is the retry that used to restore `false`.
        reopen_with(
            &mut live,
            None,
            hardware_free_reopen("AirPods Pro", fmt(24_000)),
        );
        assert!(
            live.is_capturing(),
            "a successful retry must reopen the gate for the take still running"
        );
        assert!(!live.has_failed(), "the flag belonged to the closed stream");

        feed_one_callback(&live, &tone_at(24_000, 0.2, 220.0, 0.3));
        assert!(
            live.output_samples() > before,
            "and post-retry frames must land in the take, not be dropped at the gate"
        );

        // The other direction: a take that ENDED must not be resurrected by a
        // reopen the watchdog does afterwards.
        let _ = live.end();
        assert!(!live.is_armed());
        reopen_with(
            &mut live,
            None,
            hardware_free_reopen("AirPods Pro", fmt(24_000)),
        );
        assert!(
            !live.is_capturing(),
            "a reopen restores the take's intent — it does not invent one"
        );
    }

    /// WF-10 — the acceptance test. A fake device list: the pinned Shure MV7,
    /// AirPods ranked next, and BlackHole as the system default (virtual, so
    /// never auto-picked). The MV7 is unplugged mid-take: the stream errors,
    /// the resolver walks the ranking to the AirPods, the take is carried
    /// across, nothing captured before the removal is lost, the AirPods' audio
    /// lands in the SAME take, and the pill gets its "continuing on" line.
    #[test]
    fn removal_mid_take_switches_to_the_next_mic_without_losing_captured_audio() {
        use crate::mic_select::{resolve_ranked, Candidate, Resolution};
        fn cand(id: &str, device: &'static str, is_virtual: bool) -> Candidate<&'static str> {
            Candidate {
                id: id.to_string(),
                device,
                is_virtual,
            }
        }
        let order: Vec<String> = vec!["coreaudio:MV7".to_string(), "coreaudio:AirPods".to_string()];
        let plugged = vec![
            cand("coreaudio:MV7", "Shure MV7", false),
            cand("coreaudio:AirPods", "AirPods Pro", false),
            cand("coreaudio:BlackHole", "BlackHole 2ch", true),
        ];
        assert_eq!(
            resolve_ranked(
                Some("coreaudio:MV7"),
                &order,
                &plugged,
                Some("coreaudio:BlackHole")
            ),
            Resolution::Chosen("Shure MV7")
        );

        let mut live = hardware_free_stream("Shure MV7", fmt(48_000));
        live.begin(None, None);
        for _ in 0..4 {
            feed_one_callback(&live, &tone_at(48_000, 0.2, 220.0, 0.3));
        }
        let captured_before = live.output_samples();
        assert!(captured_before > 0, "the take is recording on the MV7");

        // Unplugged: cpal's error callback flags the stream, and the device
        // list the watchdog re-resolves against no longer has the MV7.
        live.failed.store(true, Ordering::SeqCst);
        let unplugged: Vec<_> = plugged
            .iter()
            .filter(|c| c.id != "coreaudio:MV7")
            .cloned()
            .collect();
        let next = resolve_ranked(
            Some("coreaudio:MV7"),
            &order,
            &unplugged,
            Some("coreaudio:BlackHole"),
        );
        assert_eq!(
            next,
            Resolution::FellBackToNext {
                device: "AirPods Pro",
                chosen_missing: true
            },
            "the next RANKED mic, never the virtual system default"
        );
        let next = next.device().expect("a physical mic is still plugged in");

        let seq = mic_switch_seq();
        reopen_with(&mut live, None, hardware_free_reopen(next, fmt(24_000)));
        assert_eq!(live.device_name, "AirPods Pro");
        assert!(live.is_armed() && live.is_capturing(), "the take continues");
        assert!(!live.has_failed());
        assert!(
            live.output_samples() >= captured_before,
            "every sample captured before the removal is kept"
        );
        assert!(mic_switch_seq() > seq, "the pill is told to redraw");
        assert!(mid_take_mic_note().is_some());

        feed_one_callback(&live, &tone_at(24_000, 0.2, 220.0, 0.3));
        assert!(
            live.output_samples() > captured_before,
            "the AirPods' audio lands in the same take"
        );
        let taken = live.end();
        assert!(
            taken.samples.len() as u64 > captured_before,
            "the finished take holds both halves"
        );
    }

    #[test]
    fn the_pill_line_names_the_mic_the_take_continues_on() {
        assert_eq!(
            mic_switch_note("AirPods Pro", true),
            "Mic disconnected — continuing on AirPods Pro"
        );
        assert_eq!(
            mic_switch_note("  ", true),
            "Mic disconnected — continuing on another microphone"
        );
        assert_eq!(
            mic_switch_note("Shure MV7", false),
            "Switched to Shure MV7 — still recording"
        );
        // A format change on the same mic, or no take in flight, is no switch.
        assert!(is_mid_take_switch("Shure MV7", "AirPods Pro", true));
        assert!(!is_mid_take_switch("AirPods Pro", "AirPods Pro", true));
        assert!(!is_mid_take_switch("Shure MV7", "AirPods Pro", false));
        assert!(!is_mid_take_switch("Shure MV7", "", true));
    }

    #[test]
    fn the_worker_ticks_fast_only_while_audio_is_wanted() {
        assert_eq!(ARMED_TICK.cmp(&IDLE_TICK), std::cmp::Ordering::Less);
        assert_eq!(FAILED_RETRY.cmp(&WATCHDOG_TICK), std::cmp::Ordering::Less);
        let code = include_str!("record.rs");
        assert!(code.contains("let failed_due"));
        assert!(code.contains("clear_mid_take_note();"));
    }

    /// YV92/OS-9 — the hole the format half of the cache key does NOT close,
    /// because the one production lookup asks with `None`: the stream idle-closes
    /// after 60 s, the AirPods mic engages and renegotiates to HFP while nothing
    /// is open (so no watchdog is running to notice), and the next Arm is a cold
    /// open served from a cached 48 kHz config the device no longer runs.
    ///
    /// What closes it is `close_idle_capture` retiring the cache with the stream.
    #[test]
    fn arming_cold_after_an_idle_close_opens_at_the_new_rate() {
        let mut cache: TestCache = DeviceConfigCache::new();
        // A take on the AirPods at 48 kHz warms the cache.
        cache.store("AirPods Pro".into(), fmt(48_000), "bt", 48_000);
        assert_eq!(
            resolve_device_config(&mut cache, "AirPods Pro", "bt", |_| Ok(48_000)).unwrap(),
            ("bt", 48_000, true),
            "inside the idle window the warm path still skips the HAL query"
        );

        // 60 s with no take: the stream closes, the listeners come off, and the
        // worker goes back to blocking on its channel.
        close_idle_capture(&mut cache);
        assert!(
            !cache.is_cached(),
            "nothing is watching the input any more, so nothing may be remembered about it"
        );

        // Meanwhile the AirPods mic engages: 48000 → 16000 (HFP/SCO). The next
        // Arm is a cold open and must ask the hardware, not the cache.
        let (_, rate, was_cached) =
            resolve_device_config(&mut cache, "AirPods Pro", "bt", |_| Ok(16_000)).unwrap();
        assert!(
            !was_cached,
            "an idle close must not leave a warm config behind"
        );
        assert_eq!(
            rate, 16_000,
            "the cold Arm opens at the rate the device runs NOW — a stale 48000 here is \
             three hours of time-stretched audio nobody looks at until stop"
        );

        // And a failed query never leaves a live-looking entry behind.
        cache.store("AirPods Pro".into(), fmt(16_000), "bt", 16_000);
        assert!(
            resolve_device_config(&mut cache, "MacBook Pro Microphone", "builtin", |_| Err(
                "Mic config failed".to_string()
            ))
            .is_err()
        );
        assert!(!cache.is_cached());
    }

    /// YV67 — the `Disarm` hand-off must report a mid-take device error WITHOUT
    /// touching the audio. A mic that was unplugged mid-hold still captured
    /// everything up to that moment; dropping it is what produced a silently
    /// truncated transcript with nothing to retry.
    #[test]
    fn disarm_reports_device_failure_without_dropping_samples() {
        let partial = tone_at(SR, 0.4, 220.0, 0.2);
        let captured = mark_device_failure(
            CapturedAudio {
                samples: partial.clone(),
                spill: None,
                sample_rate: SR,
                gain: 1.0,
                journal: None,
                device_failed: false,
                analysis: None,
            },
            true,
        );
        assert!(
            captured.device_failed,
            "the mid-take device error must ride out with the take"
        );
        assert!(
            !captured.samples.is_empty(),
            "the partial audio must survive Disarm"
        );
        assert_eq!(
            captured.samples, partial,
            "Disarm must not touch the samples"
        );
        // …and a healthy stream still reports a healthy take.
        let healthy = mark_device_failure(
            CapturedAudio {
                samples: partial.clone(),
                spill: None,
                sample_rate: SR,
                gain: 1.0,
                journal: None,
                device_failed: true,
                analysis: None,
            },
            false,
        );
        assert!(!healthy.device_failed);
        assert_eq!(healthy.samples, partial);
    }

    #[test]
    fn finalize_levels_fades_and_never_loses_the_take() {
        let quiet = tone_at(SR, 0.5, 220.0, 0.02);
        let gain = agc_gain(rms(&quiet), max_abs(&quiet), NORMALIZE_TARGET_DBFS);
        let captured = CapturedAudio {
            samples: quiet.clone(),
            spill: None,
            sample_rate: SR,
            gain,
            journal: None,
            device_failed: false,
            analysis: None,
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

        // Never lose audio (Y3): a 16 kHz capture that went non-finite is
        // SALVAGED — the garbage samples are zeroed and the batch chain runs
        // over everything else — instead of losing the take. Before Y3 this
        // rebuilt from a native-rate `raw` Vec that no longer exists.
        let mut broken = tone_at(SR, 0.5, 220.0, 0.3);
        broken[100] = f32::NAN;
        let broken_len = broken.len();
        let out = finalize_take(
            CapturedAudio {
                samples: broken,
                spill: None,
                sample_rate: NATIVE_SR,
                gain: 1.0,
                journal: None,
                device_failed: false,
                analysis: None,
            },
            false,
        )
        .expect("the salvage still produces the utterance");
        assert_eq!(
            out.len(),
            broken_len,
            "the salvage keeps every sample it can, at 16 kHz"
        );
        assert!(out.iter().all(|s| s.is_finite()) && rms(&out) > 0.0);

        // A take with NO audio at all is an actionable error, never silence.
        let err = finalize_take(
            CapturedAudio {
                samples: Vec::new(),
                spill: None,
                sample_rate: NATIVE_SR,
                gain: 1.0,
                journal: None,
                device_failed: false,
                analysis: None,
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

    /// yap-ptt-capture-tail (D4): the capture must end within
    /// `STOP_EDGE_BUDGET` of the stop edge even when the focus/AX context reads
    /// stall (the walk's ~3.3 s). The pre-fix order — reads, then the worker's
    /// Disarm — ends it after the stall and fails this.
    #[test]
    fn capture_ends_within_budget_of_the_stop_edge_while_context_reads_stall() {
        let edge = Instant::now();
        let (capture, ctx) = end_at_stop_edge(
            move || edge.elapsed(),
            || {
                thread::sleep(Duration::from_millis(1200));
                "context"
            },
        );
        let ended_after = capture.join().unwrap();
        assert_eq!(ctx, "context");
        assert!(
            ended_after <= STOP_EDGE_BUDGET,
            "capture ended {ended_after:?} after the stop edge (budget {STOP_EDGE_BUDGET:?})"
        );
    }

    /// WF-09 — a microphone picked while the warm stream is idle is used by the
    /// NEXT take: Arm drops the stream opened at the old pick. A stream that is
    /// mid-take or feeding a meeting is never dropped for a pick.
    #[test]
    fn a_mic_pick_reopens_the_warm_stream_for_the_next_take() {
        // Idle warm stream, pick moved: reopen.
        assert!(should_reopen_for_mic_pick(true, false, false, false, 3, 4));
        // Same pick: keep the warm path (YV35).
        assert!(!should_reopen_for_mic_pick(true, false, false, false, 4, 4));
        // No stream open: the cold open already resolves the new pick.
        assert!(!should_reopen_for_mic_pick(
            false, false, false, false, 3, 4
        ));
        // Mid-take, held for a meeting, or a meeting running: never.
        assert!(!should_reopen_for_mic_pick(true, true, false, false, 3, 4));
        assert!(!should_reopen_for_mic_pick(true, false, true, false, 3, 4));
        assert!(!should_reopen_for_mic_pick(true, false, false, true, 3, 4));
    }

    /// WF-09 — the opener and the watchdog both resolve the device through the
    /// picker, never straight through cpal's default (which would silently
    /// ignore the user's pick, or swap a pinned take back to the default on
    /// the next watchdog tick).
    #[test]
    fn capture_opens_the_picked_device_not_the_raw_default() {
        let src = include_str!("record.rs");
        // Production code only: the assertions below must not be satisfied by
        // their own string literals in this test module.
        let prod = src
            .split("#[cfg(test)]\nmod tests {")
            .next()
            .expect("record.rs has a body");
        let code: String = prod
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(code.contains("crate::mic_select::input_device_for_next_take()"));
        assert!(code.contains("let (device, dev_name) = chosen_input_device()?;"));
        assert!(code.contains("let observed = match chosen_input_device() {"));
        let raw_default = ["host", ".default_input_device()"].concat();
        assert!(
            !code.contains(&raw_default),
            "record.rs must not open the system default directly"
        );
    }

    // ── SP-06: the release DSP streamed during capture ──────────────────────

    fn sp06_bits(v: &[f32]) -> Vec<u32> {
        v.iter().map(|x| x.to_bits()).collect()
    }

    /// Speech-like voicing over hiss, with silence either side.
    fn sp06_take() -> Vec<f32> {
        let sr = TARGET_RATE;
        let mut take = vec![0.0f32; sr as usize / 2];
        take.extend(voiced(1.5, sr, 0.2));
        take.resize(take.len() + sr as usize / 2, 0.0);
        let hiss = broadband_noise(take.len(), 0.01, 0x5106);
        take.iter().zip(hiss).map(|(a, b)| a + b).collect()
    }

    #[test]
    fn linear_stream_is_byte_identical_to_resample_linear() {
        let x = sp06_take();
        for (from, to) in [(TARGET_RATE, RNNOISE_SR), (RNNOISE_SR, TARGET_RATE)] {
            let want = resample_linear(&x, from, to);
            for chunk in [1usize, 7, 170, 481, x.len()] {
                let mut stream = LinearStream::new(from, to);
                let mut got = Vec::new();
                for c in x.chunks(chunk) {
                    stream.push(c, &mut got);
                }
                stream.finish(&mut got);
                assert_eq!(
                    sp06_bits(&got),
                    sp06_bits(&want),
                    "{from}->{to} chunk {chunk}"
                );
                assert_eq!(stream.produced(), want.len());
            }
        }
    }

    #[test]
    fn streamed_fade_and_denoise_are_byte_identical_to_the_batch_chain() {
        let x = sp06_take();
        let faded = edge_fade(&apply_gain(&x, 1.0), TARGET_RATE, EDGE_FADE_MS);
        let want = denoise_rnnoise(&faded, TARGET_RATE);
        // The batch guard kept the denoised take, so the stream must too.
        assert_ne!(sp06_bits(&want), sp06_bits(&faded));
        for chunk in [1usize, 170, 997] {
            let mut analysis = TakeAnalysis::new(true, None);
            for c in x.chunks(chunk) {
                analysis.push(c, 1.0);
            }
            assert_eq!(analysis.fed(), x.len());
            let done = analysis.finish(1.0).expect("the streamed chain finishes");
            assert!(done.vad.is_none());
            assert_eq!(sp06_bits(&done.out), sp06_bits(&want), "chunk {chunk}");
        }
        // Denoise off: the stream is the batch edge fade.
        let mut analysis = TakeAnalysis::new(false, None);
        for c in x.chunks(170) {
            analysis.push(c, 1.0);
        }
        let done = analysis.finish(1.0).expect("fade-only stream finishes");
        assert_eq!(sp06_bits(&done.out), sp06_bits(&faded));
    }

    #[test]
    fn streamed_chain_refuses_what_the_batch_chain_must_decide() {
        // Shorter than its two fades.
        let mut short = TakeAnalysis::new(true, None);
        short.push(&[0.1; 100], 1.0);
        assert!(short.finish(1.0).is_err());
        // A non-finite sample: the batch chain's salvage path owns it.
        let mut bad = TakeAnalysis::new(true, None);
        let mut x = sp06_take();
        x[1000] = f32::NAN;
        bad.push(&x, 1.0);
        assert!(bad.finish(1.0).is_err());
    }

    #[test]
    fn streamed_bench_release_matches_the_batch_release_on_the_fixture() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("quick-brown-fox-16k.wav");
        let raw = read_wav_16k_mono(&fixture).expect("fixture loads");
        let leveled = bench_capture_chain(&raw);
        // The real Silero when it is cached; the energy-VAD path otherwise.
        let model = crate::models::models_dir().join("silero_vad_v4.onnx");
        let warm = if model.exists() {
            vad::WarmVad::load(&model).ok()
        } else {
            None
        };
        for denoise in [true, false] {
            let want = bench_release_dsp(&leveled, denoise, warm.as_ref()).expect("batch");
            let streamed = bench_capture_stream(&leveled, denoise, warm.as_ref());
            assert_eq!(streamed.is_some(), denoise || warm.is_some());
            let got = bench_release_streamed(&leveled, denoise, warm.as_ref(), streamed)
                .expect("streamed");
            assert_eq!(
                sp06_bits(&got.samples),
                sp06_bits(&want.samples),
                "denoise={denoise}"
            );
            // Energy-VAD (WPM denominator) is unchanged.
            assert_eq!(got.voiced_seconds.to_bits(), want.voiced_seconds.to_bits());
            assert_eq!(got.speech_present, want.speech_present);
        }
    }

    #[test]
    fn a_take_hands_its_analysis_over_only_when_it_saw_every_sample() {
        let x = sp06_take();
        let mut dsp = StreamDsp::new(TARGET_RATE, 1);
        dsp.analysis = Some(Box::new(TakeAnalysis::new(true, None)));
        for c in x.chunks(170) {
            dsp.push(c);
        }
        let captured = dsp.take();
        let analysis = captured
            .analysis
            .expect("the analysis rides out with the take");
        assert_eq!(analysis.fed(), captured.samples.len());
        assert!(analysis.finish(captured.gain).is_ok());

        // Installed late, it missed the head of the take: never trusted.
        let mut dsp = StreamDsp::new(TARGET_RATE, 1);
        for c in x.chunks(170).take(3) {
            dsp.push(c);
        }
        dsp.analysis = Some(Box::new(TakeAnalysis::new(true, None)));
        for c in x.chunks(170).skip(3) {
            dsp.push(c);
        }
        assert!(dsp.take().analysis.is_none());
    }
}

#[cfg(test)]
mod silent_mic_tests {
    //! WF-13 — the silent-mic level check, on recorded fixtures.
    use super::*;

    const RATE: u32 = 16_000;

    fn fox() -> Vec<f32> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/quick-brown-fox-16k.wav"
        );
        read_wav_16k_mono(Path::new(path)).expect("fox fixture reads")
    }

    /// The fox recording turned into QUIET speech: started at the first word
    /// and attenuated 30 dB (x0.0316) — a soft talker far from the mic.
    fn quiet_speech() -> Vec<f32> {
        let loud = fox();
        let onset = loud.iter().position(|s| s.abs() > 0.05).unwrap_or(0);
        let start = onset.saturating_sub(RATE as usize / 10);
        loud[start..].iter().map(|s| s * 0.0316).collect()
    }

    /// A muted interface as it really records: a DC offset plus a couple of
    /// LSBs of 16-bit converter noise (deterministic, so the test is stable).
    fn recorded_silence(seconds: f32) -> Vec<f32> {
        let n = (RATE as f32 * seconds) as usize;
        let lsb = 1.0 / 32_768.0;
        (0..n)
            .map(|i| 0.01 + lsb * [0.0, 1.0, -1.0, 2.0, -2.0, 1.0, 0.0][i % 7])
            .collect()
    }

    /// Feed `samples` in 10 ms callbacks, as the consumer does; collect events.
    fn run(check: &mut SilentMicCheck, samples: &[f32], channels: usize) -> Vec<SilentMicEvent> {
        samples
            .chunks(160 * channels)
            .map(|c| check.feed(c))
            .filter(|e| *e != SilentMicEvent::None)
            .collect()
    }

    #[test]
    fn silent_mic_recorded_silence_warns_after_the_window() {
        let mut check = SilentMicCheck::new(RATE, 1);
        let silence = recorded_silence(1.4);
        assert!(run(&mut check, &silence, 1).is_empty(), "not before 1.5 s");
        assert!(!check.decided());
        let more = recorded_silence(0.2);
        assert_eq!(run(&mut check, &more, 1), vec![SilentMicEvent::Silent]);
    }

    #[test]
    fn silent_mic_digital_zero_warns() {
        let mut check = SilentMicCheck::new(48_000, 2);
        let zeros = vec![0.0f32; 48_000 * 2 * 2];
        assert_eq!(run(&mut check, &zeros, 2), vec![SilentMicEvent::Silent]);
    }

    #[test]
    fn silent_mic_quiet_speech_never_warns() {
        let quiet = quiet_speech();
        assert!(
            quiet.len() >= RATE as usize * 3 / 2,
            "fixture covers the window"
        );
        let mut check = SilentMicCheck::new(RATE, 1);
        assert!(
            run(&mut check, &quiet, 1).is_empty(),
            "quiet speech is not a silent mic"
        );
        assert!(check.decided());
    }

    #[test]
    fn silent_mic_full_level_speech_never_warns() {
        let mut check = SilentMicCheck::new(RATE, 1);
        assert!(run(&mut check, &fox(), 1).is_empty());
    }

    #[test]
    fn silent_mic_fires_once_per_take_even_on_long_silence() {
        let mut check = SilentMicCheck::new(RATE, 1);
        let events = run(&mut check, &recorded_silence(20.0), 1);
        assert_eq!(
            events,
            vec![SilentMicEvent::Silent],
            "one warning, not one per block"
        );
    }

    #[test]
    fn silent_mic_recovers_once_when_speech_arrives() {
        let mut check = SilentMicCheck::new(RATE, 1);
        let mut take = recorded_silence(2.0);
        take.extend(quiet_speech());
        take.extend(recorded_silence(3.0));
        let events = run(&mut check, &take, 1);
        assert_eq!(
            events,
            vec![SilentMicEvent::Silent, SilentMicEvent::Recovered]
        );
    }

    #[test]
    fn silent_mic_once_gate_holds_across_a_mid_take_reopen() {
        let fired = AtomicU64::new(0);
        assert!(claim_once(&fired, 7), "first device of take 7 warns");
        assert!(
            !claim_once(&fired, 7),
            "the reopened device of take 7 does not"
        );
        assert!(claim_once(&fired, 8), "the next take may warn again");
        assert!(!claim_once(&fired, 0), "no take armed yet: never");
    }

    #[test]
    fn silent_mic_note_names_the_mic() {
        assert_eq!(silent_mic_note("Shure MV7"), "I can't hear Shure MV7");
        assert_eq!(silent_mic_note("  "), "I can't hear your microphone");
    }

    #[test]
    fn silent_mic_check_is_wired_into_the_consumer_and_arm() {
        // Only the code ABOVE this test module counts (the asserts below
        // would otherwise find their own string literals).
        let full = include_str!("record.rs");
        let code = full.split("mod silent_mic_tests").next().unwrap_or(full);
        assert!(code.len() < full.len(), "the test module marker is found");
        assert!(code.contains("apply_silent_mic_event(check.feed("));
        assert!(code.contains("begin_silent_mic_take(&stream.device_name);"));
        assert!(code.contains("retarget_silent_mic(&live.device_name);"));
    }
}
