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

fn park_bottom_center(app: &AppHandle) {
    let Some(w) = app.get_webview_window("float") else {
        return;
    };
    // Pin to the PRIMARY monitor (never the cursor's display) in Tauri's own
    // physical-pixel space. Fixes both the Retina points-vs-pixels mis-placement
    // and the cursor-driven cross-monitor teleport of the old CGDisplayBounds path.
    let Some(mon) = w
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| w.current_monitor().ok().flatten())
    else {
        return;
    };
    let pos = mon.position(); // physical px
    let size = mon.size(); // physical px
    let scale = mon.scale_factor();
    // PILL_W/H and the margin are logical points → scale to physical for centering.
    let pill_w = (PILL_W * scale) as i32;
    let pill_h = (PILL_H * scale) as i32;
    let margin = (52.0 * scale) as i32;
    let x = pos.x + (size.width as i32 - pill_w) / 2;
    let y = pos.y + size.height as i32 - pill_h - margin;
    let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
}

#[cfg(target_os = "macos")]
fn apply_panel_hud(app: &AppHandle) -> Result<(), String> {
    // Already converted: flags were set ONCE at conversion (below). Re-applying
    // set_style_mask at runtime does NOT re-run NSPanel's _setPreventsActivation,
    // so the WindowServer nonactivating tag desyncs → flicker + focus theft. Just
    // re-assert front (without activating the app).
    if let Ok(panel) = app.get_webview_panel("float") {
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

/// Run a pill/NSPanel op on the MAIN thread from any caller thread. AppKit panel
/// methods (set_level / order_front / show / hide) are main-thread-only; calling
/// them off-main is an instant SIGTRAP ("Must only be used from the main thread").
/// The dictation pipeline calls `after_recording` from a worker thread, which
/// crashed here (float_pill::apply_panel_hud off-main). Dispatching makes these
/// entry points thread-placement-independent so no caller can reintroduce it.
fn dispatch_main<F: FnOnce(&AppHandle) + Send + 'static>(app: &AppHandle, f: F) {
    let app2 = app.clone();
    if let Err(e) = app.clone().run_on_main_thread(move || f(&app2)) {
        log::warn!("pill main-thread dispatch failed: {e}");
    }
}

pub fn show_for_recording(app: &AppHandle) {
    dispatch_main(app, |a| {
        if let Err(e) = show_float(a) {
            log::warn!("float show: {e}");
        }
    });
}

pub fn after_recording(app: &AppHandle, keep_visible: bool) {
    dispatch_main(app, move |a| {
        if keep_visible {
            let _ = show_float(a);
        } else {
            hide_float(a);
        }
    });
}

/// Keep the NSPanel on top across Space swipes. Now benign: it re-parks to a
/// FIXED bottom-center position (no cursor read → no teleport) and only
/// re-asserts front (no style-mask reset → no flicker/focus theft). A future
/// improvement is to drive this off `activeSpaceDidChange`/`didChangeScreenParams`
/// notifications instead of a timer.
pub fn start_space_keeper(app: AppHandle) {
    if KEEPER_ON.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::Builder::new()
        .name("wv-float-keeper".into())
        .spawn(move || loop {
            thread::sleep(Duration::from_millis(1500));
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
                // apply_panel_hud already re-asserts front on the converted panel.
                let _ = apply_panel_hud(&app_inner);
            });
        })
        .ok();
}
