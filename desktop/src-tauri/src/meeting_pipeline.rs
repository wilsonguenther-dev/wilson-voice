//! yap-meetings-fix — the meeting transcription pipeline: Stop → transcript →
//! Complete (→ summary).
//!
//! Until this module, `MeetingController::stop` left every meeting in
//! `transcribing` with the comment "YV93's transcription pipeline moves it on",
//! and there was no such pipeline: `meeting_asr::MeetingAsr` was constructed
//! only by tests, `Database::append_meeting_segments` had no caller outside
//! `db.rs`, and all four of Wilson's meetings were stuck forever.
//!
//! The shape:
//!
//! - ONE named worker thread and a FIFO of meeting ids ([`spawn`]). One job at
//!   a time: there is one warm ASR engine, and dictation owns it first.
//! - Each job runs the SHIPPED [`MeetingAsr`] driver per track (mic = track 0,
//!   call audio = track 1), over [`WavWindows`] (never the whole file in RAM),
//!   with the shipped [`EngineDemand`] so a dictation preempts at a chunk
//!   boundary.
//! - Every finished window lands in SQLite through
//!   [`Database::append_meeting_segments`] (tagged `on_track`) BEFORE the resume
//!   ledger records it; a resume first deletes any segment written past the
//!   ledger, so a crash between the two writes can never duplicate text.
//! - `processed_through_seconds` advances monotonically and a
//!   [`MeetingProgressEvent`] goes out after every window, so the Meetings view
//!   can say "Transcribing 42%".
//! - The row ends `complete` only with at least one segment; `partial` when a
//!   window failed or the capture was short; `failed` with a plain sentence
//!   otherwise. Never `complete` with nothing in it.

use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;

use crate::asr_engine::TimedKind;
use crate::db::Database;
use crate::meeting::{MIC_TRACK as MIC_TRACK_INDEX, SYSTEM_TRACK as SYSTEM_TRACK_INDEX};
use crate::meeting_asr::{
    ChunkAsr, ChunkOutcome, ChunkStatus, EngineDemand, JsonProgressStore, MeetingAsr,
    MeetingAsrConfig, MeetingProgress, ProgressStore, SampleWindows, WavWindows,
};
use crate::meetings::{MeetingState, NewMeetingSegment};

/// Track 0 (the microphone) and track 1 (call audio), as stored on segments.
pub const MIC_TRACK: i64 = MIC_TRACK_INDEX as i64;
pub const SYSTEM_TRACK: i64 = SYSTEM_TRACK_INDEX as i64;

/// The event the Meetings view and the pill draw transcription progress from.
pub const MEETING_PROGRESS_EVENT: &str = "meeting:progress";

/// A stranded row whose WAV is gone (NT1). One honest sentence, never a silent
/// `complete`.
pub const AUDIO_REMOVED_MESSAGE: &str =
    "The audio for this meeting was removed before it was transcribed.";

/// Every window decoded and none of them had words in it.
pub const NO_SPEECH_MESSAGE: &str = "Yap found no speech in this meeting's audio.";

/// Every window failed speech recognition.
pub const ALL_WINDOWS_FAILED_MESSAGE: &str =
    "Yap could not transcribe this meeting — speech recognition failed on all of its audio. \
     Press Retry.";

/// No words anywhere, and SOME (not all) windows failed: neither "no speech"
/// nor "all failed" would be true.
pub const NO_SPEECH_SOME_FAILED_MESSAGE: &str =
    "Yap found no speech in the parts of this meeting it could transcribe — speech \
     recognition failed on the rest. Press Retry.";

/// The sentence for a meeting that produced zero segments. `attempted` is every
/// window run through ASR (this run's `chunks_decoded`, which counts failed
/// attempts too, plus `chunks_resumed`); `failed` is how many of those failed.
/// "No speech" only when nothing failed — a failure is never reported as silence.
pub fn empty_transcript_message(attempted: usize, failed: usize) -> &'static str {
    if failed == 0 {
        NO_SPEECH_MESSAGE
    } else if failed >= attempted {
        ALL_WINDOWS_FAILED_MESSAGE
    } else {
        NO_SPEECH_SOME_FAILED_MESSAGE
    }
}

/// Some windows failed.
pub const SOME_WINDOWS_FAILED_MESSAGE: &str =
    "Part of this meeting could not be transcribed. The rest of the transcript is here.";

/// Set by the exit path so a job stands down at the next window instead of
/// re-loading an engine the app is tearing down.
static QUIT: AtomicBool = AtomicBool::new(false);

/// The app is quitting: stop at the next window boundary. The ledger keeps
/// what is done, and the next launch's reconcile re-queues the rest.
pub fn request_quit() {
    QUIT.store(true, Ordering::SeqCst);
}

fn quitting() -> bool {
    QUIT.load(Ordering::SeqCst)
}

/// What the UI sees after every window and at the end of a job.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingProgressEvent {
    pub id: String,
    pub state: String,
    /// 0–100, across both tracks.
    pub percent: u8,
    pub processed_through_seconds: f64,
    pub duration_seconds: f64,
    pub error: Option<String>,
}

/// Builds the chunk decoder for one job. A factory, not an instance, because
/// the model the user has selected is resolved when the job STARTS (the model
/// may have been downloaded since launch), and because a test hands in a stub.
pub type AsrFactory = Arc<dyn Fn() -> Result<Box<dyn ChunkAsr>, String> + Send + Sync>;
pub type ProgressSink = Arc<dyn Fn(&MeetingProgressEvent) + Send + Sync>;
/// Called once a meeting reaches `complete`/`partial` with a transcript —
/// production runs the local summary model here when one is installed.
pub type AfterTranscript = Arc<dyn Fn(&str) + Send + Sync>;

/// Everything a job needs, owned by the worker.
#[derive(Clone)]
pub struct PipelineDeps {
    pub db: Arc<Database>,
    /// Where the per-track resume ledgers live (`<id>.tN.asr-progress.json`).
    pub progress_dir: PathBuf,
    pub asr: AsrFactory,
    pub demand: Arc<dyn EngineDemand + Send + Sync>,
    pub quit: Arc<dyn Fn() -> bool + Send + Sync>,
    pub sink: ProgressSink,
    pub after_transcript: Option<AfterTranscript>,
    pub config: MeetingAsrConfig,
}

struct Pending {
    queued: HashSet<String>,
    in_flight: usize,
}

/// A handle on the worker. Cheap to clone.
#[derive(Clone)]
pub struct MeetingPipeline {
    tx: Arc<Mutex<mpsc::Sender<String>>>,
    pending: Arc<(Mutex<Pending>, Condvar)>,
    /// yap24-NT7 — the same sink the worker reports through, so a QUEUED
    /// meeting is announced the moment it is handed over: the pill goes from
    /// the recording clock straight to "Transcribing 0%" instead of going
    /// quiet while the ASR model loads for the first window.
    sink: ProgressSink,
}

impl MeetingPipeline {
    /// Queue a meeting. A meeting already queued is not queued twice.
    pub fn enqueue(&self, id: &str) {
        let (lock, _) = &*self.pending;
        if let Ok(mut p) = lock.lock() {
            if !p.queued.insert(id.to_string()) {
                return;
            }
            p.in_flight += 1;
        }
        let sent = self
            .tx
            .lock()
            .map(|tx| tx.send(id.to_string()).is_ok())
            .unwrap_or(false);
        if !sent {
            log::warn!("meeting pipeline: worker is gone, {id} stays queued for next launch");
        } else {
            log::info!("meeting pipeline: queued {id}");
            (self.sink)(&queued_event(id));
        }
    }

    /// Block until every queued job has finished, or `timeout`. Tests and the
    /// headless CLI only — the app never waits on the pipeline.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let (lock, cv) = &*self.pending;
        let Ok(guard) = lock.lock() else {
            return false;
        };
        match cv.wait_timeout_while(guard, timeout, |p| p.in_flight > 0) {
            Ok((p, _)) => p.in_flight == 0,
            Err(_) => false,
        }
    }
}

/// yap24-NT7 — the event a meeting gets the moment it is queued: transcribing,
/// nothing processed yet. Consumers fold percent with `max` for the same id, so
/// a resumed job that was at 60% is not shown going backwards.
pub fn queued_event(id: &str) -> MeetingProgressEvent {
    MeetingProgressEvent {
        id: id.to_string(),
        state: MeetingState::Transcribing.as_str().to_string(),
        percent: 0,
        processed_through_seconds: 0.0,
        duration_seconds: 0.0,
        error: None,
    }
}

static INSTALLED: OnceLock<MeetingPipeline> = OnceLock::new();

/// Make `pipeline` the one the stop path and reconcile hand meetings to.
/// First call wins.
pub fn install(pipeline: MeetingPipeline) -> bool {
    INSTALLED.set(pipeline).is_ok()
}

/// The installed pipeline, if any.
pub fn installed() -> Option<&'static MeetingPipeline> {
    INSTALLED.get()
}

/// Queue on the installed pipeline. Returns false (and logs) when none is
/// installed — the row stays `transcribing` and the next launch re-queues it.
pub fn enqueue(id: &str) -> bool {
    match INSTALLED.get() {
        Some(p) => {
            p.enqueue(id);
            true
        }
        None => {
            log::warn!("meeting pipeline: not installed, {id} waits for the next launch");
            false
        }
    }
}

/// Spawn the ONE worker thread.
pub fn spawn(deps: PipelineDeps) -> MeetingPipeline {
    let (tx, rx) = mpsc::channel::<String>();
    let pending = Arc::new((
        Mutex::new(Pending {
            queued: HashSet::new(),
            in_flight: 0,
        }),
        Condvar::new(),
    ));
    let worker_pending = Arc::clone(&pending);
    let sink = Arc::clone(&deps.sink);
    let spawned = std::thread::Builder::new()
        .name("wv-meeting-pipeline".into())
        .spawn(move || {
            for id in rx {
                let result = catch_unwind(AssertUnwindSafe(|| run_meeting_job(&deps, &id)));
                match result {
                    Ok(Ok(state)) => {
                        log::info!("meeting pipeline: {id} → {}", state.as_str());
                    }
                    Ok(Err(e)) => {
                        log::warn!("meeting pipeline: {id} failed: {e}");
                        let msg = format!("Yap could not transcribe this meeting: {e}");
                        let _ = deps
                            .db
                            .set_meeting_state(&id, MeetingState::Failed, Some(&msg));
                        emit_final(&deps, &id, MeetingState::Failed, Some(msg));
                    }
                    Err(_) => {
                        log::error!("meeting pipeline: job for {id} panicked — marked failed");
                        let msg = "Yap hit an internal error transcribing this meeting. \
                                   Press Retry."
                            .to_string();
                        let _ = deps
                            .db
                            .set_meeting_state(&id, MeetingState::Failed, Some(&msg));
                        emit_final(&deps, &id, MeetingState::Failed, Some(msg));
                    }
                }
                let (lock, cv) = &*worker_pending;
                if let Ok(mut p) = lock.lock() {
                    p.queued.remove(&id);
                    p.in_flight = p.in_flight.saturating_sub(1);
                }
                cv.notify_all();
            }
        });
    if let Err(e) = spawned {
        log::error!("meeting pipeline: worker thread failed to spawn: {e}");
    }
    MeetingPipeline {
        tx: Arc::new(Mutex::new(tx)),
        pending,
        sink,
    }
}

/// The resume-ledger key for one track of one meeting.
pub fn ledger_key(meeting_id: &str, track: i64) -> String {
    format!("{meeting_id}.t{track}")
}

/// Every ledger file a meeting can leave next to its audio — deleted with the
/// WAVs (purge and delete), so nothing outlives the audio it describes.
pub fn ledger_paths(dir: &Path, meeting_id: &str) -> Vec<PathBuf> {
    let store = JsonProgressStore::new(dir);
    [MIC_TRACK, SYSTEM_TRACK]
        .iter()
        .map(|t| store.path_for(&ledger_key(meeting_id, *t)))
        .collect()
}

/// Forget every decoded window and segment of a meeting, so a Retry starts
/// clean rather than resuming past windows that failed.
pub fn reset_for_retry(db: &Database, dir: &Path, meeting_id: &str) -> Result<(), String> {
    for path in ledger_paths(dir, meeting_id) {
        let _ = std::fs::remove_file(path);
    }
    for track in [MIC_TRACK, SYSTEM_TRACK] {
        db.delete_meeting_segments_from(meeting_id, track, 0.0)?;
    }
    db.set_meeting_state(meeting_id, MeetingState::Transcribing, None)
}

/// One window → at most one transcript segment on `track`.
///
/// Only what the window OWNS (its content span, not the overlap it decoded for
/// context) is kept, so two neighbouring windows never both write the words at
/// their seam.
pub fn chunk_segment(chunk: &ChunkOutcome, track: i64) -> Option<NewMeetingSegment> {
    if chunk.status != ChunkStatus::Done {
        return None;
    }
    let lo = chunk.content_start_seconds - 1e-6;
    let hi = chunk.content_end_seconds;
    let owned: Vec<_> = chunk
        .spans
        .iter()
        .filter(|s| s.start_seconds >= lo && s.start_seconds < hi)
        .collect();
    let joinable = matches!(chunk.timestamp_kind, TimedKind::Word | TimedKind::Segment);
    let (start, end, text) = if joinable && !owned.is_empty() {
        let text = owned
            .iter()
            .map(|s| s.text.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        (
            owned.first().map(|s| s.start_seconds).unwrap_or(lo),
            owned.last().map(|s| s.end_seconds).unwrap_or(hi),
            text,
        )
    } else if chunk.spans.is_empty() || !joinable {
        (
            chunk.content_start_seconds,
            chunk.content_end_seconds,
            chunk.text.trim().to_string(),
        )
    } else {
        // Spans exist but all of them sit in the overlap another window owns.
        return None;
    };
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    Some(NewMeetingSegment::new(start, end.max(start), text).on_track(track))
}

/// The ledger the driver writes through: DB segment first, ledger second.
struct SegmentWritingStore<'a> {
    inner: JsonProgressStore,
    db: &'a Database,
    meeting_id: &'a str,
    track: i64,
    /// Seconds of audio already done on earlier tracks of this job.
    done_before: f64,
    total_audio: f64,
    duration: f64,
    deps: &'a PipelineDeps,
}

impl SegmentWritingStore<'_> {
    fn report(&self, processed_on_track: f64) {
        let done = self.done_before + processed_on_track;
        let fraction = if self.total_audio > 0.0 {
            (done / self.total_audio).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let processed = self.duration * fraction;
        let _ = self.db.set_meeting_progress(self.meeting_id, processed);
        (self.deps.sink)(&MeetingProgressEvent {
            id: self.meeting_id.to_string(),
            state: MeetingState::Transcribing.as_str().to_string(),
            // Never 100 while a window is still left: "100%" is "Notes ready".
            percent: ((fraction * 100.0).floor() as u8).min(99),
            processed_through_seconds: processed,
            duration_seconds: self.duration,
            error: None,
        });
    }
}

impl ProgressStore for SegmentWritingStore<'_> {
    fn load(&self, key: &str) -> Result<MeetingProgress, String> {
        self.inner.load(key)
    }

    fn record_chunk(&mut self, key: &str, chunk: &ChunkOutcome) -> Result<(), String> {
        if let Some(segment) = chunk_segment(chunk, self.track) {
            self.db
                .append_meeting_segments(self.meeting_id, std::slice::from_ref(&segment))?;
        }
        self.inner.record_chunk(key, chunk)?;
        let through = self.inner.load(key)?.processed_through_seconds;
        self.report(through);
        // Yield between windows: a dictation that started while this window
        // decoded gets the engine before the next one is even planned.
        std::thread::yield_now();
        Ok(())
    }
}

fn emit_final(deps: &PipelineDeps, id: &str, state: MeetingState, error: Option<String>) {
    let (duration, processed) = deps
        .db
        .get_meeting(id)
        .ok()
        .flatten()
        .map(|m| (m.duration_seconds, m.processed_through_seconds))
        .unwrap_or((0.0, 0.0));
    let percent = if matches!(state, MeetingState::Complete | MeetingState::Partial) {
        100
    } else if duration > 0.0 {
        ((processed / duration).clamp(0.0, 1.0) * 100.0) as u8
    } else {
        0
    };
    (deps.sink)(&MeetingProgressEvent {
        id: id.to_string(),
        state: state.as_str().to_string(),
        percent,
        processed_through_seconds: processed,
        duration_seconds: duration,
        error,
    });
}

/// Run one meeting through transcription. Public so the pipeline is provable
/// without the worker thread (and so `--transcribe-meeting` can share it).
///
/// Returns the state the row was left in. `transcribing` means the job was
/// interrupted (quit, or a dictation that would not let go) and stays queued
/// for the next launch's reconcile.
pub fn run_meeting_job(deps: &PipelineDeps, id: &str) -> Result<MeetingState, String> {
    let db = &*deps.db;
    let Some(meeting) = db.get_meeting(id)? else {
        log::info!("meeting pipeline: {id} was deleted before it was transcribed");
        return Ok(MeetingState::Failed);
    };
    let prior = MeetingState::parse(&meeting.state).unwrap_or(MeetingState::Transcribing);
    if prior == MeetingState::Complete {
        return Ok(MeetingState::Complete);
    }
    let capture_was_partial = prior == MeetingState::Partial;
    let capture_note = meeting.error.clone();

    let mut tracks: Vec<(i64, PathBuf)> = Vec::new();
    for (track, path) in [
        (MIC_TRACK, meeting.mic_wav_path.as_ref()),
        (SYSTEM_TRACK, meeting.sys_wav_path.as_ref()),
    ] {
        if let Some(p) = path {
            let p = PathBuf::from(p);
            if p.exists() {
                tracks.push((track, p));
            }
        }
    }
    if tracks.is_empty() {
        db.set_meeting_state(id, MeetingState::Failed, Some(AUDIO_REMOVED_MESSAGE))?;
        emit_final(
            deps,
            id,
            MeetingState::Failed,
            Some(AUDIO_REMOVED_MESSAGE.into()),
        );
        return Ok(MeetingState::Failed);
    }

    let mut sources: Vec<(i64, WavWindows)> = Vec::new();
    for (track, path) in &tracks {
        match WavWindows::open(path) {
            Ok(w) => sources.push((*track, w)),
            Err(e) => log::warn!("meeting {id}: track {track} unreadable ({e}) — skipped"),
        }
    }
    if sources.is_empty() {
        let msg = "Yap could not read this meeting's audio files.";
        db.set_meeting_state(id, MeetingState::Failed, Some(msg))?;
        emit_final(deps, id, MeetingState::Failed, Some(msg.into()));
        return Ok(MeetingState::Failed);
    }
    let total_audio: f64 = sources.iter().map(|(_, w)| w.total_seconds()).sum();

    // Partial-ness is remembered in a local and restored at the end; the row
    // reads `transcribing` while the job runs so the UI can draw progress.
    db.set_meeting_state(id, MeetingState::Transcribing, capture_note.as_deref())?;

    let asr = match (deps.asr)() {
        Ok(asr) => asr,
        Err(e) => {
            // The audio is kept (retention never purges a non-complete row),
            // so Retry after a model download picks it straight back up.
            db.set_meeting_state(id, MeetingState::Failed, Some(&e))?;
            emit_final(deps, id, MeetingState::Failed, Some(e));
            return Ok(MeetingState::Failed);
        }
    };

    let mut done_before = 0.0f64;
    let mut decoded = 0usize;
    let mut failed = 0usize;
    for (track, audio) in &sources {
        let key = ledger_key(id, *track);
        let mut store = SegmentWritingStore {
            inner: JsonProgressStore::new(&deps.progress_dir),
            db,
            meeting_id: id,
            track: *track,
            done_before,
            total_audio,
            duration: meeting.duration_seconds,
            deps,
        };
        // Anything written past what the ledger promises is a crash between
        // the segment insert and the ledger write — it is about to be decoded
        // again, so it goes now rather than appearing twice.
        let resume_at = store.inner.load(&key)?.processed_through_seconds;
        db.delete_meeting_segments_from(id, *track, resume_at)?;

        let quit = || (deps.quit)() || quitting();
        let transcript = MeetingAsr {
            audio,
            vad: None,
            asr: &*asr,
            demand: &*deps.demand,
            store: &mut store,
            quit: &quit,
            config: deps.config.clone(),
        }
        .run(&key)?;
        if transcript.interrupted {
            log::info!(
                "meeting {id}: track {track} interrupted at {:.1}s — resumes next launch",
                transcript.processed_through_seconds
            );
            return Ok(MeetingState::Transcribing);
        }
        decoded += transcript.chunks_decoded + transcript.chunks_resumed;
        failed += transcript.chunks_failed;
        done_before += audio.total_seconds();
    }

    let segments = db.list_meeting_segments(id)?.len();
    let (state, error): (MeetingState, Option<String>) = if segments == 0 {
        (
            MeetingState::Failed,
            Some(empty_transcript_message(decoded, failed).into()),
        )
    } else if failed > 0 {
        (
            MeetingState::Partial,
            Some(
                capture_note
                    .clone()
                    .unwrap_or_else(|| SOME_WINDOWS_FAILED_MESSAGE.into()),
            ),
        )
    } else if capture_was_partial {
        (MeetingState::Partial, capture_note.clone())
    } else {
        (MeetingState::Complete, capture_note.clone())
    };
    db.set_meeting_progress(id, meeting.duration_seconds)?;
    db.set_meeting_state(id, state, error.as_deref())?;
    log::info!(
        "meeting {id}: transcribed {segments} segment(s) over {} track(s), {failed} failed window(s) → {}",
        sources.len(),
        state.as_str()
    );
    emit_final(deps, id, state, error);
    if matches!(state, MeetingState::Complete | MeetingState::Partial) {
        if let Some(after) = deps.after_transcript.as_ref() {
            after(id);
        }
    }
    Ok(state)
}
