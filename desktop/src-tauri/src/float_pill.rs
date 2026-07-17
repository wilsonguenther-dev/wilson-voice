//! Floating Dictate pill — Wispr-style control that follows the pointer
//! across spaces/screens.
//!
//! Research (Tauri #11488, Apple NSPanel patterns, VoiceInk/Handy):
//! - Standard NSWindow cannot float above fullscreen apps.
//! - Real apps use NSPanel + .nonactivatingPanel + canJoinAllSpaces.
//! - Tauri webview windows with always_on_top work for normal desktops;
//!   full-screen-over needs tauri-nspanel later.
//!
//! This module: non-activating always-on-top float window, repositions near
//! the mouse so it "follows" across multi-monitor layouts.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

static TRACKING: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
mod mouse {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *mut c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
        fn CFRelease(cf: *mut c_void);
    }

    /// Global mouse location in Cocoa screen coords (origin bottom-left).
    pub fn cursor_position() -> Option<(f64, f64)> {
        unsafe {
            let ev = CGEventCreate(std::ptr::null_mut());
            if ev.is_null() {
                return None;
            }
            let p = CGEventGetLocation(ev);
            CFRelease(ev);
            Some((p.x, p.y))
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod mouse {
    pub fn cursor_position() -> Option<(f64, f64)> {
        None
    }
}

pub fn ensure_float(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window("float").is_some() {
        return Ok(());
    }
    let w = WebviewWindowBuilder::new(app, "float", WebviewUrl::App("index.html".into()))
        .title("Dictate")
        .inner_size(220.0, 52.0)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(true)
        .visible_on_all_workspaces(true)
        .build()
        .map_err(|e| format!("float window: {e}"))?;

    let _ = w.set_ignore_cursor_events(false);

    // Seed near cursor
    if let Some((x, y)) = mouse::cursor_position() {
        // Cocoa y is from bottom; Tauri LogicalPosition is typically top-left on macOS in recent versions
        // Use PhysicalPosition for reliability
        let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
            x: (x as i32).saturating_sub(40),
            y: (y as i32).saturating_sub(70),
        }));
    }
    Ok(())
}

pub fn show_float(app: &AppHandle) -> Result<(), String> {
    ensure_float(app)?;
    if let Some(w) = app.get_webview_window("float") {
        let _ = w.show();
    }
    start_cursor_tracking(app.clone());
    Ok(())
}

pub fn hide_float(app: &AppHandle) {
    TRACKING.store(false, Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("float") {
        let _ = w.hide();
    }
}

fn start_cursor_tracking(app: AppHandle) {
    if TRACKING.swap(true, Ordering::SeqCst) {
        return; // already running
    }
    let running = Arc::new(AtomicBool::new(true));
    // Mirror global TRACKING
    thread::spawn(move || {
        while TRACKING.load(Ordering::SeqCst) {
            if let Some(w) = app.get_webview_window("float") {
                if let Some((x, y)) = mouse::cursor_position() {
                    // Offset so pill sits just above the cursor
                    let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                        x: (x as i32).saturating_sub(30),
                        y: (y as i32).saturating_sub(64),
                    }));
                }
            } else {
                break;
            }
            thread::sleep(Duration::from_millis(80));
        }
        TRACKING.store(false, Ordering::SeqCst);
        let _ = running;
    });
}
