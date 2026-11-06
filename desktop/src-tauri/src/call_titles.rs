//! yap-call-2 — browser calls in Chromium-family browsers (Chrome, Arc, Edge,
//! Brave): Meet, Zoom web, Teams web, Slack huddles, Discord, Whereby, Webex,
//! Around — told apart by the browser window's title.
//!
//! What is read, and when (plan §2.2(3)):
//! - ONLY a Chromium-family browser that the snapshot already attributes a
//!   running microphone input to ([`title_candidates`]). A browser that is
//!   not holding the mic is never read.
//! - ONLY `AXWindows` → `AXTitle` of that app, via
//!   `AXUIElementCreateApplication(pid)` and focus.rs's `attr_string`. Titles
//!   only: no page address, no web-area traversal, no Automation permission,
//!   no window-server listing, and so no Screen Recording.
//! - Once per snapshot per candidate; this module owns no timer (the cadence
//!   is call_signals.rs / meeting_detect.rs).
//! - A title is matched in memory against call_catalog.json and DROPPED. What
//!   leaves this module is `{ pattern_id, len }` ([`TitleObs`]); the log line
//!   carries the pattern id and the length, never the text ([`log_line`]).
//!
//! Permission truth (plan §3): when Accessibility is NOT granted,
//! `browser_titles` is `None`, the offer reads "Browser call (site unknown)"
//! and fires only with a calendar match. Yap never prompts for Accessibility
//! from here; Settings > Meetings links the existing permissions flow.
//!
//! The honest limit: a Chromium window's title is its FRONT tab. A call in a
//! background tab produces no title match, so no offer — the safe failure.
//!
//! Safari (`com.apple.WebKit.GPU`) and Firefox are deferred
//! (docs/loop/DEFERRED.md): a WebKit.GPU input never produces an offer.

use crate::call_signals::{
    bundle_input_running, observe_title, AppKind, CatalogApp, CallCatalog, CallSnapshot, TitleObs,
};

/// What the offer calls a browser call whose site Yap cannot read.
pub const SITE_UNKNOWN_NAME: &str = "Browser call (site unknown)";
/// The offer headline for that call.
pub const SITE_UNKNOWN_COPY: &str = "Browser call (site unknown) — record it?";
/// Settings > Meetings, when Accessibility is off.
pub const AX_OFF_LINE: &str = "Turn on Accessibility so Yap can tell which site the call is on";
/// Settings > Meetings, always: the front-tab limit.
pub const FRONT_TAB_LINE: &str = "Yap reads the front tab's title; a call in a background tab is not offered";

/// A browser row whose window titles Yap may read: Chromium family only.
/// Safari (`helper_needs_title`) is deferred and never read here.
pub fn is_chromium_browser(app: &CatalogApp) -> bool {
    app.kind == AppKind::Browser && !app.helper_needs_title
}

/// The browsers to read titles from: running Chromium-family rows that a
/// process with `running_input == Some(true)` is attributed to (Yap
/// excluded). `(row, app pid)`, at most one entry per row.
pub fn title_candidates<'a>(
    snap: &CallSnapshot,
    catalog: &'a CallCatalog,
) -> Vec<(&'a CatalogApp, i32)> {
    let mut out: Vec<(&'a CatalogApp, i32)> = Vec::new();
    for app in catalog.apps.iter().filter(|a| is_chromium_browser(a)) {
        let Some(running) = snap
            .running_apps
            .iter()
            .find(|r| r.bundle_id.eq_ignore_ascii_case(&app.bundle_id))
        else {
            continue;
        };
        if bundle_input_running(snap, catalog, &app.bundle_id) {
            out.push((app, running.pid));
        }
    }
    out
}

/// The ONE log line for an observed title: pattern id and length only.
pub fn log_line(obs: &TitleObs) -> String {
    format!(
        "call_titles observed bundle={} pattern={} len={}",
        obs.bundle_id,
        obs.pattern_id.as_deref().unwrap_or("none"),
        obs.len
    )
}

/// Read and reduce the front-tab titles of every candidate.
///
/// `ax_trusted == false` → `None` (AX unknown: "site unknown", calendar-only
/// offer). `read(pid)` returns that app's window titles (each window's front
/// tab); every title is reduced by [`observe_title`] and dropped. `log`
/// receives one [`log_line`] per window plus one summary line — the text is
/// never passed to it. `raw` is the developer `--raw-titles` flag only.
pub fn read_titles<R, L>(
    snap: &CallSnapshot,
    catalog: &CallCatalog,
    ax_trusted: bool,
    raw: bool,
    mut read: R,
    mut log: L,
) -> Option<Vec<TitleObs>>
where
    R: FnMut(i32) -> Vec<String>,
    L: FnMut(String),
{
    let candidates = title_candidates(snap, catalog);
    if !ax_trusted {
        if !candidates.is_empty() {
            log(format!(
                "call_titles ax_off candidates={} site=unknown",
                candidates.len()
            ));
        }
        return None;
    }
    let started = std::time::Instant::now();
    let mut out: Vec<TitleObs> = Vec::new();
    for (app, pid) in candidates.iter().copied() {
        for title in read(pid) {
            let obs = observe_title(app, &title, raw);
            log(log_line(&obs));
            out.push(obs);
        }
    }
    if !candidates.is_empty() {
        log(format!(
            "call_titles read candidates={} windows={} matched={} ms={}",
            candidates.len(),
            out.len(),
            out.iter().filter(|o| o.pattern_id.is_some()).count(),
            started.elapsed().as_millis()
        ));
    }
    Some(out)
}

/// The macOS reader: `AXWindows` → `AXTitle`, read-only, short timeout.
#[cfg(target_os = "macos")]
pub mod sys {
    use crate::focus::ax::{attr_string, AXUIElementRef};
    use core_foundation::array::CFArray;
    use core_foundation::base::{CFRelease, CFType, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
        fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: f32) -> i32;
    }

    /// Every window title (each window's front tab) of the app at `pid`.
    /// Read, matched by the caller, and dropped.
    pub fn window_titles(pid: i32) -> Vec<String> {
        let mut out = Vec::new();
        // SAFETY: AXUIElementCreateApplication returns +1 (or null).
        let app = unsafe { AXUIElementCreateApplication(pid) };
        if app.is_null() {
            return out;
        }
        // SAFETY: a live element; a short timeout so a hung browser never
        // stalls the watcher.
        unsafe { AXUIElementSetMessagingTimeout(app, 0.25) };
        let key = CFString::new("AXWindows");
        let mut val: CFTypeRef = std::ptr::null_mut();
        // SAFETY: a live AX element and a CFString key; +1 on success.
        let err = unsafe { AXUIElementCopyAttributeValue(app, key.as_concrete_TypeRef(), &mut val) };
        if err == 0 && !val.is_null() {
            // SAFETY: AXWindows is a CFArray of AXUIElement; create rule.
            let windows: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(val as _) };
            for w in windows.iter() {
                if let Some(title) = attr_string(w.as_CFTypeRef() as AXUIElementRef, "AXTitle") {
                    out.push(title);
                }
            }
        } else {
            log::debug!("call_titles windows_unreadable err={err}");
        }
        // SAFETY: balances the create above.
        unsafe { CFRelease(app as CFTypeRef) };
        out
    }
}
