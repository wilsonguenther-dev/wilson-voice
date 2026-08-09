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
/// How often the idle watcher wakes up to check.
const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(30);
/// Hard ceiling on ONE transcription. Way above any real clip (a 60 s take is
/// ~1 s on Metal) — this exists only so a wedged native call surfaces as an
/// error instead of freezing dictation forever.
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(120);

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

    /// A handle that ends an in-flight [`Transcriber::transcribe`] on this
    /// engine early (YV70). `None` for an engine with no cancel hook, in which
    /// case the exit drain can only wait the decode out.
    fn cancel_handle(&self) -> Option<CancelHandle> {
        None
    }
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

    fn cancel_handle(&self) -> Option<CancelHandle> {
        let token = asr_engine::cancel_token(self);
        Some(Arc::new(move || token.cancel()))
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
    /// the model was already resident (the warm path).
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
    /// YV70 — cancel hook of the transcription currently in flight, if that
    /// engine has one. Kept here because the engine itself is leased out.
    in_flight_cancel: Mutex<Option<CancelHandle>>,
    loading: AtomicBool,
    last_activity_ms: AtomicU64,
    idle_timeout: Duration,
    idle_check_interval: Duration,
    transcribe_timeout: Duration,
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
                in_flight_cancel: Mutex::new(None),
                loading: AtomicBool::new(false),
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
    /// Bounded on purpose. A decode that ignores the cancel is rarer than the
    /// crash this prevents, so a timed-out drain logs at WARN and lets the quit
    /// proceed rather than hanging the user's Cmd-Q.
    ///
    /// [`unload`]: Self::unload
    pub fn drain_and_unload(&self, wait: Duration) -> DrainOutcome {
        self.inner.exiting.store(true, Ordering::Release);
        if self.inner.leases.load(Ordering::Acquire) == 0 {
            self.unload();
            return DrainOutcome::Idle;
        }
        if let Some(cancel) = self.inner.in_flight_cancel.lock().clone() {
            log::info!("exit: cancelling the in-flight transcription");
            cancel();
        }
        let deadline = Instant::now() + wait;
        while self.inner.leases.load(Ordering::Acquire) != 0 {
            if Instant::now() >= deadline {
                log::warn!(
                    "exit: transcription still holds the ASR engine after {}ms — quitting anyway",
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

    /// Blocking load — this is the multi-second call, so run it off the main
    /// thread (see [`load_async`](Self::load_async)). A model already loaded
    /// under the same id is kept as-is.
    pub fn load(&self, model_id: &str, model_path: &Path) -> Result<(), String> {
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
        self.inner.loading.store(true, Ordering::Release);
        // A panicking native load must not take the app with it, and must not
        // leave a half-built engine in the slot.
        let loaded = catch_unwind(AssertUnwindSafe(|| (self.inner.loader)(model_path)))
            .unwrap_or_else(|p| Err(format!("ASR model load panicked: {}", panic_message(&p))));
        self.inner.loading.store(false, Ordering::Release);
        let engine = loaded?;
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
        *self.inner.engine.lock() = Some(LoadedEngine {
            model_id: model_id.to_string(),
            engine,
        });
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
        self.touch();
        if samples_16k_mono.is_empty() {
            return Ok(String::new());
        }
        let generation = self.inner.generation.load(Ordering::Acquire);
        let Some(engine) = self.inner.engine.lock().take() else {
            return Err("no ASR model is loaded".into());
        };
        self.inner.leases.fetch_add(1, Ordering::AcqRel);
        // YV70: publish this run's cancel hook while it is in flight, so the
        // exit drain can end the decode instead of only waiting on it.
        *self.inner.in_flight_cancel.lock() = engine.engine.cancel_handle();

        let (tx, rx) = mpsc::channel::<(Option<LoadedEngine>, Result<String, String>)>();
        std::thread::spawn(move || {
            let mut engine = engine;
            // Panic containment: on unwind the engine is dropped instead of
            // returned, so a poisoned native session is never reused.
            let sent = match catch_unwind(AssertUnwindSafe(|| {
                engine.engine.transcribe(
                    &samples_16k_mono,
                    language.as_deref(),
                    bias_prompt.as_deref(),
                )
            })) {
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
        *self.inner.in_flight_cancel.lock() = None;
        // YV70: the lease is released only AFTER the engine has been put back
        // (or dropped). It used to drop first, so the exit drain could see a
        // zero lease and unload while `return_engine` was still on its way to
        // refill the slot — the live Metal device the drain exists to prevent.
        self.inner.leases.fetch_sub(1, Ordering::AcqRel);
        self.touch();
        result
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
}
