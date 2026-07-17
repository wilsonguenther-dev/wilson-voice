//! Wilson Voice — product-grade local dictation (Wispr Flow alternative)
//!
//! Stack (researched from Handy, VoiceInk, OpenWhispr, Muesli, sflow):
//! - Tauri 2 + React UI (native shell, tray, multi-window)
//! - Rust for hotkeys / clipboard paste / recording
//! - SQLite WAL + FTS5 for history, dictionary, insights, scratchpad
//! - Python MLX Whisper sidecar for on-device ASR
//!
//! GraphQL is not used: single-user local app → typed Tauri commands over SQLite
//! is faster and simpler than an HTTP GraphQL layer.

mod asr;
mod db;
mod paste;
mod record;

use db::{Database, DictEntry, Insights, ScratchNote, TranscriptEntry};
use parking_lot::Mutex as PLMutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent,
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
    pub show_floating: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            model: "mlx-community/whisper-large-v3-turbo".into(),
            language: "en".into(),
            auto_paste: true,
            hotkey_label: "⌥Space hold · ⌘⇧V hold".into(),
            // Off by default — second always-on-top webview caused freezes on some Macs
            show_floating: false,
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
    pub python_ok: bool,
    pub worker_ok: bool,
}

struct AppState {
    settings: PLMutex<AppSettings>,
    recording: PLMutex<bool>,
    busy: PLMutex<bool>,
    recorder: PLMutex<Option<record::ActiveRecording>>,
    db: Arc<Database>,
    last_error: PLMutex<Option<String>>,
    venv_python: PathBuf,
    asr_worker: PathBuf,
}

fn data_dir() -> PathBuf {
    let p = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("WilsonVoice");
    let _ = std::fs::create_dir_all(&p);
    let _ = std::fs::create_dir_all(p.join("recordings"));
    p
}

fn status_message(state: &AppState) -> String {
    if *state.recording.lock() {
        "Recording… release hotkey to transcribe".into()
    } else if *state.busy.lock() {
        "Transcribing with local Whisper…".into()
    } else if let Some(err) = state.last_error.lock().clone() {
        format!("Error: {err}")
    } else if !state.venv_python.exists() {
        "Python ASR venv missing — open Settings".into()
    } else {
        "Ready — hold ⌥Space to dictate".into()
    }
}

fn emit_status(app: &AppHandle, state: &AppState) {
    let status = AppStatus {
        recording: *state.recording.lock(),
        busy: *state.busy.lock(),
        last_error: state.last_error.lock().clone(),
        message: status_message(state),
        python_ok: state.venv_python.exists(),
        worker_ok: state.asr_worker.exists(),
    };
    let _ = app.emit("status", &status);
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
    match record::start_recording(data_dir().join("recordings")) {
        Ok(active) => {
            *state.recorder.lock() = Some(active);
            *state.recording.lock() = true;
            *state.last_error.lock() = None;
            log::info!("recording started");
            let _ = app.emit("recording", true);
            emit_status(app, state);
            notify(app, "Wilson Voice", "Listening — release hotkey when done");
        }
        Err(e) => {
            log::error!("record start failed: {e}");
            *state.last_error.lock() = Some(e.clone());
            emit_status(app, state);
            notify(app, "Wilson Voice — Mic error", e);
        }
    }
}

fn stop_and_transcribe(app: AppHandle, state: Arc<AppState>) {
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

    std::thread::spawn(move || {
        let result = (|| -> Result<TranscriptEntry, String> {
            let wav = record::stop_recording(active)?;
            log::info!("wav ready: {}", wav.display());
            let asr = asr::run_asr(&venv, &worker, &wav, &settings.model, &settings.language)?;
            let text = db.apply_dictionary(&asr.text).unwrap_or(asr.text);
            let entry = db.insert_transcript(text, asr.backend, asr.seconds, None)?;
            if let Err(e) = paste::copy_text(&app2, &entry.text) {
                log::warn!("clipboard: {e}");
            }
            if settings.auto_paste {
                std::thread::sleep(std::time::Duration::from_millis(120));
                if let Err(e) = paste::paste_frontmost() {
                    log::warn!("paste: {e}");
                    *state2.last_error.lock() =
                        Some(format!("Paste failed — grant Accessibility: {e}"));
                }
            }
            Ok(entry)
        })();

        match result {
            Ok(entry) => {
                *state2.last_error.lock() = None;
                let _ = app2.emit("transcript", &entry);
                let preview = if entry.text.chars().count() > 100 {
                    let s: String = entry.text.chars().take(100).collect();
                    format!("{s}…")
                } else {
                    entry.text.clone()
                };
                notify(&app2, "Wilson Voice", preview);
                log::info!("transcript ok words={}", entry.word_count);
            }
            Err(e) => {
                log::error!("pipeline failed: {e}");
                *state2.last_error.lock() = Some(e.clone());
                notify(&app2, "Wilson Voice — Failed", e);
            }
        }
        *state2.busy.lock() = false;
        emit_status(&app2, &state2);
    });
}

// --- Commands ---

#[tauri::command]
fn get_settings(state: State<'_, Arc<AppState>>) -> AppSettings {
    state.settings.lock().clone()
}

fn ensure_float_window(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window("float").is_some() {
        return Ok(());
    }
    let w = WebviewWindowBuilder::new(app, "float", WebviewUrl::App("index.html".into()))
        .title("Dictate")
        .inner_size(220.0, 56.0)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .visible(true)
        .focused(false)
        .build()
        .map_err(|e| format!("float window: {e}"))?;
    let _ = w.set_position(tauri::LogicalPosition::new(40.0, 80.0));
    Ok(())
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: AppSettings,
) -> Result<(), String> {
    *state.settings.lock() = settings.clone();
    let path = data_dir().join("settings.json");
    let s = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, s).map_err(|e| e.to_string())?;
    if settings.show_floating {
        ensure_float_window(&app)?;
        if let Some(w) = app.get_webview_window("float") {
            let _ = w.show();
        }
    } else if let Some(w) = app.get_webview_window("float") {
        let _ = w.hide();
    }
    Ok(())
}

#[tauri::command]
fn get_history(
    state: State<'_, Arc<AppState>>,
    query: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<TranscriptEntry>, String> {
    state
        .db
        .list_transcripts(limit.unwrap_or(200), query)
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
    AppStatus {
        recording: *state.recording.lock(),
        busy: *state.busy.lock(),
        last_error: state.last_error.lock().clone(),
        message: status_message(&state),
        python_ok: state.venv_python.exists(),
        worker_ok: state.asr_worker.exists(),
    }
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
fn paste_entry(app: AppHandle, text: String) -> Result<(), String> {
    paste::copy_text(&app, &text)?;
    std::thread::sleep(std::time::Duration::from_millis(80));
    paste::paste_frontmost()
}

#[tauri::command]
fn open_data_dir() -> Result<(), String> {
    std::process::Command::new("open")
        .arg(data_dir())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn open_privacy_settings(pane: String) -> Result<(), String> {
    let url = match pane.as_str() {
        "Microphone" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
        "Accessibility" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        "InputMonitoring" | "ListenEvent" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        }
        _ => "x-apple.systempreferences:com.apple.preference.security",
    };
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn manual_toggle(app: AppHandle, state: State<'_, Arc<AppState>>) {
    if *state.recording.lock() {
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

fn resolve_paths() -> (PathBuf, PathBuf) {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let root = home.join("Desktop/wilson-voice");
    (root.join(".venv/bin/python"), root.join("python/asr_worker.py"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let (venv_py, worker) = resolve_paths();
    let db_path = data_dir().join("wilson_voice.db");
    let db = Database::open(db_path).expect("sqlite open failed");
    let legacy = data_dir().join("history").join("transcripts.json");
    match db.migrate_json_if_needed(legacy) {
        Ok(n) if n > 0 => log::info!("migrated {n} legacy transcripts into SQLite"),
        Err(e) => log::warn!("legacy migrate: {e}"),
        _ => {}
    }
    let db = Arc::new(db);

    let settings = {
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
        recorder: PLMutex::new(None),
        db,
        last_error: PLMutex::new(None),
        venv_python: venv_py,
        asr_worker: worker,
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler({
                    let state = state.clone();
                    move |app, shortcut, event| {
                        if event.state == ShortcutState::Pressed {
                            log::info!("shortcut pressed: {shortcut:?}");
                            start_recording(app, &state);
                        } else if event.state == ShortcutState::Released {
                            log::info!("shortcut released: {shortcut:?}");
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
            open_privacy_settings,
            manual_toggle,
            show_main
        ])
        .setup(move |app| {
            // Main window first — must stay responsive
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
                let w = win.clone();
                win.on_window_event(move |e| {
                    if let WindowEvent::CloseRequested { api, .. } = e {
                        // Hide to tray instead of quit
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

            // Tray (non-blocking)
            let show_i = MenuItem::with_id(app, "show", "Open Wilson Voice", true, None::<&str>)?;
            let toggle_i =
                MenuItem::with_id(app, "toggle", "Start / Stop dictation", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &toggle_i, &quit_i])?;

            let icon = app.default_window_icon().cloned().ok_or("missing app icon")?;

            let _tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("Wilson Voice — ⌘⇧V hold to dictate")
                .on_menu_event({
                    let state = state.clone();
                    move |app, event| match event.id.as_ref() {
                        "quit" => {
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

            // Hotkeys AFTER UI is up. Use ⌘⇧V only — ⌥Space can fight system shortcuts.
            // Fail soft so missing Input Monitoring never freezes the app.
            let sc = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyV);
            match app.global_shortcut().register(sc) {
                Ok(()) => log::info!("registered ⌘⇧V"),
                Err(e) => log::warn!("hotkey ⌘⇧V unavailable (Input Monitoring?): {e}"),
            }
            let sc2 = Shortcut::new(Some(Modifiers::ALT), Code::Space);
            match app.global_shortcut().register(sc2) {
                Ok(()) => log::info!("registered ⌥Space"),
                Err(e) => log::warn!("hotkey ⌥Space unavailable: {e}"),
            }

            // Floating pill is opt-in (Settings) — never create on cold start
            if state.settings.lock().show_floating {
                if let Err(e) = ensure_float_window(app.handle()) {
                    log::warn!("float window skipped: {e}");
                }
            }

            emit_status(app.handle(), &state);
            log::info!("Wilson Voice desktop ready");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Wilson Voice");
}
