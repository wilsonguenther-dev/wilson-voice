//! FB-03 — the pill shows over every app.
//!
//! Wilson, 2026-11-06: "the pill doesn't show on some certain apps". Root cause
//! found while reproducing: `ensure_float` called Tauri's
//! `set_always_on_top(true)` AFTER the NSPanel conversion, and on macOS that is
//! tao's `setLevel:NSFloatingWindowLevel` (3), dispatched async. So every show
//! quietly dropped the pill from Status (25) to the same level as every other
//! app's floating window (Chrome picture-in-picture, Slack huddles, Zoom's video
//! strip, Keynote's presenter windows) and it lost the tie. The space keeper only
//! re-ordered the panel front; it never put the level back.
//!
//! This module holds the pure half of the fix, so it is testable anywhere:
//!   * [`level_for`] — the one level rule: a base above every ordinary app
//!     window, raised just above the frontmost app's highest on-screen window
//!     when that app climbs higher (presenting, full-screen share bars), capped
//!     below the assistive-technology level so VoiceOver / Zoom stay on top.
//!   * [`read_front`] — what the frontmost app looks like in the window list
//!     (highest layer, full screen or not), from public CGWindowList keys only.
//!     Owner PID, layer, bounds and alpha need no Screen Recording grant; window
//!     titles do, and are never read.
//!   * [`presence_gap`] + [`GapLatch`] — the "reproduce first" log: when the pill
//!     is meant to be up but is ordered out, off the active Space, missing from
//!     the on-screen window list, covered, or below its level, one line on the
//!     edge, one line when it comes back with how long it was gone.
//!
//! The intentional hides are untouched: WF-16 (hidden from screen shares while a
//! meeting records) is `NSWindow.sharingType`, which never moves the pill on the
//! user's own screen, and every real hide goes through `hide_float`, which drops
//! `PILL_SHOWN` first so the keeper never fights it.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

/// NSPopUpMenuWindowLevel. Above normal, floating, modal, utility, Dock, main
/// menu and status windows, i.e. above every window an app puts up in normal use.
pub const BASE_LEVEL: i64 = 101;
/// Just under kCGAssistiveTechHighWindowLevel (1500): the pill may climb over a
/// presenting app, never over the Mac's own accessibility overlays.
pub const CAP_LEVEL: i64 = 1499;

/// The level the panel should hold right now ([`level_for`] of the last reading).
static WANTED_LEVEL: AtomicI64 = AtomicI64::new(BASE_LEVEL);

pub fn wanted_level() -> i64 {
    WANTED_LEVEL.load(Ordering::SeqCst)
}

pub fn set_wanted_level(level: i64) {
    WANTED_LEVEL.store(level.clamp(BASE_LEVEL, CAP_LEVEL), Ordering::SeqCst);
}

/// A rect in CoreGraphics global points (top-left origin), the space both
/// `kCGWindowBounds` and `CGDisplayBounds` use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// One on-screen window from CGWindowList (no title, ever).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WinInfo {
    pub pid: i32,
    pub number: i64,
    pub layer: i64,
    pub alpha: f64,
    pub bounds: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FrontReading {
    /// Highest layer of a visible window the frontmost app owns.
    pub max_layer: Option<i64>,
    /// The frontmost app owns a layer-0 window covering a whole display.
    pub full_screen: bool,
}

fn covers(win: &Rect, display: &Rect) -> bool {
    const EPS: f64 = 1.0;
    (win.x - display.x).abs() <= EPS
        && (win.y - display.y).abs() <= EPS
        && (win.w - display.w).abs() <= EPS
        && (win.h - display.h).abs() <= EPS
}

/// Read the frontmost app out of an on-screen window list. Yap's own windows
/// never count (its main window can be frontmost; the pill must not chase itself),
/// and nor do fully transparent or empty windows (click catchers, share borders).
pub fn read_front(
    windows: &[WinInfo],
    front_pid: i32,
    own_pid: i32,
    displays: &[Rect],
) -> FrontReading {
    if front_pid <= 0 || front_pid == own_pid {
        return FrontReading::default();
    }
    let mut out = FrontReading::default();
    for w in windows
        .iter()
        .filter(|w| w.pid == front_pid && w.alpha > 0.0 && w.bounds.w > 0.0 && w.bounds.h > 0.0)
    {
        out.max_layer = Some(out.max_layer.map_or(w.layer, |m| m.max(w.layer)));
        if w.layer == 0 && displays.iter().any(|d| covers(&w.bounds, d)) {
            out.full_screen = true;
        }
    }
    out
}

/// The one level rule. Below the base nothing changes; a frontmost app that
/// owns a window at or above the base gets the pill one level higher, capped.
pub fn level_for(front_max_layer: Option<i64>) -> i64 {
    match front_max_layer {
        Some(layer) if layer >= BASE_LEVEL => (layer + 1).min(CAP_LEVEL),
        _ => BASE_LEVEL,
    }
}

/// Is the panel's window number in the on-screen window list? The same
/// question the headless check (`scripts/pill-over-apps.sh`) asks from outside.
pub fn panel_listed(windows: &[WinInfo], own_number: i64) -> bool {
    own_number > 0 && windows.iter().any(|w| w.number == own_number)
}

/// Why a pill that is meant to be up is not on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gap {
    OrderedOut,
    OffActiveSpace,
    NotInWindowList,
    LevelDropped,
    Covered,
}

impl Gap {
    pub fn as_str(self) -> &'static str {
        match self {
            Gap::OrderedOut => "ordered_out",
            Gap::OffActiveSpace => "off_active_space",
            Gap::NotInWindowList => "not_in_window_list",
            Gap::LevelDropped => "level_dropped",
            Gap::Covered => "covered",
        }
    }
}

/// What the keeper read off the panel on one tick (main thread).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelState {
    pub visible: bool,
    pub on_active_space: bool,
    pub listed: bool,
    /// `occlusionState` has the Visible bit.
    pub unoccluded: bool,
    pub level: i64,
    pub wanted_level: i64,
}

/// First failing check wins, in the order a human would look: is it ordered
/// in, is it on this Space, does the window server list it, is it at its
/// level, can any of it be seen.
pub fn presence_gap(s: PanelState) -> Option<Gap> {
    if !s.visible {
        Some(Gap::OrderedOut)
    } else if !s.on_active_space {
        Some(Gap::OffActiveSpace)
    } else if !s.listed {
        Some(Gap::NotInWindowList)
    } else if s.level < s.wanted_level {
        Some(Gap::LevelDropped)
    } else if !s.unoccluded {
        Some(Gap::Covered)
    } else {
        None
    }
}

/// An edge of the presence log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapEdge {
    Lost(Gap),
    Back { gap: Gap, after_ms: u64 },
}

/// Edge latch so a pill that stays covered for an hour is one line, not 2,400.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GapLatch {
    current: Option<Gap>,
    since_ms: u64,
}

impl GapLatch {
    pub const fn new() -> Self {
        GapLatch {
            current: None,
            since_ms: 0,
        }
    }

    pub fn step(&mut self, gap: Option<Gap>, now_ms: u64) -> Option<GapEdge> {
        match (self.current, gap) {
            (None, Some(g)) => {
                self.current = Some(g);
                self.since_ms = now_ms;
                Some(GapEdge::Lost(g))
            }
            (Some(was), None) => {
                self.current = None;
                Some(GapEdge::Back {
                    gap: was,
                    after_ms: now_ms.saturating_sub(self.since_ms),
                })
            }
            // Still gone (whatever the reason now) or still fine: no new line.
            _ => None,
        }
    }

    /// The pill was hidden on purpose: forget any open gap without a line.
    pub fn reset(&mut self) {
        self.current = None;
    }
}

pub static LATCH: Mutex<GapLatch> = Mutex::new(GapLatch::new());

// ─── macOS readers (CGWindowList / CGDisplay; public API, no permission) ────

#[cfg(target_os = "macos")]
mod mac {
    use super::{Rect, WinInfo};
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    impl From<CGRect> for Rect {
        fn from(r: CGRect) -> Self {
            Rect {
                x: r.origin.x,
                y: r.origin.y,
                w: r.size.width,
                h: r.size.height,
            }
        }
    }

    const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0;
    const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;
    const K_CG_NULL_WINDOW_ID: u32 = 0;
    const K_CF_NUMBER_SINT64_TYPE: isize = 4;
    const K_CF_NUMBER_FLOAT64_TYPE: isize = 6;

    #[allow(non_upper_case_globals)]
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *const c_void;
        fn CGRectMakeWithDictionaryRepresentation(dict: *const c_void, rect: *mut CGRect) -> bool;
        fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
        fn CGDisplayBounds(display: u32) -> CGRect;
        static kCGWindowOwnerPID: *const c_void;
        static kCGWindowNumber: *const c_void;
        static kCGWindowLayer: *const c_void;
        static kCGWindowAlpha: *const c_void;
        static kCGWindowBounds: *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, idx: isize) -> *const c_void;
        fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
        fn CFNumberGetValue(number: *const c_void, the_type: isize, out: *mut c_void) -> bool;
        fn CFRelease(cf: *mut c_void);
    }

    /// SAFETY (callers): `dict` is a live CFDictionary from the window list.
    unsafe fn num_i64(dict: *const c_void, key: *const c_void) -> Option<i64> {
        let v = CFDictionaryGetValue(dict, key);
        if v.is_null() {
            return None;
        }
        let mut out: i64 = 0;
        let ok = CFNumberGetValue(
            v,
            K_CF_NUMBER_SINT64_TYPE,
            &mut out as *mut i64 as *mut c_void,
        );
        if ok {
            Some(out)
        } else {
            None
        }
    }

    unsafe fn num_f64(dict: *const c_void, key: *const c_void) -> Option<f64> {
        let v = CFDictionaryGetValue(dict, key);
        if v.is_null() {
            return None;
        }
        let mut out: f64 = 0.0;
        let ok = CFNumberGetValue(
            v,
            K_CF_NUMBER_FLOAT64_TYPE,
            &mut out as *mut f64 as *mut c_void,
        );
        if ok {
            Some(out)
        } else {
            None
        }
    }

    /// Every on-screen window, front to back. Empty on any failure.
    pub fn window_list() -> Vec<WinInfo> {
        let mut out = Vec::new();
        // SAFETY: the Copy-rule array is released below exactly once; every
        // dictionary and number read is borrowed from it while it is alive, and
        // the key statics are CoreGraphics constants.
        unsafe {
            let arr = CGWindowListCopyWindowInfo(
                K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
                K_CG_NULL_WINDOW_ID,
            );
            if arr.is_null() {
                return out;
            }
            let n = CFArrayGetCount(arr);
            out.reserve(n.max(0) as usize);
            for i in 0..n {
                let d = CFArrayGetValueAtIndex(arr, i);
                if d.is_null() {
                    continue;
                }
                let mut rect = CGRect::default();
                let bounds_dict = CFDictionaryGetValue(d, kCGWindowBounds);
                if !bounds_dict.is_null() {
                    let _ = CGRectMakeWithDictionaryRepresentation(bounds_dict, &mut rect);
                }
                out.push(WinInfo {
                    pid: num_i64(d, kCGWindowOwnerPID).unwrap_or(0) as i32,
                    number: num_i64(d, kCGWindowNumber).unwrap_or(0),
                    layer: num_i64(d, kCGWindowLayer).unwrap_or(0),
                    alpha: num_f64(d, kCGWindowAlpha).unwrap_or(1.0),
                    bounds: rect.into(),
                });
            }
            CFRelease(arr as *mut c_void);
        }
        out
    }

    /// Every active display's bounds in global points.
    pub fn display_bounds() -> Vec<Rect> {
        let mut ids = [0u32; 16];
        let mut count: u32 = 0;
        // SAFETY: the buffer holds `ids.len()` entries and the call writes at
        // most that many; `count` is a plain out-parameter.
        let err = unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
        if err != 0 {
            return Vec::new();
        }
        ids.iter()
            .take(count as usize)
            // SAFETY: a display id the window server just listed.
            .map(|&id| unsafe { CGDisplayBounds(id) }.into())
            .collect()
    }
}

#[cfg(target_os = "macos")]
pub use mac::{display_bounds, window_list};

#[cfg(not(target_os = "macos"))]
pub fn window_list() -> Vec<WinInfo> {
    Vec::new()
}

#[cfg(not(target_os = "macos"))]
pub fn display_bounds() -> Vec<Rect> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1512.0,
        h: 982.0,
    };
    const ME: i32 = 100;
    const FRONT: i32 = 200;

    fn win(pid: i32, number: i64, layer: i64, bounds: Rect) -> WinInfo {
        WinInfo {
            pid,
            number,
            layer,
            alpha: 1.0,
            bounds,
        }
    }

    fn small() -> Rect {
        Rect {
            x: 100.0,
            y: 100.0,
            w: 800.0,
            h: 600.0,
        }
    }

    #[test]
    fn the_base_level_sits_above_every_ordinary_app_window() {
        // Floating (3), Utility (19), Dock (20), MainMenu (24), Status (25):
        // what Tauri's always_on_top and most apps' "on top" windows use.
        for layer in [0, 3, 8, 19, 20, 24, 25, 100] {
            assert!(level_for(Some(layer)) > layer, "layer {layer}");
            assert_eq!(level_for(Some(layer)), BASE_LEVEL);
        }
        assert_eq!(level_for(None), BASE_LEVEL);
        // The regression itself: the old code landed the pill at Floating (3).
        assert!(BASE_LEVEL > 3);
    }

    #[test]
    fn a_presenting_app_above_the_base_gets_the_pill_one_level_higher() {
        assert_eq!(level_for(Some(BASE_LEVEL)), BASE_LEVEL + 1);
        assert_eq!(level_for(Some(1000)), 1001); // screen-saver-level presenters
    }

    #[test]
    fn the_level_never_climbs_over_the_accessibility_overlays() {
        assert_eq!(level_for(Some(1499)), CAP_LEVEL);
        assert_eq!(level_for(Some(2_147_483_628)), CAP_LEVEL); // shielding level
        assert!(CAP_LEVEL < 1500);
        set_wanted_level(i64::MAX);
        assert_eq!(wanted_level(), CAP_LEVEL);
        set_wanted_level(0);
        assert_eq!(wanted_level(), BASE_LEVEL);
    }

    #[test]
    fn the_front_app_reading_takes_its_highest_visible_window() {
        let windows = [
            win(FRONT, 1, 0, small()),
            win(FRONT, 2, 1000, small()),
            win(300, 3, 2000, small()), // someone else's
        ];
        let r = read_front(&windows, FRONT, ME, &[SCREEN]);
        assert_eq!(r.max_layer, Some(1000));
        assert!(!r.full_screen);
        assert_eq!(level_for(r.max_layer), 1001);
    }

    #[test]
    fn transparent_and_empty_windows_never_raise_the_pill() {
        let mut ghost = win(FRONT, 1, 1200, small());
        ghost.alpha = 0.0;
        let empty = win(
            FRONT,
            2,
            1300,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
        );
        let r = read_front(&[ghost, empty], FRONT, ME, &[SCREEN]);
        assert_eq!(r.max_layer, None);
        assert_eq!(level_for(r.max_layer), BASE_LEVEL);
    }

    #[test]
    fn yap_being_frontmost_never_chases_its_own_windows() {
        let windows = [win(ME, 1, 1400, small())];
        assert_eq!(
            read_front(&windows, ME, ME, &[SCREEN]),
            FrontReading::default()
        );
        assert_eq!(
            read_front(&windows, 0, ME, &[SCREEN]),
            FrontReading::default()
        );
    }

    #[test]
    fn a_layer_zero_window_covering_a_display_reads_as_full_screen() {
        let second = Rect {
            x: 1512.0,
            y: -200.0,
            w: 2560.0,
            h: 1440.0,
        };
        let r = read_front(&[win(FRONT, 1, 0, second)], FRONT, ME, &[SCREEN, second]);
        assert!(r.full_screen);
        // A covering window at a non-zero layer is an overlay, not full screen.
        let r = read_front(&[win(FRONT, 1, 25, SCREEN)], FRONT, ME, &[SCREEN]);
        assert!(!r.full_screen);
        // A big window that misses the display edge is windowed.
        let mut almost = SCREEN;
        almost.h -= 40.0;
        let r = read_front(&[win(FRONT, 1, 0, almost)], FRONT, ME, &[SCREEN]);
        assert!(!r.full_screen);
    }

    #[test]
    fn the_panel_is_found_in_the_window_list_by_number_only() {
        let windows = [win(ME, 77, BASE_LEVEL, small()), win(FRONT, 78, 0, SCREEN)];
        assert!(panel_listed(&windows, 77));
        assert!(!panel_listed(&windows, 79));
        assert!(!panel_listed(&windows, 0)); // not yet assigned a number
    }

    fn up() -> PanelState {
        PanelState {
            visible: true,
            on_active_space: true,
            listed: true,
            unoccluded: true,
            level: BASE_LEVEL,
            wanted_level: BASE_LEVEL,
        }
    }

    #[test]
    fn a_pill_that_is_up_has_no_gap() {
        assert_eq!(presence_gap(up()), None);
        let mut higher = up();
        higher.level = BASE_LEVEL + 5;
        assert_eq!(presence_gap(higher), None);
    }

    #[test]
    fn each_gap_is_named_in_the_order_a_human_checks() {
        let mut s = up();
        s.unoccluded = false;
        assert_eq!(presence_gap(s), Some(Gap::Covered));
        s.level = 3; // the always_on_top regression
        assert_eq!(presence_gap(s), Some(Gap::LevelDropped));
        s.listed = false;
        assert_eq!(presence_gap(s), Some(Gap::NotInWindowList));
        s.on_active_space = false;
        assert_eq!(presence_gap(s), Some(Gap::OffActiveSpace));
        s.visible = false;
        assert_eq!(presence_gap(s), Some(Gap::OrderedOut));
        assert_eq!(Gap::OrderedOut.as_str(), "ordered_out");
        assert_eq!(Gap::LevelDropped.as_str(), "level_dropped");
    }

    #[test]
    fn the_latch_logs_one_line_per_edge_with_the_time_gone() {
        let mut l = GapLatch::new();
        assert_eq!(l.step(None, 0), None);
        assert_eq!(
            l.step(Some(Gap::LevelDropped), 1_000),
            Some(GapEdge::Lost(Gap::LevelDropped))
        );
        // Still gone, even for a different reason: no new line.
        assert_eq!(l.step(Some(Gap::Covered), 2_500), None);
        assert_eq!(l.step(Some(Gap::Covered), 4_000), None);
        assert_eq!(
            l.step(None, 5_500),
            Some(GapEdge::Back {
                gap: Gap::LevelDropped,
                after_ms: 4_500
            })
        );
        assert_eq!(l.step(None, 7_000), None);
    }

    #[test]
    fn an_intentional_hide_closes_the_gap_without_a_back_line() {
        let mut l = GapLatch::new();
        assert!(l.step(Some(Gap::OrderedOut), 10).is_some());
        l.reset();
        assert_eq!(l.step(None, 20), None);
    }
}
