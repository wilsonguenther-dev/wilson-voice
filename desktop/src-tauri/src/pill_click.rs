//! yap-wf-01-click-the-pill-to — WF-01: click the pill to dictate.
//!
//! A click on the resting pill starts a hands-free take; a click while it is
//! listening stops it and the text pastes into the app that had focus BEFORE
//! the click. The float panel is non-activating, so in the normal case the
//! user's app never loses frontmost and nothing has to be done. This module is
//! the guard for the case where it did (Yap's main window was raised during the
//! take, or the panel took key): at every press on the pill the frontmost app
//! that is NOT Yap is latched, and the stop click hands focus back to it before
//! the stop edge reads the paste target.
//!
//! The decision is pure (`PreClickLatch`, `paste_target`) so the "paste target
//! is the pre-click app" rule is a unit test; only `frontmost` and `refocus`
//! touch AppKit.

/// How long a latched press stays authoritative. The click that consumes it
/// lands one pointer-up (< 300 ms, see `CLICK_MAX_MS` in pill/drag.ts) plus an
/// IPC hop after the press, so anything older is not this click's press.
pub const LATCH_FRESH_MS: u64 = 2_000;

/// How long the stop click waits for the refocused app to become frontmost
/// before it reads the paste target anyway.
pub const REFOCUS_WAIT_MS: u64 = 250;

/// The app that was frontmost, as AppKit names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontApp {
    pub pid: i32,
    pub bundle_id: Option<String>,
}

impl FrontApp {
    /// True when this is Yap itself (its own bundle id, or its own process).
    pub fn is_self(&self, own_bundle: &str, own_pid: i32) -> bool {
        self.pid == own_pid || self.bundle_id.as_deref() == Some(own_bundle)
    }
}

/// The last non-Yap app that was frontmost when the pill was pressed.
#[derive(Debug, Default, Clone)]
pub struct PreClickLatch {
    app: Option<FrontApp>,
    at_ms: u64,
}

impl PreClickLatch {
    pub const fn new() -> Self {
        Self {
            app: None,
            at_ms: 0,
        }
    }

    /// Record a press. A press while Yap itself is frontmost keeps the earlier
    /// latch (the app the user was in before Yap came forward), but refreshes
    /// its time so the click that follows can still use it.
    pub fn press(&mut self, front: Option<FrontApp>, own_bundle: &str, own_pid: i32, now_ms: u64) {
        match front {
            Some(f) if !f.is_self(own_bundle, own_pid) => {
                self.app = Some(f);
                self.at_ms = now_ms;
            }
            _ => {
                if self.app.is_some() {
                    self.at_ms = now_ms;
                }
            }
        }
    }

    /// The pre-click app for a click at `now_ms`, if the press is fresh.
    pub fn pre_click(&self, now_ms: u64) -> Option<&FrontApp> {
        self.app
            .as_ref()
            .filter(|_| now_ms.saturating_sub(self.at_ms) <= LATCH_FRESH_MS)
    }
}

/// THE latch: written at every press on the pill (`pill_drag_start`), read
/// by the click that follows it (`pill_click_toggle`).
pub static PRESS: parking_lot::Mutex<PreClickLatch> = parking_lot::Mutex::new(PreClickLatch::new());

/// What the stop click must do before the stop edge reads the paste target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteTarget {
    /// The frontmost app already is the pre-click app (the normal case: the
    /// panel is non-activating), or there is no pre-click app to return to.
    Keep,
    /// Hand focus back to this app first: it had focus before the click.
    Refocus(FrontApp),
}

/// The paste target is the PRE-CLICK app. If something else is frontmost now
/// (Yap itself, or nothing), focus goes back to the pre-click app first.
pub fn paste_target(pre_click: Option<&FrontApp>, front_now: Option<&FrontApp>) -> PasteTarget {
    match (pre_click, front_now) {
        (None, _) => PasteTarget::Keep,
        (Some(pre), Some(now)) if now.pid == pre.pid => PasteTarget::Keep,
        (Some(pre), _) => PasteTarget::Refocus(pre.clone()),
    }
}

/// The current frontmost app (NSWorkspace; needs no Accessibility grant).
pub fn frontmost() -> Option<FrontApp> {
    #[cfg(target_os = "macos")]
    {
        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        let pid = app.processIdentifier();
        let bundle_id = app
            .bundleIdentifier()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        Some(FrontApp { pid, bundle_id })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Ask AppKit to bring `target` forward. `false` when the app is gone or
/// refused the request.
pub fn refocus(target: &FrontApp) -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(target.pid)
        else {
            return false;
        };
        app.activateWithOptions(NSApplicationActivationOptions::empty())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = target;
        false
    }
}

/// Wait (bounded by `REFOCUS_WAIT_MS`) until `target` is frontmost. Returns
/// whether it got there. Never called on the main thread: activation lands
/// through the main run loop.
pub fn wait_until_front(target: &FrontApp) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(REFOCUS_WAIT_MS);
    loop {
        if frontmost().is_some_and(|f| f.pid == target.pid) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAP: &str = "com.wilsonguenther.yap.test";
    const YAP_PID: i32 = 100;

    fn app(pid: i32, id: &str) -> FrontApp {
        FrontApp {
            pid,
            bundle_id: Some(id.to_string()),
        }
    }

    #[test]
    fn paste_target_is_the_pre_click_app() {
        let mut latch = PreClickLatch::default();
        let notes = app(200, "com.apple.Notes");
        latch.press(Some(notes.clone()), YAP, YAP_PID, 1_000);
        // Yap came forward during the take; the stop click returns focus to Notes.
        let yap = app(YAP_PID, YAP);
        assert_eq!(
            paste_target(latch.pre_click(1_150), Some(&yap)),
            PasteTarget::Refocus(notes.clone())
        );
        // Nothing frontmost at all: still Notes.
        assert_eq!(
            paste_target(latch.pre_click(1_150), None),
            PasteTarget::Refocus(notes.clone())
        );
        // The normal case: the non-activating panel left Notes frontmost.
        assert_eq!(
            paste_target(latch.pre_click(1_150), Some(&notes)),
            PasteTarget::Keep
        );
    }

    #[test]
    fn a_press_while_yap_is_frontmost_keeps_the_pre_click_app() {
        let mut latch = PreClickLatch::default();
        let mail = app(300, "com.apple.mail");
        latch.press(Some(mail.clone()), YAP, YAP_PID, 1_000);
        // The stop press lands while Yap's own window is frontmost — by bundle
        // id, and by pid with no bundle id at all.
        latch.press(Some(app(YAP_PID, YAP)), YAP, YAP_PID, 9_000);
        latch.press(
            Some(FrontApp {
                pid: YAP_PID,
                bundle_id: None,
            }),
            YAP,
            YAP_PID,
            9_010,
        );
        assert_eq!(latch.pre_click(9_100), Some(&mail));
        assert_eq!(
            paste_target(latch.pre_click(9_100), Some(&app(YAP_PID, YAP))),
            PasteTarget::Refocus(mail)
        );
    }

    #[test]
    fn switching_apps_mid_take_retargets_to_the_app_before_the_stop_click() {
        let mut latch = PreClickLatch::default();
        latch.press(Some(app(200, "com.apple.Notes")), YAP, YAP_PID, 1_000);
        let mail = app(300, "com.apple.mail");
        latch.press(Some(mail.clone()), YAP, YAP_PID, 5_000);
        assert_eq!(latch.pre_click(5_100), Some(&mail));
    }

    #[test]
    fn a_stale_press_is_not_this_clicks_press() {
        let mut latch = PreClickLatch::default();
        latch.press(Some(app(200, "com.apple.Notes")), YAP, YAP_PID, 1_000);
        assert!(latch.pre_click(1_000 + LATCH_FRESH_MS + 1).is_none());
        assert_eq!(
            paste_target(
                latch.pre_click(1_000 + LATCH_FRESH_MS + 1),
                Some(&app(YAP_PID, YAP))
            ),
            PasteTarget::Keep
        );
    }

    #[test]
    fn no_press_ever_latched_keeps_whatever_is_front() {
        let latch = PreClickLatch::default();
        assert_eq!(
            paste_target(latch.pre_click(10), Some(&app(YAP_PID, YAP))),
            PasteTarget::Keep
        );
        // A press with only Yap frontmost latches nothing.
        let mut latch = PreClickLatch::default();
        latch.press(Some(app(YAP_PID, YAP)), YAP, YAP_PID, 5);
        assert!(latch.pre_click(6).is_none());
    }
}
