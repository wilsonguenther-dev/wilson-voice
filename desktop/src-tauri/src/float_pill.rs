//! Floating Dictate island — rides all Spaces (Mission Control swipe).
//!
//! Shell: transparent always-on-top + canJoinAllSpaces + fullScreenAuxiliary.
//! Parked bottom-center of the active display; re-asserted on a light interval
//! so Space switches don't bury the HUD in the main app.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const PILL_W: f64 = 216.0;
const PILL_H: f64 = 56.0;

static KEEPER_ON: AtomicBool = AtomicBool::new(false);

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
        fn CGGetActiveDisplayList(max: u32, list: *mut u32, count: *mut u32) -> i32;
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

    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn sel_registerName(name: *const i8) -> *mut c_void;
        fn objc_msgSend();
        fn objc_getClass(name: *const i8) -> *mut c_void;
    }

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

    /// Prefer display under cursor; else first active display.
    pub fn active_screen_bounds() -> (f64, f64, f64, f64) {
        unsafe {
            let p = cursor_point().unwrap_or(CGPoint { x: 0.0, y: 0.0 });
            let mut id: u32 = 0;
            let mut count: u32 = 0;
            let err = CGGetDisplaysWithPoint(p, 1, &mut id, &mut count);
            if err != 0 || count == 0 {
                let mut list = [0u32; 16];
                let mut n = 0u32;
                if CGGetActiveDisplayList(16, list.as_mut_ptr(), &mut n) == 0 && n > 0 {
                    id = list[0];
                } else {
                    id = CGMainDisplayID();
                }
            }
            let r = CGDisplayBounds(id);
            (r.origin.x, r.origin.y, r.size.width, r.size.height)
        }
    }

    pub fn harden_as_hud(ns_window: *mut c_void) {
        if ns_window.is_null() {
            return;
        }
        unsafe {
            let sel = |name: &str| {
                sel_registerName(std::ffi::CString::new(name).unwrap().as_ptr())
            };
            type MsgSetBool = unsafe extern "C" fn(*mut c_void, *mut c_void, bool);
            type MsgSetI64 = unsafe extern "C" fn(*mut c_void, *mut c_void, i64);
            type MsgGet = unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64;
            type MsgSetU64 = unsafe extern "C" fn(*mut c_void, *mut c_void, u64);
            type MsgVoid = unsafe extern "C" fn(*mut c_void, *mut c_void);
            type MsgObj = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
            type MsgSetObj = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void);

            let msg_bool: MsgSetBool = std::mem::transmute(objc_msgSend as *const ());
            let msg_i64: MsgSetI64 = std::mem::transmute(objc_msgSend as *const ());
            let msg_get: MsgGet = std::mem::transmute(objc_msgSend as *const ());
            let msg_u64: MsgSetU64 = std::mem::transmute(objc_msgSend as *const ());
            let msg_void: MsgVoid = std::mem::transmute(objc_msgSend as *const ());
            let msg_obj: MsgObj = std::mem::transmute(objc_msgSend as *const ());
            let msg_set_obj: MsgSetObj = std::mem::transmute(objc_msgSend as *const ());

            msg_bool(ns_window, sel("setHidesOnDeactivate:"), false);
            msg_bool(ns_window, sel("setCanHide:"), false);
            msg_bool(ns_window, sel("setOpaque:"), false);
            msg_bool(ns_window, sel("setHasShadow:"), false);

            let class_name = std::ffi::CString::new("NSColor").unwrap();
            let ns_color = objc_getClass(class_name.as_ptr());
            if !ns_color.is_null() {
                let clear = msg_obj(ns_color, sel("clearColor"));
                if !clear.is_null() {
                    msg_set_obj(ns_window, sel("setBackgroundColor:"), clear);
                }
            }

            // Status-level float so it rides above normal app windows when swiping Spaces
            // NSStatusWindowLevel = 25
            msg_i64(ns_window, sel("setLevel:"), 25);

            // canJoinAllSpaces (1) | fullScreenAuxiliary (256) | ignoresCycle (64)
            // Do NOT set stationary — that can pin oddly vs “follow me across Spaces”
            let want: u64 = (1 << 0) | (1 << 6) | (1 << 8);
            msg_u64(ns_window, sel("setCollectionBehavior:"), want);

            // borderless | nonactivatingPanel
            let mask: u64 = msg_get(ns_window, sel("styleMask"));
            msg_u64(ns_window, sel("setStyleMask:"), mask | (1 << 7));

            msg_void(ns_window, sel("orderFrontRegardless"));
        }
    }
}

pub fn ensure_float(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window("float").is_some() {
        return Ok(());
    }

    let url = WebviewUrl::App("float.html".into());
    let mut builder = WebviewWindowBuilder::new(app, "float", url)
        .title("Dictate")
        .inner_size(PILL_W, PILL_H)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .visible_on_all_workspaces(true);

    #[cfg(any(not(target_os = "macos"), feature = "macos-private-api"))]
    {
        builder = builder.transparent(true);
    }

    let w = builder
        .build()
        .map_err(|e| format!("float window: {e}"))?;

    harden_window(&w);
    park_bottom_center(&w);
    Ok(())
}

fn harden_window(w: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        match w.ns_window() {
            Ok(ns) => mac::harden_as_hud(ns as *mut std::ffi::c_void),
            Err(e) => log::warn!("ns_window for harden: {e}"),
        }
    }
    let _ = w.set_ignore_cursor_events(false);
    let _ = w.set_always_on_top(true);
}

fn park_bottom_center(w: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        let (ox, oy, sw, sh) = mac::active_screen_bounds();
        let x = (ox + (sw - PILL_W) / 2.0).max(ox) as i32;
        // Bottom-center of active display (middle horizontally, near dock)
        let y = (oy + sh - PILL_H - 48.0).max(oy) as i32;
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

pub fn show_for_recording(app: &AppHandle) {
    if let Err(e) = show_float(app) {
        log::warn!("float show: {e}");
    }
}

pub fn after_recording(app: &AppHandle, keep_visible: bool) {
    if keep_visible {
        let _ = show_float(app);
    } else {
        hide_float(app);
    }
}

/// Periodically re-assert float on top + re-park so Space swipes keep the island
/// centered on the active display instead of vanishing into the main window.
pub fn start_space_keeper(app: AppHandle) {
    if KEEPER_ON.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::Builder::new()
        .name("wv-float-keeper".into())
        .spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(800));
                let app_main = app.clone();
                let app_inner = app.clone();
                let _ = app_main.run_on_main_thread(move || {
                    if let Some(w) = app_inner.get_webview_window("float") {
                        if w.is_visible().unwrap_or(false) {
                            harden_window(&w);
                            park_bottom_center(&w);
                            let _ = w.set_always_on_top(true);
                            #[cfg(target_os = "macos")]
                            {
                                if let Ok(ns) = w.ns_window() {
                                    mac::harden_as_hud(ns as *mut std::ffi::c_void);
                                }
                            }
                        }
                    }
                });
            }
        })
        .ok();
}
