//! Real macOS NSPanel Dictate island (via tauri-nspanel).
//!
//! This is NOT a normal NSWindow with rounded CSS.
//! Flow: WebviewWindow → to_panel::<DictatePill>() → NSPanel flags:
//!   nonactivatingPanel, floating/status level, canJoinAllSpaces,
//!   fullScreenAuxiliary, transparent host so only the glass pill is visible.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

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

/// YV65 — a drag that releases within this many POINTS of a screen's left or
/// right edge docks to that edge; anything further in lands on the bottom
/// island. ~120pt is a comfortable throw target without turning the whole lower
/// third of the screen into a side dock.
const EDGE_SNAP_PT: f64 = 120.0;
/// How often the hover watch re-tests whether the cursor is over the capsule
/// (YV65). Fast enough that the pill is already grabbable by the time a hand
/// arrives on it, slow enough to stay off the main thread's back.
///
/// POLL, not an event, because the panel is click-through: an NSTrackingArea
/// only reports enter/exit for a window that takes the cursor, which is the very
/// thing this watch exists to turn on. It ticks ONLY while the pill is on
/// screen — see [`PILL_SHOWN`] and [`HOVER_IDLE_TICK_MS`] (YV81).
const HOVER_TICK_MS: u64 = 75;
/// The hover watch's tick while the pill is HIDDEN (YV81): there is no capsule
/// to be over, so the thread parks on a long sleep instead of waking the main
/// thread 13 times a second to answer "no" for a window nobody can see.
const HOVER_IDLE_TICK_MS: u64 = 1000;
/// How often the space keeper re-asserts the panel's dock + level. One tick per
/// 1.5s is a Space swipe's own timescale — the pill is back on top before a
/// hand leaves the trackpad — and, like the hover watch, it does no main-thread
/// work at all while the pill is hidden (YV81).
const KEEPER_TICK_MS: u64 = 1500;

static KEEPER_ON: AtomicBool = AtomicBool::new(false);
static PANEL_READY: AtomicBool = AtomicBool::new(false);
/// Current dock edge as `PillPosition::as_u8` (YV53). Read by every park, so
/// the space-keeper and any display change pick the new edge up on their own.
static POSITION: AtomicU8 = AtomicU8::new(0);
static HOVER_ON: AtomicBool = AtomicBool::new(false);
/// YV81 — is the pill on screen right now? Set by [`show_float`] /
/// [`hide_float`] (both main-thread), read by the two watch threads so a hidden
/// pill costs nothing, and reported to the webview as `pill_visible` so the
/// canvas can park its animation loop with it.
static PILL_SHOWN: AtomicBool = AtomicBool::new(false);
/// True from pointer-down on the capsule to pointer-up (YV65). Parks are
/// suspended while it is set so the space-keeper cannot yank the panel back to
/// its old dock in the middle of the gesture.
static DRAGGING: AtomicBool = AtomicBool::new(false);
/// Panel origin (physical px) captured at drag start. Every move is an offset
/// from THIS, not from the last position, so a dropped move event can never
/// accumulate drift.
static DRAG_ORIGIN_X: AtomicI32 = AtomicI32::new(0);
static DRAG_ORIGIN_Y: AtomicI32 = AtomicI32::new(0);
/// Monitor scale factor at drag start, ×1000 — the webview reports its deltas
/// in logical points and the panel is positioned in physical px.
static DRAG_SCALE_M: AtomicI32 = AtomicI32::new(1000);
/// The VISIBLE capsule's rect inside the float window, in logical points,
/// reported by the pill webview (`pill_set_hitbox`). Width 0 → nothing is
/// grabbable yet, so the panel stays fully click-through.
static HIT_X: AtomicI32 = AtomicI32::new(0);
static HIT_Y: AtomicI32 = AtomicI32::new(0);
static HIT_W: AtomicI32 = AtomicI32::new(0);
static HIT_H: AtomicI32 = AtomicI32::new(0);
/// Mirrors the last `set_ignore_cursor_events` call so AppKit is only touched
/// on a transition, not 13 times a second.
static INTERACTIVE: AtomicBool = AtomicBool::new(false);

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

    /// The value this dock writes into `AppSettings::pill_position` (YV65) —
    /// the exact strings [`PillPosition::from_settings`] parses, so a snap and
    /// the Settings picker persist the same thing.
    pub fn as_settings(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
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

/// Pure snap math (YV65): which dock a drag release belongs to.
///
/// `release` is the cursor point at mouse-up and `screen_frame` is the
/// (x, y, width, height) of the monitor the pill lives on — BOTH in logical
/// points in the same (desktop-global) coordinate space the webview reports
/// `screenX`/`screenY` in. Only the horizontal throw matters: the side docks are
/// the two edges, and the bottom island is the neutral place everything else
/// falls back to — the same three placements [`panel_origin`] already lays out.
pub fn snap_position(release: (f64, f64), screen_frame: (f64, f64, f64, f64)) -> PillPosition {
    let (x, _) = release;
    let (frame_x, _, frame_w, _) = screen_frame;
    if x - frame_x <= EDGE_SNAP_PT {
        PillPosition::Left
    } else if (frame_x + frame_w) - x <= EDGE_SNAP_PT {
        PillPosition::Right
    } else {
        PillPosition::Bottom
    }
}

/// The pill webview reports the capsule's rect inside the float window, in
/// logical points (YV65). See [`start_hover_watch`] for why this exists.
pub fn set_hitbox(x: f64, y: f64, w: f64, h: f64) {
    HIT_X.store(x.round() as i32, Ordering::SeqCst);
    HIT_Y.store(y.round() as i32, Ordering::SeqCst);
    HIT_W.store(w.round() as i32, Ordering::SeqCst);
    HIT_H.store(h.round() as i32, Ordering::SeqCst);
}

/// Pure hit test for the reported capsule rect. `point` is window-relative in
/// logical points; a zero-width rect (nothing reported yet) is never a hit.
fn point_in_hitbox(point: (f64, f64), hit: (i32, i32, i32, i32)) -> bool {
    let (px, py) = point;
    let (hx, hy, hw, hh) = (hit.0 as f64, hit.1 as f64, hit.2 as f64, hit.3 as f64);
    hw > 0.0 && hh > 0.0 && px >= hx && px <= hx + hw && py >= hy && py <= hy + hh
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
    // A drag owns the panel origin until the user lets go (YV65) — otherwise the
    // 1.5 s space-keeper re-park snatches the pill back to its old dock in the
    // middle of the gesture.
    if DRAGGING.load(Ordering::SeqCst) {
        return;
    }
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
        // YV65 carved the ONE exception: while the cursor is over the capsule
        // itself the hover watch turns events back on so the pill can be dragged.
        // Re-assert from that live state, never a blind `true`, or a show landing
        // mid-hover would take the grab away until the pointer left and returned.
        let _ = w.set_ignore_cursor_events(!INTERACTIVE.load(Ordering::SeqCst));
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
            // AFTER the panel is actually up (YV81): the webview treats this
            // event as "you are on screen now", and a pill that heard it while
            // still ordered out would have nothing to wake for.
            set_shown(app, true);
            return Ok(());
        }
    }

    if let Some(w) = app.get_webview_window("float") {
        let _ = w.show();
    }
    set_shown(app, true);
    Ok(())
}

pub fn hide_float(app: &AppHandle) {
    set_shown(app, false);
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

/// YV81 — record the pill's on-screen state and tell the webview about it, on
/// the EDGE only. The canvas parks its animation loop while the answer is
/// `false`: a window nobody can see must not paint 60 (or even 10) times a
/// second.
fn set_shown(app: &AppHandle, shown: bool) {
    if PILL_SHOWN.swap(shown, Ordering::SeqCst) == shown {
        return;
    }
    let _ = app.emit_to("float", "pill_visible", shown);
}

/// Is the pill on screen? The gate both watch threads below poll against, and
/// the `pill_shown` field of the YV81 energy telemetry line.
pub fn is_shown() -> bool {
    PILL_SHOWN.load(Ordering::SeqCst)
}

/// How many of this module's recurring polls are doing real work right now
/// (YV81 telemetry). Both threads exist for the visible pill only, so a hidden
/// pill answers `0` however many threads are alive.
pub fn active_polls() -> usize {
    if !is_shown() {
        return 0;
    }
    usize::from(HOVER_ON.load(Ordering::SeqCst)) + usize::from(KEEPER_ON.load(Ordering::SeqCst))
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

/// The monitor the pill parks on, as a LOGICAL-point frame (x, y, w, h) — the
/// same space the webview reports cursor coordinates in. Mirrors `park_pill`'s
/// primary-monitor pin so a snap can never resolve against a different screen
/// than the one the pill will be parked on.
fn monitor_frame(w: &tauri::WebviewWindow) -> Option<(f64, f64, f64, f64)> {
    let mon = w
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| w.current_monitor().ok().flatten())?;
    let scale = mon.scale_factor();
    let pos = mon.position();
    let size = mon.size();
    Some((
        pos.x as f64 / scale,
        pos.y as f64 / scale,
        size.width as f64 / scale,
        size.height as f64 / scale,
    ))
}

/// YV65 — the user pressed the capsule. Latch the panel's current origin so
/// every subsequent move is an absolute offset from it, and suspend parking.
pub fn drag_start(app: &AppHandle) {
    let Some(w) = app.get_webview_window("float") else {
        return;
    };
    let Ok(origin) = w.outer_position() else {
        return;
    };
    DRAG_ORIGIN_X.store(origin.x, Ordering::SeqCst);
    DRAG_ORIGIN_Y.store(origin.y, Ordering::SeqCst);
    DRAG_SCALE_M.store(
        (w.scale_factor().unwrap_or(1.0) * 1000.0).round() as i32,
        Ordering::SeqCst,
    );
    DRAGGING.store(true, Ordering::SeqCst);
}

/// Move the panel live. `dx`/`dy` are the cursor's total travel since
/// `drag_start`, in logical points (what the webview measures).
pub fn drag_move(app: &AppHandle, dx: f64, dy: f64) {
    if !DRAGGING.load(Ordering::SeqCst) {
        return;
    }
    let Some(w) = app.get_webview_window("float") else {
        return;
    };
    let scale = DRAG_SCALE_M.load(Ordering::SeqCst) as f64 / 1000.0;
    let x = DRAG_ORIGIN_X.load(Ordering::SeqCst) + (dx * scale).round() as i32;
    let y = DRAG_ORIGIN_Y.load(Ordering::SeqCst) + (dy * scale).round() as i32;
    let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
}

/// End a drag and dock. `release` is the cursor point at mouse-up in logical
/// points; `snap` is false for a press that never passed the drag slop (a plain
/// click), which just re-parks the pill where it already lived.
///
/// Returns the resulting dock so the caller can persist it — the SETTING is the
/// source of truth (see `pill_drag_end` in lib.rs), exactly as it is for the
/// Settings picker.
pub fn drag_end(app: &AppHandle, release: (f64, f64), snap: bool) -> Option<PillPosition> {
    if !DRAGGING.swap(false, Ordering::SeqCst) {
        return None;
    }
    let pos = snap
        .then(|| {
            app.get_webview_window("float")
                .and_then(|w| monitor_frame(&w))
                .map(|frame| snap_position(release, frame))
        })
        .flatten()
        .unwrap_or_else(|| PillPosition::from_u8(POSITION.load(Ordering::SeqCst)));
    set_position(pos);
    reposition(app);
    Some(pos)
}

/// YV65 — make the panel cursor-interactive ONLY while the pointer is over the
/// visible capsule.
///
/// The float window is deliberately much larger than the pill (shadow room) and
/// has been click-through since it shipped, so it never swallows a click in its
/// transparent margin. Dropping that wholesale to get drag events would eat
/// clicks in an area that looks empty. Instead the pill webview reports the
/// capsule rect and this watch flips `ignore_cursor_events` on the way in and
/// out of it — which is what makes press-and-drag (and the pill's own click)
/// reachable without costing the margin its click-through.
pub fn start_hover_watch(app: AppHandle) {
    if HOVER_ON.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::Builder::new()
        .name("wv-float-hover".into())
        .spawn(move || loop {
            // YV81 — a hidden pill has no capsule to hover, so the cursor test
            // (and the main-thread hop it costs) is skipped entirely and the
            // thread waits on the long tick instead. `hide_float`/`show_float`
            // flip the gate, so the next tick after a show is back at 75ms.
            if !is_shown() {
                thread::sleep(Duration::from_millis(HOVER_IDLE_TICK_MS));
                continue;
            }
            thread::sleep(Duration::from_millis(HOVER_TICK_MS));
            let app_main = app.clone();
            let app_inner = app.clone();
            let _ = app_main.run_on_main_thread(move || update_interactive(&app_inner));
        })
        .ok();
}

fn update_interactive(app: &AppHandle) {
    let Some(w) = app.get_webview_window("float") else {
        return;
    };
    // Never hand the panel back to click-through mid-gesture: a fast drag
    // outruns the cursor test and the mouse-up would land on the floor.
    let want = DRAGGING.load(Ordering::SeqCst)
        || (w.is_visible().unwrap_or(false) && cursor_over_capsule(app, &w));
    if INTERACTIVE.swap(want, Ordering::SeqCst) != want {
        let _ = w.set_ignore_cursor_events(!want);
    }
}

fn cursor_over_capsule(app: &AppHandle, w: &tauri::WebviewWindow) -> bool {
    let (Ok(cursor), Ok(origin), Ok(scale)) =
        (app.cursor_position(), w.outer_position(), w.scale_factor())
    else {
        return false;
    };
    let point = (
        (cursor.x - origin.x as f64) / scale,
        (cursor.y - origin.y as f64) / scale,
    );
    point_in_hitbox(
        point,
        (
            HIT_X.load(Ordering::SeqCst),
            HIT_Y.load(Ordering::SeqCst),
            HIT_W.load(Ordering::SeqCst),
            HIT_H.load(Ordering::SeqCst),
        ),
    )
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
            thread::sleep(Duration::from_millis(KEEPER_TICK_MS));
            // YV81 — a hidden pill cannot lose its Space, so the tick costs a
            // relaxed atomic read rather than a main-thread hop + a window
            // query. The `is_visible` check below stays as the authority; this
            // only avoids paying for it when the answer is already known.
            if !is_shown() {
                continue;
            }
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
    use super::{panel_origin, point_in_hitbox, snap_position, PillPosition, EDGE_SNAP_PT};

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

    // ── YV65: drag-release snapping ──
    // A 1440x900-point screen at the desktop origin, in the LOGICAL points the
    // webview reports a release point in.
    const FRAME: (f64, f64, f64, f64) = (0.0, 0.0, 1440.0, 900.0);

    #[test]
    fn snap_left_edge_release_docks_left() {
        assert_eq!(snap_position((0.0, 450.0), FRAME), PillPosition::Left);
        assert_eq!(snap_position((37.0, 880.0), FRAME), PillPosition::Left);
    }

    #[test]
    fn snap_right_edge_release_docks_right() {
        assert_eq!(snap_position((1440.0, 450.0), FRAME), PillPosition::Right);
        assert_eq!(snap_position((1402.0, 20.0), FRAME), PillPosition::Right);
    }

    #[test]
    fn snap_middle_release_falls_back_to_the_bottom_island() {
        assert_eq!(snap_position((720.0, 450.0), FRAME), PillPosition::Bottom);
        // Anywhere in the middle band, at any height — the throw is horizontal.
        assert_eq!(snap_position((300.0, 12.0), FRAME), PillPosition::Bottom);
        assert_eq!(snap_position((1100.0, 890.0), FRAME), PillPosition::Bottom);
    }

    /// The threshold is INCLUSIVE on both edges, and one point past it is
    /// already the bottom island — no dead zone, no overlap.
    #[test]
    fn snap_threshold_boundary_is_inclusive_on_both_edges() {
        assert_eq!(
            snap_position((EDGE_SNAP_PT, 450.0), FRAME),
            PillPosition::Left
        );
        assert_eq!(
            snap_position((EDGE_SNAP_PT + 1.0, 450.0), FRAME),
            PillPosition::Bottom
        );
        assert_eq!(
            snap_position((1440.0 - EDGE_SNAP_PT, 450.0), FRAME),
            PillPosition::Right
        );
        assert_eq!(
            snap_position((1440.0 - EDGE_SNAP_PT - 1.0, 450.0), FRAME),
            PillPosition::Bottom
        );
    }

    /// A release on a secondary display snaps against THAT screen's frame — a
    /// monitor to the left of the primary has negative x, and a point sitting at
    /// x = 0 there is its RIGHT edge, not its left one.
    #[test]
    fn snap_follows_a_monitor_that_is_not_at_the_desktop_origin() {
        let frame = (-1920.0, -300.0, 1920.0, 1080.0);
        assert_eq!(snap_position((-1920.0, 0.0), frame), PillPosition::Left);
        assert_eq!(snap_position((0.0, 0.0), frame), PillPosition::Right);
        assert_eq!(snap_position((-960.0, 0.0), frame), PillPosition::Bottom);
    }

    /// Every snap result is a dock the existing YV53 position math can lay out,
    /// so a drag can only ever land the pill where the picker could put it.
    #[test]
    fn snap_results_round_trip_through_the_settings_string() {
        for x in [0.0, 60.0, 720.0, 1380.0, 1440.0] {
            let pos = snap_position((x, 450.0), FRAME);
            assert_eq!(PillPosition::from_settings(pos.as_settings()), pos);
        }
    }

    /// The capsule hit-box gates whether the panel takes the cursor at all: an
    /// unreported (zero) box must stay fully click-through.
    #[test]
    fn hitbox_only_captures_the_reported_capsule() {
        let hit = (120, 62, 60, 16);
        assert!(point_in_hitbox((150.0, 70.0), hit));
        assert!(point_in_hitbox((120.0, 62.0), hit)); // top-left corner is a hit
        assert!(point_in_hitbox((180.0, 78.0), hit)); // bottom-right corner too
        assert!(!point_in_hitbox((119.0, 70.0), hit));
        assert!(!point_in_hitbox((150.0, 40.0), hit));
        assert!(!point_in_hitbox((150.0, 70.0), (0, 0, 0, 0)));
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
