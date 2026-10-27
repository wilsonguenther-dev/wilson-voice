//! yap-hotkey-stop — a take the hotkey started, the hotkey stops.
//!
//! Wilson 2026-10-16: fifteen fn presses and the take would not stop. The fn
//! state machine is now a PURE value (`ptt_macos::PttMachine`) fed by two
//! sources — the CGEventTap and, while a take is live, a 30 ms poll of the HID
//! flags state — through ONE edge function. Everything below drives that
//! machine with synthetic edges on a synthetic clock: no window server, no tap,
//! no permission grant.

#![cfg(target_os = "macos")]

use wilson_voice_lib::ptt_macos::{
    end_hands_free, end_hands_free_calls, re_arm_verdict, stop_take_with, stops_by, DisableWindow,
    EdgeSource, PttEvent, PttMachine, Step, StopOrigin, DEBOUNCE_MS, HOLD_ARM_MS, POLL_INTERVAL_MS,
    POLL_SOURCE_STATE, RECREATE_WINDOW_MS, RE_ARM_VERIFY_MS, STILL_RECORDING_AFTER_SECS,
    STILL_RECORDING_MESSAGE, TAP_LOCATION,
};

const TAP: EdgeSource = EdgeSource::Tap;

fn count(events: &[PttEvent], which: PttEvent) -> usize {
    events.iter().filter(|e| **e == which).count()
}

/// Press at `t`, let the hold arm, return the machine with a live HOLD take.
fn hold_take(m: &mut PttMachine, t: u64) -> Vec<PttEvent> {
    let mut all = Vec::new();
    let down = m.edge(true, false, t, TAP);
    let gen = down.arm_hold.expect("a fresh press arms the hold timer");
    all.extend(down.events);
    let armed = m.hold_timer(gen, t + HOLD_ARM_MS);
    all.extend(armed.events);
    all
}

/// Two short taps → hands-free latch.
fn hands_free_take(m: &mut PttMachine, t: u64) -> Vec<PttEvent> {
    let mut all = Vec::new();
    for s in [
        m.edge(true, false, t, TAP),
        m.edge(false, false, t + 80, TAP),
        m.edge(true, false, t + 200, TAP),
        m.edge(false, false, t + 280, TAP),
    ] {
        all.extend(s.events);
    }
    all
}

#[test]
fn hold_up_edge_ends_the_take_exactly_once() {
    let mut m = PttMachine::new();
    let ev = hold_take(&mut m, 1_000);
    assert_eq!(ev, vec![PttEvent::Start]);
    assert!(m.take_live());
    assert!(m.poll_wanted(), "a live take runs the second mechanism");
    let up = m.edge(false, false, 5_000, TAP);
    assert_eq!(up.events, vec![PttEvent::Stop]);
    assert!(!m.take_live());
}

#[test]
fn hands_free_double_tap_then_one_tap_ends_the_take() {
    let mut m = PttMachine::new();
    let ev = hands_free_take(&mut m, 1_000);
    assert_eq!(ev, vec![PttEvent::HandsFreeOn, PttEvent::Start]);
    assert!(m.hands_free());
    let down = m.edge(true, false, 9_000, TAP);
    assert_eq!(down.events, vec![PttEvent::HandsFreeOff, PttEvent::Stop]);
    let up = m.edge(false, false, 9_100, TAP);
    assert!(up.events.is_empty(), "the ending tap's release is consumed");
    assert!(!m.take_live());
}

#[test]
fn missed_up_edge_is_recovered_by_the_poll_and_never_stops_twice() {
    let mut m = PttMachine::new();
    hold_take(&mut m, 1_000);
    // Prove the HID read can see fn (the first poll sample of a hold take).
    assert!(m.poll(true, true, false, 1_300).events.is_empty());
    // The tap LOST the release. The poll sees fn up.
    let polled = m.poll(false, false, false, 4_000);
    assert_eq!(polled.events, vec![PttEvent::Stop]);
    // The tap's late copy of the same up-edge is not a transition → nothing.
    let late = m.edge(false, false, 4_010, TAP);
    assert_eq!(late, Step::default());
    let edge = m.last_edge().expect("an applied edge is recorded");
    assert_eq!(edge.source, EdgeSource::Poll);
    assert!(!edge.down);
}

#[test]
fn tap_and_poll_reporting_the_same_hands_free_tap_stop_once() {
    let mut m = PttMachine::new();
    hands_free_take(&mut m, 1_000);
    let mut all = Vec::new();
    all.extend(m.edge(true, false, 9_000, TAP).events);
    all.extend(m.poll(true, true, false, 9_000 + POLL_INTERVAL_MS).events);
    all.extend(m.poll(false, false, false, 9_100).events);
    all.extend(m.edge(false, false, 9_110, TAP).events);
    assert_eq!(count(&all, PttEvent::Stop), 1, "{all:?}");
}

#[test]
fn hands_free_stop_seen_only_by_the_poll() {
    let mut m = PttMachine::new();
    hands_free_take(&mut m, 1_000);
    // Tap dead: only the poll sees the fn down-up.
    let down = m.poll(true, true, false, 9_000);
    assert_eq!(down.events, vec![PttEvent::HandsFreeOff, PttEvent::Stop]);
    assert!(m.poll(false, false, false, 9_090).events.is_empty());
    assert!(!m.take_live());
    assert!(
        !m.poll_wanted(),
        "the poll winds down once the key is released"
    );
}

#[test]
fn tap_disabled_mid_take_prompts_then_fn_up_still_ends_it() {
    let mut m = PttMachine::new();
    hold_take(&mut m, 1_000);
    m.poll(true, true, false, 1_310);
    let disabled = m.tap_disabled();
    assert_eq!(disabled.events, vec![PttEvent::StillRecording]);
    assert!(m.still_recording());
    assert!(m.tap_disabled().events.is_empty(), "prompted once per take");
    // Tap is off; the poll delivers the release.
    let up = m.poll(false, false, false, 2_000);
    assert_eq!(up.events, vec![PttEvent::Stop]);
    assert!(!m.still_recording());
}

#[test]
fn a_blind_hid_read_never_stops_a_hold_take() {
    let mut m = PttMachine::new();
    hold_take(&mut m, 1_000);
    // The read never shows fn: unproven → ignored, rather than reading as an
    // instant key-up that ends every take the moment it starts.
    for t in (1_300..3_000).step_by(POLL_INTERVAL_MS as usize) {
        assert!(m.poll(false, false, false, t).events.is_empty());
    }
    assert!(m.take_live());
    assert!(!m.hid_proven());
    assert_eq!(
        m.edge(false, false, 3_100, TAP).events,
        vec![PttEvent::Stop]
    );
}

#[test]
fn still_recording_after_90_seconds_of_hold_then_one_fn_tap_ends_it() {
    assert_eq!(STILL_RECORDING_AFTER_SECS, 90);
    let mut m = PttMachine::new();
    hold_take(&mut m, 0);
    let start = HOLD_ARM_MS;
    assert!(m.tick(start + 89_999).events.is_empty());
    assert_eq!(
        m.tick(start + 90_000).events,
        vec![PttEvent::StillRecording]
    );
    assert!(
        m.tick(start + 95_000).events.is_empty(),
        "raised once per take"
    );
    assert!(m.still_recording());
    // The release was lost, so the machine still holds the key down: the
    // user's fn tap is a down (not a transition) then an up — the up ends it.
    assert!(m.edge(true, false, start + 96_000, TAP).events.is_empty());
    let up = m.edge(false, false, start + 96_100, TAP);
    assert_eq!(up.events, vec![PttEvent::Stop], "one fn tap ends it");
    assert!(!m.still_recording());
}

#[test]
fn a_stuck_combo_down_is_reconciled_so_the_next_press_is_not_swallowed() {
    let mut m = PttMachine::new();
    m.poll(true, true, false, 0); // read proven earlier in the session
    m.edge(false, false, 100, TAP);
    assert!(!m.combo_down());
    let d = m.edge(true, false, 1_000, TAP);
    assert!(d.arm_hold.is_some());
    // A very fast release lands inside the debounce window and is dropped.
    assert!(m
        .edge(false, false, 1_000 + DEBOUNCE_MS - 5, TAP)
        .events
        .is_empty());
    assert!(m.combo_down(), "the bug: combo_down stuck true");
    assert!(
        m.poll_wanted(),
        "a proven read keeps polling a held/stuck key"
    );
    // The poll reconciles it.
    m.poll(false, false, false, 1_060);
    assert!(!m.combo_down());
    // So the user's NEXT press is a real edge that arms a hold.
    let next = m.edge(true, false, 3_000, TAP);
    assert!(next.arm_hold.is_some(), "next press must not be swallowed");
}

#[test]
fn an_external_stop_does_not_swallow_the_next_press() {
    let mut m = PttMachine::new();
    hands_free_take(&mut m, 1_000);
    m.external_stop();
    assert!(!m.take_live());
    let next = m.edge(true, false, 5_000, TAP);
    assert!(next.events.is_empty());
    let gen = next
        .arm_hold
        .expect("the next press is a press, not a latch reset");
    assert_eq!(
        m.hold_timer(gen, 5_000 + HOLD_ARM_MS).events,
        vec![PttEvent::Start]
    );
}

#[test]
fn an_interrupted_press_is_ignored_until_released() {
    let mut m = PttMachine::new();
    hold_take(&mut m, 0);
    assert_eq!(m.key_down(false).events, vec![PttEvent::Interrupted]);
    assert!(m.key_down(false).events.is_empty());
    // The poll still sees fn held — that must not become a new press.
    assert!(m.poll(true, true, false, 500).events.is_empty());
    assert!(m.edge(false, false, 600, TAP).events.is_empty());
    assert!(m.edge(true, false, 2_000, TAP).arm_hold.is_some());
}

#[test]
fn menu_bar_and_pill_stops_reach_end_hands_free() {
    for origin in [
        StopOrigin::MenuBar,
        StopOrigin::OnScreen,
        StopOrigin::Hotkey,
    ] {
        let calls = end_hands_free_calls();
        let before = stops_by(origin);
        let mut ran = false;
        stop_take_with(origin, || ran = true);
        assert!(ran, "{origin:?}: the app's stop ran");
        assert!(
            end_hands_free_calls() > calls,
            "{origin:?}: end_hands_free ran"
        );
        assert!(stops_by(origin) > before);
    }
    end_hands_free(); // safe with no tap installed
}

#[test]
fn every_stop_call_site_goes_through_stop_take() {
    let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("read lib.rs");
    let body_start = lib.find("fn stop_take(").expect("stop_take exists");
    let body_len = lib[body_start..]
        .find("\n}\n")
        .expect("stop_take body ends");
    let body = &lib[body_start..body_start + body_len];
    assert!(body.contains("ptt_macos::stop_take_with(origin"), "{body}");
    assert!(lib.contains("stop_take(&app, state.inner(), StopOrigin::OnScreen)"));
    assert_eq!(
        lib.matches("stop_take(app, &state, StopOrigin::MenuBar)")
            .count(),
        1,
        "the tray Stop item"
    );
    // Y8-A: the tray Hands-free item and the ⌃⌘H chord share ONE toggle,
    // and its only stop path is stop_take.
    let hf_start = lib
        .find("fn toggle_hands_free(")
        .expect("toggle_hands_free exists");
    let hf_len = lib[hf_start..]
        .find("\n}\n")
        .expect("toggle_hands_free body ends");
    assert!(lib[hf_start..hf_start + hf_len].contains("stop_take(app, state, origin)"));
    assert!(lib.contains("toggle_hands_free(app, &state, StopOrigin::MenuBar)"));
    assert!(lib.contains("toggle_hands_free(app, state, StopOrigin::Hotkey)"));
    // yap-p0-hotkeys-no-permission: the hotkey stop moved into the ONE
    // dispatcher, `take_hotkey_edge`, which both the fn tap's PttEvent::Stop
    // and the Carbon dictate chord reach.
    assert!(lib.contains("hotkeys::TakeAction::Stop => stop_take(app, state, StopOrigin::Hotkey)"));
    assert!(lib.contains("take_hotkey_edge(&h, &st, HotkeyEdge::Release, HotkeySource::FnTap)"));
    // No stop path left that skips end_hands_free.
    assert_eq!(
        lib.matches("ptt_macos::end_hands_free();").count(),
        0,
        "every caller goes through stop_take_with"
    );
}

#[test]
fn the_stop_path_does_not_depend_on_the_frontmost_app() {
    assert_eq!(TAP_LOCATION, 0, "kCGHIDEventTap");
    assert_eq!(POLL_SOURCE_STATE, 1, "kCGEventSourceStateHIDSystemState");
    assert_eq!(POLL_INTERVAL_MS, 30);
}

#[test]
fn three_disables_in_a_minute_rebuild_the_tap_and_re_arm_is_verified() {
    let mut w = DisableWindow::new();
    assert!(!w.record(0));
    assert!(!w.record(10_000));
    assert!(w.record(20_000), "third disable inside 60 s → recreate");
    assert!(!w.record(30_000), "window cleared after a recreate");
    let mut slow = DisableWindow::new();
    assert!(!slow.record(0));
    assert!(!slow.record(RECREATE_WINDOW_MS));
    assert!(
        !slow.record(2 * RECREATE_WINDOW_MS),
        "spread out → re-arm only"
    );
    assert_eq!(re_arm_verdict(Some(3)), "re-arm verified");
    assert_eq!(re_arm_verdict(Some(RE_ARM_VERIFY_MS + 1)), "re-arm FAILED");
    assert_eq!(re_arm_verdict(None), "re-arm FAILED");
}

#[test]
fn the_pill_and_the_backend_say_the_same_still_recording_line() {
    let live = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/pill/live.ts"))
        .expect("read live.ts");
    assert!(
        live.contains(STILL_RECORDING_MESSAGE),
        "{STILL_RECORDING_MESSAGE}"
    );
    let float = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src/float-main.tsx"
    ))
    .expect("read float-main.tsx");
    // yap-qa-hotkey-walk-binding added the fn hint as a third, lower-ranked
    // argument; still-recording still wins inside `stripLine`.
    assert!(float.contains("stripLine(line, stillRecording, fnHint)"));
}
