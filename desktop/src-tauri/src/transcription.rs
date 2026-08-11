//! Warm ASR engine lifecycle (YV31) — modeled on Handy's `TranscriptionManager`
//! (`~/oss/handy/src-tauri/src/managers/transcription.rs`).
//!
//! One loaded GGUF engine is kept warm behind `Arc<Mutex<Option<LoadedEngine>>>`
//! so repeat dictation reuses the session instead of paying the multi-second
//! model load every time, with three hard guarantees copied from Handy:
//!
//! * **Off-main loads** — `load_async` runs the blocking `Model::load_with` on a
//!   blocking-pool thread, so a 700 MB load never freezes the UI.
//! * **Panic containment** — both load and transcribe run inside `catch_unwind`;
//!   a panicking native engine is dropped rather than left in the slot, so the
//!   next use reloads cleanly instead of the app wedging.
//! * **Never hangs** — `transcribe` runs the engine on its own thread and waits
//!   with a hard timeout, returning `Err` (and abandoning the engine) if the
//!   native call never comes back.
//!
//! An idle watcher unloads the engine after [`IDLE_UNLOAD_AFTER`] of no use so a
//! background Yap doesn't hold ~1 GB of model resident all day.
//!
//! YV32 put this on the live dictation path and behind the headless
//! `--transcribe-file` CLI; YV34 deleted the Python sidecar, so this is now the
//! ONLY transcriber in the app. A few lifecycle helpers are still driven by the
//! model-management commands alone.
//!
//! YV80 made the FIRST load lazy (nothing is resident until a take arms it), so
//! two callers now race for that one load — the take's arm and the transcribe
//! that follows it. [`load`](TranscriptionManager::load) serialises them behind
//! `load_gate`, so the second caller waits for the first instead of building a
//! second copy of the same model.
#![allow(dead_code)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::Serialize;

use crate::asr_engine;

/// Unload a warm engine after this long without a transcription (Handy's
/// default model-unload timeout).
pub const IDLE_UNLOAD_AFTER: Duration = Duration::from_secs(15 * 60);
/// How often the idle watcher wakes up to check. YV81 raised it from 30s: it
/// resolves a FIFTEEN-minute window, so a minute of granularity is already an
/// order of magnitude finer than the thing it measures, and every extra wake is
/// paid for the whole life of the process.
const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(60);
/// Hard ceiling on ONE transcription. Way above any real clip (a 60 s take is
/// ~1 s on Metal) — this exists only so a wedged native call surfaces as an
/// error instead of freezing dictation forever.
pub const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(120);

/// What a caller gets when the engine is not in the slot — because nothing is
/// loaded, or because another caller has it leased out. A constant since YV93:
/// meeting ASR has to tell THIS apart from a real decode failure (it means
/// "come back at the next boundary", not "this chunk is unrecoverable"), and a
/// string compared against a literal in another module is a bug waiting for
/// someone to reword the message.
pub const NO_ENGINE_LOADED: &str = "no ASR model is loaded";

/// What a meeting chunk decode gets back when a dictation took the engine off
/// it mid-decode (YV93). NOT a decode failure: the chunk's audio is still on
/// disk, so meeting ASR re-decodes it after the handback instead of writing an
/// `asr_failed` hole into the transcript.
pub const PREEMPTED_FOR_DICTATION: &str = "meeting chunk preempted by a dictation";

/// The share of [`TRANSCRIBE_TIMEOUT`] one full-width meeting chunk is allowed
/// to spend, at a real-time factor of 1.0.
///
/// Half the budget: the engine is shared with dictation and the timeout also
/// has to cover a cold Metal warm-up. `meeting_asr::ChunkConfig::validate`
/// enforces it against the widest window the geometry can produce, and
/// [`ENGINE_HANDBACK_WAIT`] is derived from the same number below — which is
/// the point of it living here rather than beside the geometry it constrains.
/// The two constants disagreeing is precisely how a legal 37 s chunk could
/// decode for 60 s against a 5 s handback wait and destroy a dictation.
pub const WORST_CASE_RTF_BUDGET: f64 = 0.5;

/// The longest decode the chunk geometry is allowed to hand the engine, in
/// seconds of audio — and, at RTF 1.0, in seconds of wall clock. Anything
/// wider is refused at plan time by `ChunkConfig::validate`.
pub const MAX_SANCTIONED_CHUNK_DECODE_SECONDS: f64 =
    TRANSCRIBE_TIMEOUT.as_secs() as f64 * WORST_CASE_RTF_BUDGET;

/// How long a caller waits for the engine to come BACK from another caller
/// before giving up (YV93).
///
/// Before meetings there was only ever one caller, so an empty slot could only
/// mean "nothing is loaded" and failing instantly was right. Now a dictation can
/// arrive while a meeting chunk holds the engine, and meeting ASR's whole
/// preemption contract — stand down at the next chunk boundary and hand the
/// engine back (finding #2b) — is worth nothing if the dictation has already
/// given up by the time the handback happens. So a caller that finds the engine
/// LEASED OUT (or mid-load) waits for it; a caller that finds nothing loaded at
/// all still fails immediately.
///
/// **It is DERIVED, not chosen, and that is the whole fix.** At a chosen 5 s it
/// silently contradicted the geometry the same module sanctions: a full-width
/// 37 s chunk is legal at up to 60 s of decode, twelve times the wait, so every
/// decode between 5 s and 60 s — a thermally throttled machine, a cold Metal
/// warm-up, exactly the conditions a long meeting creates — turned "the
/// dictation waits one chunk" into `Err(NO_ENGINE_LOADED)` and a LOST take. The
/// preemption below normally makes the real wait a cancel latency rather than a
/// decode; this ceiling is what holds when the engine has no cancel hook at
/// all, and it can no longer disagree with `ChunkConfig::validate` because both
/// come off [`MAX_SANCTIONED_CHUNK_DECODE_SECONDS`].
pub const ENGINE_HANDBACK_WAIT: Duration =
    Duration::from_secs(MAX_SANCTIONED_CHUNK_DECODE_SECONDS as u64);
/// How often that wait re-checks the slot.
const ENGINE_HANDBACK_POLL: Duration = Duration::from_millis(5);

/// How often [`TranscriptionManager::drain_and_unload`] re-checks the lease.
const DRAIN_POLL: Duration = Duration::from_millis(10);

/// Asks an in-flight decode to stop (YV70). Cheap to clone and safe to hold
/// while the engine itself is leased out to a transcription thread, which is
/// the whole point: the exit drain has no other handle on a running decode.
pub type CancelHandle = Arc<dyn Fn() + Send + Sync>;

/// Anything that turns 16 kHz mono f32 samples into text. The real engine is
/// [`asr_engine::AsrEngine`]; the trait exists so the lifecycle above can be
/// tested without a multi-hundred-MB GGUF download.
pub trait Transcriber: Send + 'static {
    fn transcribe(
        &mut self,
        samples_16k_mono: &[f32],
        language: Option<&str>,
        bias_prompt: Option<&str>,
    ) -> Result<String, String>;

    /// The same decode, with the alignment kept (YV93 — meeting chunking).
    ///
    /// Defaulted rather than required: an engine that produces no timestamps
    /// (and every stub in these tests) is still a legal `Transcriber`, and the
    /// meeting seam merge already has to handle a timeless chunk — that is the
    /// text-LCS fallback path plan finding #11 insists on keeping.
    fn transcribe_timed(
        &mut self,
        samples_16k_mono: &[f32],
        language: Option<&str>,
        bias_prompt: Option<&str>,
    ) -> Result<asr_engine::TimedTranscript, String> {
        self.transcribe(samples_16k_mono, language, bias_prompt)
            .map(asr_engine::TimedTranscript::text_only)
    }

    /// A handle that ends an in-flight [`Transcriber::transcribe`] on this
    /// engine early (YV70). `None` for an engine with no cancel hook, in which
    /// case the exit drain can only wait the decode out.
    fn cancel_handle(&self) -> Option<CancelHandle> {
        None
    }

    /// Clear a cancellation left over from a PREVIOUS decode (YV93).
    ///
    /// The real engine's cancel flag is sticky (see
    /// [`asr_engine::reset_cancel`]), and since YV93 it is fired in the normal
    /// course of events — every time a dictation preempts a meeting chunk — not
    /// only on the way out. Without this the second preemption would find an
    /// engine that aborts every decode instantly.
    fn reset_cancel(&mut self) {}
}

impl Transcriber for asr_engine::AsrEngine {
    fn transcribe(
        &mut self,
        samples_16k_mono: &[f32],
        language: Option<&str>,
        bias_prompt: Option<&str>,
    ) -> Result<String, String> {
        asr_engine::transcribe(self, samples_16k_mono, language, bias_prompt)
    }

    fn transcribe_timed(
        &mut self,
        samples_16k_mono: &[f32],
        language: Option<&str>,
        bias_prompt: Option<&str>,
    ) -> Result<asr_engine::TimedTranscript, String> {
        asr_engine::transcribe_timed(self, samples_16k_mono, language, bias_prompt)
    }

    fn cancel_handle(&self) -> Option<CancelHandle> {
        let token = asr_engine::cancel_token(self);
        Some(Arc::new(move || token.cancel()))
    }

    fn reset_cancel(&mut self) {
        asr_engine::reset_cancel(self);
    }
}

/// One finished transcription as the dictation pipeline consumes it: the text
/// plus how it was produced. `backend` / `seconds` are persisted on the
/// transcript row and drive the latency logging (YV34 — this used to live in
/// the deleted `asr` sidecar client).
pub struct AsrOutput {
    pub text: String,
    pub backend: String,
    pub seconds: f64,
    /// YV40 latency span: model load into the warm engine, in ms — `0` whenever
    /// the model was already resident (the warm path). Since YV80 made the load
    /// lazy this is where the one-time first-take penalty shows up: what the
    /// take actually WAITED for, i.e. whatever was left of the load the arm
    /// started when the user released their key.
    pub load_ms: i64,
    /// YV40 latency span: the decode itself, in ms (excludes the load above).
    pub decode_ms: i64,
}

/// The warm engine plus the catalog id it was loaded from.
pub struct LoadedEngine {
    model_id: String,
    engine: Box<dyn Transcriber>,
}

/// Loads a model file into a ready engine. Swapped out in tests.
type EngineLoader = Arc<dyn Fn(&Path) -> Result<Box<dyn Transcriber>, String> + Send + Sync>;

/// Snapshot of the engine lifecycle for the UI (`engine_status` command).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    /// A model is resident — either sitting in the slot or leased to an
    /// in-flight transcription.
    pub loaded: bool,
    /// A load is running right now (off-main).
    pub loading: bool,
    /// A transcription is running right now.
    pub transcribing: bool,
    /// Catalog id of the resident model, if any.
    pub model_id: Option<String>,
    /// Seconds since the last load/transcription — what the idle watcher counts.
    pub idle_seconds: u64,
    /// Seconds of idleness after which the engine is unloaded.
    pub idle_unload_seconds: u64,
    /// YV75 — the polish sidecar's lifecycle (`not-installed` | `starting` |
    /// `ready` | `failed`, with a reason on a failure). It rides this snapshot
    /// because Diagnostics asks one question — "what is actually running?" —
    /// and the answer has always included the second process.
    pub polish_sidecar: crate::polish::SidecarStatus,
}

/// What the exit drain found (YV70), so the caller can log the shutdown it
/// actually got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainOutcome {
    /// Nothing was in flight — the engine was freed straight from the slot.
    Idle,
    /// A transcription had the engine; it came back inside the budget and was
    /// freed.
    Drained,
    /// The transcription never gave the engine back inside the budget. It is
    /// still alive on the worker thread — the quit proceeds regardless.
    TimedOut,
}

/// The decode holding the engine right now.
struct InFlight {
    /// Its cancel hook, if the engine has one (YV70).
    cancel: Option<CancelHandle>,
    /// True for a meeting chunk: re-decodable from disk, so a dictation may
    /// take the engine off it. False for a dictation, whose audio exists
    /// nowhere else — an interactive take is never thrown away.
    preemptible: bool,
    /// Set once a dictation has asked this decode to stop. Read back by the
    /// lease so the result is discarded rather than reported as a decode
    /// failure.
    preempted: bool,
}

/// Shared guts. Held by an `Arc` so the idle watcher can keep a `Weak` and exit
/// on its own once the last manager handle is dropped (no shutdown flag, and no
/// leaked thread in tests).
struct Inner {
    engine: Mutex<Option<LoadedEngine>>,
    loader: EngineLoader,
    /// Bumped on every load/unload. A transcription that started before a bump
    /// must not put its now-stale engine back.
    generation: AtomicU64,
    /// Nonzero while an engine is leased out to a transcription thread. Dropped
    /// back to zero only once that engine is BACK (in the slot, or dropped), so
    /// a zero lease means nothing is holding a live Metal device (YV70).
    leases: AtomicU64,
    /// YV70 — set once the app is on its way out. Nothing loads after this, and
    /// a transcription that finishes now drops its engine instead of putting a
    /// live device back into the slot the exit drain has already emptied.
    exiting: AtomicBool,
    /// YV70/YV93 — the decode currently holding the engine: its cancel hook (the
    /// engine itself is leased out, so this is the only handle on it), whether
    /// it is the kind of work that may be thrown away, and whether it already
    /// has been. ONE mutex for all three so a dictation arriving at any instant
    /// either sees the meeting chunk and cancels THAT decode, or misses it and
    /// finds the engine already on its way back — and never cancels the decode
    /// that took its place.
    in_flight: Mutex<Option<InFlight>>,
    /// YV80 — ONE load at a time. The first take now arms the engine at press
    /// time and the transcribe that follows calls `load` again; the slot is
    /// still empty while the first load runs, so without this the second caller
    /// would start a SECOND multi-hundred-MB load of the same model. Held across
    /// the whole load on purpose: the second caller waits for the first, then
    /// finds the engine resident and returns.
    load_gate: Mutex<()>,
    /// Set while a load is building an engine — claimed and cleared around the
    /// device's whole lifetime in `load` (it stays set until the engine is in
    /// the slot or dropped), so the exit drain can wait on it like a lease.
    loading: AtomicBool,
    /// YV93 — how many INTERACTIVE decodes (dictation, `--transcribe-file`) are
    /// waiting for or holding the one warm engine right now. Meeting ASR reads
    /// it between chunks and gets out of the way: the single engine is what
    /// makes a long meeting decode able to starve the sub-second dictation path
    /// (plan finding #2b), and this counter is how the meeting side can see the
    /// dictation coming without the engine mutex having any fairness at all.
    /// Raised BEFORE the engine is taken, dropped only once it is back.
    interactive_waiting: AtomicU64,
    last_activity_ms: AtomicU64,
    idle_timeout: Duration,
    idle_check_interval: Duration,
    transcribe_timeout: Duration,
}

/// YV91 (finding #27): how many times a model load has been ASKED for, and how
/// many times the idle sweeper's clock has been pushed out. Process-global
/// counters rather than per-manager state, because the property under test is
/// "the meeting capture path never does either of these", and the capture path
/// does not hold a manager at all.
static MODEL_LOAD_CALLS: AtomicU64 = AtomicU64::new(0);
static ENGINE_TOUCHES: AtomicU64 = AtomicU64::new(0);

/// Times `TranscriptionManager::load` has been called in this process.
pub fn model_load_calls() -> u64 {
    MODEL_LOAD_CALLS.load(Ordering::Relaxed)
}

/// Times the ASR idle sweeper's deadline has been pushed out
/// (`TranscriptionManager::touch`).
pub fn engine_touches() -> u64 {
    ENGINE_TOUCHES.load(Ordering::Relaxed)
}

/// Owns the warm engine. Cheap to clone (all handles share one `Inner`).
#[derive(Clone)]
pub struct TranscriptionManager {
    inner: Arc<Inner>,
}

impl TranscriptionManager {
    /// Production manager: real transcribe-cpp loads, Handy's idle timeout.
    pub fn new() -> Self {
        Self::with_loader(
            Arc::new(|path: &Path| {
                asr_engine::load(path).map(|e| Box::new(e) as Box<dyn Transcriber>)
            }),
            IDLE_UNLOAD_AFTER,
            IDLE_CHECK_INTERVAL,
            TRANSCRIBE_TIMEOUT,
        )
    }

    /// Manager with an injected loader + timings — the seam the lifecycle tests
    /// use to exercise load/idle-unload/timeout without a real model.
    pub fn with_loader(
        loader: EngineLoader,
        idle_timeout: Duration,
        idle_check_interval: Duration,
        transcribe_timeout: Duration,
    ) -> Self {
        let manager = Self {
            inner: Arc::new(Inner {
                engine: Mutex::new(None),
                loader,
                generation: AtomicU64::new(0),
                leases: AtomicU64::new(0),
                exiting: AtomicBool::new(false),
                in_flight: Mutex::new(None),
                load_gate: Mutex::new(()),
                loading: AtomicBool::new(false),
                interactive_waiting: AtomicU64::new(0),
                last_activity_ms: AtomicU64::new(now_ms()),
                idle_timeout,
                idle_check_interval,
                transcribe_timeout,
            }),
        };
        manager.spawn_idle_watcher();
        manager
    }

    /// Watcher thread: every `idle_check_interval`, unload an engine that has
    /// gone `idle_timeout` without use. It holds only a `Weak`, so dropping the
    /// last manager handle ends the thread.
    fn spawn_idle_watcher(&self) {
        let weak = Arc::downgrade(&self.inner);
        let interval = self.inner.idle_check_interval;
        std::thread::spawn(move || loop {
            std::thread::sleep(interval);
            let Some(inner) = Weak::upgrade(&weak) else {
                log::debug!("ASR idle watcher exiting — manager dropped");
                return;
            };
            let manager = TranscriptionManager { inner };
            manager.unload_if_idle();
        });
    }

    fn unload_if_idle(&self) {
        // An in-flight transcription counts as activity, not idleness.
        if self.inner.leases.load(Ordering::Acquire) != 0
            || self.inner.loading.load(Ordering::Acquire)
        {
            self.touch();
            return;
        }
        if self.inner.engine.lock().is_none() {
            return;
        }
        let idle = Duration::from_millis(now_ms().saturating_sub(self.idle_ms_raw()));
        if idle >= self.inner.idle_timeout {
            log::info!("unloading ASR model after {}s idle", idle.as_secs());
            self.unload();
        }
    }

    fn idle_ms_raw(&self) -> u64 {
        self.inner.last_activity_ms.load(Ordering::Relaxed)
    }

    /// Reset the idle timer to now.
    pub fn touch(&self) {
        ENGINE_TOUCHES.fetch_add(1, Ordering::Relaxed);
        self.inner
            .last_activity_ms
            .store(now_ms(), Ordering::Relaxed);
    }

    /// True while a model is resident (in the slot OR leased to a transcription).
    pub fn is_loaded(&self) -> bool {
        self.inner.engine.lock().is_some() || self.inner.leases.load(Ordering::Acquire) != 0
    }

    /// Catalog id of the resident model, if any.
    pub fn loaded_model(&self) -> Option<String> {
        self.inner
            .engine
            .lock()
            .as_ref()
            .map(|e| e.model_id.clone())
    }

    pub fn status(&self) -> EngineStatus {
        let slot = self.inner.engine.lock();
        let leases = self.inner.leases.load(Ordering::Acquire);
        EngineStatus {
            loaded: slot.is_some() || leases != 0,
            loading: self.inner.loading.load(Ordering::Acquire),
            transcribing: leases != 0,
            model_id: slot.as_ref().map(|e| e.model_id.clone()),
            idle_seconds: now_ms().saturating_sub(self.idle_ms_raw()) / 1000,
            idle_unload_seconds: self.inner.idle_timeout.as_secs(),
            polish_sidecar: crate::polish::sidecar_status(),
        }
    }

    /// Drop the warm engine (frees the model's memory). Safe to call any time —
    /// a transcription already in flight keeps running on its leased engine and
    /// simply won't put it back.
    pub fn unload(&self) {
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
        let previous = self.inner.engine.lock().take();
        if let Some(engine) = previous {
            log::info!("ASR model '{}' unloaded", engine.model_id);
            drop(engine);
        }
    }

    /// Exit drain (YV70): get the engine back, then free it — the ONE unload
    /// that must not no-op.
    ///
    /// YV69 unloads the engine before `exit()` so ggml's static destructor has
    /// no Metal device left to free (it aborts if it does). Plain [`unload`] is
    /// enough only when the engine is sitting in the slot: during a take it is
    /// LEASED OUT to the transcription thread, the slot is empty, and the
    /// unload frees nothing — quitting mid-transcription still reached `exit()`
    /// with a live device. So: mark the manager as exiting (a returning engine
    /// is dropped from here on, never put back), ask the in-flight decode to
    /// stop, and wait up to `wait` for the lease to come home before unloading.
    ///
    /// A LOAD in flight counts as in-flight too: the loader runs for seconds and
    /// ends by making a fresh Metal device, so a drain that ignored it would
    /// unload an empty slot and let that device be born into it afterwards —
    /// the same crash, one window over. `load` refuses to store once `exiting`
    /// is set, and this waits for it to finish refusing.
    ///
    /// Bounded on purpose. A decode that ignores the cancel is rarer than the
    /// crash this prevents, so a timed-out drain logs at WARN and lets the quit
    /// proceed rather than hanging the user's Cmd-Q.
    ///
    /// [`unload`]: Self::unload
    pub fn drain_and_unload(&self, wait: Duration) -> DrainOutcome {
        self.inner.exiting.store(true, Ordering::Release);
        if !self.engine_in_flight() {
            self.unload();
            return DrainOutcome::Idle;
        }
        let cancel = self
            .inner
            .in_flight
            .lock()
            .as_ref()
            .and_then(|f| f.cancel.clone());
        if let Some(cancel) = cancel {
            log::info!("exit: cancelling the in-flight transcription");
            cancel();
        }
        let deadline = Instant::now() + wait;
        while self.engine_in_flight() {
            if Instant::now() >= deadline {
                log::warn!(
                    "exit: an ASR engine is still in flight after {}ms — quitting anyway",
                    wait.as_millis()
                );
                // Belt and braces: the slot should be empty (the lease is still
                // out), but if anything IS resident it must not survive to exit.
                self.unload();
                return DrainOutcome::TimedOut;
            }
            std::thread::sleep(DRAIN_POLL);
        }
        self.unload();
        DrainOutcome::Drained
    }

    /// YV70: is a live device out of the slot right now — leased to a decode, or
    /// being built by an in-flight [`load`](Self::load)? Read under the slot
    /// lock, which is where `load` claims `loading`, so the drain cannot slip
    /// between that claim and the flag.
    fn engine_in_flight(&self) -> bool {
        let _slot = self.inner.engine.lock();
        self.inner.leases.load(Ordering::Acquire) != 0 || self.inner.loading.load(Ordering::Acquire)
    }

    /// True while a load is building an engine right now. Read straight off the
    /// atomic (no slot lock), because YV80 asks it on every status emit so the
    /// UI can say "Preparing your speech engine…" instead of nothing.
    pub fn is_loading(&self) -> bool {
        self.inner.loading.load(Ordering::Acquire)
    }

    /// Blocking load — this is the multi-second call, so run it off the main
    /// thread (see [`load_async`](Self::load_async)). A model already loaded
    /// under the same id is kept as-is.
    ///
    /// Idempotent AND coalesced (YV80): concurrent calls for the same model
    /// produce exactly ONE load — the losers block on `load_gate` and return
    /// once the winner's engine is in the slot.
    pub fn load(&self, model_id: &str, model_path: &Path) -> Result<(), String> {
        // YV91 finding #27: nothing in a meeting CAPTURE may bring an inference
        // model resident on a fanless 8 GB M1 Air, and nothing in it may push
        // the idle sweeper's deadline out. Counting the two entry points is how
        // `tests/meeting_no_model_resident.rs` proves that from outside.
        MODEL_LOAD_CALLS.fetch_add(1, Ordering::Relaxed);
        // YV70: never make a new Metal device after the exit drain has run —
        // that would put back exactly what the drain freed.
        if self.inner.exiting.load(Ordering::Acquire) {
            return Err("the app is exiting".into());
        }
        if self.loaded_model().as_deref() == Some(model_id) {
            self.touch();
            return Ok(());
        }
        if !model_path.is_file() {
            return Err(format!(
                "model '{model_id}' is not downloaded ({} missing)",
                model_path.display()
            ));
        }
        // YV80: from here on exactly one thread is loading. Everything above is
        // a cheap read, so the warm path never touches this gate.
        let _gate = self.inner.load_gate.lock();
        // Whoever held the gate before us may have loaded the very model we
        // came for — the arm-then-transcribe pair of a first dictation.
        if self.loaded_model().as_deref() == Some(model_id) {
            self.touch();
            return Ok(());
        }
        // YV70: claim `loading` under the slot lock, re-checking `exiting` while
        // holding it. The drain sets `exiting` before it reads the same state
        // under the same lock, so exactly one of the two wins: either the drain
        // waits this load out, or this load is refused before it starts.
        {
            let _slot = self.inner.engine.lock();
            if self.inner.exiting.load(Ordering::Acquire) {
                return Err("the app is exiting".into());
            }
            self.inner.loading.store(true, Ordering::Release);
        }
        // A panicking native load must not take the app with it, and must not
        // leave a half-built engine in the slot.
        let loaded = catch_unwind(AssertUnwindSafe(|| (self.inner.loader)(model_path)))
            .unwrap_or_else(|p| Err(format!("ASR model load panicked: {}", panic_message(&p))));
        let engine = match loaded {
            Ok(engine) => engine,
            Err(e) => {
                self.inner.loading.store(false, Ordering::Release);
                return Err(e);
            }
        };
        // YV70: the loader above runs for SECONDS — the exit drain can start
        // inside that window, and it ends by making a fresh Metal device. Check
        // `exiting` again before storing, under the slot lock, and drop the
        // device rather than park it in the slot the drain just emptied.
        // `loading` stays set until it is gone, so the drain's wait covers the
        // drop too.
        {
            let mut slot = self.inner.engine.lock();
            if self.inner.exiting.load(Ordering::Acquire) {
                drop(slot);
                drop(engine);
                self.inner.loading.store(false, Ordering::Release);
                log::info!("dropping freshly loaded ASR engine '{model_id}' — the app is exiting");
                return Err("the app is exiting".into());
            }
            self.inner.generation.fetch_add(1, Ordering::AcqRel);
            *slot = Some(LoadedEngine {
                model_id: model_id.to_string(),
                engine,
            });
        }
        self.inner.loading.store(false, Ordering::Release);
        self.touch();
        log::info!("ASR model '{model_id}' loaded and warm");
        Ok(())
    }

    /// Load off the main thread. Tauri commands call this so a cold load never
    /// blocks the UI thread or the async runtime.
    pub async fn load_async(&self, model_id: String, model_path: PathBuf) -> Result<(), String> {
        let manager = self.clone();
        tauri::async_runtime::spawn_blocking(move || manager.load(&model_id, &model_path))
            .await
            .map_err(|e| format!("ASR model load task failed: {e}"))?
    }

    /// Transcribe 16 kHz mono f32 samples with the warm engine. `language` is
    /// the user's spoken-language ISO code, or `None` to autodetect;
    /// `bias_prompt` is the YV47 dictionary bias (see
    /// [`asr_engine::build_bias_prompt`]), or `None` for an unbiased decode.
    ///
    /// The engine is taken OUT of the mutex for the duration (no lock is held
    /// across the native call) and runs on its own thread so the wait can be
    /// bounded: on timeout this returns `Err` and abandons the engine — the
    /// worker thread owns it and drops it whenever it finally returns, so the
    /// next transcription simply reloads. It can never hang the caller.
    pub fn transcribe(
        &self,
        samples_16k_mono: Vec<f32>,
        language: Option<String>,
        bias_prompt: Option<String>,
    ) -> Result<String, String> {
        if samples_16k_mono.is_empty() {
            self.touch();
            return Ok(String::new());
        }
        // YV93: an INTERACTIVE decode. The claim is raised here — before the
        // engine is taken — so meeting ASR sees a dictation that is still only
        // waiting, which is the only moment yielding to it is worth anything.
        let _claim = self.claim_interactive();
        // NOT preemptible: this take exists nowhere but in the samples above.
        self.leased(false, move |engine| {
            engine.transcribe(
                &samples_16k_mono,
                language.as_deref(),
                bias_prompt.as_deref(),
            )
        })
    }

    /// Transcribe with the alignment KEPT (YV93) — what meeting chunk ASR runs.
    ///
    /// Same lifecycle as [`transcribe`](Self::transcribe) (off-slot engine,
    /// bounded wait, panic containment) with two deliberate differences:
    ///
    /// * it does NOT raise the interactive claim. A meeting is the thing that
    ///   yields; counting it as a waiter would make it yield to itself.
    /// * it IS preemptible. Yielding at chunk boundaries bounds a dictation's
    ///   wait by one chunk decode, and one chunk decode is a number this module
    ///   sanctions at up to [`MAX_SANCTIONED_CHUNK_DECODE_SECONDS`] — a minute
    ///   of a user staring at a dictation that has not come back. So a
    ///   dictation raising its claim cancels this decode outright and gets
    ///   [`PREEMPTED_FOR_DICTATION`] back to the meeting driver, which
    ///   re-decodes the chunk from disk afterwards. The asymmetry is the whole
    ///   argument: a chunk is re-readable, a take is not.
    pub fn transcribe_timed(
        &self,
        samples_16k_mono: Vec<f32>,
        language: Option<String>,
        bias_prompt: Option<String>,
    ) -> Result<asr_engine::TimedTranscript, String> {
        if samples_16k_mono.is_empty() {
            self.touch();
            return Ok(asr_engine::TimedTranscript::text_only(String::new()));
        }
        self.leased(true, move |engine| {
            engine.transcribe_timed(
                &samples_16k_mono,
                language.as_deref(),
                bias_prompt.as_deref(),
            )
        })
    }

    /// True while an interactive decode (a dictation, the headless CLI) is
    /// waiting for or holding the one warm engine (YV93). Meeting ASR checks
    /// this at every chunk boundary and stands down until it clears.
    pub fn interactive_pending(&self) -> bool {
        self.inner.interactive_waiting.load(Ordering::Acquire) != 0
    }

    fn claim_interactive(&self) -> InteractiveClaim {
        self.inner
            .interactive_waiting
            .fetch_add(1, Ordering::AcqRel);
        // The claim is raised BEFORE the engine is taken, so this runs while the
        // meeting chunk still holds it — which is the only moment cancelling it
        // is worth anything.
        self.preempt_in_flight();
        InteractiveClaim {
            inner: self.inner.clone(),
        }
    }

    /// Take the engine off an in-flight PREEMPTIBLE decode for an interactive
    /// one (YV93).
    ///
    /// Meeting ASR already stands down at chunk boundaries; this is what
    /// happens when the dictation arrives in the middle of a chunk instead. The
    /// decode is cancelled (YV70's hook — the same one the exit drain uses), its
    /// result is discarded by the lease, and the chunk is re-decoded from disk
    /// after the handback. Without it, a dictation's wait is a whole chunk
    /// decode, and `ChunkConfig::validate` sanctions chunk decodes up to
    /// [`MAX_SANCTIONED_CHUNK_DECODE_SECONDS`].
    ///
    /// No-op when the in-flight decode has no cancel hook: marking it preempted
    /// would throw away a decode that is going to run to completion anyway.
    /// [`ENGINE_HANDBACK_WAIT`] is what covers the dictation in that case.
    fn preempt_in_flight(&self) {
        let cancel = {
            let mut slot = self.inner.in_flight.lock();
            match slot.as_mut() {
                Some(f) if f.preemptible && !f.preempted && f.cancel.is_some() => {
                    f.preempted = true;
                    f.cancel.clone()
                }
                _ => None,
            }
        };
        if let Some(cancel) = cancel {
            log::info!("a dictation is waiting — cancelling the in-flight meeting chunk decode");
            cancel();
        }
    }

    /// Run one job against the warm engine: take it out of the slot, run it on
    /// its own thread with a hard timeout, put it back (or drop it).
    ///
    /// The engine is taken OUT of the mutex for the duration (no lock is held
    /// across the native call) and runs on its own thread so the wait can be
    /// bounded: on timeout this returns `Err` and abandons the engine — the
    /// worker thread owns it and drops it whenever it finally returns, so the
    /// next transcription simply reloads. It can never hang the caller.
    ///
    /// `preemptible` marks work a waiting dictation may cancel outright — a
    /// meeting chunk, which is re-decodable from disk. An interactive decode is
    /// never preemptible: its audio exists nowhere but in the buffer it was
    /// handed.
    fn leased<R: Send + 'static>(
        &self,
        preemptible: bool,
        job: impl FnOnce(&mut dyn Transcriber) -> Result<R, String> + Send + 'static,
    ) -> Result<R, String> {
        self.touch();
        let generation = self.inner.generation.load(Ordering::Acquire);
        let mut engine = self.take_engine()?;
        self.inner.leases.fetch_add(1, Ordering::AcqRel);
        // The cancel flag is sticky, so it is cleared HERE — with the engine
        // exclusively in hand and before anything can see it as in-flight —
        // rather than inside the decode, where a cancel arriving in the gap
        // between publishing and running would be reset away.
        engine.engine.reset_cancel();
        // YV70: publish this run's cancel hook while it is in flight, so the
        // exit drain can end the decode instead of only waiting on it — and,
        // since YV93, so a waiting dictation can end a meeting chunk with it.
        *self.inner.in_flight.lock() = Some(InFlight {
            cancel: engine.engine.cancel_handle(),
            preemptible,
            preempted: false,
        });
        // A dictation that raised its claim between `take_engine` and the line
        // above found no in-flight record and cancelled nothing. Re-ask now that
        // there is one: the claim is still up (it is dropped only once the
        // dictation is through), so this closes the window rather than papering
        // over it.
        if preemptible && self.interactive_pending() {
            self.preempt_in_flight();
        }

        let (tx, rx) = mpsc::channel::<(Option<LoadedEngine>, Result<R, String>)>();
        std::thread::spawn(move || {
            let mut engine = engine;
            // Panic containment: on unwind the engine is dropped instead of
            // returned, so a poisoned native session is never reused.
            let sent = match catch_unwind(AssertUnwindSafe(|| job(&mut *engine.engine))) {
                Ok(result) => (Some(engine), result),
                Err(p) => (
                    None,
                    Err(format!("ASR engine panicked: {}", panic_message(&p))),
                ),
            };
            // A send failure means the caller timed out; `sent` (and with it the
            // engine) drops right here.
            let _ = tx.send(sent);
        });

        let outcome = rx.recv_timeout(self.inner.transcribe_timeout);
        let result = match outcome {
            Ok((engine, result)) => {
                if let Some(engine) = engine {
                    self.return_engine(engine, generation);
                }
                result
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
                "transcription timed out after {}s",
                self.inner.transcribe_timeout.as_secs()
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("transcription worker died unexpectedly".into())
            }
        };
        let preempted = {
            let mut slot = self.inner.in_flight.lock();
            let preempted = slot.as_ref().is_some_and(|f| f.preempted);
            *slot = None;
            preempted
        };
        // YV70: the lease is released only AFTER the engine has been put back
        // (or dropped). It used to drop first, so the exit drain could see a
        // zero lease and unload while `return_engine` was still on its way to
        // refill the slot — the live Metal device the drain exists to prevent.
        self.inner.leases.fetch_sub(1, Ordering::AcqRel);
        self.touch();
        if preempted {
            // Discarded whatever came back, including an `Ok`. A cancelled
            // decode that still returns text has been cut short somewhere the
            // caller cannot see, and half a chunk silently spliced into a
            // meeting is worse than the second of Metal time it costs to decode
            // the chunk again from disk.
            return Err(PREEMPTED_FOR_DICTATION.into());
        }
        result
    }

    /// Take the engine out of the slot, waiting out another caller who has it.
    ///
    /// "Empty slot" stopped meaning one thing when meetings arrived: it is
    /// either *nothing is loaded* (fail now — reloading is the caller's job) or
    /// *someone else is using it* (wait — they are on their way back, and with
    /// meeting ASR yielding at chunk boundaries that wait is one chunk long).
    /// The two are told apart by the lease/loading counters, not by guessing.
    fn take_engine(&self) -> Result<LoadedEngine, String> {
        let started = Instant::now();
        loop {
            if let Some(engine) = self.inner.engine.lock().take() {
                return Ok(engine);
            }
            let busy = self.inner.leases.load(Ordering::Acquire) != 0
                || self.inner.loading.load(Ordering::Acquire);
            if !busy || started.elapsed() >= ENGINE_HANDBACK_WAIT {
                return Err(NO_ENGINE_LOADED.into());
            }
            std::thread::sleep(ENGINE_HANDBACK_POLL);
        }
    }

    /// Put a borrowed engine back — unless the app is exiting, or the model was
    /// unloaded or swapped while it was out, in which case it is dropped.
    fn return_engine(&self, engine: LoadedEngine, generation: u64) {
        if self.inner.exiting.load(Ordering::Acquire) {
            log::info!(
                "dropping ASR engine '{}' — the app is exiting",
                engine.model_id
            );
            return;
        }
        if self.inner.generation.load(Ordering::Acquire) != generation {
            log::debug!(
                "dropping stale ASR engine '{}' (model changed during transcription)",
                engine.model_id
            );
            return;
        }
        let mut slot = self.inner.engine.lock();
        if slot.is_none() {
            *slot = Some(engine);
        }
    }
}

/// Live for exactly as long as one interactive decode is waiting for or holding
/// the engine (YV93). RAII rather than a manual decrement so an early `return`
/// on the error paths above can never leave meeting ASR yielding forever to a
/// dictation that is already over.
struct InteractiveClaim {
    inner: Arc<Inner>,
}

impl Drop for InteractiveClaim {
    fn drop(&mut self) {
        self.inner
            .interactive_waiting
            .fetch_sub(1, Ordering::AcqRel);
    }
}

impl Default for TranscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Best-effort text of a `catch_unwind` payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests (stub engine — no model file download, no network)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-in for a loaded GGUF session: returns canned text, optionally
    /// after a delay (timeout test) or by panicking (containment test).
    struct StubEngine {
        text: String,
        delay: Duration,
        panics: bool,
        /// Language hint the manager handed to the engine on the last run.
        seen_language: Arc<Mutex<Option<String>>>,
        /// YV47 dictionary bias prompt handed to the engine on the last run.
        seen_prompt: Arc<Mutex<Option<String>>>,
    }

    impl Transcriber for StubEngine {
        fn transcribe(
            &mut self,
            _samples: &[f32],
            language: Option<&str>,
            bias_prompt: Option<&str>,
        ) -> Result<String, String> {
            *self.seen_language.lock() = language.map(str::to_string);
            *self.seen_prompt.lock() = bias_prompt.map(str::to_string);
            if self.panics {
                panic!("stub engine exploded");
            }
            std::thread::sleep(self.delay);
            Ok(self.text.clone())
        }
    }

    /// A real (empty) file on disk so the manager's "is it downloaded?" gate
    /// passes without a 700 MB download.
    fn stub_model_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-yv31-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join(name);
        std::fs::write(&path, b"stub gguf").expect("write stub model");
        path
    }

    fn stub_loader(text: &'static str, delay: Duration, panics: bool) -> EngineLoader {
        stub_loader_recording(
            text,
            delay,
            panics,
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
        )
    }

    /// Same stub, but the caller keeps the handles the engine records its
    /// language hint and YV47 bias prompt into.
    fn stub_loader_recording(
        text: &'static str,
        delay: Duration,
        panics: bool,
        seen_language: Arc<Mutex<Option<String>>>,
        seen_prompt: Arc<Mutex<Option<String>>>,
    ) -> EngineLoader {
        Arc::new(move |_path: &Path| {
            Ok(Box::new(StubEngine {
                text: text.to_string(),
                delay,
                panics,
                seen_language: seen_language.clone(),
                seen_prompt: seen_prompt.clone(),
            }) as Box<dyn Transcriber>)
        })
    }

    /// A stub loader that counts how many times it actually built an engine —
    /// the only way to see YV80's "load once, then stay warm" from outside.
    fn counting_loader(loads: Arc<AtomicU64>, load_time: Duration) -> EngineLoader {
        Arc::new(move |_path: &Path| {
            std::thread::sleep(load_time);
            loads.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(StubEngine {
                text: "lazy".to_string(),
                delay: Duration::ZERO,
                panics: false,
                seen_language: Arc::new(Mutex::new(None)),
                seen_prompt: Arc::new(Mutex::new(None)),
            }) as Box<dyn Transcriber>)
        })
    }

    /// A loader that takes its time, like the real multi-second GGUF load — the
    /// seam the YV70 drain-vs-load tests need to catch a load in flight.
    fn slow_loader(load_time: Duration) -> EngineLoader {
        Arc::new(move |_path: &Path| {
            std::thread::sleep(load_time);
            Ok(Box::new(StubEngine {
                text: "born during the drain".to_string(),
                delay: Duration::ZERO,
                panics: false,
                seen_language: Arc::new(Mutex::new(None)),
                seen_prompt: Arc::new(Mutex::new(None)),
            }) as Box<dyn Transcriber>)
        })
    }

    /// Starts a load off-thread and returns once it is really inside the loader.
    fn load_in_flight(
        m: &TranscriptionManager,
        path: &Path,
    ) -> std::thread::JoinHandle<Result<(), String>> {
        let handle = {
            let m = m.clone();
            let path = path.to_path_buf();
            std::thread::spawn(move || m.load("stub/model", &path))
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !m.status().loading && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            m.status().loading,
            "the load must be in flight before the drain runs"
        );
        handle
    }

    fn manager(loader: EngineLoader, idle: Duration, transcribe: Duration) -> TranscriptionManager {
        TranscriptionManager::with_loader(loader, idle, Duration::from_millis(10), transcribe)
    }

    #[test]
    fn state_transitions_unloaded_to_loaded_to_idle_unloaded() {
        let path = stub_model_file("stub.gguf");
        let m = manager(
            stub_loader("hello from the stub", Duration::ZERO, false),
            Duration::from_millis(80),
            Duration::from_secs(5),
        );

        // Unloaded: nothing resident, and transcribing says so instead of hanging.
        assert!(!m.is_loaded());
        assert!(m.loaded_model().is_none());
        assert_eq!(
            m.transcribe(vec![0.0; 16], None, None).unwrap_err(),
            "no ASR model is loaded"
        );
        let s = m.status();
        assert!(!s.loaded && !s.loading && !s.transcribing);
        assert_eq!(s.idle_unload_seconds, 0); // 80 ms test timeout

        // Loaded (off-main, through the same spawn_blocking path production uses).
        tauri::async_runtime::block_on(m.load_async("stub/model".into(), path.clone()))
            .expect("load");
        assert!(m.is_loaded());
        assert_eq!(m.loaded_model().as_deref(), Some("stub/model"));
        assert_eq!(
            m.transcribe(vec![0.1; 16], None, None).unwrap(),
            "hello from the stub"
        );
        // Still warm after use — the engine is returned to the slot.
        assert!(m.is_loaded());

        // Idle-unloaded: the watcher drops it once the idle window passes.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while m.is_loaded() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!m.is_loaded(), "idle watcher must unload the engine");
        assert!(m.status().model_id.is_none());

        // And it reloads on demand after an idle unload.
        m.load("stub/model", &path).expect("reload");
        assert!(m.is_loaded());
        m.unload();
        assert!(!m.is_loaded());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn load_rejects_missing_model_file_and_stays_unloaded() {
        let m = manager(
            stub_loader("unused", Duration::ZERO, false),
            Duration::from_secs(60),
            Duration::from_secs(5),
        );
        let err = m
            .load("stub/model", Path::new("/definitely/not/here.gguf"))
            .unwrap_err();
        assert!(err.contains("not downloaded"), "unexpected error: {err}");
        assert!(!m.is_loaded());
        assert!(!m.status().loading, "loading flag must be cleared");
    }

    #[test]
    fn load_panic_is_contained_and_leaves_manager_unloaded() {
        let path = stub_model_file("stub.gguf");
        let loader: EngineLoader = Arc::new(|_p: &Path| panic!("native load exploded"));
        let m = manager(loader, Duration::from_secs(60), Duration::from_secs(5));

        let err = m.load("stub/model", &path).unwrap_err();
        assert!(err.contains("load panicked"), "unexpected error: {err}");
        assert!(!m.is_loaded());
        assert!(!m.status().loading);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn transcribe_panic_is_contained_and_drops_the_engine() {
        let path = stub_model_file("stub.gguf");
        let m = manager(
            stub_loader("never returned", Duration::ZERO, true),
            Duration::from_secs(60),
            Duration::from_secs(5),
        );
        m.load("stub/model", &path).expect("load");

        let err = m.transcribe(vec![0.2; 16], None, None).unwrap_err();
        assert!(err.contains("panicked"), "unexpected error: {err}");
        // A panicked engine is never put back — the next use reloads.
        assert!(!m.is_loaded());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn transcribe_times_out_instead_of_hanging() {
        let path = stub_model_file("stub.gguf");
        let m = manager(
            stub_loader("too late", Duration::from_secs(30), false),
            Duration::from_secs(60),
            Duration::from_millis(120),
        );
        m.load("stub/model", &path).expect("load");

        let started = std::time::Instant::now();
        let err = m.transcribe(vec![0.3; 16], None, None).unwrap_err();
        assert!(err.contains("timed out"), "unexpected error: {err}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "transcribe must return on its own timeout, took {:?}",
            started.elapsed()
        );
        // The abandoned engine stays with the worker thread; nothing is resident.
        assert!(!m.is_loaded());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn empty_audio_is_a_no_op_and_use_keeps_the_engine_warm() {
        let path = stub_model_file("stub.gguf");
        let m = manager(
            stub_loader("warm", Duration::ZERO, false),
            Duration::from_millis(200),
            Duration::from_secs(5),
        );
        m.load("stub/model", &path).expect("load");
        assert_eq!(m.transcribe(Vec::new(), None, None).unwrap(), "");

        // Using it across the idle window keeps it resident (each use touches
        // the idle timer), unlike the idle-unload test above.
        for _ in 0..6 {
            std::thread::sleep(Duration::from_millis(50));
            assert_eq!(m.transcribe(vec![0.4; 8], None, None).unwrap(), "warm");
        }
        assert!(m.is_loaded(), "an actively used engine must stay warm");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// YV70 (review): the `exiting` guard was a check-then-act with a
    /// multi-second gap. `load` tested the flag on entry, spent seconds in the
    /// loader, then stored unconditionally — so a load already in flight when
    /// the exit drain ran put a brand-new Metal device into the slot the drain
    /// had just emptied, and `exit()` hit the same SIGABRT YV69/YV70 exist to
    /// close. The drain also ignored `loading` entirely and reported `Idle`.
    #[test]
    fn drain_waits_for_an_in_flight_load_and_leaves_nothing_resident() {
        let path = stub_model_file("stub.gguf");
        let m = manager(
            slow_loader(Duration::from_millis(300)),
            Duration::from_secs(60),
            Duration::from_secs(5),
        );
        let loading = load_in_flight(&m, &path);

        let started = Instant::now();
        let outcome = m.drain_and_unload(Duration::from_secs(3));
        assert_eq!(
            outcome,
            DrainOutcome::Drained,
            "a load in flight must be drained, not reported as Idle"
        );
        let err = loading.join().expect("loader thread").unwrap_err();
        assert!(err.contains("exiting"), "unexpected error: {err}");

        // The moment `exit()` would fire: nothing may hold a Metal device.
        assert!(
            !m.is_loaded(),
            "a live engine is resident at exit() after the drain ran"
        );
        assert!(m.loaded_model().is_none());
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the drain must finish with the load, not burn its budget ({:?})",
            started.elapsed()
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// The other half of the same guard: a load that outlasts the drain's budget
    /// still finishes AFTER the quit is under way, so the store itself has to be
    /// refused — otherwise the device it just built lands in the slot the drain
    /// already freed and `exit()` finds it there.
    #[test]
    fn a_load_that_outlasts_the_drain_never_stores_its_engine() {
        let path = stub_model_file("stub.gguf");
        let m = manager(
            slow_loader(Duration::from_millis(400)),
            Duration::from_secs(60),
            Duration::from_secs(5),
        );
        let loading = load_in_flight(&m, &path);

        let outcome = m.drain_and_unload(Duration::from_millis(50));
        assert_eq!(
            outcome,
            DrainOutcome::TimedOut,
            "a load slower than the budget must time the drain out, not block the quit"
        );
        let err = loading.join().expect("loader thread").unwrap_err();
        assert!(err.contains("exiting"), "unexpected error: {err}");
        assert!(
            !m.is_loaded(),
            "the load stored a live engine into the slot the drain had freed"
        );
        assert!(m.loaded_model().is_none());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// YV80: with the startup preload gone, the FIRST take is what brings the
    /// engine up — and only the first. Two sequential takes, each doing exactly
    /// what `transcribe_native` does (idempotent `load`, then `transcribe`),
    /// must cost ONE load and leave the engine warm in between.
    #[test]
    fn first_transcribe_triggers_load_once() {
        let path = stub_model_file("stub.gguf");
        let loads = Arc::new(AtomicU64::new(0));
        let m = manager(
            counting_loader(loads.clone(), Duration::ZERO),
            Duration::from_secs(60),
            Duration::from_secs(5),
        );

        // Lazy: nothing is resident until a take asks for it.
        assert!(!m.is_loaded());
        assert_eq!(loads.load(Ordering::SeqCst), 0, "nothing may load up front");

        for take in 1..=2 {
            m.load("stub/model", &path).expect("load");
            assert_eq!(m.transcribe(vec![0.1; 16], None, None).unwrap(), "lazy");
            assert!(m.is_loaded(), "take {take} must leave the engine warm");
        }
        assert_eq!(
            loads.load(Ordering::SeqCst),
            1,
            "the second take must reuse the warm engine, not reload it"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// YV80: the arm-time load and the transcribe-time load of the SAME first
    /// take overlap by construction (the arm runs while the user is still
    /// talking). Both call `load`; only one engine may be built, or a lazy Yap
    /// would briefly hold two copies of the model — worse than the eager path
    /// it replaced.
    #[test]
    fn a_concurrent_arm_and_take_load_build_one_engine() {
        let path = stub_model_file("stub.gguf");
        let loads = Arc::new(AtomicU64::new(0));
        let m = manager(
            counting_loader(loads.clone(), Duration::from_millis(150)),
            Duration::from_secs(60),
            Duration::from_secs(5),
        );

        let arm = {
            let m = m.clone();
            let path = path.clone();
            std::thread::spawn(move || m.load("stub/model", &path))
        };
        // Let the arm get inside the loader, then do what the take does.
        std::thread::sleep(Duration::from_millis(20));
        m.load("stub/model", &path).expect("take load");
        arm.join().expect("arm thread").expect("arm load");

        assert_eq!(
            loads.load(Ordering::SeqCst),
            1,
            "the take's load must wait for the arm's, not start a second one"
        );
        assert_eq!(m.transcribe(vec![0.1; 16], None, None).unwrap(), "lazy");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// YV34: the "Language I speak" setting used to reach Whisper as the Python
    /// sidecar's `--language`. With the sidecar gone it must reach the embedded
    /// engine instead, or the picker silently stops doing anything.
    #[test]
    fn language_hint_reaches_the_engine() {
        let path = stub_model_file("stub.gguf");
        let seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let m = manager(
            stub_loader_recording(
                "hola",
                Duration::ZERO,
                false,
                seen.clone(),
                Arc::new(Mutex::new(None)),
            ),
            Duration::from_secs(60),
            Duration::from_secs(5),
        );
        m.load("stub/model", &path).expect("load");

        m.transcribe(vec![0.5; 16], Some("es".into()), None)
            .expect("run");
        assert_eq!(seen.lock().as_deref(), Some("es"));

        // No selection stays on autodetect rather than forcing a language.
        m.transcribe(vec![0.5; 16], None, None).expect("run");
        assert_eq!(*seen.lock(), None);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// YV47: the dictionary bias prompt has to survive the whole warm-engine
    /// hand-off (lease → worker thread → engine), or starring a term does
    /// nothing to the decode.
    #[test]
    fn bias_prompt_reaches_the_engine() {
        let path = stub_model_file("stub.gguf");
        let seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let m = manager(
            stub_loader_recording(
                "drivia",
                Duration::ZERO,
                false,
                Arc::new(Mutex::new(None)),
                seen.clone(),
            ),
            Duration::from_secs(60),
            Duration::from_secs(5),
        );
        m.load("stub/model", &path).expect("load");

        m.transcribe(vec![0.5; 16], None, Some("Drivia, Jeisil".into()))
            .expect("run");
        assert_eq!(seen.lock().as_deref(), Some("Drivia, Jeisil"));

        // An empty dictionary means an unbiased decode, not an empty prompt.
        m.transcribe(vec![0.5; 16], None, None).expect("run");
        assert_eq!(*seen.lock(), None);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    // -----------------------------------------------------------------------
    // YV93 — preemption: a dictation takes the engine off a meeting chunk
    // -----------------------------------------------------------------------

    /// A stub that can be ENDED mid-decode, the way the real engine's YV70
    /// cancel token ends a native run — including the part that matters most:
    /// the flag is STICKY, so a stub that is never reset stays cancelled
    /// forever. If the manager stopped resetting it, the tests below would find
    /// every decode after the first preemption aborting instantly.
    struct CancellableStub {
        text: &'static str,
        /// A meeting chunk decode (`transcribe_timed`) — the slow one.
        chunk_delay: Duration,
        /// A dictation (`transcribe`). Sub-second, as it is in the app.
        dictation_delay: Duration,
        cancel: Arc<AtomicBool>,
        decodes: Arc<AtomicU64>,
    }

    impl CancellableStub {
        fn run(&self, delay: Duration) -> Result<String, String> {
            let deadline = Instant::now() + delay;
            while Instant::now() < deadline {
                if self.cancel.load(Ordering::Acquire) {
                    return Err("aborted".to_string());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            self.decodes.fetch_add(1, Ordering::AcqRel);
            Ok(self.text.to_string())
        }
    }

    impl Transcriber for CancellableStub {
        fn transcribe(
            &mut self,
            _samples: &[f32],
            _language: Option<&str>,
            _bias_prompt: Option<&str>,
        ) -> Result<String, String> {
            self.run(self.dictation_delay)
        }

        fn transcribe_timed(
            &mut self,
            _samples: &[f32],
            _language: Option<&str>,
            _bias_prompt: Option<&str>,
        ) -> Result<asr_engine::TimedTranscript, String> {
            self.run(self.chunk_delay)
                .map(asr_engine::TimedTranscript::text_only)
        }

        fn cancel_handle(&self) -> Option<CancelHandle> {
            let flag = self.cancel.clone();
            Some(Arc::new(move || flag.store(true, Ordering::Release)))
        }

        fn reset_cancel(&mut self) {
            self.cancel.store(false, Ordering::Release);
        }
    }

    fn cancellable_manager(
        chunk_delay: Duration,
        dictation_delay: Duration,
        decodes: Arc<AtomicU64>,
    ) -> (TranscriptionManager, PathBuf) {
        let path = stub_model_file("stub.gguf");
        let m = manager(
            Arc::new(move |_p: &Path| {
                Ok(Box::new(CancellableStub {
                    text: "the take came back",
                    chunk_delay,
                    dictation_delay,
                    cancel: Arc::new(AtomicBool::new(false)),
                    decodes: decodes.clone(),
                }) as Box<dyn Transcriber>)
            }),
            Duration::from_secs(600),
            Duration::from_secs(120),
        );
        m.load("stub/model", &path).expect("load");
        (m, path)
    }

    /// Wait until a decode is actually holding the engine.
    fn wait_for_a_decode_in_flight(m: &TranscriptionManager) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !m.status().transcribing {
            assert!(Instant::now() < deadline, "no decode ever went in flight");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// The defect this exists for: a meeting chunk decode LONGER than the
    /// handback wait did not delay the dictation, it destroyed it —
    /// `take_engine` gave up and the dictation path has no retry above it, so
    /// the user's take came back as `no ASR model is loaded`. A five-second
    /// decode is not hypothetical: `ChunkConfig::validate` sanctions a
    /// full-width chunk at up to [`MAX_SANCTIONED_CHUNK_DECODE_SECONDS`].
    ///
    /// So a dictation now takes the engine OFF the chunk rather than queueing
    /// behind it, and the chunk — which is re-decodable from disk — is the one
    /// that gets thrown away.
    #[test]
    fn a_dictation_cancels_an_in_flight_meeting_chunk_instead_of_queueing_behind_it() {
        let decodes = Arc::new(AtomicU64::new(0));
        // Ten seconds: twice the old handback wait, and well inside what the
        // chunk geometry declares legal.
        let (m, path) = cancellable_manager(
            Duration::from_secs(10),
            Duration::from_millis(50),
            decodes.clone(),
        );

        let meeting = {
            let m = m.clone();
            std::thread::spawn(move || m.transcribe_timed(vec![0.1; 16_000], None, None))
        };
        wait_for_a_decode_in_flight(&m);

        let started = Instant::now();
        let dictated = m.transcribe(vec![0.2; 16_000], None, None);
        let waited = started.elapsed();

        assert_eq!(
            dictated.as_deref(),
            Ok("the take came back"),
            "the dictation was lost to a decode the geometry itself sanctions"
        );
        assert!(
            waited < Duration::from_secs(2),
            "the dictation waited {waited:?} — it queued behind the chunk instead \
             of preempting it"
        );
        assert_eq!(
            meeting.join().expect("meeting thread").unwrap_err(),
            PREEMPTED_FOR_DICTATION,
            "a preempted chunk must be told apart from a decode failure, or the \
             meeting driver writes an asr_failed hole for it"
        );
        // …and the engine is still usable afterwards: the sticky cancel flag is
        // reset per decode, so the NEXT chunk is not aborted by the last
        // dictation's cancel.
        let (m2, _p2) = (m.clone(), ());
        assert!(
            m2.transcribe_timed(vec![0.3; 16_000], None, None).is_ok(),
            "the cancel flag stayed set and wedged the warm engine"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// …and the asymmetry holds the other way: a dictation is NEVER cancelled.
    /// A second interactive caller waits (the engine comes back), because a
    /// take exists nowhere but in the buffer being decoded.
    #[test]
    fn a_dictation_is_never_preempted_by_another_dictation() {
        let decodes = Arc::new(AtomicU64::new(0));
        let (m, path) = cancellable_manager(
            Duration::from_millis(400),
            Duration::from_millis(400),
            decodes.clone(),
        );

        let first = {
            let m = m.clone();
            std::thread::spawn(move || m.transcribe(vec![0.1; 16_000], None, None))
        };
        wait_for_a_decode_in_flight(&m);
        let second = m.transcribe(vec![0.2; 16_000], None, None);

        assert_eq!(
            first.join().expect("first thread").as_deref(),
            Ok("the take came back"),
            "one dictation cancelled another"
        );
        assert_eq!(second.as_deref(), Ok("the take came back"));
        assert_eq!(
            decodes.load(Ordering::Acquire),
            2,
            "both takes must have actually decoded"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// The floor under the preemption above, for an engine with NO cancel hook:
    /// the dictation waits, but it must not be DESTROYED. This is the constant
    /// that used to disagree with the geometry — 5 s of patience against a
    /// decode the same module sanctions at up to 60 s.
    #[test]
    fn a_slow_uncancellable_chunk_delays_a_dictation_but_never_loses_it() {
        let path = stub_model_file("stub.gguf");
        // No cancel hook (the default `Transcriber` impl), 6.5 s decode — past
        // the wait this used to have, well inside what the geometry allows.
        let m = manager(
            stub_loader("the take came back", Duration::from_millis(6_500), false),
            Duration::from_secs(600),
            Duration::from_secs(120),
        );
        m.load("stub/model", &path).expect("load");

        let meeting = {
            let m = m.clone();
            std::thread::spawn(move || m.transcribe_timed(vec![0.1; 16_000], None, None))
        };
        wait_for_a_decode_in_flight(&m);
        let dictated = m.transcribe(vec![0.2; 16_000], None, None);

        assert_eq!(
            dictated.as_deref(),
            Ok("the take came back"),
            "the dictation was destroyed rather than delayed"
        );
        assert!(meeting.join().expect("meeting thread").is_ok());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// The two constants can no longer disagree, which is the structural half
    /// of the fix: `ChunkConfig::validate` and [`ENGINE_HANDBACK_WAIT`] are both
    /// derived from [`MAX_SANCTIONED_CHUNK_DECODE_SECONDS`], so no legal chunk
    /// geometry can outlast the patience a dictation has for it.
    #[test]
    fn the_handback_wait_covers_every_chunk_decode_the_geometry_sanctions() {
        assert!(
            ENGINE_HANDBACK_WAIT.as_secs_f64() >= MAX_SANCTIONED_CHUNK_DECODE_SECONDS,
            "a sanctioned {MAX_SANCTIONED_CHUNK_DECODE_SECONDS}s chunk decode outlasts \
             the {ENGINE_HANDBACK_WAIT:?} a dictation waits for the engine"
        );
        assert!(
            MAX_SANCTIONED_CHUNK_DECODE_SECONDS <= TRANSCRIBE_TIMEOUT.as_secs_f64(),
            "the sanctioned decode is longer than the timeout that abandons it"
        );
    }
}
