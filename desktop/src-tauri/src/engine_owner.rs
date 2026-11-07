//! The speech-engine owner (yap-1106-engine-owner).
//!
//! Wilson asked: "why does it load a second copy of the model? what is the
//! actual professional way?" The forensics of 2026-11-06 answered the first
//! half: after a decode timed out, the stuck engine was abandoned while it kept
//! running on the GPU and ANOTHER copy was loaded, so GPU memory filled until
//! window drawing froze (one morning's log: 8 GPU setups, 3 frees); and quitting
//! with a model still resident aborted inside ggml-metal's static destructors.
//!
//! The professional way is one owner, cancellable jobs and supervised restarts:
//!
//! 1. **One construction site.** [`load_engine`] in this file is the only code
//!    in the app that brings a speech model up. Everything else asks for one
//!    through it (`TranscriptionManager`'s loader) or sends a job to an
//!    [`EngineOwner`]. `only_the_owner_constructs_an_engine` below fails the
//!    build's tests if any other module calls the loader directly.
//! 2. **The model lives in a helper process** (`yap-asr`, protocol in
//!    `asr_protocol.rs`). The owner spawns it and is the only thing that does.
//!    A decode that runs past its deadline is cancelled; one that ignores the
//!    cancel gets its helper killed BY THE PID THE OWNER SPAWNED (never by
//!    name or pattern), macOS frees the GPU memory with the process, and the
//!    next job brings up exactly one fresh helper. The app process never holds
//!    Metal/ggml state, so `exit()` has no ggml destructors to run.
//! 3. **Every decode is a job with a deadline** ([`job_deadline`], from the
//!    clip's length) on a queue ordered dictation > retry > meeting/recovery.
//!    A new take waits behind the running job or fails fast with a plain
//!    sentence. Nothing ever starts a parallel engine.
//! 4. **A health state** ([`EngineHealth`]): idle | loading | ready | busy |
//!    stuck | restarting, logged on every transition and published to a
//!    listener the app forwards to the pill and Diagnostics.
//! 5. **Quit** stops accepting jobs, cancels the running one, waits for the
//!    engine to be freed up to a hard cap, kills any helper it spawned by PID,
//!    and — if an engine is somehow still live in-process — ends with `_exit`
//!    after the DB checkpoint so no C++ static destructor runs.
//!    Log: `exit: engines freed=N waited_ms=… skipped_destructors=bool`.
//!
//! The next item (yap-1106-engine-watchdog-restart) adds the hung-decode
//! watchdog and the pill's display; the seams it needs are
//! [`EngineOwner::restart`] and the `stuck` transition.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use serde::Serialize;

use crate::asr_engine;
use crate::asr_protocol::{samples_to_le_bytes, AsrReady, AsrRequest, AsrResponse, WireCaps};
use crate::transcription::{CancelHandle, Transcriber};

// ---------------------------------------------------------------------------
// Health state
// ---------------------------------------------------------------------------

/// The engine's one health state, in plain words for the pill and Diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineHealth {
    Idle,
    Loading,
    Ready,
    Busy,
    Stuck,
    Restarting,
}

impl EngineHealth {
    pub const ALL: [EngineHealth; 6] = [
        EngineHealth::Idle,
        EngineHealth::Loading,
        EngineHealth::Ready,
        EngineHealth::Busy,
        EngineHealth::Stuck,
        EngineHealth::Restarting,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            EngineHealth::Idle => "idle",
            EngineHealth::Loading => "loading",
            EngineHealth::Ready => "ready",
            EngineHealth::Busy => "busy",
            EngineHealth::Stuck => "stuck",
            EngineHealth::Restarting => "restarting",
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            EngineHealth::Idle => 0,
            EngineHealth::Loading => 1,
            EngineHealth::Ready => 2,
            EngineHealth::Busy => 3,
            EngineHealth::Stuck => 4,
            EngineHealth::Restarting => 5,
        }
    }

    fn from_u8(v: u8) -> EngineHealth {
        match v {
            1 => EngineHealth::Loading,
            2 => EngineHealth::Ready,
            3 => EngineHealth::Busy,
            4 => EngineHealth::Stuck,
            5 => EngineHealth::Restarting,
            _ => EngineHealth::Idle,
        }
    }
}

/// Tauri event the app emits with the new state (payload: the lowercase name).
pub const ENGINE_HEALTH_EVENT: &str = "engine_health";

type HealthListener = Box<dyn Fn(EngineHealth) + Send + Sync>;

static APP_HEALTH: AtomicU8 = AtomicU8::new(0);
static HEALTH_LISTENER: OnceLock<HealthListener> = OnceLock::new();

/// The app-wide engine health (what the helper engines report).
pub fn health() -> EngineHealth {
    EngineHealth::from_u8(APP_HEALTH.load(Ordering::Acquire))
}

/// Install the one listener (the app forwards it as [`ENGINE_HEALTH_EVENT`]).
/// A second call is ignored.
pub fn set_health_listener(listener: HealthListener) {
    let _ = HEALTH_LISTENER.set(listener);
}

/// The telemetry line every transition writes.
pub fn transition_line(
    from: EngineHealth,
    to: EngineHealth,
    reason: &str,
    engines_live: u64,
    job: &str,
) -> String {
    format!(
        "engine state from={} to={} reason={reason} engines_live={engines_live} job={job}",
        from.as_str(),
        to.as_str()
    )
}

/// Move the APP-WIDE health to `to`, log it, and tell the listener.
fn publish(to: EngineHealth, reason: &str, job: &str) {
    let from = EngineHealth::from_u8(APP_HEALTH.swap(to.to_u8(), Ordering::AcqRel));
    log::info!("{}", transition_line(from, to, reason, LIVE.live(), job));
    if from != to {
        if let Some(listener) = HEALTH_LISTENER.get() {
            listener(to);
        }
    }
}

// ---------------------------------------------------------------------------
// Live-engine accounting: at most two, ever (one stuck + its replacement)
// ---------------------------------------------------------------------------

/// The hard ceiling on resident engines. One is the target; the second exists
/// only while a stuck one is being killed. A third load is refused.
pub const MAX_LIVE_ENGINES: u64 = 2;

/// What a take sees when the ceiling refuses a load: it is queued behind the
/// earlier take, its audio is kept, and the pill says "Catching up…".
pub const CATCHING_UP: &str =
    "Catching up on an earlier take. Your audio is saved and will be transcribed shortly.";

/// Counts engines that hold a model right now, and every load and free.
pub struct LiveCounter {
    live: AtomicU64,
    loads: AtomicU64,
    freed: AtomicU64,
    peak: AtomicU64,
}

impl LiveCounter {
    pub const fn new() -> LiveCounter {
        LiveCounter {
            live: AtomicU64::new(0),
            loads: AtomicU64::new(0),
            freed: AtomicU64::new(0),
            peak: AtomicU64::new(0),
        }
    }

    pub fn live(&self) -> u64 {
        self.live.load(Ordering::Acquire)
    }
    pub fn loads(&self) -> u64 {
        self.loads.load(Ordering::Acquire)
    }
    pub fn freed(&self) -> u64 {
        self.freed.load(Ordering::Acquire)
    }
    pub fn peak(&self) -> u64 {
        self.peak.load(Ordering::Acquire)
    }

    /// Admit one more live engine, or refuse with [`CATCHING_UP`] at `max`.
    pub fn acquire(&'static self, max: u64) -> Result<LiveToken, String> {
        let mut current = self.live.load(Ordering::Acquire);
        loop {
            if current >= max {
                log::warn!("engine load refused engines_live={current} max={max}");
                return Err(CATCHING_UP.to_string());
            }
            match self.live.compare_exchange(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(seen) => current = seen,
            }
        }
        self.loads.fetch_add(1, Ordering::AcqRel);
        self.peak.fetch_max(current + 1, Ordering::AcqRel);
        Ok(LiveToken { counter: self })
    }
}

impl Default for LiveCounter {
    fn default() -> Self {
        LiveCounter::new()
    }
}

/// Held by an engine while it holds a model; dropping it counts the free.
pub struct LiveToken {
    counter: &'static LiveCounter,
}

impl Drop for LiveToken {
    fn drop(&mut self) {
        self.counter.live.fetch_sub(1, Ordering::AcqRel);
        self.counter.freed.fetch_add(1, Ordering::AcqRel);
    }
}

/// The app's counter.
static LIVE: LiveCounter = LiveCounter::new();

/// Engines holding a model right now (logged on every load and free).
pub fn engines_live() -> u64 {
    LIVE.live()
}

/// Engines freed in this process so far.
pub fn engines_freed() -> u64 {
    LIVE.freed()
}

// ---------------------------------------------------------------------------
// Deadlines
// ---------------------------------------------------------------------------

/// Fixed part of every decode deadline: model warm-up and pipe overhead.
pub const DEADLINE_BASE: Duration = Duration::from_secs(15);
/// Per second of audio. A deliberately generous worst case until FX-04 tunes
/// it from measured speed.
pub const DEADLINE_PER_AUDIO_SECOND: Duration = Duration::from_secs(3);
/// No single decode may run longer than this, however long the clip.
pub const DEADLINE_CEILING: Duration = Duration::from_secs(20 * 60);

/// The deadline for one decode, from the clip's length.
pub fn job_deadline(samples: usize, sample_rate: u32) -> Duration {
    let rate = sample_rate.max(1) as f64;
    let audio_seconds = samples as f64 / rate;
    let budget = DEADLINE_BASE + DEADLINE_PER_AUDIO_SECOND.mul_f64(audio_seconds);
    budget.min(DEADLINE_CEILING)
}

/// What a job past its deadline returns.
pub const JOB_DEADLINE_EXCEEDED: &str =
    "The speech engine took too long on this clip, so Yap stopped it. Your audio is saved.";
/// What a job queued during quit returns.
pub const OWNER_QUITTING: &str = "Yap is quitting, so this clip was not transcribed.";
/// What a take sees when the queue is full.
pub const QUEUE_FULL: &str =
    "Too many clips are waiting for the speech engine. Your audio is saved; try again in a minute.";

// ---------------------------------------------------------------------------
// The one construction site
// ---------------------------------------------------------------------------

const ASR_HELPER_BIN: &str = "yap-asr";

/// Bring up ONE speech engine for `model_path`. The only function in the app
/// that constructs an engine; `TranscriptionManager::new` uses it as its
/// loader, so dictation, retry, meeting ASR, live transcript and recovery all
/// get their engine from here.
///
/// The engine is the `yap-asr` helper when it is staged next to the app (the
/// bundle, and every `npm run sidecar` dev build). When it is not (a bare
/// `cargo test` binary), it falls back to an in-process engine and says so,
/// so nothing regresses; the quit path then ends with `_exit`.
pub fn load_engine(model_path: &Path) -> Result<Box<dyn Transcriber>, String> {
    publish(EngineHealth::Loading, "load_requested", "none");
    let result = match helper_binary() {
        Some(bin) => {
            HelperEngine::spawn(&bin, model_path).map(|e| Box::new(e) as Box<dyn Transcriber>)
        }
        None => load_in_process(model_path),
    };
    match &result {
        Ok(_) => publish(EngineHealth::Ready, "loaded", "none"),
        Err(e) => {
            log::warn!("engine load failed error_len={}", e.len());
            publish(EngineHealth::Idle, "load_failed", "none");
        }
    }
    result
}

/// Said when there is no helper and the in-process engine is not compiled in
/// (every shipped build): a plain sentence, never a crash.
pub const HELPER_MISSING: &str =
    "Yap's speech helper is missing from this install, so it cannot transcribe. Reinstall Yap to fix it.";

/// The dev/test fallback: an in-process engine, counted like any other. Only
/// compiled with `--features in-process-asr`; the shipped app has no way to
/// put ggml/Metal state into its own process.
#[cfg(feature = "in-process-asr")]
fn load_in_process(model_path: &Path) -> Result<Box<dyn Transcriber>, String> {
    log::warn!("engine helper missing — loading in process (in-process-asr dev build only)");
    LIVE.acquire(MAX_LIVE_ENGINES).and_then(|token| {
        asr_engine::load(model_path).map(|inner| {
            Box::new(LocalEngine {
                inner,
                _live: token,
            }) as Box<dyn Transcriber>
        })
    })
}

#[cfg(not(feature = "in-process-asr"))]
fn load_in_process(_model_path: &Path) -> Result<Box<dyn Transcriber>, String> {
    log::warn!("engine helper missing and in-process-asr not built — no engine");
    Err(HELPER_MISSING.to_string())
}

/// `wilson-voice --asr-capabilities` only: a short-lived CLI process that
/// asks the model what it can do and exits. Goes through the helper like the
/// app does (the helper reports its capabilities on load); with
/// `in-process-asr` and no helper staged it reads them in process instead.
pub fn probe_capabilities(model_path: &Path) -> Result<asr_engine::ModelCapabilities, String> {
    if let Some(bin) = helper_binary() {
        log::info!("engine probe via=helper");
        // `engine` drops at the end of this block: the helper is asked to
        // exit, then killed by its PID past the cap.
        let engine = HelperEngine::spawn(&bin, model_path)?;
        return engine
            .caps
            .clone()
            .ok_or_else(|| "the speech helper did not report its capabilities".to_string());
    }
    log::info!("engine probe via=in_process");
    probe_in_process(model_path)
}

#[cfg(feature = "in-process-asr")]
fn probe_in_process(model_path: &Path) -> Result<asr_engine::ModelCapabilities, String> {
    let engine = asr_engine::load(model_path)?;
    Ok(asr_engine::capabilities(&engine))
}

#[cfg(not(feature = "in-process-asr"))]
fn probe_in_process(_model_path: &Path) -> Result<asr_engine::ModelCapabilities, String> {
    Err(HELPER_MISSING.to_string())
}

/// The staged helper: next to the executable (where Tauri puts an
/// `externalBin`, and where a workspace build puts it in dev), or one level up
/// for a test binary in `target/<profile>/deps/` (the workspace build puts
/// `yap-asr` in `target/<profile>/`).
fn helper_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let here = dir.join(ASR_HELPER_BIN);
    if here.is_file() {
        return Some(here);
    }
    if dir.file_name().map(|n| n == "deps").unwrap_or(false) {
        let up = dir.parent()?.join(ASR_HELPER_BIN);
        if up.is_file() {
            return Some(up);
        }
    }
    None
}

/// An in-process engine (fallback only), counted like any other.
#[cfg(feature = "in-process-asr")]
struct LocalEngine {
    inner: asr_engine::AsrEngine,
    _live: LiveToken,
}

#[cfg(feature = "in-process-asr")]
impl Transcriber for LocalEngine {
    fn transcribe(
        &mut self,
        samples_16k_mono: &[f32],
        language: Option<&str>,
        bias_prompt: Option<&str>,
    ) -> Result<String, String> {
        self.inner
            .transcribe(samples_16k_mono, language, bias_prompt)
    }
    fn transcribe_timed(
        &mut self,
        samples_16k_mono: &[f32],
        language: Option<&str>,
        bias_prompt: Option<&str>,
    ) -> Result<asr_engine::TimedTranscript, String> {
        self.inner
            .transcribe_timed(samples_16k_mono, language, bias_prompt)
    }
    fn cancel_handle(&self) -> Option<CancelHandle> {
        self.inner.cancel_handle()
    }
    fn reset_cancel(&mut self) {
        self.inner.reset_cancel();
    }
    fn language_support(&self) -> Option<crate::languages::LanguageSupport> {
        self.inner.language_support()
    }
    fn last_detected_language(&self) -> Option<String> {
        self.inner.last_detected_language()
    }
}

#[cfg(feature = "in-process-asr")]
impl Drop for LocalEngine {
    fn drop(&mut self) {
        log::info!(
            "engine free kind=in_process engines_live={}",
            LIVE.live().saturating_sub(1)
        );
    }
}

// ---------------------------------------------------------------------------
// The helper process
// ---------------------------------------------------------------------------

/// How long a fresh helper has to print its ready line.
pub const HELPER_READY_BUDGET: Duration = Duration::from_secs(10);
/// How long a model load inside the helper may take.
pub const HELPER_LOAD_BUDGET: Duration = Duration::from_secs(120);
/// After a cancel, how long the helper has to answer before it is killed.
pub const HELPER_CANCEL_GRACE: Duration = Duration::from_secs(3);
/// QUIT's hard cap: how long a helper has to free its model and exit before
/// it is killed by PID.
pub const HELPER_EXIT_CAP: Duration = Duration::from_millis(1_500);
const HELPER_EXIT_POLL: Duration = Duration::from_millis(20);

/// PIDs of helpers THIS process spawned and has not yet reaped. The only PIDs
/// the owner ever signals.
static SPAWNED_HELPERS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

fn helpers() -> std::sync::MutexGuard<'static, Vec<u32>> {
    SPAWNED_HELPERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn register_pid(pid: u32) {
    helpers().push(pid);
}

fn unregister_pid(pid: u32) {
    helpers().retain(|p| *p != pid);
}

/// Helper PIDs spawned by this process and not yet reaped.
pub fn spawned_helper_pids() -> Vec<u32> {
    helpers().clone()
}

/// Quit's last resort: SIGKILL every helper this process spawned and has not
/// reaped — by its recorded PID, never by name or pattern. Returns how many.
pub fn kill_spawned_helpers() -> usize {
    // Signalled under the registry lock: a PID is reaped only after it leaves
    // the registry (see `kill_process` / `HelperEngine::shutdown`), so every
    // PID signalled here is still our unreaped child and cannot be reused.
    let registry = helpers();
    let pids = registry.clone();
    for pid in &pids {
        // SAFETY: plain syscall on a PID we spawned and still own (not reaped,
        // so it cannot have been reused).
        unsafe {
            libc::kill(*pid as libc::pid_t, libc::SIGKILL);
        }
        log::warn!("engine helper killed pid={pid} reason=quit_cap");
    }
    drop(registry);
    pids.len()
}

struct HelperProcess {
    child: Child,
    pid: u32,
    stdin: Arc<Mutex<ChildStdin>>,
    responses: mpsc::Receiver<AsrResponse>,
    _live: LiveToken,
}

/// The app's view of a `yap-asr` helper: a `Transcriber` whose every call is a
/// job with a deadline over the pipe.
struct HelperEngine {
    bin: PathBuf,
    model_path: PathBuf,
    proc_: Option<HelperProcess>,
    next_id: u64,
    caps: Option<asr_engine::ModelCapabilities>,
    last_language: Option<String>,
    /// The id of the decode in flight on the helper, 0 when none. The cancel
    /// hook reads it to escalate a cancel the helper ignores into a kill by
    /// PID — and only for the job it was asked to stop.
    running: Arc<AtomicU64>,
}

/// Escalation of an ignored cancel: past [`HELPER_CANCEL_GRACE`], if job `job`
/// is still the one running on helper `pid` and that PID is still our unreaped
/// child, SIGKILL it. The decode then returns at once (its pipe closes), the
/// engine is dropped, and macOS frees the helper's GPU memory.
fn kill_if_still_running(pid: u32, job: u64, running: Arc<AtomicU64>) {
    let spawned = std::thread::Builder::new()
        .name("yap-asr-cancel-escalation".into())
        .spawn(move || {
            std::thread::sleep(HELPER_CANCEL_GRACE);
            if running.load(Ordering::Acquire) != job {
                return;
            }
            let registry = helpers();
            if !registry.contains(&pid) {
                return;
            }
            // SAFETY: plain syscall on a PID we spawned and have not reaped
            // (it is still in the registry, checked under its lock).
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
            drop(registry);
            publish(EngineHealth::Stuck, "cancel_ignored", "decode");
            log::warn!("engine helper killed pid={pid} reason=cancel_ignored job={job}");
        });
    if let Err(e) = spawned {
        log::warn!("engine cancel escalation not started: {e}");
    }
}

fn timed_kind(s: &str) -> asr_engine::TimedKind {
    match s {
        "segment" => asr_engine::TimedKind::Segment,
        "word" => asr_engine::TimedKind::Word,
        "token" => asr_engine::TimedKind::Token,
        _ => asr_engine::TimedKind::None,
    }
}

fn caps_from_wire(w: &WireCaps) -> asr_engine::ModelCapabilities {
    asr_engine::ModelCapabilities {
        arch: w.arch.clone(),
        backend: w.backend.clone(),
        native_sample_rate: w.native_sample_rate,
        languages: w.languages.clone(),
        max_timestamp_kind: timed_kind(&w.max_timestamp_kind),
        max_audio_ms: w.max_audio_ms,
        supports_language_detect: w.supports_language_detect,
        supports_streaming: w.supports_streaming,
    }
}

fn send_line(stdin: &Mutex<ChildStdin>, req: &AsrRequest) -> Result<(), String> {
    let line = serde_json::to_string(req).map_err(|e| format!("encode request: {e}"))?;
    let mut pipe = stdin.lock();
    writeln!(pipe, "{line}").map_err(|_| "the speech helper stopped listening".to_string())?;
    pipe.flush()
        .map_err(|_| "the speech helper stopped listening".to_string())
}

impl HelperEngine {
    fn spawn(bin: &Path, model_path: &Path) -> Result<HelperEngine, String> {
        let mut engine = HelperEngine {
            bin: bin.to_path_buf(),
            model_path: model_path.to_path_buf(),
            proc_: None,
            next_id: 1,
            caps: None,
            last_language: None,
            running: Arc::new(AtomicU64::new(0)),
        };
        engine.start()?;
        Ok(engine)
    }

    /// Spawn the helper and load the model in it. Counted against
    /// [`MAX_LIVE_ENGINES`] BEFORE the process exists.
    fn start(&mut self) -> Result<(), String> {
        let token = LIVE.acquire(MAX_LIVE_ENGINES)?;
        let started = Instant::now();
        let mut child = Command::new(&self.bin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not start the speech helper: {e}"))?;
        let pid = child.id();
        register_pid(pid);
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            unregister_pid(pid);
            return Err("the speech helper has no pipes".to_string());
        };
        if let Some(stderr) = child.stderr.take() {
            let _ = std::thread::Builder::new()
                .name("yap-asr-stderr".into())
                .spawn(move || {
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        log::debug!("asr-helper {line}");
                    }
                });
        }
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
        let (resp_tx, resp_rx) = mpsc::channel::<AsrResponse>();
        std::thread::Builder::new()
            .name("yap-asr-reader".into())
            .spawn(move || {
                let mut lines = BufReader::new(stdout).lines();
                match lines.next() {
                    Some(Ok(first)) => match serde_json::from_str::<AsrReady>(&first) {
                        Ok(r) if r.is_ready() => {
                            let _ = ready_tx.send(Ok(r.pid));
                        }
                        _ => {
                            let _ = ready_tx.send(Err("bad ready line".into()));
                            return;
                        }
                    },
                    _ => {
                        let _ = ready_tx.send(Err("helper closed before ready".into()));
                        return;
                    }
                }
                for line in lines.map_while(Result::ok) {
                    if let Ok(resp) = serde_json::from_str::<AsrResponse>(&line) {
                        if resp_tx.send(resp).is_err() {
                            return;
                        }
                    }
                }
            })
            .map_err(|e| format!("spawn helper reader: {e}"))?;
        let mut process = HelperProcess {
            child,
            pid,
            stdin: Arc::new(Mutex::new(stdin)),
            responses: resp_rx,
            _live: token,
        };
        match ready_rx.recv_timeout(HELPER_READY_BUDGET) {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                kill_process(&mut process, "not_ready");
                return Err(format!("the speech helper did not start: {e}"));
            }
            Err(_) => {
                kill_process(&mut process, "ready_timeout");
                return Err("the speech helper did not start in time".to_string());
            }
        }
        let id = self.take_id();
        let model = self.model_path.to_string_lossy().to_string();
        if let Err(e) = send_line(&process.stdin, &AsrRequest::load(id, &model)) {
            kill_process(&mut process, "load_send_failed");
            return Err(e);
        }
        let resp = match wait_for(&process.responses, id, HELPER_LOAD_BUDGET) {
            Ok(r) => r,
            Err(e) => {
                kill_process(&mut process, "load_timeout");
                return Err(e);
            }
        };
        if !resp.ok {
            kill_process(&mut process, "load_failed");
            return Err(resp
                .error
                .unwrap_or_else(|| "the speech model did not load".to_string()));
        }
        self.caps = resp.caps.as_ref().map(caps_from_wire);
        log::info!(
            "engine helper up pid={pid} load_ms={} engines_live={} loads={}",
            started.elapsed().as_millis(),
            LIVE.live(),
            LIVE.loads()
        );
        self.proc_ = Some(process);
        Ok(())
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// One decode job. Past its deadline it is cancelled; past the cancel
    /// grace the helper is killed by PID (freeing its GPU memory) and the
    /// error is returned. The NEXT call brings up one fresh helper.
    fn run_job(
        &mut self,
        samples: &[f32],
        language: Option<&str>,
        bias_prompt: Option<&str>,
        timed: bool,
    ) -> Result<AsrResponse, String> {
        if self.proc_.is_none() {
            publish(EngineHealth::Restarting, "helper_gone", "decode");
            let restarted = self.start();
            publish(
                if restarted.is_ok() {
                    EngineHealth::Ready
                } else {
                    EngineHealth::Idle
                },
                if restarted.is_ok() {
                    "restarted"
                } else {
                    "restart_failed"
                },
                "decode",
            );
            restarted?;
        }
        let id = self.take_id();
        let deadline = job_deadline(samples.len(), 16_000);
        let running = self.running.clone();
        let Some(process) = self.proc_.as_mut() else {
            return Err(crate::transcription::NO_ENGINE_LOADED.to_string());
        };
        let path = std::env::temp_dir().join(format!("yap-asr-{}-{id}.f32", std::process::id()));
        write_private(&path, &samples_to_le_bytes(samples))?;
        let started = Instant::now();
        publish(EngineHealth::Busy, "job_started", "decode");
        running.store(id, Ordering::Release);
        let sent = send_line(
            &process.stdin,
            &AsrRequest::transcribe(id, &path.to_string_lossy(), language, bias_prompt, timed),
        );
        // Err((message, kill_helper)).
        let outcome: Result<AsrResponse, (String, bool)> = match sent {
            Err(e) => Err((e, true)),
            Ok(()) => match wait_for(&process.responses, id, deadline) {
                Ok(resp) => Ok(resp),
                Err(_) => {
                    log::warn!(
                        "engine job deadline_passed ms={} deadline_ms={} samples={}",
                        started.elapsed().as_millis(),
                        deadline.as_millis(),
                        samples.len()
                    );
                    let _ = send_line(&process.stdin, &AsrRequest::cancel(0));
                    match wait_for(&process.responses, id, HELPER_CANCEL_GRACE) {
                        // The helper honoured the cancel: it stays warm.
                        Ok(_) => Err((JOB_DEADLINE_EXCEEDED.to_string(), false)),
                        Err(_) => {
                            publish(EngineHealth::Stuck, "cancel_ignored", "decode");
                            Err((JOB_DEADLINE_EXCEEDED.to_string(), true))
                        }
                    }
                }
            },
        };
        running.store(0, Ordering::Release);
        let _ = std::fs::remove_file(&path);
        log::info!(
            "engine job done samples={} ms={} ok={}",
            samples.len(),
            started.elapsed().as_millis(),
            outcome.as_ref().map(|r| r.ok).unwrap_or(false)
        );
        match outcome {
            Ok(resp) if resp.ok => {
                publish(EngineHealth::Ready, "job_done", "decode");
                self.last_language = resp.language.clone();
                Ok(resp)
            }
            Ok(resp) => {
                publish(EngineHealth::Ready, "job_failed", "decode");
                Err(resp
                    .error
                    .unwrap_or_else(|| "transcription failed".to_string()))
            }
            Err((e, false)) => {
                publish(EngineHealth::Ready, "job_cancelled", "decode");
                Err(e)
            }
            Err((e, true)) => {
                // A stuck or vanished helper: kill it by PID so macOS frees
                // its GPU memory. Never abandoned while it still runs.
                if let Some(mut process) = self.proc_.take() {
                    kill_process(&mut process, "stuck");
                }
                publish(EngineHealth::Idle, "helper_killed", "decode");
                Err(e)
            }
        }
    }

    /// Ask the helper to free its model and exit; past [`HELPER_EXIT_CAP`],
    /// kill it by PID. Returns (freed_cleanly, waited_ms).
    fn shutdown(&mut self) -> (bool, u128) {
        let Some(mut process) = self.proc_.take() else {
            return (false, 0);
        };
        let started = Instant::now();
        let id = self.take_id();
        let _ = send_line(&process.stdin, &AsrRequest::shutdown(id));
        let deadline = started + HELPER_EXIT_CAP;
        let mut clean = false;
        while Instant::now() < deadline {
            // Reaped under the registry lock and unregistered in the same
            // breath, so a registered PID is always an unreaped child.
            let mut registry = helpers();
            match process.child.try_wait() {
                Ok(Some(_)) => {
                    registry.retain(|p| *p != process.pid);
                    clean = true;
                    break;
                }
                Ok(None) => {
                    drop(registry);
                    std::thread::sleep(HELPER_EXIT_POLL);
                }
                Err(_) => break,
            }
        }
        if !clean {
            kill_process(&mut process, "exit_cap");
        }
        let waited = started.elapsed().as_millis();
        log::info!(
            "engine helper exit pid={} waited_ms={waited} killed={}",
            process.pid,
            !clean
        );
        (clean, waited)
    }
}

/// Kill exactly the helper we spawned, by its PID, and reap it.
fn kill_process(process: &mut HelperProcess, reason: &str) {
    // Out of the registry BEFORE the reap, so nothing that signals registered
    // PIDs can ever hit a reaped (reusable) one.
    unregister_pid(process.pid);
    let _ = process.child.kill();
    let _ = process.child.wait();
    log::warn!(
        "engine helper killed pid={} reason={reason} engines_live={}",
        process.pid,
        LIVE.live().saturating_sub(1)
    );
}

fn wait_for(
    rx: &mpsc::Receiver<AsrResponse>,
    id: u64,
    budget: Duration,
) -> Result<AsrResponse, String> {
    let deadline = Instant::now() + budget;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(JOB_DEADLINE_EXCEEDED.to_string());
        }
        match rx.recv_timeout(left) {
            Ok(resp) if resp.id == id => return Ok(resp),
            Ok(_) => continue,
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(JOB_DEADLINE_EXCEEDED.to_string()),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("the speech helper exited unexpectedly".to_string())
            }
        }
    }
}

/// Write the samples file readable by this user only.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("could not stage audio for the speech helper: {e}"))?;
    f.write_all(bytes)
        .map_err(|e| format!("could not stage audio for the speech helper: {e}"))
}

impl Transcriber for HelperEngine {
    fn transcribe(
        &mut self,
        samples_16k_mono: &[f32],
        language: Option<&str>,
        bias_prompt: Option<&str>,
    ) -> Result<String, String> {
        self.run_job(samples_16k_mono, language, bias_prompt, false)
            .map(|r| r.timed.map(|t| t.text).unwrap_or_default())
    }

    fn transcribe_timed(
        &mut self,
        samples_16k_mono: &[f32],
        language: Option<&str>,
        bias_prompt: Option<&str>,
    ) -> Result<asr_engine::TimedTranscript, String> {
        let resp = self.run_job(samples_16k_mono, language, bias_prompt, true)?;
        let Some(t) = resp.timed else {
            return Ok(asr_engine::TimedTranscript::text_only(String::new()));
        };
        let span = |s: &crate::asr_protocol::WireSpan| asr_engine::TimedSpan {
            start_seconds: s.start_seconds,
            end_seconds: s.end_seconds,
            text: s.text.clone(),
        };
        Ok(asr_engine::TimedTranscript {
            text: t.text.clone(),
            kind: timed_kind(&t.kind),
            segments: t.segments.iter().map(span).collect(),
            words: t.words.iter().map(span).collect(),
        })
    }

    fn cancel_handle(&self) -> Option<CancelHandle> {
        let process = self.proc_.as_ref()?;
        let stdin = process.stdin.clone();
        let pid = process.pid;
        let running = self.running.clone();
        Some(Arc::new(move || {
            let job = running.load(Ordering::Acquire);
            log::info!("engine cancel requested pid={pid} job={job}");
            let _ = send_line(&stdin, &AsrRequest::cancel(0));
            if job != 0 {
                kill_if_still_running(pid, job, running.clone());
            }
        }))
    }

    fn language_support(&self) -> Option<crate::languages::LanguageSupport> {
        self.caps
            .as_ref()
            .map(crate::languages::LanguageSupport::from_capabilities)
    }

    fn last_detected_language(&self) -> Option<String> {
        self.last_language.clone()
    }
}

impl Drop for HelperEngine {
    fn drop(&mut self) {
        let (clean, waited) = self.shutdown();
        if clean || waited > 0 {
            publish(EngineHealth::Idle, "unloaded", "none");
        }
    }
}

// ---------------------------------------------------------------------------
// Quit
// ---------------------------------------------------------------------------

/// The one grep-able quit line.
pub fn exit_line(freed: u64, waited_ms: u128, skipped_destructors: bool) -> String {
    format!("exit: engines freed={freed} waited_ms={waited_ms} skipped_destructors={skipped_destructors}")
}

/// Engines freed before the quit teardown ran — pass to [`finish_exit`].
pub fn exit_snapshot() -> (u64, Instant) {
    (engines_freed(), Instant::now())
}

/// The last thing the Exit arm does, AFTER the quit teardown (which drains the
/// manager and checkpoints the DB). Kills any helper still unreaped by PID,
/// logs [`exit_line`], and when an in-process engine is still live, ends the
/// process with `_exit(0)` so ggml's static destructors never run.
pub fn finish_exit(snapshot: (u64, Instant)) {
    let (freed_before, started) = snapshot;
    let killed = kill_spawned_helpers();
    let live = engines_live();
    let freed = engines_freed().saturating_sub(freed_before) + killed as u64;
    let skipped = live > 0;
    log::info!(
        "{}",
        exit_line(freed, started.elapsed().as_millis(), skipped)
    );
    if skipped {
        log::logger().flush();
        // SAFETY: the DB checkpoint has run (quit teardown's last step); `_exit`
        // skips C++ static destructors, which is the point.
        unsafe { libc::_exit(0) }
    }
}

// ---------------------------------------------------------------------------
// The owner actor: one thread, one engine, one queue
// ---------------------------------------------------------------------------

/// Who a job is for. Lower priority value runs first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    Dictation,
    Retry,
    Meeting,
    Recovery,
}

impl JobKind {
    pub fn priority(self) -> u8 {
        match self {
            JobKind::Dictation => 0,
            JobKind::Retry => 1,
            JobKind::Meeting | JobKind::Recovery => 2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::Dictation => "dictation",
            JobKind::Retry => "retry",
            JobKind::Meeting => "meeting",
            JobKind::Recovery => "recovery",
        }
    }
}

/// One decode request.
pub struct JobRequest {
    pub kind: JobKind,
    pub samples: Vec<f32>,
    pub language: Option<String>,
    pub bias_prompt: Option<String>,
    pub deadline: Duration,
}

impl JobRequest {
    /// A job whose deadline comes from the clip length ([`job_deadline`]).
    pub fn new(kind: JobKind, samples: Vec<f32>) -> JobRequest {
        let deadline = job_deadline(samples.len(), 16_000);
        JobRequest {
            kind,
            samples,
            language: None,
            bias_prompt: None,
            deadline,
        }
    }
}

/// How the owner builds its one engine.
pub type OwnerLoader = Arc<dyn Fn() -> Result<Box<dyn Transcriber>, String> + Send + Sync>;

/// Owner tunables.
#[derive(Debug, Clone, Copy)]
pub struct OwnerConfig {
    /// How long a submitter waits beyond its own deadline for the jobs ahead
    /// of it, before it gives up with [`CATCHING_UP`].
    pub queue_wait_cap: Duration,
    /// Jobs allowed to wait at once; one more fails fast with [`QUEUE_FULL`].
    pub max_queue: usize,
    /// After the deadline's cancel, how long a decode has to return before
    /// the owner marks the engine `stuck`.
    pub stuck_grace: Duration,
}

impl Default for OwnerConfig {
    fn default() -> Self {
        OwnerConfig {
            queue_wait_cap: Duration::from_secs(120),
            max_queue: 32,
            stuck_grace: HELPER_CANCEL_GRACE,
        }
    }
}

struct QueuedJob {
    id: u64,
    req: JobRequest,
    reply: mpsc::Sender<Result<String, String>>,
}

struct OwnerState {
    queue: VecDeque<QueuedJob>,
    accepting: bool,
    stop: bool,
    restart_requested: bool,
    running: Option<(u64, JobKind, Option<CancelHandle>)>,
    health: EngineHealth,
    finished: bool,
}

struct OwnerShared {
    state: Mutex<OwnerState>,
    cv: Condvar,
    loader: OwnerLoader,
    config: OwnerConfig,
    next_id: AtomicU64,
    loads: AtomicU64,
    freed: AtomicU64,
}

impl OwnerShared {
    fn set_health(&self, st: &mut OwnerState, to: EngineHealth, reason: &str, job: &str) {
        let from = st.health;
        st.health = to;
        log::info!(
            "{}",
            transition_line(
                from,
                to,
                reason,
                self.loads
                    .load(Ordering::Acquire)
                    .saturating_sub(self.freed.load(Ordering::Acquire)),
                job
            )
        );
    }
}

/// What quit reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuitReport {
    pub freed: u64,
    pub waited_ms: u128,
    /// The owner thread finished (engine dropped) inside the cap.
    pub finished: bool,
    /// A job was running and was asked to cancel.
    pub cancelled_running: bool,
    /// Queued jobs failed with [`OWNER_QUITTING`].
    pub failed_queued: usize,
}

/// The single owner of one speech engine. Cheap to clone.
#[derive(Clone)]
pub struct EngineOwner {
    shared: Arc<OwnerShared>,
}

impl EngineOwner {
    /// Start the owner thread. Nothing is loaded until the first job.
    pub fn spawn(loader: OwnerLoader, config: OwnerConfig) -> EngineOwner {
        let shared = Arc::new(OwnerShared {
            state: Mutex::new(OwnerState {
                queue: VecDeque::new(),
                accepting: true,
                stop: false,
                restart_requested: false,
                running: None,
                health: EngineHealth::Idle,
                finished: false,
            }),
            cv: Condvar::new(),
            loader,
            config,
            next_id: AtomicU64::new(1),
            loads: AtomicU64::new(0),
            freed: AtomicU64::new(0),
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("yap-engine-owner".into())
            .spawn(move || owner_loop(worker))
            .expect("spawn engine owner thread");
        EngineOwner { shared }
    }

    /// Loads the owner has performed.
    pub fn loads(&self) -> u64 {
        self.shared.loads.load(Ordering::Acquire)
    }

    /// Engines the owner has freed.
    pub fn freed(&self) -> u64 {
        self.shared.freed.load(Ordering::Acquire)
    }

    pub fn health(&self) -> EngineHealth {
        self.shared.state.lock().health
    }

    /// Jobs waiting (not counting the running one).
    pub fn queued(&self) -> usize {
        self.shared.state.lock().queue.len()
    }

    pub fn is_busy(&self) -> bool {
        self.shared.state.lock().running.is_some()
    }

    /// Run one job and wait for its text. Waits behind running/queued jobs
    /// (dictation first), never starts a parallel engine.
    pub fn submit(&self, req: JobRequest) -> Result<String, String> {
        let kind = req.kind;
        let wait = req.deadline + self.shared.config.queue_wait_cap;
        let id = self.shared.next_id.fetch_add(1, Ordering::AcqRel);
        let (tx, rx) = mpsc::channel();
        {
            let mut st = self.shared.state.lock();
            if !st.accepting {
                return Err(OWNER_QUITTING.to_string());
            }
            if st.queue.len() >= self.shared.config.max_queue {
                log::warn!(
                    "engine job refused kind={} reason=queue_full",
                    kind.as_str()
                );
                return Err(QUEUE_FULL.to_string());
            }
            st.queue.push_back(QueuedJob { id, req, reply: tx });
            log::info!(
                "engine job queued id={id} kind={} queued={}",
                kind.as_str(),
                st.queue.len()
            );
            self.shared.cv.notify_all();
        }
        match rx.recv_timeout(wait) {
            Ok(result) => result,
            Err(_) => {
                let mut st = self.shared.state.lock();
                st.queue.retain(|j| j.id != id);
                if st.running.is_some() && st.health == EngineHealth::Busy {
                    self.shared.set_health(
                        &mut st,
                        EngineHealth::Stuck,
                        "submit_wait_passed",
                        kind.as_str(),
                    );
                }
                log::warn!(
                    "engine job gave_up id={id} kind={} wait_ms={}",
                    kind.as_str(),
                    wait.as_millis()
                );
                Err(CATCHING_UP.to_string())
            }
        }
    }

    /// The watchdog's seam (yap-1106-engine-watchdog-restart): drop the engine
    /// at the next idle moment so the next job loads exactly one fresh copy.
    pub fn restart(&self) {
        let mut st = self.shared.state.lock();
        st.restart_requested = true;
        self.shared.set_health(
            &mut st,
            EngineHealth::Restarting,
            "restart_requested",
            "none",
        );
        self.shared.cv.notify_all();
    }

    /// The watchdog's other seam: mark the running job's engine stuck.
    pub fn mark_stuck(&self, reason: &str) {
        let mut st = self.shared.state.lock();
        self.shared
            .set_health(&mut st, EngineHealth::Stuck, reason, "none");
    }

    /// Stop accepting, fail queued jobs, cancel the running one, and wait up
    /// to `cap` for the owner thread to drop the engine.
    pub fn quit(&self, cap: Duration) -> QuitReport {
        let started = Instant::now();
        let (cancelled_running, failed_queued) = {
            let mut st = self.shared.state.lock();
            st.accepting = false;
            st.stop = true;
            let failed = st.queue.len();
            for job in st.queue.drain(..) {
                let _ = job.reply.send(Err(OWNER_QUITTING.to_string()));
            }
            let cancelled = match st.running.as_ref() {
                Some((_, _, Some(cancel))) => {
                    cancel();
                    true
                }
                _ => false,
            };
            self.shared.cv.notify_all();
            (cancelled, failed)
        };
        let deadline = started + cap;
        let mut st = self.shared.state.lock();
        while !st.finished {
            if self.shared.cv.wait_until(&mut st, deadline).timed_out() {
                break;
            }
        }
        let report = QuitReport {
            freed: self.freed(),
            waited_ms: started.elapsed().as_millis(),
            finished: st.finished,
            cancelled_running,
            failed_queued,
        };
        log::info!(
            "engine owner quit freed={} waited_ms={} finished={} cancelled_running={} failed_queued={}",
            report.freed,
            report.waited_ms,
            report.finished,
            report.cancelled_running,
            report.failed_queued
        );
        report
    }
}

enum Next {
    Job(QueuedJob),
    Restart,
    Stop,
}

/// Highest priority first, FIFO within a priority.
fn pop_best(queue: &mut VecDeque<QueuedJob>) -> Option<QueuedJob> {
    let best = queue
        .iter()
        .enumerate()
        .min_by_key(|(_, j)| (j.req.kind.priority(), j.id))
        .map(|(i, _)| i)?;
    queue.remove(best)
}

fn owner_loop(shared: Arc<OwnerShared>) {
    let mut engine: Option<Box<dyn Transcriber>> = None;
    loop {
        let next = {
            let mut st = shared.state.lock();
            loop {
                if st.stop {
                    break Next::Stop;
                }
                if st.restart_requested {
                    st.restart_requested = false;
                    break Next::Restart;
                }
                if let Some(job) = pop_best(&mut st.queue) {
                    break Next::Job(job);
                }
                shared.cv.wait(&mut st);
            }
        };
        let job = match next {
            Next::Stop => break,
            Next::Restart => {
                if engine.take().is_some() {
                    shared.freed.fetch_add(1, Ordering::AcqRel);
                }
                let mut st = shared.state.lock();
                shared.set_health(&mut st, EngineHealth::Idle, "restarted", "none");
                continue;
            }
            Next::Job(job) => job,
        };
        let kind = job.req.kind;
        if engine.is_none() {
            {
                let mut st = shared.state.lock();
                shared.set_health(&mut st, EngineHealth::Loading, "first_job", kind.as_str());
            }
            match (shared.loader)() {
                Ok(e) => {
                    shared.loads.fetch_add(1, Ordering::AcqRel);
                    engine = Some(e);
                }
                Err(e) => {
                    let mut st = shared.state.lock();
                    shared.set_health(&mut st, EngineHealth::Idle, "load_failed", kind.as_str());
                    drop(st);
                    let _ = job.reply.send(Err(e));
                    continue;
                }
            }
        }
        let Some(eng) = engine.as_mut() else { continue };
        eng.reset_cancel();
        let cancel = eng.cancel_handle();
        {
            let mut st = shared.state.lock();
            st.running = Some((job.id, kind, cancel.clone()));
            shared.set_health(&mut st, EngineHealth::Busy, "job_started", kind.as_str());
        }
        let started = Instant::now();
        let timed_out = Arc::new(AtomicBool::new(false));
        let (done_tx, done_rx) = mpsc::channel::<()>();
        {
            let timed_out = timed_out.clone();
            let deadline = job.req.deadline;
            let grace = shared.config.stuck_grace;
            let watcher = shared.clone();
            let _ = std::thread::Builder::new()
                .name("yap-engine-deadline".into())
                .spawn(move || {
                    if let Err(mpsc::RecvTimeoutError::Timeout) = done_rx.recv_timeout(deadline) {
                        timed_out.store(true, Ordering::Release);
                        log::warn!(
                            "engine job deadline_passed kind={} deadline_ms={}",
                            kind.as_str(),
                            deadline.as_millis()
                        );
                        if let Some(cancel) = cancel {
                            cancel();
                        }
                        if let Err(mpsc::RecvTimeoutError::Timeout) = done_rx.recv_timeout(grace) {
                            let mut st = watcher.state.lock();
                            watcher.set_health(
                                &mut st,
                                EngineHealth::Stuck,
                                "cancel_ignored",
                                kind.as_str(),
                            );
                        }
                    }
                });
        }
        let result = eng.transcribe(
            &job.req.samples,
            job.req.language.as_deref(),
            job.req.bias_prompt.as_deref(),
        );
        drop(done_tx);
        let result = if timed_out.load(Ordering::Acquire) {
            Err(JOB_DEADLINE_EXCEEDED.to_string())
        } else {
            result
        };
        log::info!(
            "engine job done id={} kind={} ms={} ok={}",
            job.id,
            kind.as_str(),
            started.elapsed().as_millis(),
            result.is_ok()
        );
        {
            let mut st = shared.state.lock();
            st.running = None;
            shared.set_health(&mut st, EngineHealth::Ready, "job_done", kind.as_str());
        }
        let _ = job.reply.send(result);
    }
    // Quit: the engine is dropped HERE, on its owning thread, before the
    // owner reports finished — so the GPU memory is released before quit
    // returns to the Exit arm.
    if let Some(e) = engine.take() {
        drop(e);
        shared.freed.fetch_add(1, Ordering::AcqRel);
    }
    let mut st = shared.state.lock();
    for job in st.queue.drain(..) {
        let _ = job.reply.send(Err(OWNER_QUITTING.to_string()));
    }
    shared.set_health(&mut st, EngineHealth::Idle, "owner_stopped", "none");
    st.finished = true;
    shared.cv.notify_all();
}

// ---------------------------------------------------------------------------
// The production job queue: every decode the app makes is a job here
// ---------------------------------------------------------------------------
//
// `TranscriptionManager` keeps the one engine (the `yap-asr` helper built by
// `load_engine`) and lends it out per decode. Every one of those leases —
// dictation, a retry of a failed dictation, a meeting chunk, meeting recovery —
// first takes a ticket from the manager's `JobQueue`. Exactly one ticket runs
// at a time; waiting tickets are admitted dictation first, then retry, then
// meeting/recovery, FIFO inside a kind. A take that cannot get its turn inside
// its wait cap fails with the plain `CATCHING_UP` sentence (its audio is kept
// by the caller), a full queue fails fast with `QUEUE_FULL`, and once quit has
// closed the queue nothing new is admitted (`OWNER_QUITTING`). Nothing here
// can start a second engine: the queue only ever decides WHO decodes next on
// the engine that exists.

thread_local! {
    static JOB_KIND: std::cell::Cell<Option<JobKind>> = std::cell::Cell::new(None);
}

/// Run `f` with every decode it makes on THIS thread filed as `kind` — how a
/// retry of a failed dictation is queued as a retry and not as a live take.
/// The previous kind is restored afterwards, even on unwind.
pub fn as_job<R>(kind: JobKind, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<JobKind>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0;
            JOB_KIND.with(|k| k.set(previous));
        }
    }
    let previous = JOB_KIND.with(|k| k.replace(Some(kind)));
    let _restore = Restore(previous);
    f()
}

/// The kind a lease taken on this thread is filed as: an explicit [`as_job`]
/// wins; otherwise a preemptible lease is a meeting chunk and anything else is
/// a live dictation.
pub fn current_job_kind(preemptible: bool) -> JobKind {
    JOB_KIND.with(|k| k.get()).unwrap_or(if preemptible {
        JobKind::Meeting
    } else {
        JobKind::Dictation
    })
}

/// Tickets allowed to wait at once before one more fails fast.
pub const JOB_QUEUE_MAX_WAITING: usize = 32;

struct JobQueueState {
    running: Option<(u64, JobKind)>,
    waiting: Vec<(u64, JobKind)>,
    next_id: u64,
    closed: bool,
}

impl JobQueueState {
    /// The waiting ticket that runs next: highest priority, then oldest.
    fn best(&self) -> Option<u64> {
        self.waiting
            .iter()
            .min_by_key(|(id, kind)| (kind.priority(), *id))
            .map(|(id, _)| *id)
    }

    fn forget(&mut self, id: u64) {
        self.waiting.retain(|(w, _)| *w != id);
    }
}

/// One engine's queue of decode jobs. See the section comment above.
pub struct JobQueue {
    state: Mutex<JobQueueState>,
    cv: Condvar,
    max_waiting: usize,
}

/// The right to decode on the engine. Dropping it hands the turn on.
pub struct JobTicket<'a> {
    queue: &'a JobQueue,
    id: u64,
    kind: JobKind,
    admitted: Instant,
}

impl JobTicket<'_> {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn kind(&self) -> JobKind {
        self.kind
    }
}

impl Drop for JobTicket<'_> {
    fn drop(&mut self) {
        let waiting = {
            let mut st = self.queue.state.lock();
            if matches!(st.running, Some((id, _)) if id == self.id) {
                st.running = None;
            }
            self.queue.cv.notify_all();
            st.waiting.len()
        };
        log::info!(
            "engine job done id={} kind={} ran_ms={} queued={waiting}",
            self.id,
            self.kind.as_str(),
            self.admitted.elapsed().as_millis()
        );
        if health() == EngineHealth::Busy {
            let to = if engines_live() > 0 {
                EngineHealth::Ready
            } else {
                EngineHealth::Idle
            };
            publish(to, "job_done", self.kind.as_str());
        }
    }
}

impl JobQueue {
    pub fn new(max_waiting: usize) -> JobQueue {
        JobQueue {
            state: Mutex::new(JobQueueState {
                running: None,
                waiting: Vec::new(),
                next_id: 1,
                closed: false,
            }),
            cv: Condvar::new(),
            max_waiting,
        }
    }

    /// Wait for this job's turn (at most `wait_cap`) and take it.
    pub fn admit(&self, kind: JobKind, wait_cap: Duration) -> Result<JobTicket<'_>, String> {
        let started = Instant::now();
        let deadline = started + wait_cap;
        let mut st = self.state.lock();
        if st.closed {
            log::info!("engine job refused kind={} reason=quitting", kind.as_str());
            return Err(OWNER_QUITTING.to_string());
        }
        let id = st.next_id;
        st.next_id += 1;
        if st.running.is_none() && st.waiting.is_empty() {
            st.running = Some((id, kind));
            drop(st);
            return Ok(self.started(id, kind, started));
        }
        if st.waiting.len() >= self.max_waiting {
            log::warn!(
                "engine job refused id={id} kind={} reason=queue_full queued={}",
                kind.as_str(),
                st.waiting.len()
            );
            return Err(QUEUE_FULL.to_string());
        }
        st.waiting.push((id, kind));
        let ahead = st.running.map(|(_, k)| k.as_str()).unwrap_or("none");
        log::info!(
            "engine job queued id={id} kind={} queued={} behind={ahead}",
            kind.as_str(),
            st.waiting.len()
        );
        loop {
            if st.closed {
                st.forget(id);
                self.cv.notify_all();
                log::info!(
                    "engine job refused id={id} kind={} reason=quitting",
                    kind.as_str()
                );
                return Err(OWNER_QUITTING.to_string());
            }
            if st.running.is_none() && st.best() == Some(id) {
                st.forget(id);
                st.running = Some((id, kind));
                drop(st);
                return Ok(self.started(id, kind, started));
            }
            if self.cv.wait_until(&mut st, deadline).timed_out() {
                if !st.closed && st.running.is_none() && st.best() == Some(id) {
                    continue;
                }
                st.forget(id);
                // This ticket may have been the best one: let the next look.
                self.cv.notify_all();
                drop(st);
                log::warn!(
                    "engine job gave_up id={id} kind={} wait_ms={}",
                    kind.as_str(),
                    started.elapsed().as_millis()
                );
                return Err(CATCHING_UP.to_string());
            }
        }
    }

    fn started(&self, id: u64, kind: JobKind, queued_at: Instant) -> JobTicket<'_> {
        log::info!(
            "engine job started id={id} kind={} waited_ms={}",
            kind.as_str(),
            queued_at.elapsed().as_millis()
        );
        publish(EngineHealth::Busy, "job_started", kind.as_str());
        JobTicket {
            queue: self,
            id,
            kind,
            admitted: Instant::now(),
        }
    }

    /// Quit: admit nothing more and fail every waiting ticket with
    /// [`OWNER_QUITTING`]. The running job keeps its ticket; the drain
    /// cancels it. Returns how many waiting jobs were turned away.
    pub fn close(&self) -> usize {
        let mut st = self.state.lock();
        st.closed = true;
        let turned_away = st.waiting.len();
        self.cv.notify_all();
        turned_away
    }

    pub fn is_closed(&self) -> bool {
        self.state.lock().closed
    }

    /// Jobs waiting for their turn (not counting the running one).
    pub fn waiting(&self) -> usize {
        self.state.lock().waiting.len()
    }

    /// The kind of the job decoding right now, if any.
    pub fn running(&self) -> Option<JobKind> {
        self.state.lock().running.map(|(_, k)| k)
    }
}

#[cfg(test)]
mod engine_owner_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// Shared event log the fakes write to, in order.
    type Events = Arc<Mutex<Vec<String>>>;

    #[derive(Clone, Copy, PartialEq)]
    enum Behaviour {
        /// Returns at once.
        Quick,
        /// Runs until cancelled (or 10 s).
        LongButCancellable,
        /// Waits for the gate, then returns; honours cancel too.
        Gated,
        /// Ignores cancel; returns only when the gate opens.
        HangsUntilGate,
    }

    struct Gate {
        open: Mutex<bool>,
        cv: Condvar,
    }

    impl Gate {
        fn new() -> Arc<Gate> {
            Arc::new(Gate {
                open: Mutex::new(false),
                cv: Condvar::new(),
            })
        }
        fn release(&self) {
            *self.open.lock() = true;
            self.cv.notify_all();
        }
        fn is_open(&self) -> bool {
            *self.open.lock()
        }
        fn wait_a_bit(&self) {
            let mut g = self.open.lock();
            if !*g {
                self.cv.wait_for(&mut g, Duration::from_millis(5));
            }
        }
    }

    struct FakeEngine {
        events: Events,
        behaviour: Behaviour,
        gate: Arc<Gate>,
        cancelled: Arc<AtomicBool>,
        live: Arc<AtomicUsize>,
    }

    impl Transcriber for FakeEngine {
        fn transcribe(
            &mut self,
            samples: &[f32],
            _language: Option<&str>,
            _bias: Option<&str>,
        ) -> Result<String, String> {
            let tag = samples.first().copied().unwrap_or(0.0) as i64;
            self.events.lock().push(format!("decode:{tag}"));
            let started = Instant::now();
            loop {
                let cancelled = self.cancelled.load(Ordering::Acquire);
                match self.behaviour {
                    Behaviour::Quick => break,
                    Behaviour::LongButCancellable => {
                        if cancelled || started.elapsed() > Duration::from_secs(10) {
                            break;
                        }
                    }
                    Behaviour::Gated => {
                        if cancelled || self.gate.is_open() {
                            break;
                        }
                    }
                    Behaviour::HangsUntilGate => {
                        if self.gate.is_open() {
                            break;
                        }
                    }
                }
                self.gate.wait_a_bit();
            }
            if self.cancelled.load(Ordering::Acquire) {
                self.events.lock().push(format!("returned_cancelled:{tag}"));
                return Err("aborted".into());
            }
            Ok(format!("text {tag}"))
        }

        fn cancel_handle(&self) -> Option<CancelHandle> {
            let flag = self.cancelled.clone();
            let events = self.events.clone();
            Some(Arc::new(move || {
                events.lock().push("cancel".into());
                flag.store(true, Ordering::Release);
            }))
        }

        fn reset_cancel(&mut self) {
            self.cancelled.store(false, Ordering::Release);
        }
    }

    impl Drop for FakeEngine {
        fn drop(&mut self) {
            let thread = std::thread::current();
            self.events.lock().push(format!(
                "engine_dropped_on:{}",
                thread.name().unwrap_or("?")
            ));
            self.live.fetch_sub(1, Ordering::AcqRel);
        }
    }

    struct Rig {
        owner: EngineOwner,
        events: Events,
        gate: Arc<Gate>,
        loads: Arc<AtomicUsize>,
        live: Arc<AtomicUsize>,
        peak_live: Arc<AtomicUsize>,
    }

    fn rig(behaviour: Behaviour, config: OwnerConfig) -> Rig {
        let events: Events = Arc::new(Mutex::new(Vec::new()));
        let gate = Gate::new();
        let loads = Arc::new(AtomicUsize::new(0));
        let live = Arc::new(AtomicUsize::new(0));
        let peak_live = Arc::new(AtomicUsize::new(0));
        let loader: OwnerLoader = {
            let (events, gate, loads, live, peak_live) = (
                events.clone(),
                gate.clone(),
                loads.clone(),
                live.clone(),
                peak_live.clone(),
            );
            Arc::new(move || {
                loads.fetch_add(1, Ordering::AcqRel);
                let now = live.fetch_add(1, Ordering::AcqRel) + 1;
                peak_live.fetch_max(now, Ordering::AcqRel);
                // A load takes a moment, so concurrent takes really overlap it.
                std::thread::sleep(Duration::from_millis(30));
                events.lock().push("load".into());
                Ok(Box::new(FakeEngine {
                    events: events.clone(),
                    behaviour,
                    gate: gate.clone(),
                    cancelled: Arc::new(AtomicBool::new(false)),
                    live: live.clone(),
                }) as Box<dyn Transcriber>)
            })
        };
        Rig {
            owner: EngineOwner::spawn(loader, config),
            events,
            gate,
            loads,
            live,
            peak_live,
        }
    }

    fn job(kind: JobKind, tag: f32, deadline: Duration) -> JobRequest {
        JobRequest {
            kind,
            samples: vec![tag; 16],
            language: None,
            bias_prompt: None,
            deadline,
        }
    }

    fn wait_until(what: impl Fn() -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !what() {
            assert!(Instant::now() < end, "condition never became true");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn position(events: &[String], needle: &str) -> usize {
        events
            .iter()
            .position(|e| e.starts_with(needle))
            .unwrap_or_else(|| panic!("no event {needle} in {events:?}"))
    }

    // (a)
    #[test]
    fn ten_concurrent_takes_produce_exactly_one_load() {
        let r = rig(Behaviour::Quick, OwnerConfig::default());
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let owner = r.owner.clone();
                std::thread::spawn(move || {
                    owner.submit(job(JobKind::Dictation, i as f32, Duration::from_secs(5)))
                })
            })
            .collect();
        for h in handles {
            assert!(h.join().unwrap().is_ok());
        }
        assert_eq!(r.loads.load(Ordering::Acquire), 1);
        assert_eq!(r.owner.loads(), 1);
        assert_eq!(r.peak_live.load(Ordering::Acquire), 1);
        r.owner.quit(Duration::from_secs(2));
    }

    // (b)
    #[test]
    fn job_past_its_deadline_is_cancelled_and_returns_an_error() {
        let r = rig(Behaviour::LongButCancellable, OwnerConfig::default());
        let started = Instant::now();
        let out = r
            .owner
            .submit(job(JobKind::Dictation, 1.0, Duration::from_millis(100)));
        assert_eq!(out, Err(JOB_DEADLINE_EXCEEDED.to_string()));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "waited the decode out"
        );
        let events = r.events.lock().clone();
        assert!(position(&events, "cancel") > position(&events, "decode:1"));
        // The engine survives a honoured cancel: the next job reuses it.
        assert_eq!(r.owner.loads(), 1);
        r.owner.quit(Duration::from_secs(2));
    }

    // (c)
    #[test]
    fn the_queue_orders_dictation_before_retry_before_meeting() {
        let r = rig(Behaviour::Gated, OwnerConfig::default());
        let submit = |kind: JobKind, tag: f32| {
            let owner = r.owner.clone();
            std::thread::spawn(move || owner.submit(job(kind, tag, Duration::from_secs(10))))
        };
        let first = submit(JobKind::Meeting, 1.0);
        wait_until(|| r.owner.is_busy());
        let meeting = submit(JobKind::Meeting, 2.0);
        wait_until(|| r.owner.queued() == 1);
        let retry = submit(JobKind::Retry, 3.0);
        wait_until(|| r.owner.queued() == 2);
        let recovery = submit(JobKind::Recovery, 4.0);
        wait_until(|| r.owner.queued() == 3);
        let dictation = submit(JobKind::Dictation, 5.0);
        wait_until(|| r.owner.queued() == 4);
        r.gate.release();
        for h in [first, meeting, retry, recovery, dictation] {
            assert!(h.join().unwrap().is_ok());
        }
        let decodes: Vec<String> = r
            .events
            .lock()
            .iter()
            .filter(|e| e.starts_with("decode:"))
            .cloned()
            .collect();
        assert_eq!(
            decodes,
            vec!["decode:1", "decode:5", "decode:3", "decode:2", "decode:4"]
        );
        r.owner.quit(Duration::from_secs(2));
    }

    // (d) + the teardown-order test
    #[test]
    fn quit_with_a_job_in_flight_cancels_waits_frees_and_reports_freed_1() {
        let r = rig(Behaviour::LongButCancellable, OwnerConfig::default());
        let owner = r.owner.clone();
        let take = std::thread::spawn(move || {
            owner.submit(job(JobKind::Meeting, 7.0, Duration::from_secs(30)))
        });
        wait_until(|| r.owner.is_busy());
        let report = r.owner.quit(Duration::from_secs(3));
        // What the Exit arm does next.
        r.events.lock().push("db_checkpoint".into());
        assert!(
            report.finished,
            "owner thread did not finish inside the cap"
        );
        assert!(report.cancelled_running);
        assert_eq!(report.freed, 1);
        assert!(report.waited_ms < 3_000);
        assert!(take.join().unwrap().is_err());
        assert_eq!(r.live.load(Ordering::Acquire), 0);
        let events = r.events.lock().clone();
        let cancel = position(&events, "cancel");
        let returned = position(&events, "returned_cancelled:7");
        let dropped = position(&events, "engine_dropped_on:yap-engine-owner");
        let checkpoint = position(&events, "db_checkpoint");
        assert!(
            cancel < returned && returned < dropped && dropped < checkpoint,
            "{events:?}"
        );
        // After quit nothing new is accepted.
        assert_eq!(
            r.owner
                .submit(job(JobKind::Dictation, 9.0, Duration::from_secs(1))),
            Err(OWNER_QUITTING.to_string())
        );
    }

    #[test]
    fn quit_past_the_cap_returns_instead_of_hanging() {
        let r = rig(Behaviour::HangsUntilGate, OwnerConfig::default());
        let owner = r.owner.clone();
        let take = std::thread::spawn(move || {
            owner.submit(job(JobKind::Dictation, 1.0, Duration::from_secs(30)))
        });
        wait_until(|| r.owner.is_busy());
        let started = Instant::now();
        let report = r.owner.quit(Duration::from_millis(200));
        assert!(!report.finished);
        assert_eq!(report.freed, 0);
        assert!(started.elapsed() < Duration::from_secs(2));
        r.gate.release();
        let _ = take.join();
    }

    // FX-02: a fake engine that hangs forever never causes a pile-up.
    #[test]
    fn a_hung_engine_yields_at_most_one_live_copy_across_ten_takes() {
        let config = OwnerConfig {
            queue_wait_cap: Duration::from_millis(50),
            max_queue: 32,
            stuck_grace: Duration::from_millis(50),
        };
        let r = rig(Behaviour::HangsUntilGate, config);
        let mut errors = 0;
        for i in 0..10 {
            let out = r
                .owner
                .submit(job(JobKind::Dictation, i as f32, Duration::from_millis(50)));
            if out.is_err() {
                errors += 1;
            }
        }
        assert_eq!(errors, 10, "every take reports a failure the caller keeps");
        assert_eq!(r.loads.load(Ordering::Acquire), 1);
        assert!(r.peak_live.load(Ordering::Acquire) <= 2);
        assert_eq!(r.owner.health(), EngineHealth::Stuck);
        // When the hung decode finally returns, its engine is freed at quit.
        r.gate.release();
        let report = r.owner.quit(Duration::from_secs(3));
        assert!(report.finished);
        assert_eq!(report.freed, 1);
        assert_eq!(r.live.load(Ordering::Acquire), 0);
    }

    #[test]
    fn restart_drops_the_engine_and_the_next_job_loads_exactly_once_more() {
        let r = rig(Behaviour::Quick, OwnerConfig::default());
        assert!(r
            .owner
            .submit(job(JobKind::Dictation, 1.0, Duration::from_secs(5)))
            .is_ok());
        r.owner.restart();
        wait_until(|| r.owner.freed() == 1);
        assert!(r
            .owner
            .submit(job(JobKind::Dictation, 2.0, Duration::from_secs(5)))
            .is_ok());
        assert_eq!(r.owner.loads(), 2);
        assert_eq!(r.peak_live.load(Ordering::Acquire), 1);
        r.owner.quit(Duration::from_secs(2));
    }

    #[test]
    fn a_third_live_engine_is_refused_with_catching_up() {
        let counter: &'static LiveCounter = Box::leak(Box::new(LiveCounter::new()));
        let a = counter.acquire(MAX_LIVE_ENGINES).unwrap();
        let _b = counter.acquire(MAX_LIVE_ENGINES).unwrap();
        assert_eq!(
            counter.acquire(MAX_LIVE_ENGINES).err().as_deref(),
            Some(CATCHING_UP)
        );
        drop(a);
        assert!(counter.acquire(MAX_LIVE_ENGINES).is_ok());
        assert_eq!(counter.peak(), 2);
        assert_eq!(counter.loads(), 3);
        assert_eq!(counter.freed(), 2);
    }

    #[test]
    fn deadline_grows_with_the_clip_and_is_capped() {
        assert_eq!(job_deadline(0, 16_000), DEADLINE_BASE);
        assert_eq!(
            job_deadline(16_000 * 10, 16_000),
            DEADLINE_BASE + DEADLINE_PER_AUDIO_SECOND * 10
        );
        assert_eq!(job_deadline(16_000 * 60 * 60 * 5, 16_000), DEADLINE_CEILING);
    }

    #[test]
    fn telemetry_lines_have_the_documented_shape() {
        assert_eq!(
            transition_line(
                EngineHealth::Busy,
                EngineHealth::Stuck,
                "cancel_ignored",
                1,
                "dictation"
            ),
            "engine state from=busy to=stuck reason=cancel_ignored engines_live=1 job=dictation"
        );
        assert_eq!(
            exit_line(1, 42, false),
            "exit: engines freed=1 waited_ms=42 skipped_destructors=false"
        );
        let names: Vec<&str> = EngineHealth::ALL.iter().map(|h| h.as_str()).collect();
        assert_eq!(
            names,
            ["idle", "loading", "ready", "busy", "stuck", "restarting"]
        );
        for h in EngineHealth::ALL {
            assert_eq!(EngineHealth::from_u8(h.to_u8()), h);
        }
    }

    // (e) the grep gate: no module but this one constructs a speech engine.
    #[test]
    fn only_the_owner_constructs_an_engine() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        // Built from pieces so this file's own text never matches.
        let forbidden = [
            ["asr_engine", "::load("].concat(),
            ["HelperEngine", "::spawn("].concat(),
            ["Model::", "load_with("].concat(),
            ["Command::new(", "\"yap-asr\")"].concat(),
        ];
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("read src") {
            let path = entry.expect("dir entry").path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !name.ends_with(".rs") || name == "engine_owner.rs" || name == "asr_engine.rs" {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("read source");
            for needle in &forbidden {
                if src.contains(needle.as_str()) {
                    offenders.push(format!("{name}: {needle}"));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "only engine_owner.rs may construct a speech engine: {offenders:?}"
        );
    }

    // (g), unit level: quitting while the helper is mid-decode. A fake
    // `yap-asr` (a shell script speaking the real wire protocol) comes up,
    // "loads", takes a decode and then hangs in it. The quit path's
    // `kill_spawned_helpers` must kill exactly the PID the owner spawned, the
    // in-flight job must come back with an error well inside the exit cap,
    // and the process must be gone (macOS frees its GPU memory with it).
    #[test]
    fn quit_while_the_helper_is_mid_decode_kills_it_by_pid() {
        let _serial = helper_serial();
        use std::os::unix::fs::PermissionsExt;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("yap-fake-asr-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let ready = serde_json::to_string(&AsrReady::now(0)).expect("ready line");
        let loaded = serde_json::to_string(&AsrResponse::ok(1)).expect("load reply");
        let script = dir.join(ASR_HELPER_BIN);
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho '{ready}'\nread _load\necho '{loaded}'\nread _job\nexec sleep 600\n"
            ),
        )
        .expect("write fake helper");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake helper");

        let mut engine =
            HelperEngine::spawn(&script, &dir.join("never-read.gguf")).expect("fake helper up");
        let pid = engine.proc_.as_ref().expect("helper process").pid;
        assert!(spawned_helper_pids().contains(&pid));

        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = engine.transcribe(&[0.0f32; 16_000], None, None);
            let _ = tx.send(result);
            drop(engine);
        });
        // Let the decode request reach the helper; either way (sent or not)
        // the job must fail once its helper is gone.
        std::thread::sleep(Duration::from_millis(300));
        let started = Instant::now();
        assert!(kill_spawned_helpers() >= 1, "the quit path killed nothing");
        let result = rx
            .recv_timeout(HELPER_EXIT_CAP * 4)
            .expect("the mid-decode job returned after its helper was killed");
        assert!(result.is_err(), "a killed helper cannot have transcribed");
        assert!(started.elapsed() < HELPER_EXIT_CAP * 4);
        worker.join().expect("worker thread");
        assert!(
            !spawned_helper_pids().contains(&pid),
            "the killed helper is reaped and unregistered"
        );
        // SAFETY: signal 0 only probes; the PID was ours and is now reaped.
        let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
        assert!(!alive, "helper pid {pid} must be gone after quit");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The helper-process tests share one PID registry (and the quit test
    /// SIGKILLs everything in it), so they run one at a time.
    static HELPER_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn helper_serial() -> std::sync::MutexGuard<'static, ()> {
        HELPER_SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A fake `yap-asr` that speaks the real wire protocol, "loads", takes one
    /// decode and then hangs in it, never reading the cancel line.
    fn fake_hanging_helper(tag: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("yap-fake-asr-{tag}-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let ready = serde_json::to_string(&AsrReady::now(0)).expect("ready line");
        let loaded = serde_json::to_string(&AsrResponse::ok(1)).expect("load reply");
        let script = dir.join(ASR_HELPER_BIN);
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho '{ready}'\nread _load\necho '{loaded}'\nread _job\nexec sleep 600\n"
            ),
        )
        .expect("write fake helper");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake helper");
        (dir, script)
    }

    // (b) on the real helper: a cancel the helper ignores is escalated to a
    // kill BY ITS PID past the cancel grace, so the stuck decode returns and
    // the helper's GPU memory is freed instead of being left running.
    #[test]
    fn an_ignored_cancel_kills_the_helper_by_pid_after_the_grace() {
        let _serial = helper_serial();
        let (dir, script) = fake_hanging_helper("escalate");
        let mut engine =
            HelperEngine::spawn(&script, &dir.join("never-read.gguf")).expect("fake helper up");
        let pid = engine.proc_.as_ref().expect("helper process").pid;
        let running = engine.running.clone();
        let cancel = engine
            .cancel_handle()
            .expect("helper engines are cancellable");
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = engine.transcribe(&[0.0f32; 16_000], None, None);
            let _ = tx.send(result);
            drop(engine);
        });
        let until = Instant::now() + Duration::from_secs(5);
        while running.load(Ordering::Acquire) == 0 {
            assert!(Instant::now() < until, "the decode never started");
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = Instant::now();
        cancel();
        let result = rx
            .recv_timeout(HELPER_CANCEL_GRACE + Duration::from_secs(3))
            .expect("the stuck decode returned after its helper was killed");
        assert!(result.is_err(), "a killed helper cannot have transcribed");
        assert!(
            started.elapsed() >= HELPER_CANCEL_GRACE,
            "the helper gets its grace before the kill"
        );
        worker.join().expect("worker thread");
        assert!(!spawned_helper_pids().contains(&pid));
        // SAFETY: signal 0 only probes; the PID was ours and is now reaped.
        let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
        assert!(!alive, "helper pid {pid} must be gone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // FX-02 at the production seam: when `TranscriptionManager`'s lease gives
    // up on a hung helper decode, it cancels it; the ignored cancel becomes a
    // kill by PID, the abandoned engine is freed (never left on the GPU), and
    // the manager loaded exactly one engine for it.
    #[test]
    fn a_lease_timeout_frees_the_abandoned_helper_instead_of_leaving_it_running() {
        let _serial = helper_serial();
        let (dir, script) = fake_hanging_helper("lease");
        let model = dir.join("model.gguf");
        std::fs::write(&model, b"GGUF").expect("stub model file");
        let bin = script.clone();
        let manager = crate::transcription::TranscriptionManager::with_loader(
            Arc::new(move |path: &Path| {
                HelperEngine::spawn(&bin, path).map(|e| Box::new(e) as Box<dyn Transcriber>)
            }),
            Duration::from_secs(600),
            Duration::from_secs(600),
            Duration::from_millis(200),
        );
        let before_load = spawned_helper_pids();
        manager
            .load("fake/model", &model)
            .expect("fake helper loads");
        assert_eq!(manager.engine_loads(), 1);
        let mine: Vec<u32> = spawned_helper_pids()
            .into_iter()
            .filter(|p| !before_load.contains(p))
            .collect();
        assert_eq!(mine.len(), 1, "one helper for one load: {mine:?}");
        let err = manager
            .transcribe(vec![0.0f32; 16_000], None, None)
            .unwrap_err();
        assert!(err.contains("timed out"), "{err}");
        let until = Instant::now() + HELPER_CANCEL_GRACE + Duration::from_secs(5);
        while manager.abandoned_engines() != 0 {
            assert!(
                Instant::now() < until,
                "the abandoned helper was never freed"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        for pid in mine {
            assert!(
                !spawned_helper_pids().contains(&pid),
                "helper {pid} still registered after the lease gave up on it"
            );
        }
        assert_eq!(manager.engine_loads(), 1, "nothing reloaded on its own");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// yap-1106-engine-owner — the production queue every `TranscriptionManager`
/// lease takes a ticket from.
#[cfg(test)]
mod job_queue_tests {
    use super::*;
    use std::sync::Arc;

    fn wait_for_waiting(queue: &JobQueue, n: usize) {
        let started = Instant::now();
        while queue.waiting() < n {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "only {} of {n} jobs reached the queue",
                queue.waiting()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn the_job_queue_runs_dictation_before_retry_before_meeting() {
        let queue = Arc::new(JobQueue::new(JOB_QUEUE_MAX_WAITING));
        let order = Arc::new(Mutex::new(Vec::<JobKind>::new()));
        let first = queue
            .admit(JobKind::Meeting, Duration::from_secs(5))
            .expect("an idle queue admits at once");
        assert_eq!(queue.running(), Some(JobKind::Meeting));
        let mut threads = Vec::new();
        // Queued worst-first, so FIFO alone would get the order wrong.
        for (n, kind) in [JobKind::Meeting, JobKind::Retry, JobKind::Dictation]
            .into_iter()
            .enumerate()
        {
            let q = queue.clone();
            let o = order.clone();
            threads.push(std::thread::spawn(move || {
                let ticket = q
                    .admit(kind, Duration::from_secs(5))
                    .expect("admitted in turn");
                o.lock().push(ticket.kind());
                std::thread::sleep(Duration::from_millis(5));
            }));
            wait_for_waiting(&queue, n + 1);
        }
        drop(first);
        for t in threads {
            t.join().expect("job thread");
        }
        assert_eq!(
            *order.lock(),
            vec![JobKind::Dictation, JobKind::Retry, JobKind::Meeting]
        );
        assert_eq!(queue.running(), None);
        assert_eq!(queue.waiting(), 0);
    }

    #[test]
    fn only_one_job_holds_the_engine_at_a_time() {
        let queue = Arc::new(JobQueue::new(JOB_QUEUE_MAX_WAITING));
        let inside = Arc::new(AtomicU64::new(0));
        let peak = Arc::new(AtomicU64::new(0));
        let threads: Vec<_> = (0..10)
            .map(|_| {
                let q = queue.clone();
                let inside = inside.clone();
                let peak = peak.clone();
                std::thread::spawn(move || {
                    let _ticket = q
                        .admit(JobKind::Dictation, Duration::from_secs(10))
                        .expect("admitted");
                    let now = inside.fetch_add(1, Ordering::AcqRel) + 1;
                    peak.fetch_max(now, Ordering::AcqRel);
                    std::thread::sleep(Duration::from_millis(3));
                    inside.fetch_sub(1, Ordering::AcqRel);
                })
            })
            .collect();
        for t in threads {
            t.join().expect("job thread");
        }
        assert_eq!(peak.load(Ordering::Acquire), 1, "never two decodes at once");
    }

    #[test]
    fn a_take_that_cannot_get_its_turn_says_catching_up() {
        let queue = JobQueue::new(JOB_QUEUE_MAX_WAITING);
        let _running = queue
            .admit(JobKind::Dictation, Duration::from_secs(1))
            .expect("admitted");
        let started = Instant::now();
        let err = queue
            .admit(JobKind::Dictation, Duration::from_millis(40))
            .err()
            .expect("the engine is held");
        assert_eq!(err, CATCHING_UP);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(queue.waiting(), 0, "a job that gave up leaves the queue");
    }

    #[test]
    fn a_full_queue_fails_fast_with_a_plain_sentence() {
        let queue = Arc::new(JobQueue::new(1));
        let running = queue
            .admit(JobKind::Meeting, Duration::from_secs(5))
            .expect("admitted");
        let q = queue.clone();
        let waiter = std::thread::spawn(move || {
            q.admit(JobKind::Meeting, Duration::from_secs(5))
                .map(|t| t.kind())
        });
        wait_for_waiting(&queue, 1);
        let started = Instant::now();
        let err = queue
            .admit(JobKind::Dictation, Duration::from_secs(5))
            .err()
            .expect("the queue is full");
        assert_eq!(err, QUEUE_FULL);
        assert!(started.elapsed() < Duration::from_secs(1), "fails fast");
        drop(running);
        assert_eq!(waiter.join().expect("waiter"), Ok(JobKind::Meeting));
    }

    #[test]
    fn quit_closes_the_queue_and_turns_every_waiting_job_away() {
        let queue = Arc::new(JobQueue::new(JOB_QUEUE_MAX_WAITING));
        let running = queue
            .admit(JobKind::Meeting, Duration::from_secs(5))
            .expect("admitted");
        let q = queue.clone();
        let waiter = std::thread::spawn(move || {
            q.admit(JobKind::Dictation, Duration::from_secs(5))
                .map(|t| t.kind())
        });
        wait_for_waiting(&queue, 1);
        assert_eq!(queue.close(), 1);
        assert_eq!(
            waiter.join().expect("waiter"),
            Err(OWNER_QUITTING.to_string())
        );
        assert!(queue.is_closed());
        drop(running);
        assert_eq!(
            queue
                .admit(JobKind::Dictation, Duration::from_secs(1))
                .err()
                .as_deref(),
            Some(OWNER_QUITTING)
        );
    }

    #[test]
    fn as_job_files_the_kind_and_restores_it() {
        assert_eq!(current_job_kind(false), JobKind::Dictation);
        assert_eq!(current_job_kind(true), JobKind::Meeting);
        let inner = as_job(JobKind::Retry, || {
            let nested = as_job(JobKind::Recovery, || current_job_kind(true));
            (nested, current_job_kind(false))
        });
        assert_eq!(inner, (JobKind::Recovery, JobKind::Retry));
        assert_eq!(current_job_kind(false), JobKind::Dictation, "restored");
    }
}
