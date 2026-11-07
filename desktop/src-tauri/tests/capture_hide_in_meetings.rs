//! WF-16 — "Hide Yap from screen sharing during meetings". Pure: no GUI, no
//! Mac, no audio. The toggle function (`float_pill::CaptureToggle::step`) sets
//! the pill and Scratchpad windows' sharing type to "none" on a meeting's
//! record start and restores it on stop, touching the windows ONLY on those
//! edges. The manual half (a QuickTime screen recording during a meeting shows
//! neither window; after Stop both are back) is Wilson's live check.

use wilson_voice_lib::call_offer::CALL_OFFER_WINDOW;
use wilson_voice_lib::float_pill::{
    capture_hidden_for, capture_hidden_wanted, CaptureToggle, MEETING_CAPTURE_HIDDEN_WINDOWS,
};
use wilson_voice_lib::scratchpad::WINDOW_LABEL as SCRATCHPAD;
use wilson_voice_lib::AppSettings;

#[test]
fn sharing_type_is_none_on_record_start_and_restored_on_stop() {
    let mut t = CaptureToggle::new();
    // Idle at launch: windows were built visible to capture; nothing to do.
    assert_eq!(t.step(true, false), None);
    assert_eq!(t.applied(), Some(false));
    // Record start → hide (sharing type none).
    assert_eq!(t.step(true, true), Some(true));
    // Every 1 Hz tick while recording is a no-op.
    for _ in 0..100 {
        assert_eq!(t.step(true, true), None);
    }
    assert_eq!(t.applied(), Some(true));
    // Stop → restored.
    assert_eq!(t.step(true, false), Some(false));
    assert_eq!(t.step(true, false), None);
    // A second meeting hides again.
    assert_eq!(t.step(true, true), Some(true));
    assert_eq!(t.step(true, false), Some(false));
}

#[test]
fn a_meeting_already_recording_at_the_first_tick_is_hidden() {
    // A recovered meeting's first status arrives with recording = true.
    let mut t = CaptureToggle::new();
    assert_eq!(t.applied(), None);
    assert_eq!(t.step(true, true), Some(true));
}

#[test]
fn setting_off_never_hides_and_switching_it_mid_meeting_applies_at_once() {
    let mut t = CaptureToggle::new();
    assert_eq!(t.step(false, false), None);
    assert_eq!(
        t.step(false, true),
        None,
        "setting off: a meeting hides nothing"
    );
    // Turned on mid-meeting → hidden now.
    assert_eq!(t.step(true, true), Some(true));
    // Turned off mid-meeting → restored now, not at stop.
    assert_eq!(t.step(false, true), Some(false));
    assert_eq!(t.step(false, false), None);
}

#[test]
fn hidden_only_while_a_meeting_records_with_the_setting_on() {
    assert!(capture_hidden_wanted(true, true));
    assert!(!capture_hidden_wanted(true, false));
    assert!(!capture_hidden_wanted(false, true));
    assert!(!capture_hidden_wanted(false, false));
}

#[test]
fn the_pill_and_scratchpad_hide_in_a_meeting_and_the_call_chip_always() {
    assert_eq!(MEETING_CAPTURE_HIDDEN_WINDOWS, ["float", SCRATCHPAD]);
    assert_eq!(SCRATCHPAD, "scratchpad");
    for label in ["float", "scratchpad"] {
        assert!(
            capture_hidden_for(label, true),
            "{label} hidden during a meeting"
        );
        assert!(
            !capture_hidden_for(label, false),
            "{label} back in captures after"
        );
    }
    // The call offer / chip window: hidden from capture at all times.
    assert!(capture_hidden_for(CALL_OFFER_WINDOW, false));
    assert!(capture_hidden_for(CALL_OFFER_WINDOW, true));
    // The main window is never hidden.
    assert!(!capture_hidden_for("main", true));
    assert!(!capture_hidden_for("main", false));
}

#[test]
fn the_setting_is_on_by_default_and_for_an_old_settings_file() {
    assert!(AppSettings::default().hide_from_capture_in_meetings);
    let old: AppSettings = serde_json::from_str("{}").expect("empty store loads");
    assert!(old.hide_from_capture_in_meetings);
    let off: AppSettings =
        serde_json::from_str(r#"{"hideFromCaptureInMeetings":false}"#).expect("loads");
    assert!(!off.hide_from_capture_in_meetings);
}

#[test]
fn every_window_and_both_edges_go_through_the_one_rule() {
    let lib = include_str!("../src/lib.rs");
    let float = include_str!("../src/float_pill.rs");
    let pad = include_str!("../src/scratchpad.rs");
    let offer = include_str!("../src/call_offer.rs");
    // The meeting status sink (start + stop edges) drives the toggle.
    let sink = &lib[lib.find("fn meeting_status_sink(").expect("sink")..];
    let sink = &sink[..sink.find("\n}\n").expect("end of sink")];
    assert!(sink.contains("float_pill::set_meeting_recording(status.recording);"));
    assert!(sink.contains("float_pill::sync_capture_hidden(&app);"));
    // A settings save applies the switch at once; launch reads it first.
    assert!(lib.contains("float_pill::set_hide_from_capture(next.hide_from_capture_in_meetings);"));
    assert!(lib.contains(
        "float_pill::set_hide_from_capture(state.settings.lock().hide_from_capture_in_meetings);"
    ));
    // Both windows are built through the rule, so one opened mid-meeting is hidden.
    assert!(float.contains("crate::call_offer::apply_content_protection(&win);"));
    assert!(pad.contains("crate::call_offer::apply_content_protection(&w);"));
    // The rule reads the live meeting-time answer.
    assert!(offer.contains(
        "crate::float_pill::capture_hidden_for(w.label(), crate::float_pill::capture_hidden_now())"
    ));
    assert_eq!(offer.matches(".set_content_protected(").count(), 1);
}
