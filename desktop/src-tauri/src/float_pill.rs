//! Floating Dictate pill — compact HUD only (not a second full app).
//!
//! Fixed near bottom-center of the primary display. Does **not** chase the
//! cursor (that glitch made a mini Wilson Voice UI follow the user).
//! Cursor-relative positioning only when recording starts (optional snap).

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

#[cfg(target_os = "macos")]
mod screen {
    use std::ffi::c_void;

    #[repr(C)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }
    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGDisplayBounds(display: u32) -> CGRect;
        fn CGMainDisplayID() -> u32;
    }

    /// Primary display size (points/pixels depending on backing).
    pub fn main_display_size() -> (f64, f64) {
        unsafe {
            let id = CGMainDisplayID();
            let r = CGDisplayBounds(id);
            (r.size.width, r.size.height)
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod screen {
    pub fn main_display_size() -> (f64, f64) {
        (1440.0, 900.0)
    }
}

const PILL_W: f64 = 200.0;
const PILL_H: f64 = 48.0;

pub fn ensure_float(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window("float").is_some() {
        return Ok(());
    }

    // Hash route so React only mounts the compact pill
    let url = WebviewUrl::App("index.html#float".into());

    let w = WebviewWindowBuilder::new(app, "float", url)
        .title("Dictate")
        .inner_size(PILL_W, PILL_H)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(true)
        .visible_on_all_workspaces(true)
        .build()
        .map_err(|e| format!("float window: {e}"))?;

    park_bottom_center(&w);
    Ok(())
}

fn park_bottom_center(w: &tauri::WebviewWindow) {
    let (sw, sh) = screen::main_display_size();
    // Physical coords: origin top-left on Tauri for many setups; bottom-center of main display
    let x = ((sw - PILL_W) / 2.0).max(0.0) as i32;
    let y = (sh - PILL_H - 28.0).max(0.0) as i32;
    let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
}

pub fn show_float(app: &AppHandle) -> Result<(), String> {
    ensure_float(app)?;
    if let Some(w) = app.get_webview_window("float") {
        park_bottom_center(&w);
        let _ = w.show();
    }
    Ok(())
}

pub fn hide_float(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("float") {
        let _ = w.hide();
    }
}

/// Snap pill near cursor once (e.g. when recording starts) — not continuous follow.
#[allow(dead_code)]
pub fn snap_near_cursor(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::c_void;
        #[repr(C)]
        struct CGPoint {
            x: f64,
            y: f64,
        }
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGEventCreate(source: *mut c_void) -> *mut c_void;
            fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
            fn CFRelease(cf: *mut c_void);
        }
        if let Some(w) = app.get_webview_window("float") {
            unsafe {
                let ev = CGEventCreate(std::ptr::null_mut());
                if !ev.is_null() {
                    let p = CGEventGetLocation(ev);
                    CFRelease(ev);
                    let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                        x: (p.x as i32).saturating_sub(40),
                        y: (p.y as i32).saturating_sub(56),
                    }));
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}
