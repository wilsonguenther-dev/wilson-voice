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

// ─── WF-07: upcoming meetings in the menu bar ───────────────────────────────

/// `settings_kv` — the tray's "Hide meeting titles" toggle (default OFF). When
/// on, the menu says "Next meeting 2:00 PM" instead of naming the event.
pub const HIDE_MEETING_TITLES_KEY: &str = "meeting_menu_hide_titles_v1";
/// At most this many of today's meetings are listed in the tray submenu.
pub const UPCOMING_MAX: usize = 8;

/// The menu bar reads the calendar only when "Use my calendar" is on AND
/// EventKit already reports full access. Unlike [`calendar_signal_enabled`] it
/// does not need "Offer to record": listing today's meetings never raises an
/// offer. It never asks for access.
pub fn menu_calendar_enabled(s: &DetectSettings, calendar_full_access: bool) -> bool {
    s.use_calendar && calendar_full_access
}

pub fn load_hide_titles(db: &Database) -> bool {
    flag(db.setting_get(HIDE_MEETING_TITLES_KEY), false)
}

/// Persist the tray toggle and wake the watcher so the menu is re-published.
pub fn save_hide_titles(db: &Database, on: bool) -> Result<(), String> {
    db.setting_set(HIDE_MEETING_TITLES_KEY, flag_str(on))?;
    wake();
    Ok(())
}

/// One of today's remaining meetings, as the tray lists it. Holds only what
/// the menu shows (title, start) plus the occurrence key Record snoozes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpcomingMeeting {
    /// [`CalendarEvent::offer_key`] — Record from the menu snoozes the pill
    /// offer for this same occurrence.
    pub key: String,
    pub title: Option<String>,
    pub start_unix: i64,
    /// Carries a conferencing link (the recording is started as Virtual).
    pub has_link: bool,
}

/// What the watcher hands the tray. `None` from [`upcoming_snapshot`] means
/// the calendar is not in use and the menu shows no calendar rows at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpcomingSnapshot {
    pub meetings: Vec<UpcomingMeeting>,
    pub hide_titles: bool,
    /// Local UTC offset when the snapshot was taken, for the "2:00 PM" text.
    pub utc_offset_secs: i32,
}

/// The Unix second at which the local day containing `now_unix` ends.
pub fn local_day_end(now_unix: i64, utc_offset_secs: i32) -> i64 {
    let off = i64::from(utc_offset_secs);
    ((now_unix + off).div_euclid(86_400) + 1) * 86_400 - off
}

/// Today's remaining meetings: started less than [`IN_PROGRESS_SECS`] ago (so
/// one that just began can still be recorded) through the end of the local
/// day, earliest first, one row per occurrence, at most [`UPCOMING_MAX`].
pub fn upcoming_today(
    events: &[CalendarEvent],
    now_unix: i64,
    day_end_unix: i64,
) -> Vec<UpcomingMeeting> {
    let mut seen = HashSet::new();
    let mut list: Vec<&CalendarEvent> = events
        .iter()
        .filter(|e| e.start_unix > now_unix - IN_PROGRESS_SECS && e.start_unix < day_end_unix)
        .collect();
    list.sort_by(|a, b| a.start_unix.cmp(&b.start_unix).then(a.id.cmp(&b.id)));
    list.into_iter()
        .filter(|e| seen.insert(e.offer_key()))
        .take(UPCOMING_MAX)
        .map(|e| UpcomingMeeting {
            key: e.offer_key(),
            title: e
                .title
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string),
            start_unix: e.start_unix,
            has_link: e.has_link(),
        })
        .collect()
}

type UpcomingSink = Box<dyn Fn(Option<UpcomingSnapshot>) + Send + Sync>;

fn upcoming_sink() -> &'static OnceLock<UpcomingSink> {
    static SINK: OnceLock<UpcomingSink> = OnceLock::new();
    &SINK
}

fn upcoming_last() -> &'static Mutex<Option<Option<UpcomingSnapshot>>> {
    static LAST: Mutex<Option<Option<UpcomingSnapshot>>> = Mutex::new(None);
    &LAST
}

/// `lib.rs` registers the tray's applier ONCE, before [`start_watch`]. The
/// sink is called from the watcher thread, only when the snapshot CHANGED.
pub fn set_upcoming_sink(sink: impl Fn(Option<UpcomingSnapshot>) + Send + Sync + 'static) {
    if upcoming_sink().set(Box::new(sink)).is_err() {
        log::warn!("meeting_detect upcoming_sink_set_twice");
    }
}

/// The last snapshot the watcher published (`None` = calendar not in use or
/// nothing published yet). The tray re-applies it after a rebuild.
pub fn upcoming_snapshot() -> Option<UpcomingSnapshot> {
    upcoming_last()
        .lock()
        .ok()
        .and_then(|l| l.clone())
        .flatten()
}

/// Hand `snap` to the tray if it differs from the last one. Returns whether it
/// changed (the sink, once registered, is called exactly then). A steady
/// calendar mutates the menu zero times.
pub fn publish_upcoming(snap: Option<UpcomingSnapshot>) -> bool {
    {
        let Ok(mut last) = upcoming_last().lock() else {
            return false;
        };
        if last.as_ref() == Some(&snap) {
            return false;
        }
        *last = Some(snap.clone());
    }
    match &snap {
        Some(s) => log::info!(
            "meeting_detect upcoming_published count={} hide_titles={}",
            s.meetings.len(),
            s.hide_titles
        ),
        None => log::info!("meeting_detect upcoming_published calendar=off"),
    }
    if let Some(sink) = upcoming_sink().get() {
        sink(snap);
    }
    true
}

/// The menu half of one watcher wake. Reads EventKit (read-only, a fresh
/// store) ONLY when [`menu_calendar_enabled`]; otherwise publishes "off".
fn evaluate_upcoming(db: &Database, s: &DetectSettings) {
    if !menu_calendar_enabled(s, calendar_full_access()) {
        publish_upcoming(None);
        return;
    }
    sys::keep_change_store_alive();
    let local = chrono::Local::now();
    let now = local.timestamp();
    let offset = local.offset().local_minus_utc();
    let started = std::time::Instant::now();
    let events = sys::events_between(now - IN_PROGRESS_SECS, local_day_end(now, offset));
    crate::diagnostics::record_duration(
        "meeting_detect",
        "upcoming_read_ms",
        started.elapsed().as_millis() as u64,
    );
    publish_upcoming(Some(UpcomingSnapshot {
        meetings: upcoming_today(&events, now, local_day_end(now, offset)),
        hide_titles: load_hide_titles(db),
        utc_offset_secs: offset,
    }));
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
    // WF-07 — the tray's upcoming meetings, refreshed on every wake (EventKit's
    // change notification or the 30 s backstop), also while a meeting records.
    evaluate_upcoming(db, &load_settings(db));
    if meeting_recording {
        clear(app, None);
        // fix-b1-1 — a still-live call offer / chip goes too (a no-op while
        // the recording is the call offer's own): never "Record Zoom call"
        // on screen during a recording.
        crate::call_offer_meeting_started_elsewhere(app, "meeting_recording");
        return;
    }
    let s = load_settings(db);
    let (calendar_raised, hint) = evaluate_calendar(app, db, &s);
    if calendar_raised {
        // fix-b1-1 (A4) — the calendar offer leads: a live call offer is
        // withdrawn, so two offers are never on screen at once.
        crate::call_offer_meeting_started_elsewhere(app, "calendar_offer");
    } else {
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
        crate::on_call_sighting(app, None);
        return;
    }
    #[cfg(target_os = "macos")]
    let (snapshot, snapshot_ms) = {
        call_signals::sys::sync_listeners();
        let started = std::time::Instant::now();
        let snap = call_signals::sys::snapshot(
            call_signals::catalog(),
            call_signals::AxSource::Yap,
            false,
            calendar,
        );
        (snap, Some(started.elapsed().as_millis() as u64))
    };
    #[cfg(not(target_os = "macos"))]
    let (snapshot, snapshot_ms): (Result<call_signals::CallSnapshot, String>, Option<u64>) = {
        let _ = calendar;
        (Err("call detection is macOS-only".to_string()), None)
    };
    // SP-11 — the duration stat is kept on every snapshot; its log line is
    // written only when the snapshot was slow or its result changed.
    let record_ms = |changed: bool| {
        if let Some(ms) = snapshot_ms {
            crate::diagnostics::record_duration_quiet(
                "call_signals",
                "snapshot_ms",
                ms,
                call_signals::snapshot_duration_due(ms, changed),
            );
        }
    };
    let (snap, recovered) = match snapshot {
        Ok(snap) => (snap, calls.last_error.take().is_some()),
        Err(e) => {
            let changed = calls.last_error.as_deref() != Some(e.as_str());
            record_ms(changed);
            if changed {
                log::warn!("call_signals snapshot_failed error={e:?}");
                calls.last_error = Some(e);
            }
            clear(app, Some(OfferSource::App));
            crate::on_call_sighting(app, None);
            return;
        }
    };
    let candidate = call_signals::score(&snap, call_signals::catalog(), &calls.memory);
    calls.memory.observe(candidate.as_ref());
    let logged = candidate
        .as_ref()
        .map(|c| (c.bundle_id.clone(), c.offers()));
    record_ms(recovered || logged != calls.last_logged);
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
    // yap-call-1 — an app/browser call goes to the call offer (its own,
    // content-protected window: countdown, chip, Never per app). The pill's
    // MeetingOffer slot keeps the calendar offer only.
    let _ = db;
    clear(app, Some(OfferSource::App));
    crate::on_call_sighting(
        app,
        candidate
            .filter(|c| c.offers())
            .map(|c| crate::call_offer::Sighting::from_candidate(&c, call_signals::catalog())),
    );
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
                // WF-07: the same 30 s timer also runs while the menu lists
                // today's meetings, so a meeting that passed drops off. Never
                // faster than POLL_INTERVAL.
                let wake_settings = load_settings(&db);
                let woke = if crate::call_signals::timer_enabled(wake_settings.offer_detected)
                    || menu_calendar_enabled(&wake_settings, calendar_full_access())
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

    /// WF-07 — EventKit posts `EKEventStoreChangedNotification` only while
    /// some EKEventStore instance is alive, and every read above uses a
    /// short-lived fresh store. Keep ONE store for the rest of the process so
    /// the menu (and the offer) refresh on a calendar edit instead of waiting
    /// for the 30 s backstop. Called ONLY after the "Use my calendar" + full
    /// access gate passed (`evaluate_upcoming`), so with the calendar off Yap
    /// never touches EventKit at all; created after the grant, so its view is
    /// live. Never prompts; never read from or written to. Once per process.
    pub fn keep_change_store_alive() {
        static KEPT: std::sync::Once = std::sync::Once::new();
        KEPT.call_once(|| {
            let Some(cls) = store_class() else {
                return;
            };
            // SAFETY: +[EKEventStore new] -> retained EKEventStore.
            let keep: Option<Retained<AnyObject>> = unsafe { msg_send![cls, new] };
            log::info!("meeting_detect change_store kept={}", keep.is_some());
            std::mem::forget(keep);
        });
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
    pub fn keep_change_store_alive() {}
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

#[cfg(test)]
mod upcoming_tests {
    //! WF-07 — the tray's upcoming meetings: what is listed, when the
    //! calendar may be read at all, and how often the menu can change.
    use super::*;

    fn ev(id: &str, title: Option<&str>, start: i64, url: Option<&str>) -> CalendarEvent {
        CalendarEvent {
            id: id.to_string(),
            title: title.map(str::to_string),
            start_unix: start,
            url: url.map(str::to_string),
            location: None,
            notes: None,
        }
    }

    #[test]
    fn upcoming_today_lists_remaining_meetings_in_order() {
        let now = 1_800_000_000;
        let end = now + 6 * 3600;
        let events = vec![
            ev("late", Some("Retro"), now + 3 * 3600, None),
            ev("past", Some("Breakfast"), now - 3600, None),
            ev(
                "soon",
                Some("  Standup  "),
                now + 600,
                Some("https://zoom.us/j/1"),
            ),
            ev("just", None, now - 60, None),
            ev("tomorrow", Some("Planning"), end + 60, None),
            // The same occurrence twice (two calendars sharing it) is one row.
            ev("soon", Some("Standup"), now + 600, None),
        ];
        let list = upcoming_today(&events, now, end);
        let keys: Vec<&str> = list.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                format!("cal:just:{}", now - 60),
                format!("cal:soon:{}", now + 600),
                format!("cal:late:{}", now + 3 * 3600),
            ]
        );
        assert_eq!(list[0].title, None, "an untitled event stays untitled");
        assert_eq!(list[1].title.as_deref(), Some("Standup"));
        assert!(list[1].has_link);
        assert!(!list[2].has_link);
        // Capped.
        let many: Vec<CalendarEvent> = (0..20)
            .map(|i| ev(&format!("e{i}"), Some("M"), now + 60 * (i + 1), None))
            .collect();
        assert_eq!(upcoming_today(&many, now, end).len(), UPCOMING_MAX);
        assert!(upcoming_today(&[], now, end).is_empty());
    }

    #[test]
    fn upcoming_local_day_end_respects_the_offset() {
        // A UTC noon.
        let noon_utc = 1_800_014_400 - (1_800_014_400 % 86_400) + 43_200;
        let midnight_utc = noon_utc - 43_200 + 86_400;
        assert_eq!(local_day_end(noon_utc, 0), midnight_utc);
        // UTC-6: local noon-6h = 06:00 local; the day ends at 06:00Z next day.
        assert_eq!(local_day_end(noon_utc, -6 * 3600), midnight_utc + 6 * 3600);
        // UTC+9: 21:00 local; the day ends at 15:00Z today.
        assert_eq!(local_day_end(noon_utc, 9 * 3600), midnight_utc - 9 * 3600);
        assert!(local_day_end(noon_utc, -6 * 3600) > noon_utc);
    }

    /// Acceptance: EventKit is read only when "Use my calendar" is on (and
    /// access is already granted). "Offer to record" does not matter here.
    #[test]
    fn upcoming_menu_reads_calendar_only_when_use_my_calendar_is_on() {
        for offer_detected in [true, false] {
            let off = DetectSettings {
                offer_detected,
                use_calendar: false,
            };
            assert!(!menu_calendar_enabled(&off, true));
            assert!(!menu_calendar_enabled(&off, false));
            let on = DetectSettings {
                offer_detected,
                use_calendar: true,
            };
            assert!(menu_calendar_enabled(&on, true));
            assert!(!menu_calendar_enabled(&on, false), "no grant, no read");
        }
        // The reader is gated: evaluate_upcoming publishes "off" BEFORE any
        // events_between call when the gate is shut.
        let src = include_str!("meeting_detect.rs");
        let body_start = src
            .find(&["fn evaluate", "_upcoming(db"].concat())
            .expect("evaluate_upcoming");
        let body = &src[body_start..];
        let gate = body.find("menu_calendar_enabled(").expect("gate");
        let read = body.find("sys::events_between(").expect("read");
        assert!(gate < read, "the gate must come before the EventKit read");
        assert!(body[..read].contains("publish_upcoming(None)"));
        // The change-notification store is created behind the same gate, never
        // at watcher start: with the calendar off Yap does not touch EventKit.
        let keep = body
            .find("sys::keep_change_store_alive(")
            .expect("keep-alive call");
        assert!(gate < keep && keep < read);
        let obs_start = src
            .find(&["pub fn observe", "_changes(tx"].concat())
            .expect("observe_changes");
        let obs = &src[obs_start..];
        let obs = &obs[..obs.find("\n    }\n").expect("observe_changes end")];
        assert!(
            !obs.contains(&["msg_send![cls, ", "new]"].concat()),
            "observe_changes must not create an EKEventStore"
        );
        // Default: the calendar is off.
        assert!(!DetectSettings::default().use_calendar);
    }

    /// Acceptance: the menu refreshes on EventKit's change notification, and
    /// the only timer is the 30 s backstop.
    #[test]
    fn upcoming_menu_refresh_is_event_driven_never_faster_than_30s() {
        assert!(POLL_INTERVAL >= Duration::from_secs(30));
        let src = include_str!("meeting_detect.rs");
        assert!(src.contains("\"EKEventStoreChangedNotification\""));
        let eval = &src[src.find("fn evaluate(app").expect("evaluate")..];
        let upcoming = eval
            .find("evaluate_upcoming(")
            .expect("menu evaluated per wake");
        let early = eval.find("if meeting_recording").expect("early return");
        assert!(
            upcoming < early,
            "the menu refreshes while a meeting records"
        );
        let timer = ["recv_", "timeout("].concat();
        let timed = [timer.as_str(), "POLL_INTERVAL)"].concat();
        assert_eq!(
            src.matches(&timer).count(),
            src.matches(&timed).count(),
            "every timed wait is POLL_INTERVAL"
        );
    }

    #[test]
    fn upcoming_publish_only_on_change_and_hide_titles_persists() {
        let snap = UpcomingSnapshot {
            meetings: vec![UpcomingMeeting {
                key: "cal:x:1".into(),
                title: Some("Standup".into()),
                start_unix: 1,
                has_link: true,
            }],
            hide_titles: false,
            utc_offset_secs: 0,
        };
        publish_upcoming(Some(snap.clone()));
        assert_eq!(upcoming_snapshot(), Some(snap.clone()));
        assert!(
            !publish_upcoming(Some(snap.clone())),
            "same snapshot, no change"
        );
        assert!(publish_upcoming(None));
        assert_eq!(upcoming_snapshot(), None);
        assert!(!publish_upcoming(None));
        assert!(publish_upcoming(Some(snap)));

        let dir = std::env::temp_dir().join(format!(
            "yap-meeting-upcoming-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::open(dir.join("wilson_voice.db")).unwrap();
        assert!(!load_hide_titles(&db), "titles show by default");
        save_hide_titles(&db, true).unwrap();
        assert!(load_hide_titles(&db));
        save_hide_titles(&db, false).unwrap();
        assert!(!load_hide_titles(&db));
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
