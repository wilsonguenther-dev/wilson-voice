//! Clipboard + optional frontmost-app paste.
//!
//! Paste uses CGEvent synthesis via enigo → requires Accessibility trust for
//! **Wilson Voice** (com.wilsonguenther.wilson-voice), not Python.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::permissions;

pub fn copy_text(app: &AppHandle, text: &str) -> Result<(), String> {
    app.clipboard()
        .write_text(text.to_string())
        .map_err(|e| format!("clipboard write: {e}"))
}

/// Result of paste attempt for UI messaging.
#[derive(Debug)]
pub struct PasteOutcome {
    pub copied: bool,
    pub pasted: bool,
    pub message: String,
}

/// Always copy; paste only when Accessibility is granted.
pub fn copy_and_maybe_paste(app: &AppHandle, text: &str, want_paste: bool) -> PasteOutcome {
    if let Err(e) = copy_text(app, text) {
        return PasteOutcome {
            copied: false,
            pasted: false,
            message: format!("Clipboard failed: {e}"),
        };
    }
    if !want_paste {
        return PasteOutcome {
            copied: true,
            pasted: false,
            message: "Copied to clipboard".into(),
        };
    }
    if !permissions::is_accessibility_trusted() {
        return PasteOutcome {
            copied: true,
            pasted: false,
            message: "Copied. Enable Accessibility for Wilson Voice to auto-paste (Settings → Permissions).".into(),
        };
    }
    match paste_frontmost() {
        Ok(()) => PasteOutcome {
            copied: true,
            pasted: true,
            message: "Copied and pasted into frontmost app".into(),
        },
        Err(e) => PasteOutcome {
            copied: true,
            pasted: false,
            message: format!(
                "Copied, but paste failed ({e}). Grant Accessibility to Wilson Voice."
            ),
        },
    }
}

/// Simulate Cmd+V into the frontmost application (macOS).
/// Caller must check Accessibility first for good UX.
pub fn paste_frontmost() -> Result<(), String> {
    if !permissions::is_accessibility_trusted() {
        return Err(
            "Accessibility not granted to Wilson Voice — open Settings → Privacy → Accessibility"
                .into(),
        );
    }
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| format!("enigo: {e}"))?;
    enigo
        .key(Key::Meta, Direction::Press)
        .map_err(|e| format!("meta press: {e}"))?;
    // Small delay improves reliability in some Electron apps
    std::thread::sleep(std::time::Duration::from_millis(20));
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| format!("v click: {e}"))?;
    std::thread::sleep(std::time::Duration::from_millis(20));
    enigo
        .key(Key::Meta, Direction::Release)
        .map_err(|e| format!("meta release: {e}"))?;
    Ok(())
}
