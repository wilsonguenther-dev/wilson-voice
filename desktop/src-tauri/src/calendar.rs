//! yap-notetaker-onboard — Connect calendar, step 2 of Set up Notetaker.
//!
//! v1 is Apple Calendar through EventKit, READ-ONLY and on-device. Reading the
//! macOS calendar store also covers every Google / Exchange / iCloud account
//! the user has already added in System Settings → Internet Accounts, which is
//! why no OAuth exists anywhere in Yap (Google / Microsoft direct are "Coming").
//!
//! THREE RULES, each one checked by `tests/calendar_permission_readonly.rs`:
//!   1. READ ONLY. This module asks for read access and counts calendars. It
//!      never calls an EventKit write selector — there is no event or calendar
//!      mutation anywhere in `src/`, and the grep gate fails the build if one
//!      ever appears.
//!   2. OPT-IN. [`request_access`] is called from exactly ONE place: the
//!      `connect_calendar` command, which only the card's Connect calendar
//!      button invokes. Never at launch, never by default, never from another
//!      item. [`status`] never prompts and may be read freely.
//!   3. NEVER CRASH ON A BARE BINARY. TCC terminates a process that asks for
//!      Calendars access without the usage string in its main bundle, and
//!      `tauri dev` / `cargo test` run a bare Mach-O with no Info.plist. So the
//!      request is refused, in words, when the string is missing — the same
//!      guard `mic_auth::request_access` has.
//!
//! "Done" for the card = full access granted AND at least one calendar
//! readable ([`CalendarAccess::connected`]).
//!
//! WF-07 — the menu bar's "Next meeting" / "Today's Meetings" rows read events
//! through `meeting_detect::sys::events_between` (the same read-only fresh
//! store), gated by `meeting_detect::menu_calendar_enabled`: "Use my calendar"
//! on AND full access already granted. Nothing on that path asks for access.

use serde::Serialize;

/// What the card needs to draw the Connect calendar row.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarAccess {
    /// `not_determined` | `restricted` | `denied` | `full_access` |
    /// `write_only` | `unavailable`.
    pub status: String,
    /// Event calendars readable right now. 0 unless `status == full_access`.
    pub calendars: usize,
    /// The step's done condition: full access AND >= 1 readable calendar.
    pub connected: bool,
    /// A sentence for the card when the answer is not "connected".
    pub message: Option<String>,
}

impl CalendarAccess {
    pub fn from_parts(status: &str, calendars: usize) -> Self {
        let connected = status == "full_access" && calendars >= 1;
        let message = match status {
            "full_access" if calendars == 0 => Some(
                "Calendar access is on, but this Mac has no calendars yet. Add an account in System Settings → Internet Accounts.".to_string(),
            ),
            "full_access" => None,
            // yap-test-channel-product-name: the Calendars row is the bundle's own.
            "denied" | "write_only" => Some(crate::permissions::named(
                "Yap can't read your calendars. Turn on Full Access for Yap in System Settings → Privacy & Security → Calendars.",
            )),
            "restricted" => Some(
                "Calendar access is restricted on this Mac (a device policy decides it).".to_string(),
            ),
            "unavailable" => Some(
                "Calendar access needs the installed Yap app — this build has no calendar usage string.".to_string(),
            ),
            _ => None,
        };
        Self {
            status: status.to_string(),
            calendars,
            connected,
            message,
        }
    }
}

/// `EKAuthorizationStatus` → the string the card branches on.
pub fn status_name(raw: isize) -> &'static str {
    match raw {
        0 => "not_determined",
        1 => "restricted",
        2 => "denied",
        // `EKAuthorizationStatusAuthorized` (pre-14) and
        // `EKAuthorizationStatusFullAccess` (14+) are the same value, 3.
        3 => "full_access",
        4 => "write_only",
        _ => "restricted",
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::sel;
    use objc2_foundation::NSString;
    use std::sync::mpsc;
    use std::time::Duration;

    #[link(name = "EventKit", kind = "framework")]
    extern "C" {}

    /// `EKEntityTypeEvent`.
    const ENTITY_EVENT: usize = 0;

    fn store_class() -> Option<&'static AnyClass> {
        AnyClass::get(c"EKEventStore")
    }

    pub fn raw_status() -> Option<isize> {
        let cls = store_class()?;
        // SAFETY: class method, (EKEntityType) -> EKAuthorizationStatus (NSInteger).
        Some(unsafe { msg_send![cls, authorizationStatusForEntityType: ENTITY_EVENT] })
    }

    /// Count readable event calendars on a FRESH store (a store created before
    /// the grant keeps its old, empty view of the database).
    pub fn calendar_count() -> usize {
        let Some(cls) = store_class() else { return 0 };
        // SAFETY: +[EKEventStore new] -> retained EKEventStore.
        let store: Option<Retained<AnyObject>> = unsafe { msg_send![cls, new] };
        let Some(store) = store else { return 0 };
        // SAFETY: -[EKEventStore calendarsForEntityType:] -> NSArray* (non-null).
        let list: *mut AnyObject =
            unsafe { msg_send![&*store, calendarsForEntityType: ENTITY_EVENT] };
        if list.is_null() {
            return 0;
        }
        // SAFETY: -[NSArray count] -> NSUInteger.
        unsafe { msg_send![list, count] }
    }

    fn bundle_has(key: &str) -> bool {
        let Some(cls) = AnyClass::get(c"NSBundle") else {
            return false;
        };
        // SAFETY: +[NSBundle mainBundle] -> NSBundle* (may be nil).
        let bundle: *mut AnyObject = unsafe { msg_send![cls, mainBundle] };
        if bundle.is_null() {
            return false;
        }
        let key = NSString::from_str(key);
        // SAFETY: -[NSBundle objectForInfoDictionaryKey:] -> id (may be nil).
        let value: *mut AnyObject = unsafe { msg_send![bundle, objectForInfoDictionaryKey: &*key] };
        !value.is_null()
    }

    /// Show the system's Calendars prompt (only ever from `not_determined`) and
    /// block THIS worker thread until the user answers or `wait` elapses.
    /// Returns `false` without asking when the usage string is absent.
    pub fn request_full_access(wait: Duration) -> bool {
        let Some(cls) = store_class() else {
            return false;
        };
        let full = sel!(requestFullAccessToEventsWithCompletion:);
        // SAFETY: +[EKEventStore new] -> retained EKEventStore.
        let store: Option<Retained<AnyObject>> = unsafe { msg_send![cls, new] };
        let Some(store) = store else { return false };
        // SAFETY: -[NSObject respondsToSelector:] -> BOOL.
        let has_full: bool = unsafe { msg_send![&*store, respondsToSelector: full] };
        let key = if has_full {
            "NSCalendarsFullAccessUsageDescription"
        } else {
            "NSCalendarsUsageDescription"
        };
        if !bundle_has(key) {
            log::warn!(
                "calendar: {key} is not in the main bundle (un-bundled dev/test run). \
                 Not asking — asking without it makes TCC kill the process."
            );
            return false;
        }
        let (tx, rx) = mpsc::channel::<bool>();
        let handler = RcBlock::new(move |granted: Bool, _error: *mut AnyObject| {
            let _ = tx.send(granted.as_bool());
        });
        // SAFETY: both selectors take a completion block that EventKit copies;
        // `store` stays alive on this thread until we stop waiting below.
        unsafe {
            if has_full {
                let _: () = msg_send![&*store, requestFullAccessToEventsWithCompletion: &*handler];
            } else {
                let _: () = msg_send![
                    &*store,
                    requestAccessToEntityType: ENTITY_EVENT,
                    completion: &*handler,
                ];
            }
        }
        rx.recv_timeout(wait).unwrap_or(false)
    }

    pub fn usage_string_present() -> bool {
        bundle_has("NSCalendarsFullAccessUsageDescription")
            || bundle_has("NSCalendarsUsageDescription")
    }
}

#[cfg(not(target_os = "macos"))]
mod sys {
    use std::time::Duration;
    pub fn raw_status() -> Option<isize> {
        None
    }
    pub fn calendar_count() -> usize {
        0
    }
    pub fn request_full_access(_wait: Duration) -> bool {
        false
    }
    pub fn usage_string_present() -> bool {
        false
    }
}

/// Where calendar access stands. NEVER prompts.
pub fn status() -> CalendarAccess {
    let Some(raw) = sys::raw_status() else {
        return CalendarAccess::from_parts("unavailable", 0);
    };
    let name = status_name(raw);
    let count = if name == "full_access" {
        sys::calendar_count()
    } else {
        0
    };
    if name == "not_determined" && !sys::usage_string_present() {
        return CalendarAccess::from_parts("unavailable", 0);
    }
    CalendarAccess::from_parts(name, count)
}

/// Ask macOS for READ access to calendars, then report. Blocks the calling
/// thread while the system prompt is up, so call it from a worker. The ONLY
/// caller is the `connect_calendar` command (rule 2 above).
pub fn request_access() -> CalendarAccess {
    let before = status();
    if before.status != "not_determined" {
        // macOS shows the prompt only from not_determined; anything else is
        // already answered and the next step is System Settings.
        return before;
    }
    let _ = sys::request_full_access(std::time::Duration::from_secs(300));
    status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_done_needs_full_access_and_a_calendar() {
        assert!(CalendarAccess::from_parts("full_access", 2).connected);
        assert!(!CalendarAccess::from_parts("full_access", 0).connected);
        assert!(!CalendarAccess::from_parts("write_only", 3).connected);
        assert!(!CalendarAccess::from_parts("denied", 0).connected);
        assert_eq!(status_name(3), "full_access");
        assert_eq!(status_name(4), "write_only");
        assert_eq!(status_name(0), "not_determined");
    }
}

/// One attendee of the calendar event a meeting was matched to (yap24-NT4 R5).
/// Only ever read from the opt-in calendar match; never from Accessibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarAttendee {
    /// What the calendar shows for this person ("Jordan Lee"); may be empty.
    pub display_name: String,
    /// The attendee's address, as the calendar stores it (`mailto:` or bare).
    pub address: String,
}

fn bare_address(address: &str) -> String {
    let a = address.trim();
    let a = a
        .strip_prefix("mailto:")
        .or_else(|| a.strip_prefix("MAILTO:"))
        .unwrap_or(a);
    a.trim().to_ascii_lowercase()
}

/// yap24-NT4 R5 — the v1 remote-name path. The display name of the OTHER
/// party when the event has exactly one attendee who is not the user, and
/// `None` for zero or two-plus (a group call's far track is not one person,
/// so the notes must not pick one of them). `self_addresses` are the user's
/// own addresses; they are excluded however the calendar spells them. The
/// same address listed twice is one person. A lone other attendee with no
/// display name is `None`: an address is not a name to show in the notes.
///
/// A Them-track item's owner in the notes uses this name when it is `Some`
/// and reads "Them" otherwise; transcript labels stay Me / Them.
pub fn other_party_name(attendees: &[CalendarAttendee], self_addresses: &[&str]) -> Option<String> {
    let mine: Vec<String> = self_addresses.iter().map(|a| bare_address(a)).collect();
    let mut others: Vec<&CalendarAttendee> = Vec::new();
    for a in attendees {
        let addr = bare_address(&a.address);
        if addr.is_empty() || mine.contains(&addr) {
            continue;
        }
        if others.iter().any(|o| bare_address(&o.address) == addr) {
            continue;
        }
        others.push(a);
    }
    match others.as_slice() {
        [one] => {
            let name = one.display_name.trim();
            (!name.is_empty()).then(|| name.to_string())
        }
        _ => None,
    }
}

/// The owner label a Them-track notes item carries: the other party's name
/// from [`other_party_name`] when there is one, else "Them".
pub fn them_owner_label(other_party: Option<&str>) -> String {
    other_party
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(crate::meetings::SYSTEM_SPEAKER_LABEL)
        .to_string()
}

// ─── WF-08 — the pill's Meeting popover ─────────────────────────────────────
//
// The rail's Meeting button opens a small popover: today's next meetings with
// a Record row each, "Record now", and — when the calendar is not in use —
// "No upcoming meetings" plus "Connect calendar". It reads the SAME snapshot
// the menu bar's "Today's Meetings" reads (`meeting_detect::upcoming_today` →
// `publish_upcoming` → `upcoming_snapshot`), so the two can never disagree and
// the list is decided by one tested function. Nothing here reads EventKit or
// asks for access: "Connect calendar" only NAVIGATES to the setup card, whose
// own button stays the one place access is requested (rule 2 above).

/// At most this many of today's meetings are listed in the pill's popover.
pub const POPOVER_MAX: usize = 3;
/// The popover's line when nothing is coming up (or the calendar is off).
pub const POPOVER_EMPTY_LABEL: &str = "No upcoming meetings";
/// The popover's way to the setup card when the calendar is not in use.
pub const POPOVER_CONNECT_LABEL: &str = "Connect calendar";
/// Start a recording right now, no calendar event needed.
pub const POPOVER_RECORD_NOW_LABEL: &str = "Record now";
/// A row whose title the user hid in the menu bar, or an untitled event.
pub const POPOVER_UNTITLED_LABEL: &str = "Meeting";
/// `meeting_popover_record` for a row that left today's list after it opened.
pub const POPOVER_STALE_ERROR: &str =
    "That meeting is no longer on today's list — use Record now instead.";

/// Which of the three states the popover is in.
pub const POPOVER_STATE_MEETINGS: &str = "meetings";
pub const POPOVER_STATE_EMPTY: &str = "empty";
pub const POPOVER_STATE_NOT_CONNECTED: &str = "not_connected";

/// One Record row of the popover.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PopoverMeeting {
    /// The occurrence key (`meeting_detect::UpcomingMeeting::key`) handed back
    /// to `meeting_popover_record`; Record snoozes the pill offer for it.
    pub key: String,
    /// The event title, or [`POPOVER_UNTITLED_LABEL`] when it is hidden/empty.
    pub label: String,
    /// "2:00 PM", the same text the menu bar shows.
    pub clock: String,
    /// Carries a conferencing link (recorded as Virtual).
    pub has_link: bool,
}

/// Where "Connect calendar" goes: the main window's `navigate` payload and,
/// for Settings, the `settings-tab` payload.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PopoverConnect {
    pub nav: String,
    pub tab: Option<String>,
}

/// Everything the popover draws, decided from ONE snapshot.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingPopover {
    /// [`POPOVER_STATE_MEETINGS`] | [`POPOVER_STATE_EMPTY`] | [`POPOVER_STATE_NOT_CONNECTED`].
    pub state: String,
    /// At most [`POPOVER_MAX`], earliest first. Empty unless state is "meetings".
    pub meetings: Vec<PopoverMeeting>,
    /// [`POPOVER_EMPTY_LABEL`] when there is nothing to list.
    pub empty_text: Option<String>,
    /// [`POPOVER_CONNECT_LABEL`] when state is "not_connected".
    pub connect_text: Option<String>,
    /// Where "Connect calendar" opens. `Some` exactly when `connect_text` is.
    pub connect: Option<PopoverConnect>,
    /// Always [`POPOVER_RECORD_NOW_LABEL`] — Record now is in every state.
    pub record_now_text: String,
}

/// THE decision for the pill's Meeting popover.
///
/// * `snap` — `meeting_detect::upcoming_snapshot()`: `None` = the calendar is
///   not in use ("Use my calendar" off, or no full access), the same meaning
///   the menu bar gives it.
/// * `now_unix` — rows that started more than `IN_PROGRESS_SECS` ago are
///   dropped, so a snapshot that is a few minutes old never offers a meeting
///   the menu bar would no longer list.
/// * `setup_card_offers_connect` — the Meetings tab's setup card still shows
///   its Connect calendar step (not dismissed, step not done). When it does,
///   "Connect calendar" opens that card; otherwise it opens Settings →
///   Meetings, where "Use my calendar" and the access line live.
///
/// Titles follow the menu bar's "Hide Meeting Titles" toggle: the pill is on
/// screen during calls too.
pub fn meeting_popover(
    snap: Option<&crate::meeting_detect::UpcomingSnapshot>,
    now_unix: i64,
    setup_card_offers_connect: bool,
) -> MeetingPopover {
    let record_now_text = POPOVER_RECORD_NOW_LABEL.to_string();
    let Some(snap) = snap else {
        let connect = if setup_card_offers_connect {
            PopoverConnect {
                nav: "meetings".to_string(),
                tab: None,
            }
        } else {
            PopoverConnect {
                nav: "settings".to_string(),
                tab: Some("meetings".to_string()),
            }
        };
        return MeetingPopover {
            state: POPOVER_STATE_NOT_CONNECTED.to_string(),
            meetings: Vec::new(),
            empty_text: Some(POPOVER_EMPTY_LABEL.to_string()),
            connect_text: Some(POPOVER_CONNECT_LABEL.to_string()),
            connect: Some(connect),
            record_now_text,
        };
    };
    let meetings: Vec<PopoverMeeting> = snap
        .meetings
        .iter()
        .filter(|m| m.start_unix > now_unix - crate::meeting_detect::IN_PROGRESS_SECS)
        .take(POPOVER_MAX)
        .map(|m| PopoverMeeting {
            key: m.key.clone(),
            label: if snap.hide_titles {
                None
            } else {
                m.title.clone()
            }
            .unwrap_or_else(|| POPOVER_UNTITLED_LABEL.to_string()),
            clock: crate::tray_menu::meeting_clock(m.start_unix, snap.utc_offset_secs),
            has_link: m.has_link,
        })
        .collect();
    let empty = meetings.is_empty();
    MeetingPopover {
        state: if empty {
            POPOVER_STATE_EMPTY
        } else {
            POPOVER_STATE_MEETINGS
        }
        .to_string(),
        meetings,
        empty_text: empty.then(|| POPOVER_EMPTY_LABEL.to_string()),
        connect_text: None,
        connect: None,
        record_now_text,
    }
}

/// `meeting_popover_record` — the meeting behind a row, looked up by its key
/// in the SAME snapshot the popover was drawn from. `None` = the row went
/// stale (the day moved on, or the calendar was turned off meanwhile).
pub fn popover_meeting_for(
    snap: Option<&crate::meeting_detect::UpcomingSnapshot>,
    key: &str,
) -> Option<crate::meeting_detect::UpcomingMeeting> {
    snap?.meetings.iter().find(|m| m.key == key).cloned()
}

#[cfg(test)]
mod popover_tests {
    //! WF-08 — the three states of the pill's Meeting popover, from the ONE
    //! snapshot WF-07's menu bar reads.
    use super::*;
    use crate::meeting_detect::{UpcomingMeeting, UpcomingSnapshot};

    /// 2026-11-06 14:00:00 UTC.
    const TWO_PM: i64 = 1_794_060_000;

    fn meeting(key: &str, title: Option<&str>, start: i64, link: bool) -> UpcomingMeeting {
        UpcomingMeeting {
            key: key.to_string(),
            title: title.map(str::to_string),
            start_unix: start,
            has_link: link,
        }
    }

    fn snap(hide_titles: bool, meetings: Vec<UpcomingMeeting>) -> UpcomingSnapshot {
        UpcomingSnapshot {
            meetings,
            hide_titles,
            utc_offset_secs: 0,
        }
    }

    #[test]
    fn popover_connected_with_meetings_lists_next_three_with_record_rows() {
        let s = snap(
            false,
            vec![
                meeting("cal:a:1", Some("Standup"), TWO_PM, true),
                meeting("cal:b:2", None, TWO_PM + 1800, false),
                meeting("cal:c:3", Some("  Design review "), TWO_PM + 3600, false),
                meeting("cal:d:4", Some("Fourth"), TWO_PM + 5400, false),
            ],
        );
        let p = meeting_popover(Some(&s), TWO_PM - 600, true);
        assert_eq!(p.state, POPOVER_STATE_MEETINGS);
        assert_eq!(p.meetings.len(), POPOVER_MAX);
        assert_eq!(p.meetings[0].label, "Standup");
        assert_eq!(p.meetings[0].clock, "2:00 PM");
        assert!(p.meetings[0].has_link);
        assert_eq!(p.meetings[0].key, "cal:a:1");
        assert_eq!(p.meetings[1].label, POPOVER_UNTITLED_LABEL);
        assert_eq!(p.meetings[1].clock, "2:30 PM");
        assert_eq!(p.meetings[2].key, "cal:c:3");
        assert_eq!(p.empty_text, None);
        assert_eq!(p.connect_text, None);
        assert_eq!(p.connect, None);
        assert_eq!(p.record_now_text, POPOVER_RECORD_NOW_LABEL);
    }

    #[test]
    fn popover_hides_titles_when_the_menu_bar_does() {
        let s = snap(
            true,
            vec![meeting("cal:a:1", Some("Standup"), TWO_PM, false)],
        );
        let p = meeting_popover(Some(&s), TWO_PM - 60, false);
        assert_eq!(p.meetings[0].label, POPOVER_UNTITLED_LABEL);
        assert_eq!(p.meetings[0].clock, "2:00 PM");
    }

    #[test]
    fn popover_connected_and_empty_says_no_upcoming_meetings() {
        let p = meeting_popover(Some(&snap(false, Vec::new())), TWO_PM, true);
        assert_eq!(p.state, POPOVER_STATE_EMPTY);
        assert!(p.meetings.is_empty());
        assert_eq!(p.empty_text.as_deref(), Some(POPOVER_EMPTY_LABEL));
        assert_eq!(p.connect, None, "a connected calendar never offers Connect");
        assert_eq!(p.record_now_text, POPOVER_RECORD_NOW_LABEL);

        // A stale snapshot whose only meeting is long over is "empty" too.
        let old = snap(
            false,
            vec![meeting("cal:a:1", Some("Earlier"), TWO_PM, false)],
        );
        let p = meeting_popover(
            Some(&old),
            TWO_PM + crate::meeting_detect::IN_PROGRESS_SECS + 1,
            true,
        );
        assert_eq!(p.state, POPOVER_STATE_EMPTY);
        // ...while one that just began is still recordable.
        let p = meeting_popover(Some(&old), TWO_PM + 60, true);
        assert_eq!(p.state, POPOVER_STATE_MEETINGS);
    }

    #[test]
    fn popover_not_connected_offers_connect_calendar_to_the_setup_card() {
        let p = meeting_popover(None, TWO_PM, true);
        assert_eq!(p.state, POPOVER_STATE_NOT_CONNECTED);
        assert!(p.meetings.is_empty());
        assert_eq!(p.empty_text.as_deref(), Some(POPOVER_EMPTY_LABEL));
        assert_eq!(p.connect_text.as_deref(), Some(POPOVER_CONNECT_LABEL));
        assert_eq!(
            p.connect,
            Some(PopoverConnect {
                nav: "meetings".to_string(),
                tab: None
            })
        );
        assert_eq!(p.record_now_text, POPOVER_RECORD_NOW_LABEL);
        // The card was dismissed or its calendar step is done: Settings → Meetings.
        let p = meeting_popover(None, TWO_PM, false);
        assert_eq!(
            p.connect,
            Some(PopoverConnect {
                nav: "settings".to_string(),
                tab: Some("meetings".to_string())
            })
        );
    }

    #[test]
    fn popover_reads_the_same_upcoming_today_list_as_the_menu_bar() {
        // ONE data source: the snapshot WF-07's `upcoming_today` builds from
        // EventKit events. The popover shows the first three of exactly the
        // rows the menu bar lists, in the same order, with the same clock.
        use crate::meeting_detect::{upcoming_today, CalendarEvent};
        let ev = |id: &str, title: &str, start: i64| CalendarEvent {
            id: id.to_string(),
            title: Some(title.to_string()),
            start_unix: start,
            ..Default::default()
        };
        let events = vec![
            ev("late", "Retro", TWO_PM + 7200),
            ev("first", "Standup", TWO_PM),
            ev("past", "Breakfast", TWO_PM - 7200),
            ev("mid", "Design review", TWO_PM + 1800),
            ev("third", "1:1", TWO_PM + 3600),
        ];
        let s = snap(false, upcoming_today(&events, TWO_PM, TWO_PM + 36_000));
        let menu = crate::tray_menu::upcoming_menu(&s);
        let pop = meeting_popover(Some(&s), TWO_PM, true);
        let menu_keys: Vec<&str> = menu.rows.iter().map(|r| r.key.as_str()).collect();
        let pop_keys: Vec<&str> = pop.meetings.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(pop_keys, menu_keys[..POPOVER_MAX].to_vec());
        assert_eq!(
            pop.meetings
                .iter()
                .map(|m| m.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Standup", "Design review", "1:1"]
        );
        // Nothing left today → the same "empty" for both.
        let s = snap(false, upcoming_today(&[], TWO_PM, TWO_PM + 36_000));
        assert!(crate::tray_menu::upcoming_menu(&s).rows.is_empty());
        assert_eq!(
            meeting_popover(Some(&s), TWO_PM, true).state,
            POPOVER_STATE_EMPTY
        );
    }

    #[test]
    fn popover_record_finds_the_row_by_key_or_reports_it_stale() {
        let s = snap(
            false,
            vec![meeting("cal:a:1", Some("Standup"), TWO_PM, true)],
        );
        let m = popover_meeting_for(Some(&s), "cal:a:1").expect("row");
        assert_eq!(m.title.as_deref(), Some("Standup"));
        assert!(m.has_link);
        assert_eq!(popover_meeting_for(Some(&s), "cal:zzz:9"), None);
        assert_eq!(popover_meeting_for(None, "cal:a:1"), None);
    }
}
