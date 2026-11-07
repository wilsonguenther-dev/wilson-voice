//! Real macOS NSPanel Dictate island (via tauri-nspanel).
//!
//! This is NOT a normal NSWindow with rounded CSS.
//! Flow: WebviewWindow → to_panel::<DictatePill>() → NSPanel flags:
//!   nonactivatingPanel, floating/status level, canJoinAllSpaces,
//!   fullScreenAuxiliary, transparent host so only the glass pill is visible.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;
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
pub const HOVER_TICK_MS: u64 = 75;
/// The hover watch's tick while the pill is HIDDEN (YV81): there is no capsule
/// to be over, so the thread parks on a long sleep instead of waking the main
/// thread 13 times a second to answer "no" for a window nobody can see.
///
/// YV95 / OS-12 gave this constant a second job — see [`hover_tick_ms`].
pub const HOVER_IDLE_TICK_MS: u64 = 1000;
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
/// WF-02 — wall-clock ms at drag start, for the one duration the drag logs.
static DRAG_T0_MS: AtomicU64 = AtomicU64::new(0);
/// WF-02 — was Esc down at the last check? Latched at drag start and updated by
/// the hover watch, so only a FRESH Esc press mid-drag cancels (an Esc already
/// held when the press began is not the user changing their mind).
static ESC_DOWN: AtomicBool = AtomicBool::new(false);
/// WF-02 — backend → pill webview: the hover watch saw Esc mid-drag and put the
/// panel back on its dock. pill/drag.ts `PILL_DRAG_CANCEL_EVENT` is the same
/// string; the webview's gesture catches up so the release is inert.
pub const PILL_DRAG_CANCEL_EVENT: &str = "pill-drag-cancel";
/// The VISIBLE capsule's rect inside the float window, in logical points,
/// reported by the pill webview (`pill_set_hitbox`). Width 0 → nothing is
/// grabbable yet, so the panel stays fully click-through.
static HIT_X: AtomicI32 = AtomicI32::new(0);
static HIT_Y: AtomicI32 = AtomicI32::new(0);
static HIT_W: AtomicI32 = AtomicI32::new(0);
static HIT_H: AtomicI32 = AtomicI32::new(0);
/// Mirrors the last `set_ignore_cursor_events` call so AppKit is only touched
/// on a transition, not 13 times a second. Doubles as the "is the cursor on the
/// capsule right now" answer the hover watch feeds back into its own cadence
/// (YV95 / OS-12) — it is set from the same test.
static INTERACTIVE: AtomicBool = AtomicBool::new(false);
/// YV95 — is a MEETING being recorded (as opposed to a seconds-long dictation)?
/// Set by the meeting controller's status sink. Read by the hover watch, which
/// is the whole point: see [`hover_tick_ms`].
static MEETING_RECORDING: AtomicBool = AtomicBool::new(false);
/// YV95 (fix round 3) — is a DICTATION take live right now?
///
/// The hover watch's meeting cadence must never apply to a take: a dictation
/// lasts seconds, the pill is its primary control, and this flag is what keeps
/// the two cases apart when they overlap. Set from `lib.rs` at exactly the three
/// places `AppState::recording` is written, so it cannot drift from the thing it
/// mirrors. See [`hover_tick_ms`].
static DICTATING: AtomicBool = AtomicBool::new(false);

/// The capsule's rect in **AppKit screen points** (bottom-left origin), packed
/// as `f64::to_bits`. Written on the main thread by [`update_interactive`],
/// where the window geometry is legal to read; read by the hover watch's cheap
/// proximity check, which runs OFF the main thread.
///
/// Zero width/height means "not known yet", and the proximity check answers
/// `false` — the same "never reported" convention [`point_in_hitbox`] uses.
static CAPSULE_SCREEN_X: AtomicU64 = AtomicU64::new(0);
static CAPSULE_SCREEN_Y: AtomicU64 = AtomicU64::new(0);
static CAPSULE_SCREEN_W: AtomicU64 = AtomicU64::new(0);
static CAPSULE_SCREEN_H: AtomicU64 = AtomicU64::new(0);
/// The last answer the cheap proximity check gave, so the hover watch reacts to
/// the EDGE (cursor arrived) rather than re-waking on every slice while the
/// cursor simply sits on the pill.
static CURSOR_NEAR: AtomicBool = AtomicBool::new(false);

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

/// A monitor in Tauri's global physical-pixel space, with its scale factor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub scale: f64,
}

impl ScreenRect {
    fn same_frame(&self, o: &ScreenRect) -> bool {
        self.x == o.x && self.y == o.y && self.w == o.w && self.h == o.h
    }
}

/// Pure (Y6-D): which screen the pill parks on, given the screens that exist
/// NOW. The primary, when it is one of them; otherwise the first screen that
/// exists. A screen that was unplugged is never in `screens`, so it can never
/// be chosen — the classic floating-HUD bug is reusing a remembered origin on
/// a display that is gone. `None` only when no screen at all is reported.
pub fn pill_screen(screens: &[ScreenRect], primary: Option<ScreenRect>) -> Option<ScreenRect> {
    match primary {
        Some(p) if screens.is_empty() || screens.iter().any(|s| s.same_frame(&p)) => Some(p),
        _ => screens.first().copied(),
    }
}

/// Pure (Y6-D): keep a `win`-sized window whose top-left is `origin` inside
/// `screen`. A window larger than the screen pins to the screen's origin.
pub fn clamp_to_screen(origin: (i32, i32), win: (i32, i32), screen: ScreenRect) -> (i32, i32) {
    let max_x = (screen.x + screen.w - win.0).max(screen.x);
    let max_y = (screen.y + screen.h - win.1).max(screen.y);
    (
        origin.0.clamp(screen.x, max_x),
        origin.1.clamp(screen.y, max_y),
    )
}

/// Pure (Y6-D): the float window's top-left in physical px, on a screen that
/// exists, for a window of `win_pt` logical points docked at `pos`.
pub fn pill_origin_on_visible_screen(
    screens: &[ScreenRect],
    primary: Option<ScreenRect>,
    win_pt: (f64, f64),
    margin_pt: f64,
    pos: PillPosition,
) -> Option<(i32, i32)> {
    let s = pill_screen(screens, primary)?;
    let win = ((win_pt.0 * s.scale) as i32, (win_pt.1 * s.scale) as i32);
    let margin = (margin_pt * s.scale) as i32;
    let origin = panel_origin((s.x, s.y), (s.w, s.h), win, margin, pos);
    Some(clamp_to_screen(origin, win, s))
}

/// The float window's size in logical points: the frame the pill webview last
/// asked for (yap-1106-one-pill), or the historical 300x140 before it has.
pub fn float_window_size_pt() -> (f64, f64) {
    let f = panel_frame();
    (f.w, f.h)
}

/// yap-1106-one-pill — the panel is sized to its content. The pill webview's
/// one layout (desktop/src/pill/state.ts `pillLayout`) is the union of the
/// character, the clock, the hint and the bubble plus a margin; it sends that
/// size and the point the character sits at inside it (`anchor_*`), all in
/// logical points. Nothing is drawn outside this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelFrame {
    pub w: f64,
    pub h: f64,
    pub anchor_x: f64,
    pub anchor_y: f64,
}

/// The smallest and largest frame a request may set (points). A request
/// outside is clamped, never trusted: a bad layout must not make a panel that
/// covers the screen or vanishes.
pub const PANEL_MIN_PT: (f64, f64) = (40.0, 30.0);
pub const PANEL_MAX_PT: (f64, f64) = (640.0, 400.0);

impl PanelFrame {
    /// The fixed panel every build before yap-1106-one-pill used, centred.
    pub const LEGACY: PanelFrame = PanelFrame {
        w: PILL_W,
        h: PILL_H,
        anchor_x: 150.0,
        anchor_y: 70.0,
    };

    /// Pure: a webview request made safe. `None` for a non-finite number; the
    /// size is clamped to [`PANEL_MIN_PT`]..[`PANEL_MAX_PT`] and the anchor
    /// into the frame.
    pub fn sanitize(w: f64, h: f64, anchor_x: f64, anchor_y: f64) -> Option<PanelFrame> {
        if !(w.is_finite() && h.is_finite() && anchor_x.is_finite() && anchor_y.is_finite()) {
            return None;
        }
        let w = w.round().clamp(PANEL_MIN_PT.0, PANEL_MAX_PT.0);
        let h = h.round().clamp(PANEL_MIN_PT.1, PANEL_MAX_PT.1);
        Some(PanelFrame {
            w,
            h,
            anchor_x: anchor_x.clamp(0.0, w),
            anchor_y: anchor_y.clamp(0.0, h),
        })
    }
}

static PANEL_FRAME: Mutex<PanelFrame> = Mutex::new(PanelFrame::LEGACY);

/// The current panel frame (logical points).
pub fn panel_frame() -> PanelFrame {
    PANEL_FRAME.lock().map(|f| *f).unwrap_or(PanelFrame::LEGACY)
}

/// Store a new panel frame and, if it changed, resize the panel and re-park it
/// by its anchor on the main thread. One log line per change.
pub fn set_panel_frame(app: &AppHandle, w: f64, h: f64, anchor_x: f64, anchor_y: f64) {
    let Some(next) = PanelFrame::sanitize(w, h, anchor_x, anchor_y) else {
        log::warn!("pill panel_frame rejected reason=non_finite");
        return;
    };
    let changed = match PANEL_FRAME.lock() {
        Ok(mut cur) => {
            let changed = *cur != next;
            *cur = next;
            changed
        }
        Err(_) => {
            log::warn!("pill panel_frame rejected reason=lock_poisoned");
            return;
        }
    };
    if !changed {
        return;
    }
    log::info!(
        "pill panel_frame w={} h={} anchor={:.0}x{:.0} dock={:?}",
        next.w,
        next.h,
        next.anchor_x,
        next.anchor_y,
        PillPosition::from_u8(POSITION.load(Ordering::SeqCst))
    );
    let app2 = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        if let Some(win) = app2.get_webview_window("float") {
            if let Err(e) = win.set_size(tauri::Size::Logical(tauri::LogicalSize {
                width: next.w,
                height: next.h,
            })) {
                log::warn!("pill panel_frame resize_failed err={e}");
            }
        }
        park_pill(&app2);
    }) {
        log::warn!("pill panel_frame dispatch_failed err={e}");
    }
}

/// Pure (yap-1106-one-pill): the panel's top-left, in physical px, parked so
/// the CHARACTER (at `anchor` inside the window, physical px) sits where the
/// fixed panel used to centre it: the bottom dock centres him horizontally, a
/// side dock centres him vertically, flush to that edge. So when the clock
/// slot appears and the panel grows around him, Yappy does not move.
fn panel_origin_anchored(
    mon_pos: (i32, i32),
    mon_size: (i32, i32),
    win_size: (i32, i32),
    anchor: (i32, i32),
    margin: i32,
    pos: PillPosition,
) -> (i32, i32) {
    let (mon_x, mon_y) = mon_pos;
    let (mon_w, mon_h) = mon_size;
    let (win_w, win_h) = win_size;
    let (ax, ay) = anchor;
    match pos {
        PillPosition::Bottom => (mon_x + mon_w / 2 - ax, mon_y + mon_h - win_h - margin),
        PillPosition::Left => (mon_x, mon_y + mon_h / 2 - ay),
        PillPosition::Right => (mon_x + mon_w - win_w, mon_y + mon_h / 2 - ay),
    }
}

/// Pure (yap-1106-one-pill): [`pill_origin_on_visible_screen`] for a panel
/// frame with an anchor — picks a screen that exists, parks by the anchor and
/// clamps the whole frame onto that screen.
pub fn pill_origin_for_frame(
    screens: &[ScreenRect],
    primary: Option<ScreenRect>,
    frame: PanelFrame,
    margin_pt: f64,
    pos: PillPosition,
) -> Option<(i32, i32)> {
    let s = pill_screen(screens, primary)?;
    let win = ((frame.w * s.scale) as i32, (frame.h * s.scale) as i32);
    let anchor = (
        (frame.anchor_x * s.scale) as i32,
        (frame.anchor_y * s.scale) as i32,
    );
    let margin = (margin_pt * s.scale) as i32;
    let origin = panel_origin_anchored((s.x, s.y), (s.w, s.h), win, anchor, margin, pos);
    Some(clamp_to_screen(origin, win, s))
}

/// Pure (Y6-D): does the pill come up at launch? Only once onboarding is done.
/// A cold launch — no settings file, so `onboarded == false` — lands on
/// onboarding with no model and no grants; an idle pill there would promise a
/// dictation the app cannot take.
pub fn launch_pill_visible(show_floating_pill: bool, onboarded: bool) -> bool {
    show_floating_pill && onboarded
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
    // Y6-D: chosen from the monitors that exist NOW (re-read every keeper tick),
    // and clamped inside the chosen one — an unplugged display can never hold it.
    let rect = |m: &tauri::Monitor| ScreenRect {
        x: m.position().x,
        y: m.position().y,
        w: m.size().width as i32,
        h: m.size().height as i32,
        scale: m.scale_factor(),
    };
    let screens: Vec<ScreenRect> = w
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(rect)
        .collect();
    let primary = w
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| w.current_monitor().ok().flatten())
        .map(|m| rect(&m));
    // Window bottom sits ~14pt off the screen bottom; the pill (centered in the
    // taller window) then floats ~50pt up with shadow room below it.
    // yap-1106-one-pill — parked by the character's anchor inside the frame
    // the webview sized to its content (PanelFrame::LEGACY until it asks).
    let Some((x, y)) = pill_origin_for_frame(
        &screens,
        primary,
        panel_frame(),
        14.0,
        PillPosition::from_u8(POSITION.load(Ordering::SeqCst)),
    ) else {
        return;
    };
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
        // Y7-B — a `--smoke` dev run may pin the pill to one phase/dock for
        // scripts/smoke-windowed.sh; always "" in a shipped build (lib.rs).
        let url = WebviewUrl::App(
            format!(
                "float.html{}",
                crate::smoke_forced_float_query().unwrap_or_default()
            )
            .into(),
        );
        let mut builder = WebviewWindowBuilder::new(app, "float", url)
            .title("")
            .inner_size(float_window_size_pt().0, float_window_size_pt().1)
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

        let win = builder.build().map_err(|e| format!("float window: {e}"))?;
        // yap-call-1 / WF-16 — built through the one rule: outside a meeting
        // the pill shows in a screen share; while a meeting records (and the
        // "Hide Yap from screen sharing" setting is on) it is hidden, and
        // `sync_capture_hidden` flips it back on stop.
        crate::call_offer::apply_content_protection(&win);
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

/// YV95 — tell the pill a meeting is (or is no longer) recording. Called from
/// the meeting controller's status sink, so the flag flips on exactly the same
/// event the UI does.
pub fn set_meeting_recording(recording: bool) {
    MEETING_RECORDING.store(recording, Ordering::SeqCst);
}

pub fn meeting_recording() -> bool {
    MEETING_RECORDING.load(Ordering::SeqCst)
}

// ─── WF-16: hide Yap from screen sharing while a meeting records ─────────────
//
// A setting (`AppSettings::hide_from_capture_in_meetings`, on by default)
// keeps the pill (with its meeting chip) and the Scratchpad out of screen
// shares and screenshots while a meeting records; they come back in captures
// the moment it stops. The call-offer window is ALWAYS hidden
// (`call_offer::content_protected_for`). On macOS Tauri's content protection
// sets the NSWindow `sharingType` to `.none` when on and restores `.readOnly`,
// the default, when off. The only caller of the Tauri call stays
// `call_offer::apply_content_protection`, which reads [`capture_hidden_for`].
// No Screen Recording permission and no private API are involved.

/// The windows WF-16 hides from capture while a meeting records: the pill
/// panel (which also draws the meeting chip) and the Scratchpad.
pub const MEETING_CAPTURE_HIDDEN_WINDOWS: [&str; 2] = ["float", crate::scratchpad::WINDOW_LABEL];

/// WF-16 — mirror of the setting. Starts true, the setting's default, and is
/// re-set from the stored settings at launch and on every save.
static HIDE_FROM_CAPTURE_SETTING: AtomicBool = AtomicBool::new(true);

/// WF-16 — the edge latch shared by every caller of [`sync_capture_hidden`].
static CAPTURE_TOGGLE: Mutex<CaptureToggle> = Mutex::new(CaptureToggle::new());

/// WF-16 — the pure decision: hide while a meeting records AND the setting is on.
pub fn capture_hidden_wanted(setting_on: bool, meeting_recording: bool) -> bool {
    setting_on && meeting_recording
}

/// WF-16 — the per-window rule. The call offer is always hidden; the pill and
/// the Scratchpad only while `meeting_hidden` (see [`capture_hidden_wanted`]);
/// every other window (main) is never hidden.
pub fn capture_hidden_for(label: &str, meeting_hidden: bool) -> bool {
    crate::call_offer::content_protected_for(label)
        || (meeting_hidden && MEETING_CAPTURE_HIDDEN_WINDOWS.contains(&label))
}

/// WF-16 — the live answer: what a window built right now should get.
pub fn capture_hidden_now() -> bool {
    capture_hidden_wanted(
        HIDE_FROM_CAPTURE_SETTING.load(Ordering::SeqCst),
        meeting_recording(),
    )
}

/// WF-16 — tell the toggle what the setting says (launch + `save_settings`).
pub fn set_hide_from_capture(on: bool) {
    HIDE_FROM_CAPTURE_SETTING.store(on, Ordering::SeqCst);
}

pub fn hide_from_capture() -> bool {
    HIDE_FROM_CAPTURE_SETTING.load(Ordering::SeqCst)
}

/// WF-16 — the toggle function. Fed the setting and the meeting state on every
/// meeting status tick (1 Hz while recording) and on every settings save, it
/// returns `Some(hidden)` ONLY on an edge, so the windows are touched once on
/// record start and once on stop, never 3,600 times an hour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureToggle {
    applied: Option<bool>,
}

impl CaptureToggle {
    pub const fn new() -> Self {
        Self { applied: None }
    }

    /// What the windows currently carry (`None` = never applied: every window
    /// was built with the launch value from [`capture_hidden_now`]).
    pub fn applied(&self) -> Option<bool> {
        self.applied
    }

    pub fn step(&mut self, setting_on: bool, meeting_recording: bool) -> Option<bool> {
        let want = capture_hidden_wanted(setting_on, meeting_recording);
        // First observation while nothing is hidden: the windows were built
        // with `false` already — record it without touching them.
        if self.applied.is_none() && !want {
            self.applied = Some(false);
            return None;
        }
        if self.applied == Some(want) {
            return None;
        }
        self.applied = Some(want);
        Some(want)
    }
}

impl Default for CaptureToggle {
    fn default() -> Self {
        Self::new()
    }
}

/// Why a capture edge happened, for the log line only.
fn capture_reason(hidden: bool, setting_on: bool) -> &'static str {
    match (hidden, setting_on) {
        (true, _) => "meeting_start",
        (false, false) => "setting_off",
        (false, true) => "meeting_stop",
    }
}

/// WF-16 — bring the pill and Scratchpad windows in line with the setting and
/// the meeting state. Cheap when nothing changed (two atomics + a latch), so
/// the meeting status sink calls it on every tick. A window that does not
/// exist yet is skipped: it reads [`capture_hidden_now`] when it is built.
pub fn sync_capture_hidden<R: tauri::Runtime>(app: &AppHandle<R>) {
    let setting_on = hide_from_capture();
    let recording = meeting_recording();
    let edge = match CAPTURE_TOGGLE.lock() {
        Ok(mut t) => t.step(setting_on, recording),
        Err(poisoned) => poisoned.into_inner().step(setting_on, recording),
    };
    let Some(hidden) = edge else {
        return;
    };
    let mut windows = 0u32;
    for label in MEETING_CAPTURE_HIDDEN_WINDOWS {
        if let Some(w) = app.get_webview_window(label) {
            crate::call_offer::apply_content_protection(&w);
            windows += 1;
        }
    }
    log::info!(
        "float_pill capture_hidden hidden={hidden} reason={} windows={windows}",
        capture_reason(hidden, setting_on)
    );
}

/// YV95 — tell the pill a dictation take is (or is no longer) live. Called from
/// `lib.rs` beside every write to `AppState::recording`, so the pill's idea of
/// "a take is running" is the app's idea of it.
pub fn set_dictating(dictating: bool) {
    DICTATING.store(dictating, Ordering::SeqCst);
}

pub fn dictating() -> bool {
    DICTATING.load(Ordering::SeqCst)
}

/// Is the cursor currently on the capsule? (The panel takes the cursor exactly
/// when it is, so this is the same bit.)
pub fn is_hovered() -> bool {
    INTERACTIVE.load(Ordering::SeqCst)
}

/// **YV95 / OS-12 fix (2)** — how long the hover watch waits between
/// **main-thread** cursor tests.
///
/// YV81's rule was binary: hidden → 1 Hz, shown → 13 Hz. That was written for a
/// pill whose visible life is one dictation, a few seconds long. A meeting makes
/// the pill visible for **up to three hours**, which under the old rule is
/// ~144,000 cursor tests and ~144,000 `run_on_main_thread` hops — precisely the
/// state YV81 exists to prevent, held for 180 minutes, on battery.
///
/// So visibility stops being the whole question and INTERACTION joins it: a pill
/// that is on screen during a meeting but has no cursor on it is doing exactly
/// what a hidden pill was doing — answering "no" — and pays the hidden price
/// for it.
///
/// ## Two things this function is NOT allowed to cost (fix round 3)
///
/// The first version of this policy took three inputs and got both of them
/// wrong, because it treated "a meeting is recording" as a statement about the
/// pill rather than about the machine:
///
/// 1. **It starved dictation.** Dictating *during* a meeting is a designed case
///    — YV91 fans the same block out to both consumers, and `live.ts`'s
///    `framePlan` deliberately puts the live-take branch BEFORE the meeting
///    park for exactly this reason. The Rust side did not have the matching
///    ordering, so a take taken during a meeting polled at 1 Hz: the pill is
///    that take's primary control, and it became unresponsive for the length of
///    every sentence. `dictating` is now an input, and it is checked FIRST, so
///    the two files answer this question the same way.
///
/// 2. **It swallowed the first click on the pill.** The panel is click-through
///    until this watch notices the cursor on the capsule, so "noticed within a
///    second" is not a second of latency — it is a click delivered to whatever
///    app is underneath, which during a meeting is the meeting. Slowing the
///    poll down is only safe because the cadence below is no longer the only
///    thing that can wake the watch: [`start_hover_watch`] sleeps in short
///    slices and breaks out the instant the OFF-main-thread proximity check
///    ([`cursor_on_capsule_cheap`]) sees the cursor arrive. The main-thread test
///    rate is what this function sets, and it is still 1 Hz through the
///    far-field of a meeting; the *reachability* of the pill is no longer tied
///    to it.
pub fn hover_tick_ms(shown: bool, hovered: bool, meeting_recording: bool, dictating: bool) -> u64 {
    if !shown {
        // A window nobody can see must not paint — or poll — 13 times a second.
        return HOVER_IDLE_TICK_MS;
    }
    // A take lasts seconds and the pill is its primary control. This branch is
    // deliberately ABOVE the meeting branch: a dictation inside a meeting is a
    // dictation, and OS-12 never measured a few seconds of 13 Hz.
    if dictating {
        return HOVER_TICK_MS;
    }
    if meeting_recording && !hovered {
        // Visible, but nobody is touching it, and this will be true for hours.
        return HOVER_IDLE_TICK_MS;
    }
    HOVER_TICK_MS
}

/// How long the hover watch actually sleeps at a stretch before re-running the
/// cheap proximity check. Short enough that a cursor arriving on the capsule is
/// noticed faster than any hand can follow it with a click; cheap enough to be
/// irrelevant to OS-12, because a slice costs four relaxed atomic loads and one
/// `+[NSEvent mouseLocation]` on a background thread — no main-thread hop, no
/// window query, no AppKit object.
pub const PROXIMITY_SLICE_MS: u64 = 30;

/// The slice plan for one `tick_ms` wait: `(sleeps, last_ms)`. The first
/// `sleeps - 1` naps are `slice_ms` long and the last is `last_ms`, so the total
/// is always exactly `tick_ms`.
///
/// Pure, so "a 1 Hz tick is still 1 Hz" is arithmetic a test can check rather
/// than a claim in a comment — the whole defence of fix (2) is that the
/// MAIN-THREAD test rate did not change, and this is the function that has to
/// keep that true.
pub fn sleep_slices(tick_ms: u64, slice_ms: u64) -> (u64, u64) {
    let slice = slice_ms.max(1);
    let sleeps = (tick_ms / slice).max(1);
    (sleeps, tick_ms - (sleeps - 1) * slice)
}

/// Should the hover watch abandon the rest of its wait and test the cursor on
/// the main thread right now?
///
/// Only on the RISING edge — the cursor was not on the capsule and now is.
/// Without the edge, a cursor parked on the pill would break every slice and
/// turn the slow tick into a 33 Hz one, which is worse than the bug this whole
/// item is about.
pub fn should_wake_early(was_near: bool, is_near: bool) -> bool {
    is_near && !was_near
}

/// The capsule's rect in AppKit screen points, from the geometry the main
/// thread can see.
///
/// * `win_origin_phys` — the float window's outer position, Tauri physical px,
///   top-left origin (`WebviewWindow::outer_position`).
/// * `scale` — the pill's monitor scale factor. `park_pill` pins the pill to the
///   PRIMARY monitor, so this is the primary's scale and the conversion below
///   is anchored to the same screen AppKit measures from.
/// * `primary_height_pts` — the primary display's height in points.
/// * `hit` — the capsule rect the webview reported, window-relative logical
///   points with a TOP-LEFT origin (DOM coordinates).
///
/// AppKit's screen space has its origin at the bottom-left of the primary
/// display with y increasing upwards, which is why the y term is flipped twice:
/// once to get the window's bottom edge, once to get the capsule's.
///
/// Returns `None` when nothing has been reported yet — the same convention
/// [`point_in_hitbox`] uses for a zero-sized rect.
pub fn capsule_screen_rect(
    win_origin_phys: (f64, f64),
    scale: f64,
    primary_height_pts: f64,
    hit: (f64, f64, f64, f64),
) -> Option<(f64, f64, f64, f64)> {
    let (hx, hy, hw, hh) = hit;
    if hw <= 0.0 || hh <= 0.0 || scale <= 0.0 {
        return None;
    }
    let win_x_pts = win_origin_phys.0 / scale;
    let win_top_pts = win_origin_phys.1 / scale;
    // The capsule's top edge, measured down from the top of the primary display.
    let capsule_top_pts = win_top_pts + hy;
    // …and the same edge measured up from the bottom, which is AppKit's y.
    let capsule_bottom_appkit = primary_height_pts - (capsule_top_pts + hh);
    Some((win_x_pts + hx, capsule_bottom_appkit, hw, hh))
}

/// Is `point` (AppKit screen points) inside the cached capsule rect?
fn point_in_screen_rect(point: (f64, f64), rect: (f64, f64, f64, f64)) -> bool {
    let (px, py) = point;
    let (x, y, w, h) = rect;
    w > 0.0 && h > 0.0 && px >= x && px <= x + w && py >= y && py <= y + h
}

fn store_f64(cell: &AtomicU64, v: f64) {
    cell.store(v.to_bits(), Ordering::Relaxed);
}

fn load_f64(cell: &AtomicU64) -> f64 {
    f64::from_bits(cell.load(Ordering::Relaxed))
}

/// Publish the capsule's screen rect for the background proximity check.
fn set_capsule_screen_rect(rect: Option<(f64, f64, f64, f64)>) {
    let (x, y, w, h) = rect.unwrap_or((0.0, 0.0, 0.0, 0.0));
    store_f64(&CAPSULE_SCREEN_X, x);
    store_f64(&CAPSULE_SCREEN_Y, y);
    store_f64(&CAPSULE_SCREEN_W, w);
    store_f64(&CAPSULE_SCREEN_H, h);
}

/// The **cheap**, off-main-thread answer to "is the cursor on the capsule?".
///
/// `+[NSEvent mouseLocation]` is a class method on a thread-safe class (objc2
/// exposes it without a `MainThreadMarker`, which is that crate's way of saying
/// so) and it reads the window server's last known cursor position in AppKit
/// screen points — no NSWindow, no view, no main queue.
///
/// **This answer is advisory and can only ever cause MORE work.** It is used in
/// exactly one place: to end a sleep early so [`update_interactive`] — still the
/// only authority on `set_ignore_cursor_events` — runs sooner. If the cached
/// rect is stale or the arithmetic above is wrong, the worst case is a spurious
/// main-thread hop that finds nothing changed and does nothing, or a miss that
/// falls back to the ordinary [`hover_tick_ms`] cadence. Neither is a behaviour
/// change; the cadence is the floor, not the ceiling.
#[cfg(target_os = "macos")]
fn cursor_on_capsule_cheap() -> bool {
    let rect = (
        load_f64(&CAPSULE_SCREEN_X),
        load_f64(&CAPSULE_SCREEN_Y),
        load_f64(&CAPSULE_SCREEN_W),
        load_f64(&CAPSULE_SCREEN_H),
    );
    if rect.2 <= 0.0 || rect.3 <= 0.0 {
        return false;
    }
    let p = objc2_app_kit::NSEvent::mouseLocation();
    point_in_screen_rect((p.x, p.y), rect)
}

#[cfg(not(target_os = "macos"))]
fn cursor_on_capsule_cheap() -> bool {
    false
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

/// yap-p0-insertion-bridge-fallback — the Copy card waiting on the pill. While
/// it is set the pill stays up after the take, whatever `show_floating_pill`
/// says, and a pill webview that mounts late reads it via `pending_copy_card`.
static COPY_CARD: Mutex<Option<crate::paste::CopyCard>> = Mutex::new(None);

/// Park `card` on the pill and make sure the pill is on screen. Safe from any thread.
pub fn hold_copy_card(app: &AppHandle, card: crate::paste::CopyCard) {
    if let Ok(mut slot) = COPY_CARD.lock() {
        *slot = Some(card);
    }
    show_for_recording(app);
}

/// The card the pill should be showing, if any.
pub fn pending_copy_card() -> Option<crate::paste::CopyCard> {
    COPY_CARD.lock().ok().and_then(|slot| slot.clone())
}

fn copy_card_held() -> bool {
    COPY_CARD.lock().map(|slot| slot.is_some()).unwrap_or(false)
}

/// The Tauri event that tells the pill to drop its Copy card without a hide
/// (a new take started; desktop/src/pill/CopyCard.tsx listens).
pub const COPY_CARD_CLEAR_EVENT: &str = "insert-copy-card-clear";

/// Should the pill stay up once the Copy card is released? A take (or a
/// meeting) in flight always keeps it: the card's TTL or × must never hide
/// the pill of a recording or processing take.
pub fn pill_visible_after_card(show_floating_pill: bool, take_in_flight: bool) -> bool {
    show_floating_pill || take_in_flight
}

/// The card was dismissed (Copy, Repair, ×, or its timeout): forget it and let
/// the pill follow `show_floating_pill` again — unless a take is in flight
/// (`take_in_flight`: recording or processing), in which case the pill stays
/// and the take's own `after_recording` decides.
pub fn release_copy_card(app: &AppHandle, show_floating_pill: bool, take_in_flight: bool) {
    if let Ok(mut slot) = COPY_CARD.lock() {
        *slot = None;
    }
    let in_flight = take_in_flight || dictating() || meeting_recording();
    after_recording(app, pill_visible_after_card(show_floating_pill, in_flight));
}

/// A new take started: drop any Copy card from an earlier take (so it can no
/// longer hold the pill's hitbox or the pill itself) WITHOUT hiding the pill.
pub fn clear_copy_card_for_new_take(app: &AppHandle) {
    if let Ok(mut slot) = COPY_CARD.lock() {
        *slot = None;
    }
    // Always emitted: the pill webview may hold a card the slot no longer does.
    if let Err(e) = app.emit(COPY_CARD_CLEAR_EVENT, ()) {
        log::warn!("emit {COPY_CARD_CLEAR_EVENT}: {e}");
    }
}

/// What keeps the pill on screen after a take, whatever `show_floating_pill`
/// says: the caller's own reason, a parked Copy card, or a pending "Record
/// this meeting?" offer (#272 — `meeting_detect::raise` put the pill up for
/// it, so a dictation ending under it must not hide a card nobody answered).
pub fn pill_keeps_up(keep_visible: bool, copy_card_held: bool, offer_pending: bool) -> bool {
    keep_visible || copy_card_held || offer_pending
}

/// yap-fx-06 — a dictation that ends (done, cancelled by Esc, failed) while a
/// meeting is recording never takes the pill down with it: the pill is where
/// the meeting's clock and its one-click Stop live, and the Esc toast
/// ("Dictation cancelled — meeting still recording. Stop meeting?") is drawn
/// in it. Before this, a user with the idle pill switched off lost the meeting
/// badge the moment a dictation inside the meeting ended — a recording with no
/// visible Stop. Pure, so the rule is tested without a window.
pub fn keeps_up_for_live_meeting(keep_visible: bool, meeting_recording: bool) -> bool {
    keep_visible || meeting_recording
}

pub fn after_recording(app: &AppHandle, keep_visible: bool) {
    let meeting_recording = crate::meeting::active_capture().is_some();
    if meeting_recording && !keep_visible {
        log::info!("float_pill after_take kept_up=true reason=meeting_recording");
    }
    let keep_visible = pill_keeps_up(
        keeps_up_for_live_meeting(keep_visible, meeting_recording),
        copy_card_held(),
        crate::meeting_detect::pending().is_some(),
    );
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
    DRAG_T0_MS.store(now_ms(), Ordering::SeqCst);
    ESC_DOWN.store(esc_key_down(), Ordering::SeqCst);
    DRAGGING.store(true, Ordering::SeqCst);
    log::info!(
        "pill drag event=start dock={}",
        PillPosition::from_u8(POSITION.load(Ordering::SeqCst)).as_settings()
    );
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
    let home = PillPosition::from_u8(POSITION.load(Ordering::SeqCst));
    let frame = if snap {
        app.get_webview_window("float")
            .and_then(|w| monitor_frame(&w))
    } else {
        None
    };
    let pos = drag_end_dock(snap, release, frame, home);
    set_position(pos);
    reposition(app);
    log::info!(
        "pill drag event=end snap={snap} from={} dock={} ms={}",
        home.as_settings(),
        pos.as_settings(),
        drag_elapsed_ms()
    );
    Some(pos)
}

/// Pure (WF-02): the dock a drag ends on. A release that SNAPS lands on the
/// dock under the cursor; anything else — a plain click, an Esc mid-drag
/// (pill/drag.ts sends `snap: false`), or a monitor that could not be read —
/// goes HOME to the dock the pill had, whatever point it was released at.
pub fn drag_end_dock(
    snap: bool,
    release: (f64, f64),
    screen_frame: Option<(f64, f64, f64, f64)>,
    home: PillPosition,
) -> PillPosition {
    match (snap, screen_frame) {
        (true, Some(frame)) => snap_position(release, frame),
        _ => home,
    }
}

/// WF-02 — Esc mid-drag: put the panel back where the press found it and
/// re-park it on the dock it already had. Nothing is persisted (the setting
/// never changed). Returns that dock, or `None` when no drag was live (the
/// webview's own `pill_drag_end` got there first). Main thread.
pub fn drag_cancel(app: &AppHandle) -> Option<PillPosition> {
    if !DRAGGING.swap(false, Ordering::SeqCst) {
        return None;
    }
    let home = PillPosition::from_u8(POSITION.load(Ordering::SeqCst));
    if let Some(w) = app.get_webview_window("float") {
        let x = DRAG_ORIGIN_X.load(Ordering::SeqCst);
        let y = DRAG_ORIGIN_Y.load(Ordering::SeqCst);
        let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
    }
    reposition(app);
    log::info!(
        "pill drag event=cancel reason=esc dock={} ms={}",
        home.as_settings(),
        drag_elapsed_ms()
    );
    if let Err(e) = app.emit(PILL_DRAG_CANCEL_EVENT, ()) {
        log::warn!("pill drag event=cancel_emit_failed error={e}");
    }
    Some(home)
}

/// Pure (WF-02): does this Esc reading cancel the drag? Only a FRESH press
/// (up at the last check, down now) while a drag is live.
pub fn esc_cancels_drag(dragging: bool, esc_was_down: bool, esc_down: bool) -> bool {
    dragging && esc_down && !esc_was_down
}

/// The hover watch's per-slice Esc check (WF-02). The float panel is a
/// non-activating NSPanel, so the webview may never be key and never see the
/// keydown; the backend reads the key state instead, only while a drag is live.
fn poll_drag_escape(app: &AppHandle) -> bool {
    if !DRAGGING.load(Ordering::SeqCst) {
        return false;
    }
    let now = esc_key_down();
    let was = ESC_DOWN.swap(now, Ordering::SeqCst);
    if !esc_cancels_drag(true, was, now) {
        return false;
    }
    let a = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        let _ = drag_cancel(&a);
    }) {
        log::warn!("pill drag event=cancel_dispatch_failed error={e}");
    }
    true
}

/// Is Escape down right now (macOS virtual key 0x35), from the combined session
/// state — a read of the key table, no event tap and no main thread.
#[cfg(target_os = "macos")]
fn esc_key_down() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
    }
    const COMBINED_SESSION_STATE: i32 = 0;
    const KVK_ESCAPE: u16 = 0x35;
    // SAFETY: a pure read of the window server's key state; both arguments are
    // plain integers and the call has no preconditions.
    unsafe { CGEventSourceKeyState(COMBINED_SESSION_STATE, KVK_ESCAPE) }
}

#[cfg(not(target_os = "macos"))]
fn esc_key_down() -> bool {
    false
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn drag_elapsed_ms() -> u64 {
    now_ms().saturating_sub(DRAG_T0_MS.load(Ordering::SeqCst))
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
            //
            // YV95 / OS-12 — the same reasoning, one case wider: during a
            // MEETING the pill is visible for hours, so "visible but nobody has
            // touched it" also waits on the long tick. See `hover_tick_ms`.
            //
            // The wait is SLICED (fix round 3). `hover_tick_ms` still sets the
            // main-thread test rate — 1 Hz through the far-field of a meeting,
            // exactly as OS-12 asks — but the thread no longer commits to being
            // asleep for the whole of it. Between slices it asks the cheap,
            // off-main-thread question "is the cursor on the capsule yet?", and
            // the instant the answer flips to yes it stops waiting and lets the
            // main thread turn click-through off. That is what makes a slow poll
            // safe: a slow poll that also gates the panel's clicks does not
            // delay the first click on the pill, it feeds it to the app
            // underneath.
            let tick = hover_tick_ms(is_shown(), is_hovered(), meeting_recording(), dictating());
            let (sleeps, last_ms) = sleep_slices(tick, PROXIMITY_SLICE_MS);
            for i in 0..sleeps {
                thread::sleep(Duration::from_millis(if i + 1 == sleeps {
                    last_ms
                } else {
                    PROXIMITY_SLICE_MS
                }));
                // WF-02 — Esc mid-drag puts the pill back. Checked every slice
                // (30 ms) and only while a drag is live, so it costs nothing at
                // rest and never waits on a slow meeting tick.
                if poll_drag_escape(&app) {
                    break;
                }
                let near = cursor_on_capsule_cheap();
                let was = CURSOR_NEAR.swap(near, Ordering::Relaxed);
                if should_wake_early(was, near) {
                    break;
                }
            }
            if !is_shown() {
                continue;
            }
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
    // While we are on the main thread anyway, refresh the screen rect the
    // background proximity check reads. Window geometry is only legal to ask
    // for from here, and this costs nothing extra: the hop already happened.
    refresh_capsule_screen_rect(&w);
    // Never hand the panel back to click-through mid-gesture: a fast drag
    // outruns the cursor test and the mouse-up would land on the floor.
    let want = DRAGGING.load(Ordering::SeqCst)
        || (w.is_visible().unwrap_or(false) && cursor_over_capsule(app, &w));
    if INTERACTIVE.swap(want, Ordering::SeqCst) != want {
        let _ = w.set_ignore_cursor_events(!want);
        // Y5-E — once the panel is click-through again the webview gets no
        // more pointer events, so a hover-expanded pill would never learn the
        // cursor left. Tell it; its hover reducer starts the collapse debounce.
        if !want {
            if let Err(e) = app.emit(PILL_HOVER_OUT_EVENT, ()) {
                log::warn!("emit {PILL_HOVER_OUT_EVENT}: {e}");
            }
        }
    }
}

/// Y5-E — the Tauri event `update_interactive` emits when it hands the panel
/// back to click-through (the cursor left the published hitbox). The pill's
/// hover reducer listens (desktop/src/pill/hitbox.ts `PILL_HOVER_OUT_EVENT`).
pub const PILL_HOVER_OUT_EVENT: &str = "pill-hover-out";

/// Recompute and publish the capsule's AppKit screen rect. Main thread only —
/// `outer_position`/`primary_monitor` are window-server queries.
fn refresh_capsule_screen_rect(w: &tauri::WebviewWindow) {
    let rect = (|| {
        let origin = w.outer_position().ok()?;
        let mon = w.primary_monitor().ok().flatten()?;
        let scale = mon.scale_factor();
        if scale <= 0.0 {
            return None;
        }
        let primary_height_pts = mon.size().height as f64 / scale;
        capsule_screen_rect(
            (origin.x as f64, origin.y as f64),
            scale,
            primary_height_pts,
            (
                HIT_X.load(Ordering::SeqCst) as f64,
                HIT_Y.load(Ordering::SeqCst) as f64,
                HIT_W.load(Ordering::SeqCst) as f64,
                HIT_H.load(Ordering::SeqCst) as f64,
            ),
        )
    })();
    set_capsule_screen_rect(rect);
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
    /// yap-fx-06 — a dictation ending inside a meeting keeps the pill (and so
    /// the meeting's Stop and the Esc toast) on screen, even with the idle
    /// pill switched off; with no meeting, the caller's answer stands.
    #[test]
    fn a_take_ending_during_a_meeting_keeps_the_pill_up() {
        assert!(super::keeps_up_for_live_meeting(false, true));
        assert!(super::keeps_up_for_live_meeting(true, true));
        assert!(super::keeps_up_for_live_meeting(true, false));
        assert!(!super::keeps_up_for_live_meeting(false, false));
    }

    #[test]
    fn copy_card_release_never_hides_a_take_in_flight() {
        // yap-p0-insertion-bridge-fallback: with show_floating_pill off, the
        // card's TTL / × used to hide the pill mid-take.
        assert!(super::pill_visible_after_card(false, true));
        assert!(super::pill_visible_after_card(true, false));
        assert!(super::pill_visible_after_card(true, true));
        assert!(!super::pill_visible_after_card(false, false));
    }

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

#[cfg(test)]
mod wf02_drag_tests {
    use super::*;

    /// A 1440x900-point primary monitor at the global origin.
    const FRAME: (f64, f64, f64, f64) = (0.0, 0.0, 1440.0, 900.0);

    #[test]
    fn esc_restore_never_reads_the_release_point() {
        // pill/drag.ts sends `snap: false` on Esc: wherever the blob was
        // dragged to, the pill goes home to the dock it had.
        for home in [
            PillPosition::Bottom,
            PillPosition::Left,
            PillPosition::Right,
        ] {
            assert_eq!(drag_end_dock(false, (4.0, 450.0), Some(FRAME), home), home);
            assert_eq!(
                drag_end_dock(false, (1436.0, 450.0), Some(FRAME), home),
                home
            );
            assert_eq!(drag_end_dock(false, (720.0, 880.0), None, home), home);
        }
    }

    #[test]
    fn a_snapping_release_lands_on_the_dock_under_the_cursor() {
        let home = PillPosition::Bottom;
        assert_eq!(
            drag_end_dock(true, (30.0, 450.0), Some(FRAME), home),
            PillPosition::Left
        );
        assert_eq!(
            drag_end_dock(true, (1420.0, 450.0), Some(FRAME), home),
            PillPosition::Right
        );
        assert_eq!(
            drag_end_dock(true, (720.0, 880.0), Some(FRAME), PillPosition::Left),
            PillPosition::Bottom
        );
        // No readable monitor: never guess a dock, stay home.
        assert_eq!(
            drag_end_dock(true, (30.0, 450.0), None, PillPosition::Right),
            PillPosition::Right
        );
    }

    #[test]
    fn only_a_fresh_esc_press_mid_drag_cancels() {
        assert!(esc_cancels_drag(true, false, true));
        // Held since before the press, or still held from the last check.
        assert!(!esc_cancels_drag(true, true, true));
        // Released, or no drag at all.
        assert!(!esc_cancels_drag(true, false, false));
        assert!(!esc_cancels_drag(false, false, true));
    }

    #[test]
    fn the_cancel_event_name_matches_the_webview() {
        // pill/drag.ts PILL_DRAG_CANCEL_EVENT
        assert_eq!(PILL_DRAG_CANCEL_EVENT, "pill-drag-cancel");
    }
}

#[cfg(test)]
mod one_pill_frame_tests {
    use super::*;

    const SCREEN: ScreenRect = ScreenRect {
        x: 0,
        y: 0,
        w: 2880,
        h: 1800,
        scale: 2.0,
    };

    #[test]
    fn legacy_frame_parks_exactly_where_the_fixed_panel_did() {
        for pos in [
            PillPosition::Bottom,
            PillPosition::Left,
            PillPosition::Right,
        ] {
            let old =
                pill_origin_on_visible_screen(&[SCREEN], Some(SCREEN), (PILL_W, PILL_H), 14.0, pos);
            let new = pill_origin_for_frame(&[SCREEN], Some(SCREEN), PanelFrame::LEGACY, 14.0, pos);
            assert_eq!(old, new, "{pos:?}");
        }
    }

    #[test]
    fn a_grown_frame_keeps_the_character_where_he_was() {
        // The clock slot widens the frame to the right; the character's centre
        // (the anchor) stays on the screen's centre line at the bottom dock.
        let narrow = PanelFrame {
            w: 212.0,
            h: 140.0,
            anchor_x: 106.0,
            anchor_y: 110.0,
        };
        let wide = PanelFrame {
            w: 352.0,
            h: 140.0,
            anchor_x: 106.0,
            anchor_y: 110.0,
        };
        let a = pill_origin_for_frame(&[SCREEN], Some(SCREEN), narrow, 14.0, PillPosition::Bottom)
            .unwrap();
        let b = pill_origin_for_frame(&[SCREEN], Some(SCREEN), wide, 14.0, PillPosition::Bottom)
            .unwrap();
        assert_eq!(a.0 + 212, b.0 + 212, "the anchor's screen x must not move");
        assert_eq!(a.0 + (106.0 * 2.0) as i32, SCREEN.w / 2);
        // A side dock: flush to the edge, the character centred vertically.
        let l =
            pill_origin_for_frame(&[SCREEN], Some(SCREEN), wide, 14.0, PillPosition::Left).unwrap();
        assert_eq!(l.0, 0);
        assert_eq!(l.1 + (110.0 * 2.0) as i32, SCREEN.h / 2);
        let r = pill_origin_for_frame(&[SCREEN], Some(SCREEN), wide, 14.0, PillPosition::Right)
            .unwrap();
        assert_eq!(r.0 + (352.0 * 2.0) as i32, SCREEN.w);
    }

    #[test]
    fn a_frame_request_is_clamped_never_trusted() {
        assert_eq!(PanelFrame::sanitize(f64::NAN, 10.0, 0.0, 0.0), None);
        assert_eq!(PanelFrame::sanitize(10.0, f64::INFINITY, 0.0, 0.0), None);
        let tiny = PanelFrame::sanitize(1.0, 1.0, 50.0, 50.0).unwrap();
        assert_eq!((tiny.w, tiny.h), PANEL_MIN_PT);
        assert!(tiny.anchor_x <= tiny.w && tiny.anchor_y <= tiny.h);
        let huge = PanelFrame::sanitize(5000.0, 5000.0, -3.0, 9000.0).unwrap();
        assert_eq!((huge.w, huge.h), PANEL_MAX_PT);
        assert_eq!(huge.anchor_x, 0.0);
        assert_eq!(huge.anchor_y, PANEL_MAX_PT.1);
        let ok = PanelFrame::sanitize(290.4, 139.6, 100.0, 90.0).unwrap();
        assert_eq!((ok.w, ok.h), (290.0, 140.0));
    }

    #[test]
    fn the_frame_never_leaves_the_screen() {
        let f = PanelFrame {
            w: 400.0,
            h: 200.0,
            anchor_x: 390.0,
            anchor_y: 5.0,
        };
        for pos in [
            PillPosition::Bottom,
            PillPosition::Left,
            PillPosition::Right,
        ] {
            let (x, y) = pill_origin_for_frame(&[SCREEN], Some(SCREEN), f, 14.0, pos).unwrap();
            assert!(x >= 0 && y >= 0, "{pos:?}");
            assert!(x + 800 <= SCREEN.w && y + 400 <= SCREEN.h, "{pos:?}");
        }
    }
}
