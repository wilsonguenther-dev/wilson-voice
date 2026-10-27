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
        .unwrap_or("Them")
        .to_string()
}
