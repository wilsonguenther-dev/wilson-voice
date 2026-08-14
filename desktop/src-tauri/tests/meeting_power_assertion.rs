//! YV91 acceptance — **the idle-sleep power assertion** (plan finding OS-1,
//! matrix row #16).
//!
//! A student starts a recording, does not touch the keyboard for ninety
//! minutes, and with default energy settings the Mac idle-sleeps: the audio HAL
//! stops and the recording ends with nobody told. The fix is one IOKit
//! assertion held for exactly as long as capture runs.
//!
//! ## What this file proves, and what it cannot
//!
//! The cheap tests below are the ones that hold every run: the assertion type
//! is the system-idle one and never the display one (a display kept awake for
//! three hours is a battery bug, not a feature), and taking + releasing is
//! panic-free.
//!
//! The plan's stated acceptance line — *"a meeting survives 60 minutes of zero
//! user input on battery with display sleep enabled and default energy
//! settings"* — is a MANUAL repro by construction: it needs a real hour, a real
//! battery, and a machine nobody touches. What is automatable is the mechanism
//! underneath it, and that is
//! [`the_assertion_is_visible_to_the_system_while_it_is_held`]: while the
//! assertion is held, `pmset -g assertions` reports
//! `PreventUserIdleSystemSleep` for this process, and after the release it does
//! not. It is `#[ignore]`d because it shells out to `pmset` and depends on the
//! machine granting the assertion — run it with
//! `cargo test --test meeting_power_assertion -- --ignored --nocapture`, and
//! see `docs/pr-screenshots/YV91/` for the recorded run and the manual
//! 60-minute repro.

use wilson_voice_lib::power::{
    assertion_reason, is_allowed_assertion_type, PowerAssertion, ASSERTION_TYPE,
    FORBIDDEN_ASSERTION_TYPE,
};

#[test]
fn a_meeting_prevents_system_idle_sleep_and_never_display_sleep() {
    assert_eq!(ASSERTION_TYPE, "PreventUserIdleSystemSleep");
    assert!(is_allowed_assertion_type(ASSERTION_TYPE));
    assert!(!is_allowed_assertion_type(FORBIDDEN_ASSERTION_TYPE));
    assert!(
        !ASSERTION_TYPE.contains("Display"),
        "keeping the screen on for a three-hour recording is a battery bug"
    );
}

#[test]
fn the_reason_the_user_sees_names_the_app_and_the_activity() {
    // Shown in Activity Monitor's Energy tab and in `pmset -g assertions`. A
    // user who wonders why their Mac is awake deserves an answer.
    let reason = assertion_reason("a meeting");
    assert!(reason.starts_with("Yap "), "{reason}");
    assert!(reason.contains("a meeting"), "{reason}");
}

#[test]
fn taking_and_releasing_the_assertion_is_panic_free() {
    // Not asserted to SUCCEED: a refusal is a warn-and-continue path by design,
    // because failing to prevent idle sleep is exactly the behaviour that
    // shipped before this item.
    let held = PowerAssertion::prevent_idle_sleep("a meeting (test)");
    if let Some(assertion) = held.as_ref() {
        assert_ne!(assertion.id(), 0, "a held assertion has a real IOKit id");
    }
    drop(held);
}

#[test]
#[ignore = "shells out to pmset and needs the machine to grant the assertion; run with --ignored"]
fn the_assertion_is_visible_to_the_system_while_it_is_held() {
    fn pmset_assertions() -> String {
        std::process::Command::new("/usr/bin/pmset")
            .args(["-g", "assertions"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default()
    }

    let pid = std::process::id().to_string();
    let held = PowerAssertion::prevent_idle_sleep("a meeting").expect("IOKit granted it");
    let during = pmset_assertions();
    println!("--- pmset -g assertions WHILE held (pid {pid}) ---\n{during}");
    let held_line = during
        .lines()
        .find(|l| l.contains("PreventUserIdleSystemSleep") && l.contains(&pid));
    assert!(
        held_line.is_some(),
        "pmset does not show this process holding PreventUserIdleSystemSleep"
    );
    let line = held_line.unwrap();
    assert!(
        line.contains("Yap is recording"),
        "the assertion is anonymous to the user: {line}"
    );
    // Scoped to OUR pid on purpose: `pmset` opens with a system-wide summary
    // that names every assertion type (including a zero count for the display
    // one), so a bare substring search here passes or fails on other people's
    // processes rather than on ours.
    assert!(
        !during
            .lines()
            .any(|l| l.contains(FORBIDDEN_ASSERTION_TYPE) && l.contains(&pid)),
        "this process holds a display-sleep assertion and must not"
    );

    drop(held);
    let after = pmset_assertions();
    println!("--- pmset -g assertions AFTER release ---\n{after}");
    assert!(
        !after
            .lines()
            .any(|l| l.contains("PreventUserIdleSystemSleep") && l.contains(&pid)),
        "the assertion outlived the meeting — the Mac would never idle-sleep again"
    );
}
