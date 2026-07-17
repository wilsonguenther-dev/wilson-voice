//! Wilson Voice v0.4 — production dictation desktop app
//!
//! Phases (PLAN.md):
//! 0 Hygiene — single /Applications install, no DMG litter
//! 1 Permissions — AX + mic probes, System Settings deep-links
//! 2 Hotkeys — ⌘⇧V hold via Carbon on main thread (deferred)
//! 3 Record + paste — cpal in-process mic + CGEvent paste on main thread
//! 4 Product UI — Home / Insights / Dictionary / Scratchpad / Permissions

mod asr;
mod db;
mod float_pill;
mod mic_auth;
mod paste;
mod permissions;
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
    pub show_floating_pill: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            // large-v3-turbo is the best speed/accuracy balance on Apple Silicon MLX
            model: "mlx-community/whisper-large-v3-turbo".into(),
            language: "en".into(),
            auto_paste: true,
            hotkey_label: "⌘⇧V hold".into(),
            show_floating_pill: true,
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
    pub accessibility: bool,
    pub hotkey_registered: bool,
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
    let last_error = state.last_error.lock().clone();
    let accessibility = permissions::is_accessibility_trusted();
    let hotkey_registered = *state.hotkey_registered.lock();
    let message = if recording {
        "Recording… release ⌘⇧V or click Stop".into()
    } else if busy {
        "Transcribing with local Whisper…".into()
    } else if let Some(ref err) = last_error {
        format!("Error: {err}")
    } else if !state.venv_python.exists() {
        "ASR venv missing — see Permissions".into()
    } else if !accessibility {
        "Ready (copy-only) — enable Accessibility to auto-paste".into()
    } else {
        "Ready — hold ⌘⇧V to dictate".into()
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
        let result = (|| -> Result<(TranscriptEntry, paste::PasteOutcome), String> {
            let wav = record::stop_recording(active)?;
            log::info!("wav ready: {}", wav.display());
            let asr = asr::run_asr(&venv, &worker, &wav, &settings.model, &settings.language)?;
            let text = db.apply_dictionary(&asr.text).unwrap_or(asr.text);
            let entry = db.insert_transcript(text, asr.backend, asr.seconds, None)?;
            // Focus should return to previous app; brief delay helps paste target
            std::thread::sleep(std::time::Duration::from_millis(150));
            let outcome = paste::copy_and_maybe_paste(&app2, &entry.text, settings.auto_paste);
            Ok((entry, outcome))
        })();

        match result {
            Ok((entry, outcome)) => {
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
                let preview = if entry.text.chars().count() > 100 {
                    let s: String = entry.text.chars().take(100).collect();
                    format!("{s}…")
                } else {
                    entry.text.clone()
                };
                notify(&app2, "Wilson Voice", format!("{preview}\n({})", outcome.message));
                log::info!(
                    "transcript ok words={} pasted={}",
                    entry.word_count,
                    outcome.pasted
                );
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
    if settings.show_floating_pill {
        float_pill::show_float(&app)?;
    } else {
        float_pill::hide_float(&app);
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
fn paste_entry(app: AppHandle, text: String) -> Result<String, String> {
    // IPC may not be main thread — copy_and_maybe_paste hops to main for CGEvent
    let o = paste::copy_and_maybe_paste(&app, &text, true);
    if !o.copied {
        return Err(o.message);
    }
    Ok(o.message)
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
    permissions::open_privacy_pane(&pane)
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
        hotkey_registered: PLMutex::new(false),
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
                        // Only react to our hold-to-talk combo
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
            // Lightweight setup only — no hotkey register, no second window

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
                .tooltip("Wilson Voice — hold ⌘⇧V to dictate")
                .on_menu_event({
                    let state = state.clone();
                    move |app, event| match event.id.as_ref() {
                        "quit" => app.exit(0),
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

            emit_status(app.handle(), &state);

            // Floating pill (opt-in via settings; default on)
            if state.settings.lock().show_floating_pill {
                if let Err(e) = float_pill::show_float(app.handle()) {
                    log::warn!("float pill: {e}");
                }
            }

            // Soft AX prompt once if not trusted (does not block)
            if !permissions::is_accessibility_trusted() {
                log::info!("Accessibility not trusted yet — user should enable Wilson Voice");
            }

            // Phase 2: register ⌘⇧V on main thread AFTER event loop is live
            let handle = app.handle().clone();
            let state_hk = state.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(600));
                let h = handle.clone();
                let st = state_hk.clone();
                let _ = handle.run_on_main_thread(move || {
                    let sc = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyV);
                    match h.global_shortcut().register(sc) {
                        Ok(()) => {
                            *st.hotkey_registered.lock() = true;
                            log::info!("registered hold-to-talk ⌘⇧V");
                            emit_status(&h, &st);
                        }
                        Err(e) => {
                            *st.hotkey_registered.lock() = false;
                            log::warn!("⌘⇧V register failed: {e}");
                            *st.last_error.lock() = Some(format!(
                                "Hotkey unavailable: {e}. Use the Dictate button in the app."
                            ));
                            emit_status(&h, &st);
                        }
                    }
                });
            });

            log::info!("Wilson Voice v0.4.1 setup complete");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Wilson Voice");
}
