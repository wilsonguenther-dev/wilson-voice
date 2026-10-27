//! yap-notetaker-detect — the calendar prompt POLICY, as a table.
//!
//! `meeting_detect::calendar_offer` is the one function that decides whether a
//! calendar event raises "Record this meeting?". These rows are the item's
//! acceptance: link + 2 min → offer; no link → none; snoozed → none; in
//! progress 4 min → offer, 6 min → none. Plus the edges a refactor would move.

use wilson_voice_lib::meeting_detect::{
    calendar_offer, has_conferencing_link, in_window, may_raise, CalendarEvent, DetectSettings,
    MeetingOffer, OfferSource, Snoozes, IN_PROGRESS_SECS, LEAD_SECS,
};

const NOW: i64 = 1_800_000_000;

fn event(id: &str, start_offset: i64, url: Option<&str>) -> CalendarEvent {
    CalendarEvent {
        id: id.into(),
        title: Some(format!("Meeting {id}")),
        start_unix: NOW + start_offset,
        url: url.map(str::to_string),
        location: None,
        notes: None,
    }
}

const ZOOM: &str = "https://us02web.zoom.us/j/81234567890?pwd=abc";

#[test]
fn table() {
    let mut snoozed = Snoozes::default();
    snoozed.snooze(&event("snz", 120, Some(ZOOM)).offer_key());
    let none = Snoozes::default();
    // (name, event, snoozes, expect offer)
    let rows: Vec<(&str, CalendarEvent, &Snoozes, bool)> = vec![
        (
            "link + starts in 2 min → offer",
            event("a", 120, Some(ZOOM)),
            &none,
            true,
        ),
        (
            "link + starts in 2 min 1 s → none",
            event("b", 121, Some(ZOOM)),
            &none,
            false,
        ),
        ("no link → none", event("c", 60, None), &none, false),
        (
            "non-call link → none",
            event("d", 60, Some("https://example.com/agenda")),
            &none,
            false,
        ),
        (
            "snoozed → none",
            event("snz", 120, Some(ZOOM)),
            &snoozed,
            false,
        ),
        (
            "in progress 4 min → offer",
            event("e", -240, Some(ZOOM)),
            &none,
            true,
        ),
        (
            "in progress 6 min → none",
            event("f", -360, Some(ZOOM)),
            &none,
            false,
        ),
        ("starts now → offer", event("g", 0, Some(ZOOM)), &none, true),
        (
            "starts in 10 min → none",
            event("h", 600, Some(ZOOM)),
            &none,
            false,
        ),
    ];
    for (name, ev, sn, expect) in rows {
        let got = calendar_offer(std::slice::from_ref(&ev), NOW, sn);
        assert_eq!(got.is_some(), expect, "{name}: got {got:?}");
        if let Some(o) = got {
            assert_eq!(o.source, OfferSource::Calendar, "{name}");
            assert_eq!(o.key, ev.offer_key(), "{name}");
            assert_eq!(o.title.as_deref(), ev.title.as_deref(), "{name}");
        }
    }
}

#[test]
fn window_edges_match_the_constants() {
    assert_eq!(LEAD_SECS, 120);
    assert_eq!(IN_PROGRESS_SECS, 300);
    assert!(in_window(NOW + LEAD_SECS, NOW));
    assert!(!in_window(NOW + LEAD_SECS + 1, NOW));
    assert!(in_window(NOW - IN_PROGRESS_SECS + 1, NOW));
    assert!(
        !in_window(NOW - IN_PROGRESS_SECS, NOW),
        "5 min in progress is past the window"
    );
}

#[test]
fn every_named_provider_matches_in_url_location_or_notes() {
    for link in [
        "https://zoom.us/j/123",
        "https://meet.google.com/abc-defg-hij",
        "https://teams.microsoft.com/l/meetup-join/19%3a",
        "https://facetime.apple.com/join#v=1&p=xyz",
        "https://whereby.com/team-room",
        "https://acme.webex.com/meet/pat",
        "HTTPS://MEET.GOOGLE.COM/ABC",
    ] {
        assert!(has_conferencing_link(link), "{link}");
        let mut loc = event("loc", 30, None);
        loc.location = Some(format!("Room 4 / {link}"));
        assert!(
            calendar_offer(&[loc], NOW, &Snoozes::default()).is_some(),
            "location: {link}"
        );
        let mut notes = event("notes", 30, None);
        notes.notes = Some(format!("Join here:\n{link}\nAgenda below"));
        assert!(
            calendar_offer(&[notes], NOW, &Snoozes::default()).is_some(),
            "notes: {link}"
        );
    }
    assert!(!has_conferencing_link("zoom in on the chart"));
}

#[test]
fn the_closest_offerable_event_wins_and_snoozing_it_surfaces_the_next() {
    let early = event("early", -200, Some(ZOOM));
    let soon = event("soon", 30, Some(ZOOM));
    let events = vec![early.clone(), soon.clone()];
    let first = calendar_offer(&events, NOW, &Snoozes::default()).unwrap();
    assert_eq!(first.key, soon.offer_key());
    let mut sn = Snoozes::default();
    sn.snooze(&first.key);
    let second = calendar_offer(&events, NOW, &sn).unwrap();
    assert_eq!(
        second.key,
        early.offer_key(),
        "Not now never repeats the same event"
    );
}

#[test]
fn a_recurring_meetings_next_occurrence_is_a_new_offer() {
    let today = event("weekly", 60, Some(ZOOM));
    let mut sn = Snoozes::default();
    sn.snooze(&today.offer_key());
    let next_week = CalendarEvent {
        start_unix: today.start_unix + 7 * 86_400,
        ..today.clone()
    };
    assert_ne!(today.offer_key(), next_week.offer_key());
    assert!(calendar_offer(&[next_week], NOW + 7 * 86_400, &sn).is_some());
}

#[test]
fn may_raise_respects_the_toggle_a_running_meeting_and_snoozes() {
    let offer = MeetingOffer {
        key: "app:zoom:42".into(),
        title: None,
        source: OfferSource::App,
    };
    let on = DetectSettings::default();
    let off = DetectSettings {
        offer_detected: false,
        ..on
    };
    let mut sn = Snoozes::default();
    assert!(may_raise(&on, false, &sn, &offer));
    assert!(
        !may_raise(&off, false, &sn, &offer),
        "toggle off → never offered"
    );
    assert!(
        !may_raise(&on, true, &sn, &offer),
        "already recording → no offer"
    );
    sn.snooze(&offer.key);
    assert!(
        !may_raise(&on, false, &sn, &offer),
        "Not now holds for the app session"
    );
}

#[test]
fn a_blank_title_is_no_title() {
    let mut ev = event("blank", 10, Some(ZOOM));
    ev.title = Some("   ".into());
    let o = calendar_offer(&[ev], NOW, &Snoozes::default()).unwrap();
    assert_eq!(o.title, None);
}

/// #272 fix round 1 (BLOCKING, failure polarity) — Record must START before it
/// clears or snoozes the offer. Clearing first unmounted the card, so a failed
/// start (no engine, mic error, "still starting up") was swallowed and the
/// occurrence was snoozed for good.
#[test]
fn record_answer_starts_before_it_clears_or_snoozes_the_offer() {
    let src = include_str!("../src/lib.rs");
    let body = &src[src.find("fn meeting_offer_respond(").unwrap()..];
    let body = &body[..body.find("\n}\n").unwrap()];
    let head = &body[..body.find("match choice").unwrap()];
    assert!(
        !head.contains("meeting_detect::respond(") && !head.contains("meeting_detect::emit_clear("),
        "nothing may snooze or clear the offer before the answer is acted on"
    );
    let rec = &body[body.find("OfferChoice::Record =>").unwrap()
        ..body.find("OfferChoice::Snooze =>").unwrap()];
    let start = rec.find("begin_meeting(").expect("Record starts a meeting");
    let start_err = rec[start..]
        .find(")?;")
        .expect("a failed start returns its error first");
    for later in ["meeting_detect::respond(", "meeting_detect::emit_clear("] {
        let at = rec
            .find(later)
            .unwrap_or_else(|| panic!("Record arm calls {later}"));
        assert!(
            at > start + start_err,
            "{later} must come after begin_meeting(...)?"
        );
    }
}
