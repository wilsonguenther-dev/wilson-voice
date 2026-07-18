//! Real macOS NSPanel Dictate island (via tauri-nspanel).
//!
//! This is NOT a normal NSWindow with rounded CSS.
//! Flow: WebviewWindow → to_panel::<DictatePill>() → NSPanel flags:
//!   nonactivatingPanel, floating/status level, canJoinAllSpaces,
//!   fullScreenAuxiliary, transparent host so only the glass pill is visible.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

#[cfg(target_os = "macos")]
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt,
};

const PILL_W: f64 = 200.0;
const PILL_H: f64 = 52.0;

static KEEPER_ON: AtomicBool = AtomicBool::new(false);
static PANEL_READY: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(DictatePill {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            becomes_key_only_if_needed: true,
            is_floating_panel: true,
            hides_on_deactivate: false,
            works_when_modal: true
        }
    })
}

#[cfg(target_os = "macos")]
mod screen {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
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

    pub fn active_screen_bounds() -> (f64, f64, f64, f64) {
        unsafe {
            let ev = CGEventCreate(std::ptr::null_mut());
            let p = if ev.is_null() {
                CGPoint { x: 0.0, y: 0.0 }
            } else {
                let pt = CGEventGetLocation(ev);
                CFRelease(ev);
                pt
            };
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
}

#[cfg(not(target_os = "macos"))]
mod screen {
    pub fn active_screen_bounds() -> (f64, f64, f64, f64) {
        (0.0, 0.0, 1440.0, 900.0)
    }
}

fn park_bottom_center(app: &AppHandle) {
    let Some(w) = app.get_webview_window("float") else {
        return;
    };
    let (ox, oy, sw, sh) = screen::active_screen_bounds();
    let x = (ox + (sw - PILL_W) / 2.0).max(ox) as i32;
    let y = (oy + sh - PILL_H - 52.0).max(oy) as i32;
    let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
}

#[cfg(target_os = "macos")]
fn apply_panel_hud(app: &AppHandle) -> Result<(), String> {
    // Prefer panel API if already converted
    if let Ok(panel) = app.get_webview_panel("float") {
        panel.set_level(PanelLevel::Status.value());
        panel.set_style_mask(
            StyleMask::empty()
                .nonactivating_panel()
                .borderless()
                .into(),
        );
        panel.set_collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary()
                .ignores_cycle()
                .into(),
        );
        panel.set_hides_on_deactivate(false);
        panel.set_floating_panel(true);
        // order front without activating app
        panel.order_front_regardless();
        PANEL_READY.store(true, Ordering::SeqCst);
        return Ok(());
    }

    let window = app
        .get_webview_window("float")
        .ok_or_else(|| "float webview missing".to_string())?;

    let panel = window
        .to_panel::<DictatePill>()
        .map_err(|e| format!("to_panel: {e}"))?;

    panel.set_level(PanelLevel::Status.value());
    panel.set_style_mask(
        StyleMask::empty()
            .nonactivating_panel()
            .borderless()
            .into(),
    );
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .ignores_cycle()
            .into(),
    );
    panel.set_hides_on_deactivate(false);
    panel.set_floating_panel(true);
    panel.order_front_regardless();
    PANEL_READY.store(true, Ordering::SeqCst);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn apply_panel_hud(_app: &AppHandle) -> Result<(), String> {
    Ok(())
}

/// Create the float webview once, convert to NSPanel, park bottom-center.
pub fn ensure_float(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window("float").is_none() {
        let url = WebviewUrl::App("float.html".into());
        let mut builder = WebviewWindowBuilder::new(app, "float", url)
            .title("")
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

        builder
            .build()
            .map_err(|e| format!("float window: {e}"))?;
    }

    // Convert / re-apply NSPanel HUD flags
    if let Err(e) = apply_panel_hud(app) {
        log::warn!("NSPanel convert: {e} — falling back to webview flags");
    }

    park_bottom_center(app);

    if let Some(w) = app.get_webview_window("float") {
        let _ = w.set_ignore_cursor_events(false);
        let _ = w.set_always_on_top(true);
        // Transparent content so only CSS pill paints
        let _ = w.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
    }

    Ok(())
}

pub fn show_float(app: &AppHandle) -> Result<(), String> {
    ensure_float(app)?;
    park_bottom_center(app);

    #[cfg(target_os = "macos")]
    {
        if let Ok(panel) = app.get_webview_panel("float") {
            let _ = apply_panel_hud(app);
            panel.show();
            panel.order_front_regardless();
            return Ok(());
        }
    }

    if let Some(w) = app.get_webview_window("float") {
        let _ = w.show();
    }
    Ok(())
}

pub fn hide_float(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        if let Ok(panel) = app.get_webview_panel("float") {
            panel.hide();
            return;
        }
    }
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

/// Keep NSPanel on top + re-park after Space swipes (does not recreate window).
pub fn start_space_keeper(app: AppHandle) {
    if KEEPER_ON.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::Builder::new()
        .name("wv-float-keeper".into())
        .spawn(move || loop {
            thread::sleep(Duration::from_millis(900));
            let app_main = app.clone();
            let app_inner = app.clone();
            let _ = app_main.run_on_main_thread(move || {
                // Only re-assert if visible
                let visible = app_inner
                    .get_webview_window("float")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                if !visible {
                    return;
                }
                park_bottom_center(&app_inner);
                let _ = apply_panel_hud(&app_inner);
                #[cfg(target_os = "macos")]
                {
                    if let Ok(panel) = app_inner.get_webview_panel("float") {
                        panel.order_front_regardless();
                    }
                }
            });
        })
        .ok();
}
