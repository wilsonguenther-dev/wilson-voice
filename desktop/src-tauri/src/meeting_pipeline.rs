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
//!
//! yap24-NT2 (residual guards):
//!
//! - R2 crash-loop guard: a per-meeting attempt marker (`<id>.attempts.json`,
//!   beside the resume ledgers) is written when a job starts and removed when it
//!   ends by ANY in-process route (finished, errored, interrupted, panicked —
//!   the worker body runs under `catch_unwind`). A marker found at the next
//!   start therefore means the PROCESS died mid-job. After
//!   [`MAX_UNPROGRESSED_ATTEMPTS`] such deaths with no ledger progress between
//!   them the row is `failed` with [`CRASH_LOOP_MESSAGE`], never retried
//!   silently on every launch. The worker also waits
//!   [`PipelineDeps::start_delay`] (30 s in the app) before its first job, so a
//!   launch-time requeue never races the dictation engine's first warm-up.
//! - R3 `waiting_for_model`: when the pinned model's file is not on disk the row
//!   becomes [`MeetingState::WaitingForModel`], KEEPS its audio, and
//!   [`requeue_waiting_for_model`] puts it back on the queue when a model
//!   download completes (and reconcile does at the next launch).
//! - R4 `asr_model`: the catalog id of the engine that transcribed the row is
//!   recorded on it (migration 6). A resume never splices two models' output: a
//!   ledger left by a different model is discarded and the meeting re-decoded.

use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::asr_engine::TimedKind;
use crate::db::Database;
use crate::meeting::{MIC_TRACK as MIC_TRACK_INDEX, SYSTEM_TRACK as SYSTEM_TRACK_INDEX};
use crate::meeting_asr::{
    ChunkAsr, ChunkOutcome, ChunkStatus, ChunkTiming, EngineDemand, JsonProgressStore, MeetingAsr,
    MeetingAsrConfig, MeetingProgress, ProgressStore, SampleWindows, VoiceActivity, WavWindows,
};
use crate::meetings::{MeetingState, NewMeetingSegment};

/// yap24-NT2 R2 — how many launches may die mid-job WITHOUT the ledger moving
/// before the meeting is marked failed instead of retried again.
pub const MAX_UNPROGRESSED_ATTEMPTS: u32 = 2;

/// yap24-NT2 R2 — how long after launch the worker takes its first job: after
/// the dictation engine's first warm-up, never racing it.
pub const QUEUE_START_DELAY: Duration = Duration::from_secs(30);

/// yap24-NT2 R2 — the one honest sentence for a crash loop.
pub const CRASH_LOOP_MESSAGE: &str =
    "Yap quit unexpectedly twice while transcribing this meeting, so it stopped trying \
     automatically. Your audio is kept — press Retry to try again.";

/// yap24-NT2 R3 — what a `waiting_for_model` row says.
/// It names what the user does next and never claims a download is running:
/// nothing starts one on the user's behalf.
pub const WAITING_FOR_MODEL_MESSAGE: &str =
    "No speech model is downloaded yet. Download one in Settings and Yap transcribes this \
     meeting automatically. Your audio is kept.";

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

/// The chunk decoder for one job, and the catalog id of the model behind it
/// (recorded on the row as `asr_model`, R4).
pub struct MeetingEngine {
    pub model_id: String,
    pub asr: Box<dyn ChunkAsr>,
}

impl MeetingEngine {
    pub fn new(model_id: impl Into<String>, asr: Box<dyn ChunkAsr>) -> Self {
        MeetingEngine {
            model_id: model_id.into(),
            asr,
        }
    }
}

/// Why a job could not get a decoder.
#[derive(Debug, Clone, PartialEq)]
pub enum AsrUnavailable {
    /// R3 — the pinned model (`model_id`) is configured but its file is not on
    /// disk. The row waits (`waiting_for_model`) with its audio kept.
    ModelMissing { model_id: String },
    /// A refusal no download fixes (the English-only gate). The row fails with
    /// this sentence.
    Refused(String),
}

/// yap24-NT2 R3/R4 (fix round 1) — which model a meeting job decodes with.
///
/// The pinned model (`pinned_id`, the catalog's recommended one) when its file
/// is on disk. Otherwise the user's own selected model (`selected_id`) when it
/// is downloaded AND passes the meeting gate — exactly what the factory used
/// before the pin, so an install whose user picked another model keeps getting
/// its meetings transcribed. Only when neither is on disk is the answer
/// [`AsrUnavailable::ModelMissing`]. A spoken-language refusal is
/// [`AsrUnavailable::Refused`] whatever is downloaded.
///
/// `ready` answers "is this catalog id downloaded, and where" — the app passes
/// its `native_model_ready`, a test passes a stub.
pub fn resolve_meeting_model(
    pinned_id: &str,
    selected_id: &str,
    language: &str,
    ready: &dyn Fn(&str) -> Option<(String, PathBuf)>,
) -> Result<(String, PathBuf), AsrUnavailable> {
    crate::meeting_asr::meeting_availability(Some(pinned_id), Some(language))
        .map_err(|e| AsrUnavailable::Refused(e.message()))?;
    if let Some(found) = ready(pinned_id) {
        return Ok(found);
    }
    let selected = selected_id.trim();
    if !selected.is_empty()
        && selected != pinned_id
        && crate::meeting_asr::meeting_availability(Some(selected), Some(language)).is_ok()
    {
        if let Some(found) = ready(selected) {
            return Ok(found);
        }
    }
    Err(AsrUnavailable::ModelMissing {
        model_id: pinned_id.to_string(),
    })
}

/// Builds the chunk decoder for one job. A factory, not an instance, because
/// the pinned model is resolved when the job STARTS (it may have been
/// downloaded since launch), and because a test hands in a stub.
pub type AsrFactory = Arc<dyn Fn() -> Result<MeetingEngine, AsrUnavailable> + Send + Sync>;
pub type ProgressSink = Arc<dyn Fn(&MeetingProgressEvent) + Send + Sync>;
/// The silence map the chunker cuts on, asked for once per job. `Err` (the
/// Silero model is not downloaded yet, or its session failed to load) is never
/// fatal: the job logs one warn line and plans on the fixed clock, as it did
/// before the VAD was wired.
pub type VadFactory =
    Arc<dyn Fn() -> Result<Arc<dyn VoiceActivity + Send + Sync>, String> + Send + Sync>;
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
    /// The warm Silero session the app already holds (`AppState::vad`), handed
    /// to [`MeetingAsr`] so boundaries land in pauses, not on a 30 s clock.
    pub vad: VadFactory,
    pub demand: Arc<dyn EngineDemand + Send + Sync>,
    pub quit: Arc<dyn Fn() -> bool + Send + Sync>,
    pub sink: ProgressSink,
    pub after_transcript: Option<AfterTranscript>,
    pub config: MeetingAsrConfig,
    /// R2 — how long the worker waits before its first job
    /// ([`QUEUE_START_DELAY`] in the app, zero in tests).
    pub start_delay: Duration,
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
            diag_warn!("meeting_pipeline", "meeting pipeline: worker is gone, {id} stays queued for next launch", id = id);
        } else {
            log::info!("meeting pipeline: queued {id}");
            (self.sink)(&queued_event(id));
        }
    }

    /// Is a meeting queued or being transcribed? (yap24-NT4 R4)
    pub fn busy(&self) -> bool {
        let (lock, _) = &*self.pending;
        lock.lock().map(|p| p.in_flight > 0).unwrap_or(false)
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

/// yap24-NT4 R4 — is the installed pipeline transcribing (or holding a queued
/// meeting) right now? The notes worker reads it so a summary never runs
/// concurrently with a meeting's ASR decode. `false` with no pipeline.
pub fn transcribing_now() -> bool {
    INSTALLED.get().is_some_and(MeetingPipeline::busy)
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
            diag_warn!("meeting_pipeline", "meeting pipeline: not installed, {id} waits for the next launch", id = id);
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
            // R2: let the dictation engine warm up first. Stepped, so a quit
            // during the delay is not held up by it.
            let mut waited = Duration::ZERO;
            while waited < deps.start_delay && !quitting() && !(deps.quit)() {
                let step = Duration::from_millis(250).min(deps.start_delay - waited);
                std::thread::sleep(step);
                waited += step;
            }
            for id in rx {
                // R2: the whole job body is panic-contained — one panic fails
                // that row and the FIFO keeps going.
                crate::diagnostics::set_state("meeting.state", "processing");
                crate::diagnostics::meeting_stage("start");
                let job_started = Instant::now();
                let result = catch_unwind(AssertUnwindSafe(|| guarded_job(&deps, &id)));
                crate::diagnostics::record_duration(
                    "meeting",
                    "job",
                    job_started.elapsed().as_millis() as u64,
                );
                let failed = !matches!(&result, Ok(Ok(s)) if *s != MeetingState::Failed);
                crate::diagnostics::set_state("meeting.state", if failed { "failed" } else { "idle" });
                // Any in-process end of the job (including a caught panic)
                // clears the crash marker: only a dead PROCESS leaves it.
                clear_attempt_marker(&deps.progress_dir, &id);
                match result {
                    Ok(Ok(state)) => {
                        log::info!("meeting pipeline: {id} → {}", state.as_str());
                    }
                    Ok(Err(e)) => {
                        diag_warn!("meeting_pipeline", "meeting pipeline: {id} failed: {err}", id = id, err = e);
                        let msg = format!("Yap could not transcribe this meeting: {e}");
                        let _ = deps
                            .db
                            .set_meeting_state(&id, MeetingState::Failed, Some(&msg));
                        emit_final(&deps, &id, MeetingState::Failed, Some(msg));
                    }
                    Err(_) => {
                        diag_error!("meeting_pipeline", "meeting pipeline: job for {id} panicked — marked failed", id = id);
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
        diag_error!("meeting_pipeline", "meeting pipeline: worker thread failed to spawn: {err}", err = e);
    }
    MeetingPipeline {
        tx: Arc::new(Mutex::new(tx)),
        pending,
        sink,
    }
}

// ── yap-audit-meeting-timings: the lines that explain a slow meeting ────────
//
// A 31-minute meeting took 4:01 to transcribe (RTF 0.13) where a 24-minute one
// three days earlier took 0:28 (RTF 0.019), and the log could not say why. These
// two lines are what it says now: one DEBUG line per window (what it decoded,
// how long the decode took, what kind of cut it ended at) and one INFO line per
// track when the track is done. Pure formatters, so the shape is tested without
// scraping a real log.

/// The DEBUG line for one decoded window.
pub fn chunk_timing_line(meeting_id: &str, track: i64, timing: &ChunkTiming) -> String {
    format!(
        "meeting chunk track={track} index={} audio_s={:.1} decode_ms={} boundary={} status={} meeting={meeting_id}",
        timing.index,
        timing.audio_seconds,
        timing.decode_ms,
        timing.boundary.as_str(),
        timing.status.as_str(),
    )
}

/// What one track's run cost, end to end.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TrackTiming {
    pub track: i64,
    /// Seconds of the meeting's timeline this run covered (resumed windows are
    /// not in here — they were not decoded this time).
    pub audio_seconds: f64,
    /// Wall seconds from the start of the track's run to its end: planning,
    /// decoding, standing down for dictations, and writing segments.
    pub wall_seconds: f64,
    /// Seconds spent inside the decoder, summed over the run's windows. The gap
    /// between this and `wall_seconds` is time the meeting spent waiting.
    pub decode_seconds: f64,
    /// Windows decoded in this run, failed attempts included.
    pub chunks: usize,
    /// Windows already in the ledger, not decoded again.
    pub resumed: usize,
    /// Decodes a dictation cancelled mid-window.
    pub preempted: usize,
    /// Window boundaries that stood down for a dictation.
    pub yields: usize,
    pub interrupted: bool,
}

/// Wall time over audio time. `None` when there was no audio to divide by —
/// an RTF of a run that decoded nothing is not zero, it is not a number.
pub fn real_time_factor(wall_seconds: f64, audio_seconds: f64) -> Option<f64> {
    if audio_seconds > 0.0 && wall_seconds.is_finite() && wall_seconds >= 0.0 {
        Some(wall_seconds / audio_seconds)
    } else {
        None
    }
}

/// The INFO line for one finished (or interrupted) track:
/// `meeting rtf track=N audio_s=.. wall_s=.. rtf=.. chunks=.. preempted=..`,
/// then the fields that tell a slow decoder from a meeting that waited.
pub fn track_rtf_line(meeting_id: &str, timing: &TrackTiming) -> String {
    let rtf = real_time_factor(timing.wall_seconds, timing.audio_seconds)
        .map(|r| format!("{r:.3}"))
        .unwrap_or_else(|| "n/a".to_string());
    let mut line = format!(
        "meeting rtf track={} audio_s={:.1} wall_s={:.1} rtf={rtf} chunks={} preempted={} \
         decode_s={:.1} yields={} resumed={} meeting={meeting_id}",
        timing.track,
        timing.audio_seconds,
        timing.wall_seconds,
        timing.chunks,
        timing.preempted,
        timing.decode_seconds,
        timing.yields,
        timing.resumed,
    );
    if timing.interrupted {
        line.push_str(" interrupted=true");
    }
    line
}

/// The resume-ledger key for one track of one meeting.
pub fn ledger_key(meeting_id: &str, track: i64) -> String {
    format!("{meeting_id}.t{track}")
}

/// Every ledger file a meeting can leave next to its audio — deleted with the
/// WAVs (purge and delete), so nothing outlives the audio it describes. The
/// R2 attempt marker is one of them.
pub fn ledger_paths(dir: &Path, meeting_id: &str) -> Vec<PathBuf> {
    let store = JsonProgressStore::new(dir);
    let mut paths: Vec<PathBuf> = [MIC_TRACK, SYSTEM_TRACK]
        .iter()
        .map(|t| store.path_for(&ledger_key(meeting_id, *t)))
        .collect();
    paths.push(attempt_marker_path(dir, meeting_id));
    paths.push(capture_carry_path(dir, meeting_id));
    paths
}

/// #250 fix round 1 (independent B1) — whether this meeting's per-track resume
/// ledgers record a window that failed to decode (`asr_failed`). That is the
/// exact condition that ends a transcript `partial` with
/// [`SOME_WINDOWS_FAILED_MESSAGE`], and unlike the row's `error` (which every
/// shipped writer of `summarizing` cleared) the ledger stays beside the audio
/// until a Retry, a purge or a delete. An absent or unreadable ledger is
/// `false`: nothing is claimed that cannot be read.
pub fn ledger_records_failed_window(dir: &Path, meeting_id: &str) -> bool {
    let store = JsonProgressStore::new(dir);
    [MIC_TRACK, SYSTEM_TRACK]
        .iter()
        .filter_map(|t| store.load(&ledger_key(meeting_id, *t)).ok())
        .any(|p| {
            p.chunks
                .iter()
                .any(|c| c.status == ChunkStatus::AsrFailed)
        })
}

/// The per-track resume ledgers only — what a re-decode discards (never the
/// R2 attempt marker or the R3 capture carry, which describe the job and the
/// capture, not the decoded windows).
fn decode_ledger_paths(dir: &Path, meeting_id: &str) -> Vec<PathBuf> {
    let store = JsonProgressStore::new(dir);
    [MIC_TRACK, SYSTEM_TRACK]
        .iter()
        .map(|t| store.path_for(&ledger_key(meeting_id, *t)))
        .collect()
}

// ── R3: the capture carry ───────────────────────────────────────────────────

/// What the capture said about itself (partial or not, and its note), carried
/// across a `waiting_for_model` round trip. The row's own state and error are
/// overwritten by the wait, so without this a partial capture would come back
/// as `transcribing` with no note and could end `complete`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CaptureCarry {
    pub partial: bool,
    pub note: Option<String>,
}

/// `<progress_dir>/<id>.capture.json`, beside the ledgers (and deleted with
/// them by purge and delete).
pub fn capture_carry_path(dir: &Path, meeting_id: &str) -> PathBuf {
    dir.join(format!("{meeting_id}.capture.json"))
}

/// The carry a `waiting_for_model` pass left, if any.
pub fn read_capture_carry(dir: &Path, meeting_id: &str) -> Option<CaptureCarry> {
    let bytes = std::fs::read(capture_carry_path(dir, meeting_id)).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(c) => Some(c),
        Err(e) => {
            // Unreadable carry: assume the capture WAS partial — never let a
            // corrupt file promote a short capture to `complete`.
            diag_warn!("meeting_pipeline", "meeting {id}: unreadable capture carry ({err}) — treated as partial", id = meeting_id, err = e);
            Some(CaptureCarry {
                partial: true,
                note: None,
            })
        }
    }
}

/// Written BEFORE the row flips to `waiting_for_model`; an error stops the
/// flip, so the row never loses its partial-ness to a failed write.
fn write_capture_carry(dir: &Path, meeting_id: &str, carry: &CaptureCarry) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(carry).map_err(|e| e.to_string())?;
    std::fs::write(capture_carry_path(dir, meeting_id), bytes).map_err(|e| e.to_string())
}

// ── R2: the crash-loop guard ────────────────────────────────────────────────

/// What a job leaves on disk while it runs.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AttemptMarker {
    /// Launches that died mid-job with no ledger progress since the one before.
    pub unprogressed_crashes: u32,
    /// Ledger progress (seconds, summed over tracks) when this attempt began.
    pub progress_at_start: f64,
}

/// `<progress_dir>/<id>.attempts.json`, beside the per-track ledgers.
pub fn attempt_marker_path(dir: &Path, meeting_id: &str) -> PathBuf {
    dir.join(format!("{meeting_id}.attempts.json"))
}

/// The marker a previous attempt left, if the process died during it.
pub fn read_attempt_marker(dir: &Path, meeting_id: &str) -> Option<AttemptMarker> {
    let bytes = std::fs::read(attempt_marker_path(dir, meeting_id)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_attempt_marker(dir: &Path, meeting_id: &str, marker: AttemptMarker) {
    let path = attempt_marker_path(dir, meeting_id);
    let _ = std::fs::create_dir_all(dir);
    match serde_json::to_vec(&marker) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                diag_warn!("meeting_pipeline", "meeting pipeline: cannot write {file}: {err}", file = crate::diagnostics::basename(&path), err = e);
            }
        }
        Err(e) => diag_warn!("meeting_pipeline", "meeting pipeline: cannot encode attempt marker: {err}", err = e),
    }
}

fn clear_attempt_marker(dir: &Path, meeting_id: &str) {
    let _ = std::fs::remove_file(attempt_marker_path(dir, meeting_id));
}

/// Ledger progress summed over both tracks — what "progress" means for R2.
fn ledger_progress(dir: &Path, meeting_id: &str) -> f64 {
    let store = JsonProgressStore::new(dir);
    [MIC_TRACK, SYSTEM_TRACK]
        .iter()
        .filter_map(|t| store.load(&ledger_key(meeting_id, *t)).ok())
        .map(|p| p.processed_through_seconds)
        .sum()
}

/// The crash-loop decision, pure: given the marker the last attempt left (if
/// the process died during it) and the ledger progress now, how many
/// unprogressed crashes does this meeting have?
pub fn unprogressed_crashes(previous: Option<AttemptMarker>, progress_now: f64) -> u32 {
    match previous {
        None => 0,
        Some(m) if progress_now > m.progress_at_start + 1e-6 => 0,
        Some(m) => m.unprogressed_crashes.saturating_add(1),
    }
}

/// [`run_meeting_job`] behind the R2 guard. The worker runs THIS.
pub fn guarded_job(deps: &PipelineDeps, id: &str) -> Result<MeetingState, String> {
    let dir = &deps.progress_dir;
    let progress_now = ledger_progress(dir, id);
    let crashes = unprogressed_crashes(read_attempt_marker(dir, id), progress_now);
    if crashes >= MAX_UNPROGRESSED_ATTEMPTS {
        diag_error!(
            "meeting_pipeline",
            "meeting pipeline: {id} took Yap down {crashes} time(s) without progress — \
             marked failed, not retried",
            id = id,
            crashes = crashes,
        );
        clear_attempt_marker(dir, id);
        deps.db
            .set_meeting_state(id, MeetingState::Failed, Some(CRASH_LOOP_MESSAGE))?;
        emit_final(
            deps,
            id,
            MeetingState::Failed,
            Some(CRASH_LOOP_MESSAGE.into()),
        );
        return Ok(MeetingState::Failed);
    }
    if crashes > 0 {
        diag_warn!("meeting_pipeline", "meeting pipeline: {id} — the last launch died mid-job ({crashes}/{max})", id = id, crashes = crashes, max = MAX_UNPROGRESSED_ATTEMPTS);
    }
    write_attempt_marker(
        dir,
        id,
        AttemptMarker {
            unprogressed_crashes: crashes,
            progress_at_start: progress_now,
        },
    );
    let out = run_meeting_job(deps, id);
    clear_attempt_marker(dir, id);
    out
}

// ── R3: waiting_for_model ───────────────────────────────────────────────────

/// A model download completed: every `waiting_for_model` row goes back on the
/// queue (its audio was kept). Returns how many were requeued.
pub fn requeue_waiting_for_model(db: &Database, enqueue: &dyn Fn(&str)) -> usize {
    let rows = match db.list_meetings(100_000, None) {
        Ok(rows) => rows,
        Err(e) => {
            diag_warn!("meeting_pipeline", "meeting pipeline: cannot list waiting meetings ({err})", err = e);
            return 0;
        }
    };
    let mut n = 0;
    for m in rows {
        if MeetingState::parse(&m.state) == Some(MeetingState::WaitingForModel) {
            let _ = db.set_meeting_state(&m.id, MeetingState::Transcribing, None);
            enqueue(&m.id);
            n += 1;
        }
    }
    if n > 0 {
        log::info!("meeting pipeline: model ready — {n} waiting meeting(s) requeued");
    }
    n
}

/// yap24-NT1 (R3) — every sidecar a meeting keeps next to its audio in `dir`:
/// the per-track ASR resume ledgers (`<id>.*.asr-progress.json`, see
/// [`ledger_paths`]) and the kept per-track host-time anchors
/// (`<id>.t*.index.jsonl`, plus track 0's legacy `<id>.index.jsonl`). Found by
/// SCANNING rather than by enumerating track numbers, so a sidecar for a track
/// this build does not know about still goes with the audio it describes.
/// Keyed by the meeting's ROW id, which is a UUID: the `<id>.` prefix cannot
/// match another meeting's files.
pub fn meeting_sidecar_paths(dir: &Path, meeting_id: &str) -> Vec<PathBuf> {
    let prefix = format!("{meeting_id}.");
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let Some(rest) = name.strip_prefix(&prefix) else {
                        return false;
                    };
                    rest.ends_with(".asr-progress.json")
                        || rest == "asr-progress.json"
                        || rest == "index.jsonl"
                        || (rest.starts_with('t') && rest.ends_with(".index.jsonl"))
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    // The ledger paths the store would write are included even when the scan
    // could not read the directory; removing a file that is not there is a
    // no-op for every caller.
    out.extend(ledger_paths(dir, meeting_id));
    out.sort();
    out.dedup();
    out
}

/// yap24-NT1 — what one retention sweep released, per class.
#[derive(Debug, Default, Clone)]
pub struct MeetingAudioSweep {
    /// Complete meetings with a transcript, past [`crate::meetings::AUDIO_RETENTION_DAYS`].
    pub transcribed: Vec<crate::db::PurgedMeetingAudio>,
    /// Everything else with audio, past
    /// [`crate::meetings::FAILED_AUDIO_RETENTION_DAYS`] or over the ceiling.
    pub untranscribed: Vec<crate::db::PurgedMeetingAudio>,
    /// Every file actually unlinked: WAVs, ledgers, anchors.
    pub removed: Vec<PathBuf>,
}

impl MeetingAudioSweep {
    pub fn meetings(&self) -> usize {
        self.transcribed.len() + self.untranscribed.len()
    }
}

/// yap24-NT1 (R2, R3) — the ONE meeting-audio retention sweep the hygiene
/// thread runs: the 7-day class first, then the 30-day / ceiling class, each
/// class's files unlinked right after its own commit — each purged meeting's
/// WAVs together with its ledger and anchor sidecars, so nothing outlives the
/// audio it describes. Rows are kept; only audio goes. A failed untranscribed
/// purge is logged and retried next sweep; it never undoes the 7-day class.
pub fn sweep_meeting_audio(
    db: &Database,
    now: chrono::DateTime<chrono::Utc>,
    ceiling_bytes: u64,
) -> Result<MeetingAudioSweep, String> {
    // yap24-NT1 fix round 1 — each class's files leave the disk right after
    // ITS OWN commit. The two purges are separate transactions, so once the
    // first has committed its rows already say "audio expired"; a failure in
    // the second must never strand the first class's WAVs on disk forever.
    let mut removed = Vec::new();
    let transcribed = db.purge_transcribed_meeting_audio(
        now - chrono::Duration::days(crate::meetings::AUDIO_RETENTION_DAYS),
    )?;
    unlink_purged_meeting_audio(&transcribed, &mut removed);
    let untranscribed = match db.purge_untranscribed_meeting_audio(
        now - chrono::Duration::days(crate::meetings::FAILED_AUDIO_RETENTION_DAYS),
        ceiling_bytes,
    ) {
        Ok(purged) => purged,
        Err(e) => {
            // Rolled back: those rows still point at their audio, which stays
            // on disk and is retried on the next sweep.
            diag_warn!("meeting_pipeline", "retention: untranscribed meeting audio purge skipped: {err}", err = e);
            Vec::new()
        }
    };
    unlink_purged_meeting_audio(&untranscribed, &mut removed);
    Ok(MeetingAudioSweep {
        transcribed,
        untranscribed,
        removed,
    })
}

/// The disk half of a committed purge: every WAV plus the meeting's sidecars.
fn unlink_purged_meeting_audio(
    purged: &[crate::db::PurgedMeetingAudio],
    removed: &mut Vec<PathBuf>,
) {
    for purged in purged {
        let mut files = purged.wavs.clone();
        let mut dirs: Vec<PathBuf> = purged
            .wavs
            .iter()
            .filter_map(|w| w.parent().map(Path::to_path_buf))
            .collect();
        dirs.sort();
        dirs.dedup();
        for dir in dirs {
            files.extend(meeting_sidecar_paths(&dir, &purged.meeting_id));
        }
        files.sort();
        files.dedup();
        for file in files {
            match std::fs::remove_file(&file) {
                Ok(()) => removed.push(file),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => diag_warn!("meeting_pipeline", "retention: {file} not removed: {err}", file = crate::diagnostics::basename(&file), err = e),
            }
        }
    }
}

/// yap24-NT6 — what a transcript Retry says when the meeting's audio is gone.
pub const NO_AUDIO_RETRY_REFUSAL: &str =
    "This meeting's audio is gone, so it can't be transcribed again. Its transcript is kept.";

/// yap24-NT6 — a Retry re-decodes the WAV, so it needs `audio_kept = 1` AND a
/// WAV path that still names a file. `Some(reason)` when either is missing
/// (NT1 marks old rows failed with their audio already purged). Mirrors
/// `canRetry(state, audioKept, micWavPath)` in `desktop/src/meetings/progress.ts`.
pub fn retry_audio_refusal(
    audio_kept: bool,
    mic_wav_path: Option<&str>,
    is_file: impl Fn(&Path) -> bool,
) -> Option<&'static str> {
    match mic_wav_path {
        Some(p) if audio_kept && !p.trim().is_empty() && is_file(Path::new(p)) => None,
        _ => Some(NO_AUDIO_RETRY_REFUSAL),
    }
}

/// Forget every decoded window and segment of a meeting, so a Retry starts
/// clean rather than resuming past windows that failed.
///
/// yap24-NT6: refuses BEFORE deleting anything when the audio is gone —
/// deleting a partial meeting's segments with nothing to re-decode would
/// destroy the only transcript it has.
pub fn reset_for_retry(db: &Database, dir: &Path, meeting_id: &str) -> Result<(), String> {
    let meeting = db
        .get_meeting(meeting_id)?
        .ok_or_else(|| "that meeting is no longer here".to_string())?;
    if let Some(reason) = retry_audio_refusal(
        meeting.audio_kept,
        meeting.mic_wav_path.as_deref(),
        Path::is_file,
    ) {
        log::warn!(
            "meeting_pipeline retry_refused id={meeting_id} state={} audio_kept={} has_wav_path={}",
            meeting.state,
            meeting.audio_kept,
            meeting.mic_wav_path.is_some()
        );
        return Err(reason.to_string());
    }
    for path in ledger_paths(dir, meeting_id)
        .into_iter()
        .filter(|p| *p != capture_carry_path(dir, meeting_id))
    {
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
    /// Decoder milliseconds summed over this run's windows on this track.
    decode_ms: u64,
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

    fn record_timing(&mut self, _key: &str, timing: &ChunkTiming) {
        self.decode_ms = self.decode_ms.saturating_add(timing.decode_ms);
        log::debug!("{}", chunk_timing_line(self.meeting_id, self.track, timing));
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
/// yap-telemetry-diagnostics — one duration per pipeline stage
/// (`meeting.stage_<name>_ms`), and the stage becomes the health line's last stage.
fn stage_done(stage: &str, clock: &mut Instant) {
    crate::diagnostics::record_duration(
        "meeting",
        &format!("stage_{stage}"),
        clock.elapsed().as_millis() as u64,
    );
    crate::diagnostics::meeting_stage(stage);
    *clock = Instant::now();
}

pub fn run_meeting_job(deps: &PipelineDeps, id: &str) -> Result<MeetingState, String> {
    let mut stage_clock = Instant::now();
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
    // R3 (fix round 1): a row coming back from `waiting_for_model` (by the
    // download requeue, which clears the error, or by launch reconcile, which
    // leaves the waiting sentence in it) gets its capture's own partial-ness
    // and note back from the carry, never the waiting sentence.
    let (capture_was_partial, capture_note) = match read_capture_carry(&deps.progress_dir, id) {
        Some(carry) => (capture_was_partial || carry.partial, carry.note),
        None if capture_note.as_deref() == Some(WAITING_FOR_MODEL_MESSAGE) => {
            (capture_was_partial, None)
        }
        None => (capture_was_partial, capture_note),
    };

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
            Err(e) => diag_warn!("meeting_pipeline", "meeting {id}: track {track} unreadable ({err}) — skipped", id = id, track = track, err = e),
        }
    }
    if sources.is_empty() {
        let msg = "Yap could not read this meeting's audio files.";
        db.set_meeting_state(id, MeetingState::Failed, Some(msg))?;
        emit_final(deps, id, MeetingState::Failed, Some(msg.into()));
        return Ok(MeetingState::Failed);
    }
    let total_audio: f64 = sources.iter().map(|(_, w)| w.total_seconds()).sum();
    stage_done("open_audio", &mut stage_clock);

    // Partial-ness is remembered in a local and restored at the end; the row
    // reads `transcribing` while the job runs so the UI can draw progress.
    db.set_meeting_state(id, MeetingState::Transcribing, capture_note.as_deref())?;

    let engine = match (deps.asr)() {
        Ok(engine) => engine,
        Err(AsrUnavailable::ModelMissing { model_id }) => {
            // R3: not a failure. The audio is kept (retention only purges
            // `complete`), and a finished download requeues the row.
            log::info!("meeting {id}: waiting_for_model ({model_id} is not on disk)");
            write_capture_carry(
                &deps.progress_dir,
                id,
                &CaptureCarry {
                    partial: capture_was_partial,
                    note: capture_note.clone(),
                },
            )?;
            db.set_meeting_state(
                id,
                MeetingState::WaitingForModel,
                Some(WAITING_FOR_MODEL_MESSAGE),
            )?;
            emit_final(
                deps,
                id,
                MeetingState::WaitingForModel,
                Some(WAITING_FOR_MODEL_MESSAGE.into()),
            );
            return Ok(MeetingState::WaitingForModel);
        }
        Err(AsrUnavailable::Refused(e)) => {
            db.set_meeting_state(id, MeetingState::Failed, Some(&e))?;
            emit_final(deps, id, MeetingState::Failed, Some(e));
            return Ok(MeetingState::Failed);
        }
    };
    // R4: one model per transcript. A ledger another model left is discarded
    // rather than spliced onto this model's output.
    let asr_model = engine.model_id.clone();
    // A row with NO recorded model but with segments or ledgers on disk was
    // partly transcribed before migration 6 — by an unknown model, so it is
    // treated exactly like a different one (fix round 1: never fail open).
    // This build records the model before it decodes a single window, so a row
    // it touched always has one.
    let discard = match db.meeting_asr_model(id)? {
        Some(previous) if previous != asr_model => Some(previous),
        Some(_) => None,
        None => {
            let has_segments = !db.list_meeting_segments(id)?.is_empty();
            let has_ledger = decode_ledger_paths(&deps.progress_dir, id)
                .iter()
                .any(|p| p.exists());
            (has_segments || has_ledger).then(|| "an unrecorded model".to_string())
        }
    };
    if let Some(previous) = discard {
        diag_warn!(
            "meeting_pipeline",
            "meeting {id}: was partly transcribed by {previous}, now decoding with {model} — starting over",
            id = id,
            previous = previous,
            model = asr_model,
        );
        for path in decode_ledger_paths(&deps.progress_dir, id) {
            let _ = std::fs::remove_file(path);
        }
        for track in [MIC_TRACK, SYSTEM_TRACK] {
            db.delete_meeting_segments_from(id, track, 0.0)?;
        }
    }
    db.set_meeting_asr_model(id, &asr_model)?;
    let asr = engine.asr;
    stage_done("load_engine", &mut stage_clock);

    // One VAD lookup per job. A failure keeps the fixed-clock plan and says so
    // once; it never fails a meeting.
    let vad: Option<Arc<dyn VoiceActivity + Send + Sync>> = match (deps.vad)() {
        Ok(v) => Some(v),
        Err(e) => {
            diag_warn!("meeting_pipeline", "meeting {id}: no VAD ({err}) — chunking on the fixed clock", id = id, err = e);
            None
        }
    };
    stage_done("vad", &mut stage_clock);

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
            decode_ms: 0,
        };
        // Anything written past what the ledger promises is a crash between
        // the segment insert and the ledger write — it is about to be decoded
        // again, so it goes now rather than appearing twice.
        let resume_at = store.inner.load(&key)?.processed_through_seconds;
        db.delete_meeting_segments_from(id, *track, resume_at)?;

        let quit = || (deps.quit)() || quitting();
        let track_started = Instant::now();
        let transcript = MeetingAsr {
            audio,
            vad: vad.as_ref().map(|v| &**v as &dyn VoiceActivity),
            asr: &*asr,
            demand: &*deps.demand,
            store: &mut store,
            quit: &quit,
            config: deps.config.clone(),
        }
        .run(&key)?;
        log::info!(
            "{}",
            track_rtf_line(
                id,
                &TrackTiming {
                    track: *track,
                    audio_seconds: (transcript.processed_through_seconds - resume_at).max(0.0),
                    wall_seconds: track_started.elapsed().as_secs_f64(),
                    decode_seconds: store.decode_ms as f64 / 1000.0,
                    chunks: transcript.chunks_decoded,
                    resumed: transcript.chunks_resumed,
                    preempted: transcript.preempted_decodes,
                    yields: transcript.preempt_yields,
                    interrupted: transcript.interrupted,
                },
            )
        );
        if transcript.interrupted {
            log::info!(
                "meeting {id}: track {track} interrupted at {:.1}s — resumes next launch",
                transcript.processed_through_seconds
            );
            return Ok(MeetingState::Transcribing);
        }
        stage_done(&format!("transcribe_track{track}"), &mut stage_clock);
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
    // The capture's partial-ness now lives in the row's final state.
    let _ = std::fs::remove_file(capture_carry_path(&deps.progress_dir, id));
    log::info!(
        "meeting {id}: transcribed {segments} segment(s) over {} track(s), {failed} failed window(s) → {}",
        sources.len(),
        state.as_str()
    );
    stage_done("finalize", &mut stage_clock);
    emit_final(deps, id, state, error);
    if matches!(state, MeetingState::Complete | MeetingState::Partial) {
        if let Some(after) = deps.after_transcript.as_ref() {
            after(id);
        }
    }
    Ok(state)
}

#[cfg(test)]
mod nt6_retry_tests {
    use super::*;
    use crate::meetings::NewMeetingSegment;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "yap-nt6-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn retry_needs_audio_kept_and_a_wav_on_disk() {
        let yes = |_: &Path| true;
        let no = |_: &Path| false;
        assert_eq!(retry_audio_refusal(true, Some("/m/a.wav"), yes), None);
        assert_eq!(
            retry_audio_refusal(false, Some("/m/a.wav"), yes),
            Some(NO_AUDIO_RETRY_REFUSAL)
        );
        assert_eq!(
            retry_audio_refusal(true, None, yes),
            Some(NO_AUDIO_RETRY_REFUSAL)
        );
        assert_eq!(
            retry_audio_refusal(true, Some("  "), yes),
            Some(NO_AUDIO_RETRY_REFUSAL)
        );
        assert_eq!(
            retry_audio_refusal(true, Some("/m/a.wav"), no),
            Some(NO_AUDIO_RETRY_REFUSAL)
        );
    }

    #[test]
    fn retry_without_audio_refuses_and_keeps_the_transcript() {
        let db = Database::open_in_memory().expect("db");
        let dir = scratch("noaudio");
        let m = db.create_meeting("Standup", "mic").expect("meeting");
        db.finish_meeting(&m.id, 90.0, None).expect("finish");
        db.append_meeting_segments(&m.id, &[NewMeetingSegment::new(0.0, 30.0, "hello")])
            .expect("segment");
        db.set_meeting_state(&m.id, MeetingState::Partial, None)
            .expect("partial");

        let err = reset_for_retry(&db, &dir, &m.id).expect_err("no audio must refuse");
        assert_eq!(err, NO_AUDIO_RETRY_REFUSAL);
        let after = db.get_meeting(&m.id).expect("read").expect("row");
        assert_eq!(
            after.segment_count, 1,
            "the only transcript survives a refused Retry"
        );
        assert_eq!(after.state, "partial");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retry_with_audio_resets_to_transcribing() {
        let db = Database::open_in_memory().expect("db");
        let dir = scratch("audio");
        let wav = dir.join("mic.wav");
        std::fs::write(&wav, b"RIFF").expect("wav");
        let m = db.create_meeting("Standup", "mic").expect("meeting");
        db.finish_meeting(&m.id, 90.0, Some(&wav)).expect("finish");
        db.append_meeting_segments(&m.id, &[NewMeetingSegment::new(0.0, 30.0, "hello")])
            .expect("segment");
        db.set_meeting_state(&m.id, MeetingState::Failed, Some("boom"))
            .expect("failed");

        reset_for_retry(&db, &dir, &m.id).expect("audio on disk → retry runs");
        let after = db.get_meeting(&m.id).expect("read").expect("row");
        assert_eq!(after.state, "transcribing");
        assert_eq!(after.segment_count, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
