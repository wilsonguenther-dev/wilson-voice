//! Floating Dictate pill — Wispr-class HUD.
//!
//! Research (VoiceInk MiniRecorderPanel / tauri-nspanel patterns, clean-room):
//! - Prefer true NSPanel with nonactivatingPanel + fullScreenAuxiliary
//! - Park bottom-center of the screen under the cursor (not continuous chase)
//! - Separate float.html MPA — never load full App into the pill
//!
//! Continuous cursor follow is intentionally OFF (glitchy mini-app feel).

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const PILL_W: f64 = 220.0;
const PILL_H: f64 = 48.0;

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }
    #[repr(C)]
    struct CGSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *mut c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
        fn CGMainDisplayID() -> u32;
        fn CGDisplayBounds(display: u32) -> CGRect;
        fn CGGetDisplaysWithPoint(
            point: CGPoint,
            max: u32,
            displays: *mut u32,
            count: *mut u32,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *mut c_void);
    }

    // objc runtime for NSPanel-ish flags on the Tauri NSWindow
    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn objc_getClass(name: *const i8) -> *mut c_void;
        fn sel_registerName(name: *const i8) -> *mut c_void;
        fn objc_msgSend();
    }

    /// Cursor position in Quartz coords (origin top-left of primary, Y down).
    pub fn cursor_point() -> Option<CGPoint> {
        unsafe {
            let ev = CGEventCreate(std::ptr::null_mut());
            if ev.is_null() {
                return None;
            }
            let p = CGEventGetLocation(ev);
            CFRelease(ev);
            Some(p)
        }
    }

    /// Display under cursor; falls back to main. Returns (x, y, w, h) Quartz bounds.
    pub fn screen_under_cursor() -> (f64, f64, f64, f64) {
        unsafe {
            let p = cursor_point().unwrap_or(CGPoint { x: 0.0, y: 0.0 });
            let mut id: u32 = 0;
            let mut count: u32 = 0;
            let err = CGGetDisplaysWithPoint(p, 1, &mut id, &mut count);
            if err != 0 || count == 0 {
                id = CGMainDisplayID();
            }
            let r = CGDisplayBounds(id);
            (r.origin.x, r.origin.y, r.size.width, r.size.height)
        }
    }

    /// Apply nonactivating + floating + join-all-spaces + fullscreen-auxiliary via objc.
    /// Best-effort on the underlying NSWindow (Tauri webview). True NSPanel is better
    /// (tauri-nspanel) but this gets us most of the way without an extra crate risk.
    pub fn harden_as_hud(ns_window: *mut c_void) {
        if ns_window.is_null() {
            return;
        }
        unsafe {
            // selectors
            let sel = |name: &str| {
                sel_registerName(std::ffi::CString::new(name).unwrap().as_ptr())
            };
            // Use msg_send via transmute for common setters
            type MsgSetBool = unsafe extern "C" fn(*mut c_void, *mut c_void, bool);
            type MsgSetI64 = unsafe extern "C" fn(*mut c_void, *mut c_void, i64);
            type MsgGet = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64;
            type MsgSetU64 = unsafe extern "C" fn(*mut c_void, *mut c_void, u64);
            type MsgVoid = unsafe extern "C" fn(*mut c_void, *mut c_void);

            let msg_bool: MsgSetBool = std::mem::transmute(objc_msgSend as *const ());
            let msg_i64: MsgSetI64 = std::mem::transmute(objc_msgSend as *const ());
            let msg_get: MsgGet = std::mem::transmute(objc_msgSend as *const ());
            let msg_u64: MsgSetU64 = std::mem::transmute(objc_msgSend as *const ());
            let msg_void: MsgVoid = std::mem::transmute(objc_msgSend as *const ());

            // setHidesOnDeactivate: NO
            msg_bool(ns_window, sel("setHidesOnDeactivate:"), false);
            // setCanHide: NO
            msg_bool(ns_window, sel("setCanHide:"), false);
            // setLevel: NSFloatingWindowLevel = 3
            msg_i64(ns_window, sel("setLevel:"), 3);
            // collectionBehavior |= canJoinAllSpaces(1<<0) | fullScreenAuxiliary(1<<8) | ignoresCycle(1<<6) | stationary(1<<4)
            let behavior: u64 = msg_get(ns_window, sel("collectionBehavior"));
            let want: u64 = (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8);
            msg_u64(ns_window, sel("setCollectionBehavior:"), behavior | want);
            // styleMask |= nonactivatingPanel (1 << 7) — only meaningful on NSPanel, harmless attempt
            let mask: u64 = msg_get(ns_window, sel("styleMask"));
            msg_u64(ns_window, sel("setStyleMask:"), mask | (1 << 7));
            // orderFrontRegardless so it appears without activating us
            msg_void(ns_window, sel("orderFrontRegardless"));
            let _ = objc_getClass; // silence
        }
    }
}

pub fn ensure_float(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window("float").is_some() {
        return Ok(());
    }

    let url = WebviewUrl::App("float.html".into());
    let w = WebviewWindowBuilder::new(app, "float", url)
        .title("Dictate")
        .inner_size(PILL_W, PILL_H)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .visible_on_all_workspaces(true)
        .build()
        .map_err(|e| format!("float window: {e}"))?;

    harden_window(&w);
    park_bottom_center(&w);
    Ok(())
}

fn harden_window(w: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        // ns_window() returns Result in Tauri 2
        match w.ns_window() {
            Ok(ns) => mac::harden_as_hud(ns as *mut std::ffi::c_void),
            Err(e) => log::warn!("ns_window for harden: {e}"),
        }
    }
    let _ = w.set_ignore_cursor_events(false);
}

/// Park bottom-center of the display under the cursor (multi-monitor).
fn park_bottom_center(w: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        let (ox, oy, sw, sh) = mac::screen_under_cursor();
        let x = (ox + (sw - PILL_W) / 2.0).max(ox) as i32;
        // Quartz Y grows downward; bottom of this display:
        let y = (oy + sh - PILL_H - 36.0).max(oy) as i32;
        let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
        return;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
            x: 100,
            y: 100,
        }));
    }
}

pub fn show_float(app: &AppHandle) -> Result<(), String> {
    ensure_float(app)?;
    if let Some(w) = app.get_webview_window("float") {
        harden_window(&w);
        park_bottom_center(&w);
        let _ = w.show();
        #[cfg(target_os = "macos")]
        {
            if let Ok(ns) = w.ns_window() {
                mac::harden_as_hud(ns as *mut std::ffi::c_void);
            }
        }
    }
    Ok(())
}

pub fn hide_float(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("float") {
        let _ = w.hide();
    }
}

/// Show + re-park when recording starts (Wispr-style: appear for the hold).
pub fn show_for_recording(app: &AppHandle) {
    if let Err(e) = show_float(app) {
        log::warn!("float show: {e}");
    }
}

/// One-shot snap near cursor (optional; not continuous follow).
#[allow(dead_code)]
pub fn snap_near_cursor(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        if let Some(w) = app.get_webview_window("float") {
            if let Some(p) = mac::cursor_point() {
                let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                    x: (p.x as i32).saturating_sub(40),
                    y: (p.y as i32).saturating_sub(56),
                }));
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}
