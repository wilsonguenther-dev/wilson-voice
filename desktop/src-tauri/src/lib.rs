//! Wilson Voice — production dictation desktop app
//!
//! Hotkeys: primary FN / FN+Control via CGEvent tap (ptt_macos).
//! Optional legacy ⌘⇧V via Carbon global-shortcut.
//! HUD: floating float.html pill, parked bottom-center (no cursor chase).

mod asr_engine;
mod cli;
mod db;
mod dictation;
mod float_pill;
mod focus;
mod logging;
mod mic_auth;
mod models;
mod paste;
mod permissions;
#[cfg(target_os = "macos")]
mod ptt_macos;
mod record;
mod sysaudio;
mod transcription;
mod vad;

use db::{Database, DayCount, DictEntry, Insights, ScratchNote, TranscriptEntry};
use parking_lot::Mutex as PLMutex;
use permissions::PermissionReport;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Emitter, Manager, State, WindowEvent, Wry,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub language: String,
    pub auto_paste: bool,
    pub hotkey_label: String,
    /// Always show HUD; also auto-shows while recording even if false.
    pub show_floating_pill: bool,
    /// fn | fn_control | both — CGEvent FN PTT (not Carbon).
    #[serde(default = "default_ptt_binding")]
    pub ptt_binding: String,
    /// Keep Carbon ⌘⇧V as secondary hold binding.
    #[serde(default = "default_true")]
    pub keep_cmd_shift_v: bool,
    /// Floating pill style: "classic" (obsidian capsule) | "yappy" (pixel pet).
    #[serde(default = "default_pill_style")]
    pub pill_style: String,
    /// Companion tone (YV27): friendly | rude | rose (default "friendly").
    /// Drives Yappy's reactive lines (pill chatter + house mood). Kept
    /// independent of `pill_style` so either companion can be warm, sassy, or
    /// sweet. The pill/house read this live off the emitted settings.
    #[serde(default = "default_companion_tone")]
    pub companion_tone: String,
    /// Smart-dictation mode: auto | plain | list | email | code | notes.
    /// "auto" infers the mode from the frontmost app; any other value forces it.
    #[serde(default = "default_dictation_mode")]
    pub dictation_mode: String,
    /// Auto-Cleanup level (YV10): none | light | medium | high (default "light").
    /// Gates the cleanup pipeline — "none" is a raw passthrough; higher levels
    /// enable dictionary → backtrack → formatting → local-LLM polish (see
    /// `dictation::CleanupLevel` / `run_cleanup`).
    #[serde(default = "default_cleanup_level")]
    pub cleanup_level: String,
    /// Denoise the captured clip with RNNoise before transcription (YV12).
    /// Suppresses steady background noise (fans, hum, keyboard) over the
    /// native-rate buffer before the 16 kHz downsample. Defaults on; the
    /// denoiser itself falls back to the raw audio on any degeneracy.
    #[serde(default = "default_true")]
    pub denoise: bool,
    /// Auto-mute the whole Mac's system output while dictating (YV28). On record
    /// start we snapshot + mute the default output device so nothing plays over
    /// the user; on stop / cancel / error / exit we restore the EXACT prior mute
    /// + volume. Defaults on; the restore is unconditional so the Mac is never
    /// left muted even if this is toggled off mid-take.
    #[serde(default = "default_true")]
    pub mute_while_dictating: bool,
    /// First-run onboarding completed (YV9). While false the UI shows the
    /// welcome → permissions → voice-calibration flow; set true on finish.
    #[serde(default)]
    pub onboarded: bool,
    /// Calibration phrase captured during onboarding, kept for later
    /// personalization (initial_prompt biasing / future voice adaptation).
    #[serde(default)]
    pub calibration_sample: Option<String>,
    /// Selected embedded (GGUF) ASR model — a `models::catalog()` repo id
    /// (YV31). Since YV34 deleted the Python sidecar this is THE model the
    /// dictation path runs on; there is no other ASR selector.
    #[serde(default = "default_native_model")]
    pub native_model: String,
}

fn default_ptt_binding() -> String {
    "fn_control".into()
}
fn default_pill_style() -> String {
    "classic".into()
}
fn default_companion_tone() -> String {
    "friendly".into()
}
fn default_dictation_mode() -> String {
    "auto".into()
}
fn default_cleanup_level() -> String {
    "light".into()
}
fn default_true() -> bool {
    true
}
/// Fresh installs (and settings written before YV31) select the catalog's
/// recommended model — one source of truth, no hardcoded id to drift.
fn default_native_model() -> String {
    models::recommended_model().id.clone()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: "en".into(),
            auto_paste: true,
            hotkey_label: "fn⌃".into(),
            // Always-on glass island rides all Spaces
            show_floating_pill: true,
            ptt_binding: "fn_control".into(),
            keep_cmd_shift_v: false,
            pill_style: "classic".into(),
            companion_tone: "friendly".into(),
            dictation_mode: "auto".into(),
            cleanup_level: "light".into(),
            denoise: true,
            mute_while_dictating: true,
            onboarded: false,
            calibration_sample: None,
            native_model: default_native_model(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub recording: bool,
    pub busy: bool,
    pub last_error: Option<String>,
    pub message: String,
    pub accessibility: bool,
    pub hotkey_registered: bool,
    #[serde(default)]
    pub hands_free: bool,
    /// YV33 — dictation can actually run right now: the selected embedded model
    /// is downloaded. The UI shows "Ready" only when this is true; otherwise it
    /// routes to the model step.
    #[serde(default)]
    pub model_ready: bool,
}

struct AppState {
    settings: PLMutex<AppSettings>,
    recording: PLMutex<bool>,
    busy: PLMutex<bool>,
    /// Hands-free latch (double-tap fn⌃); release keys but keep recording.
    hands_free: PLMutex<bool>,
    recorder: PLMutex<Option<record::ActiveRecording>>,
    /// System-output state captured when we muted the Mac for a take (YV28).
    /// `Some` only while a recording has the output muted; restored + cleared on
    /// stop / cancel / error / exit so the Mac is never left muted.
    saved_audio: PLMutex<Option<sysaudio::OutputAudioState>>,
    db: Arc<Database>,
    last_error: PLMutex<Option<String>>,
    hotkey_registered: PLMutex<bool>,
    /// Menu-bar (tray) handles, set once during setup after the tray is built.
    /// Kept so state transitions can reflect into the dropdown + tooltip (YV26).
    tray: PLMutex<Option<TrayIcon<Wry>>>,
    /// The dynamic Start/Stop Dictation item (label follows `recording`).
    tray_dictation: PLMutex<Option<MenuItem<Wry>>>,
    /// The Hands-free check item (check follows the `hands_free` latch).
    tray_hands_free: PLMutex<Option<CheckMenuItem<Wry>>>,
    /// Warm Silero VAD (YV36) — loaded ONCE from disk during startup (see the
    /// background thread in `run()`) and reused by every dictation, instead of
    /// re-loading + re-failing an ONNX graph on each clip. `None` until the
    /// model asset is on disk (or forever, if the download/load fails), which
    /// is the explicit energy-VAD fallback.
    vad: PLMutex<Option<Arc<vad::WarmVad>>>,
    /// Warm embedded-ASR engine lifecycle (YV31). Owns the loaded GGUF model
    /// and its idle-unload watcher; backs the model-management commands. Since
    /// YV34 it is the app's ONLY transcriber (see `stop_and_transcribe`).
    transcription: transcription::TranscriptionManager,
}

fn data_dir() -> PathBuf {
    let p = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("WilsonVoice");
    let _ = std::fs::create_dir_all(&p);
    let _ = std::fs::create_dir_all(p.join("recordings"));
    p
}

/// Can a take actually be transcribed right now? YV34 leaves exactly one
/// answer: the selected embedded model is downloaded. False means "Model
/// needed", never "Ready".
fn model_ready(state: &AppState) -> bool {
    let native = state.settings.lock().native_model.clone();
    native_model_ready(&native).is_some()
}

/// The status-line policy, pure so the "Ready" honesty rule is testable without
/// an AppState. `model_ready` false outranks everything except live
/// recording/transcribing/errors: a fresh install with no model must say so.
fn status_message(
    recording: bool,
    hands_free: bool,
    busy: bool,
    last_error: Option<&str>,
    model_ready: bool,
    accessibility: bool,
    ptt: &str,
) -> String {
    if recording && hands_free {
        format!("Hands-free… tap {ptt} to stop")
    } else if recording {
        format!("Recording… release {ptt} or click Stop")
    } else if busy {
        "Transcribing with local Whisper…".into()
    } else if let Some(err) = last_error {
        format!("Error: {err}")
    } else if !model_ready {
        "Model needed — download a speech model to start".into()
    } else if !accessibility {
        format!("Ready — {ptt} (enable Accessibility)")
    } else {
        format!("Ready — hold {ptt} · double-tap hands-free")
    }
}

fn build_status(state: &AppState) -> AppStatus {
    let recording = *state.recording.lock();
    let busy = *state.busy.lock();
    let hands_free = *state.hands_free.lock();
    let last_error = state.last_error.lock().clone();
    let accessibility = permissions::is_accessibility_trusted();
    let hotkey_registered = *state.hotkey_registered.lock();
    let ptt = state.settings.lock().hotkey_label.clone();
    let ready = model_ready(state);
    let message = status_message(
        recording,
        hands_free,
        busy,
        last_error.as_deref(),
        ready,
        accessibility,
        &ptt,
    );
    AppStatus {
        recording,
        busy,
        last_error,
        message,
        accessibility,
        hotkey_registered,
        hands_free,
        model_ready: ready,
    }
}

fn emit_status(app: &AppHandle, state: &AppState) {
    let _ = app.emit("status", &build_status(state));
    sync_tray(app, state);
}

/// Reflect the live recording / busy / hands-free state into the menu-bar
/// dropdown (Start↔Stop Dictation label, Hands-free check) and the tray tooltip
/// (idle vs recording vs transcribing). Called from `emit_status`, so every
/// state transition already funneling through it also refreshes the toolbar —
/// keeping the menu-bar control in lockstep with the pill + hotkey (YV26).
///
/// muda menu / NSStatusItem mutations must run on the main thread on macOS, and
/// `emit_status` is sometimes invoked from the ASR worker thread, so the actual
/// updates are hopped onto the main thread with cheap Arc-backed handle clones.
fn sync_tray(app: &AppHandle, state: &AppState) {
    let recording = *state.recording.lock();
    let busy = *state.busy.lock();
    let hands_free = *state.hands_free.lock();
    let dictation = state.tray_dictation.lock().clone();
    let hf_item = state.tray_hands_free.lock().clone();
    let tray = state.tray.lock().clone();
    if dictation.is_none() && hf_item.is_none() && tray.is_none() {
        return; // tray not built yet (early startup emit)
    }
    let _ = app.run_on_main_thread(move || {
        if let Some(item) = dictation {
            let _ = item.set_text(if recording {
                "Stop Dictation"
            } else {
                "Start Dictation"
            });
        }
        if let Some(item) = hf_item {
            let _ = item.set_checked(hands_free);
        }
        if let Some(tray) = tray {
            let tip = if recording && hands_free {
                "Yap — hands-free recording"
            } else if recording {
                "Yap — recording"
            } else if busy {
                "Yap — transcribing…"
            } else {
                "Yap — hold fn to dictate"
            };
            let _ = tray.set_tooltip(Some(tip));
        }
    });
}

fn notify(app: &AppHandle, title: &str, body: impl Into<String>) {
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(body.into())
        .show();
}

/// Mute the Mac's whole system output for the duration of a take (YV28), gated
/// by the `mute_while_dictating` setting. Snapshots the prior mute + volume into
/// `state.saved_audio` so `restore_system_output` can put it back verbatim.
fn mute_system_output(state: &AppState) {
    if !state.settings.lock().mute_while_dictating {
        return;
    }
    if let Some(saved) = sysaudio::mute_and_save() {
        *state.saved_audio.lock() = Some(saved);
        log::info!("system output muted for dictation");
    }
}

/// Restore the system output to its pre-dictation state (YV28). Unconditional
/// by design — it always puts back whatever `mute_system_output` saved,
/// regardless of the current setting, so the Mac is NEVER left muted (covers
/// stop, cancel, every error path, and app exit). No-op if nothing was muted.
fn restore_system_output(state: &AppState) {
    let saved = state.saved_audio.lock().take();
    if let Some(saved) = saved {
        sysaudio::restore(saved);
        log::info!("system output restored after dictation");
    }
}

fn start_recording(app: &AppHandle, state: &AppState) {
    if *state.recording.lock() || *state.busy.lock() {
        return;
    }
    // Do NOT call mic_auth::request_microphone_access here — that is Permissions-only.
    // Opening the real capture stream is enough for TCC (Allow once after install).
    let denoise = state.settings.lock().denoise;
    // YV35: anchor the press→capture_start span on the physical key-down when
    // this take came from the PTT hold (None for tray/button/hands-free starts).
    let pressed_at = ptt_macos::press_started_at();
    match record::start_recording(data_dir().join("recordings"), denoise, pressed_at) {
        Ok(active) => {
            let level = active.level.clone();
            let stop_flag = active.stop_flag();
            *state.recorder.lock() = Some(active);
            *state.recording.lock() = true;
            *state.last_error.lock() = None;
            // YV28: silence the whole Mac so nothing plays over the user while
            // they talk. Restored the instant recording stops (see below).
            mute_system_output(state);
            log::info!("recording started");
            let _ = app.emit("recording", true);
            emit_status(app, state);
            // Wispr-style: glass island appears for the hold
            float_pill::show_for_recording(app);
            // Stream mic levels to float HUD (~20 fps)
            let app_lv = app.clone();
            std::thread::spawn(move || {
                use std::sync::atomic::Ordering;
                while !stop_flag.load(Ordering::SeqCst) {
                    let v = level.load(Ordering::Relaxed) as f64 / 1000.0;
                    let _ = app_lv.emit("audio_level", v);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                let _ = app_lv.emit("audio_level", 0.0);
            });
        }
        Err(e) => {
            log::error!("record start failed: {e}");
            *state.last_error.lock() = Some(e.clone());
            emit_status(app, state);
            notify(app, "Yap — Mic", e);
        }
    }
}

/// Discard in-flight take (FN interrupt / cancel) — no ASR.
fn cancel_recording(app: &AppHandle, state: &AppState) {
    if !*state.recording.lock() {
        return;
    }
    *state.recording.lock() = false;
    *state.hands_free.lock() = false;
    // YV28: un-mute the Mac the moment the take ends (cancel path).
    restore_system_output(state);
    let _ = app.emit("recording", false);
    if let Some(active) = state.recorder.lock().take() {
        // Discarded take — no ASR, so skip the Silero isolation pass entirely.
        // The returned clip guard unlinks the recovery wav as it drops here.
        let _ = record::stop_recording(active, None);
    }
    *state.last_error.lock() = Some("Dictation cancelled (key while holding)".into());
    emit_status(app, state);
    float_pill::after_recording(app, state.settings.lock().show_floating_pill);
    log::info!("recording cancelled");
}

/// Panic-safety net for the transcribe worker (audit finding [8]). `busy` is set
/// true before the worker thread spawns and reset at the end of its normal path;
/// if the worker *panics* before that reset, `busy` stays true forever and
/// dictation is permanently soft-locked until restart. This guard force-resets
/// `busy`/`hands_free` on unwind. It is disarmed on the normal path, so it only
/// fires on an actual panic.
struct WorkerBusyGuard {
    state: Arc<AppState>,
    app: AppHandle,
    armed: bool,
}
impl Drop for WorkerBusyGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        *self.state.busy.lock() = false;
        *self.state.hands_free.lock() = false;
        log::error!(
            "transcribe worker unwound before cleanup — force-reset busy/hands_free (was soft-lock)"
        );
        emit_status(&self.app, &self.state);
    }
}

/// Emitted whenever a take fails to produce a transcript (YV32). Before this the
/// error path only logged + notified, so the UI had NOTHING to listen to: the
/// onboarding overlay clears its "Transcribing…" state on `transcript` alone and
/// spun forever on any ASR failure. Payload: `{ message }`.
pub const TRANSCRIPT_ERROR_EVENT: &str = "transcript_error";

/// The selected embedded model when it is actually downloaded, as
/// `(catalog id, on-disk path)`. `None` means nothing has been fetched yet, so
/// no take can be transcribed at all (YV34 — there is no sidecar to fall back
/// to any more).
fn native_model_ready(native_model: &str) -> Option<(String, PathBuf)> {
    let model = models::catalog_model(native_model)?;
    if !models::is_downloaded(model) {
        return None;
    }
    models::model_path(model).map(|path| (model.id.clone(), path))
}

/// Transcribe a captured take with the embedded GGUF engine — since YV34 the
/// app's only ASR path: load the model into the warm manager (a no-op when it is
/// already resident) and run it over the take's samples. Both the load and the
/// transcription are already bounded + panic-contained by the manager, so this
/// can fail but can never hang.
///
/// YV37: `samples` are the 16 kHz mono floats the capture path already produced
/// in memory. The clip is no longer written, read back and re-decoded from disk
/// before ASR sees a single sample.
fn transcribe_native(
    manager: &transcription::TranscriptionManager,
    model_id: &str,
    model_path: &Path,
    samples: Vec<f32>,
    language: &str,
) -> Result<transcription::AsrOutput, String> {
    let started = std::time::Instant::now();
    manager.load(model_id, model_path)?;
    // The "Language I speak" setting used to reach Whisper as the sidecar's
    // `--language`; it now rides the engine's own language hint (YV34) so the
    // picker keeps working. Blank = autodetect.
    let language = Some(language.trim())
        .filter(|l| !l.is_empty())
        .map(str::to_string);
    let text = manager.transcribe(samples, language)?;
    if text.trim().is_empty() {
        return Err("Empty transcript".into());
    }
    Ok(transcription::AsrOutput {
        text,
        backend: "native".into(),
        seconds: started.elapsed().as_secs_f64(),
    })
}

fn stop_and_transcribe(app: AppHandle, state: Arc<AppState>) {
    // Hands-free: ignore key-release; only stop on explicit tap / button
    if *state.hands_free.lock() {
        log::debug!("stop ignored — hands-free latch active");
        return;
    }
    if !*state.recording.lock() {
        return;
    }
    *state.recording.lock() = false;
    // YV28: un-mute the Mac immediately on release — restore the exact prior
    // mute + volume BEFORE the (async) transcription so audio comes right back.
    restore_system_output(&state);
    let _ = app.emit("recording", false);

    let active = state.recorder.lock().take();
    let Some(active) = active else {
        return;
    };

    *state.busy.lock() = true;
    emit_status(&app, &state);

    let settings = state.settings.lock().clone();
    let db = state.db.clone();
    let app2 = app.clone();
    let state2 = state.clone();
    // Capture focused app *before* we steal focus with notifications / main window.
    let source_app = focus::frontmost_app_name();
    let t_release = std::time::Instant::now();

    std::thread::spawn(move || {
        // Panic-safety: disarmed after the normal cleanup below (see [8]).
        let mut busy_guard = WorkerBusyGuard {
            state: state2.clone(),
            app: app2.clone(),
            armed: true,
        };
        // `Ok(None)` = nothing to paste (no speech, or a rejected hallucination
        // loop): the caller shows a gentle soft status instead of a hard error.
        let result = (|| -> Result<Option<(TranscriptEntry, paste::PasteOutcome, i64)>, String> {
            // YV36 voice isolation — borrow the WARM Silero VAD built once at
            // startup (gated behind the `denoise` setting). Nothing is loaded,
            // downloaded or analysed on the hot path; `None` (model still
            // downloading, load failed, or denoise off) → the energy-VAD path,
            // so an utterance is never lost.
            let iso_vad = if settings.denoise {
                state2.vad.lock().clone()
            } else {
                None
            };
            let rec = record::stop_recording(active, iso_vad.as_deref())?;
            // YV20/M3: `rec.clip` unlinks the history wav on ANY exit of this
            // closure (success, the gates below, or the ASR/DB `?` failures) —
            // never leaked on error. YV37: that wav is written on a background
            // thread from here on, so it is off the release→ASR path entirely.
            let t_dsp = t_release.elapsed().as_millis() as i64;
            log::info!(
                "samples ready: {} speech={:.2}s voiced={:.2}s hold_wall={:.2}s dsp_ms={}",
                rec.clip.path().display(),
                rec.speech_seconds,
                rec.voiced_seconds,
                rec.hold_wall_seconds,
                t_dsp
            );
            // No-speech gate (YV16): a near-silent / sub-second tap has ~0 TRUE
            // voiced time. Skip ASR entirely so Whisper never hallucinates
            // repetitive garbage ("WPM-SERV-SERV…") on silence. YV29 STRENGTHENS
            // this: Silero VAD (`speech_present`) also rejects a clip it finds no
            // speech in — catching steady-noise cases the energy RMS gate scores
            // as voiced. `speech_present` is true whenever Silero is unavailable,
            // so a missing model never drops a real utterance.
            if !dictation::has_enough_speech(rec.voiced_seconds) || !rec.speech_present {
                log::info!(
                    "no-speech gate: voiced={:.2}s speech_present={} below threshold — skipping ASR",
                    rec.voiced_seconds,
                    rec.speech_present
                );
                return Ok(None);
            }
            // YV34 — the embedded GGUF engine is the ONLY transcriber: it runs
            // in-process (no interpreter, no HuggingFace download) on the
            // selected native model. With nothing downloaded we fail fast with an
            // actionable message instead of pretending; the `transcript_error`
            // emit below is what unspins any UI waiting on this take.
            let Some((model_id, model_path)) = native_model_ready(&settings.native_model) else {
                return Err(
                    "No speech model installed — open Settings → Models and download one.".into(),
                );
            };
            let asr = transcribe_native(
                &state2.transcription,
                &model_id,
                &model_path,
                rec.samples,
                &settings.language,
            )?;
            let t_asr = t_release.elapsed().as_millis() as i64;
            // Raw ASR output — preserved verbatim so both raw and polished text are
            // stored on the transcript (Wispr Flow "Undo AI edit" / raw↔polished).
            let raw_text = asr.text;
            // Hallucination gate (YV16): reject degenerate Whisper repetition loops
            // ("WPM-SERV-SERV-SERV…") BEFORE they reach the clipboard/paste path.
            if dictation::is_hallucinated_repetition(&raw_text) {
                // YV20/M2: never write the transcript body to the log — count only.
                log::info!(
                    "hallucination gate: rejected degenerate ASR output ({} chars)",
                    raw_text.chars().count()
                );
                return Ok(None);
            }
            // Smart dictation (YV5): resolve the effective mode — a user-picked fixed mode
            // wins, otherwise it's inferred from the focused app.
            let dictation_mode = dictation::resolve_mode(
                &settings.dictation_mode,
                source_app.as_deref().unwrap_or_default(),
            );
            // YV10 cleanup pipeline: apply_dictionary → backtrack → format_dictation →
            // local-LLM polish (guarded stub), gated by the Auto-Cleanup level. Every
            // stage is guarded so a non-empty transcript can never become empty (falls
            // back to its input on any error/empty result — "never lose text").
            let cleanup_level = dictation::CleanupLevel::from_setting(&settings.cleanup_level);
            let text = dictation::run_cleanup(
                &raw_text,
                cleanup_level,
                dictation_mode,
                |t| db.apply_dictionary(t).unwrap_or_else(|_| t.to_string()),
                dictation::polish_llm,
            );
            log::info!(
                "cleanup-pipeline: level={:?} mode={:?} dictation_setting={} cleanup_level={}",
                cleanup_level,
                dictation_mode,
                settings.dictation_mode,
                settings.cleanup_level
            );
            // Always copy first (Wispr Flow: never lose text). The wrong-target
            // guard (YV21) inside copy_and_maybe_paste re-checks the frontmost app
            // against `source_app` immediately before ⌘V and falls back to
            // clipboard-only if the user switched apps during the ASR delay.
            let want_paste = settings.auto_paste && focus::should_auto_paste();
            let outcome =
                paste::copy_and_maybe_paste(&app2, &text, want_paste, source_app.as_deref());
            // North-star metric: release hotkey → text on clipboard
            let pipeline_ms = t_release.elapsed().as_millis() as i64;
            log::info!(
                "latency hold→clipboard={}ms (press→capture={}ms dsp={} asr_done={} asr_model={:.0}ms) backend={}",
                pipeline_ms,
                rec.capture_start_ms,
                t_dsp,
                t_asr,
                asr.seconds * 1000.0,
                asr.backend
            );
            // Store BOTH the polished text and the raw ASR transcript (YV10).
            let entry = db.insert_transcript_at(
                text,
                asr.backend,
                asr.seconds,
                rec.speech_seconds,
                pipeline_ms,
                source_app,
                chrono::Utc::now(),
                Some(raw_text),
            )?;
            // Hygiene: the history wav is unlinked by `rec.clip` on scope exit
            // (audio stays local only during the process, success or error alike).
            Ok(Some((entry, outcome, pipeline_ms)))
        })();

        match result {
            Ok(Some((entry, outcome, pipeline_ms))) => {
                if outcome.pasted {
                    *state2.last_error.lock() = None;
                } else if !outcome.copied {
                    *state2.last_error.lock() = Some(outcome.message.clone());
                } else if settings.auto_paste && !outcome.pasted {
                    // Soft warning — text is still on clipboard
                    *state2.last_error.lock() = Some(outcome.message.clone());
                } else {
                    *state2.last_error.lock() = None;
                }
                let _ = app2.emit("transcript", &entry);
                let _ = app2.emit("paste_outcome", &outcome.message);
                let _ = app2.emit(
                    "latency",
                    serde_json::json!({
                        "pipelineMs": pipeline_ms,
                        "asrSeconds": entry.asr_seconds,
                        "speechSeconds": entry.speech_seconds,
                    }),
                );
                let preview = if entry.text.chars().count() > 100 {
                    let s: String = entry.text.chars().take(100).collect();
                    format!("{s}…")
                } else {
                    entry.text.clone()
                };
                notify(
                    &app2,
                    "Yap",
                    format!("{preview}\n({} · {}ms)", outcome.message, pipeline_ms),
                );
                log::info!(
                    "transcript ok words={} pasted={} pipeline_ms={}",
                    entry.word_count,
                    outcome.pasted,
                    pipeline_ms
                );
            }
            Ok(None) => {
                // No speech / rejected hallucination: nothing to paste. This is a
                // NORMAL fumbled tap, not a failure — surface a gentle soft status
                // (transient flash toast) and do NOT set a hard last_error, do NOT
                // insert a transcript.
                *state2.last_error.lock() = None;
                let msg = "Didn't catch any speech — hold and speak";
                let _ = app2.emit("paste_outcome", msg);
                log::info!("no-speech / hallucination: skipped paste, soft status shown");
            }
            Err(e) => {
                log::error!("pipeline failed: {e}");
                *state2.last_error.lock() = Some(e.clone());
                // YV32: tell the UI, not just the log. Any view waiting on a
                // transcript (onboarding calibration, the pill) can now clear its
                // busy state and show the reason instead of spinning forever.
                let _ = app2.emit(
                    TRANSCRIPT_ERROR_EVENT,
                    serde_json::json!({ "message": e.clone() }),
                );
                notify(&app2, "Yap — Failed", e);
            }
        }
        *state2.busy.lock() = false;
        *state2.hands_free.lock() = false;
        // Keep island on all Spaces if always-on; else hide after take
        float_pill::after_recording(&app2, state2.settings.lock().show_floating_pill);
        emit_status(&app2, &state2);
        // Reached the normal end — no soft-lock, disarm the panic guard.
        busy_guard.armed = false;
    });
}

// --- Commands ---

/// Write settings.json (the single on-disk copy every settings writer goes
/// through — `save_settings` and the YV31 model commands).
fn persist_settings(settings: &AppSettings) -> Result<(), String> {
    let path = data_dir().join("settings.json");
    let s = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, s).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_settings(state: State<'_, Arc<AppState>>) -> AppSettings {
    state.settings.lock().clone()
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: AppSettings,
) -> Result<(), String> {
    let mut next = settings;
    // Keep label in sync with binding
    next.hotkey_label = match next.ptt_binding.as_str() {
        "fn" => "fn".into(),
        "both" | "fn_or_fn_control" => "fn / fn⌃".into(),
        _ => "fn⌃".into(),
    };
    *state.settings.lock() = next.clone();
    persist_settings(&next)?;
    // Tell the floating pill to re-read (e.g. switch classic ↔ yappy live).
    let _ = app.emit("settings", &next);
    if next.show_floating_pill {
        float_pill::show_float(&app)?;
    } else if !*state.recording.lock() {
        float_pill::hide_float(&app);
    }
    #[cfg(target_os = "macos")]
    {
        ptt_macos::set_binding(ptt_macos::PttBinding::from_settings(&next.ptt_binding));
    }
    Ok(())
}

#[tauri::command]
fn get_history(
    state: State<'_, Arc<AppState>>,
    query: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<TranscriptEntry>, String> {
    state.db.list_transcripts(limit.unwrap_or(200), query)
}

#[tauri::command]
fn clear_history(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.db.clear_transcripts()
}

#[tauri::command]
fn delete_entry(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.db.delete_transcript(&id)
}

#[tauri::command]
fn get_status(state: State<'_, Arc<AppState>>) -> AppStatus {
    build_status(&state)
}

#[tauri::command]
fn get_permissions(state: State<'_, Arc<AppState>>) -> PermissionReport {
    // YV33/YV34: the ASR row reports the engine that actually runs — a
    // downloaded embedded model. Nothing here spawns a process, so the 1.2 s
    // onboarding poll is now free (it used to exec the CLT python shim and pop
    // a developer-tools dialog on every tick).
    let native = state.settings.lock().native_model.clone();
    permissions::report(false, native_model_ready(&native).is_some())
}

#[tauri::command]
fn request_accessibility() -> bool {
    permissions::request_accessibility_prompt()
}

#[tauri::command]
fn request_microphone() -> bool {
    mic_auth::request_microphone_access()
}

#[tauri::command]
fn show_float_pill(app: AppHandle) -> Result<(), String> {
    float_pill::show_float(&app)
}

#[tauri::command]
fn hide_float_pill(app: AppHandle) {
    float_pill::hide_float(&app);
}

#[tauri::command]
fn get_insights(state: State<'_, Arc<AppState>>) -> Result<Insights, String> {
    state.db.insights()
}

/// Contiguous, zero-filled day-by-day words/sessions series (oldest first) for the
/// last `days` days. Feeds the Insights daily bar chart + the activity heatmap.
#[tauri::command]
fn daily_series(state: State<'_, Arc<AppState>>, days: i64) -> Result<Vec<DayCount>, String> {
    state.db.daily_series(days)
}

/// Contiguous, zero-filled month-by-month rollup (oldest first, "YYYY-MM") for the
/// last `months` months. Feeds the Insights monthly bar chart / long-range views.
#[tauri::command]
fn monthly_series(state: State<'_, Arc<AppState>>, months: i64) -> Result<Vec<DayCount>, String> {
    state.db.monthly_series(months)
}

#[tauri::command]
fn list_dictionary(state: State<'_, Arc<AppState>>) -> Result<Vec<DictEntry>, String> {
    state.db.list_dictionary()
}

#[tauri::command]
fn add_dictionary_term(
    state: State<'_, Arc<AppState>>,
    term: String,
    preferred: Option<String>,
) -> Result<DictEntry, String> {
    state.db.add_dictionary_term(term, preferred)
}

#[tauri::command]
fn delete_dictionary_term(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.db.delete_dictionary_term(&id)
}

#[tauri::command]
fn list_scratch(state: State<'_, Arc<AppState>>) -> Result<Vec<ScratchNote>, String> {
    state.db.list_scratch()
}

#[tauri::command]
fn save_scratch(
    state: State<'_, Arc<AppState>>,
    id: Option<String>,
    title: String,
    body: String,
) -> Result<ScratchNote, String> {
    state.db.save_scratch(id, title, body)
}

#[tauri::command]
fn delete_scratch(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.db.delete_scratch(&id)
}

#[tauri::command]
fn copy_entry(app: AppHandle, text: String) -> Result<(), String> {
    paste::copy_text(&app, &text)
}

/// Re-paste the most recent transcript — backs the tray "Paste Last Transcript"
/// item and the global ⌃⌘V shortcut (Wispr-parity). Reads the newest row
/// (`list_transcripts` is `ORDER BY created_at DESC`) and pastes it into the
/// current frontmost app. Like `paste_entry`, the CGEvent ⌘V hop BLOCKS the
/// main thread, so the paste MUST run off-main via `spawn_blocking` — calling
/// it directly from a main-thread menu/shortcut callback would deadlock. This
/// only copies+pastes; it does NOT insert a new history row (no duplicate).
fn paste_last_transcript(app: &AppHandle, state: &AppState) {
    let text = match state.db.list_transcripts(1, None) {
        Ok(entries) => match entries.into_iter().next() {
            Some(e) => e.text,
            None => {
                log::info!("paste last transcript: history is empty");
                return;
            }
        },
        Err(err) => {
            log::warn!("paste last transcript: db read failed: {err}");
            return;
        }
    };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Explicit re-paste: the current frontmost app is the intended target,
        // so pass `None` (the source-app guard does not apply here).
        let o = paste::copy_and_maybe_paste(&app, &text, true, None);
        if !o.copied {
            log::warn!("paste last transcript failed: {}", o.message);
        }
    });
}

#[tauri::command]
async fn paste_entry(app: AppHandle, text: String) -> Result<String, String> {
    // MUST be async → runs OFF the main thread. copy_and_maybe_paste hops to the
    // main thread for the CGEvent ⌘V and blocks waiting on it; a *sync* command
    // runs ON main and would deadlock waiting for a closure queued to that same
    // thread (3s freeze, then "paste timed out"). spawn_blocking keeps the async
    // runtime unblocked while the short main-thread hop completes.
    tauri::async_runtime::spawn_blocking(move || {
        // Explicit user re-paste of a stored entry: the current frontmost app IS
        // the intended target, so pass `None` — the YV21 source-app guard does
        // not apply here.
        let o = paste::copy_and_maybe_paste(&app, &text, true, None);
        if o.copied {
            Ok(o.message)
        } else {
            Err(o.message)
        }
    })
    .await
    .map_err(|e| format!("paste task join failed: {e}"))?
}

#[tauri::command]
fn open_data_dir() -> Result<(), String> {
    std::process::Command::new("open")
        .arg(data_dir())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Export diagnostics (YV7): reveal the rotating logs folder (`data_dir/logs/`)
/// so users can attach yap.log to a support/bug report. Reuses the open pattern.
#[tauri::command]
fn open_logs_dir() -> Result<(), String> {
    std::process::Command::new("open")
        .arg(logging::logs_dir(&data_dir()))
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Export transcript history to Application Support for backup / future LoRA corpus.
#[tauri::command]
fn export_history(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let json = state.db.export_transcripts_json()?;
    let path = data_dir().join(format!(
        "export-transcripts-{}.json",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    std::fs::write(&path, &json).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Force rebuild daily_stats from transcripts (source of truth).
#[tauri::command]
fn recompute_stats(state: State<'_, Arc<AppState>>) -> Result<Insights, String> {
    state.db.recompute_daily_stats()?;
    state.db.insights()
}

#[tauri::command]
fn open_privacy_settings(pane: String) -> Result<(), String> {
    permissions::open_privacy_pane(&pane)
}

#[tauri::command]
fn manual_toggle(app: AppHandle, state: State<'_, Arc<AppState>>) {
    if *state.recording.lock() {
        // A manual stop (Home button / pill / sidebar) always exits hands-free —
        // clear BOTH latches: the app-side flag (or stop_and_transcribe's guard
        // makes every on-screen Stop a no-op while locked) AND the PTT tap-side
        // latch (or the next fn gesture is swallowed resetting it).
        *state.hands_free.lock() = false;
        #[cfg(target_os = "macos")]
        ptt_macos::end_hands_free();
        stop_and_transcribe(app, state.inner().clone());
    } else {
        start_recording(&app, state.inner());
    }
}

#[tauri::command]
fn show_main(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

// --- Embedded ASR model management (YV31) ---

/// One bundled-catalog model as the model manager sees it: catalog metadata
/// merged with on-disk (downloaded) and selection state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub architecture: String,
    /// Default-quant file that gets downloaded for this model.
    pub filename: String,
    pub quant: String,
    pub size_bytes: u64,
    pub downloaded: bool,
    pub selected: bool,
    pub recommended: bool,
    pub recommended_rank: Option<u32>,
}

/// The bundled catalog merged with downloaded + selected state. Offline: the
/// catalog is compiled into the binary and "downloaded" is a file-system check.
#[tauri::command]
async fn list_models(state: State<'_, Arc<AppState>>) -> Result<Vec<ModelEntry>, String> {
    let selected = state.settings.lock().native_model.clone();
    Ok(models::catalog()
        .models
        .iter()
        .filter_map(|m| {
            let file = m.default_file()?;
            Some(ModelEntry {
                id: m.id.clone(),
                name: m.name.clone(),
                description: m.description.clone(),
                architecture: m.architecture.clone(),
                filename: file.filename.clone(),
                quant: file.quant.clone(),
                size_bytes: file.size_bytes,
                downloaded: models::is_downloaded(m),
                selected: m.id == selected,
                recommended: m.recommended,
                recommended_rank: m.recommended_rank,
            })
        })
        .collect())
}

/// Download a catalog model's default-quant file (resumable, sha256-verified),
/// emitting `model_download_progress` events. Returns the on-disk path.
#[tauri::command]
async fn download_model(app: AppHandle, id: String) -> Result<String, String> {
    let path = models::download_model(&app, &id).await?;
    log::info!("model '{id}' downloaded to {}", path.display());
    Ok(path.to_string_lossy().into_owned())
}

/// Select the embedded ASR model. Persists `native_model` and drops a warm
/// engine of a DIFFERENT model so the next use loads the new selection.
#[tauri::command]
async fn select_model(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    let model =
        models::catalog_model(&id).ok_or_else(|| format!("unknown catalog model '{id}'"))?;
    let next = {
        let mut settings = state.settings.lock();
        settings.native_model = model.id.clone();
        settings.clone()
    };
    persist_settings(&next)?;
    let _ = app.emit("settings", &next);
    if state.transcription.loaded_model().as_deref() != Some(model.id.as_str()) {
        state.transcription.unload();
    }
    log::info!("embedded ASR model selected: {}", model.id);
    Ok(())
}

/// Delete a downloaded model's file (and any interrupted `.partial`). Unloads
/// it first if it is the warm one, so the bytes aren't in use.
#[tauri::command]
async fn delete_model(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    let model =
        models::catalog_model(&id).ok_or_else(|| format!("unknown catalog model '{id}'"))?;
    if state.transcription.loaded_model().as_deref() == Some(model.id.as_str()) {
        state.transcription.unload();
    }
    models::delete_downloaded(model)?;
    log::info!("model '{}' deleted from disk", model.id);
    Ok(())
}

/// Warm-engine lifecycle snapshot: what is resident, whether a load or a
/// transcription is in flight, and how close the idle watcher is to unloading.
#[tauri::command]
async fn engine_status(
    state: State<'_, Arc<AppState>>,
) -> Result<transcription::EngineStatus, String> {
    Ok(state.transcription.status())
}

/// Open the transcript DB without panicking at launch. A transient failure
/// (another instance briefly holding the lock, disk momentarily full) is retried
/// once after a short backoff; if it still fails we degrade to an in-memory DB so
/// the app stays usable this session — history won't persist, but the process
/// never crashes on startup. (Genuine corruption is already self-healed inside
/// `Database::open` via quarantine.)
fn open_db_graceful(db_path: std::path::PathBuf) -> Database {
    match Database::open(db_path.clone()) {
        Ok(db) => db,
        Err(e) => {
            log::error!("DB open failed ({e}); retrying once after backoff");
            std::thread::sleep(std::time::Duration::from_millis(500));
            match Database::open(db_path) {
                Ok(db) => {
                    log::info!("DB opened on retry");
                    db
                }
                Err(e2) => {
                    log::error!(
                        "DB open failed again ({e2}); running with an IN-MEMORY DB — \
                         transcripts will NOT persist this session"
                    );
                    Database::open_in_memory().unwrap_or_else(|e3| {
                        // Opening an in-memory SQLite DB does not realistically fail;
                        // if it does, the sqlite runtime itself is unusable. Exit
                        // gracefully with a logged reason instead of panicking.
                        log::error!("in-memory DB fallback also failed ({e3}); exiting");
                        std::process::exit(1);
                    })
                }
            }
        }
    }
}

/// First name of the macOS account (`id -F` full name), for UI greetings.
/// Empty string when unavailable — the UI must not assume a name exists.
#[tauri::command]
fn user_display_name() -> String {
    std::process::Command::new("id")
        .arg("-F")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .and_then(|full| full.split_whitespace().next().map(str::to_string))
        .unwrap_or_default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // YV32 headless mode (`--transcribe-file <wav>`): transcribe with the
    // embedded engine, print the text, exit. Handled FIRST so no window, tray,
    // hotkey tap, or app logger is ever created for a one-shot CLI run.
    if let Some(code) = cli::handle_args() {
        std::process::exit(code);
    }

    // YV7: structured rotating file logging under data_dir()/logs/ (yap.log) +
    // a panic hook — keeps the console output and mirrors it to disk for support.
    logging::init(&data_dir());

    // YV20/M3: sweep any stale voice WAVs left by a prior crash / hard-kill (the
    // happy path deletes its own clip, but a SIGKILL mid-transcribe can't). No
    // recording is in flight at startup, so every *.wav here is safe to remove.
    let swept = record::sweep_stale_wavs(&data_dir().join("recordings"));
    if swept > 0 {
        log::info!("startup: swept {swept} stale voice wav(s) from recordings dir");
    }

    let db_path = data_dir().join("wilson_voice.db");
    let db = open_db_graceful(db_path);
    let legacy = data_dir().join("history").join("transcripts.json");
    match db.migrate_json_if_needed(legacy) {
        Ok(n) if n > 0 => log::info!("migrated {n} legacy transcripts into SQLite"),
        Err(e) => log::warn!("legacy migrate: {e}"),
        _ => {}
    }
    let db = Arc::new(db);

    let settings: AppSettings = {
        let p = data_dir().join("settings.json");
        std::fs::read_to_string(p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    };

    let state = Arc::new(AppState {
        settings: PLMutex::new(settings),
        recording: PLMutex::new(false),
        busy: PLMutex::new(false),
        hands_free: PLMutex::new(false),
        recorder: PLMutex::new(None),
        saved_audio: PLMutex::new(None),
        db,
        last_error: PLMutex::new(None),
        hotkey_registered: PLMutex::new(false),
        tray: PLMutex::new(None),
        tray_dictation: PLMutex::new(None),
        tray_hands_free: PLMutex::new(None),
        vad: PLMutex::new(None),
        transcription: transcription::TranscriptionManager::new(),
    });

    // YV36: fetch the Silero v4 VAD model ONCE in the background (never
    // HuggingFace — a direct URL, sha256-checked, cached under Application
    // Support, then OFFLINE forever), then build the WARM VAD once and park it
    // in the app state. Both steps are off the hot path so they never delay a
    // dictation, and a failure at either step simply leaves `state.vad` as
    // `None` → the capture pipeline stays on the energy VAD.
    {
        let state = state.clone();
        std::thread::spawn(move || {
            let Some(path) = vad::ensure_model(&data_dir()) else {
                log::warn!("startup: no silero VAD model — energy VAD only");
                return;
            };
            match vad::WarmVad::load(&path) {
                Ok(warm) => {
                    *state.vad.lock() = Some(Arc::new(warm));
                    log::info!("startup: warm silero VAD loaded from {}", path.display());
                }
                Err(e) => log::warn!("startup: silero VAD load failed, energy VAD only: {e}"),
            }
        });
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        // Auto-update: checks the GitHub Releases updater manifest (see
        // tauri.conf.json plugins.updater) and installs signed DMG updates.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Real NSPanel support for Dictate island (macOS)
        .plugin(tauri_nspanel::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler({
                    let state = state.clone();
                    move |app, shortcut, event| {
                        // ⌃⌘V — Paste Last Transcript (Wispr-parity, always on,
                        // independent of the ⌘⇧V dictation toggle below).
                        let paste_last_sc =
                            Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SUPER), Code::KeyV);
                        if shortcut == &paste_last_sc {
                            if event.state == ShortcutState::Pressed {
                                log::info!("⌃⌘V pressed — paste last transcript");
                                paste_last_transcript(app, &state);
                            }
                            return;
                        }
                        // Legacy secondary: ⌘⇧V (only if keep_cmd_shift_v)
                        if !state.settings.lock().keep_cmd_shift_v {
                            return;
                        }
                        log::debug!("shortcut event {shortcut:?} {:?}", event.state);
                        if event.state == ShortcutState::Pressed {
                            log::info!("⌘⇧V pressed — start");
                            start_recording(app, &state);
                        } else if event.state == ShortcutState::Released {
                            log::info!("⌘⇧V released — stop");
                            stop_and_transcribe(app.clone(), state.clone());
                        }
                    }
                })
                .build(),
        )
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            user_display_name,
            get_settings,
            save_settings,
            get_history,
            clear_history,
            delete_entry,
            get_status,
            get_permissions,
            request_accessibility,
            request_microphone,
            show_float_pill,
            hide_float_pill,
            get_insights,
            daily_series,
            monthly_series,
            list_dictionary,
            add_dictionary_term,
            delete_dictionary_term,
            list_scratch,
            save_scratch,
            delete_scratch,
            copy_entry,
            paste_entry,
            open_data_dir,
            open_logs_dir,
            export_history,
            recompute_stats,
            open_privacy_settings,
            manual_toggle,
            show_main,
            list_models,
            download_model,
            select_model,
            delete_model,
            engine_status
        ])
        .setup(move |app| {
            // Lightweight setup only — no hotkey register, no second window

            // Regular app so Yap shows in the Dock, Launchpad, and Applications — it
            // has a real main window (and a social layer coming), not just a menubar
            // agent. The floating Dictate pill STILL hovers over other apps' fullscreen
            // Spaces: that comes from the NSPanel's collection behavior
            // (canJoinAllSpaces + fullScreenAuxiliary + nonactivating, see float_pill.rs),
            // which works regardless of activation policy — not from being .accessory.
            // LSUIElement is false in Info.plist to match.
            #[cfg(target_os = "macos")]
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);

            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
                let w = win.clone();
                win.on_window_event(move |e| {
                    if let WindowEvent::CloseRequested { api, .. } = e {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

            // Menu-bar dropdown (Wispr parity): a full control surface so Yap is
            // driveable from the toolbar as well as the pill + hotkey (YV26).
            // `toggle` label + `hands_free` check are refreshed live by sync_tray.
            let recording_now = *state.recording.lock();
            let dictation_i = MenuItem::with_id(
                app,
                "toggle",
                if recording_now {
                    "Stop Dictation"
                } else {
                    "Start Dictation"
                },
                true,
                None::<&str>,
            )?;
            let hands_free_i = CheckMenuItem::with_id(
                app,
                "hands_free",
                "Hands-free",
                true,
                *state.hands_free.lock(),
                None::<&str>,
            )?;
            // "Paste Last Transcript" (⌃⌘V) — re-injects the newest transcript,
            // the marquee Wispr-parity action. The accelerator is also registered
            // globally below so it fires anywhere, not just when the menu is open.
            let paste_last_i = MenuItem::with_id(
                app,
                "paste_last",
                "Paste Last Transcript",
                true,
                Some("Ctrl+Cmd+V"),
            )?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let show_i = MenuItem::with_id(app, "show", "Open Yap", true, None::<&str>)?;
            let settings_i = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let shortcuts_i =
                MenuItem::with_id(app, "shortcuts", "Keyboard Shortcuts…", true, None::<&str>)?;
            let sep3 = PredefinedMenuItem::separator(app)?;
            // Feedback/report affordance (Wispr's "Share feedback" / "Talk to
            // support") — opens the repo's new-issue page in the browser.
            let feedback_i =
                MenuItem::with_id(app, "feedback", "Send Feedback…", true, None::<&str>)?;
            let sep4 = PredefinedMenuItem::separator(app)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit Yap", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &dictation_i,
                    &paste_last_i,
                    &sep1,
                    &hands_free_i,
                    &sep2,
                    &show_i,
                    &settings_i,
                    &shortcuts_i,
                    &sep3,
                    &feedback_i,
                    &sep4,
                    &quit_i,
                ],
            )?;
            // Keep handles so state transitions can reflect into the dropdown.
            *state.tray_dictation.lock() = Some(dictation_i);
            *state.tray_hands_free.lock() = Some(hands_free_i);
            // Menu-bar TEMPLATE icon: a monochrome Yappy silhouette. `icon_as_template`
            // lets macOS tint it for the light/dark menu bar — a full-color app icon
            // crammed into the status bar looks wrong (that was the "terrible toolbar").
            let icon = tauri::image::Image::from_bytes(include_bytes!(
                "../icons/tray-template.png"
            ))
            .map_err(|e| format!("tray template icon: {e}"))?;

            let tray = TrayIconBuilder::new()
                .icon(icon)
                .icon_as_template(true)
                .menu(&menu)
                // Left-click opens the dropdown (Wispr behavior). Previously a
                // left-click only toggled the main window and the menu hid on
                // right-click, so it read as "no dropdown". The window is now
                // reached via the "Open Yap" item.
                .show_menu_on_left_click(true)
                .tooltip("Yap — hold fn to dictate")
                .on_menu_event({
                    let state = state.clone();
                    move |app, event| match event.id.as_ref() {
                        "quit" => {
                            state.db.checkpoint(); // flush WAL before we exit
                            app.exit(0);
                        }
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "settings" => {
                            // Open the app AND jump it to the Settings view.
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                            let _ = app.emit("navigate", "settings");
                        }
                        "paste_last" => {
                            paste_last_transcript(app, &state);
                        }
                        "shortcuts" => {
                            // Open Settings and hint the Shortcut sub-tab.
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                            let _ = app.emit("navigate", "settings");
                            let _ = app.emit("settings-tab", "shortcut");
                        }
                        "feedback" => {
                            // Open the repo's new-issue page in the default browser.
                            let _ = std::process::Command::new("open")
                                .arg("https://github.com/wilsonguenther-dev/wilson-voice/issues/new")
                                .spawn();
                        }
                        "hands_free" => {
                            if *state.hands_free.lock() {
                                // Turning hands-free OFF finalizes the take, mirroring
                                // the physical "tap-to-end-hands-free" path — clear
                                // both the app-side and PTT tap-side latches, then
                                // transcribe what was captured.
                                *state.hands_free.lock() = false;
                                #[cfg(target_os = "macos")]
                                ptt_macos::end_hands_free();
                                stop_and_transcribe(app.clone(), state.clone());
                            } else {
                                // Turning hands-free ON: ensure a take is running,
                                // then latch it so key-release / stop won't finalize.
                                if !*state.recording.lock() && !*state.busy.lock() {
                                    start_recording(app, &state);
                                }
                                if *state.recording.lock() {
                                    *state.hands_free.lock() = true;
                                }
                                emit_status(app, &state);
                            }
                        }
                        "toggle" => {
                            if *state.recording.lock() {
                                // Tray Stop also exits hands-free (see manual_toggle) —
                                // clear both the app-side and PTT tap-side latches.
                                *state.hands_free.lock() = false;
                                #[cfg(target_os = "macos")]
                                ptt_macos::end_hands_free();
                                stop_and_transcribe(app.clone(), state.clone());
                            } else {
                                start_recording(app, &state);
                            }
                        }
                        _ => {}
                    }
                })
                .build(app)?;
            // Retain the tray so its tooltip can be updated on state changes and
            // so it isn't dropped when this setup scope ends.
            *state.tray.lock() = Some(tray);

            emit_status(app.handle(), &state);

            // Glass island: always-on + rides all Spaces (re-park loop)
            if state.settings.lock().show_floating_pill {
                if let Err(e) = float_pill::show_float(app.handle()) {
                    log::warn!("float pill: {e}");
                }
                float_pill::start_space_keeper(app.handle().clone());
            } else {
                float_pill::hide_float(app.handle());
            }

            if !permissions::is_accessibility_trusted() {
                log::info!(
                    "Accessibility not trusted — fn⌃ needs it. Enable Wilson Voice in Privacy → Accessibility"
                );
            }

            // Primary: fn⌃ hybrid (hold PTT / double-tap hands-free)
            #[cfg(target_os = "macos")]
            {
                let binding = ptt_macos::PttBinding::from_settings(
                    &state.settings.lock().ptt_binding,
                );
                let st = state.clone();
                let h = app.handle().clone();
                ptt_macos::start(
                    binding,
                    Arc::new(move |ev| {
                        let st = st.clone();
                        let h = h.clone();
                        if let Err(e) = h.clone().run_on_main_thread(move || match ev {
                            ptt_macos::PttEvent::Start => {
                                start_recording(&h, &st);
                            }
                            ptt_macos::PttEvent::Stop => {
                                *st.hands_free.lock() = false;
                                stop_and_transcribe(h.clone(), st.clone());
                            }
                            ptt_macos::PttEvent::Interrupted => {
                                *st.hands_free.lock() = false;
                                cancel_recording(&h, &st);
                            }
                            ptt_macos::PttEvent::HandsFreeOn => {
                                *st.hands_free.lock() = true;
                                emit_status(&h, &st);
                                float_pill::show_for_recording(&h);
                                log::info!("hands-free ON");
                            }
                            ptt_macos::PttEvent::HandsFreeOff => {
                                *st.hands_free.lock() = false;
                                emit_status(&h, &st);
                                log::info!("hands-free OFF");
                            }
                        }) {
                            log::error!("PTT main-thread hop failed: {e}");
                        }
                    }),
                );
                *state.hotkey_registered.lock() = true;
                log::info!("PTT hybrid started ({})", binding.label());
            }

            // Global ⌃⌘V (Paste Last Transcript, always on) + optional ⌘⇧V
            // dictation toggle (off by default).
            let handle = app.handle().clone();
            let state_hk = state.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(600));
                let h = handle.clone();
                let st = state_hk.clone();
                let _ = handle.run_on_main_thread(move || {
                    // ⌃⌘V — Paste Last Transcript, registered unconditionally so
                    // the tray item's accelerator fires system-wide (Wispr-parity).
                    let paste_sc =
                        Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SUPER), Code::KeyV);
                    match h.global_shortcut().register(paste_sc) {
                        Ok(()) => log::info!("⌃⌘V paste-last registered"),
                        Err(e) => log::warn!("⌃⌘V register failed: {e}"),
                    }
                    if !st.settings.lock().keep_cmd_shift_v {
                        log::info!("⌘⇧V secondary disabled");
                        emit_status(&h, &st);
                        return;
                    }
                    let sc = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyV);
                    match h.global_shortcut().register(sc) {
                        Ok(()) => {
                            log::info!("secondary ⌘⇧V registered");
                            emit_status(&h, &st);
                        }
                        Err(e) => {
                            log::warn!("⌘⇧V register failed: {e}");
                            emit_status(&h, &st);
                        }
                    }
                });
            });

            log::info!("Yap v0.6.0 — NSPanel Dictate island (tauri-nspanel)");
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Wilson Voice")
        .run(|app_handle, event| {
            // Checkpoint the WAL on exit so it never grows unbounded and the .db
            // isn't left a deceptive 4 KB stub. Covers Cmd-Q / window-driven exit;
            // the tray Quit item also checkpoints before app.exit(0).
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app_handle.try_state::<Arc<AppState>>() {
                    // YV28 safety net: never leave the Mac muted if we exit
                    // mid-take. Restores the saved output state (no-op otherwise).
                    restore_system_output(&state);
                    state.db.checkpoint();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{status_message, AppSettings};

    // YV33: "Ready" is a claim about being able to transcribe. With no usable
    // model the status line must say a model is needed — the old rule ("a file
    // exists at the resolved interpreter path") reported Ready on every fresh
    // Mac, where that path was the non-functional Command Line Tools shim.
    #[test]
    fn status_says_model_needed_until_a_model_is_ready() {
        let no_model = status_message(false, false, false, None, false, true, "fn⌃");
        assert!(no_model.contains("Model needed"), "{no_model}");
        assert!(!no_model.contains("Ready"), "{no_model}");

        let ready = status_message(false, false, false, None, true, true, "fn⌃");
        assert!(ready.starts_with("Ready — hold fn⌃"), "{ready}");

        // Accessibility is a paste-only concern: still Ready, with a nudge.
        let no_ax = status_message(false, false, false, None, true, false, "fn⌃");
        assert!(no_ax.starts_with("Ready —"), "{no_ax}");
        assert!(no_ax.contains("Accessibility"), "{no_ax}");

        // Live states and hard errors still outrank the model gate.
        assert!(status_message(true, false, false, None, false, true, "fn⌃").contains("Recording"));
        assert!(status_message(true, true, false, None, false, true, "fn⌃").contains("Hands-free"));
        assert!(
            status_message(false, false, true, None, false, true, "fn⌃").contains("Transcribing")
        );
        let err = status_message(false, false, false, Some("boom"), true, true, "fn⌃");
        assert_eq!(err, "Error: boom");
    }

    // YV9: onboarding gate must default to false so a fresh install shows the
    // first-run flow, and older settings.json (written before the field existed)
    // must deserialize with onboarded=false via serde default — never panic.
    #[test]
    fn onboarded_defaults_false_and_is_backward_compatible() {
        assert!(!AppSettings::default().onboarded);
        assert!(AppSettings::default().calibration_sample.is_none());

        // Legacy settings JSON with no `onboarded` / `calibrationSample` keys.
        // YV34: it also still carries the retired sidecar keys (`model`,
        // `speedProfile`), which must be ignored rather than fail the parse —
        // every existing install has them on disk.
        let legacy = r#"{
            "model": "whisper-large-v3-turbo",
            "speedProfile": "balanced",
            "language": "en",
            "autoPaste": true,
            "hotkeyLabel": "fn⌃",
            "showFloatingPill": true
        }"#;
        let parsed: AppSettings = serde_json::from_str(legacy).expect("legacy parse");
        assert!(!parsed.onboarded);
        assert!(parsed.calibration_sample.is_none());
        // YV10: cleanup_level defaults to "light" for fresh installs and legacy JSON.
        assert_eq!(AppSettings::default().cleanup_level, "light");
        assert_eq!(parsed.cleanup_level, "light");
        // YV12: denoise defaults ON for fresh installs and legacy JSON (serde default).
        assert!(AppSettings::default().denoise);
        assert!(parsed.denoise);
        // YV27: companion_tone defaults to "friendly" for fresh installs and
        // legacy JSON (written before the field existed) via serde default.
        assert_eq!(AppSettings::default().companion_tone, "friendly");
        assert_eq!(parsed.companion_tone, "friendly");

        // A finished onboarding round-trips (camelCase key on the wire).
        let mut done = AppSettings::default();
        done.onboarded = true;
        done.calibration_sample = Some("the quick brown fox".into());
        let json = serde_json::to_string(&done).expect("serialize");
        assert!(json.contains("\"onboarded\":true"));
        assert!(json.contains("\"calibrationSample\":\"the quick brown fox\""));
        let back: AppSettings = serde_json::from_str(&json).expect("round-trip");
        assert!(back.onboarded);
        assert_eq!(
            back.calibration_sample.as_deref(),
            Some("the quick brown fox")
        );

        // YV27: a chosen companion tone round-trips on the camelCase wire key.
        let mut toned = AppSettings::default();
        toned.companion_tone = "rude".into();
        let tjson = serde_json::to_string(&toned).expect("serialize tone");
        assert!(tjson.contains("\"companionTone\":\"rude\""));
        let tback: AppSettings = serde_json::from_str(&tjson).expect("tone round-trip");
        assert_eq!(tback.companion_tone, "rude");

        // YV31: nativeModel defaults to the catalog's recommended model for
        // fresh installs AND for legacy JSON written before the field existed,
        // and round-trips on its camelCase wire key.
        let recommended = crate::models::recommended_model().id.clone();
        assert_eq!(AppSettings::default().native_model, recommended);
        assert_eq!(parsed.native_model, recommended);
        let mut native = AppSettings::default();
        native.native_model = "handy-computer/whisper-tiny-gguf".into();
        let njson = serde_json::to_string(&native).expect("serialize native model");
        assert!(njson.contains("\"nativeModel\":\"handy-computer/whisper-tiny-gguf\""));
        let nback: AppSettings = serde_json::from_str(&njson).expect("native round-trip");
        assert_eq!(nback.native_model, "handy-computer/whisper-tiny-gguf");
    }
}
