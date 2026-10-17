//! yap-p0-hotkeys-no-permission — a take starts and stops with ZERO TCC grants.
//!
//! Wilson 2026-10-17: "none of the hotkeys are working". Root cause (proven on
//! unmodified main with the first version of this file, see
//! docs/loop/evidence/yap-p0-hotkeys-no-permission.md): the only chord that
//! could start a take was the fn CGEvent tap, which needs Input Monitoring, and
//! `hotkey_registered` was written `true` the moment the tap THREAD spawned.
//!
//! These tests drive the SHIPPING decision code — `hotkeys::arm_at_launch`,
//! `hotkeys::attempt_tap`, `hotkeys::HotkeyArbiter`, `hotkeys::degraded_line` —
//! through the registrar seam with every grant denied and no OS hook. The OS
//! half is modelled honestly: an unregistered Carbon chord is never delivered,
//! and a tap that is not `Delivering` delivers no fn edge.

use std::collections::VecDeque;

use tauri_plugin_global_shortcut::Shortcut;
use wilson_voice_lib::hotkeys::{
    self, CarbonRegistrar, CarbonStatus, FnTapStatus, HotkeyArbiter, HotkeyEdge, HotkeySource,
    HotkeysStatus, PermissionProbe, TakeAction, TapAttempt, TapEventSource,
};
use wilson_voice_lib::shortcuts::{self, DICTATE_TOGGLE};

const LIB: &str = include_str!("../src/lib.rs");

struct FakeCarbon {
    result: Result<(), String>,
    registered: Vec<Shortcut>,
}
impl FakeCarbon {
    fn ok() -> Self {
        Self { result: Ok(()), registered: Vec::new() }
    }
    fn failing(why: &str) -> Self {
        Self { result: Err(why.into()), registered: Vec::new() }
    }
}
impl CarbonRegistrar for FakeCarbon {
    fn register(&mut self, chord: Shortcut) -> Result<(), String> {
        if self.result.is_ok() {
            self.registered.push(chord);
        }
        self.result.clone()
    }
}

/// Every grant denied.
struct Denied;
impl PermissionProbe for Denied {
    fn accessibility(&self) -> bool {
        false
    }
    fn input_monitoring(&self) -> bool {
        false
    }
}
struct Granted;
impl PermissionProbe for Granted {
    fn accessibility(&self) -> bool {
        true
    }
    fn input_monitoring(&self) -> bool {
        true
    }
}

struct FakeTap {
    results: VecDeque<Result<TapAttempt, String>>,
    attempts: usize,
}
impl FakeTap {
    fn new(results: Vec<Result<TapAttempt, String>>) -> Self {
        Self { results: results.into(), attempts: 0 }
    }
    fn refused() -> Self {
        Self::new(vec![Err("macOS refused the key listener (Input Monitoring is off for Yap)".into())])
    }
}
impl TapEventSource for FakeTap {
    fn install(&mut self) -> Result<TapAttempt, String> {
        self.attempts += 1;
        self.results.pop_front().unwrap_or(Err("no grant".into()))
    }
}

/// The app side of a take, modelled: the recording flag, start/stop counts,
/// and which surfaces the OS will deliver an edge from.
struct Sim {
    arb: HotkeyArbiter,
    status: HotkeysStatus,
    recording: bool,
    starts: u32,
    stops: u32,
}
impl Sim {
    fn armed(carbon: &mut FakeCarbon, probe: &dyn PermissionProbe, tap: &mut FakeTap) -> Self {
        let mut status = HotkeysStatus::new();
        hotkeys::arm_at_launch(DICTATE_TOGGLE.shortcut(), carbon, probe, tap, &mut status);
        Self { arb: HotkeyArbiter::new(), status, recording: false, starts: 0, stops: 0 }
    }
    /// One physical edge. Returns what the dispatcher decided, or `None` when
    /// the OS never delivered it at all.
    fn edge(&mut self, edge: HotkeyEdge, source: HotkeySource, now_ms: u64) -> Option<TakeAction> {
        let delivered = match source {
            HotkeySource::Carbon => self.status.carbon == CarbonStatus::Registered,
            HotkeySource::FnTap => self.status.fn_tap == FnTapStatus::Delivering,
        };
        if !delivered {
            return None;
        }
        let a = self.arb.edge(edge, source, self.recording, now_ms);
        match a {
            TakeAction::Start => {
                self.recording = true;
                self.starts += 1;
            }
            TakeAction::Stop => {
                self.recording = false;
                self.stops += 1;
            }
            TakeAction::Ignore => {}
        }
        Some(a)
    }
}

#[test]
fn zero_grants_dictate_toggle_starts_and_stops() {
    let mut carbon = FakeCarbon::ok();
    let mut tap = FakeTap::refused();
    let mut sim = Sim::armed(&mut carbon, &Denied, &mut tap);
    assert_eq!(sim.status.carbon, CarbonStatus::Registered);
    assert_eq!(carbon.registered, vec![DICTATE_TOGGLE.shortcut()]);
    // A quick tap of the chord toggles a take on...
    assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::Carbon, 0), Some(TakeAction::Start));
    assert_eq!(sim.edge(HotkeyEdge::Release, HotkeySource::Carbon, 90), Some(TakeAction::Ignore));
    let started = sim.recording;
    assert!(started, "the chord must start a take with every grant denied");
    // ...and the next press stops it.
    assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::Carbon, 4_000), Some(TakeAction::Stop));
    assert_eq!(sim.edge(HotkeyEdge::Release, HotkeySource::Carbon, 4_080), Some(TakeAction::Ignore));
    let stopped = !sim.recording;
    assert!(stopped);
    assert_eq!((sim.starts, sim.stops), (1, 1));
    // The fn tap delivered nothing — the chord alone did this.
    assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::FnTap, 5_000), None);
    println!("hotkey_no_tcc: grants=none source=carbon started={started} stopped={stopped}");
}

#[test]
fn zero_grants_hold_release_stops_take() {
    assert_eq!(hotkeys::CARBON_HOLD_MS, wilson_voice_lib::ptt_macos::HOLD_ARM_MS);
    let mut carbon = FakeCarbon::ok();
    let mut tap = FakeTap::refused();
    let mut sim = Sim::armed(&mut carbon, &Denied, &mut tap);
    assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::Carbon, 1_000), Some(TakeAction::Start));
    assert!(sim.recording);
    // Held past the arm window, then released: hold-to-talk stops the take.
    let release_at = 1_000 + hotkeys::CARBON_HOLD_MS + 1_200;
    assert_eq!(sim.edge(HotkeyEdge::Release, HotkeySource::Carbon, release_at), Some(TakeAction::Stop));
    assert!(!sim.recording);
    assert_eq!((sim.starts, sim.stops), (1, 1));
}

#[test]
fn zero_grants_fn_tap_attempted_status_blocked() {
    let mut carbon = FakeCarbon::ok();
    let mut tap = FakeTap::refused();
    let sim = Sim::armed(&mut carbon, &Denied, &mut tap);
    // Attempted even though the probe said "denied" — never gated on it.
    assert_eq!(tap.attempts, 1);
    assert!(matches!(sim.status.fn_tap, FnTapStatus::Blocked(_)));
    assert_ne!(sim.status.fn_tap, FnTapStatus::NotAttempted);
    let line = hotkeys::degraded_line(&sim.status.fn_tap, false, DICTATE_TOGGLE.label)
        .expect("a blocked tap is a degraded mode, said out loud");
    assert!(line.starts_with("fn is off — "), "{line}");
    assert!(line.ends_with("Use ⌃⌥⌘Space (works without permissions)."), "{line}");
}

#[test]
fn failed_registration_never_reported_registered() {
    let mut carbon = FakeCarbon::failing("HotKey already registered");
    let mut tap = FakeTap::refused();
    let mut sim = Sim::armed(&mut carbon, &Denied, &mut tap);
    assert_eq!(sim.status.carbon, CarbonStatus::Failed("HotKey already registered".into()));
    assert_ne!(sim.status.carbon, CarbonStatus::Registered);
    let json = serde_json::to_value(&sim.status).unwrap();
    assert_eq!(json["carbon"]["state"], "failed");
    assert_eq!(json["fnTap"]["state"], "blocked");
    // An unregistered chord is never delivered, so it cannot start anything.
    assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::Carbon, 0), None);
    assert!(!sim.recording);
    // The old unconditional flag is gone, and lib.rs records each register result.
    assert!(!LIB.contains("hotkey_registered"));
    assert!(LIB.contains("hotkeys::set_carbon_status(hotkeys::CarbonStatus::from_result(&r))"));
    assert!(LIB.contains("sink.carbon") || LIB.contains("fn carbon(&mut self, s: hotkeys::CarbonStatus)"));
}

#[test]
fn secure_input_carbon_still_starts_take() {
    let mut carbon = FakeCarbon::ok();
    let mut tap = FakeTap::new(vec![Ok(TapAttempt::Installed)]);
    let mut sim = Sim::armed(&mut carbon, &Granted, &mut tap);
    hotkeys::note_delivered_edge(&mut sim.status);
    // Secure Input on: the tap is installed but macOS hides every key from it.
    let shown = hotkeys::effective_fn_tap(&sim.status.fn_tap, true, Some("Terminal (pid 431)"));
    assert_eq!(shown, FnTapStatus::SecureInput("Terminal".into()));
    sim.status.fn_tap = shown.clone();
    let line = hotkeys::degraded_line(&shown, true, DICTATE_TOGGLE.label).unwrap();
    assert!(line.contains("Terminal has Secure Keyboard Entry on"), "{line}");
    // fn is blind, the Carbon chord is not.
    assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::FnTap, 0), None);
    assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::Carbon, 10), Some(TakeAction::Start));
    assert!(sim.recording);
}

#[test]
fn grant_arriving_later_arms_tap_without_restart() {
    let mut carbon = FakeCarbon::ok();
    let mut tap = FakeTap::new(vec![Err("denied".into()), Ok(TapAttempt::Installed)]);
    let mut sim = Sim::armed(&mut carbon, &Denied, &mut tap);
    assert_eq!(sim.status.fn_tap, FnTapStatus::Blocked("denied".into()));
    // The permission watcher fires an edge when the grant lands: re-attempt.
    assert!(hotkeys::attempt_tap(&Granted, &mut tap, &mut sim.status));
    assert_eq!(sim.status.fn_tap, FnTapStatus::ArmedNoDelivery);
    hotkeys::note_delivered_edge(&mut sim.status);
    assert_eq!(sim.status.fn_tap, FnTapStatus::Delivering);
    // A later edge does not reinstall a working tap.
    assert!(!hotkeys::attempt_tap(&Granted, &mut tap, &mut sim.status));
    assert_eq!(tap.attempts, 2);
    assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::FnTap, 0), Some(TakeAction::Start));
}

#[test]
fn valid_grants_fn_start_stop_unchanged() {
    let mut carbon = FakeCarbon::ok();
    let mut tap = FakeTap::new(vec![Ok(TapAttempt::Installed)]);
    let mut sim = Sim::armed(&mut carbon, &Granted, &mut tap);
    hotkeys::note_delivered_edge(&mut sim.status);
    assert_eq!(hotkeys::degraded_line(&sim.status.fn_tap, true, DICTATE_TOGGLE.label), None);
    // PttEvent::Start → Press, PttEvent::Stop → Release: exactly as on main.
    for round in 0..3u64 {
        let t = round * 10_000;
        assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::FnTap, t), Some(TakeAction::Start));
        assert_eq!(sim.edge(HotkeyEdge::Release, HotkeySource::FnTap, t + 50), Some(TakeAction::Stop));
    }
    assert_eq!((sim.starts, sim.stops), (3, 3));
    // Both PttEvent arms route through the one dispatcher.
    assert!(LIB.contains("take_hotkey_edge(&h, &st, HotkeyEdge::Press, HotkeySource::FnTap)"));
    assert!(LIB.contains("take_hotkey_edge(&h, &st, HotkeyEdge::Release, HotkeySource::FnTap)"));
}

#[test]
fn probe_denied_but_tap_delivering_stays_armed() {
    let mut status = HotkeysStatus::new();
    status.fn_tap = FnTapStatus::Delivering;
    let mut tap = FakeTap::new(vec![]);
    // The probe says "not trusted" (yap.log 07:40:34Z) while fn delivers.
    assert!(!hotkeys::attempt_tap(&Denied, &mut tap, &mut status));
    assert_eq!(tap.attempts, 0);
    assert_eq!(status.fn_tap, FnTapStatus::Delivering);
    assert_eq!(hotkeys::degraded_line(&status.fn_tap, false, DICTATE_TOGGLE.label), None);
}

#[test]
fn carbon_and_fn_never_double_start() {
    let mut carbon = FakeCarbon::ok();
    let mut tap = FakeTap::new(vec![Ok(TapAttempt::Installed)]);
    let mut sim = Sim::armed(&mut carbon, &Granted, &mut tap);
    hotkeys::note_delivered_edge(&mut sim.status);
    // Chord starts; fn while it is live is never a second start.
    assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::Carbon, 0), Some(TakeAction::Start));
    assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::FnTap, 100), Some(TakeAction::Ignore));
    assert_eq!(sim.starts, 1);
    // ...only a stop.
    assert_eq!(sim.edge(HotkeyEdge::Release, HotkeySource::FnTap, 900), Some(TakeAction::Stop));
    // fn starts; the chord while it is live is a stop, never a start.
    assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::FnTap, 2_000), Some(TakeAction::Start));
    assert_eq!(sim.edge(HotkeyEdge::Press, HotkeySource::Carbon, 2_500), Some(TakeAction::Stop));
    assert_eq!(sim.edge(HotkeyEdge::Release, HotkeySource::Carbon, 2_600), Some(TakeAction::Ignore));
    assert_eq!((sim.starts, sim.stops), (2, 2));
    // The table still has no collisions with the new chord in it.
    assert_eq!(shortcuts::first_collision(), None);
}
