//! In-process microphone capture via cpal (cpal 0.18).
//!
//! Recording inside the Wilson Voice process (not external ffmpeg) so macOS TCC
//! attributes Microphone permission to com.wilsonguenther.wilson-voice.
//! Live RMS level is exposed for the floating HUD waveform.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
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
const TARGET_RATE: u32 = 16_000;
/// Shortest take worth transcribing: 30 ms at the target rate. Below this it was
/// a stray tap, not speech (this used to be a "wav smaller than 1 kB" check —
/// same threshold, measured on the in-memory buffer now).
const MIN_CLIP_SAMPLES: usize = TARGET_RATE as usize * 30 / 1000;

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
    /// YV67 — the capture device errored DURING this take (unplugged mic, format
    /// change). The take still comes back with every sample that made it, because
    /// a truncated recording is the user's audio; the pipeline turns this into a
    /// retryable failure instead of pasting a silently cut-off transcript. Until
    /// YV67 the flag was only read on the NEXT arm, so the take it truncated
    /// looked perfectly healthy.
    pub device_failed: bool,
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
) -> Result<ActiveRecording, String> {
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let wav_path = dir.join(format!("{}.wav", Uuid::new_v4()));
    // The hold clock starts at the request, NOT after the handshake, so
    // `hold_wall_seconds` reports the real press→release wall time.
    let started = Instant::now();
    let journal = CaptureJournal::start(journal_dir);

    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    dispatch(CaptureCmd::Arm {
        journal,
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
    // YV38: signal (not a flag another thread has to notice) — the HUD level
    // thread is woken here, on release, instead of up to 50 ms afterwards.
    active.stop.stop();
    active.level.store(0, Ordering::Relaxed);

    // Stop buffering and take the raw capture off the persistent worker; the
    // stream itself stays open for the next take (see `capture_worker_loop`).
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    dispatch(CaptureCmd::Disarm { reply: reply_tx })?;
    let mut captured = await_reply(&reply_rx, DISARM_TIMEOUT, "stop")?;
    // YV63: the take is off the capture worker, so its crash journal comes back
    // with it. Held here (not passed on to the DSP) so EVERY exit from this
    // function retires it — the early return below drops it, which cleans the
    // marker + spill exactly like the normal completion at the end does.
    let journal = captured.journal.take();
    // YV67: the device's health at Disarm belongs to THIS take — read it off
    // before `finalize_take` consumes the capture.
    let device_failed = captured.device_failed;
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
        hold_wall_seconds,
        speech_present,
        capture_start_ms: active.capture_start_ms,
        device_failed,
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
        Self {
            path,
            writer,
            kept: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Test-only guard over a wav that is ALREADY on disk, so the recovery path
    /// (YV52/YV67) can be driven from a unit test without a capture device. Same
    /// drop semantics as a real clip — it unlinks unless `keep_for_recovery` ran.
    #[cfg(test)]
    pub fn adopt_for_test(path: PathBuf) -> Self {
        Self {
            path,
            writer: None,
            kept: false,
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
        Ok(target)
    }
}

impl Drop for ClipWav {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
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
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / i16::MAX as f32)
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
    /// YV63 crash journal, handed back with the take so `stop_recording` can
    /// retire it. `None` when journalling could not be started for this take.
    journal: Option<CaptureJournal>,
    /// YV67 — stamped at Disarm from `LiveStream::has_failed`: did the capture
    /// device error while this take was being held? The DSP never sets it; the
    /// worker does, because only the worker owns the stream.
    device_failed: bool,
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
    /// Streaming DSP + buffers, written by the audio callback (YV37).
    dsp: Arc<Mutex<StreamDsp>>,
    /// The momentary GATE the audio callback reads: push this buffer, or drop
    /// it. Shared with the callback, so a reopen closes and reopens it.
    capturing: Arc<AtomicBool>,
    /// The take's ARMED INTENT — "a take is in flight and wants audio" — as
    /// opposed to `capturing`, which is only whether the gate is open right
    /// now (YV92 review).
    ///
    /// The two differ for exactly as long as a reopen takes, and conflating
    /// them re-created OS-9's failure mode from the inside: `reopen_after_change`
    /// read the momentary gate, closed it, and on a failed reopen never restored
    /// it — so the self-healing retry on the next watchdog tick read the gate as
    /// already-closed and faithfully restored *false*. One transient AirPods/HFP
    /// reopen error and the stream came back live, the DSP intact, and every
    /// remaining frame of a three-hour take dropped at the gate with nothing
    /// logged. Armed intent is set by [`LiveStream::begin`] and cleared by
    /// [`LiveStream::end`] — by the take, never by the device — so a reopen has
    /// something durable to restore the gate *to*.
    armed: AtomicBool,
    failed: Arc<AtomicBool>,
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
}

impl LiveStream {
    /// Begin a take: drop anything the callback saw while idle (and every bit of
    /// the previous take's DSP state), arm this take's crash journal, zero the
    /// meter, then open the gate. The journal is installed AFTER the reset — the
    /// reset is what retires a previous take's journal, and the segment
    /// numbering restarts with it because the marker sidecar is per-take.
    fn begin(&mut self, journal: Option<CaptureJournal>) {
        if let Ok(mut dsp) = self.dsp.lock() {
            dsp.reset();
            dsp.journal = journal;
        }
        self.watch.restart_segments();
        capture_level().store(0, Ordering::Relaxed);
        // Intent first, gate second: a watchdog tick that lands between the two
        // must never see an open gate with nothing armed behind it.
        self.armed.store(true, Ordering::SeqCst);
        self.capturing.store(true, Ordering::SeqCst);
    }

    /// End a take: close the gate and take the streamed 16 kHz buffer (plus its
    /// raw fallback and AGC stats) off the DSP state.
    fn end(&self) -> CapturedAudio {
        // Disarm before closing the gate — the take is over, so a reopen racing
        // this must not resurrect it.
        self.armed.store(false, Ordering::SeqCst);
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
                journal: None,
                device_failed: false,
            })
    }

    fn is_capturing(&self) -> bool {
        self.capturing.load(Ordering::SeqCst)
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
        self.dsp.lock().map(|d| d.out.len() as u64).unwrap_or(0)
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

/// The worker thread: owns the persistent stream + the device/config cache and
/// answers every arm/stop with an explicit signal (never a poll).
fn capture_worker_loop(rx: mpsc::Receiver<CaptureCmd>) {
    let mut cache: DeviceConfigCache<cpal::Device, cpal::SupportedStreamConfig> =
        DeviceConfigCache::new();
    let mut live: Option<LiveStream> = None;
    let mut idle_since = Instant::now();
    let mut last_watchdog = Instant::now();

    loop {
        // YV81 — the tick exists ONLY to close an open stream that has gone
        // `IDLE_CLOSE` without a take. With no stream open there is nothing for
        // it to close, so the worker blocks on the channel instead of waking
        // every five seconds for the rest of the session: the next command
        // wakes it, which is the same signal-not-poll contract the arm/stop
        // path already keeps.
        let next = if live.is_some() {
            rx.recv_timeout(IDLE_TICK)
        } else {
            rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        match next {
            Ok(CaptureCmd::Arm { journal, reply }) => {
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
                if let Some(stream) = live.as_mut() {
                    if stream.is_capturing() {
                        log::warn!("arm while already capturing — dropping the orphaned take");
                    }
                    stream.begin(journal);
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
                    None => Err(NO_SAMPLES_ERR.to_string()),
                };
                let _ = reply.send(captured);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // YV92 — the health watchdog. A dictation always has an Arm
                // coming to re-check the device; a long capture does not, so
                // this is the only thing standing between a mid-session format
                // change and three hours of half-speed audio nobody looks at
                // until stop.
                let event = input_format::take_event();
                match live.as_mut() {
                    Some(stream) => {
                        if event.is_some() || last_watchdog.elapsed() >= WATCHDOG_TICK {
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
                let idle = live.as_ref().is_some_and(|s| !s.is_capturing());
                if idle && idle_since.elapsed() >= IDLE_CLOSE {
                    live = None;
                    close_idle_capture(&mut cache);
                    log::info!(
                        "capture stream closed after {}s idle (mic indicator off)",
                        IDLE_CLOSE.as_secs()
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
    let observed = match default_input_device() {
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
/// The gate is restored from the take's ARMED INTENT, never from the momentary
/// `is_capturing()` this function just closed. Reading the gate to decide what
/// to restore the gate to is only correct while every reopen succeeds: after a
/// failed attempt the gate is already false, so the self-healing retry on the
/// next watchdog tick would "restore" false onto a perfectly live stream and
/// silently drop the rest of the take (YV92 review).
fn reopen_with(
    live: &mut LiveStream,
    to: Option<InputFormat>,
    open: impl FnOnce(TakeInFlight) -> Result<LiveStream, String>,
) {
    live.capturing.store(false, Ordering::SeqCst);
    live.stream = None;
    let reuse = (
        live.dsp.clone(),
        live.capturing.clone(),
        live.failed.clone(),
    );
    match open(reuse) {
        Ok(reopened) => {
            live.stream = reopened.stream;
            // `reopened.watch` is discarded on purpose — the take's segment
            // numbering lives on `live`, and the reopen only tells it what
            // actually opened.
            live.adopt_reopened(reopened.device_name, reopened.format);
            live.capturing.store(live.is_armed(), Ordering::SeqCst);
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

/// The system default input device and the name it reports, or the "no mic"
/// error. Split out so the format watchdog can ask the same question the opener
/// asks without duplicating cpal's default-device rules.
fn default_input_device() -> Result<(cpal::Device, String), String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| NO_MIC_ERR.to_string())?;
    let name = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_default();
    Ok((device, name))
}

/// The three things a reopened stream inherits from the take that was already
/// running: the DSP (which owns the buffered audio, the AGC statistics and the
/// crash journal), the capture gate, and the error flag. Named so the reopen
/// signature reads as "carry the take across", not as a tuple of Arcs.
type TakeInFlight = (Arc<Mutex<StreamDsp>>, Arc<AtomicBool>, Arc<AtomicBool>);

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
    let (device, dev_name) = default_input_device()?;

    let (device, supported, was_cached) = resolve_device_config(
        cache,
        &dev_name,
        device,
        |device| {
            device.default_input_config().map_err(|e| {
                format!("Mic config failed ({e}). Enable Microphone for Yap (not Python) in System Settings.")
            })
        },
    )?;

    let sample_rate = supported.sample_rate();
    let channels = supported.channels();
    let sample_format = supported.sample_format();
    let conf: cpal::StreamConfig = supported.into();

    let format = InputFormat::new(sample_rate, channels);
    let (dsp, capturing, failed) = match reuse {
        Some((dsp, capturing, failed)) => {
            // The take survives the device: only the rate-dependent state is
            // rebuilt, and the error flag is cleared because it belonged to the
            // stream we just closed.
            if let Ok(mut dsp) = dsp.lock() {
                dsp.reconfigure(sample_rate, channels);
            }
            failed.store(false, Ordering::SeqCst);
            (dsp, capturing, failed)
        }
        None => (
            Arc::new(Mutex::new(StreamDsp::new(sample_rate, channels))),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        ),
    };

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

    Ok(LiveStream {
        stream: Some(stream),
        dsp,
        capturing,
        // A freshly opened stream is not itself a take. On the reuse path the
        // caller (`reopen_with`) owns the armed intent and restores the gate
        // from ITS copy — this one is discarded with the rest of the shell.
        armed: AtomicBool::new(false),
        failed,
        watch: InputFormatWatch::new(dev_name.clone(), format, TARGET_RATE),
        device_name: dev_name,
        format,
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
    /// YV63 crash journal for the take in flight, installed by `LiveStream::begin`
    /// and handed back by `take`. `None` between takes (and whenever journalling
    /// could not be started), which turns every spill below into a no-op.
    journal: Option<CaptureJournal>,
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
            journal: None,
        }
    }

    /// Drop the previous take entirely (buffers AND filter/resampler state) so
    /// no audio — and no biquad tail — ever bleeds from one dictation into the
    /// next.
    fn reset(&mut self) {
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
        log::warn!(
            "YV92 input format changed mid-capture: {}Hz/{}ch → {sample_rate}Hz/{channels}ch \
             ({} samples captured so far; raw fallback now spans two rates)",
            self.sample_rate,
            self.channels,
            self.out.len()
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
        let before = self.out.len();
        self.resampler.push(&self.mono, &mut self.out);
        // YV63: the same frames go to the crash journal on their way past, so a
        // take the app dies in the middle of is already on disk. Bounded
        // hand-off — if the writer falls behind the SPILL is dropped, never the
        // frame, and the callback never waits on the disk.
        if let Some(journal) = self.journal.as_ref() {
            journal.append(&self.out[before..]);
        }
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
            // Taken (not reset) so the journal rides out with its take — the
            // reset below must not retire a journal `stop_recording` still owns.
            journal: self.journal.take(),
            // A device error is stamped by the worker at Disarm (YV67), not here.
            device_failed: false,
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
        ..
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
const HIGH_PASS_HZ: f32 = 80.0;
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
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
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
            capturing: Arc::new(AtomicBool::new(false)),
            armed: AtomicBool::new(false),
            failed: Arc::new(AtomicBool::new(false)),
            watch: InputFormatWatch::new(device, format, TARGET_RATE),
            device_name: device.to_string(),
            format,
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

    /// Exactly the gate the cpal input callback applies before it touches the
    /// DSP (see `build_capture_stream`): closed gate, dropped buffer.
    fn feed_one_callback(live: &LiveStream, frames: &[f32]) {
        if !live.capturing.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut dsp) = live.dsp.lock() {
            dsp.push(frames);
        }
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
        live.begin(Some(journal));

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
        live.begin(None);

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
        live.begin(None);
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
        move |(dsp, capturing, failed)| {
            if let Ok(mut dsp) = dsp.lock() {
                dsp.reconfigure(format.sample_rate_hz, format.channels);
            }
            failed.store(false, Ordering::SeqCst);
            Ok(LiveStream {
                stream: None,
                dsp,
                capturing,
                armed: AtomicBool::new(false),
                failed,
                watch: InputFormatWatch::new(device, format, TARGET_RATE),
                device_name: device.to_string(),
                format,
            })
        }
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
        live.begin(None);
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
                raw: partial.clone(),
                sample_rate: SR,
                gain: 1.0,
                journal: None,
                device_failed: false,
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
        assert_eq!(captured.raw, partial, "…nor the never-lose-audio fallback");
        // …and a healthy stream still reports a healthy take.
        let healthy = mark_device_failure(
            CapturedAudio {
                samples: partial.clone(),
                raw: Vec::new(),
                sample_rate: SR,
                gain: 1.0,
                journal: None,
                device_failed: true,
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
            raw: Vec::new(),
            sample_rate: SR,
            gain,
            journal: None,
            device_failed: false,
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
                journal: None,
                device_failed: false,
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
                journal: None,
                device_failed: false,
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
                journal: None,
                device_failed: false,
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
