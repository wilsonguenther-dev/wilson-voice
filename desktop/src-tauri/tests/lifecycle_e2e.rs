//! Y6-D — the app-wide lifecycle events the meeting matrix (rows 12, 14, 15, 16
//! and the offline phase) covers only for MEETINGS: cold launch, sleep/wake,
//! a display change, fullscreen, an output-device swap mid-take, a second
//! copy of Yap, and the TCC grants across a relaunch.
//!
//! WHAT THIS FILE CAN AND CANNOT PROVE (PANEL 2026-09-12, correction b). A
//! `cargo test` process has no window-server session: it cannot sleep the Mac,
//! unplug a monitor, put an app in fullscreen, launch a second Yap or change a
//! TCC grant. So every test below is at the STATE-MACHINE level — the decision
//! the running app makes, through the same function the running app calls —
//! and is named after that decision, not after the physical event. The events
//! themselves are rows in `docs/RELEASE.md` → "LIFECYCLE MANUAL CHECKLIST",
//! and the last two tests here only assert that those rows exist.
//!
//! Every test is headless: no GUI launch, no real CoreAudio write, no real
//! data directory (locks and settings live under a per-test temp dir).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use wilson_voice_lib::float_pill::{
    clamp_to_screen, float_window_size_pt, launch_pill_visible, pill_origin_on_visible_screen,
    pill_screen, PillPosition, ScreenRect,
};
use wilson_voice_lib::lifecycle::{
    self, acquire_instance_lock, activation_action, instance_lock_path, launch_decision,
    ActivationSource, InstanceLock, LaunchDecision, MainWindowState, WindowAction,
};
use wilson_voice_lib::meeting_matrix::SleepEvent;
use wilson_voice_lib::power;
use wilson_voice_lib::sysaudio::{restore_target, OutputAudioState};

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("yap-y6d-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn src(rel: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn repo(rel: &str) -> String {
    src(&format!("../../{rel}"))
}

/// The body of `pub fn run()` in lib.rs — where launch order is decided.
fn run_body(lib: &str) -> &str {
    let start = lib.find("pub fn run() {").expect("lib.rs has pub fn run()");
    &lib[start..]
}

// ─── COLD LAUNCH ─────────────────────────────────────────────────────────

#[test]
fn cold_launch_with_nothing_installed_never_shows_a_ready_pill() {
    // No settings file at all — the state of a Mac that never ran Yap.
    let dir = tmpdir("cold");
    let settings = wilson_voice_lib::load_settings(&dir.join("settings.json"));
    assert!(
        !settings.onboarded,
        "a cold launch must land on onboarding (onboarded defaults to false)"
    );
    assert!(
        settings.show_floating_pill,
        "precondition: the always-on pill setting defaults ON, which is why the launch rule matters"
    );
    assert!(
        !launch_pill_visible(settings.show_floating_pill, settings.onboarded),
        "cold launch: no model, no grants, onboarding on screen — an idle 'ready' pill there \
         promises a dictation the app cannot take"
    );

    // The full table.
    assert!(
        launch_pill_visible(true, true),
        "onboarded + setting on → pill up"
    );
    assert!(
        !launch_pill_visible(false, true),
        "setting off → never at launch"
    );
    assert!(!launch_pill_visible(false, false));

    // And the launch actually consults it: the setup block's first show of the
    // pill is gated on `launch_pill_visible`, not on the setting alone.
    let lib = src("src/lib.rs");
    let gate = lib
        .find("if float_pill::launch_pill_visible(show_setting, onboarded) {")
        .expect("setup gates the launch show on launch_pill_visible");
    let show = lib[gate..]
        .find("float_pill::show_float(app.handle())")
        .expect("show_float inside the gate");
    assert!(
        show < 300,
        "show_float must be the gated branch, not a later call"
    );
}

// ─── SLEEP / WAKE ────────────────────────────────────────────────────────

#[test]
fn sleep_mid_take_parks_the_dictation_take_and_the_dictation_path_subscribes_to_power_hub() {
    // The decision table the dictation consumer runs.
    assert!(power::dictation_should_park_take(
        SleepEvent::WillSleep,
        true
    ));
    assert!(!power::dictation_should_park_take(
        SleepEvent::WillSleep,
        false
    ));
    assert!(!power::dictation_should_park_take(
        SleepEvent::DidWake,
        true
    ));
    assert!(!power::dictation_should_park_take(
        SleepEvent::DidWake,
        false
    ));

    // The hub fan-out: a dictation consumer subscribed beside the meeting one
    // hears the SAME will-sleep and parks exactly once.
    let hub = power::SleepWakeHub::new();
    let parked = Arc::new(AtomicUsize::new(0));
    let in_flight = Arc::new(std::sync::atomic::AtomicBool::new(true));
    {
        let parked = parked.clone();
        let in_flight = in_flight.clone();
        hub.subscribe(power::CONSUMER_DICTATION, move |ev| {
            if power::dictation_should_park_take(ev, in_flight.load(Ordering::SeqCst)) {
                in_flight.store(false, Ordering::SeqCst); // cancel_recording ends the take
                parked.fetch_add(1, Ordering::SeqCst);
            }
        });
    }
    hub.subscribe(power::CONSUMER_MEETINGS, |_| {});
    assert_eq!(hub.publish(SleepEvent::WillSleep), 2);
    // NSWorkspace AND IOKit both deliver will-sleep for one sleep.
    assert_eq!(hub.publish(SleepEvent::WillSleep), 2);
    assert_eq!(hub.publish(SleepEvent::DidWake), 2);
    assert_eq!(
        parked.load(Ordering::SeqCst),
        1,
        "parked once, never twice, never on wake"
    );

    // The app wires it: lib.rs subscribes to power::SleepWakeHub as the
    // dictation consumer, and the consumer parks through cancel_recording
    // (Y3-D keeps the clip recoverable) — never a half-written take.
    let lib = src("src/lib.rs");
    let sub = lib
        .find("hub.subscribe(power::CONSUMER_DICTATION")
        .expect("the dictation path subscribes to power::SleepWakeHub");
    let body = &lib[sub..sub + 1500];
    assert!(body.contains("power::dictation_should_park_take(event, in_flight)"));
    assert!(body.contains("cancel_recording(&dict_app, &dict_state)"));
    assert!(lib.contains("power::install_sleep_wake_observer(hub)"));
}

// ─── DISPLAY CHANGE ──────────────────────────────────────────────────────

const BUILTIN: ScreenRect = ScreenRect {
    x: 0,
    y: 0,
    w: 3024,
    h: 1964,
    scale: 2.0,
};
/// An external display arranged to the LEFT of the built-in: negative x.
const EXTERNAL_LEFT: ScreenRect = ScreenRect {
    x: -2560,
    y: 0,
    w: 2560,
    h: 1440,
    scale: 1.0,
};

fn win_px(s: ScreenRect) -> (i32, i32) {
    let (w, h) = float_window_size_pt();
    ((w * s.scale) as i32, (h * s.scale) as i32)
}

fn fully_on(origin: (i32, i32), win: (i32, i32), s: ScreenRect) -> bool {
    origin.0 >= s.x
        && origin.1 >= s.y
        && origin.0 + win.0 <= s.x + s.w
        && origin.1 + win.1 <= s.y + s.h
}

#[test]
fn display_change_lands_the_pill_on_a_visible_screen() {
    let all = [
        PillPosition::Bottom,
        PillPosition::Left,
        PillPosition::Right,
    ];

    // 1. The external display was the primary and is unplugged. macOS promotes
    //    the built-in, but a primary looked up a moment earlier can still name
    //    the gone display. The pill must land on a screen that EXISTS.
    let after_unplug = [BUILTIN];
    for pos in all {
        let o = pill_origin_on_visible_screen(
            &after_unplug,
            Some(EXTERNAL_LEFT),
            float_window_size_pt(),
            14.0,
            pos,
        )
        .expect("a screen exists");
        assert!(
            fully_on(o, win_px(BUILTIN), BUILTIN),
            "{pos:?}: pill at {o:?} is not on the remaining built-in display"
        );
        assert!(
            o.0 >= 0,
            "{pos:?}: negative x {o:?} on a desk with no display left of 0"
        );
    }
    assert_eq!(
        pill_screen(&after_unplug, Some(EXTERNAL_LEFT)),
        Some(BUILTIN)
    );

    // 2. Both connected, built-in primary: stays on the primary, and the left
    //    display's legitimately-negative coordinates are never chosen.
    let both = [EXTERNAL_LEFT, BUILTIN];
    assert_eq!(pill_screen(&both, Some(BUILTIN)), Some(BUILTIN));
    for pos in all {
        let o =
            pill_origin_on_visible_screen(&both, Some(BUILTIN), float_window_size_pt(), 14.0, pos)
                .unwrap();
        assert!(fully_on(o, win_px(BUILTIN), BUILTIN), "{pos:?}: {o:?}");
    }

    // 3. The external is primary and stays plugged in: the pill docks there,
    //    negative x and all — that IS a visible screen.
    let o = pill_origin_on_visible_screen(
        &both,
        Some(EXTERNAL_LEFT),
        float_window_size_pt(),
        14.0,
        PillPosition::Bottom,
    )
    .unwrap();
    assert!(fully_on(o, win_px(EXTERNAL_LEFT), EXTERNAL_LEFT), "{o:?}");

    // 4. A screen smaller than the float window (a tiny sidecar / mirrored
    //    projector): pinned to the screen's origin, never off its top-left.
    let tiny = ScreenRect {
        x: 100,
        y: 50,
        w: 200,
        h: 100,
        scale: 1.0,
    };
    for pos in all {
        let o =
            pill_origin_on_visible_screen(&[tiny], Some(tiny), float_window_size_pt(), 14.0, pos)
                .unwrap();
        assert_eq!(o, (100, 50), "{pos:?}");
    }
    assert_eq!(clamp_to_screen((-500, -500), (10, 10), BUILTIN), (0, 0));
    assert_eq!(
        clamp_to_screen((99_999, 99_999), (10, 10), BUILTIN),
        (3014, 1954)
    );

    // 5. No screen reported at all (a headless moment): no placement, the
    //    window stays where it is rather than being sent to (0,0) of nothing.
    assert_eq!(
        pill_origin_on_visible_screen(
            &[],
            None,
            float_window_size_pt(),
            14.0,
            PillPosition::Bottom
        ),
        None
    );

    // The running app parks through this function, from the monitors that
    // exist on every keeper tick.
    let fp = src("src/float_pill.rs");
    let park = fp.find("fn park_pill(").unwrap();
    let body = &fp[park..park + 2600];
    assert!(body.contains(".available_monitors()"));
    assert!(body.contains("pill_origin_on_visible_screen("));
}

// ─── FULLSCREEN ──────────────────────────────────────────────────────────

/// NOT A MEASUREMENT. Whether the pill floats above a fullscreen app is a
/// window-server fact; a test process cannot put an app in fullscreen. What
/// this test pins is (a) the configuration the measurement has to be taken
/// against — the pill is ALREADY an `NSPanel` (tauri-nspanel, nonactivating,
/// `fullScreenAuxiliary` + `canJoinAllSpaces`, status level), which makes the
/// ROADMAP's "a normal NSWindow cannot float above fullscreen apps" premise
/// stale for this window — and (b) that the open answer is written down as a
/// named limitation and a manual row, not left unknown or claimed green.
#[test]
fn fullscreen_float_is_configured_for_it_but_unmeasured_and_written_down_as_such() {
    let fp = src("src/float_pill.rs");
    assert!(fp.contains("window\n        .to_panel::<DictatePill>()"));
    assert!(fp.contains(".full_screen_auxiliary()"));
    assert!(fp.contains(".can_join_all_spaces()"));
    assert!(fp.contains("PanelLevel::Status.value()"));
    assert!(fp.contains("nonactivating_panel()"));

    let arch = repo("ARCHITECTURE.md");
    assert!(
        arch.contains("FULLSCREEN — named limitation (Y6-D)"),
        "ARCHITECTURE.md names the fullscreen float as an unmeasured limitation"
    );
    let release = repo("docs/RELEASE.md");
    assert!(release.contains("Pill floats over a FULLSCREEN app"));
}

// ─── OUTPUT DEVICE CHANGE WHILE MUTED-FOR-DICTATION ──────────────────────

/// A Mac with output devices, modelled as CoreAudio's per-device mute+volume.
struct FakeMac {
    default: Option<u32>,
    devices: HashMap<u32, (bool, f32)>,
}

const SPEAKERS: u32 = 41;
const AIRPODS: u32 = 77;

impl FakeMac {
    fn new() -> Self {
        let mut devices = HashMap::new();
        devices.insert(SPEAKERS, (false, 0.7));
        devices.insert(AIRPODS, (false, 0.4));
        Self {
            default: Some(SPEAKERS),
            devices,
        }
    }
    /// `sysaudio::imp::mute_and_save`, step for step.
    fn mute_and_save(&mut self) -> Option<OutputAudioState> {
        let d = self.default?;
        let (muted, vol) = *self.devices.get(&d)?;
        if !muted {
            self.devices.get_mut(&d).unwrap().0 = true;
        }
        Some(OutputAudioState {
            muted,
            volume: Some(vol),
            device: Some(d),
        })
    }
    /// `sysaudio::imp::restore`, step for step: resolve the target through the
    /// SAME `restore_target` the app calls, write the plan if the device still
    /// exists (`has_property` fails closed on a gone device).
    fn restore(&mut self, saved: OutputAudioState) {
        let plan = saved.restore_plan();
        let Some(d) = restore_target(saved.device, self.default) else {
            return;
        };
        if let Some(dev) = self.devices.get_mut(&d) {
            dev.0 = plan.mute == 1;
            if let Some(v) = plan.volume {
                dev.1 = v;
            }
        }
    }
    fn any_muted(&self) -> bool {
        self.devices.values().any(|(m, _)| *m)
    }
}

#[test]
fn device_swap_mid_take_never_leaves_the_mac_muted() {
    // AirPods connect mid-take and become the default output.
    let mut mac = FakeMac::new();
    let saved = mac.mute_and_save().expect("speakers muted for the take");
    assert!(
        mac.devices[&SPEAKERS].0,
        "precondition: the take muted the speakers"
    );
    mac.default = Some(AIRPODS);
    mac.restore(saved);
    assert!(
        !mac.any_muted(),
        "a swap mid-take left a device muted: {:?}",
        mac.devices
    );
    assert_eq!(
        mac.devices[&SPEAKERS],
        (false, 0.7),
        "speakers back exactly"
    );
    assert_eq!(
        mac.devices[&AIRPODS],
        (false, 0.4),
        "the AirPods were never lent to Yap — their volume is untouched"
    );

    // The user had the speakers muted on purpose: a restore keeps them muted
    // and still never touches the new default.
    let mut mac = FakeMac::new();
    mac.devices.get_mut(&SPEAKERS).unwrap().0 = true;
    let saved = mac.mute_and_save().unwrap();
    mac.default = Some(AIRPODS);
    mac.restore(saved);
    assert!(mac.devices[&SPEAKERS].0, "the user's own mute is kept");
    assert_eq!(mac.devices[&AIRPODS], (false, 0.4));

    // The muted device was UNPLUGGED mid-take (headphones out): the write
    // aimed at it fails closed, and the device the Mac now plays through was
    // never muted by Yap. The re-plugged device's remembered mute is a manual
    // row (docs/RELEASE.md), not something this process can observe.
    let mut mac = FakeMac::new();
    let saved = mac.mute_and_save().unwrap();
    mac.devices.remove(&SPEAKERS);
    mac.default = Some(AIRPODS);
    mac.restore(saved);
    assert!(!mac.any_muted(), "{:?}", mac.devices);

    // The rule itself.
    assert_eq!(
        restore_target(Some(SPEAKERS), Some(AIRPODS)),
        Some(SPEAKERS)
    );
    assert_eq!(restore_target(None, Some(AIRPODS)), Some(AIRPODS));
    assert_eq!(restore_target(None, None), None);

    // And the real restore resolves through it.
    let sa = src("src/sysaudio.rs");
    assert!(sa.contains("super::restore_target(saved.device, default_output_device())"));
    assert!(sa.contains("device: Some(device),"));
}

// ─── SECOND INSTANCE ─────────────────────────────────────────────────────

#[test]
fn second_instance_focuses_the_first_and_exits() {
    // The FIRST instance, told about the second, presents its window.
    let hidden = MainWindowState {
        exists: true,
        visible: false,
        minimized: false,
        focused: false,
    };
    assert_eq!(
        activation_action(ActivationSource::SecondInstance, hidden),
        WindowAction::Present { unminimize: false }
    );
    let minimized = MainWindowState {
        minimized: true,
        ..hidden
    };
    assert_eq!(
        activation_action(ActivationSource::SecondInstance, minimized),
        WindowAction::Present { unminimize: true }
    );

    // The SECOND instance exits before it opens the database: the data
    // directory's lock is held by the first.
    let dir = tmpdir("si");
    let path = instance_lock_path(&dir);
    let first = acquire_instance_lock(&path);
    assert!(matches!(first, InstanceLock::Acquired(_)), "{first:?}");
    assert_eq!(launch_decision(&first), LaunchDecision::Proceed);
    let second = acquire_instance_lock(&path);
    assert!(matches!(second, InstanceLock::HeldByAnother), "{second:?}");
    assert_eq!(launch_decision(&second), LaunchDecision::ExitAlreadyRunning);
    // A different data directory (YAP_DATA_DIR smoke runs) is not blocked.
    let other = acquire_instance_lock(&instance_lock_path(&tmpdir("si-other")));
    assert!(matches!(other, InstanceLock::Acquired(_)));
    // The first quits (or crashes): the kernel drops the lock with the fd.
    drop(first);
    let third = acquire_instance_lock(&path);
    assert!(matches!(third, InstanceLock::Acquired(_)), "{third:?}");
    // A lock that cannot even be attempted does not stop a launch.
    let unavailable = InstanceLock::Unavailable("read-only".into());
    assert_eq!(launch_decision(&unavailable), LaunchDecision::Proceed);

    // Launch ORDER in run(): handoff probe → instance lock (exit when held) →
    // the ONE database open. A lock taken after the open would be the bug.
    let lib = src("src/lib.rs");
    let run = run_body(&lib);
    let handoff = run
        .find("lifecycle::handoff_to_running(")
        .expect("handoff in run()");
    let lock = run
        .find("lifecycle::acquire_instance_lock(")
        .expect("instance lock in run()");
    let exit = run
        .find("lifecycle::LaunchDecision::ExitAlreadyRunning")
        .expect("exit on a held lock");
    let db = run
        .find("open_db_graceful(db_path)")
        .expect("db open in run()");
    assert!(
        handoff < lock && lock < exit && exit < db,
        "order: {handoff} {lock} {exit} {db}"
    );
    assert!(run[exit..db].contains("std::process::exit(0)"));
    assert!(!lifecycle::ALREADY_RUNNING_MESSAGE.is_empty());
}

// ─── WHAT CARGO TEST CANNOT REACH ────────────────────────────────────────

#[test]
fn lifecycle_manual_checklist_names_what_cargo_test_cannot_reach() {
    let release = repo("docs/RELEASE.md");
    let at = release
        .find("LIFECYCLE MANUAL CHECKLIST")
        .expect("docs/RELEASE.md has the LIFECYCLE MANUAL CHECKLIST");
    let list = &release[at..];
    for row in [
        "Sleep the Mac mid-take",
        "Unplug the monitor the pill is on",
        "Pill floats over a FULLSCREEN app",
        "Swap the output device mid-take",
        "Launch Yap a second time",
        "TCC grants survive a relaunch",
        "Cold launch with nothing installed",
    ] {
        assert!(list.contains(row), "manual row missing: {row}");
    }
}
