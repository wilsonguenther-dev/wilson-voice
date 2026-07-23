//! Wilson Voice — production dictation desktop app
//!
//! Hotkeys: primary FN / FN+Control via CGEvent tap (ptt_macos).
//! Optional legacy ⌘⇧V via Carbon global-shortcut.
//! HUD: floating float.html pill, parked bottom-center (no cursor chase).

mod asr;
mod asr_paths;
mod db;
mod dictation;
mod float_pill;
mod focus;
mod logging;
mod mic_auth;
mod paste;
mod permissions;
#[cfg(target_os = "macos")]
mod ptt_macos;
mod record;

use db::{Database, DictEntry, Insights, ScratchNote, TranscriptEntry};
use parking_lot::Mutex as PLMutex;
use permissions::PermissionReport;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub model: String,
    pub language: String,
    pub auto_paste: bool,
    pub hotkey_label: String,
    /// Always show HUD; also auto-shows while recording even if false.
    pub show_floating_pill: bool,
    /// fast | balanced | max — maps to model; user can still override model.
    #[serde(default = "default_speed_profile")]
    pub speed_profile: String,
    /// fn | fn_control | both — CGEvent FN PTT (not Carbon).
    #[serde(default = "default_ptt_binding")]
    pub ptt_binding: String,
    /// Keep Carbon ⌘⇧V as secondary hold binding.
    #[serde(default = "default_true")]
    pub keep_cmd_shift_v: bool,
    /// Floating pill style: "classic" (obsidian capsule) | "yappy" (pixel pet).
    #[serde(default = "default_pill_style")]
    pub pill_style: String,
    /// Smart-dictation mode: auto | plain | list | email | code | notes.
    /// "auto" infers the mode from the frontmost app; any other value forces it.
    #[serde(default = "default_dictation_mode")]
    pub dictation_mode: String,
}

fn default_speed_profile() -> String {
    "balanced".into()
}
fn default_ptt_binding() -> String {
    "fn_control".into()
}
fn default_pill_style() -> String {
    "classic".into()
}
fn default_dictation_mode() -> String {
    "auto".into()
}
fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            // Balanced: large-v3-turbo — warm daemon keeps it snappy
            model: "mlx-community/whisper-large-v3-turbo".into(),
            language: "en".into(),
            auto_paste: true,
            hotkey_label: "fn⌃".into(),
            // Always-on glass island rides all Spaces
            show_floating_pill: true,
            speed_profile: "balanced".into(),
            ptt_binding: "fn_control".into(),
            keep_cmd_shift_v: false,
            pill_style: "classic".into(),
            dictation_mode: "auto".into(),
        }
    }
}

/// Resolve model from speed profile when user picks a profile button.
/// IDs must match real mlx-community HF repos (…-mlx for most sizes).
pub fn model_for_profile(profile: &str) -> &'static str {
    match profile {
        "fast" => "mlx-community/whisper-small-mlx",
        "max" => "mlx-community/whisper-large-v3-mlx",
        _ => "mlx-community/whisper-large-v3-turbo", // balanced
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub recording: bool,
    pub busy: bool,
    pub last_error: Option<String>,
    pub message: String,
    pub python_ok: bool,
    pub worker_ok: bool,
    pub accessibility: bool,
    pub hotkey_registered: bool,
    #[serde(default)]
    pub hands_free: bool,
}

struct AppState {
    settings: PLMutex<AppSettings>,
    recording: PLMutex<bool>,
    busy: PLMutex<bool>,
    /// Hands-free latch (double-tap fn⌃); release keys but keep recording.
    hands_free: PLMutex<bool>,
    recorder: PLMutex<Option<record::ActiveRecording>>,
    db: Arc<Database>,
    last_error: PLMutex<Option<String>>,
    venv_python: PathBuf,
    asr_worker: PathBuf,
    hotkey_registered: PLMutex<bool>,
}

fn data_dir() -> PathBuf {
    let p = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("WilsonVoice");
    let _ = std::fs::create_dir_all(&p);
    let _ = std::fs::create_dir_all(p.join("recordings"));
    p
}

fn build_status(state: &AppState) -> AppStatus {
    let recording = *state.recording.lock();
    let busy = *state.busy.lock();
    let hands_free = *state.hands_free.lock();
    let last_error = state.last_error.lock().clone();
    let accessibility = permissions::is_accessibility_trusted();
    let hotkey_registered = *state.hotkey_registered.lock();
    let ptt = state.settings.lock().hotkey_label.clone();
    let message = if recording && hands_free {
        format!("Hands-free… tap {ptt} to stop")
    } else if recording {
        format!("Recording… release {ptt} or click Stop")
    } else if busy {
        "Transcribing with local Whisper…".into()
    } else if let Some(ref err) = last_error {
        format!("Error: {err}")
    } else if !state.venv_python.exists() {
        "ASR venv missing — see Permissions".into()
    } else if !accessibility {
        format!("Ready — {ptt} (enable Accessibility)")
    } else {
        format!("Ready — hold {ptt} · double-tap hands-free")
    };
    AppStatus {
        recording,
        busy,
        last_error,
        message,
        python_ok: state.venv_python.exists(),
        worker_ok: state.asr_worker.exists(),
        accessibility,
        hotkey_registered,
        hands_free,
    }
}

fn emit_status(app: &AppHandle, state: &AppState) {
    let _ = app.emit("status", &build_status(state));
}

fn notify(app: &AppHandle, title: &str, body: impl Into<String>) {
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(body.into())
        .show();
}

fn start_recording(app: &AppHandle, state: &AppState) {
    if *state.recording.lock() || *state.busy.lock() {
        return;
    }
    // Do NOT call mic_auth::request_microphone_access here — that is Permissions-only.
    // Opening the real capture stream is enough for TCC (Allow once after install).
    match record::start_recording(data_dir().join("recordings")) {
        Ok(active) => {
            let level = active.level.clone();
            let stop_flag = active.stop_flag();
            *state.recorder.lock() = Some(active);
            *state.recording.lock() = true;
            *state.last_error.lock() = None;
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
    let _ = app.emit("recording", false);
    if let Some(active) = state.recorder.lock().take() {
        let path = active.wav_path.clone();
        let _ = record::stop_recording(active);
        let _ = std::fs::remove_file(path);
    }
    *state.last_error.lock() = Some("Dictation cancelled (key while holding)".into());
    emit_status(app, state);
    float_pill::after_recording(app, state.settings.lock().show_floating_pill);
    log::info!("recording cancelled");
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
    let _ = app.emit("recording", false);

    let active = state.recorder.lock().take();
    let Some(active) = active else {
        return;
    };

    *state.busy.lock() = true;
    emit_status(&app, &state);

    let settings = state.settings.lock().clone();
    let venv = state.venv_python.clone();
    let worker = state.asr_worker.clone();
    let db = state.db.clone();
    let app2 = app.clone();
    let state2 = state.clone();
    // Capture focused app *before* we steal focus with notifications / main window.
    let source_app = focus::frontmost_app_name();
    let t_release = std::time::Instant::now();

    std::thread::spawn(move || {
        let result = (|| -> Result<(TranscriptEntry, paste::PasteOutcome, i64), String> {
            let rec = record::stop_recording(active)?;
            let t_wav = t_release.elapsed().as_millis() as i64;
            log::info!(
                "wav ready: {} speech={:.2}s hold_wall={:.2}s wav_ms={}",
                rec.wav_path.display(),
                rec.speech_seconds,
                rec.hold_wall_seconds,
                t_wav
            );
            // Bias Whisper toward the user's learned vocabulary/jargon BEFORE it
            // decodes (initial_prompt), ordered most-frequent-last.
            let vocab = db.top_dictionary_terms(60).unwrap_or_default();
            let asr = asr::run_asr(
                &venv,
                &worker,
                &rec.wav_path,
                &settings.model,
                &settings.language,
                &vocab,
            )?;
            let t_asr = t_release.elapsed().as_millis() as i64;
            let text = db.apply_dictionary(&asr.text).unwrap_or(asr.text);
            // Smart dictation (YV5): resolve the effective mode — a user-picked fixed mode
            // wins, otherwise it's inferred from the focused app — and format the transcript
            // BEFORE it's stored/pasted. `apply_dictation` is guarded so it can never lose
            // text (falls back to the raw transcript on any error/empty result).
            let dictation_mode = dictation::resolve_mode(
                &settings.dictation_mode,
                source_app.as_deref().unwrap_or_default(),
            );
            let text = if dictation::should_format(dictation_mode) {
                let formatted = dictation::format_dictation(&text);
                // Guard: never lose text — if formatting yields nothing from a non-empty
                // transcript, fall back to the raw text.
                if formatted.trim().is_empty() && !text.trim().is_empty() {
                    text
                } else {
                    formatted
                }
            } else {
                // Code / Plain modes stay verbatim.
                text
            };
            log::info!(
                "smart-dictation: mode={:?} setting={}",
                dictation_mode,
                settings.dictation_mode
            );
            // Always copy first (Wispr Flow: never lose text)
            let want_paste = settings.auto_paste && focus::should_auto_paste();
            let outcome = paste::copy_and_maybe_paste(&app2, &text, want_paste);
            // North-star metric: release hotkey → text on clipboard
            let pipeline_ms = t_release.elapsed().as_millis() as i64;
            log::info!(
                "latency hold→clipboard={}ms (wav={} asr_done={} asr_model={:.0}ms) backend={}",
                pipeline_ms,
                t_wav,
                t_asr,
                asr.seconds * 1000.0,
                asr.backend
            );
            let entry = db.insert_transcript(
                text,
                asr.backend,
                asr.seconds,
                rec.speech_seconds,
                pipeline_ms,
                source_app,
            )?;
            // Hygiene: drop wav after successful ASR (audio stays local only during process)
            let _ = std::fs::remove_file(&rec.wav_path);
            Ok((entry, outcome, pipeline_ms))
        })();

        match result {
            Ok((entry, outcome, pipeline_ms)) => {
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
            Err(e) => {
                log::error!("pipeline failed: {e}");
                *state2.last_error.lock() = Some(e.clone());
                notify(&app2, "Yap — Failed", e);
            }
        }
        *state2.busy.lock() = false;
        *state2.hands_free.lock() = false;
        // Keep island on all Spaces if always-on; else hide after take
        float_pill::after_recording(&app2, state2.settings.lock().show_floating_pill);
        emit_status(&app2, &state2);
    });
}

// --- Commands ---

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
    // Keep model aligned with speed profile when profile is one of the known names
    if matches!(next.speed_profile.as_str(), "fast" | "balanced" | "max") {
        let want = model_for_profile(&next.speed_profile);
        // Only auto-map if current model is one of our known profile models (don't clobber custom)
        let known = [
            "mlx-community/whisper-small-mlx",
            "mlx-community/whisper-base-mlx",
            "mlx-community/whisper-tiny-mlx",
            "mlx-community/whisper-large-v3-turbo",
            "mlx-community/whisper-large-v3-mlx",
            "mlx-community/whisper-medium-mlx",
            // legacy aliases from earlier builds
            "mlx-community/whisper-small",
            "mlx-community/whisper-base",
            "mlx-community/whisper-large-v3",
            "mlx-community/whisper-medium",
        ];
        if known.iter().any(|m| *m == next.model) {
            next.model = want.into();
        }
    }
    // Keep label in sync with binding
    next.hotkey_label = match next.ptt_binding.as_str() {
        "fn" => "fn".into(),
        "both" | "fn_or_fn_control" => "fn / fn⌃".into(),
        _ => "fn⌃".into(),
    };
    *state.settings.lock() = next.clone();
    let path = data_dir().join("settings.json");
    let s = serde_json::to_string_pretty(&next).map_err(|e| e.to_string())?;
    std::fs::write(path, s).map_err(|e| e.to_string())?;
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
    // Re-preload the active model in background (warm daemon)
    asr::preload_async(
        state.venv_python.clone(),
        state.asr_worker.clone(),
        next.model.clone(),
        next.language.clone(),
    );
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
    permissions::report(&state.venv_python, &state.asr_worker, false)
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

#[tauri::command]
async fn paste_entry(app: AppHandle, text: String) -> Result<String, String> {
    // MUST be async → runs OFF the main thread. copy_and_maybe_paste hops to the
    // main thread for the CGEvent ⌘V and blocks waiting on it; a *sync* command
    // runs ON main and would deadlock waiting for a closure queued to that same
    // thread (3s freeze, then "paste timed out"). spawn_blocking keeps the async
    // runtime unblocked while the short main-thread hop completes.
    tauri::async_runtime::spawn_blocking(move || {
        let o = paste::copy_and_maybe_paste(&app, &text, true);
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

#[tauri::command]
fn setup_asr_venv() -> Result<String, String> {
    asr_paths::setup_local_venv()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // YV7: structured rotating file logging under data_dir()/logs/ (yap.log) +
    // a panic hook — keeps the console output and mirrors it to disk for support.
    logging::init(&data_dir());

    // NEVER resolve ASR to ~/Desktop — that triggers Desktop folder TCC on every stop.
    // Prefer pre-built Application Support venv; seed worker file only (no Desktop read).
    let (venv_py, worker) = asr_paths::resolve_paths();
    if !venv_py.exists() || !asr_paths::python_has_mlx(&venv_py) {
        log::warn!(
            "ASR venv not ready at {} — user should click Install local ASR once",
            venv_py.display()
        );
    } else {
        log::info!(
            "ASR ready python={} worker={}",
            venv_py.display(),
            worker.display()
        );
    }
    let db_path = data_dir().join("wilson_voice.db");
    let db = Database::open(db_path).expect("sqlite open failed");
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

    // Warm ASR daemon + preload model in background (biggest speed win)
    if venv_py.exists() {
        asr::preload_async(
            venv_py.clone(),
            worker.clone(),
            settings.model.clone(),
            settings.language.clone(),
        );
    }

    let state = Arc::new(AppState {
        settings: PLMutex::new(settings),
        recording: PLMutex::new(false),
        busy: PLMutex::new(false),
        hands_free: PLMutex::new(false),
        recorder: PLMutex::new(None),
        db,
        last_error: PLMutex::new(None),
        venv_python: venv_py,
        asr_worker: worker,
        hotkey_registered: PLMutex::new(false),
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::new().build())
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
            setup_asr_venv,
            get_insights,
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
            show_main
        ])
        .setup(move |app| {
            // Lightweight setup only — no hotkey register, no second window

            // Accessory (agent) app: no Dock icon AND — the actual fix — lets the
            // NSPanel Dictate island float over OTHER apps' fullscreen Spaces in a
            // packaged build (a .regular app cannot). Pairs with LSUIElement=true.
            #[cfg(target_os = "macos")]
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);

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

            let show_i = MenuItem::with_id(app, "show", "Open Yap", true, None::<&str>)?;
            let toggle_i =
                MenuItem::with_id(app, "toggle", "Start / Stop dictation", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &toggle_i, &quit_i])?;
            let icon = app.default_window_icon().cloned().ok_or("missing app icon")?;

            let _tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .show_menu_on_left_click(false)
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
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            if w.is_visible().unwrap_or(false) {
                                let _ = w.hide();
                            } else {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

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

            // Optional secondary ⌘⇧V (off by default)
            let handle = app.handle().clone();
            let state_hk = state.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(600));
                let h = handle.clone();
                let st = state_hk.clone();
                let _ = handle.run_on_main_thread(move || {
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

            log::info!("Wilson Voice v0.5.5 — NSPanel Dictate island (tauri-nspanel)");
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
                    state.db.checkpoint();
                }
            }
        });
}
