//! yap-notetaker-detect — notice a meeting is about to start and OFFER to
//! record it. It never records on its own: there is no auto-start setting, and
//! the only path from an offer to a recording is the user pressing Record.
//!
//! Shape: a PURE policy (`calendar_offer`, `in_window`, `has_conferencing_link`,
//! `may_raise`) that `tests/calendar_prompt_policy.rs` tables, plus thin OS
//! adapters (`sys`) that only read.
//!
//! Signals. (a) The calendar: an EventKit event starting in the next
//! [`LEAD_SECS`] or in progress for less than [`IN_PROGRESS_SECS`] whose URL,
//! location or notes carry a conferencing link. (b) The app/browser call
//! signal is owned by yap-call-0: [`evaluate_calls`] takes a
//! `call_signals::sys::snapshot`, scores it with `call_signals::score` and
//! raises an [`OfferSource::App`] offer with `call_signals::offer_copy`'s text.
//! There is no second, UI-pid app signal anywhere.
//!
//! Permissions. Nothing here ever ASKS for Calendars access. The calendar is
//! read only when the user has turned "Use my calendar" on (default OFF; it is
//! turned on by the setup card's Connect calendar click via
//! [`calendar_connected`]) AND EventKit already reports full access.
//!
//! Cadence. Event-driven: EventKit's change notification, NSWorkspace
//! launch/terminate and (yap-call-0) CoreAudio's ProcessObjectList and
//! per-process IsRunningInput listeners wake the watcher, plus ONE
//! [`POLL_INTERVAL`] (30 s) backstop timer, OFF while "Offer to record" is off
//! (`call_signals::timer_enabled`). Never faster. No network, and nothing reads meeting content beyond the
//! three fields that can carry a call link.

use crate::db::Database;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// An event starting within this many seconds is "about to start".
pub const LEAD_SECS: i64 = 120;
/// An event that started less than this many seconds ago is still offerable.
pub const IN_PROGRESS_SECS: i64 = 300;
/// The one timer. The policy forbids anything faster.
pub const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Rust → pill: an offer to show (payload [`MeetingOffer`]). Mirrored by
/// `desktop/src/pill/meetingOffer.ts`.
pub const MEETING_OFFER_EVENT: &str = "meeting_offer";
/// Rust → pill: drop whatever offer is showing (answered, expired, or a
/// meeting started some other way).
pub const MEETING_OFFER_CLEAR_EVENT: &str = "meeting_offer_clear";

/// `settings_kv` — "Offer to record detected meetings" (default ON).
pub const OFFER_DETECTED_KEY: &str = "meeting_detect_offer_v1";
/// `settings_kv` — "Use my calendar" (default OFF).
pub const USE_CALENDAR_KEY: &str = "meeting_detect_use_calendar_v1";

/// Case-insensitive substrings that mark a conferencing link.
pub const CONFERENCING_MARKERS: &[&str] = &[
    "zoom.us/j",
    "meet.google.com",
    "teams.microsoft.com",
    "facetime.apple.com",
    "whereby.com",
    "webex.com",
];

/// Does `text` carry a link to a call? A pure substring test on a lowercase
/// copy — no URL parsing, no network.
pub fn has_conferencing_link(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    CONFERENCING_MARKERS.iter().any(|m| t.contains(m))
}

/// The four fields of an EventKit event this policy reads (plus its id).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CalendarEvent {
    /// `EKEvent.eventIdentifier` (stable across edits of the same event).
    pub id: String,
    pub title: Option<String>,
    /// Start, seconds since the Unix epoch.
    pub start_unix: i64,
    pub url: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
}

impl CalendarEvent {
    /// URL, location or notes carries a conferencing link.
    pub fn has_link(&self) -> bool {
        [&self.url, &self.location, &self.notes]
            .iter()
            .any(|f| f.as_deref().is_some_and(has_conferencing_link))
    }

    /// The snooze key: one per OCCURRENCE (a recurring meeting's next
    /// occurrence is a new offer; this one is not offered twice).
    pub fn offer_key(&self) -> String {
        format!("cal:{}:{}", self.id, self.start_unix)
    }
}

/// Where an offer came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OfferSource {
    Calendar,
    /// yap-call-0 / yap-call-2 (a conferencing app or browser with live audio).
    App,
}

/// What the pill shows. `key` is what Not now snoozes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingOffer {
    pub key: String,
    pub title: Option<String>,
    pub source: OfferSource,
}

/// Is an event starting at `start_unix` offerable at `now_unix`? Starting in
/// `0..=LEAD_SECS`, or started less than `IN_PROGRESS_SECS` ago.
pub fn in_window(start_unix: i64, now_unix: i64) -> bool {
    let until = start_unix - now_unix;
    if until >= 0 {
        until <= LEAD_SECS
    } else {
        -until < IN_PROGRESS_SECS
    }
}

/// Offers the user said Not now to, for this app run. The same calendar
/// occurrence (or app session) is never offered twice.
#[derive(Debug, Default, Clone)]
pub struct Snoozes(HashSet<String>);

impl Snoozes {
    pub fn snooze(&mut self, key: &str) {
        self.0.insert(key.to_string());
    }
    pub fn is_snoozed(&self, key: &str) -> bool {
        self.0.contains(key)
    }
}

/// THE calendar policy. The offerable event closest to now, or `None`.
pub fn calendar_offer(
    events: &[CalendarEvent],
    now_unix: i64,
    snoozes: &Snoozes,
) -> Option<MeetingOffer> {
    events
        .iter()
        .filter(|e| e.has_link() && in_window(e.start_unix, now_unix))
        .filter(|e| !snoozes.is_snoozed(&e.offer_key()))
        .min_by_key(|e| ((e.start_unix - now_unix).abs(), e.start_unix))
        .map(|e| MeetingOffer {
            key: e.offer_key(),
            title: e
                .title
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string),
            source: OfferSource::Calendar,
        })
}

/// Settings › Meetings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectSettings {
    /// "Offer to record detected meetings". Default ON. Needs no permission.
    pub offer_detected: bool,
    /// "Use my calendar". Default OFF; turned on by the setup card's Connect
    /// calendar click, never by default.
    pub use_calendar: bool,
}

impl Default for DetectSettings {
    fn default() -> Self {
        Self {
            offer_detected: true,
            use_calendar: false,
        }
    }
}

fn flag(raw: Option<String>, default: bool) -> bool {
    match raw.as_deref() {
        Some("1") | Some("true") => true,
        Some("0") | Some("false") => false,
        _ => default,
    }
}

fn flag_str(on: bool) -> &'static str {
    if on {
        "1"
    } else {
        "0"
    }
}

/// Read both toggles. Never fails: a missing or garbled row is its default.
pub fn load_settings(db: &Database) -> DetectSettings {
    let d = DetectSettings::default();
    DetectSettings {
        offer_detected: flag(db.setting_get(OFFER_DETECTED_KEY), d.offer_detected),
        use_calendar: flag(db.setting_get(USE_CALENDAR_KEY), d.use_calendar),
    }
}

pub fn save_offer_detected(db: &Database, on: bool) -> Result<(), String> {
    db.setting_set(OFFER_DETECTED_KEY, flag_str(on))?;
    // The backstop timer is off while this is off, so the watcher must be
    // woken to notice it came back on.
    wake();
    Ok(())
}

pub fn save_use_calendar(db: &Database, on: bool) -> Result<(), String> {
    db.setting_set(USE_CALENDAR_KEY, flag_str(on))
}

/// The setup card's Connect calendar click finished. A grant is the ONE thing
/// that turns "Use my calendar" on; a refusal leaves it as it was.
/// Consumer: `connect_calendar` (yap-notetaker-onboard, PR #271).
pub fn calendar_connected(db: &Database, granted: bool) -> Result<(), String> {
    if granted {
        save_use_calendar(db, true)?;
        wake();
    }
    Ok(())
}

/// The calendar signal runs only when the user asked for both AND EventKit
/// already reports full access. It never asks.
pub fn calendar_signal_enabled(s: &DetectSettings, calendar_full_access: bool) -> bool {
    s.offer_detected && s.use_calendar && calendar_full_access
}

/// May `offer` be shown right now?
pub fn may_raise(
    s: &DetectSettings,
    meeting_recording: bool,
    snoozes: &Snoozes,
    offer: &MeetingOffer,
) -> bool {
    s.offer_detected && !meeting_recording && !snoozes.is_snoozed(&offer.key)
}

/// The user's answer on the pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OfferChoice {
    Record,
    /// "Not now".
    Snooze,
}

impl OfferChoice {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "record" => Some(Self::Record),
            "snooze" => Some(Self::Snooze),
            _ => None,
        }
    }
}

// ─── runtime state ──────────────────────────────────────────────────────────

#[derive(Default)]
struct Detector {
    snoozes: Snoozes,
    pending: Option<MeetingOffer>,
}

fn detector() -> &'static Mutex<Detector> {
    static D: OnceLock<Mutex<Detector>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(Detector::default()))
}

fn wake_slot() -> &'static Mutex<Option<mpsc::Sender<()>>> {
    static W: OnceLock<Mutex<Option<mpsc::Sender<()>>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(None))
}

/// Re-evaluate now (a setting changed). A no-op before the watcher starts.
pub fn wake() {
    if let Ok(slot) = wake_slot().lock() {
        if let Some(tx) = slot.as_ref() {
            let _ = tx.send(());
        }
    }
}

/// The offer the pill should be showing, if any (a pill webview that mounts
/// after the emit reads it here).
pub fn pending() -> Option<MeetingOffer> {
    detector().lock().ok().and_then(|d| d.pending.clone())
}

/// Record the user's answer. Both answers snooze the key — after Record the
/// same occurrence must not be offered again once that recording stops.
/// Returns the offer that was answered (its title names the recording).
pub fn respond(key: &str) -> Option<MeetingOffer> {
    let mut d = detector().lock().ok()?;
    d.snoozes.snooze(key);
    match d.pending.take() {
        Some(p) if p.key == key => Some(p),
        other => {
            d.pending = other;
            None
        }
    }
}

/// THE offer entry point — the calendar watcher below and yap-call-0/2 call
/// this. Shows the pill and emits [`MEETING_OFFER_EVENT`]. Returns whether it
/// was raised. It cannot start a recording; only Record on the pill does.
pub fn raise(app: &AppHandle, db: &Database, meeting_recording: bool, offer: MeetingOffer) -> bool {
    let settings = load_settings(db);
    {
        let Ok(mut d) = detector().lock() else {
            return false;
        };
        if !may_raise(&settings, meeting_recording, &d.snoozes, &offer) {
            return false;
        }
        if d.pending.as_ref() == Some(&offer) {
            return true;
        }
        d.pending = Some(offer.clone());
    }
    crate::float_pill::show_for_recording(app);
    if let Err(e) = app.emit(MEETING_OFFER_EVENT, &offer) {
        log::warn!("emit {MEETING_OFFER_EVENT}: {e}");
    }
    log::info!("meeting_detect: offered ({:?})", offer.source);
    true
}

/// Drop the pending offer (if any) and tell the pill. `only` limits it to one
/// source. Returns whether anything was cleared.
pub fn clear(app: &AppHandle, only: Option<OfferSource>) -> bool {
    let cleared = match detector().lock() {
        Ok(mut d) => match &d.pending {
            Some(p) if only.is_none_or(|s| s == p.source) => {
                d.pending = None;
                true
            }
            _ => false,
        },
        Err(_) => false,
    };
    if cleared {
        emit_clear(app);
        // #272: `raise` put the pill up for this offer; with the offer gone
        // (window passed, setting off, meeting started) the pill follows
        // `show_floating_pill` again unless a take, meeting or Copy card holds it.
        crate::release_offer_pill(app);
    }
    cleared
}

pub fn emit_clear(app: &AppHandle) {
    if let Err(e) = app.emit(MEETING_OFFER_CLEAR_EVENT, ()) {
        log::warn!("emit {MEETING_OFFER_CLEAR_EVENT}: {e}");
    }
}

/// Settings › Meetings, with where calendar access stands (read, never asked).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectSettingsView {
    pub offer_detected: bool,
    pub use_calendar: bool,
    pub calendar_access: &'static str,
}

/// `EKAuthorizationStatus` → name. 3 is Authorized (≤13) / FullAccess (14+).
pub fn access_name(raw: Option<isize>) -> &'static str {
    match raw {
        None => "unavailable",
        Some(0) => "not_determined",
        Some(1) => "restricted",
        Some(2) => "denied",
        Some(3) => "full_access",
        Some(4) => "write_only",
        Some(_) => "restricted",
    }
}

pub fn calendar_full_access() -> bool {
    sys::raw_status() == Some(3)
}

pub fn settings_view(db: &Database) -> DetectSettingsView {
    let s = load_settings(db);
    DetectSettingsView {
        offer_detected: s.offer_detected,
        use_calendar: s.use_calendar,
        calendar_access: access_name(sys::raw_status()),
    }
}

/// One evaluation of both signals. The calendar leads: when it raised an
/// offer (it carries the event title) the app signal does not replace it.
fn evaluate(app: &AppHandle, db: &Database, meeting_recording: bool, calls: &mut CallWatch) {
    if meeting_recording {
        clear(app, None);
        return;
    }
    let s = load_settings(db);
    let (calendar_raised, hint) = evaluate_calendar(app, db, &s);
    if !calendar_raised {
        evaluate_calls(app, db, &s, hint, calls);
    }
}

/// The calendar signal. Returns (raised an offer, the hint the call score reads).
fn evaluate_calendar(
    app: &AppHandle,
    db: &Database,
    s: &DetectSettings,
) -> (bool, Option<crate::call_signals::CalendarHint>) {
    if !calendar_signal_enabled(s, calendar_full_access()) {
        clear(app, Some(OfferSource::Calendar));
        return (false, None);
    }
    let now = chrono::Utc::now().timestamp();
    let events = sys::events_between(now - IN_PROGRESS_SECS, now + LEAD_SECS);
    let hint = Some(crate::call_signals::CalendarHint {
        linked_event_in_window: events
            .iter()
            .any(|e| e.has_link() && in_window(e.start_unix, now)),
    });
    let snoozes = match detector().lock() {
        Ok(d) => d.snoozes.clone(),
        Err(_) => return (false, hint),
    };
    match calendar_offer(&events, now, &snoozes) {
        Some(offer) => (raise(app, db, false, offer), hint),
        None => {
            // The window passed (or the event lost its link): the offer goes.
            clear(app, Some(OfferSource::Calendar));
            (false, hint)
        }
    }
}

/// What the call signal remembers between evaluations.
#[derive(Default)]
struct CallWatch {
    memory: crate::call_signals::CallMemory,
    /// The last logged (bundle, offered) so a steady state logs once.
    last_logged: Option<(String, bool)>,
    /// The last snapshot error, logged once per distinct error.
    last_error: Option<String>,
}

/// yap-call-0 — the app/browser call signal: snapshot → `call_signals::score`
/// → an [`OfferSource::App`] offer with `call_signals::offer_copy`'s text.
fn evaluate_calls(
    app: &AppHandle,
    db: &Database,
    s: &DetectSettings,
    calendar: Option<crate::call_signals::CalendarHint>,
    calls: &mut CallWatch,
) {
    use crate::call_signals;
    if !s.offer_detected {
        clear(app, Some(OfferSource::App));
        return;
    }
    #[cfg(target_os = "macos")]
    let snapshot = {
        call_signals::sys::sync_listeners();
        let started = std::time::Instant::now();
        let snap = call_signals::sys::snapshot(
            call_signals::catalog(),
            call_signals::AxSource::Yap,
            false,
            calendar,
        );
        crate::diagnostics::record_duration(
            "call_signals",
            "snapshot_ms",
            started.elapsed().as_millis() as u64,
        );
        snap
    };
    #[cfg(not(target_os = "macos"))]
    let snapshot: Result<call_signals::CallSnapshot, String> = {
        let _ = calendar;
        Err("call detection is macOS-only".to_string())
    };
    let snap = match snapshot {
        Ok(snap) => {
            calls.last_error = None;
            snap
        }
        Err(e) => {
            if calls.last_error.as_deref() != Some(e.as_str()) {
                log::warn!("call_signals snapshot_failed error={e:?}");
                calls.last_error = Some(e);
            }
            clear(app, Some(OfferSource::App));
            return;
        }
    };
    let candidate = call_signals::score(&snap, call_signals::catalog(), &calls.memory);
    calls.memory.observe(candidate.as_ref());
    let logged = candidate
        .as_ref()
        .map(|c| (c.bundle_id.clone(), c.offers()));
    if logged != calls.last_logged {
        match &candidate {
            Some(c) => log::info!(
                "call_signals scored bundle={} via={:?} confidence={} offers={}",
                c.bundle_id,
                c.via,
                c.confidence,
                c.offers()
            ),
            None => log::info!("call_signals scored bundle=none procs={}", snap.procs.len()),
        }
        calls.last_logged = logged;
    }
    match candidate.filter(|c| c.offers()) {
        Some(c) => {
            let offer = MeetingOffer {
                key: c.offer_key(),
                title: Some(call_signals::offer_copy(&c, call_signals::catalog())),
                source: OfferSource::App,
            };
            raise(app, db, false, offer);
        }
        None => {
            clear(app, Some(OfferSource::App));
        }
    }
}

/// Start the watcher thread. `meeting_recording` is read on every wake.
pub fn start_watch(
    app: AppHandle,
    db: std::sync::Arc<Database>,
    meeting_recording: impl Fn() -> bool + Send + 'static,
) {
    let (tx, rx) = mpsc::channel::<()>();
    if let Ok(mut slot) = wake_slot().lock() {
        *slot = Some(tx.clone());
    }
    sys::observe_changes(tx);
    let spawned = std::thread::Builder::new()
        .name("yap-meeting-detect".into())
        .spawn(move || {
            let mut calls = CallWatch::default();
            loop {
                evaluate(&app, &db, meeting_recording(), &mut calls);
                // The ONE backstop timer, off while "Offer to record" is off;
                // the listeners and notifications still wake the thread.
                let woke = if crate::call_signals::timer_enabled(load_settings(&db).offer_detected)
                {
                    rx.recv_timeout(POLL_INTERVAL)
                } else {
                    rx.recv()
                        .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
                };
                match woke {
                    // Coalesce a burst of notifications into one evaluation.
                    Ok(()) => while rx.try_recv().is_ok() {},
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        std::thread::sleep(POLL_INTERVAL)
                    }
                }
            }
        });
    if let Err(e) = spawned {
        log::warn!("meeting_detect: watcher thread: {e}");
    }
}

// ─── OS adapters (read-only) ────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod sys {
    use super::CalendarEvent;
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;
    use std::sync::mpsc;

    #[link(name = "EventKit", kind = "framework")]
    extern "C" {}

    /// `EKEntityTypeEvent`.
    const ENTITY_EVENT: usize = 0;

    fn store_class() -> Option<&'static AnyClass> {
        AnyClass::get(c"EKEventStore")
    }

    /// `+[EKEventStore authorizationStatusForEntityType:]`. Never prompts.
    pub fn raw_status() -> Option<isize> {
        let cls = store_class()?;
        // SAFETY: class method, (EKEntityType: NSUInteger) -> NSInteger.
        Some(unsafe { msg_send![cls, authorizationStatusForEntityType: ENTITY_EVENT] })
    }

    unsafe fn ns_string(p: *mut AnyObject) -> Option<String> {
        if p.is_null() {
            return None;
        }
        // SAFETY: caller passes an NSString* (or nil, handled above).
        let s: &NSString = unsafe { &*(p as *const NSString) };
        Some(s.to_string())
    }

    pub(super) unsafe fn date(secs: i64) -> *mut AnyObject {
        let Some(cls) = AnyClass::get(c"NSDate") else {
            return std::ptr::null_mut();
        };
        // SAFETY: +[NSDate dateWithTimeIntervalSince1970:] -> autoreleased NSDate*.
        unsafe { msg_send![cls, dateWithTimeIntervalSince1970: secs as f64] }
    }

    /// Events overlapping `[from, to]` (Unix seconds) on every calendar.
    /// Called only after `raw_status() == Some(3)`; a FRESH store each time,
    /// because a store created before the grant keeps an empty view.
    pub fn events_between(from: i64, to: i64) -> Vec<CalendarEvent> {
        if raw_status() != Some(3) {
            return Vec::new();
        }
        let Some(cls) = store_class() else {
            return Vec::new();
        };
        autoreleasepool(|_| {
            let mut out = Vec::new();
            // SAFETY: +[EKEventStore new] -> retained EKEventStore.
            let store: Option<Retained<AnyObject>> = unsafe { msg_send![cls, new] };
            let Some(store) = store else { return out };
            // SAFETY: every message below is a documented read-only EventKit /
            // Foundation getter; nil returns are checked before use.
            unsafe {
                let start = date(from);
                let end = date(to);
                if start.is_null() || end.is_null() {
                    return out;
                }
                let nil: *mut AnyObject = std::ptr::null_mut();
                let pred: *mut AnyObject = msg_send![
                    &*store,
                    predicateForEventsWithStartDate: start,
                    endDate: end,
                    calendars: nil,
                ];
                if pred.is_null() {
                    return out;
                }
                let list: *mut AnyObject = msg_send![&*store, eventsMatchingPredicate: pred];
                if list.is_null() {
                    return out;
                }
                let n: usize = msg_send![list, count];
                for i in 0..n {
                    let ev: *mut AnyObject = msg_send![list, objectAtIndex: i];
                    if ev.is_null() {
                        continue;
                    }
                    let id = ns_string(msg_send![ev, eventIdentifier]).unwrap_or_default();
                    let sd: *mut AnyObject = msg_send![ev, startDate];
                    if sd.is_null() {
                        continue;
                    }
                    let start_secs: f64 = msg_send![sd, timeIntervalSince1970];
                    let url_obj: *mut AnyObject = msg_send![ev, URL];
                    let url = if url_obj.is_null() {
                        None
                    } else {
                        ns_string(msg_send![url_obj, absoluteString])
                    };
                    out.push(CalendarEvent {
                        id,
                        title: ns_string(msg_send![ev, title]),
                        start_unix: start_secs as i64,
                        url,
                        location: ns_string(msg_send![ev, location]),
                        notes: ns_string(msg_send![ev, notes]),
                    });
                }
            }
            out
        })
    }

    fn observe(center: *mut AnyObject, name: &str, tx: mpsc::Sender<()>) {
        if center.is_null() {
            return;
        }
        let name = NSString::from_str(name);
        let block = RcBlock::new(move |_note: *mut AnyObject| {
            let _ = tx.send(());
        });
        let nil: *mut AnyObject = std::ptr::null_mut();
        // SAFETY: -[NSNotificationCenter addObserverForName:object:queue:usingBlock:]
        // copies the block; the returned token lives for the app's lifetime.
        let token: Option<Retained<AnyObject>> = unsafe {
            msg_send![
                center,
                addObserverForName: &*name,
                object: nil,
                queue: nil,
                usingBlock: &*block,
            ]
        };
        std::mem::forget(token);
    }

    /// EventKit's change notification + NSWorkspace launch/terminate. Each one
    /// only sends a wake; the watcher thread does the reading.
    pub fn observe_changes(tx: mpsc::Sender<()>) {
        if let Some(nc) = AnyClass::get(c"NSNotificationCenter") {
            // SAFETY: +[NSNotificationCenter defaultCenter] -> NSNotificationCenter*.
            let center: *mut AnyObject = unsafe { msg_send![nc, defaultCenter] };
            observe(center, "EKEventStoreChangedNotification", tx.clone());
        }
        if let Some(ws) = AnyClass::get(c"NSWorkspace") {
            // SAFETY: +[NSWorkspace sharedWorkspace] / -notificationCenter.
            let center: *mut AnyObject = unsafe {
                let shared: *mut AnyObject = msg_send![ws, sharedWorkspace];
                if shared.is_null() {
                    std::ptr::null_mut()
                } else {
                    msg_send![shared, notificationCenter]
                }
            };
            observe(
                center,
                "NSWorkspaceDidLaunchApplicationNotification",
                tx.clone(),
            );
            observe(center, "NSWorkspaceDidTerminateApplicationNotification", tx);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod sys {
    use super::CalendarEvent;
    use std::sync::mpsc;
    pub fn raw_status() -> Option<isize> {
        None
    }
    pub fn events_between(_from: i64, _to: i64) -> Vec<CalendarEvent> {
        Vec::new()
    }
    pub fn observe_changes(_tx: mpsc::Sender<()>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #272 fix round 1 — a pending offer holds the pill the way a Copy card
    /// does: `after_recording` ORs `pending().is_some()` into its keep-visible.
    #[test]
    fn pending_offer_keeps_the_pill_up_after_a_take() {
        use crate::float_pill::pill_keeps_up;
        assert!(
            !pill_keeps_up(false, false, false),
            "nothing holds it: hide"
        );
        assert!(
            pill_keeps_up(false, false, true),
            "a pending offer holds it"
        );
        assert!(pill_keeps_up(false, true, false), "a Copy card holds it");
        let src = include_str!("float_pill.rs");
        let body = &src[src.find("pub fn after_recording(").unwrap()..];
        let body = &body[..body.find("dispatch_main(").unwrap()];
        assert!(
            body.contains("crate::meeting_detect::pending().is_some()"),
            "after_recording must consult the pending offer before it hides the pill"
        );
        let clear_src = include_str!("meeting_detect.rs");
        let clear_body = &clear_src[clear_src.find("pub fn clear(").unwrap()..];
        let clear_body = &clear_body[..clear_body.find("\n}\n").unwrap()];
        assert!(
            clear_body.contains("crate::release_offer_pill(app)"),
            "clearing an offer must hand the pill back to show_floating_pill"
        );
    }

    fn tmp_db(tag: &str) -> (std::path::PathBuf, Database) {
        let dir = std::env::temp_dir().join(format!(
            "yap-meeting-detect-{tag}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::open(dir.join("wilson_voice.db")).unwrap();
        (dir, db)
    }

    #[test]
    fn meeting_detect_defaults_offer_on_calendar_off() {
        let (dir, db) = tmp_db("defaults");
        let s = load_settings(&db);
        assert!(
            s.offer_detected,
            "Offer to record detected meetings is default ON"
        );
        assert!(!s.use_calendar, "Use my calendar is default OFF");
        assert!(
            !calendar_signal_enabled(&s, true),
            "no calendar read by default"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn meeting_detect_toggles_round_trip_and_only_a_grant_turns_calendar_on() {
        let (dir, db) = tmp_db("toggles");
        calendar_connected(&db, false).unwrap();
        assert!(!load_settings(&db).use_calendar, "a refusal leaves it off");
        calendar_connected(&db, true).unwrap();
        assert!(load_settings(&db).use_calendar);
        save_offer_detected(&db, false).unwrap();
        let s = load_settings(&db);
        assert!(!s.offer_detected && s.use_calendar);
        assert!(
            !calendar_signal_enabled(&s, true),
            "offer OFF silences the calendar too"
        );
        save_use_calendar(&db, false).unwrap();
        assert!(!load_settings(&db).use_calendar);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn meeting_detect_full_access_alone_never_enables_the_calendar() {
        let on = DetectSettings {
            offer_detected: true,
            use_calendar: true,
        };
        assert!(calendar_signal_enabled(&on, true));
        assert!(!calendar_signal_enabled(&on, false), "no grant → no read");
    }

    #[test]
    fn meeting_detect_choice_parse_and_wire_names() {
        assert_eq!(OfferChoice::parse("record"), Some(OfferChoice::Record));
        assert_eq!(OfferChoice::parse("snooze"), Some(OfferChoice::Snooze));
        assert_eq!(OfferChoice::parse("auto"), None);
        let o = MeetingOffer {
            key: "k".into(),
            title: Some("Standup".into()),
            source: OfferSource::Calendar,
        };
        assert_eq!(
            serde_json::to_string(&o).unwrap(),
            r#"{"key":"k","title":"Standup","source":"calendar"}"#
        );
    }

    #[test]
    fn meeting_detect_poll_is_never_faster_than_30s() {
        assert!(POLL_INTERVAL >= Duration::from_secs(30));
    }

    #[test]
    fn meeting_detect_access_names_and_status_read_never_prompts() {
        assert_eq!(access_name(Some(3)), "full_access");
        assert_eq!(access_name(Some(0)), "not_determined");
        assert_eq!(access_name(None), "unavailable");
        // A real EventKit status read: a class method that never prompts.
        let _ = access_name(sys::raw_status());
    }

    /// The OS adapters' message encodings are checked by objc2 at CALL time,
    /// so a wrong selector type is a startup panic no compile catches. Register
    /// the real observers (no TCC involved) and build a real NSDate.
    #[cfg(target_os = "macos")]
    #[test]
    fn meeting_detect_os_adapters_register_and_message_cleanly() {
        let (tx, _rx) = mpsc::channel::<()>();
        sys::observe_changes(tx);
        // SAFETY: test-only call of the adapter's NSDate constructor.
        let d = objc2::rc::autoreleasepool(|_| unsafe { sys::date(1_800_000_000) }.is_null());
        assert!(!d, "NSDate dateWithTimeIntervalSince1970 returned nil");
    }

    #[test]
    fn meeting_detect_source_never_requests_calendar_access() {
        let src = include_str!("meeting_detect.rs");
        let needle = ["request", "FullAccessToEvents"].concat();
        assert!(
            !src.contains(&needle),
            "meeting_detect must never ask for Calendars"
        );
        let needle2 = ["requestAccess", "ToEntityType"].concat();
        assert!(!src.contains(&needle2));
    }
}

#[cfg(test)]
mod contract_tests {
    /// The event and command names are a contract across the process boundary
    /// that no compiler checks; pin the TS literals against the Rust ones.
    #[test]
    fn meeting_detect_event_names_match_the_pill() {
        let ts = include_str!("../../src/pill/meetingOffer.ts");
        for (rust, ts_const) in [
            (super::MEETING_OFFER_EVENT, "MEETING_OFFER_EVENT"),
            (
                super::MEETING_OFFER_CLEAR_EVENT,
                "MEETING_OFFER_CLEAR_EVENT",
            ),
            ("meeting_offer_respond", "MEETING_OFFER_RESPOND"),
        ] {
            let line = format!("export const {ts_const} = \"{rust}\";");
            assert!(ts.contains(&line), "meetingOffer.ts must carry `{line}`");
        }
        let lib = include_str!("lib.rs");
        assert!(lib.contains("fn meeting_offer_respond("));
    }
}
