//! yap24-NT4 R5 — the v1 remote-name path: `other_party_name` returns the
//! display name only when exactly one non-self calendar attendee exists.

use wilson_voice_lib::calendar::{other_party_name, them_owner_label, CalendarAttendee};

fn att(name: &str, address: &str) -> CalendarAttendee {
    CalendarAttendee { display_name: name.to_string(), address: address.to_string() }
}

const ME: &str = "me@example.test";

#[test]
fn one_other_attendee_gives_their_name() {
    let a = [att("Me Myself", "mailto:ME@example.test"), att("Jordan Lee", "jordan@example.test")];
    assert_eq!(other_party_name(&a, &[ME]).as_deref(), Some("Jordan Lee"));
}

#[test]
fn no_attendees_gives_none() {
    assert_eq!(other_party_name(&[], &[ME]), None);
    // Only the user on the invite is also nobody else.
    assert_eq!(other_party_name(&[att("Me", ME)], &[ME]), None);
}

#[test]
fn three_attendees_give_none() {
    let a = [
        att("Jordan Lee", "jordan@example.test"),
        att("Sam Park", "sam@example.test"),
        att("Alex Kim", "alex@example.test"),
    ];
    assert_eq!(other_party_name(&a, &[ME]), None);
    // Two others besides the user is a group too.
    let b = [att("Me", ME), att("Jordan Lee", "jordan@example.test"), att("Sam Park", "sam@example.test")];
    assert_eq!(other_party_name(&b, &[ME]), None);
}

#[test]
fn the_self_address_is_excluded_however_it_is_spelled() {
    let a = [att("Me", " MAILTO:Me@Example.Test "), att("Jordan Lee", "jordan@example.test")];
    assert_eq!(other_party_name(&a, &[ME]).as_deref(), Some("Jordan Lee"));
    // Without the self address the user counts as an attendee: two others, None.
    assert_eq!(other_party_name(&a, &[]), None);
}

#[test]
fn a_duplicate_entry_is_one_person_and_a_nameless_one_is_none() {
    let dup = [att("Jordan Lee", "jordan@example.test"), att("Jordan", "mailto:jordan@example.test")];
    assert_eq!(other_party_name(&dup, &[ME]).as_deref(), Some("Jordan Lee"));
    assert_eq!(other_party_name(&[att("  ", "x@example.test")], &[ME]), None);
}

#[test]
fn the_them_owner_label_falls_back_to_them() {
    assert_eq!(them_owner_label(Some("Jordan Lee")), "Jordan Lee");
    assert_eq!(them_owner_label(None), "Them");
    assert_eq!(them_owner_label(Some("  ")), "Them");
}
