use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

pub fn copy_text(app: &AppHandle, text: &str) -> Result<(), String> {
    app.clipboard()
        .write_text(text.to_string())
        .map_err(|e| format!("clipboard write: {e}"))
}

/// Simulate Cmd+V into the frontmost application (macOS).
pub fn paste_frontmost() -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| format!("enigo: {e}"))?;
    // Cmd down, v, Cmd up
    enigo
        .key(Key::Meta, Direction::Press)
        .map_err(|e| format!("meta press: {e}"))?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| format!("v click: {e}"))?;
    enigo
        .key(Key::Meta, Direction::Release)
        .map_err(|e| format!("meta release: {e}"))?;
    Ok(())
}
