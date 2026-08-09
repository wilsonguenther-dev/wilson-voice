//! Wilson Voice — production dictation desktop app
//!
//! Hotkeys: primary FN / FN+Control via CGEvent tap (ptt_macos).
//! Optional legacy ⌘⇧V via Carbon global-shortcut.
//! HUD: floating float.html pill, parked bottom-center (no cursor chase).

mod asr_engine;
mod cli;
mod command_mode;
// YV64: reads macOS' own crash reports + the panic hook's log lines back into
// `crash_events`. Local only — see the module docs for the privacy rules.
mod crash;
mod db;
// Public so the golden formatting corpus in `tests/fixtures/formatting/` can run
// the real pipeline from an integration test (YV59).
pub mod dictation;
mod float_pill;
mod focus;
mod latency;
mod logging;
mod mic_auth;
mod models;
mod paste;
mod paste_tx;
mod permissions;
// YV61: the validated polish stage. Public for the same reason `dictation` is —
// the golden formatting corpus runs the real stage from an integration test.
pub mod polish;
// The JSONL contract with the `yap-polish` sidecar. Compiled into BOTH binaries
// from this one file (see the module docs) so the two ends cannot drift.
mod polish_protocol;
#[cfg(target_os = "macos")]
mod ptt_macos;
mod record;
mod secure_input;
mod snippets;
mod sysaudio;
mod transcription;
mod vad;

use db::{
    CrashEvent, Database, DayCount, DictCandidate, DictEntry, FailedDictation, Insights,
    ScratchNote, Snippet, TranscriptEntry,
};
use parking_lot::Mutex as PLMutex;
use permissions::PermissionReport;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Emitter, Manager, State, WindowEvent, Wry,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;

/// YV41: the container-level `serde(default)` (backed by the `Default` impl
/// below) means a field missing from settings.json falls back to its default
/// instead of failing the whole load — a partial store can never reset a user's
/// config. Individually *invalid* fields are handled by `salvage_settings`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// YV41 settings-schema marker for one-time forward migrations. Fresh
    /// installs stamp the current version; a store written before the field
    /// existed reads as version 0 (see `apply_settings_migrations`) and is
    /// migrated + rewritten once.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub language: String,
    pub auto_paste: bool,
    pub hotkey_label: String,
    /// Always show HUD; also auto-shows while recording even if false.
    pub show_floating_pill: bool,
    /// fn | fn_control | both — CGEvent FN PTT (not Carbon).
    #[serde(default = "default_ptt_binding")]
    pub ptt_binding: String,
    /// YV49 command mode: which EXTRA modifier, held with `ptt_binding`, makes a
    /// press edit the selection instead of typing. command (⌘, default) | option
    /// (⌥) | off. Independent of `ptt_binding` so plain dictation is unaffected.
    #[serde(default = "default_command_binding")]
    pub command_binding: String,
    /// Keep Carbon ⌘⇧V as secondary hold binding.
    #[serde(default = "default_true")]
    pub keep_cmd_shift_v: bool,
    /// Floating pill style: "classic" (obsidian capsule) | "yappy" (pixel pet).
    #[serde(default = "default_pill_style")]
    pub pill_style: String,
    /// Where the pill docks on screen (YV53): "bottom" (centred island, the
    /// default) | "left" | "right" (Wispr-style side dock, vertically centred
    /// and flush to that screen edge). Drives `float_pill::PillPosition` for the
    /// NSPanel origin and the pill's own edge alignment in the webview.
    #[serde(default = "default_pill_position")]
    pub pill_position: String,
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
    /// Where snippet triggers may fire (YV48): `inline` (anywhere in the
    /// transcript, the default) or `utterance` (only when the trigger is the
    /// WHOLE utterance). See `snippets::SnippetScope`.
    #[serde(default = "default_snippet_scope")]
    pub snippet_scope: String,
    /// YV61 local-LLM polish: the catalog id of the installed polish model
    /// (`models::polish_models()`), or `""` — the default — for OFF. With no id
    /// (or no downloaded file) `polish::polish_llm` never spawns the sidecar and
    /// the cleanup pipeline is exactly what it is today.
    #[serde(default)]
    pub polish_model: String,
    /// Hard deadline for one polish pass, in ms (spec §2.3, default 1200). Past
    /// it the model's answer is dropped and the rules text is what pastes.
    #[serde(default = "default_polish_deadline_ms")]
    pub polish_deadline_ms: u64,
    /// The `style_<mode>` tone dial (spec §2.4/R14), keyed by dictation mode
    /// (`email`, `chat`, …) with `very casual | casual | default | formal` as
    /// values. A mode with no entry — the default for all of them — is
    /// `default`. A map, not one field per mode, so a new mode needs no schema
    /// change.
    #[serde(default)]
    pub polish_styles: std::collections::BTreeMap<String, String>,
    /// YV62 (R13) — the sign-off block appended to a take, e.g.
    /// `"Wilson — drivia.consulting"`. Empty by default. It is copied BYTE FOR
    /// BYTE by `snippets::append_signature` AFTER the polish stage, so no model
    /// can rewrite it and none can invent one (`polish::validate_polish` rejects
    /// an invented signature on the V3/V5 path).
    #[serde(default)]
    pub signature: String,
    /// When that block is appended: `off` (the default — never) | `cue` (only
    /// when the take ends with "sign it") | `auto` (a cue, or any email that
    /// closes on a sign-off line). See `snippets::SignatureMode`.
    #[serde(default = "default_signature_mode")]
    pub signature_mode: String,
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
    /// Launch Yap at login (YV42), applied through tauri-plugin-autostart's
    /// macOS LaunchAgent. Defaults OFF — nothing installs a login item behind
    /// the user's back; the toggle is applied immediately on save and re-applied
    /// at every startup so this setting, not a stale LaunchAgent, is the truth.
    #[serde(default)]
    pub autostart: bool,
    /// Look for a newer Yap on GitHub Releases (YV44). The check ONLY notifies:
    /// nothing is downloaded or installed until the user clicks "Install now"
    /// (`install_update`). Defaults on; off means Yap never contacts the
    /// release endpoint at all.
    #[serde(default = "default_true")]
    pub check_updates: bool,
    /// A version the user dismissed with "Skip this version" (YV44). That exact
    /// version is never offered again; any newer release still is.
    #[serde(default)]
    pub skipped_update_version: Option<String>,
    /// YV46 first-run gate for the pre-SQLite `history/transcripts.json` import
    /// (`Database::migrate_json_if_needed`). Backend-owned bookkeeping, not a
    /// user preference: it flips to true after the one import attempt so no
    /// later launch stats that path again. Every install created after the
    /// SQLite switch simply flips it on its first launch with nothing to read.
    /// `save_settings` re-asserts the stored value, so a UI save can never
    /// clear it and re-arm the migration.
    #[serde(default)]
    pub legacy_json_migrated: bool,
}

/// Current settings-schema version (YV41). Bump this ONLY together with a new
/// arm in `apply_settings_migrations` that upgrades stores written at the
/// previous version.
const CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    CURRENT_SETTINGS_SCHEMA_VERSION
}
fn default_ptt_binding() -> String {
    "fn_control".into()
}
fn default_command_binding() -> String {
    "command".into()
}
fn default_pill_style() -> String {
    "classic".into()
}
fn default_pill_position() -> String {
    "bottom".into()
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
fn default_snippet_scope() -> String {
    "inline".into()
}
fn default_polish_deadline_ms() -> u64 {
    polish::DEFAULT_POLISH_DEADLINE_MS
}
fn default_signature_mode() -> String {
    "off".into()
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
            schema_version: CURRENT_SETTINGS_SCHEMA_VERSION,
            language: "en".into(),
            auto_paste: true,
            hotkey_label: "fn⌃".into(),
            // Always-on glass island rides all Spaces
            show_floating_pill: true,
            ptt_binding: "fn_control".into(),
            command_binding: "command".into(),
            keep_cmd_shift_v: false,
            pill_style: "classic".into(),
            pill_position: "bottom".into(),
            companion_tone: "friendly".into(),
            dictation_mode: "auto".into(),
            cleanup_level: "light".into(),
            snippet_scope: "inline".into(),
            polish_model: String::new(),
            polish_deadline_ms: polish::DEFAULT_POLISH_DEADLINE_MS,
            polish_styles: std::collections::BTreeMap::new(),
            signature: String::new(),
            signature_mode: "off".into(),
            denoise: true,
            mute_while_dictating: true,
            onboarded: false,
            calibration_sample: None,
            native_model: default_native_model(),
            autostart: false,
            check_updates: true,
            skipped_update_version: None,
            legacy_json_migrated: false,
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
    /// YV43 — another app holds macOS Secure Input, so the fn PTT event tap is
    /// blind and the hotkey cannot fire. True means the UI must SAY so instead
    /// of continuing to advertise "hold fn⌃".
    #[serde(default)]
    pub secure_input_blocked: bool,
    /// The holder + workaround line for the banner. `None` when not blocked.
    #[serde(default)]
    pub secure_input_detail: Option<String>,
}

struct AppState {
    settings: PLMutex<AppSettings>,
    recording: PLMutex<bool>,
    busy: PLMutex<bool>,
    /// Hands-free latch (double-tap fn⌃); release keys but keep recording.
    hands_free: PLMutex<bool>,
    /// YV49 command mode: the selection captured when THIS take began, read via
    /// AX at key-down. `Some` marks the in-flight take as a command (edit the
    /// selection) rather than dictation (type the transcript). Consumed by
    /// `stop_and_transcribe` and cleared by `cancel_recording`, so it can never
    /// leak into a later, ordinary take.
    command_selection: PLMutex<Option<String>>,
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
    /// YV51 — the "Undo AI Edit" item; its enabled state follows `undo_available`.
    tray_paste_raw: PLMutex<Option<MenuItem<Wry>>>,
    /// YV51 — does the newest take have a raw transcript that differs from the
    /// polished text that was pasted? Gates the "Undo AI Edit" tray item and
    /// ⌃⌘Z, so the action is only offered when there IS an AI edit to undo.
    /// Cached (seeded at startup, rewritten as each take lands) so `sync_tray`
    /// never touches the DB on a state transition.
    undo_available: PLMutex<bool>,
    /// YV43 — latest Secure Input watchdog snapshot. Written ONLY by the
    /// watchdog thread on an edge; read by every `build_status`, so the pill,
    /// the banner and the tray tooltip all agree.
    secure_input: PLMutex<secure_input::SecureInputStatus>,
    /// Warm Silero VAD (YV36) — loaded ONCE from disk during startup (see the
    /// background thread in `run()`) and reused by every dictation, instead of
    /// re-loading + re-failing an ONNX graph on each clip. `None` until the
    /// model asset is on disk (or forever, if the download/load fails), which
    /// is the explicit energy-VAD fallback.
    vad: PLMutex<Option<Arc<vad::WarmVad>>>,
    /// YV39 cancellation generation. Every cancel/interrupt bumps it; the
    /// transcribe worker captures it when the take stops and re-checks it right
    /// before the paste, so a dictation the user cancelled while ASR was still
    /// running can never land ⌘V in their app seconds later.
    paste_generation: AtomicU64,
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

/// The user's home, where macOS keeps its crash reports (YV64). Falls back to
/// `.` for the same reason `data_dir` does — a missing home must not panic.
fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Where a failed take's WAV is parked so it can be retried (YV52).
///
/// Deliberately NOT the recordings dir: `record::sweep_stale_wavs` empties that
/// at every startup, which would destroy exactly the clips a recovery needs.
/// Everything in here is purged after `db::FAILED_TAKE_RETENTION_DAYS`.
fn recovery_dir() -> PathBuf {
    data_dir().join("recovery")
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
///
/// YV43: `secure_input_blocked` outranks the model + accessibility lines for the
/// same honesty reason — while another app holds Secure Input the fn tap is
/// blind, so telling the user to "hold fn⌃" is telling them to do something that
/// physically cannot work.
fn status_message(
    recording: bool,
    hands_free: bool,
    busy: bool,
    last_error: Option<&str>,
    model_ready: bool,
    accessibility: bool,
    secure_input_blocked: bool,
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
    } else if secure_input_blocked {
        secure_input::BLOCKED_MESSAGE.into()
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
    let secure = state.secure_input.lock().clone();
    let message = status_message(
        recording,
        hands_free,
        busy,
        last_error.as_deref(),
        ready,
        accessibility,
        secure.blocked,
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
        secure_input_blocked: secure.blocked,
        secure_input_detail: secure.blocked.then(|| secure.detail()),
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
    let secure_input_blocked = state.secure_input.lock().blocked;
    let dictation = state.tray_dictation.lock().clone();
    let hf_item = state.tray_hands_free.lock().clone();
    let raw_item = state.tray_paste_raw.lock().clone();
    let undo_available = *state.undo_available.lock();
    let tray = state.tray.lock().clone();
    if dictation.is_none() && hf_item.is_none() && tray.is_none() {
        return; // tray not built yet (early startup emit)
    }
    let _ = app.run_on_main_thread(move || {
        // YV51: "Undo AI Edit" is greyed out unless the newest take actually has
        // a raw that differs from what was pasted — an always-enabled item that
        // silently does nothing is worse than a disabled one.
        if let Some(item) = raw_item {
            let _ = item.set_enabled(undo_available);
        }
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
            // YV43: the idle tooltip must not keep saying "hold fn" while
            // Secure Input has the tap blind — that is the exact instruction
            // that silently does nothing. Live states still win: they are
            // already-running takes, not an invitation to press the key.
            let tip = if recording && hands_free {
                "Yap — hands-free recording"
            } else if recording {
                "Yap — recording"
            } else if busy {
                "Yap — transcribing…"
            } else if secure_input_blocked {
                secure_input::BLOCKED_MESSAGE
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

/// Mic-level HUD cadence: the float pill's waveform redraws at 20 frames per
/// second, so the level thread emits one `audio_level` per frame. This is a
/// render rate, not a wait on anything — the take's end arrives as a signal
/// (see `record::StopSignal`), never by re-reading state on a timer.
const HUD_FPS: u64 = 20;
const HUD_FRAME: std::time::Duration = std::time::Duration::from_millis(1_000 / HUD_FPS);

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
    // YV63: the take's crash journal spills into the SAME recovery dir a failed
    // take is parked in, so a recovered clip lands inside the retry + 7-day
    // purge lifecycle that already exists.
    match record::start_recording(
        data_dir().join("recordings"),
        &recovery_dir(),
        denoise,
        pressed_at,
    ) {
        Ok(active) => {
            let level = active.level.clone();
            let stop = active.stop_signal();
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
            // Stream mic levels to the float HUD at HUD_FPS. YV38: the thread
            // parks on the take's stop signal between frames instead of sleeping
            // a fixed slice and re-reading a flag, so key-release ends the meter
            // immediately (it used to keep drawing for up to one frame after the
            // hold, and the zeroing frame landed that late too).
            let app_lv = app.clone();
            std::thread::spawn(move || {
                use std::sync::atomic::Ordering;
                loop {
                    let v = level.load(Ordering::Relaxed) as f64 / 1000.0;
                    let _ = app_lv.emit("audio_level", v);
                    if stop.wait_stopped(HUD_FRAME) {
                        break;
                    }
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
    // YV39: bump BEFORE the recording check. A cancel that arrives while a
    // previous take is still transcribing finds `recording` already false and
    // returns below — but that take must still be invalidated, or its ⌘V lands
    // seconds after the user cancelled.
    state.paste_generation.fetch_add(1, Ordering::SeqCst);
    if !*state.recording.lock() {
        return;
    }
    *state.recording.lock() = false;
    *state.hands_free.lock() = false;
    // YV49: a cancelled command press must not leave its selection armed for
    // the next, ordinary take.
    *state.command_selection.lock() = None;
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

/// Transient toast channel (the `.toast` flash in App.tsx). Carries the paste
/// result line and, since YV49, the command-mode outcomes — including the
/// "Didn't catch a command" rejection that leaves the selection untouched.
pub const PASTE_OUTCOME_EVENT: &str = "paste_outcome";

/// Soft status for a take that produced nothing to paste — a fumbled tap or a
/// rejected hallucination loop. Normal, not an error.
const NO_SPEECH_MESSAGE: &str = "Didn't catch any speech — hold and speak";

/// YV67 — what a hallucination-gate rejection writes on its recovery row, so
/// History says WHY the take is there and Retry re-runs ASR on the same audio.
const GATE_REPETITION_REASON: &str = "gate: possible repetition loop";

/// YV67 — a take the microphone died in the middle of. Errors the take (so the
/// `Err` arm preserves the partial wav) rather than pasting a transcript that is
/// silently cut off wherever the device dropped out.
const DEVICE_FAILED_MESSAGE: &str =
    "Microphone disconnected mid-take — the partial audio was saved, press Retry.";

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
/// How many dictionary terms are considered for the decode bias (YV47). The
/// real ceiling is the prompt's own token window — this only stops us reading
/// the whole table on every take.
const BIAS_TERM_LIMIT: i64 = 64;

/// How many pending dictionary suggestions the UI shows at once (YV47).
const DICT_CANDIDATE_LIMIT: i64 = 12;

fn transcribe_native(
    manager: &transcription::TranscriptionManager,
    model_id: &str,
    model_path: &Path,
    samples: Vec<f32>,
    language: &str,
    bias_prompt: Option<String>,
) -> Result<transcription::AsrOutput, String> {
    let started = std::time::Instant::now();
    manager.load(model_id, model_path)?;
    // YV40: load and decode are timed apart — the load is 0 on the warm path and
    // seconds on a cold one, so folding them together (as `asr_model` used to)
    // made the decode span unreadable on exactly the take that is slowest.
    let load_ms = started.elapsed().as_millis() as i64;
    // The "Language I speak" setting used to reach Whisper as the sidecar's
    // `--language`; it now rides the engine's own language hint (YV34) so the
    // picker keeps working. Blank = autodetect.
    let language = Some(language.trim())
        .filter(|l| !l.is_empty())
        .map(str::to_string);
    let decode_started = std::time::Instant::now();
    let text = manager.transcribe(samples, language, bias_prompt)?;
    let decode_ms = decode_started.elapsed().as_millis() as i64;
    if text.trim().is_empty() {
        return Err("Empty transcript".into());
    }
    Ok(transcription::AsrOutput {
        text,
        backend: "native".into(),
        seconds: started.elapsed().as_secs_f64(),
        load_ms,
        decode_ms,
    })
}

/// What a finished take produced (YV49).
///
/// Dictation inserts a transcript row and pastes it. A command-mode take does
/// neither: it edits the selection in place (or is rejected), and the spoken
/// instruction is NOT the user's text, so it never enters history. Both of the
/// pre-YV49 "nothing to paste" exits (no speech, hallucination loop) are the
/// same `Soft` shape, which is why the worker's `Ok(None)` became this enum.
enum TakeOutcome {
    /// Transcript row + paste result + release→clipboard latency.
    Dictated(TranscriptEntry, paste::PasteOutcome, i64),
    /// A transient toast, not an error: no speech, an applied command edit, or
    /// an unrecognised command. Nothing was captured worth keeping.
    Soft(String),
    /// YV67 — a gate threw the take away AFTER the wav was written. Still not a
    /// crash (no hard `last_error`), but the audio is real and the verdict can
    /// be wrong: a false positive here used to destroy a genuine dictation with
    /// no transcript row, no recovery row and no wav. This arm keeps the audio
    /// and writes the retryable row, so History offers Recover.
    ///
    /// `reason` is the short, non-transcript string stored on the recovery row —
    /// `'static` on purpose so a gate can never smuggle the spoken text into it.
    Rejected {
        message: String,
        reason: &'static str,
    },
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
    // YV50 context awareness — read the text just before the caret while the
    // user's app is still the focused one, for the same reason as `source_app`.
    // Bounded to `focus::CONTEXT_CHAR_LIMIT` chars; `None` on AX denial or a
    // secure field. PRIVACY: this string only ever steers the formatting
    // decisions below — it is never logged, stored, or emitted (see the
    // `cursor_context_is_never_logged_or_persisted` test).
    let cursor_context = focus::text_before_cursor();
    // YV49: `Some` means this take was a command press — the selection it is
    // about to edit was read at key-down. Taken (not read) so exactly one take
    // can ever consume it.
    let command_selection = state.command_selection.lock().take();
    // YV39: the generation this take belongs to. Re-checked immediately before
    // the paste — a cancel during ASR bumps it and the transcript is copied
    // only, never pasted.
    let generation = state.paste_generation.load(Ordering::SeqCst);
    let t_release = std::time::Instant::now();

    std::thread::spawn(move || {
        // Panic-safety: disarmed after the normal cleanup below (see [8]).
        let mut busy_guard = WorkerBusyGuard {
            state: state2.clone(),
            app: app2.clone(),
            armed: true,
        };
        // YV52 dictation recovery — the take's clip guard, parked here as soon
        // as capture finishes so the `Err` arm below can PRESERVE the wav and
        // record a recoverable row instead of letting the guard unlink it. On
        // every other exit it is dropped at the end of this thread exactly as
        // before, so a successful take's audio still never outlives the take.
        let mut pending: Option<(record::ClipWav, f64, Option<String>)> = None;
        // `Ok(None)` = nothing to paste (no speech, or a rejected hallucination
        // loop): the caller shows a gentle soft status instead of a hard error.
        let result = (|| -> Result<TakeOutcome, String> {
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
            // YV52: hand the clip guard to the failure arm (see `pending`). The
            // wav still exists only for the life of this take on every path that
            // is not an outright failure.
            pending = Some((rec.clip, rec.speech_seconds, source_app.clone()));
            // YV67 — the mic died mid-hold (unplugged, format change). Whatever
            // it captured before that is real audio and is already on disk, but
            // the take is TRUNCATED at an arbitrary word, so transcribing and
            // pasting it would type a half sentence the user cannot get back.
            // Fail it instead: the `Err` arm below preserves the wav and writes
            // the Retry row.
            //
            // The ORDER here is load-bearing. Raising this inside
            // `record::stop_recording` (or anywhere above the line before this
            // one) would run BEFORE the clip guard is handed to `pending`, so
            // `ClipWav::drop` would unlink the very audio we are trying to save —
            // which is the bug, not the fix.
            if rec.device_failed {
                return Err(DEVICE_FAILED_MESSAGE.into());
            }
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
                return Ok(TakeOutcome::Soft(NO_SPEECH_MESSAGE.into()));
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
            // YV47 — bias the decoder with the auto-learning dictionary: starred
            // terms first, then usage-ranked, capped to the prompt window. A DB
            // hiccup here must never cost the take, so it degrades to unbiased.
            let bias_prompt = match db.bias_terms(BIAS_TERM_LIMIT) {
                Ok(terms) => asr_engine::build_bias_prompt(&terms),
                Err(e) => {
                    log::warn!("dictionary bias unavailable ({e}) — decoding unbiased");
                    None
                }
            };
            let asr = transcribe_native(
                &state2.transcription,
                &model_id,
                &model_path,
                rec.samples,
                &settings.language,
                bias_prompt,
            )?;
            // Raw ASR output — preserved verbatim so both raw and polished text are
            // stored on the transcript (Wispr Flow "Undo AI edit" / raw↔polished).
            let raw_text = asr.text;
            // Hallucination gate (YV16): reject degenerate Whisper repetition loops
            // ("WPM-SERV-SERV-SERV…") BEFORE they reach the clipboard/paste path.
            // YV67: a REJECTION, not a soft nothing — the wav exists by now, and
            // this gate is a heuristic that can be wrong (a legitimately
            // repetitive dictation reads as a loop). Keeping the audio + a
            // retryable row is the difference between "Yap ate my dictation" and
            // one click of Recover.
            if dictation::is_hallucinated_repetition(&raw_text) {
                // YV20/M2: never write the transcript body to the log — count only.
                log::info!(
                    "hallucination gate: rejected degenerate ASR output ({} chars)",
                    raw_text.chars().count()
                );
                return Ok(TakeOutcome::Rejected {
                    message: NO_SPEECH_MESSAGE.into(),
                    reason: GATE_REPETITION_REASON,
                });
            }
            // YV49 command mode — this take is an INSTRUCTION about the text the
            // user had selected when they pressed, not text to type. It branches
            // BEFORE the cleanup pipeline on purpose: list detection and polish
            // are for dictated prose and would reshape the instruction itself.
            //
            // The hard rule: an instruction outside the deterministic command
            // table NEVER edits the selection. We toast and leave the text
            // exactly as it was — guessing would silently mangle text the user
            // cannot get back.
            if let Some(selection) = command_selection {
                let Some(command) = command_mode::parse_command(&raw_text) else {
                    // YV20/M2 hygiene: count only, never log the spoken text.
                    log::info!(
                        "command mode: unrecognised instruction ({} chars) — selection untouched",
                        raw_text.chars().count()
                    );
                    return Ok(TakeOutcome::Soft(
                        command_mode::UNKNOWN_COMMAND_MESSAGE.into(),
                    ));
                };
                // Same cancellation guard as dictation (YV39): a command the user
                // cancelled during ASR must not edit their app seconds later.
                if state2.paste_generation.load(Ordering::SeqCst) != generation {
                    log::warn!("stale command (cancelled during transcription) — no edit");
                    return Ok(TakeOutcome::Soft("Command cancelled".into()));
                }
                // `Delete` is carried out as a real Delete keystroke rather than
                // pasting an empty string (a no-op in several apps), which also
                // leaves the user's clipboard alone.
                let outcome = if command == command_mode::Command::Delete {
                    paste::delete_selection(&app2, source_app.as_deref())
                } else {
                    let edited = command_mode::apply(&command, &selection);
                    paste::copy_and_maybe_paste(&app2, &edited, true, source_app.as_deref())
                };
                log::info!(
                    "command mode: {} ({} selected chars) → {}",
                    command.label(),
                    selection.chars().count(),
                    outcome.message
                );
                return Ok(TakeOutcome::Soft(if outcome.pasted {
                    command.label()
                } else {
                    format!("{} — {}", command.label(), outcome.message)
                }));
            }

            // Smart dictation (YV5): resolve the effective mode — a user-picked fixed mode
            // wins, otherwise it's inferred from the focused app. YV50 adds the
            // pre-caret context as a HINT that only fills the gap left when the
            // app alone resolved nothing (an email greeting in a browser tab).
            let dictation_mode = dictation::resolve_mode_with_context(
                &settings.dictation_mode,
                source_app.as_deref().unwrap_or_default(),
                cursor_context.as_deref(),
            );
            // YV10 cleanup pipeline: apply_dictionary → backtrack → format_dictation →
            // local-LLM polish (guarded stub), gated by the Auto-Cleanup level. Every
            // stage is guarded so a non-empty transcript can never become empty (falls
            // back to its input on any error/empty result — "never lose text").
            let cleanup_level = dictation::CleanupLevel::from_setting(&settings.cleanup_level);
            // YV61 stage 4: the validated polish path, bound to THIS take's mode
            // and the tone dial for it. It fails closed — a missing model, a
            // missed deadline, a crashed sidecar or a rewrite that fails
            // `validate_polish` all return `None`, and `run_cleanup` keeps the
            // rules text byte for byte.
            let polish_config = polish::PolishConfig::from_settings(&settings, dictation_mode);
            let t_cleanup = std::time::Instant::now();
            let text = dictation::run_cleanup(
                &raw_text,
                cleanup_level,
                dictation_mode,
                // YV62: the same tone dial the model's overlay gets, so R3's
                // trailing-period rule holds with the polish stage off.
                polish_config.style,
                |t| db.apply_dictionary(t).unwrap_or_else(|_| t.to_string()),
                |t| polish::polish_llm(t, dictation_mode, &polish_config),
            );
            // YV50: join the take onto the text already at the caret — lowercase
            // the lead word when the user is continuing a sentence, capitalise it
            // after . ? ! / a new line / in an empty field, and add the leading
            // space only when the character before the caret needs one. Purely
            // additive (casing + at most one space), so "never lose text" holds.
            let text = dictation::join_with_context(&text, cursor_context.as_deref());
            // YV48 snippets: expand saved trigger phrases AFTER cleanup and
            // before the clipboard. Its own stage — never inside the dictionary
            // closure above, and never on the history command paths — so only a
            // live dictation can expand. A DB/matcher failure degrades to the
            // un-expanded text: the transcript always pastes.
            let text = match db.snippet_rules() {
                Ok(rules) if !rules.is_empty() => snippets::expand_snippets(
                    &text,
                    &rules,
                    snippets::SnippetScope::from_setting(&settings.snippet_scope),
                ),
                Ok(_) => text,
                Err(e) => {
                    log::warn!("snippets unavailable ({e}) — pasting text unexpanded");
                    text
                }
            };
            // YV62 signature (R13): the LAST stage, after the model and after the
            // snippet expansion, so the configured block reaches the pasteboard
            // byte for byte. Off by default — nothing is appended until the user
            // configures a signature AND turns the mode on.
            let text = snippets::append_signature(
                &text,
                &settings.signature,
                snippets::SignatureMode::from_setting(&settings.signature_mode),
                dictation_mode,
            );
            let cleanup_ms = t_cleanup.elapsed().as_millis() as i64;
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
            // YV39 cancellation guard: a stale take (cancelled while this one
            // was transcribing) is copied but NEVER pasted.
            let stale = state2.paste_generation.load(Ordering::SeqCst) != generation;
            if stale {
                log::warn!(
                    "stale dictation (cancelled during transcription) — copy only, no paste"
                );
            }
            let want_paste = settings.auto_paste && !stale && focus::should_auto_paste();
            let t_paste = std::time::Instant::now();
            let outcome =
                paste::copy_and_maybe_paste(&app2, &text, want_paste, source_app.as_deref());
            let paste_ms = t_paste.elapsed().as_millis() as i64;
            // North-star metric: release hotkey → text on clipboard
            let pipeline_ms = t_release.elapsed().as_millis() as i64;
            // YV40: the whole take as disjoint spans in one stable key=value
            // line (see `latency`) — press side, capture, DSP finalize, ASR load
            // vs decode, cleanup, paste — instead of the old cumulative mix.
            log::info!(
                "{}",
                latency::PipelineSpans {
                    hold_clipboard_ms: pipeline_ms,
                    press_capture_ms: rec.capture_start_ms,
                    capture_ms: (rec.hold_wall_seconds * 1000.0).round() as i64,
                    samples_ready_ms: t_dsp,
                    asr_load_ms: asr.load_ms,
                    asr_decode_ms: asr.decode_ms,
                    cleanup_ms,
                    paste_ms,
                    backend: &asr.backend,
                    pasted: outcome.pasted,
                }
                .summary_line()
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
            Ok(TakeOutcome::Dictated(entry, outcome, pipeline_ms))
        })();

        match result {
            Ok(TakeOutcome::Dictated(entry, outcome, pipeline_ms)) => {
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
                // YV51: this take is now the newest, so it decides whether
                // "Undo AI Edit" has anything to undo. The tray item picks the
                // new value up from the `emit_status` at the end of the take.
                *state2.undo_available.lock() =
                    dictation::undo_ai_edit_text(&entry.text, entry.raw_text.as_deref()).is_some();
                let _ = app2.emit("transcript", &entry);
                let _ = app2.emit(PASTE_OUTCOME_EVENT, &outcome.message);
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
            Ok(TakeOutcome::Soft(msg)) => {
                // Nothing was typed: a fumbled tap or a command-mode outcome.
                // All NORMAL, not failures — surface a gentle soft status
                // (transient flash toast) and do NOT set a hard last_error, do
                // NOT insert a transcript.
                *state2.last_error.lock() = None;
                let _ = app2.emit(PASTE_OUTCOME_EVENT, &msg);
                log::info!("soft take outcome: {msg}");
            }
            Ok(TakeOutcome::Rejected { message, reason }) => {
                // YV67 — a gate threw away a take that HAD audio. Still not a
                // crash, so no hard `last_error`; but the wav is real and the
                // gate can be wrong, so it gets the same YV52 treatment a failed
                // take gets: keep the audio, write the retryable row. Before
                // this, `pending` was simply dropped here and `ClipWav::drop`
                // unlinked the only copy — a gate false positive was permanent
                // loss, with nothing in `failed_dictations` to show for it.
                *state2.last_error.lock() = None;
                let recoverable = pending.take().and_then(|(clip, speech_seconds, src)| {
                    keep_failed_take(&db, &recovery_dir(), clip, speech_seconds, src, reason)
                });
                log::info!(
                    "rejected take: {reason} (recoverable={})",
                    recoverable.is_some()
                );
                // Same payload shape as the `Err` arm, so History and the toast
                // offer Recover on exactly this take.
                let _ = app2.emit(
                    TRANSCRIPT_ERROR_EVENT,
                    serde_json::json!({ "message": message, "failed": recoverable }),
                );
            }
            Err(e) => {
                log::error!("pipeline failed: {e}");
                *state2.last_error.lock() = Some(e.clone());
                // YV52: a failed take is never lost. Keep the wav the pipeline
                // already wrote and record a recoverable row, so the user can
                // retry ASR on the same audio (the engine may since have
                // recovered, or the model finished downloading) instead of
                // re-speaking it. `None` = the clip could not be preserved, and
                // the toast/History simply have nothing to retry.
                let recoverable = pending.take().and_then(|(clip, speech_seconds, src)| {
                    keep_failed_take(&db, &recovery_dir(), clip, speech_seconds, src, &e)
                });
                // YV32: tell the UI, not just the log. Any view waiting on a
                // transcript (onboarding calibration, the pill) can now clear its
                // busy state and show the reason instead of spinning forever.
                // The recoverable row rides along so the error toast can offer
                // Retry on exactly this take.
                let _ = app2.emit(
                    TRANSCRIPT_ERROR_EVENT,
                    serde_json::json!({ "message": e.clone(), "failed": recoverable }),
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

/// YV52 — preserve a failed take: move its wav into the recovery dir and write
/// the row that makes it retryable. Best-effort by design — a take that cannot
/// be preserved (write failed, disk full, DB error) leaves the user exactly
/// where they were before this feature existed, never worse, and never with a
/// row pointing at audio that is not there.
///
/// YV67 also routes GATE REJECTIONS through here — `dir` is a parameter (always
/// `recovery_dir()` in the app) so the whole keep-the-audio path is drivable
/// against a temp dir in a test instead of the user's real recovery folder.
fn keep_failed_take(
    db: &Database,
    dir: &Path,
    mut clip: record::ClipWav,
    speech_seconds: f64,
    source_app: Option<String>,
    error: &str,
) -> Option<FailedDictation> {
    let path = match clip.keep_for_recovery(dir) {
        Ok(path) => path,
        Err(e) => {
            log::warn!("failed take not recoverable (clip could not be kept): {e}");
            return None;
        }
    };
    match db.record_failed_dictation(&path, speech_seconds, error, source_app) {
        Ok(row) => {
            log::info!(
                "YV52 recovery: kept failed take {} ({:.2}s) for retry",
                row.id,
                speech_seconds
            );
            Some(row)
        }
        Err(e) => {
            // No row means nothing can ever reach this wav — don't leak audio.
            log::warn!("failed-dictation row not written ({e}) — removing kept clip");
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

/// The error a crash-recovered take (YV63) carries in History — the user sees
/// why the row is there, and Retry runs ASR over the audio that survived.
const CRASH_RECOVERY_ERROR: &str = "recovered after crash";

/// YV63 — a dictation the app died in the MIDDLE of. Nothing used to survive
/// that: the wav is only written after a take ends, so a glitch mid-hold took
/// the words with it. The capture journal spills the frames to
/// `data_dir()/recovery/` as they arrive and leaves an in-progress marker behind
/// if the take never completes; this is the other half — at startup, every
/// orphaned marker is finalized into a real wav and given the SAME
/// failed-dictation row a failed transcription gets (YV52), so it shows up in
/// History with Retry instead of being gone.
///
/// Best-effort throughout, exactly like `keep_failed_take`: a take that cannot
/// be turned into a row has its wav removed rather than left as audio nothing
/// can reach. Returns how many takes were recovered.
fn recover_crashed_takes(db: &Database, dir: &Path) -> usize {
    let mut recovered = 0;
    for take in record::recover_orphaned_journals(dir) {
        match db.record_failed_dictation(&take.wav_path, take.seconds, CRASH_RECOVERY_ERROR, None) {
            Ok(row) => {
                recovered += 1;
                log::info!(
                    "YV63 crash recovery: take {} ({:.2}s) rebuilt from its capture journal",
                    row.id,
                    take.seconds
                );
            }
            Err(e) => {
                log::warn!("YV63 crash recovery: row not written ({e}) — removing rebuilt clip");
                let _ = std::fs::remove_file(&take.wav_path);
            }
        }
    }
    recovered
}

/// YV52 — expired recoverable takes are dropped with their audio. Called at
/// startup and before every list, so the 7-day window holds in a long-running
/// session too. Returns how many were purged.
fn purge_expired_failed_takes(db: &Database) -> usize {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(db::FAILED_TAKE_RETENTION_DAYS);
    match db.purge_failed_dictations(cutoff) {
        Ok(paths) => {
            for path in &paths {
                let _ = std::fs::remove_file(path);
            }
            if !paths.is_empty() {
                log::info!(
                    "YV52 retention: purged {} failed take(s) older than {} days",
                    paths.len(),
                    db::FAILED_TAKE_RETENTION_DAYS
                );
            }
            paths.len()
        }
        Err(e) => {
            log::warn!("failed-take purge skipped: {e}");
            0
        }
    }
}

// --- Commands ---

/// Write settings.json (the single on-disk copy every settings writer goes
/// through — `save_settings` and the YV31 model commands).
fn persist_settings(settings: &AppSettings) -> Result<(), String> {
    write_settings_file(&data_dir().join("settings.json"), settings)
}

/// YV41 durable settings write, split from `persist_settings` so it is testable
/// against a temp dir. Two guarantees:
///
/// 1. **Merge forward.** The frontend round-trips the whole `AppSettings`
///    object on every save, which silently drops any key this build does not
///    know about (e.g. one written by a newer version before a downgrade). Keys
///    already on disk that the serialized struct does not carry are re-added
///    here, so a save can never delete another build's settings.
/// 2. **Atomic.** The bytes land in a sibling `.tmp` file that is fsynced and
///    then renamed over the target, so a crash or power loss mid-write leaves
///    either the old file or the new one — never the truncated file that the
///    loader would have to quarantine.
fn write_settings_file(path: &Path, settings: &AppSettings) -> Result<(), String> {
    let mut value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    if let (Some(next), Some(prev)) = (
        value.as_object_mut(),
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .as_ref()
            .and_then(|v| v.as_object().cloned()),
    ) {
        for (key, stored) in prev {
            next.entry(key).or_insert(stored);
        }
    }
    let s = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;

    // Same directory as the target — rename is only atomic within a filesystem.
    let tmp = path.with_extension("json.tmp");
    let write_tmp = || -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(s.as_bytes())?;
        f.sync_all()
    };
    if let Err(e) = write_tmp() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("write {} failed: {e}", tmp.display()));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename {} -> {} failed: {e}", tmp.display(), path.display())
    })
}

/// YV41 startup read of settings.json. The old `from_str(..).ok().unwrap_or_default()`
/// threw away EVERY setting — hotkey, model, companion tone, the onboarded flag —
/// on any single unparseable field. The rules now:
///
/// * missing file → defaults (fresh install, nothing to rewrite);
/// * whole-file garbage (not JSON, or not a JSON object) → defaults, with the
///   corrupt file kept aside as `settings.json.bak` (never silently deleted);
/// * one bad field → `salvage_settings` keeps every other stored field;
/// * an older schema → `apply_settings_migrations` upgrades it and rewrites the
///   file once, atomically.
fn load_settings(path: &Path) -> AppSettings {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return AppSettings::default();
    };
    let stored: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v @ serde_json::Value::Object(_)) => v,
        Ok(_) => {
            quarantine_settings(path, "settings.json is not a JSON object");
            return AppSettings::default();
        }
        Err(e) => {
            quarantine_settings(path, &format!("settings.json is not valid JSON ({e})"));
            return AppSettings::default();
        }
    };

    let mut settings: AppSettings = match serde_json::from_value(stored.clone()) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("settings.json did not parse as a whole ({e}); salvaging field by field");
            salvage_settings(&stored)
        }
    };

    if apply_settings_migrations(&mut settings, &stored) {
        match write_settings_file(path, &settings) {
            Ok(()) => log::info!(
                "settings migrated to schema v{CURRENT_SETTINGS_SCHEMA_VERSION} and rewritten"
            ),
            Err(e) => log::warn!("settings migration could not be persisted ({e})"),
        }
    }
    settings
}

/// Move an unusable settings.json aside instead of overwriting it — the user's
/// (or a support session's) only copy of what was on disk. Mirrors the DB's
/// corrupt-file quarantine in `Database::open`.
fn quarantine_settings(path: &Path, why: &str) {
    let bak = path.with_extension("json.bak");
    match std::fs::rename(path, &bak) {
        Ok(()) => log::error!(
            "{why}; kept the corrupt file at {} and starting from defaults",
            bak.display()
        ),
        Err(e) => log::error!("{why}; could not keep a .bak copy ({e}); starting from defaults"),
    }
}

/// Rebuild settings from a stored object that failed to deserialize as a whole.
/// Every stored field that is individually valid is kept; only the broken values
/// (a wrong type, or a value written by a newer build) fall back to their
/// default. One bad field can therefore never reset the rest of the config.
/// Ported from Handy's `salvage_settings` (their #1619).
fn salvage_settings(stored: &serde_json::Value) -> AppSettings {
    let Some(stored_map) = stored.as_object() else {
        log::warn!("stored settings are not a JSON object; falling back to defaults");
        return AppSettings::default();
    };

    let mut merged = serde_json::to_value(AppSettings::default())
        .expect("default settings serialize to a JSON object");

    for (key, value) in stored_map {
        let previous = merged
            .as_object_mut()
            .expect("merged settings stay an object")
            .insert(key.clone(), value.clone());
        if serde_json::from_value::<AppSettings>(merged.clone()).is_err() {
            // Log the key only — values can carry dictated text or paths.
            log::warn!("dropping invalid settings field '{key}', keeping its default");
            let map = merged
                .as_object_mut()
                .expect("merged settings stay an object");
            match previous {
                Some(previous) => map.insert(key.clone(), previous),
                None => map.remove(key),
            };
        }
    }

    serde_json::from_value(merged).unwrap_or_else(|e| {
        log::warn!("failed to reassemble salvaged settings ({e}); falling back to defaults");
        AppSettings::default()
    })
}

/// One-time forward migrations for stores written by an older schema. Returns
/// true when `settings` was changed and the file should be rewritten. Reads the
/// version off the RAW stored value: a store written before the field existed
/// has no `schemaVersion` key and is version 0, even though serde has already
/// defaulted the struct field to the current version.
///
/// Must stay idempotent — it runs on every launch until the rewrite lands.
fn apply_settings_migrations(settings: &mut AppSettings, stored: &serde_json::Value) -> bool {
    let stored_version = stored
        .get("schemaVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if stored_version >= CURRENT_SETTINGS_SCHEMA_VERSION {
        return false;
    }

    // v0 → v1: a pre-YV41 store can name a model the bundled catalog does not
    // have — the retired Python-sidecar ids (YV34), or a newer catalog's model
    // after a downgrade. An unknown id can never be downloaded or loaded, so the
    // app would sit at "Model needed" forever with no way back; point it at the
    // catalog's recommendation once.
    if models::catalog_model(&settings.native_model).is_none() {
        log::warn!(
            "settings migration v{stored_version}→v{CURRENT_SETTINGS_SCHEMA_VERSION}: \
             model '{}' is not in the bundled catalog; selecting the recommended model",
            settings.native_model
        );
        settings.native_model = default_native_model();
    }

    settings.schema_version = CURRENT_SETTINGS_SCHEMA_VERSION;
    true
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
    // YV46: the legacy-migration flag is backend bookkeeping the UI never
    // carries — take it from the live settings so a save can't re-arm the
    // one-shot migration on the next launch.
    next.legacy_json_migrated = state.settings.lock().legacy_json_migrated;
    // Keep label in sync with binding
    next.hotkey_label = match next.ptt_binding.as_str() {
        "fn" => "fn".into(),
        "both" | "fn_or_fn_control" => "fn / fn⌃".into(),
        _ => "fn⌃".into(),
    };
    *state.settings.lock() = next.clone();
    persist_settings(&next)?;
    // Tell the floating pill to re-read (e.g. switch classic ↔ yappy live, or
    // re-align itself to a new dock edge).
    let _ = app.emit("settings", &next);
    // YV53: the dock edge moves the NSPanel itself, so it must land before the
    // show below re-parks — and `reposition` covers the still-recording case
    // where the pill stays up without a show call.
    float_pill::set_position(float_pill::PillPosition::from_settings(&next.pill_position));
    float_pill::reposition(&app);
    if next.show_floating_pill {
        float_pill::show_float(&app)?;
    } else if !*state.recording.lock() {
        float_pill::hide_float(&app);
    }
    #[cfg(target_os = "macos")]
    {
        ptt_macos::set_binding(ptt_macos::PttBinding::from_settings(&next.ptt_binding));
        ptt_macos::set_command_binding(ptt_macos::CommandBinding::from_settings(
            &next.command_binding,
        ));
    }
    // YV42: the login item follows the toggle immediately, not on next launch.
    apply_autostart(&app, next.autostart);
    Ok(())
}

/// YV65 — the pill webview reports the VISIBLE capsule's rect (window-logical
/// points). The float window is much larger than the capsule and stays
/// click-through; only this rect turns the cursor on, so the pill is grabbable
/// without the transparent margin swallowing anything.
#[tauri::command]
fn pill_set_hitbox(x: f64, y: f64, w: f64, h: f64) {
    float_pill::set_hitbox(x, y, w, h);
}

/// YV65 — pointer-down on the capsule: latch the panel origin and suspend the
/// re-park loop for the duration of the gesture.
#[tauri::command]
fn pill_drag_start(app: AppHandle) {
    float_pill::drag_start(&app);
}

/// YV65 — live move. `dx`/`dy` are the cursor's total travel since
/// `pill_drag_start`, in logical points.
#[tauri::command]
fn pill_drag_move(app: AppHandle, dx: f64, dy: f64) {
    float_pill::drag_move(&app, dx, dy);
}

/// YV65 — pointer-up: snap to the nearest dock and PERSIST it, so a drag is the
/// same durable change the Settings picker makes (same store, same "settings"
/// event, so the picker and the pill both follow). `snap` is false for a press
/// that never became a drag — the pill just re-parks where it was.
#[tauri::command]
fn pill_drag_end(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    x: f64,
    y: f64,
    snap: bool,
) -> Result<(), String> {
    let Some(pos) = float_pill::drag_end(&app, (x, y), snap) else {
        return Ok(());
    };
    let next = {
        let mut settings = state.settings.lock();
        if settings.pill_position == pos.as_settings() {
            return Ok(()); // dropped back on the dock it already had
        }
        settings.pill_position = pos.as_settings().to_string();
        settings.clone()
    };
    persist_settings(&next)?;
    let _ = app.emit("settings", &next);
    log::info!("pill dragged → {} dock", next.pill_position);
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
fn update_dictionary_term(
    state: State<'_, Arc<AppState>>,
    id: String,
    term: String,
    preferred: Option<String>,
) -> Result<(), String> {
    state.db.update_dictionary_term(&id, term, preferred)
}

/// YV47 — pin a term to "always bias": it heads the ranking, survives the
/// harvest purge, and is the last thing dropped from the decoder prompt.
#[tauri::command]
fn set_dictionary_starred(
    state: State<'_, Arc<AppState>>,
    id: String,
    starred: bool,
) -> Result<(), String> {
    state.db.set_dictionary_starred(&id, starred)
}

#[tauri::command]
fn delete_dictionary_term(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.db.delete_dictionary_term(&id)
}

/// YV47 — apply a "Fix transcription" edit: rewrite the stored transcript and
/// mine the words the user changed into dictionary candidates.
#[tauri::command]
fn correct_transcript(
    state: State<'_, Arc<AppState>>,
    id: String,
    text: String,
) -> Result<usize, String> {
    state.db.record_correction(&id, &text).map(|c| c.len())
}

/// YV52 — takes whose transcription failed and whose audio is still on disk.
/// Expired ones are purged (audio included) before the list is built, so the
/// UI can never offer a retry against a clip retention already removed.
#[tauri::command]
fn list_failed_dictations(state: State<'_, Arc<AppState>>) -> Result<Vec<FailedDictation>, String> {
    purge_expired_failed_takes(&state.db);
    state.db.list_failed_dictations()
}

/// YV52 — re-run ASR on a failed take's preserved clip. The common cause of the
/// original failure is transient (no model downloaded yet, an engine that has
/// since reloaded), so this is a real second chance rather than a replay.
///
/// On success the row CONVERTS: the transcript lands in history under the
/// moment it was spoken, the recovery wav is unlinked, and the text is put on
/// the clipboard — it is never auto-pasted, because the user is in Yap now, not
/// in the app they dictated into.
#[tauri::command]
fn retry_failed_dictation(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<TranscriptEntry, String> {
    let row = state
        .db
        .get_failed_dictation(&id)?
        .ok_or_else(|| "That recovery clip is no longer in the list".to_string())?;
    let wav = PathBuf::from(&row.wav_path);
    if !wav.exists() {
        // The audio is gone (purged, or cleaned up outside the app) — drop the
        // row rather than leaving a Retry button that can only ever fail.
        let _ = state.db.delete_failed_dictation(&id);
        return Err("The audio for this take is no longer on disk".into());
    }
    let samples = record::read_wav_16k_mono(&wav)?;
    let settings = state.settings.lock().clone();
    let Some((model_id, model_path)) = native_model_ready(&settings.native_model) else {
        return Err("No speech model installed — open Settings → Models and download one.".into());
    };
    let bias_prompt = match state.db.bias_terms(BIAS_TERM_LIMIT) {
        Ok(terms) => asr_engine::build_bias_prompt(&terms),
        Err(e) => {
            log::warn!("dictionary bias unavailable ({e}) — retry decoding unbiased");
            None
        }
    };
    let asr = transcribe_native(
        &state.transcription,
        &model_id,
        &model_path,
        samples,
        &settings.language,
        bias_prompt,
    )?;
    let raw_text = asr.text;
    if dictation::is_hallucinated_repetition(&raw_text) {
        // YV20/M2: count only, never log the transcript body.
        log::info!(
            "retry hallucination gate: rejected degenerate output ({} chars)",
            raw_text.chars().count()
        );
        return Err("Yap couldn't make sense of that clip — try dictating it again".into());
    }
    // Same cleanup pipeline as a live take, minus the caret context: the retry
    // happens in Yap, so there is no cursor to join onto.
    let mode = dictation::resolve_mode(
        &settings.dictation_mode,
        row.source_app.as_deref().unwrap_or_default(),
    );
    let polish_config = polish::PolishConfig::from_settings(&settings, mode);
    let text = dictation::run_cleanup(
        &raw_text,
        dictation::CleanupLevel::from_setting(&settings.cleanup_level),
        mode,
        polish_config.style,
        |t| {
            state
                .db
                .apply_dictionary(t)
                .unwrap_or_else(|_| t.to_string())
        },
        |t| polish::polish_llm(t, mode, &polish_config),
    );
    let entry = state.db.convert_failed_dictation(
        &id,
        text.clone(),
        asr.backend,
        asr.seconds,
        Some(raw_text),
    )?;
    // The take's audio now follows the normal lifecycle: it is gone.
    let _ = std::fs::remove_file(&wav);
    let outcome = paste::copy_and_maybe_paste(&app, &text, false, None);
    log::info!(
        "YV52 recovery: retry succeeded words={} ({})",
        entry.word_count,
        outcome.message
    );
    let _ = app.emit("transcript", &entry);
    Ok(entry)
}

/// YV52 — throw a failed take away: the row and its audio both go.
#[tauri::command]
fn discard_failed_dictation(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    if let Some(path) = state.db.delete_failed_dictation(&id)? {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

/// YV47 — pending "Yap noticed: …" dictionary suggestions.
#[tauri::command]
fn list_dict_candidates(state: State<'_, Arc<AppState>>) -> Result<Vec<DictCandidate>, String> {
    state.db.list_dict_candidates(DICT_CANDIDATE_LIMIT)
}

#[tauri::command]
fn promote_dict_candidate(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<DictEntry, String> {
    state.db.promote_dict_candidate(&id)
}

#[tauri::command]
fn dismiss_dict_candidate(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.db.dismiss_dict_candidate(&id)
}

/// YV48 — saved `trigger phrase → expansion` snippets (Settings → Snippets).
#[tauri::command]
fn list_snippets(state: State<'_, Arc<AppState>>) -> Result<Vec<Snippet>, String> {
    state.db.list_snippets()
}

#[tauri::command]
fn add_snippet(
    state: State<'_, Arc<AppState>>,
    trigger: String,
    expansion: String,
) -> Result<Snippet, String> {
    state.db.add_snippet(trigger, expansion)
}

#[tauri::command]
fn update_snippet(
    state: State<'_, Arc<AppState>>,
    id: String,
    trigger: String,
    expansion: String,
) -> Result<(), String> {
    state.db.update_snippet(&id, trigger, expansion)
}

#[tauri::command]
fn set_snippet_enabled(
    state: State<'_, Arc<AppState>>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    state.db.set_snippet_enabled(&id, enabled)
}

#[tauri::command]
fn delete_snippet(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.db.delete_snippet(&id)
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

/// YV51 "Undo AI edit" — re-paste the RAW take over the polished one the user
/// just got. Backs the tray "Undo AI Edit" item and the global ⌃⌘Z shortcut.
///
/// Reads the newest row and hands the pair to [`dictation::undo_ai_edit_text`],
/// which is the single source of truth for "is there an edit to undo?" (the same
/// rule the tray enabled-state and the history button use). A `None` verdict is
/// a logged no-op — never a paste — so this can't blank the user's text when
/// Auto-Cleanup is off or the row predates the raw column. Like
/// `paste_last_transcript` the ⌘V hop blocks the main thread, so the paste MUST
/// run off-main via `spawn_blocking`, and no new history row is inserted.
fn paste_raw_last_transcript(app: &AppHandle, state: &AppState) {
    let entry = match state.db.list_transcripts(1, None) {
        Ok(entries) => entries.into_iter().next(),
        Err(err) => {
            log::warn!("undo ai edit: db read failed: {err}");
            return;
        }
    };
    let Some(entry) = entry else {
        log::info!("undo ai edit: history is empty");
        return;
    };
    let Some(raw) = dictation::undo_ai_edit_text(&entry.text, entry.raw_text.as_deref()) else {
        log::info!("undo ai edit: raw matches the pasted text — nothing to undo");
        return;
    };
    let raw = raw.to_string();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Explicit re-paste: the current frontmost app is the intended target,
        // so pass `None` (the YV21 source-app guard does not apply here).
        let o = paste::copy_and_maybe_paste(&app, &raw, true, None);
        if !o.copied {
            log::warn!("undo ai edit failed: {}", o.message);
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
///
/// YV64: the export now also drops `crash-summary.txt` next to the logs, so a
/// report carries WHAT crashed rather than only the log tail. Writing it is
/// best-effort — a failed write still opens the folder, which is the action the
/// user asked for. It is written to the local logs folder and nowhere else.
#[tauri::command]
fn open_logs_dir(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let dir = logging::logs_dir(&data_dir());
    match state.db.list_crash_events(db::CRASH_EVENT_LIMIT) {
        Ok(events) => {
            let summary = crash::summary_text(&events);
            if let Err(e) = std::fs::write(dir.join("crash-summary.txt"), summary) {
                log::warn!("crash summary not written: {e}");
            }
        }
        Err(e) => log::warn!("crash summary skipped: {e}"),
    }
    std::process::Command::new("open")
        .arg(dir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// YV64 — the recorded crashes, newest first, for Settings → Privacy &
/// Diagnostics → Stability.
#[tauri::command]
fn list_crash_events(state: State<'_, Arc<AppState>>) -> Result<Vec<CrashEvent>, String> {
    state.db.list_crash_events(db::CRASH_EVENT_LIMIT)
}

/// YV64 — the user has seen the list; stop treating those crashes as news.
#[tauri::command]
fn acknowledge_crash_events(state: State<'_, Arc<AppState>>) -> Result<usize, String> {
    state.db.acknowledge_crash_events()
}

/// YV64 — drop Yap's stored crash history.
#[tauri::command]
fn clear_crash_events(state: State<'_, Arc<AppState>>) -> Result<usize, String> {
    state.db.clear_crash_events()
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

/// Bring the main window forward. Shared by the `show_main` command and the
/// YV42 single-instance handler (a second launch focuses this window instead of
/// starting a rival process).
fn focus_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
fn show_main(app: AppHandle) {
    focus_main_window(&app);
}

/// Push the `autostart` setting into the OS (YV42) — a macOS LaunchAgent via
/// tauri-plugin-autostart. Called at startup and on every save so the setting,
/// not a LaunchAgent left behind by an older install or a moved .app, is the
/// source of truth. A login item that cannot be written is logged, never
/// surfaced as a settings error: the rest of the save already landed.
fn apply_autostart(app: &AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    // Only touch the plist when it actually disagrees — this runs on every
    // launch and on every unrelated settings save.
    match manager.is_enabled() {
        Ok(current) if current == enabled => return,
        Err(e) => log::warn!("could not read the launch-at-login state ({e}); applying anyway"),
        _ => {}
    }
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    match result {
        Ok(()) => log::info!(
            "launch at login {}",
            if enabled { "enabled" } else { "disabled" }
        ),
        Err(e) => log::warn!(
            "could not {} launch at login: {e}",
            if enabled { "enable" } else { "disable" }
        ),
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

/// A newer Yap release the user has not dismissed (YV44). Returned by
/// `check_for_update` and rendered as the "Update available" prompt; the
/// release bytes are NOT touched until the user asks for them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
}

/// Has the user pressed "Skip this version" on exactly this release (YV44)?
/// Pure so the skip rule is testable: only that one version is suppressed, so a
/// later release is always offered again.
fn update_is_skipped(version: &str, skipped: Option<&str>) -> bool {
    skipped.is_some_and(|s| s == version)
}

/// Ask the release endpoint whether a newer Yap exists — and ONLY ask (YV44).
/// Nothing downloads, nothing installs, nothing relaunches here; the answer is
/// handed to the UI, which prompts. Returns `None` when updates are turned off,
/// when there is nothing newer, when the user skipped this exact version, or
/// when no manifest is published yet (GitHub answers 404 for
/// `releases/latest/download/latest.json` until the first release exists —
/// that is the NORMAL state, so it is DEBUG, not the launch-time ERROR the
/// plugin logs on its own). A genuine failure (offline, malformed manifest) is
/// returned as an error so the manual "Check for updates" button can say so;
/// the launch-time check ignores it.
#[tauri::command]
async fn check_for_update(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<UpdateInfo>, String> {
    let (enabled, skipped) = {
        let s = state.settings.lock();
        (s.check_updates, s.skipped_update_version.clone())
    };
    if !enabled {
        log::debug!("update check off (checkUpdates=false) — not contacting the endpoint");
        return Ok(None);
    }
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            log::debug!("updater unavailable in this build ({e})");
            return Ok(None);
        }
    };
    match updater.check().await {
        Ok(Some(update)) => {
            if update_is_skipped(&update.version, skipped.as_deref()) {
                log::debug!(
                    "update {} available but skipped by the user",
                    update.version
                );
                return Ok(None);
            }
            log::info!(
                "update available: {} (running {})",
                update.version,
                update.current_version
            );
            Ok(Some(UpdateInfo {
                version: update.version.clone(),
                current_version: update.current_version.clone(),
                notes: update.body.clone(),
            }))
        }
        Ok(None) => {
            log::debug!("update check: already on the latest Yap");
            Ok(None)
        }
        Err(tauri_plugin_updater::Error::ReleaseNotFound) => {
            log::debug!("update check: no release manifest published yet — nothing to update to");
            Ok(None)
        }
        Err(e) => {
            log::warn!("update check failed: {e}");
            Err(e.to_string())
        }
    }
}

/// Download + install a release the user just approved (YV44). This is the ONLY
/// path in Yap that calls `download_and_install`, and the only caller is the
/// "Install now" button in the update prompt — no launch-time install, ever.
/// Re-checks first so the bytes installed are the release the prompt named
/// (signature verification against `plugins.updater.pubkey` happens inside the
/// plugin). The new bundle applies on the next launch; we do not restart the
/// app out from under a dictation.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<String, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No update available anymore — you're on the latest Yap.".to_string())?;
    let version = update.version.clone();
    log::info!("installing Yap {version} (user-approved)");
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| {
            log::error!("update install failed: {e}");
            e.to_string()
        })?;
    log::info!("Yap {version} installed — applies on relaunch");
    Ok(version)
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

    // YV41: never reset a whole config over one bad field — salvage what parses,
    // quarantine a wholly corrupt file, migrate older schemas forward.
    let settings_path = data_dir().join("settings.json");
    let mut settings: AppSettings = load_settings(&settings_path);

    // YV46: the pre-SQLite `history/transcripts.json` import is a FIRST-RUN
    // step, not a per-launch one. It used to stat that path on every startup
    // for a file no install made after the SQLite switch can have; now the one
    // attempt is recorded in settings and never repeated (the import itself
    // also renames the file aside, so this is belt-and-braces, not the only
    // guard). Persisting the flag is best-effort: a failed write just means the
    // next launch re-checks a file that is no longer there.
    if !settings.legacy_json_migrated {
        let legacy = data_dir().join("history").join("transcripts.json");
        match db.migrate_json_if_needed(legacy) {
            Ok(n) if n > 0 => log::info!("migrated {n} legacy transcripts into SQLite"),
            Err(e) => log::warn!("legacy migrate: {e}"),
            _ => {}
        }
        settings.legacy_json_migrated = true;
        if let Err(e) = write_settings_file(&settings_path, &settings) {
            log::warn!("could not record the legacy-migration flag ({e})");
        }
    }
    // YV52 retention: recoverable takes (and their audio) live 7 days, then go —
    // same "audio never lingers" rule as the sweep above, just with the window a
    // retry needs. Runs after the DB is open and before anything can list them.
    purge_expired_failed_takes(&db);
    // YV63: anything the previous run was still capturing when it died is on
    // disk as a spill + an in-progress marker. Rebuild those takes into rows the
    // user can retry — after the purge, so a freshly recovered take always gets
    // its own full retention window.
    let rebuilt = recover_crashed_takes(&db, &recovery_dir());
    if rebuilt > 0 {
        log::info!("startup: recovered {rebuilt} dictation(s) the app died in the middle of");
    }
    // YV64: the app has been crashing on real machines without ever knowing it.
    // Read macOS' own reports for our processes, plus the panic hook's own log
    // lines, into `crash_events` — the UI raises ONE toast if any of them is
    // news. Local only: nothing here is ever uploaded (see `crash`'s docs).
    let fresh_crashes = crash::ingest(&db, &home_dir(), &logging::logs_dir(&data_dir()));
    if fresh_crashes > 0 {
        log::warn!("startup: {fresh_crashes} crash(es) from previous session(s) recorded");
    }

    let db = Arc::new(db);

    let state = Arc::new(AppState {
        settings: PLMutex::new(settings),
        recording: PLMutex::new(false),
        busy: PLMutex::new(false),
        hands_free: PLMutex::new(false),
        command_selection: PLMutex::new(None),
        recorder: PLMutex::new(None),
        saved_audio: PLMutex::new(None),
        db,
        last_error: PLMutex::new(None),
        hotkey_registered: PLMutex::new(false),
        tray: PLMutex::new(None),
        tray_dictation: PLMutex::new(None),
        tray_hands_free: PLMutex::new(None),
        tray_paste_raw: PLMutex::new(None),
        undo_available: PLMutex::new(false),
        secure_input: PLMutex::new(secure_input::SecureInputStatus::default()),
        vad: PLMutex::new(None),
        paste_generation: AtomicU64::new(0),
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

    // YV38: warm the ASR engine at launch instead of making the user's FIRST
    // take pay the multi-second model load. No delay in front of it — the load
    // is blocking, so it goes straight onto the blocking pool (never the main
    // thread), and it is a no-op when no model has been downloaded yet. The load
    // is already panic-contained + idempotent inside the manager (and the idle
    // watcher unloads it again after 15 minutes of no dictation), so the only
    // thing this changes is WHO pays for the cold load.
    {
        let state = state.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let native = state.settings.lock().native_model.clone();
            let Some((model_id, model_path)) = native_model_ready(&native) else {
                log::info!("startup: no downloaded ASR model — engine preload skipped");
                return;
            };
            match state.transcription.load(&model_id, &model_path) {
                Ok(()) => log::info!("startup: ASR engine preloaded ({model_id})"),
                Err(e) => {
                    log::warn!("startup: ASR engine preload failed ({e}) — the take path retries")
                }
            }
        });
    }

    tauri::Builder::default()
        // YV42 single instance — MUST stay the FIRST plugin (Handy's order): a
        // second `open -a Yap` is caught here and handed to the running app
        // before any other plugin in this chain has taken a lock the duplicate
        // would grab too. Two live processes would fight over the CGEvent FN
        // tap, the ⌃⌘V global shortcut, the pasteboard transaction and the
        // SQLite WAL. The headless `--transcribe-file` run never reaches this
        // builder (cli::handle_args exits above), so a CLI transcription still
        // works while the app is open.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            log::info!("second instance launched — focusing the running app");
            focus_main_window(app);
        }))
        // YV42 launch at login. The LaunchAgent variant is the macOS-supported
        // one (no deprecated login-item AppleScript); the `autostart` setting is
        // applied in setup() below and on every save, so it stays authoritative.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
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
                        // YV51 ⌃⌘Z — Undo AI edit: re-paste the raw take over
                        // the polished one. Inert (logged no-op) when the raw
                        // matches what was pasted.
                        let undo_sc =
                            Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SUPER), Code::KeyZ);
                        if shortcut == &undo_sc {
                            if event.state == ShortcutState::Pressed {
                                log::info!("⌃⌘Z pressed — undo ai edit (paste raw)");
                                paste_raw_last_transcript(app, &state);
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
            pill_set_hitbox,
            pill_drag_start,
            pill_drag_move,
            pill_drag_end,
            get_history,
            clear_history,
            delete_entry,
            get_status,
            get_permissions,
            request_accessibility,
            request_microphone,
            get_insights,
            daily_series,
            monthly_series,
            list_dictionary,
            add_dictionary_term,
            update_dictionary_term,
            set_dictionary_starred,
            delete_dictionary_term,
            correct_transcript,
            list_failed_dictations,
            retry_failed_dictation,
            discard_failed_dictation,
            list_dict_candidates,
            promote_dict_candidate,
            dismiss_dict_candidate,
            list_snippets,
            add_snippet,
            update_snippet,
            set_snippet_enabled,
            delete_snippet,
            list_scratch,
            save_scratch,
            delete_scratch,
            copy_entry,
            paste_entry,
            open_data_dir,
            open_logs_dir,
            list_crash_events,
            acknowledge_crash_events,
            clear_crash_events,
            export_history,
            open_privacy_settings,
            manual_toggle,
            show_main,
            list_models,
            download_model,
            select_model,
            delete_model,
            engine_status,
            check_for_update,
            install_update
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

            // YV42: re-assert launch-at-login from the persisted setting, so the
            // toggle stays authoritative even if the .app moved (which strands
            // the old LaunchAgent) or an older install left one behind.
            apply_autostart(app.handle(), state.settings.lock().autostart);

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
            // YV51: seed the "Undo AI Edit" guard from the newest stored take so
            // the item is correctly enabled/disabled on launch, not only after
            // the first dictation of the session.
            *state.undo_available.lock() = state
                .db
                .list_transcripts(1, None)
                .ok()
                .and_then(|e| e.into_iter().next())
                .map(|e| dictation::undo_ai_edit_text(&e.text, e.raw_text.as_deref()).is_some())
                .unwrap_or(false);
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
            // YV51 "Undo AI Edit" (⌃⌘Z) — re-pastes the RAW take over the
            // polished one. Starts disabled and is enabled by `sync_tray` only
            // when the newest take's raw actually differs from what was pasted.
            let paste_raw_i = MenuItem::with_id(
                app,
                "paste_raw",
                "Undo AI Edit (Paste Raw)",
                *state.undo_available.lock(),
                Some("Ctrl+Cmd+Z"),
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
                    &paste_raw_i,
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
            *state.tray_paste_raw.lock() = Some(paste_raw_i);
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
                        "paste_raw" => {
                            paste_raw_last_transcript(app, &state);
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

            // Glass island: always-on + rides all Spaces (re-park loop). The
            // dock edge (YV53) is applied BEFORE the first show so the pill
            // never appears bottom-centre and then jumps to the chosen edge.
            float_pill::set_position(float_pill::PillPosition::from_settings(
                &state.settings.lock().pill_position,
            ));
            if state.settings.lock().show_floating_pill {
                if let Err(e) = float_pill::show_float(app.handle()) {
                    log::warn!("float pill: {e}");
                }
                float_pill::start_space_keeper(app.handle().clone());
            } else {
                float_pill::hide_float(app.handle());
            }
            // YV65 drag-to-dock: the capsule is only cursor-interactive while
            // the pointer is on it. Started unconditionally — the pill still
            // appears for a dictation when the always-on setting is off, and the
            // watch is inert while the panel is hidden.
            float_pill::start_hover_watch(app.handle().clone());

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
                let command_binding = ptt_macos::CommandBinding::from_settings(
                    &state.settings.lock().command_binding,
                );
                let st = state.clone();
                let h = app.handle().clone();
                ptt_macos::start(
                    binding,
                    command_binding,
                    Arc::new(move |ev| {
                        let st = st.clone();
                        let h = h.clone();
                        if let Err(e) = h.clone().run_on_main_thread(move || match ev {
                            ptt_macos::PttEvent::Start => {
                                // YV49: a press that carried the command modifier
                                // EDITS the current selection instead of typing.
                                // Read the selection here, at press time — by the
                                // time ASR returns, focus and selection may both
                                // have moved. No selection → nothing to edit, so
                                // the take never starts (just a toast).
                                if ptt_macos::command_mode_active() {
                                    match focus::selected_text() {
                                        Some(selection) => {
                                            *st.command_selection.lock() = Some(selection);
                                            start_recording(&h, &st);
                                        }
                                        None => {
                                            *st.command_selection.lock() = None;
                                            log::info!(
                                                "command mode: no selection — take not started"
                                            );
                                            let _ = h.emit(
                                                PASTE_OUTCOME_EVENT,
                                                command_mode::NO_SELECTION_MESSAGE,
                                            );
                                        }
                                    }
                                } else {
                                    *st.command_selection.lock() = None;
                                    start_recording(&h, &st);
                                }
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
                log::info!(
                    "PTT hybrid started ({} · command mode {})",
                    binding.label(),
                    command_binding.label()
                );
            }

            // YV43: watch for the one condition that kills the tap above without
            // any error — another app enabling macOS Secure Input. The watchdog
            // only calls back on an edge, so this is a state write + one status
            // emit per episode, not per poll. There is no Carbon fallback to
            // wire: RegisterEventHotKey cannot express the fn modifier that
            // every Yap binding uses, so the honest status IS the remedy.
            {
                let st = state.clone();
                let h = app.handle().clone();
                secure_input::start(move |snapshot| {
                    *st.secure_input.lock() = snapshot;
                    emit_status(&h, &st);
                });
            }

            // Global ⌃⌘V (Paste Last Transcript, always on) + optional ⌘⇧V
            // dictation toggle (off by default).
            // YV38: no fixed startup delay. `run_on_main_thread` queues this on
            // the event loop, so registration happens the moment the loop is
            // running — which is exactly the event the old 600 ms sleep was
            // guessing at, except the shortcuts are live from launch instead of
            // dead for the first half-second.
            {
                let h = app.handle().clone();
                let st = state.clone();
                let _ = app.handle().run_on_main_thread(move || {
                    // ⌃⌘V — Paste Last Transcript, registered unconditionally so
                    // the tray item's accelerator fires system-wide (Wispr-parity).
                    let paste_sc =
                        Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SUPER), Code::KeyV);
                    match h.global_shortcut().register(paste_sc) {
                        Ok(()) => log::info!("⌃⌘V paste-last registered"),
                        Err(e) => log::warn!("⌃⌘V register failed: {e}"),
                    }
                    // YV51 ⌃⌘Z — Undo AI edit (paste the raw take). Also
                    // registered unconditionally so the tray item's accelerator
                    // fires system-wide. ⌃⌘Z is free on macOS (undo is ⌘Z and
                    // redo is ⇧⌘Z), so it does not shadow an app shortcut; if
                    // another app has claimed it, registration fails loudly here
                    // and the tray item still works.
                    let undo_sc =
                        Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SUPER), Code::KeyZ);
                    match h.global_shortcut().register(undo_sc) {
                        Ok(()) => log::info!("⌃⌘Z undo-ai-edit registered"),
                        Err(e) => log::warn!("⌃⌘Z register failed: {e}"),
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
            }

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
    use super::{
        apply_settings_migrations, load_settings, salvage_settings, status_message,
        update_is_skipped, write_settings_file, AppSettings, CURRENT_SETTINGS_SCHEMA_VERSION,
    };

    /// A fresh per-test directory (tests run in parallel — never share one).
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("yv41-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The defaults as they land on disk, as a mutable JSON object.
    fn default_settings_json() -> serde_json::Value {
        serde_json::to_value(AppSettings::default()).unwrap()
    }

    // --- Context awareness v1 (YV50) privacy ------------------------------
    //
    // The pre-caret context is the most sensitive thing Yap touches: whatever
    // the user already typed in their editor, mail draft or chat. The product
    // rule is that it exists ONLY inside the dictation call stack — these two
    // tests are the enforcement, one over the shipped source, one over a real
    // history row written through the real DB.

    /// No `log::` statement anywhere in the pipeline may interpolate the
    /// context, and the two modules that touch it must not log at all.
    #[test]
    fn cursor_context_is_never_logged_or_persisted() {
        const LIB: &str = include_str!("lib.rs");

        // 1. The binding exists and is only ever consumed by the two pure
        //    formatting calls — never by a log, an event emit, or the DB write.
        let mut uses = 0;
        for (i, line) in LIB.lines().enumerate() {
            if !line.contains("cursor_context") || line.trim_start().starts_with("//") {
                continue;
            }
            uses += 1;
            for banned in [
                "log::",
                "println!",
                "eprintln!",
                ".emit(",
                "insert_transcript",
                "notify(",
            ] {
                assert!(
                    !line.contains(banned),
                    "line {}: pre-caret context reaches `{banned}`: {line}",
                    i + 1
                );
            }
        }
        assert!(uses >= 3, "YV50 context binding missing from the pipeline");

        // 2. Same check against whole (multi-line) log macro invocations, which
        //    a line-by-line scan would miss.
        for block in log_macro_blocks(LIB) {
            assert!(
                !block.contains("cursor_context"),
                "pre-caret context interpolated into a log statement: {block}"
            );
        }

        // 3. The AX reader and the pure formatter never log anything at all.
        for (name, src) in [
            ("focus.rs", include_str!("focus.rs")),
            ("dictation.rs", include_str!("dictation.rs")),
        ] {
            assert!(
                !src.contains("log::"),
                "{name} must stay log-free — it handles the raw context string"
            );
        }
    }

    /// Every `log::` macro invocation in `src`, from `log::` to the `;` that
    /// closes it (semicolons inside the argument list / string literals sit at
    /// paren depth > 0 and are skipped).
    fn log_macro_blocks(src: &str) -> Vec<&str> {
        let mut blocks = Vec::new();
        let bytes = src.as_bytes();
        let mut from = 0;
        while let Some(rel) = src[from..].find("log::") {
            let start = from + rel;
            // Skip mentions that are not invocations: comments and the string
            // literals this very audit uses.
            let line_start = src[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let prefix = src[line_start..start].trim_start();
            if prefix.starts_with("//") || prefix.ends_with('"') {
                from = start + 5;
                continue;
            }
            let mut depth = 0i32;
            let mut end = start;
            for (i, &b) in bytes[start..].iter().enumerate() {
                match b {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    b';' if depth <= 0 => {
                        end = start + i;
                        break;
                    }
                    _ => {}
                }
            }
            if end > start {
                blocks.push(&src[start..end]);
            }
            from = start + 5;
        }
        assert!(blocks.len() > 10, "log scan found nothing — audit is broken");
        blocks
    }

    /// A dictation whose formatting was steered by the surrounding text stores
    /// the DICTATED words only: no fragment of the context reaches the row, the
    /// raw column, or the JSON the history UI receives.
    #[test]
    fn context_never_reaches_the_history_row() {
        // What the user had already typed (would be a privacy leak if stored).
        let context = "Dear Wilson, thanks for the invoice — the wire went out on";
        let dictated = "Friday afternoon";

        // The context steers the take: mid-sentence ⇒ joining space + lowercase.
        let text = crate::dictation::join_with_context(dictated, Some(context));
        assert_eq!(text, " friday afternoon");

        let dir = temp_dir("yv50-context-privacy");
        let db = crate::db::Database::open(dir.join("wilson_voice.db")).unwrap();
        let entry = db
            .insert_transcript_at(
                text,
                "native".into(),
                1.0,
                2.0,
                100,
                Some("Mail".into()),
                chrono::Utc::now(),
                Some(dictated.into()),
            )
            .unwrap();

        // Check the returned row, the row read back out of SQLite, and the JSON
        // shape the frontend gets — no context word may appear in any of them.
        let listed = db.list_transcripts(10, None).unwrap();
        assert_eq!(listed.len(), 1);
        let surfaces = [
            serde_json::to_string(&entry).unwrap(),
            serde_json::to_string(&listed).unwrap(),
        ];
        for surface in &surfaces {
            for word in context.split_whitespace().filter(|w| w.len() > 4) {
                assert!(
                    !surface.contains(word),
                    "context word {word:?} leaked into history: {surface}"
                );
            }
            // The dictated words themselves are of course still there.
            assert!(surface.contains("friday afternoon"), "{surface}");
        }

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // YV33: "Ready" is a claim about being able to transcribe. With no usable
    // model the status line must say a model is needed — the old rule ("a file
    // exists at the resolved interpreter path") reported Ready on every fresh
    // Mac, where that path was the non-functional Command Line Tools shim.
    #[test]
    fn status_says_model_needed_until_a_model_is_ready() {
        let no_model = status_message(false, false, false, None, false, true, false, "fn⌃");
        assert!(no_model.contains("Model needed"), "{no_model}");
        assert!(!no_model.contains("Ready"), "{no_model}");

        let ready = status_message(false, false, false, None, true, true, false, "fn⌃");
        assert!(ready.starts_with("Ready — hold fn⌃"), "{ready}");

        // Accessibility is a paste-only concern: still Ready, with a nudge.
        let no_ax = status_message(false, false, false, None, true, false, false, "fn⌃");
        assert!(no_ax.starts_with("Ready —"), "{no_ax}");
        assert!(no_ax.contains("Accessibility"), "{no_ax}");

        // Live states and hard errors still outrank the model gate.
        assert!(
            status_message(true, false, false, None, false, true, false, "fn⌃")
                .contains("Recording")
        );
        assert!(
            status_message(true, true, false, None, false, true, false, "fn⌃")
                .contains("Hands-free")
        );
        assert!(
            status_message(false, false, true, None, false, true, false, "fn⌃")
                .contains("Transcribing")
        );
        let err = status_message(false, false, false, Some("boom"), true, true, false, "fn⌃");
        assert_eq!(err, "Error: boom");
    }

    // YV43: while another app holds Secure Input the fn tap is blind, so the
    // idle line must never keep advertising the key — it must name the cause.
    #[test]
    fn status_reports_secure_input_instead_of_advertising_the_dead_hotkey() {
        let blocked = status_message(false, false, false, None, true, true, true, "fn⌃");
        assert_eq!(blocked, crate::secure_input::BLOCKED_MESSAGE);
        assert!(!blocked.contains("Ready"), "{blocked}");

        // It also outranks the model gate — pointing a user at a download does
        // not fix a keyboard they cannot reach the app with.
        let blocked_no_model = status_message(false, false, false, None, false, true, true, "fn⌃");
        assert_eq!(blocked_no_model, crate::secure_input::BLOCKED_MESSAGE);

        // But a live take and a real error still win: those describe what just
        // happened, not an instruction the user cannot follow.
        assert!(
            status_message(true, false, false, None, true, true, true, "fn⌃").contains("Recording")
        );
        assert_eq!(
            status_message(false, false, false, Some("boom"), true, true, true, "fn⌃"),
            "Error: boom"
        );

        // Cleared → straight back to the normal Ready line.
        assert!(
            status_message(false, false, false, None, true, true, false, "fn⌃")
                .starts_with("Ready — hold fn⌃")
        );
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

    // YV42: launch-at-login is OFF unless the user asked for it — a fresh
    // install and a store written before the field existed must both read
    // false, so no LaunchAgent is ever installed behind the user's back. When
    // it IS on it round-trips (and survives salvage) on its wire key.
    #[test]
    fn autostart_defaults_off_and_round_trips() {
        assert!(!AppSettings::default().autostart);

        let legacy = r#"{
            "language": "en",
            "autoPaste": true,
            "hotkeyLabel": "fn⌃",
            "showFloatingPill": true
        }"#;
        let parsed: AppSettings = serde_json::from_str(legacy).expect("legacy parse");
        assert!(
            !parsed.autostart,
            "a pre-YV42 store must not enable autostart"
        );

        let mut on = AppSettings::default();
        on.autostart = true;
        let json = serde_json::to_string(&on).expect("serialize autostart");
        assert!(json.contains("\"autostart\":true"));
        let back: AppSettings = serde_json::from_str(&json).expect("autostart round-trip");
        assert!(back.autostart);

        // A different field being corrupt must not silently turn it back off.
        let mut stored = serde_json::to_value(&on).expect("autostart settings to value");
        stored
            .as_object_mut()
            .unwrap()
            .insert("denoise".into(), serde_json::json!("sometimes"));
        assert!(salvage_settings(&stored).autostart);
    }

    // YV44: the update check is on by default (it only ever notifies), stays a
    // user-owned switch, and survives a store written before the field existed.
    #[test]
    fn check_updates_defaults_on_and_round_trips() {
        assert!(AppSettings::default().check_updates);
        assert!(AppSettings::default().skipped_update_version.is_none());

        let legacy = r#"{
            "language": "en",
            "autoPaste": true,
            "hotkeyLabel": "fn⌃",
            "showFloatingPill": true
        }"#;
        let parsed: AppSettings = serde_json::from_str(legacy).expect("legacy parse");
        assert!(
            parsed.check_updates,
            "a pre-YV44 store must still get update notifications"
        );

        let mut off = AppSettings::default();
        off.check_updates = false;
        let json = serde_json::to_string(&off).expect("serialize checkUpdates");
        assert!(json.contains("\"checkUpdates\":false"));
        let back: AppSettings = serde_json::from_str(&json).expect("checkUpdates round-trip");
        assert!(!back.check_updates, "turning updates off must persist");

        // A different field being corrupt must not silently re-enable it.
        let mut stored = serde_json::to_value(&off).expect("update settings to value");
        stored
            .as_object_mut()
            .unwrap()
            .insert("denoise".into(), serde_json::json!("sometimes"));
        assert!(!salvage_settings(&stored).check_updates);
    }

    // YV44: "Skip this version" suppresses exactly the version the user
    // dismissed — the next release is still offered.
    #[test]
    fn skipped_version_suppresses_only_that_version() {
        assert!(update_is_skipped("0.7.0", Some("0.7.0")));
        assert!(!update_is_skipped("0.7.1", Some("0.7.0")));
        assert!(!update_is_skipped("0.7.0", None));

        let mut skipped = AppSettings::default();
        skipped.skipped_update_version = Some("0.7.0".into());
        let json = serde_json::to_string(&skipped).expect("serialize skippedUpdateVersion");
        assert!(json.contains("\"skippedUpdateVersion\":\"0.7.0\""));
        let back: AppSettings = serde_json::from_str(&json).expect("skip round-trip");
        assert_eq!(back.skipped_update_version.as_deref(), Some("0.7.0"));
    }

    // YV41: ONE unparseable field used to throw away the entire settings.json
    // (hotkey, model, tone, the onboarded flag). Salvage keeps every field that
    // is individually valid and defaults only the broken ones.
    #[test]
    fn salvage_keeps_every_valid_field_when_one_is_corrupt() {
        let mut stored = default_settings_json();
        let map = stored.as_object_mut().unwrap();
        map.insert("onboarded".into(), serde_json::json!(true));
        map.insert("pttBinding".into(), serde_json::json!("fn"));
        map.insert(
            "nativeModel".into(),
            serde_json::json!("handy-computer/whisper-tiny-gguf"),
        );
        map.insert(
            "calibrationSample".into(),
            serde_json::json!("the quick brown fox"),
        );
        // A value with the wrong type, and one a newer build could have written.
        map.insert("denoise".into(), serde_json::json!("sometimes"));
        map.insert("companionTone".into(), serde_json::json!(42));

        // Precondition: this is exactly the whole-store parse failure that used
        // to reset everything to defaults.
        assert!(serde_json::from_value::<AppSettings>(stored.clone()).is_err());

        let salvaged = salvage_settings(&stored);
        assert!(salvaged.onboarded, "the onboarded flag must survive");
        assert_eq!(salvaged.ptt_binding, "fn");
        assert_eq!(salvaged.native_model, "handy-computer/whisper-tiny-gguf");
        assert_eq!(
            salvaged.calibration_sample.as_deref(),
            Some("the quick brown fox")
        );
        // Only the two broken fields fall back.
        assert!(salvaged.denoise);
        assert_eq!(salvaged.companion_tone, "friendly");
    }

    // A key this build has never heard of (written by a newer version) is
    // ignored, not treated as corruption.
    #[test]
    fn salvage_tolerates_unknown_keys() {
        let mut stored = default_settings_json();
        let map = stored.as_object_mut().unwrap();
        map.insert(
            "fieldFromTheFuture".into(),
            serde_json::json!({ "nested": true }),
        );
        map.insert("language".into(), serde_json::json!("fr"));
        map.insert("cleanupLevel".into(), serde_json::json!(7));

        let salvaged = salvage_settings(&stored);
        assert_eq!(salvaged.language, "fr");
        assert_eq!(salvaged.cleanup_level, "light");
    }

    // End-to-end through the file: one bad field, everything else survives, and
    // the good file is NOT quarantined.
    #[test]
    fn load_settings_salvages_one_bad_field_instead_of_resetting_everything() {
        let dir = temp_dir("salvage");
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{
                "schemaVersion": 1,
                "language": "en",
                "autoPaste": true,
                "hotkeyLabel": "fn",
                "showFloatingPill": true,
                "pttBinding": "fn",
                "onboarded": true,
                "nativeModel": "handy-computer/whisper-tiny-gguf",
                "cleanupLevel": ["high"]
            }"#,
        )
        .unwrap();

        let loaded = load_settings(&path);
        assert!(loaded.onboarded);
        assert_eq!(loaded.ptt_binding, "fn");
        assert_eq!(loaded.native_model, "handy-computer/whisper-tiny-gguf");
        assert_eq!(loaded.cleanup_level, "light", "only the bad field defaults");
        assert!(path.exists(), "a salvageable file is not quarantined");
        assert!(!dir.join("settings.json.bak").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // A wholly corrupt file (truncated by a crash mid-write, or not even an
    // object) falls back to defaults, but is KEPT as .bak — never deleted.
    #[test]
    fn load_settings_quarantines_a_corrupt_file_as_bak() {
        let dir = temp_dir("corrupt");
        let path = dir.join("settings.json");
        let bak = dir.join("settings.json.bak");

        for garbage in [r#"{"language": "en", "autoPas"#, r#""not an object""#] {
            let _ = std::fs::remove_file(&bak);
            std::fs::write(&path, garbage).unwrap();

            let loaded = load_settings(&path);
            assert_eq!(
                serde_json::to_value(&loaded).unwrap(),
                default_settings_json(),
                "corrupt store falls back to defaults"
            );
            assert!(!path.exists(), "the corrupt file is moved aside");
            assert_eq!(
                std::fs::read_to_string(&bak).unwrap(),
                garbage,
                "the corrupt bytes are kept verbatim for recovery"
            );
        }

        // Missing file (fresh install) is not an error and leaves no .bak.
        let _ = std::fs::remove_file(&bak);
        assert!(!load_settings(&path).onboarded);
        assert!(!bak.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // The write is atomic (temp file + rename in the same dir, no leftovers) and
    // merges unknown keys forward, so a save from a build that does not know a
    // field cannot delete it.
    #[test]
    fn settings_write_is_atomic_and_merges_unknown_keys_forward() {
        let dir = temp_dir("write");
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"language": "de", "fieldFromTheFuture": {"nested": true}}"#,
        )
        .unwrap();

        let mut settings = AppSettings::default();
        settings.language = "en".into();
        settings.onboarded = true;
        write_settings_file(&path, &settings).expect("atomic write");

        let on_disk: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk["language"], serde_json::json!("en"));
        assert_eq!(on_disk["onboarded"], serde_json::json!(true));
        assert_eq!(
            on_disk["fieldFromTheFuture"],
            serde_json::json!({ "nested": true }),
            "a key this build does not know must survive the save"
        );
        assert!(
            !dir.join("settings.json.tmp").exists(),
            "the temp file is renamed into place, not left behind"
        );
        assert_eq!(load_settings(&path).language, "en");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // YV41 schema versioning: a store written before the field existed is
    // version 0, gets migrated + rewritten ONCE, and is left alone afterwards.
    #[test]
    fn schema_version_migration_runs_once_then_stops() {
        let dir = temp_dir("migrate");
        let path = dir.join("settings.json");
        // A pre-YV41 store naming the retired Python-sidecar model id.
        std::fs::write(
            &path,
            r#"{
                "language": "en",
                "autoPaste": true,
                "hotkeyLabel": "fn⌃",
                "showFloatingPill": true,
                "onboarded": true,
                "nativeModel": "whisper-large-v3-turbo"
            }"#,
        )
        .unwrap();

        let migrated = load_settings(&path);
        assert_eq!(migrated.schema_version, CURRENT_SETTINGS_SCHEMA_VERSION);
        assert!(migrated.onboarded, "migration keeps the user's config");
        assert_eq!(
            migrated.native_model,
            super::default_native_model(),
            "a model the bundled catalog does not have is reset once"
        );

        // The rewrite landed, so the next launch is a no-op.
        let stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            stored["schemaVersion"],
            serde_json::json!(CURRENT_SETTINGS_SCHEMA_VERSION)
        );
        let mut again: AppSettings = serde_json::from_value(stored.clone()).unwrap();
        assert!(
            !apply_settings_migrations(&mut again, &stored),
            "a current-schema store must not be rewritten on every read"
        );

        // A version-0 store whose model IS in the catalog keeps its selection —
        // the migration only touches what it cannot resolve.
        let keeps = serde_json::json!({
            "language": "en",
            "nativeModel": "handy-computer/whisper-tiny-gguf",
        });
        let mut settings: AppSettings = serde_json::from_value(keeps.clone()).unwrap();
        assert!(apply_settings_migrations(&mut settings, &keeps));
        assert_eq!(settings.native_model, "handy-computer/whisper-tiny-gguf");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // YV46: the legacy JSON import is armed exactly once. A store written
    // before the flag existed reads as "not yet migrated"; once the flag is
    // recorded it survives every later load and save, so no launch after the
    // first ever looks for history/transcripts.json again.
    #[test]
    fn legacy_json_migration_is_armed_once_then_recorded() {
        let dir = temp_dir("legacy-migrate");
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"schemaVersion": 1, "onboarded": true}"#).unwrap();

        let mut settings = load_settings(&path);
        assert!(
            !settings.legacy_json_migrated,
            "a store from before the flag still gets its one import attempt"
        );

        settings.legacy_json_migrated = true;
        write_settings_file(&path, &settings).expect("record the flag");
        assert!(
            load_settings(&path).legacy_json_migrated,
            "the flag is durable, so the next launch skips the import"
        );

        // A later save (which carries the flag through from the live settings)
        // must not re-arm it.
        let reloaded = load_settings(&path);
        write_settings_file(&path, &reloaded).expect("ordinary save");
        assert!(load_settings(&path).legacy_json_migrated);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// YV63 — the crash half of the promise. A take the app died in the middle
    /// of leaves an in-progress marker and its spilled audio in the recovery
    /// dir; the startup scan must turn that into a real WAV and the SAME
    /// failed-dictation row (YV52) a failed transcription gets, so the words are
    /// one Retry away instead of gone.
    #[test]
    fn orphaned_marker_becomes_failed_dictation_row_on_startup() {
        let dir = temp_dir("yv63-crash-recovery");
        let db = crate::db::Database::open(dir.join("wilson_voice.db")).unwrap();
        let recovery = dir.join("recovery");

        // A take in flight: half a second already spilled, then the app dies.
        let journal = crate::record::CaptureJournal::start(&recovery).expect("journal opens");
        let spoken: Vec<f32> = (0..8_000)
            .map(|i| 0.4 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin())
            .collect();
        journal.append(&spoken);
        journal.abandon();

        assert_eq!(
            super::recover_crashed_takes(&db, &recovery),
            1,
            "the orphaned take must be recovered at startup"
        );

        let rows = db.list_failed_dictations().unwrap();
        assert_eq!(rows.len(), 1, "the recovered take gets a retryable row");
        assert_eq!(rows[0].error, super::CRASH_RECOVERY_ERROR);
        assert!((rows[0].speech_seconds - 0.5).abs() < 0.01);
        // The row points at a real, parseable WAV inside the recovery dir — the
        // same lifecycle (Retry, discard, 7-day purge) the YV52 clips live in.
        let wav = std::path::PathBuf::from(&rows[0].wav_path);
        assert!(
            wav.starts_with(&recovery),
            "recovered audio stays in {recovery:?}"
        );
        let samples = crate::record::read_wav_16k_mono(&wav).expect("recovered wav parses");
        assert_eq!(samples.len(), spoken.len());

        // Idempotent: the next launch must not re-recover the same take.
        assert_eq!(super::recover_crashed_takes(&db, &recovery), 0);
        assert_eq!(db.list_failed_dictations().unwrap().len(), 1);

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- YV67: no take that already wrote a WAV may vanish -----------------
    //
    // `failed_dictations` sat at 0 rows against 469 transcripts because YV52
    // recovery was wired ONLY to the `Err` arm. A gate rejection took the
    // `Ok(Soft)` arm, never took `pending`, and `ClipWav::drop` unlinked the
    // only copy of the audio — no transcript, no recovery row, nothing.

    /// What a finished take owes the audio it already wrote to disk.
    #[derive(Debug, PartialEq)]
    enum AudioDuty {
        /// It became a transcript row; the history wav is disposable.
        Transcribed,
        /// Nothing worth keeping was captured (fumbled tap, command edit).
        Discardable,
        /// A real wav exists and produced NO transcript — it must survive as a
        /// retryable row, or the user's words are gone for good.
        MustPersist,
    }

    /// Exhaustive on purpose (no `_` arm): a new `TakeOutcome` variant fails to
    /// COMPILE here until someone decides what happens to the wav it may have
    /// written. That compile error is the whole point of this helper.
    fn audio_duty(outcome: &super::TakeOutcome) -> AudioDuty {
        match outcome {
            super::TakeOutcome::Dictated(..) => AudioDuty::Transcribed,
            super::TakeOutcome::Soft(_) => AudioDuty::Discardable,
            super::TakeOutcome::Rejected { .. } => AudioDuty::MustPersist,
        }
    }

    /// A real 16 kHz WAV on disk plus the guard that owns it — the same shape
    /// the pipeline hands to `keep_failed_take`.
    fn take_wav(dir: &std::path::Path, name: &str) -> (std::path::PathBuf, crate::record::ClipWav) {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("{name}.wav"));
        let spoken: Vec<f32> = (0..16_000)
            .map(|i| 0.4 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin())
            .collect();
        crate::record::write_wav_i16(&path, 16_000, &spoken).unwrap();
        (path.clone(), crate::record::ClipWav::adopt_for_test(path))
    }

    #[test]
    fn every_take_outcome_that_wrote_a_wav_persists_something() {
        assert_eq!(
            audio_duty(&super::TakeOutcome::Soft(super::NO_SPEECH_MESSAGE.into())),
            AudioDuty::Discardable
        );
        let rejected = super::TakeOutcome::Rejected {
            message: super::NO_SPEECH_MESSAGE.into(),
            reason: super::GATE_REPETITION_REASON,
        };
        assert_eq!(audio_duty(&rejected), AudioDuty::MustPersist);

        // Drive the duty the gate arm now honours, against a real DB + wav.
        let super::TakeOutcome::Rejected { reason, .. } = rejected else {
            unreachable!("constructed as Rejected")
        };
        let dir = temp_dir("yv67-rejected-persists");
        let db = crate::db::Database::open(dir.join("wilson_voice.db")).unwrap();
        let recovery = dir.join("recovery");
        let (_original, clip) = take_wav(&dir.join("recordings"), "rejected");

        let row = super::keep_failed_take(&db, &recovery, clip, 1.0, Some("Notes".into()), reason)
            .expect("a rejected take must be recoverable");
        assert_eq!(row.error, super::GATE_REPETITION_REASON);

        let rows = db.list_failed_dictations().unwrap();
        assert_eq!(
            rows.len(),
            1,
            "the rejected take gets exactly one retry row"
        );
        let wav = std::path::PathBuf::from(&rows[0].wav_path);
        assert!(
            wav.starts_with(&recovery),
            "kept audio lives in {recovery:?}"
        );
        assert!(
            wav.exists(),
            "the retry row must point at audio that is there"
        );
        // …and it is NOT history: a gate rejection never becomes a transcript.
        assert!(
            db.list_transcripts(10, None).unwrap().is_empty(),
            "a rejected take must not enter history"
        );

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejected_take_keeps_wav_and_row() {
        let dir = temp_dir("yv67-keeps-wav");
        let db = crate::db::Database::open(dir.join("wilson_voice.db")).unwrap();
        let recovery = dir.join("recovery");
        let recordings = dir.join("recordings");
        let (original, clip) = take_wav(&recordings, "kept");

        // `keep_failed_take` takes the guard BY VALUE, so the `ClipWav` is
        // dropped inside this call. Everything asserted below is therefore
        // asserted AFTER the drop that used to delete the take.
        let row = super::keep_failed_take(
            &db,
            &recovery,
            clip,
            2.5,
            None,
            super::GATE_REPETITION_REASON,
        )
        .expect("the clip is preserved");

        let kept = std::path::PathBuf::from(&row.wav_path);
        assert!(
            kept.exists(),
            "dropping the guard must NOT unlink a kept recovery wav"
        );
        assert!(!original.exists(), "the clip was moved, not copied");
        assert_eq!(
            crate::record::read_wav_16k_mono(&kept).unwrap().len(),
            16_000,
            "the preserved audio is the whole take, and Retry can decode it"
        );
        assert_eq!(db.list_failed_dictations().unwrap()[0].id, row.id);

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
