//! Y2-E — the menu bar, the pill and the Settings card must not be able to
//! disagree about the trial.
//!
//! THE DEFECT THIS TEST EXISTS TO CATCH is not a crash, it is a slow drift:
//! somebody widens the pill's countdown from a week to ten days, the menu bar
//! keeps its own `7`, and for three days Yap shows a countdown in one corner of
//! the screen and nothing in the other. Nothing fails to compile, no runtime
//! error is logged, and the only symptom is a user who no longer believes the
//! app.
//!
//! So the two day thresholds are asserted ACROSS THE LANGUAGE BOUNDARY by
//! reading both source files off disk: `src/license.rs` (Rust, the tray) and
//! `../src/pill/license.ts` (TypeScript, the pill). Both the NUMBERS and the
//! COMPARISON OPERATORS are checked, because `days > PILL_SHOW_DAYS` and
//! `days >= PILL_SHOW_DAYS` are the same constant and a different boundary.
//!
//! `tests/tray_hotkey_no_collision.rs` is the sibling of this file for the tray
//! shortcut table and is untouched by this item.

use wilson_voice_lib::license::{
    tray_line, tray_offers_purchase, Entitlement, LicenseStatus, PILL_SHOW_DAYS, PILL_URGENT_DAYS,
};

const RUST_SRC: &str = include_str!("../src/license.rs");
const TS_SRC: &str = include_str!("../../src/pill/license.ts");

/// Strip comment lines, so an assertion about the CODE cannot be satisfied by a
/// doc comment that merely QUOTES the code.
///
/// This trap is not hypothetical: the first draft of this file passed with the
/// pill's urgency operator flipped from `<` to `<=`, because the doc comment
/// thirty lines above the flipped line quoted the operator the assertion was
/// looking for. A `contains` check needs the right pattern AND the right scope,
/// and prose is not in scope.
fn code_only(src: &str) -> String {
    src.lines()
        .map(str::trim_start)
        .filter(|l| {
            !(l.starts_with("//")
                || l.starts_with("/*")
                || l.starts_with('*')
                || l.starts_with("#["))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pull the integer out of the first line that contains `needle`, ignoring type
/// annotations, `=`, `;` and whitespace. Used on both languages, so the shape of
/// the declaration is allowed to differ while the value is not.
fn declared_number(src: &str, needle: &str, file: &str) -> i64 {
    let stripped = code_only(src);
    let line = stripped
        .lines()
        .find(|l| l.contains(needle) && l.contains('='))
        .unwrap_or_else(|| panic!("{file}: no declaration line containing `{needle}`"));
    let rhs = line
        .split('=')
        .nth(1)
        .unwrap_or_else(|| panic!("{file}: `{needle}` line has no right-hand side: {line}"));
    let digits: String = rhs.chars().filter(|c| c.is_ascii_digit()).collect();
    assert!(
        !digits.is_empty(),
        "{file}: `{needle}` has no number on its right-hand side: {line}"
    );
    digits
        .parse()
        .unwrap_or_else(|e| panic!("{file}: `{needle}` = `{digits}` is not an integer: {e}"))
}

/// A trial status `days_left` days out, with no key stored.
fn trial(days_left: i64) -> LicenseStatus {
    LicenseStatus {
        entitlement: Entitlement::Trial {
            days_left,
            expires_at_ms: 0,
        },
        license_problem: None,
        license_problem_message: None,
        has_stored_license: false,
        trial_days_left: days_left,
        trial_expires_at_ms: 0,
        revocation_checked_at_ms: None,
        revoked_count: 0,
    }
}

fn licensed() -> LicenseStatus {
    LicenseStatus {
        entitlement: Entitlement::Licensed {
            plan: "lifetime".to_string(),
            seats: 1,
            kid: "kid_test".to_string(),
        },
        license_problem: None,
        license_problem_message: None,
        has_stored_license: true,
        trial_days_left: 0,
        trial_expires_at_ms: 0,
        revocation_checked_at_ms: None,
        revoked_count: 0,
    }
}

fn required(reason: &str) -> LicenseStatus {
    LicenseStatus {
        entitlement: Entitlement::LicenseRequired {
            reason: reason.to_string(),
        },
        license_problem: None,
        license_problem_message: None,
        has_stored_license: false,
        trial_days_left: 0,
        trial_expires_at_ms: 0,
        revocation_checked_at_ms: None,
        revoked_count: 0,
    }
}

/// THE ITEM'S OWN LINE: the pill's day boundaries and the tray's are the same
/// boundaries, proven by reading both files rather than by trusting a comment.
#[test]
fn pill_and_tray_share_the_same_day_thresholds() {
    // 1. The numbers, as DECLARED in each language's source.
    let rust_show = declared_number(RUST_SRC, "pub const PILL_SHOW_DAYS", "src/license.rs");
    let rust_urgent = declared_number(RUST_SRC, "pub const PILL_URGENT_DAYS", "src/license.rs");
    let ts_show = declared_number(TS_SRC, "export const PILL_SHOW_DAYS", "src/pill/license.ts");
    let ts_urgent = declared_number(
        TS_SRC,
        "export const PILL_URGENT_DAYS",
        "src/pill/license.ts",
    );

    assert_eq!(
        rust_show, ts_show,
        "PILL_SHOW_DAYS disagrees across the language boundary: Rust {rust_show} vs TS {ts_show}"
    );
    assert_eq!(
        rust_urgent, ts_urgent,
        "PILL_URGENT_DAYS disagrees: Rust {rust_urgent} vs TS {ts_urgent}"
    );

    // 2. The parse agrees with what this binary actually COMPILED. Without this
    //    the test could pass against a constant nobody uses.
    assert_eq!(
        rust_show, PILL_SHOW_DAYS,
        "the parse and the compiled Rust constant disagree"
    );
    assert_eq!(
        rust_urgent, PILL_URGENT_DAYS,
        "the parse and the compiled Rust constant disagree"
    );

    // 3. The OPERATORS. A shared constant with a flipped comparison is still a
    //    drifted boundary — `> 7` and `>= 7` differ by exactly one day, which is
    //    the day a user would see a countdown in one place and not the other.
    //    Searched over CODE ONLY — see `code_only`; the comments in both files
    //    quote these operators on purpose, and an assertion a comment can
    //    satisfy is an assertion that verifies nothing.
    let rust_code = code_only(RUST_SRC);
    let ts_code = code_only(TS_SRC);
    assert!(
        ts_code.contains("days > PILL_SHOW_DAYS"),
        "the pill no longer goes quiet with `days > PILL_SHOW_DAYS`; the tray's \
         `days_left <= PILL_SHOW_DAYS` is now a different boundary"
    );
    assert!(
        ts_code.contains("days < PILL_URGENT_DAYS"),
        "the pill no longer turns urgent with `days < PILL_URGENT_DAYS`; the \
         tray's urgency is now a different boundary"
    );
    assert!(
        rust_code.contains("days < PILL_URGENT_DAYS"),
        "tray_line no longer turns urgent with `days < PILL_URGENT_DAYS`"
    );
    assert!(
        rust_code.contains("<= PILL_SHOW_DAYS"),
        "tray_offers_purchase no longer opens with `<= PILL_SHOW_DAYS`"
    );

    // 4. And the BEHAVIOUR at every day of the fortnight, driven off the parsed
    //    TS numbers so this loop is checking the pill's policy, not the tray's
    //    own opinion of it.
    for days in -3..=15 {
        let status = trial(days);
        let (_, urgent) = tray_line(&status);
        let floored = days.max(0);
        assert_eq!(
            urgent,
            floored < ts_urgent,
            "day {days}: tray urgency disagrees with the pill's `days < {ts_urgent}`"
        );
        assert_eq!(
            tray_offers_purchase(&status),
            floored <= ts_show,
            "day {days}: the upgrade item's presence disagrees with the pill's \
             `days > {ts_show}` silence"
        );
    }
}

/// The three sentences the item names, verbatim enough that a reader of the
/// menu bar can tell the three states apart.
#[test]
fn the_header_says_which_of_the_three_states_this_is() {
    assert_eq!(tray_line(&licensed()), ("Licensed".to_string(), false));
    assert_eq!(
        tray_line(&trial(5)),
        ("Trial — 5 days left".to_string(), false)
    );
    assert_eq!(
        tray_line(&required("trial_expired")),
        ("Trial ended — dictation paused".to_string(), true)
    );
    // "1 day left" is not "1 days left", and the last day is not "0 days left".
    assert_eq!(tray_line(&trial(1)).0, "Trial — 1 day left");
    assert_eq!(tray_line(&trial(0)).0, "Trial — Last day");
}

/// The icon's urgent treatment is for the clock, and for nothing else. The item
/// is explicit: "on the last day and past the trial, and only then".
#[test]
fn only_the_last_day_and_past_it_decorate_the_icon() {
    assert!(!tray_line(&licensed()).1, "a licensed Mac is never urgent");
    for days in 1..=14 {
        assert!(
            !tray_line(&trial(days)).1,
            "day {days} is not the last day and must leave the icon alone"
        );
    }
    assert!(tray_line(&trial(0)).1, "the last day is urgent");
    assert!(tray_line(&trial(-2)).1, "past the trial is urgent");
    for reason in ["trial_expired", "revoked", "bad_signature", "wrong_plan"] {
        assert!(
            tray_line(&required(reason)).1,
            "a closed gate is urgent whatever the reason ({reason})"
        );
    }
}

/// Y2-F, carried into the menu bar: a person whose STORED key stopped granting
/// is never shown a price, and never told their trial is what ran out.
#[test]
fn a_stored_key_that_granted_nothing_is_never_offered_a_purchase() {
    let mut broken = trial(5);
    broken.has_stored_license = true;
    assert!(
        !tray_offers_purchase(&broken),
        "a stored-key holder must not be shown the upgrade item"
    );
    assert!(
        tray_line(&broken).0.contains("needs attention"),
        "a broken stored key must not read as a running trial: {:?}",
        tray_line(&broken).0
    );
    // …and it does NOT decorate the icon, because nothing about it is about the
    // clock. This is the "and only then" half of the icon rule.
    assert!(!tray_line(&broken).1);

    let mut broken_ended = required("revoked");
    broken_ended.has_stored_license = true;
    assert!(!tray_offers_purchase(&broken_ended));
    assert_eq!(
        tray_line(&broken_ended).0,
        "License needs attention — dictation paused"
    );

    // A licensed Mac also has a stored key, and it is still silent.
    assert!(!tray_offers_purchase(&licensed()));
}

/// The upgrade item is absent while the trial is young — the same ambient
/// silence the pill keeps, for the same reason.
#[test]
fn the_upgrade_item_is_absent_early_in_the_trial() {
    assert!(!tray_offers_purchase(&trial(PILL_SHOW_DAYS + 1)));
    assert!(tray_offers_purchase(&trial(PILL_SHOW_DAYS)));
    assert!(tray_offers_purchase(&required("trial_expired")));
}
