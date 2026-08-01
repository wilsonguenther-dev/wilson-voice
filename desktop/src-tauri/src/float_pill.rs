//! Real macOS NSPanel Dictate island (via tauri-nspanel).
//!
//! This is NOT a normal NSWindow with rounded CSS.
//! Flow: WebviewWindow → to_panel::<DictatePill>() → NSPanel flags:
//!   nonactivatingPanel, floating/status level, canJoinAllSpaces,
//!   fullScreenAuxiliary, transparent host so only the glass pill is visible.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

#[cfg(target_os = "macos")]
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt,
};

// WINDOW size — deliberately LARGER than the pill so its ambient shadow renders
// inside the transparent window. A pill-sized window clips the shadow at its edge,
// which reads as an ugly hard rectangle. The pill is centered inside by CSS (.stage).
// Transparent window a bit larger than the pill: room for the expanded capsule
// and the speech bubble above it. The visible pill is small + centered by CSS/JS.
const PILL_W: f64 = 300.0;
const PILL_H: f64 = 140.0;

static KEEPER_ON: AtomicBool = AtomicBool::new(false);
static PANEL_READY: AtomicBool = AtomicBool::new(false);
/// Current dock edge as `PillPosition::as_u8` (YV53). Read by every park, so
/// the space-keeper and any display change pick the new edge up on their own.
static POSITION: AtomicU8 = AtomicU8::new(0);

/// Where the pill docks on screen (YV53). Wispr-style side docks in addition to
/// the historical bottom-centre island.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillPosition {
    /// Bottom-centre — the original Dictate island placement (default).
    Bottom,
    /// Flush to the LEFT screen edge, vertically centred.
    Left,
    /// Flush to the RIGHT screen edge, vertically centred.
    Right,
}

impl PillPosition {
    pub fn from_settings(s: &str) -> Self {
        match s {
            "left" => Self::Left,
            "right" => Self::Right,
            _ => Self::Bottom, // default: bottom-centre
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Self::Bottom => 0,
            Self::Left => 1,
            Self::Right => 2,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Left,
            2 => Self::Right,
            _ => Self::Bottom,
        }
    }
}

/// Set the dock edge. Does NOT move the window — call [`reposition`] (or let the
/// next show / space-keeper park) so the move happens on the main thread.
pub fn set_position(pos: PillPosition) {
    POSITION.store(pos.as_u8(), Ordering::SeqCst);
    log::info!("pill position → {pos:?}");
}

/// Pure position math (YV53): the float window's top-left corner, given the
/// monitor frame and the window size — ALL in physical px in Tauri's global
/// coordinate space (origin at the primary monitor's top-left).
///
/// `Bottom` is the historical placement: horizontally centred, `margin` off the
/// screen bottom. `Left`/`Right` dock the window FLUSH to that screen edge and
/// centre it vertically — the visual gap on a side dock comes from the pill's
/// CSS inset inside the (deliberately oversized, transparent) window, so the
/// capsule still sits a hair off the edge with its ambient shadow intact.
fn panel_origin(
    mon_pos: (i32, i32),
    mon_size: (i32, i32),
    win_size: (i32, i32),
    margin: i32,
    pos: PillPosition,
) -> (i32, i32) {
    let (mon_x, mon_y) = mon_pos;
    let (mon_w, mon_h) = mon_size;
    let (win_w, win_h) = win_size;
    match pos {
        PillPosition::Bottom => (mon_x + (mon_w - win_w) / 2, mon_y + mon_h - win_h - margin),
        PillPosition::Left => (mon_x, mon_y + (mon_h - win_h) / 2),
        PillPosition::Right => (mon_x + mon_w - win_w, mon_y + (mon_h - win_h) / 2),
    }
}

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

fn park_pill(app: &AppHandle) {
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
    // Window bottom sits ~14pt off the screen bottom; the pill (centered in the
    // taller window) then floats ~50pt up with shadow room below it.
    let margin = (14.0 * scale) as i32;
    let (x, y) = panel_origin(
        (pos.x, pos.y),
        (size.width as i32, size.height as i32),
        (pill_w, pill_h),
        margin,
        PillPosition::from_u8(POSITION.load(Ordering::SeqCst)),
    );
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
    // KILL THE GREY BOX: NSPanel defaults hasShadow=YES, and macOS draws that
    // native shadow tracing the ALPHA SILHOUETTE of the content — i.e. the outer
    // edge of the pill's big soft CSS box-shadow, a rounded rectangle much larger
    // than the pill. `.shadow(false)` on the builder isn't enough because to_panel
    // re-asserts the panel default; this panel-level call is authoritative.
    panel.set_has_shadow(false);
    panel.set_opaque(false);
    panel.set_style_mask(
        // Only OR the nonactivating bit — do NOT chain .borderless() (tauri-nspanel
        // v2.1 REPLACES the mask, which can panic; window is already decorations(false)).
        StyleMask::empty().nonactivating_panel().into(),
    );
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .stationary() // pin across Space swipes
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
            .shadow(false) // no NSWindow shadow → only the pill's CSS shadow shows (kills the grey box)
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

    park_pill(app);

    if let Some(w) = app.get_webview_window("float") {
        // Click-THROUGH: the pill is a HUD indicator over other apps and its window
        // is much larger than the visible pill (shadow room), so it must never
        // intercept clicks in the transparent margin. Control is via fn / tray.
        let _ = w.set_ignore_cursor_events(true);
        let _ = w.set_always_on_top(true);
        // Transparent content so only CSS pill paints
        let _ = w.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
    }

    Ok(())
}

pub fn show_float(app: &AppHandle) -> Result<(), String> {
    ensure_float(app)?;
    park_pill(app);

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

/// Re-park the pill on the configured edge (YV53) — call after
/// [`set_position`] so the dock change lands immediately instead of on the next
/// show or space-keeper tick. Safe from any thread.
pub fn reposition(app: &AppHandle) {
    dispatch_main(app, park_pill);
}

/// Keep the NSPanel on top across Space swipes. Now benign: it re-parks to a
/// FIXED position on the configured edge (no cursor read → no teleport) and only
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
                park_pill(&app_inner);
                // apply_panel_hud already re-asserts front on the converted panel.
                let _ = apply_panel_hud(&app_inner);
            });
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::{panel_origin, PillPosition};

    // A 2x Retina 1440x900-point display at the global origin, and the float
    // window at its physical size (PILL_W/H x 2).
    const MON_POS: (i32, i32) = (0, 0);
    const MON_SIZE: (i32, i32) = (2880, 1800);
    const WIN_SIZE: (i32, i32) = (600, 280);
    const MARGIN: i32 = 28;

    #[test]
    fn bottom_keeps_the_centred_island_placement() {
        let (x, y) = panel_origin(MON_POS, MON_SIZE, WIN_SIZE, MARGIN, PillPosition::Bottom);
        assert_eq!(x, (2880 - 600) / 2); // horizontally centred
        assert_eq!(y, 1800 - 280 - MARGIN); // margin off the screen bottom
    }

    #[test]
    fn left_docks_flush_to_the_left_edge_vertically_centred() {
        let (x, y) = panel_origin(MON_POS, MON_SIZE, WIN_SIZE, MARGIN, PillPosition::Left);
        assert_eq!(x, 0);
        assert_eq!(y, (1800 - 280) / 2);
    }

    #[test]
    fn right_docks_flush_to_the_right_edge_vertically_centred() {
        let (x, y) = panel_origin(MON_POS, MON_SIZE, WIN_SIZE, MARGIN, PillPosition::Right);
        assert_eq!(x, 2880 - 600);
        assert_eq!(y, (1800 - 280) / 2);
    }

    /// A secondary display sits at a non-zero (and possibly negative) global
    /// origin — every edge must be relative to THAT monitor's frame, never 0.
    #[test]
    fn origins_follow_a_monitor_that_is_not_at_the_global_origin() {
        let mon_pos = (-1920, -300);
        let mon_size = (1920, 1080);
        let win = (300, 140);
        assert_eq!(
            panel_origin(mon_pos, mon_size, win, 14, PillPosition::Left),
            (-1920, -300 + (1080 - 140) / 2)
        );
        assert_eq!(
            panel_origin(mon_pos, mon_size, win, 14, PillPosition::Right),
            (-1920 + 1920 - 300, -300 + (1080 - 140) / 2)
        );
        assert_eq!(
            panel_origin(mon_pos, mon_size, win, 14, PillPosition::Bottom),
            (-1920 + (1920 - 300) / 2, -300 + 1080 - 140 - 14)
        );
    }

    /// The three docks are genuinely distinct placements — left/right differ on
    /// x, and neither is the bottom-centre island.
    #[test]
    fn the_three_docks_are_distinct() {
        let bottom = panel_origin(MON_POS, MON_SIZE, WIN_SIZE, MARGIN, PillPosition::Bottom);
        let left = panel_origin(MON_POS, MON_SIZE, WIN_SIZE, MARGIN, PillPosition::Left);
        let right = panel_origin(MON_POS, MON_SIZE, WIN_SIZE, MARGIN, PillPosition::Right);
        assert!(left.0 < right.0);
        assert_ne!(bottom, left);
        assert_ne!(bottom, right);
    }

    #[test]
    fn position_from_settings() {
        assert_eq!(PillPosition::from_settings("left"), PillPosition::Left);
        assert_eq!(PillPosition::from_settings("right"), PillPosition::Right);
        assert_eq!(PillPosition::from_settings("bottom"), PillPosition::Bottom);
        // Unknown / missing values fall back to the default bottom-centre dock.
        assert_eq!(
            PillPosition::from_settings("nonsense"),
            PillPosition::Bottom
        );
    }

    /// The atomic round-trip that carries the dock edge into `park_pill`.
    #[test]
    fn position_round_trips_through_the_atomic() {
        for pos in [
            PillPosition::Bottom,
            PillPosition::Left,
            PillPosition::Right,
        ] {
            assert_eq!(PillPosition::from_u8(pos.as_u8()), pos);
        }
        // Any unexpected byte reads as the default.
        assert_eq!(PillPosition::from_u8(9), PillPosition::Bottom);
    }
}
