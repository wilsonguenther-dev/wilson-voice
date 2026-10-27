//! DB-D — the Scratchpad as a real second window.
//!
//! The window is declared in `tauri.conf.json` (label [`WINDOW_LABEL`], entry
//! `scratchpad.html`, hidden, unfocused) and converted ONCE to a
//! non-activating `NSPanel` the first time it opens — the same mechanism the
//! Dictate pill uses (`float_pill.rs`). A non-activating panel can become the
//! KEY window, so you can type and dictate into it, while the app you were in
//! stays the ACTIVE app: Yap never comes forward and the prior app keeps its
//! menu bar, its focus ring and its place in ⌘Tab. That is the whole point of
//! the feature — jot without leaving what you are doing.
//!
//! The consequence for dictation is the dangerous part: with the panel key the
//! FRONTMOST app is still the previous one, so the YV21 app guard alone would
//! paste a take that was dictated into the note into the app behind it. The
//! panel's webview therefore reports which note has focus
//! ([`set_focused_note`], via the `scratchpad_focus` command), the take reads
//! it at the stop edge ([`dictation_note`]) and `paste::take_target` turns it
//! into `PasteTarget::Note` — the one target mechanism, re-pointed.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// The window label in `tauri.conf.json` and in `capabilities/default.json`.
pub const WINDOW_LABEL: &str = "scratchpad";
/// Emitted to the Scratchpad webview with a finished take for a note.
pub const DICTATION_EVENT: &str = "scratchpad-dictation";
/// Emitted to the Scratchpad webview when it is shown (it re-reads its notes).
pub const SHOWN_EVENT: &str = "scratchpad-shown";
/// The `app=` value an insert trace line carries for a note take.
pub const TRACE_APP: &str = "yap.scratchpad";

/// One step of opening the Scratchpad. The live opener runs exactly the list
/// [`open_steps`] returns, so "does not steal focus" is a property of that
/// list (tests/scratchpad.rs) rather than of a comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenStep {
    /// Convert the config-created window to a non-activating NSPanel (once).
    ConvertToNonactivatingPanel,
    /// `orderFrontRegardless` — on screen without activating the app.
    OrderFrontRegardless,
    /// `makeKeyWindow` on the non-activating panel: it takes keystrokes, the
    /// prior app stays active.
    MakeKey,
    /// `NSApp.activate` / `set_focus` on a normal window. Steals focus.
    ActivateApp,
    /// Bring the MAIN window forward. Steals focus.
    PresentMainWindow,
}

impl OpenStep {
    /// Would this step take the active-app status away from the prior app?
    pub fn steals_focus(self) -> bool {
        matches!(self, OpenStep::ActivateApp | OpenStep::PresentMainWindow)
    }
}

/// The steps that open the Scratchpad. `already_panel` = converted on an
/// earlier open (conversion is done once: re-applying the style mask at runtime
/// desyncs the WindowServer non-activating tag, see `float_pill.rs`).
pub fn open_steps(already_panel: bool) -> Vec<OpenStep> {
    let mut steps = Vec::with_capacity(3);
    if !already_panel {
        steps.push(OpenStep::ConvertToNonactivatingPanel);
    }
    steps.push(OpenStep::OrderFrontRegardless);
    steps.push(OpenStep::MakeKey);
    steps
}

/// The note the Scratchpad panel has focused, `None` when the panel is hidden,
/// blurred or showing no note.
static FOCUSED_NOTE: Mutex<Option<String>> = Mutex::new(None);

/// The webview's focus report: `focused` = the panel is key and its editor has
/// `note_id` open. Anything else clears the target.
pub fn set_focused_note(focused: bool, note_id: Option<String>) {
    let mut slot = FOCUSED_NOTE.lock().unwrap_or_else(|p| p.into_inner());
    *slot = if focused {
        note_id.filter(|id| !id.is_empty())
    } else {
        None
    };
}

/// Read at a take's stop edge, beside the frontmost bundle id.
pub fn dictation_note() -> Option<String> {
    FOCUSED_NOTE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DictationPayload<'a> {
    note_id: &'a str,
    text: &'a str,
}

/// Hand a finished take to the Scratchpad webview, which inserts it at the
/// caret of `note_id` and finalizes the note (one version row). `Err` when the
/// panel is gone or hidden — the caller then HOLDS the take on the pill.
pub fn deliver(app: &AppHandle, note_id: &str, text: &str) -> Result<(), String> {
    let w = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or_else(|| "the Scratchpad window does not exist".to_string())?;
    if !w.is_visible().unwrap_or(false) {
        return Err("the Scratchpad was closed before the take finished".into());
    }
    w.emit(DICTATION_EVENT, DictationPayload { note_id, text })
        .map_err(|e| format!("emit {DICTATION_EVENT}: {e}"))
}

#[cfg(target_os = "macos")]
tauri_nspanel::tauri_panel! {
    panel!(ScratchPanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            becomes_key_only_if_needed: false,
            is_floating_panel: true,
            hides_on_deactivate: false
        }
    })
}

/// Open the Scratchpad without activating Yap. Safe from any thread: AppKit
/// panel calls are dispatched to the main thread.
pub fn open(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window(WINDOW_LABEL).is_none() {
        return Err("the Scratchpad window is missing from tauri.conf.json".into());
    }
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        if let Err(e) = open_on_main(&app2) {
            log::warn!("scratchpad open: {e}");
        }
    })
    .map_err(|e| format!("main-thread dispatch: {e}"))
}

#[cfg(target_os = "macos")]
fn open_on_main(app: &AppHandle) -> Result<(), String> {
    use tauri_nspanel::{CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt};
    let already_panel = app.get_webview_panel(WINDOW_LABEL).is_ok();
    for step in open_steps(already_panel) {
        match step {
            OpenStep::ConvertToNonactivatingPanel => {
                let window = app
                    .get_webview_window(WINDOW_LABEL)
                    .ok_or_else(|| "scratchpad webview missing".to_string())?;
                let panel = window
                    .to_panel::<ScratchPanel>()
                    .map_err(|e| format!("to_panel: {e}"))?;
                panel.set_style_mask(
                    StyleMask::empty()
                        .titled()
                        .closable()
                        .resizable()
                        .nonactivating_panel()
                        .into(),
                );
                panel.set_level(PanelLevel::Floating.value());
                panel.set_collection_behavior(
                    CollectionBehavior::new()
                        .move_to_active_space()
                        .full_screen_auxiliary()
                        .into(),
                );
                panel.set_hides_on_deactivate(false);
                panel.set_floating_panel(true);
            }
            OpenStep::OrderFrontRegardless => {
                let panel = app
                    .get_webview_panel(WINDOW_LABEL)
                    .map_err(|_| "scratchpad panel missing".to_string())?;
                panel.order_front_regardless();
            }
            OpenStep::MakeKey => {
                let panel = app
                    .get_webview_panel(WINDOW_LABEL)
                    .map_err(|_| "scratchpad panel missing".to_string())?;
                panel.make_key_window();
            }
            OpenStep::ActivateApp | OpenStep::PresentMainWindow => {
                unreachable!("open_steps never yields a focus-stealing step")
            }
        }
    }
    let _ = app.emit_to(WINDOW_LABEL, SHOWN_EVENT, ());
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn open_on_main(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or_else(|| "scratchpad webview missing".to_string())?;
    w.show().map_err(|e| e.to_string())?;
    let _ = app.emit_to(WINDOW_LABEL, SHOWN_EVENT, ());
    Ok(())
}

/// Hide the panel and drop the dictation target with it.
pub fn hide(app: &AppHandle) {
    set_focused_note(false, None);
    if let Some(w) = app.get_webview_window(WINDOW_LABEL) {
        let _ = w.hide();
    }
}
