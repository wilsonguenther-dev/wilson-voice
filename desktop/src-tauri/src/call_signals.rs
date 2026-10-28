//! yap-call-0 — the call signal matrix: "is the user on a call, in which app?"
//!
//! The ONE app/browser signal behind the meeting offer (meeting_detect.rs owns
//! the offer and the calendar signal and calls [`score`]; there is no second,
//! UI-pid app signal anywhere).
//!
//! Why per-PROCESS CoreAudio state (plan §2.2, measured on macOS 26.6.2): audio
//! is played and captured by HELPER processes. Chrome's CoreAudio process
//! object is `com.google.Chrome.helper` (parent pid → Google Chrome); Safari's
//! is `com.apple.WebKit.GPU` (parent pid 1, launchd); FaceTime and phone calls
//! run through `com.apple.avconferenced` / callservicesd
//! (`com.apple.TelephonyUtilities`). A UI pid produces no audio for any
//! browser (syscapture.rs, finding OS-5), and the device-level
//! `kAudioDevicePropertyDeviceIsRunningSomewhere` is useless while Yap holds
//! the mic. The industry signal is "an app started using the microphone":
//! `kAudioProcessPropertyIsRunningInput`.
//!
//! Shape: plain serde data ([`CallSnapshot`]) → pure [`score`] → pure
//! [`offer_copy`]. Fixtures in `tests/fixtures/call_snapshots/` are snapshots,
//! so every decision is tabled in `tests/call_signal_matrix.rs` without a Mac.
//! The OS adapter (`sys`) only READS: process objects, NSWorkspace, and — only
//! when Accessibility is already granted — browser window titles, which are
//! matched against the catalog and DROPPED; a snapshot carries
//! `{ pattern_id, len }`, never the text (`--raw-titles` is the one developer
//! exception and nothing in the product passes it).
//!
//! Score table (plan §3 as built; offer at >= [`OFFER_THRESHOLD`]):
//!
//! | evidence (input running, Yap excluded)                 | base |
//! |--------------------------------------------------------|------|
//! | native call app (its bundle, or its helper)            | 80   |
//! | system daemon (avconferenced / callservicesd) + app up | 70   |
//! | browser helper + catalog title match (AX granted)      | 85   |
//! | browser helper, AX unknown (titles `None`)             | 60   |
//! | browser helper, AX granted, no title match             | cap 55 |
//! | Safari `com.apple.WebKit.GPU` without a title match    | none |
//! | output only, or an unreadable flag                     | none |
//!
//! Bonuses: the app is frontmost +10, a calendar event with a call link is in
//! its window +10, the same app led the previous snapshot +5. Max 100; the
//! browser no-title cap applies after bonuses.
//!
//! Cadence (macOS 14.4+, same gate as the tap): a
//! `kAudioHardwarePropertyProcessObjectList` listener wakes the watcher when
//! processes JOIN or LEAVE, and on every list change the watcher re-syncs an
//! `kAudioProcessPropertyIsRunningInput` listener on each process object
//! ([`process_listener_diff`] is the tested seam) — Zoom and the Chrome helper
//! are already HAL clients, so a call starting changes ONLY that flag. Plus the
//! NSWorkspace notifications and the ONE 30 s timer in meeting_detect.rs as the
//! backstop, never faster, and off when "Offer to record" is off
//! ([`timer_enabled`]).

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::OnceLock;

pub use crate::syscapture::ProcAudio;

/// The bundled catalog.
pub const CATALOG_JSON: &str = include_str!("call_catalog.json");

/// Offer at or above this confidence.
pub const OFFER_THRESHOLD: u8 = 60;
/// Browser mic in use, AX granted, and no window title matches a call site.
pub const BROWSER_NO_TITLE_CAP: u8 = 55;
const BASE_NATIVE: u8 = 80;
const BASE_DAEMON: u8 = 70;
const BASE_BROWSER_TITLE: u8 = 85;
const BASE_BROWSER_AX_UNKNOWN: u8 = 60;
const BONUS_FRONTMOST: u8 = 10;
const BONUS_CALENDAR: u8 = 10;
const BONUS_MEMORY: u8 = 5;

// ─── Catalog ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    Native,
    Browser,
    SystemDaemon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitlePattern {
    pub id: String,
    /// What the offer calls the call ("Google Meet").
    pub name: String,
    /// Case-insensitive substrings of a window title.
    pub contains: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogApp {
    pub bundle_id: String,
    pub name: String,
    pub kind: AppKind,
    /// Case-insensitive bundle-id prefixes of the process carrying the audio.
    #[serde(default)]
    pub helpers: Vec<String>,
    /// Browsers only.
    #[serde(default)]
    pub title_patterns: Vec<TitlePattern>,
    /// Safari: `com.apple.WebKit.GPU` has ppid 1, so it is attributable ONLY
    /// through a window-title match.
    #[serde(default)]
    pub helper_needs_title: bool,
    /// Flipped only by a Wilson walk with a recorded, non-synthetic fixture.
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallCatalog {
    pub version: u32,
    pub apps: Vec<CatalogApp>,
}

impl CallCatalog {
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("call_catalog.json: {e}"))
    }

    pub fn app(&self, bundle_id: &str) -> Option<&CatalogApp> {
        self.apps
            .iter()
            .find(|a| a.bundle_id.eq_ignore_ascii_case(bundle_id))
    }

    /// The row whose helper patterns match `bundle_id`.
    pub fn by_helper(&self, bundle_id: &str) -> Option<&CatalogApp> {
        let b = bundle_id.to_ascii_lowercase();
        self.apps.iter().find(|a| {
            a.helpers
                .iter()
                .any(|h| b.starts_with(&h.to_ascii_lowercase()))
        })
    }

    /// Match a window title of browser `app` against its patterns.
    pub fn match_title<'a>(app: &'a CatalogApp, title: &str) -> Option<&'a TitlePattern> {
        let t = title.to_lowercase();
        app.title_patterns.iter().find(|p| {
            p.contains
                .iter()
                .any(|needle| t.contains(&needle.to_lowercase()))
        })
    }
}

/// The bundled catalog, parsed once. A catalog that does not parse is a build
/// defect (`tests/call_coverage_ledger.rs` parses it) — an empty one is used
/// so detection degrades to "no offer", never a crash.
pub fn catalog() -> &'static CallCatalog {
    static C: OnceLock<CallCatalog> = OnceLock::new();
    C.get_or_init(|| match CallCatalog::parse(CATALOG_JSON) {
        Ok(c) => c,
        Err(e) => {
            log::error!("call_signals catalog_parse_failed error={e}");
            CallCatalog {
                version: 0,
                apps: Vec::new(),
            }
        }
    })
}

// ─── Snapshot (plain data) ──────────────────────────────────────────────────

/// Whose Accessibility grant produced `browser_titles`. A CLI run from
/// Terminal is attributed by TCC to Terminal (the responsible process), so it
/// reports TERMINAL's `AXIsProcessTrusted`, not Yap's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AxSource {
    Yap,
    Terminal,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningApp {
    pub pid: i32,
    pub bundle_id: String,
}

/// One browser window title, reduced to what matched. NEVER the text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleObs {
    pub bundle_id: String,
    /// The catalog title pattern it matched, or `None`.
    pub pattern_id: Option<String>,
    /// Character count — a shape, not content.
    pub len: usize,
    /// ONLY with `--raw-titles` at a developer terminal. Never in a fixture,
    /// a log, or the product.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarHint {
    /// An event with a conferencing link is in its offer window.
    pub linked_event_in_window: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallSnapshot {
    /// Hand-built fixture (true) or recorded by `--probe-calls` (false).
    #[serde(default)]
    pub synthetic: bool,
    /// Yap's own pid: it and its children are removed before scoring.
    pub own_pid: i32,
    pub procs: Vec<ProcAudio>,
    /// Running catalog apps only (nothing else leaves the machine's memory).
    #[serde(default)]
    pub running_apps: Vec<RunningApp>,
    /// The frontmost app's bundle id when it is a catalog app.
    #[serde(default)]
    pub frontmost: Option<String>,
    /// `None` = Accessibility not granted (AX unknown).
    #[serde(default)]
    pub browser_titles: Option<Vec<TitleObs>>,
    pub ax_source: AxSource,
    #[serde(default)]
    pub calendar: Option<CalendarHint>,
}

impl CallSnapshot {
    /// Processes that are not Yap (this pid, or anything it spawned).
    pub fn others(&self) -> impl Iterator<Item = &ProcAudio> {
        let own = self.own_pid;
        self.procs
            .iter()
            .filter(move |p| p.pid != own && p.parent_pid != Some(own))
    }

    fn app_pid(&self, bundle_id: &str) -> Option<i32> {
        self.running_apps
            .iter()
            .find(|a| a.bundle_id.eq_ignore_ascii_case(bundle_id))
            .map(|a| a.pid)
    }

    fn app_at(&self, pid: i32) -> Option<&RunningApp> {
        self.running_apps.iter().find(|a| a.pid == pid)
    }
}

// ─── Scoring ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    /// The process IS the catalog app.
    Bundle,
    /// The process's parent pid is a running catalog app.
    HelperParent,
    /// The process's bundle id matches a catalog helper and the app runs.
    HelperPattern,
    /// `com.apple.WebKit.GPU` + a Safari window-title match.
    WebkitTitle,
    /// avconferenced / callservicesd while FaceTime / Phone runs.
    Daemon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallCandidate {
    /// The catalog row.
    pub bundle_id: String,
    /// What the offer calls it ("Zoom", or the title pattern: "Google Meet").
    pub name: String,
    pub confidence: u8,
    pub via: Via,
    /// The app's pid when it runs (else the audio process's) — the session
    /// part of the snooze key.
    pub pid: i32,
}

impl CallCandidate {
    pub fn offers(&self) -> bool {
        self.confidence >= OFFER_THRESHOLD
    }

    /// The snooze key: one app session is never offered twice.
    pub fn offer_key(&self) -> String {
        format!("app:{}:{}", self.bundle_id, self.pid)
    }
}

/// What the last evaluation led with (hysteresis only — never an offer by
/// itself).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallMemory {
    pub last_bundle: Option<String>,
}

impl CallMemory {
    pub fn observe(&mut self, candidate: Option<&CallCandidate>) {
        self.last_bundle = candidate.map(|c| c.bundle_id.clone());
    }
}

/// Attribute one input-running process to a catalog row: `(row, via, app pid)`.
fn attribute<'a>(
    p: &ProcAudio,
    snap: &CallSnapshot,
    catalog: &'a CallCatalog,
) -> Option<(&'a CatalogApp, Via, i32)> {
    let bundle = p.bundle_id.as_deref().unwrap_or("");
    // 1. The process is the app.
    if let Some(app) = catalog.app(bundle) {
        if app.kind != AppKind::SystemDaemon && !app.helper_needs_title {
            return Some((app, Via::Bundle, p.pid));
        }
    }
    // 2. Helper via parent pid → a running catalog app.
    if let Some(parent) = p.parent_pid.and_then(|pp| snap.app_at(pp)) {
        if let Some(app) = catalog.app(&parent.bundle_id) {
            if app.kind != AppKind::SystemDaemon && !app.helper_needs_title {
                return Some((app, Via::HelperParent, parent.pid));
            }
        }
    }
    // 3. Helper pattern (ppid unknown or launchd) — the app must be running.
    let app = catalog.by_helper(bundle)?;
    let app_pid = snap.app_pid(&app.bundle_id)?;
    let via = match app.kind {
        AppKind::SystemDaemon => Via::Daemon,
        _ if app.helper_needs_title => Via::WebkitTitle,
        _ => Via::HelperPattern,
    };
    Some((app, via, app_pid))
}

/// The title pattern a browser row's open windows matched, if any. `None`
/// outer = AX unknown.
fn title_match<'a>(snap: &CallSnapshot, app: &'a CatalogApp) -> Option<Option<&'a TitlePattern>> {
    let titles = snap.browser_titles.as_ref()?;
    Some(
        titles
            .iter()
            .filter(|t| t.bundle_id.eq_ignore_ascii_case(&app.bundle_id))
            .filter_map(|t| t.pattern_id.as_deref())
            .find_map(|id| app.title_patterns.iter().find(|p| p.id == id)),
    )
}

/// THE score. Yap's own pid and its children are removed first; only a
/// process whose `running_input` READ true counts (output-only and unreadable
/// never raise confidence). Returns the best candidate, offering or not.
pub fn score(
    snap: &CallSnapshot,
    catalog: &CallCatalog,
    memory: &CallMemory,
) -> Option<CallCandidate> {
    let mut best: Option<CallCandidate> = None;
    for p in snap.others() {
        if p.running_input != Some(true) {
            continue;
        }
        let Some((app, via, pid)) = attribute(p, snap, catalog) else {
            continue;
        };
        let mut name = app.name.clone();
        let mut cap = 100u8;
        let base = match app.kind {
            AppKind::Native => BASE_NATIVE,
            AppKind::SystemDaemon => BASE_DAEMON,
            AppKind::Browser => match title_match(snap, app) {
                Some(Some(pattern)) => {
                    name = pattern.name.clone();
                    BASE_BROWSER_TITLE
                }
                // Safari's WebKit.GPU is attributable ONLY by a title match.
                _ if app.helper_needs_title => continue,
                None => BASE_BROWSER_AX_UNKNOWN,
                Some(None) => {
                    cap = BROWSER_NO_TITLE_CAP;
                    BASE_BROWSER_AX_UNKNOWN
                }
            },
        };
        let mut confidence = base;
        if snap
            .frontmost
            .as_deref()
            .is_some_and(|f| f.eq_ignore_ascii_case(&app.bundle_id))
        {
            confidence = confidence.saturating_add(BONUS_FRONTMOST);
        }
        if snap.calendar.is_some_and(|c| c.linked_event_in_window) {
            confidence = confidence.saturating_add(BONUS_CALENDAR);
        }
        if memory.last_bundle.as_deref() == Some(app.bundle_id.as_str()) {
            confidence = confidence.saturating_add(BONUS_MEMORY);
        }
        let confidence = confidence.min(100).min(cap);
        if best.as_ref().is_none_or(|b| confidence > b.confidence) {
            best = Some(CallCandidate {
                bundle_id: app.bundle_id.clone(),
                name,
                confidence,
                via,
                pid,
            });
        }
    }
    best
}

/// The offer text. The far side of FaceTime / Phone is NOT measured, so an
/// unverified system-daemon row never claims it.
pub fn offer_copy(candidate: &CallCandidate, catalog: &CallCatalog) -> String {
    let your_side_only = catalog
        .app(&candidate.bundle_id)
        .is_some_and(|a| a.kind == AppKind::SystemDaemon && !a.verified);
    if your_side_only {
        format!("{} call — record your side?", candidate.name)
    } else {
        format!("{} call — record it?", candidate.name)
    }
}

// ─── Cadence ────────────────────────────────────────────────────────────────

/// The ONE 30 s backstop runs only while "Offer to record" is on.
pub fn timer_enabled(offer_enabled: bool) -> bool {
    offer_enabled
}

/// Which `kAudioProcessPropertyIsRunningInput` listeners to add / remove when
/// the process object list goes from `prev` to `next`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListenerDiff {
    pub add: Vec<u32>,
    pub remove: Vec<u32>,
}

pub fn process_listener_diff(prev: &[u32], next: &[u32]) -> ListenerDiff {
    let p: BTreeSet<u32> = prev.iter().copied().collect();
    let n: BTreeSet<u32> = next.iter().copied().collect();
    ListenerDiff {
        add: n.difference(&p).copied().collect(),
        remove: p.difference(&n).copied().collect(),
    }
}

/// Reduce one browser window title to `{ pattern_id, len }`; the text is kept
/// only when `raw` (the developer `--raw-titles` flag).
pub fn observe_title(app: &CatalogApp, title: &str, raw: bool) -> TitleObs {
    TitleObs {
        bundle_id: app.bundle_id.clone(),
        pattern_id: CallCatalog::match_title(app, title).map(|p| p.id.clone()),
        len: title.chars().count(),
        raw_title: raw.then(|| title.to_string()),
    }
}

/// The macOS adapters: read-only.
#[cfg(target_os = "macos")]
pub mod sys {
    use super::{AxSource, CalendarHint, CallCatalog, CallSnapshot, RunningApp, TitleObs};
    use std::ffi::c_void;
    use std::sync::Mutex;

    use objc2_core_audio::{
        kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
        kAudioProcessPropertyIsRunningInput, AudioObjectAddPropertyListener, AudioObjectID,
        AudioObjectPropertyAddress, AudioObjectRemovePropertyListener,
    };
    use std::ptr::NonNull;

    /// Runs on CoreAudio's notification thread (not the IO thread): it only
    /// wakes the meeting_detect watcher, which does the reading.
    unsafe extern "C-unwind" fn on_change(
        _object: AudioObjectID,
        _count: u32,
        _addresses: NonNull<AudioObjectPropertyAddress>,
        _client: *mut c_void,
    ) -> i32 {
        crate::meeting_detect::wake();
        0
    }

    fn addr(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        }
    }

    fn add(object: AudioObjectID, selector: u32) -> i32 {
        let a = addr(selector);
        // SAFETY: a stack address struct; a static fn; null client data.
        unsafe {
            AudioObjectAddPropertyListener(
                object,
                NonNull::from(&a),
                Some(on_change),
                std::ptr::null_mut(),
            )
        }
    }

    fn remove(object: AudioObjectID, selector: u32) -> i32 {
        let a = addr(selector);
        // SAFETY: as `add`. A process that already left returns an error,
        // which is fine — its listener went with it.
        unsafe {
            AudioObjectRemovePropertyListener(
                object,
                NonNull::from(&a),
                Some(on_change),
                std::ptr::null_mut(),
            )
        }
    }

    static LIST_ARMED: Mutex<bool> = Mutex::new(false);
    static INPUT_ARMED: Mutex<Vec<AudioObjectID>> = Mutex::new(Vec::new());

    fn gate_open() -> bool {
        crate::os_version_gate::system_audio_gate_now().is_available()
    }

    /// Arm the ProcessObjectList listener (idempotent) and re-sync one
    /// IsRunningInput listener per process object. Called by the watcher on
    /// every evaluation — the list listener wakes it on every JOIN / LEAVE.
    pub fn sync_listeners() {
        if !gate_open() {
            return;
        }
        if let Ok(mut armed) = LIST_ARMED.lock() {
            if !*armed {
                let status = add(
                    kAudioObjectSystemObject as AudioObjectID,
                    kAudioHardwarePropertyProcessObjectList,
                );
                *armed = status == 0;
                if status == 0 {
                    log::info!("call_signals listener_armed selector=ProcessObjectList");
                } else {
                    log::warn!(
                        "call_signals listener_failed selector=ProcessObjectList status={status}"
                    );
                }
            }
        }
        let next = match crate::syscapture::imp::process_object_list() {
            Ok(list) => list,
            Err(status) => {
                log::warn!("call_signals process_list_unreadable status={status}");
                return;
            }
        };
        let Ok(mut prev) = INPUT_ARMED.lock() else {
            return;
        };
        let diff = super::process_listener_diff(&prev, &next);
        if diff.add.is_empty() && diff.remove.is_empty() {
            return;
        }
        for object in &diff.remove {
            remove(*object, kAudioProcessPropertyIsRunningInput);
        }
        let mut failed = 0u32;
        for object in &diff.add {
            if add(*object, kAudioProcessPropertyIsRunningInput) != 0 {
                failed += 1;
            }
        }
        log::info!(
            "call_signals input_listeners_synced added={} removed={} failed={failed} total={}",
            diff.add.len(),
            diff.remove.len(),
            next.len()
        );
        *prev = next;
    }

    /// Running catalog apps and the frontmost one, from NSWorkspace.
    fn workspace(catalog: &CallCatalog) -> (Vec<RunningApp>, Option<String>) {
        use objc2::msg_send;
        use objc2::rc::autoreleasepool;
        use objc2::runtime::{AnyClass, AnyObject};
        use objc2_foundation::NSString;

        unsafe fn bundle_of(app: *mut AnyObject) -> Option<String> {
            if app.is_null() {
                return None;
            }
            let b: *mut AnyObject = msg_send![app, bundleIdentifier];
            if b.is_null() {
                return None;
            }
            Some((*(b as *const NSString)).to_string())
        }

        autoreleasepool(|_| {
            let Some(ws) = AnyClass::get(c"NSWorkspace") else {
                return (Vec::new(), None);
            };
            // SAFETY: +sharedWorkspace, -runningApplications (NSArray of
            // NSRunningApplication), -frontmostApplication; all read-only.
            unsafe {
                let shared: *mut AnyObject = msg_send![ws, sharedWorkspace];
                if shared.is_null() {
                    return (Vec::new(), None);
                }
                let mut running = Vec::new();
                let apps: *mut AnyObject = msg_send![shared, runningApplications];
                if !apps.is_null() {
                    let n: usize = msg_send![apps, count];
                    for i in 0..n {
                        let app: *mut AnyObject = msg_send![apps, objectAtIndex: i];
                        if let Some(bundle) = bundle_of(app) {
                            if catalog.app(&bundle).is_some() {
                                let pid: i32 = msg_send![app, processIdentifier];
                                running.push(RunningApp {
                                    pid,
                                    bundle_id: bundle,
                                });
                            }
                        }
                    }
                }
                let front: *mut AnyObject = msg_send![shared, frontmostApplication];
                let frontmost = bundle_of(front).filter(|b| catalog.app(b).is_some());
                (running, frontmost)
            }
        })
    }

    mod ax {
        use core_foundation::array::CFArray;
        use core_foundation::base::{CFRelease, CFType, CFTypeRef, TCFType};
        use core_foundation::string::{CFString, CFStringRef};
        use std::ffi::c_void;

        type AXUIElementRef = *mut c_void;

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

        fn copy(element: AXUIElementRef, name: &str) -> Option<CFTypeRef> {
            let key = CFString::new(name);
            let mut val: CFTypeRef = std::ptr::null_mut();
            // SAFETY: a live AX element and a CFString key; +1 on success.
            let err = unsafe {
                AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut val)
            };
            (err == 0 && !val.is_null()).then_some(val)
        }

        /// Every window title of the app at `pid`. Read, matched by the
        /// caller, and dropped.
        pub fn window_titles(pid: i32) -> Vec<String> {
            let mut out = Vec::new();
            // SAFETY: AXUIElementCreateApplication returns +1 (or null).
            let app = unsafe { AXUIElementCreateApplication(pid) };
            if app.is_null() {
                return out;
            }
            // SAFETY: a live element; a short timeout so a hung app never
            // stalls the watcher.
            unsafe { AXUIElementSetMessagingTimeout(app, 0.25) };
            if let Some(windows) = copy(app, "AXWindows") {
                // SAFETY: AXWindows is a CFArray of AXUIElement; create rule.
                let windows: CFArray<CFType> =
                    unsafe { CFArray::wrap_under_create_rule(windows as _) };
                for w in windows.iter() {
                    if let Some(title) = copy(w.as_CFTypeRef() as AXUIElementRef, "AXTitle") {
                        // SAFETY: AXTitle is a CFString; create rule.
                        let s = unsafe { CFString::wrap_under_create_rule(title as CFStringRef) };
                        out.push(s.to_string());
                    }
                }
            }
            // SAFETY: balances the create above.
            unsafe { CFRelease(app as CFTypeRef) };
            out
        }
    }

    /// One snapshot. `None` below macOS 14.4 or when the process list cannot
    /// be read — no evidence, never "no call".
    pub fn snapshot(
        catalog: &CallCatalog,
        ax_source: AxSource,
        raw_titles: bool,
        calendar: Option<CalendarHint>,
    ) -> Result<CallSnapshot, String> {
        if !gate_open() {
            return Err("per-process audio state needs macOS 14.4+".into());
        }
        let procs = crate::syscapture::imp::process_audio_activity()
            .map_err(|status| format!("CoreAudio process list unreadable (status {status})"))?;
        let (running_apps, frontmost) = workspace(catalog);
        let browser_titles = crate::perm_probe::accessibility_trusted().then(|| {
            let mut titles: Vec<TitleObs> = Vec::new();
            for running in &running_apps {
                let Some(app) = catalog.app(&running.bundle_id) else {
                    continue;
                };
                if app.kind != super::AppKind::Browser {
                    continue;
                }
                for title in ax::window_titles(running.pid) {
                    titles.push(super::observe_title(app, &title, raw_titles));
                }
            }
            titles
        });
        Ok(CallSnapshot {
            synthetic: false,
            own_pid: std::process::id() as i32,
            procs,
            running_apps,
            frontmost,
            browser_titles,
            ax_source,
            calendar,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(
        pid: i32,
        ppid: i32,
        bundle: &str,
        input: Option<bool>,
        output: Option<bool>,
    ) -> ProcAudio {
        ProcAudio {
            pid,
            parent_pid: Some(ppid),
            bundle_id: Some(bundle.into()),
            running_input: input,
            running_output: output,
        }
    }

    fn snap(
        procs: Vec<ProcAudio>,
        apps: &[(i32, &str)],
        titles: Option<Vec<TitleObs>>,
    ) -> CallSnapshot {
        CallSnapshot {
            synthetic: true,
            own_pid: 900,
            procs,
            running_apps: apps
                .iter()
                .map(|(pid, b)| RunningApp {
                    pid: *pid,
                    bundle_id: (*b).into(),
                })
                .collect(),
            frontmost: None,
            browser_titles: titles,
            ax_source: AxSource::Yap,
            calendar: None,
        }
    }

    const YAP: &str = "com.wilsonguenther.wilson-voice";

    #[test]
    fn call_signals_catalog_parses_and_has_the_seed_rows() {
        let c = CallCatalog::parse(CATALOG_JSON).expect("catalog parses");
        for b in [
            "us.zoom.xos",
            "com.apple.FaceTime",
            "com.apple.mobilephone",
            "com.google.Chrome",
            "com.apple.Safari",
        ] {
            assert!(c.app(b).is_some(), "{b}");
        }
    }

    #[test]
    fn call_signals_self_exclusion_yap_in_never_scores() {
        // Yap itself and a sidecar it spawned, both with the mic warm.
        let s = snap(
            vec![
                proc(900, 1, YAP, Some(true), Some(false)),
                proc(901, 900, "us.zoom.xos", Some(true), Some(false)),
            ],
            &[(901, "us.zoom.xos")],
            None,
        );
        assert_eq!(score(&s, catalog(), &CallMemory::default()), None);
    }

    #[test]
    fn call_signals_helper_attribution_via_parent_pid() {
        // The helper's own bundle id is unreadable: only its parent says Chrome.
        let mut helper = proc(2001, 2000, "", Some(true), Some(true));
        helper.bundle_id = None;
        let s = snap(vec![helper], &[(2000, "com.google.Chrome")], None);
        let c = score(&s, catalog(), &CallMemory::default()).expect("attributed");
        assert_eq!(c.bundle_id, "com.google.Chrome");
        assert_eq!(c.via, Via::HelperParent);
        assert_eq!(c.pid, 2000);
        assert!(c.offers(), "AX unknown browser input offers at 60");
    }

    #[test]
    fn call_signals_safari_capped_without_a_title() {
        let gpu = proc(3001, 1, "com.apple.WebKit.GPU", Some(true), Some(true));
        // AX granted, no window matched: no candidate at all for WebKit.GPU.
        let s = snap(
            vec![gpu.clone()],
            &[(3000, "com.apple.Safari")],
            Some(vec![TitleObs {
                bundle_id: "com.apple.Safari".into(),
                pattern_id: None,
                len: 12,
                raw_title: None,
            }]),
        );
        assert_eq!(score(&s, catalog(), &CallMemory::default()), None);
        // AX unknown: still none — WebKit.GPU is never "Safari's child".
        let s = snap(vec![gpu], &[(3000, "com.apple.Safari")], None);
        assert_eq!(score(&s, catalog(), &CallMemory::default()), None);
    }

    #[test]
    fn call_signals_browser_ax_granted_no_title_caps_at_55() {
        let helper = proc(
            2001,
            2000,
            "com.google.Chrome.helper",
            Some(true),
            Some(true),
        );
        let mut s = snap(
            vec![helper],
            &[(2000, "com.google.Chrome")],
            Some(vec![TitleObs {
                bundle_id: "com.google.Chrome".into(),
                pattern_id: None,
                len: 30,
                raw_title: None,
            }]),
        );
        s.frontmost = Some("com.google.Chrome".into());
        s.calendar = Some(CalendarHint {
            linked_event_in_window: true,
        });
        let c = score(&s, catalog(), &CallMemory::default()).unwrap();
        assert_eq!(c.confidence, BROWSER_NO_TITLE_CAP);
        assert!(!c.offers());
    }

    #[test]
    fn call_signals_output_only_never_offers() {
        let s = snap(
            vec![proc(1200, 1, "us.zoom.xos", Some(false), Some(true))],
            &[(1200, "us.zoom.xos")],
            None,
        );
        assert_eq!(score(&s, catalog(), &CallMemory::default()), None);
    }

    #[test]
    fn call_signals_unreadable_property_never_raises_confidence() {
        let s = snap(
            vec![proc(1200, 1, "us.zoom.xos", None, Some(true))],
            &[(1200, "us.zoom.xos")],
            None,
        );
        assert_eq!(score(&s, catalog(), &CallMemory::default()), None);
        // And next to a readable one it changes nothing.
        let s2 = snap(
            vec![
                proc(1200, 1, "us.zoom.xos", Some(true), Some(true)),
                proc(1300, 1, "com.hnc.Discord", None, None),
            ],
            &[(1200, "us.zoom.xos"), (1300, "com.hnc.Discord")],
            None,
        );
        let c = score(&s2, catalog(), &CallMemory::default()).unwrap();
        assert_eq!(
            (c.bundle_id.as_str(), c.confidence),
            ("us.zoom.xos", BASE_NATIVE)
        );
    }

    #[test]
    fn call_signals_daemon_copy_says_your_side() {
        let s = snap(
            vec![proc(
                4001,
                1,
                "com.apple.avconferenced",
                Some(true),
                Some(true),
            )],
            &[(4000, "com.apple.FaceTime")],
            None,
        );
        let c = score(&s, catalog(), &CallMemory::default()).unwrap();
        assert_eq!(c.via, Via::Daemon);
        assert_eq!(
            offer_copy(&c, catalog()),
            "FaceTime call — record your side?"
        );
        // Without FaceTime running, avconferenced alone is nobody's call.
        let s = snap(
            vec![proc(
                4001,
                1,
                "com.apple.avconferenced",
                Some(true),
                Some(true),
            )],
            &[],
            None,
        );
        assert_eq!(score(&s, catalog(), &CallMemory::default()), None);
    }

    #[test]
    fn call_signals_memory_is_a_bonus_never_an_offer() {
        let s = snap(
            vec![proc(1200, 1, "us.zoom.xos", Some(true), Some(true))],
            &[(1200, "us.zoom.xos")],
            None,
        );
        let mut m = CallMemory::default();
        let first = score(&s, catalog(), &m).unwrap();
        m.observe(Some(&first));
        assert_eq!(
            score(&s, catalog(), &m).unwrap().confidence,
            first.confidence + BONUS_MEMORY
        );
        let idle = snap(vec![], &[(1200, "us.zoom.xos")], None);
        assert_eq!(score(&idle, catalog(), &m), None);
    }

    #[test]
    fn call_signals_process_listener_diff_resyncs_on_every_list_change() {
        assert_eq!(
            process_listener_diff(&[], &[5, 7]),
            ListenerDiff {
                add: vec![5, 7],
                remove: vec![]
            }
        );
        assert_eq!(
            process_listener_diff(&[5, 7], &[7, 9]),
            ListenerDiff {
                add: vec![9],
                remove: vec![5]
            }
        );
        assert_eq!(
            process_listener_diff(&[7, 9], &[7, 9]),
            ListenerDiff::default()
        );
        // Replaying a sequence of list changes keeps exactly one listener per
        // live object.
        let mut armed: BTreeSet<u32> = BTreeSet::new();
        let lists: [&[u32]; 4] = [&[1, 2, 3], &[2, 3, 4], &[4], &[4, 1]];
        let mut prev: Vec<u32> = Vec::new();
        for next in lists {
            let d = process_listener_diff(&prev, next);
            for r in &d.remove {
                assert!(armed.remove(r));
            }
            for a in &d.add {
                assert!(armed.insert(*a));
            }
            assert_eq!(armed.iter().copied().collect::<Vec<_>>(), {
                let mut v = next.to_vec();
                v.sort();
                v
            });
            prev = next.to_vec();
        }
    }

    #[test]
    fn call_signals_timer_enabled_false_is_off() {
        assert!(!timer_enabled(false));
        assert!(timer_enabled(true));
    }

    #[test]
    fn call_signals_title_obs_never_carries_text_without_raw() {
        let chrome = catalog().app("com.google.Chrome").unwrap();
        let o = observe_title(chrome, "Meet - abc-defg-hij", false);
        assert_eq!(o.pattern_id.as_deref(), Some("meet"));
        assert_eq!(o.raw_title, None);
        let json = serde_json::to_string(&o).unwrap();
        assert!(!json.contains("abc-defg"), "{json}");
        assert!(observe_title(chrome, "Inbox (3)", true).raw_title.is_some());
    }
}
