//! Y1-A — a CGEvent tap that macOS switched OFF is classified, counted and
//! re-armed.
//!
//! macOS disables an event tap whose callback is slow and notifies the callback
//! itself with `kCGEventTapDisabledByTimeout` (0xFFFFFFFE) or
//! `kCGEventTapDisabledByUserInput` (0xFFFFFFFF) — delivered regardless of the
//! event mask the tap asked for. The tap then stays dead until
//! `CGEventTapEnable` is called again. Before this item the tree called
//! `CGEventTapEnable` exactly once, at startup, so one slow callback, one long
//! decode or one sleep/wake cycle killed push-to-talk for the rest of the
//! process's life — indistinguishable, from Wilson's chair, from a revoked
//! Accessibility grant.
//!
//! Everything asserted here is PURE: no window server, no tap, no permission
//! grant, no running app. That is the point of pulling the decision out of the
//! `extern "C"` callback.

#![cfg(target_os = "macos")]

use wilson_voice_lib::ptt_macos::{
    classify_tap_disable, note_tap_disabled, re_arm_tap, should_re_arm, tap_health,
    tap_health_message, TapDisable, TapHealth,
};

/// The timeout case: the literal Apple hands us is recognised, named, and
/// re-armed rather than mourned.
#[test]
fn a_timeout_disable_is_classified_and_re_armed() {
    let kind = classify_tap_disable(0xFFFF_FFFE);
    assert_eq!(
        kind,
        Some(TapDisable::Timeout),
        "0xFFFFFFFE is kCGEventTapDisabledByTimeout — if this is None the callback \
         treats the death notice as an ordinary event and the hotkey stays dead"
    );
    let kind = kind.unwrap();
    assert_eq!(kind.name(), "kCGEventTapDisabledByTimeout");
    assert!(
        should_re_arm(kind),
        "a timed-out tap is exactly the tap that must be switched back on"
    );
    // The re-arm is ATTEMPTED and its outcome reported. This test binary has no
    // tap installed, so the honest answer is false — never a panic, and never a
    // silent true that would let a real failure look like a success.
    assert!(
        !re_arm_tap(),
        "with no tap port published, re_arm_tap must report that it could not \
         re-enable anything instead of claiming it did"
    );
}

/// ByUserInput is NOT normal and is NOT swallowed: a user-input flood took the
/// tap out and the tap still needs re-arming.
#[test]
fn a_user_input_disable_is_classified_and_re_armed() {
    let kind = classify_tap_disable(0xFFFF_FFFF);
    assert_eq!(
        kind,
        Some(TapDisable::UserInput),
        "0xFFFFFFFF is kCGEventTapDisabledByUserInput"
    );
    let kind = kind.unwrap();
    assert_eq!(kind.name(), "kCGEventTapDisabledByUserInput");
    assert!(
        should_re_arm(kind),
        "ByUserInput must not be treated as normal — the tap is off either way"
    );

    // And an ordinary event type is still an ordinary event: flags-changed (12)
    // and key-down (10) must NOT be mistaken for a death notice, or every
    // keystroke would count as a re-arm.
    assert_eq!(classify_tap_disable(12), None);
    assert_eq!(classify_tap_disable(10), None);
    assert_eq!(classify_tap_disable(0), None);
}

/// Counted, not merely logged — and silent the first time, visible the second.
#[test]
fn re_arm_attempts_are_counted_not_just_logged() {
    // The pure counter, split by cause: "died once since launch" and "dies every
    // few minutes" are different bugs.
    let mut h = TapHealth::default();
    assert_eq!(
        tap_health_message(h),
        None,
        "a healthy session says nothing"
    );

    h.record(TapDisable::Timeout);
    assert_eq!(h.re_arms, 1);
    assert_eq!(h.by_timeout, 1);
    assert_eq!(h.by_user_input, 0);
    assert_eq!(
        tap_health_message(h),
        None,
        "the FIRST re-arm is silent by design — we fixed it in microseconds"
    );

    h.record(TapDisable::UserInput);
    assert_eq!(h.re_arms, 2);
    assert_eq!(h.by_timeout, 1);
    assert_eq!(h.by_user_input, 1);
    let msg = tap_health_message(h).expect("the second re-arm in a session is reportable");
    assert!(
        msg.contains("the hotkey stopped listening"),
        "the surfaced line must say what the USER experienced, not the API name: {msg}"
    );
    assert!(msg.contains("re-armed"), "…and that we fixed it: {msg}");

    // The process-wide counter the callback actually writes to is wired to the
    // same pure step — not a parallel, untested copy.
    let before = tap_health().re_arms;
    let after = note_tap_disabled(TapDisable::Timeout);
    assert_eq!(
        after.re_arms,
        before + 1,
        "note_tap_disabled must advance the session counter the support bundle reads"
    );
    assert_eq!(after.by_timeout, tap_health().by_timeout);
}
