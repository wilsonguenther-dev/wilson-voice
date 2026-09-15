//! PERM-E — the four grants are TRI-STATE, and `Unknown` is a first-class,
//! NON-NAGGING state.
//!
//! The two named tests below are the panel's binding findings, as executable
//! rules: an unreadable grant never nags, and system audio capture is Unknown
//! by construction because there is no API to read it and probing it would pop
//! a TCC alert that macOS only ever shows once.

use wilson_voice_lib::mic_auth::MicAuth;
use wilson_voice_lib::permissions::{self, AudioCaptureObservation, GrantSnapshot, GrantState};

fn report_with(
    input_monitoring: GrantState,
    audio_capture: GrantState,
) -> permissions::PermissionReport {
    permissions::build_report_with_grants(
        true,
        MicAuth::Authorized,
        input_monitoring,
        audio_capture,
        true,
        "model ready".into(),
    )
}

#[test]
fn an_unknown_grant_is_invisible_and_never_nags() {
    assert!(
        !GrantState::Unknown.nags(),
        "Unknown must never put a line in front of the user"
    );
    assert!(
        GrantState::Denied.nags(),
        "a denial is the only thing that does"
    );
    assert!(!GrantState::Authorized.nags());

    // Both PERM-E grants unreadable, everything else fine: the summary must not
    // mention either of them, and the snapshot must not be watchable.
    let r = report_with(GrantState::Unknown, GrantState::Unknown);
    assert!(
        r.all_critical_ok,
        "an unknown grant is not a critical failure"
    );
    assert_eq!(
        r.summary, "All critical permissions look good for Yap.",
        "an unknown grant leaked into the summary: {}",
        r.summary
    );
    assert!(
        !GrantSnapshot::from_report(&r).has_denial(),
        "unknown must not register as a denial"
    );
    assert_eq!(
        permissions::next_wait(GrantSnapshot::from_report(&r)),
        None,
        "an unknown grant must not keep a timer alive — it can never be resolved by re-reading"
    );

    // A DENIAL, by contrast, does speak — otherwise this test would pass on a
    // surface that says nothing about anything.
    let denied = report_with(GrantState::Denied, GrantState::Unknown);
    assert!(
        denied.summary.contains("Input Monitoring"),
        "a real denial must be named: {}",
        denied.summary
    );
}

#[test]
fn audio_capture_is_unknown_by_construction_never_probed() {
    // There is no `authorizationStatus` for system audio capture, so the ONLY
    // legitimate input is an observation from a tap that ALREADY ran. With no
    // observation, the answer is Unknown — never a guess, never a probe.
    assert_eq!(
        permissions::audio_capture_from_observation(AudioCaptureObservation::NeverRan),
        GrantState::Unknown
    );
    assert_eq!(
        permissions::audio_capture_from_observation(AudioCaptureObservation::Delivered),
        GrantState::Authorized,
        "audio that actually arrived is the grant, proven"
    );
    assert_eq!(
        permissions::audio_capture_from_observation(AudioCaptureObservation::SilentPastGrace),
        GrantState::Denied,
        "silence past the denial grace is the only denial this can ever report"
    );

    // And the row a fresh install renders says so without accusing anyone.
    let detail = permissions::audio_capture_detail(GrantState::Unknown);
    assert!(
        detail.contains("has not been tried"),
        "unknown must read as 'not tried', not as a failure: {detail}"
    );
    assert!(
        !detail.to_lowercase().contains("denied"),
        "unknown must never read as denied: {detail}"
    );
}

#[test]
fn a_working_tap_is_never_reported_blocked() {
    // PANEL (2): a listen-only CGEventTap runs fine under an Accessibility
    // grant alone, so IOKit saying "denied" while the tap is delivering must
    // never become a red line telling the user to fix what is not broken.
    assert_eq!(
        permissions::input_monitoring_state(GrantState::Denied, false, true),
        GrantState::Authorized,
        "an observably delivering tap outranks IOHIDCheckAccess"
    );
    assert_eq!(
        permissions::input_monitoring_state(GrantState::Denied, true, false),
        GrantState::Unknown,
        "a HID denial under a live Accessibility grant is unprovable, not a denial"
    );
    assert_eq!(
        permissions::input_monitoring_state(GrantState::Denied, false, false),
        GrantState::Denied,
        "no Accessibility and no working tap IS the real denial"
    );
    assert_eq!(
        permissions::input_monitoring_state(GrantState::Authorized, false, false),
        GrantState::Authorized
    );
}

#[test]
fn the_four_rows_are_the_surface_and_each_carries_its_own_deep_link() {
    let r = report_with(GrantState::Unknown, GrantState::Unknown);
    let keys: Vec<&str> = r.grants.iter().map(|g| g.key).collect();
    assert_eq!(
        keys,
        vec![
            permissions::GRANT_MICROPHONE,
            permissions::GRANT_ACCESSIBILITY,
            permissions::GRANT_INPUT_MONITORING,
            permissions::GRANT_AUDIO_CAPTURE,
        ],
        "four grants, one surface, in reading order"
    );
    for row in &r.grants {
        assert!(!row.detail.is_empty(), "{} has no sentence", row.key);
        // Every pane must be one `open_privacy_pane` actually recognises.
        assert!(
            permissions::open_privacy_pane_urls(row.pane)
                .iter()
                .any(|u| u.contains("Privacy_")),
            "{} points at a pane with no verified deep link: {}",
            row.key,
            row.pane
        );
    }
}

#[test]
fn not_determined_is_unknown_and_a_refusal_is_denied() {
    assert_eq!(
        permissions::mic_grant_state(MicAuth::NotDetermined),
        GrantState::Unknown,
        "nobody was asked — the health row must not accuse macOS of blocking it"
    );
    assert_eq!(
        permissions::mic_grant_state(MicAuth::Denied),
        GrantState::Denied
    );
    assert_eq!(
        permissions::mic_grant_state(MicAuth::Restricted),
        GrantState::Denied
    );
    assert_eq!(
        permissions::mic_grant_state(MicAuth::Authorized),
        GrantState::Authorized
    );
}

#[test]
fn a_failed_paste_under_a_revoked_accessibility_grant_names_accessibility() {
    let generic = "no receipt inside 1500ms";
    let named = permissions::paste_failure_detail(false, generic);
    assert!(
        named.contains("Accessibility"),
        "a paste that died for want of Accessibility must say so: {named}"
    );
    assert!(
        !named.contains(generic),
        "and must not read as a transcription failure: {named}"
    );
    assert_eq!(
        permissions::paste_failure_detail(true, generic),
        generic,
        "with Accessibility trusted the real reason survives untouched"
    );
}
