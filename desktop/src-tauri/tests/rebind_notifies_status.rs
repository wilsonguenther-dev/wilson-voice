//! yap-p0-hotkeys-no-permission, fix round 2 (primary reviewer BLOCKING): a
//! successful Settings rebind must re-emit `status`, or every surface that
//! names the dictate chord (Settings → Shortcut "Works everywhere", the
//! Permissions row, the Insights row, the zero-grant home banner) keeps
//! advertising the chord that was just unregistered. Carbon goes
//! Registered → Registered on a rebind, which is no change, so the chord swap
//! itself has to notify. This binary owns the process-wide `on_status_change`
//! hook (a OnceLock), so it lives in its own test file.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};

use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};
use wilson_voice_lib::hotkeys::{self, CarbonRegistrar, CarbonStatus, FnTapStatus, StatusSink};
use wilson_voice_lib::shortcuts::DICTATE_TOGGLE;

static NOTIFIES: AtomicUsize = AtomicUsize::new(0);

const LIB: &str = include_str!("../src/lib.rs");

#[derive(Default)]
struct FakeCarbon {
    live: HashSet<Shortcut>,
    refuse: Option<Shortcut>,
}

impl CarbonRegistrar for FakeCarbon {
    fn register(&mut self, chord: Shortcut) -> Result<(), String> {
        if self.refuse == Some(chord) {
            return Err("another app or macOS already uses it, eventHotKeyExistsErr -9878".into());
        }
        self.live.insert(chord);
        Ok(())
    }
    fn unregister(&mut self, chord: Shortcut) -> Result<(), String> {
        self.live.remove(&chord);
        Ok(())
    }
    fn is_registered(&self, chord: Shortcut) -> bool {
        self.live.contains(&chord)
    }
}

/// The app's sink: writes the PROCESS-WIDE status, like lib.rs `AppHotkeySink`.
struct GlobalSink;

impl StatusSink for GlobalSink {
    fn carbon(&mut self, s: CarbonStatus) {
        hotkeys::set_carbon_status(s);
    }
    fn fn_tap(&mut self, s: FnTapStatus) {
        hotkeys::set_fn_tap_status(s);
    }
    fn current_fn_tap(&self) -> FnTapStatus {
        hotkeys::status().fn_tap
    }
}

fn chord(code: Code) -> Shortcut {
    Shortcut::new(
        Some(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER),
        code,
    )
}

#[test]
fn successful_rebind_reemits_status_with_new_label() {
    hotkeys::on_status_change(|| {
        NOTIFIES.fetch_add(1, Ordering::SeqCst);
    });
    let default = DICTATE_TOGGLE.shortcut();
    // Launch state: the default chord registered and active.
    let mut carbon = FakeCarbon::default();
    carbon.live.insert(default);
    hotkeys::set_active_dictate(default, DICTATE_TOGGLE.label.to_string());
    hotkeys::set_carbon_status(CarbonStatus::Registered);
    NOTIFIES.store(0, Ordering::SeqCst);

    // Refused rebind: old chord stays active, nothing re-emitted.
    let held = chord(Code::KeyK);
    carbon.refuse = Some(held);
    let err = hotkeys::rebind_and_activate(&mut carbon, held, "⌃⌥⌘K".into(), &mut GlobalSink)
        .expect_err("a held chord must be refused");
    assert!(err.contains("kept"), "{err}");
    assert_eq!(hotkeys::active_dictate().1, DICTATE_TOGGLE.label);
    assert!(carbon.is_registered(default));
    assert_eq!(NOTIFIES.load(Ordering::SeqCst), 0);

    // Successful rebind: the new chord is active AND `status` is re-emitted.
    let new = chord(Code::KeyD);
    hotkeys::rebind_and_activate(&mut carbon, new, "⌃⌥⌘D".into(), &mut GlobalSink)
        .expect("a free chord rebinds");
    let notifies = NOTIFIES.load(Ordering::SeqCst);
    let (active, label) = hotkeys::active_dictate();
    println!(
        "rebind_notify: old_registered={} new_registered={} notifies_during_rebind={notifies} active_label={label}",
        carbon.is_registered(default),
        carbon.is_registered(new),
    );
    assert!(!carbon.is_registered(default));
    assert!(carbon.is_registered(new));
    assert_eq!(active, new);
    assert_eq!(label, "⌃⌥⌘D");
    assert_eq!(hotkeys::status().carbon, CarbonStatus::Registered);
    assert!(
        notifies >= 1,
        "a successful rebind must re-emit status, or the UI keeps naming the dropped chord"
    );

    // Re-saving the same chord is no change: no extra emit.
    NOTIFIES.store(0, Ordering::SeqCst);
    hotkeys::rebind_and_activate(&mut carbon, new, "⌃⌥⌘D".into(), &mut GlobalSink).unwrap();
    assert_eq!(NOTIFIES.load(Ordering::SeqCst), 0);
}

#[test]
fn save_settings_rebind_goes_through_rebind_and_activate() {
    // Reachability: the shipping rebind (save_settings → rebind_dictate_toggle)
    // is the function the test above drives, and the hook it notifies is the
    // one lib.rs wires to emit_status.
    let body = LIB
        .split("fn rebind_dictate_toggle(")
        .nth(1)
        .expect("rebind_dictate_toggle exists");
    let body = &body[..body.find("\n}\n").unwrap()];
    assert!(body.contains("hotkeys::rebind_and_activate("), "{body}");
    // fix-249 advisory (b): save_settings applies the rebind after persisting
    // and routes a refusal through its rollback.
    assert!(LIB.contains("if let Err(e) = rebind_dictate_toggle(&app, &next) {"));
    let hook = LIB.split("hotkeys::on_status_change(").nth(1).unwrap();
    assert!(hook[..400].contains("emit_status(&h, &st)"));
}
