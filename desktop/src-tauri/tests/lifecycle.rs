//! yap-lifecycle — Dock reopen, one Quit, the pill first, a bounded teardown,
//! and the stale-instance handoff. Every test here is headless: no GUI app is
//! launched (a GUI launch of a second copy would reach the single-instance
//! plugin and focus the user's running Yap), and the handoff tests use their
//! own sockets under a temp dir, never the real `/tmp/<bundle>_si.sock`.

use std::io::Read;
use std::os::unix::net::UnixListener;
use std::time::{Duration, Instant};

use wilson_voice_lib::lifecycle::{
    self, activation_action, handoff_to_running, quit_plan, run_bounded, ActivationSource,
    BoundedStep, HandoffVerdict, MainWindowState, QuitStep, StepOutcome, WindowAction,
};

fn hidden() -> MainWindowState {
    MainWindowState {
        exists: true,
        visible: false,
        minimized: false,
        focused: false,
    }
}

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("yap-lifecycle-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The bug: red X hides the window, the pill (an NSPanel) stays on screen, so
/// AppKit's Reopen says has_visible_windows = TRUE. The old code handled no
/// Reopen at all; a naive handler that trusts the flag would still do nothing.
#[test]
fn dock_reopen_presents_a_hidden_window_even_with_the_pill_visible() {
    for has_visible_windows in [false, true] {
        assert_eq!(
            activation_action(
                ActivationSource::DockReopen {
                    has_visible_windows
                },
                hidden()
            ),
            WindowAction::Present { unminimize: false },
            "has_visible_windows={has_visible_windows}"
        );
    }
}

#[test]
fn every_activation_path_shares_one_decision() {
    let minimized = MainWindowState {
        minimized: true,
        visible: true,
        ..hidden()
    };
    let up = MainWindowState {
        visible: true,
        focused: true,
        ..hidden()
    };
    let behind = MainWindowState {
        visible: true,
        ..hidden()
    };
    for src in [
        ActivationSource::DockReopen {
            has_visible_windows: false,
        },
        ActivationSource::SecondInstance,
        ActivationSource::TrayOpen,
    ] {
        assert_eq!(
            activation_action(src, minimized),
            WindowAction::Present { unminimize: true }
        );
        assert_eq!(activation_action(src, up), WindowAction::Nothing);
        assert_eq!(
            activation_action(src, behind),
            WindowAction::Present { unminimize: false }
        );
        assert_eq!(
            activation_action(src, MainWindowState::default()),
            WindowAction::Nothing,
            "no window: nothing to present"
        );
    }
}

#[test]
fn quit_plan_orders_the_pill_out_first_and_checkpoints_last() {
    let plan = quit_plan();
    assert_eq!(plan[0], QuitStep::PillOrderOut);
    assert!(plan[0].on_main_thread());
    let pos = |s: QuitStep| plan.iter().position(|p| *p == s).unwrap();
    assert!(pos(QuitStep::PillOrderOut) < pos(QuitStep::MeetingStop));
    assert!(pos(QuitStep::MeetingStop) < pos(QuitStep::DbCheckpoint));
    assert_eq!(*plan.last().unwrap(), QuitStep::DbCheckpoint);
    let sum: Duration = plan.iter().map(|s| s.deadline()).sum();
    assert!(sum <= lifecycle::QUIT_BUDGET, "deadlines sum to {sum:?}");
    assert!(lifecycle::QUIT_BUDGET <= Duration::from_secs(3));
}

#[test]
fn a_step_that_misses_its_deadline_is_skipped_and_quit_stays_bounded() {
    let order = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let o = |name: &'static str| {
        let order = order.clone();
        move || order.lock().push(name)
    };
    let steps = vec![
        BoundedStep::new(
            "pill_order_out",
            Duration::from_millis(100),
            true,
            o("pill"),
        ),
        BoundedStep::new("meeting_stop", Duration::from_millis(150), false, || {
            std::thread::sleep(Duration::from_secs(10)) // a wedged meeting stop
        }),
        BoundedStep::new("explodes", Duration::from_millis(150), false, || {
            panic!("boom")
        }),
        BoundedStep::new(
            "db_checkpoint",
            Duration::from_millis(150),
            false,
            o("ckpt"),
        ),
    ];
    let t = Instant::now();
    let report = run_bounded(steps, Duration::from_millis(3_000));
    assert!(
        t.elapsed() < Duration::from_millis(1_000),
        "{:?}",
        t.elapsed()
    );
    assert_eq!(report.outcome_of("pill_order_out"), Some(StepOutcome::Done));
    assert_eq!(
        report.outcome_of("meeting_stop"),
        Some(StepOutcome::TimedOut)
    );
    assert_eq!(report.outcome_of("explodes"), Some(StepOutcome::Panicked));
    assert_eq!(report.outcome_of("db_checkpoint"), Some(StepOutcome::Done));
    assert_eq!(*order.lock(), vec!["pill", "ckpt"]);
    let line = report.log_line();
    assert!(line.starts_with("exit teardown "), "{line}");
    assert!(
        line.contains("meeting_stop=") && line.contains("(timeout)"),
        "{line}"
    );
}

#[test]
fn the_whole_quit_never_exceeds_its_budget_when_every_step_hangs() {
    let steps = quit_plan()
        .into_iter()
        .filter(|s| !s.on_main_thread())
        .map(|s| {
            BoundedStep::new(s.name(), s.deadline(), false, || {
                std::thread::sleep(Duration::from_secs(30))
            })
        })
        .collect();
    let budget = Duration::from_millis(600);
    let t = Instant::now();
    let report = run_bounded(steps, budget);
    assert!(
        t.elapsed() < budget + Duration::from_millis(200),
        "{:?}",
        t.elapsed()
    );
    assert!(report
        .steps
        .iter()
        .any(|s| s.outcome == StepOutcome::Skipped));
}

#[test]
fn hide_hint_is_shown_exactly_once_per_install() {
    let d = tmpdir("hint");
    let gate = lifecycle::HideHintGate::new();
    assert!(gate.note_hide(&d));
    assert!(gate.claim(&d));
    assert!(lifecycle::hide_hint_marker(&d).exists());
    assert!(!gate.claim(&d));
    assert!(!gate.note_hide(&d), "a later hide owes nothing");
    assert!(!gate.claim(&d));
}

/// Review BLOCKING 2 (fix round 1): the red X during first-run onboarding hid
/// the window while HideHint was not mounted. The old take_hide_hint wrote the
/// marker AT THE HIDE, so the event reached no listener and the hint was gone
/// for good. A hide now only makes the hint pending; nothing is written until a
/// mounted listener claims it — which the main view does when it mounts.
#[test]
fn a_hide_with_no_mounted_listener_does_not_use_up_the_hint() {
    let d = tmpdir("hint-onboarding");
    let gate = lifecycle::HideHintGate::new();
    // The red X during onboarding: nudge emitted, nobody listening.
    assert!(gate.note_hide(&d));
    assert!(
        !lifecycle::hide_hint_marker(&d).exists(),
        "a hide must not write the marker — nothing has shown the hint yet"
    );
    // A second hide while still onboarding: still owed, still unwritten.
    assert!(gate.note_hide(&d));
    assert!(!lifecycle::hide_hint_marker(&d).exists());
    // Onboarding finishes; the main view mounts HideHint, which claims.
    assert!(gate.claim(&d), "the mounted HideHint gets the pending hint");
    assert!(lifecycle::hide_hint_marker(&d).exists());
    assert!(!gate.claim(&d));
}

#[test]
fn a_claim_without_a_hide_shows_nothing_and_writes_nothing() {
    let d = tmpdir("hint-nohide");
    let gate = lifecycle::HideHintGate::new();
    // HideHint mounts at every launch and claims; with no hide it must not
    // show the note or spend the marker.
    assert!(!gate.claim(&d));
    assert!(!lifecycle::hide_hint_marker(&d).exists());
}

#[test]
fn the_red_x_does_not_write_the_hint_marker_itself() {
    // Wiring guard: the CloseRequested arm notes the hide; only the
    // claim_hide_hint command (called by a mounted HideHint) consumes it.
    let lib = include_str!("../src/lib.rs");
    let close = lib
        .find("WindowEvent::CloseRequested { api, .. } =>")
        .expect("main window CloseRequested arm");
    let arm = &lib[close..close + 1200];
    assert!(
        arm.contains("lifecycle::HIDE_HINT.note_hide(&data_dir())"),
        "{arm}"
    );
    assert!(!arm.contains(".claim("), "{arm}");
    assert!(lib.contains("fn claim_hide_hint() -> bool"));
    assert!(lib.contains("lifecycle::HIDE_HINT.claim(&data_dir())"));
    assert!(
        lib.contains("            claim_hide_hint,\n"),
        "registered in generate_handler"
    );
}

#[test]
fn handoff_with_no_running_instance_launches_normally() {
    let d = tmpdir("none");
    let v = handoff_to_running(
        &d.join("si.sock"),
        &d.join("si.ack"),
        "/",
        &["yap".into()],
        Duration::from_millis(200),
    );
    assert_eq!(v, HandoffVerdict::NoRunningInstance);
}

#[test]
fn handoff_acknowledged_by_the_running_instance_is_handed() {
    let d = tmpdir("ack");
    let sock = d.join("si.sock");
    let ack = d.join("si.ack");
    let listener = UnixListener::bind(&sock).unwrap();
    let ack2 = ack.clone();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut got = String::new();
        s.read_to_string(&mut got).unwrap();
        lifecycle::write_handoff_ack(&ack2);
        got
    });
    let v = handoff_to_running(
        &sock,
        &ack,
        "/cwd",
        &["yap".into(), "--x".into()],
        Duration::from_millis(2_000),
    );
    assert!(matches!(v, HandoffVerdict::Handed { .. }), "{v:?}");
    // Same wire format the single-instance plugin parses.
    assert_eq!(server.join().unwrap(), "/cwd\0\0yap\0--x");
}

/// A running Yap whose main thread is wedged: the kernel still accepts the
/// connection (the listener's backlog), so the plugin's write "succeeds" and
/// the old second launch exited 0 in silence. Now it waits 2 s and says so.
#[test]
fn handoff_with_no_answer_is_reported_stale_not_silent() {
    let d = tmpdir("stale");
    let sock = d.join("si.sock");
    let _listener = UnixListener::bind(&sock).unwrap(); // never accepts, never acks
    let t = Instant::now();
    let v = handoff_to_running(
        &sock,
        &d.join("si.ack"),
        "/",
        &["yap".into()],
        Duration::from_millis(300),
    );
    assert!(matches!(v, HandoffVerdict::Stale { .. }), "{v:?}");
    assert!(t.elapsed() >= Duration::from_millis(300));
    assert_eq!(lifecycle::HANDOFF_ACK_TIMEOUT, Duration::from_secs(2));
    assert!(lifecycle::stale_message(2000).contains("did not answer"));
}

#[test]
fn bundle_identifier_matches_tauri_conf_and_socket_matches_the_plugin() {
    // On a production build the channel's identifier is the shipped one, and
    // the socket is the plugin's production path.
    let conf: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        wilson_voice_lib::instance_identifier(),
        wilson_voice_lib::channel::current().identifier
    );
    if wilson_voice_lib::channel::CURRENT == wilson_voice_lib::channel::Channel::Production {
        assert_eq!(
            conf["identifier"].as_str().unwrap(),
            wilson_voice_lib::instance_identifier()
        );
        assert_eq!(
            lifecycle::handoff_socket(wilson_voice_lib::instance_identifier()),
            std::path::PathBuf::from("/tmp/com_wilsonguenther_wilson_voice_si.sock")
        );
    }
}

/// Review BLOCKING 1 (fix round 1): Yap Test (bundle id
/// com.wilsonguenther.wilson-voice.test, #238) must probe ITS OWN plugin
/// socket, not production's. With the old hard-coded production id, a Yap
/// Test launch while Yap ran was answered by production's window and exited 0.
#[test]
fn each_channel_probes_its_own_single_instance_socket() {
    use wilson_voice_lib::channel::Channel;
    let prod = Channel::Production.profile().identifier;
    let test = Channel::Test.profile().identifier;
    assert_eq!(
        lifecycle::handoff_socket(test),
        std::path::PathBuf::from("/tmp/com_wilsonguenther_wilson_voice_test_si.sock")
    );
    assert_ne!(
        lifecycle::handoff_socket(test),
        lifecycle::handoff_socket(prod)
    );
    assert_ne!(
        lifecycle::handoff_ack_file(test),
        lifecycle::handoff_ack_file(prod)
    );
    // Wiring guard: every probe / ack path in lib.rs is derived from the
    // channel, never a literal or a fixed constant.
    let lib = include_str!("../src/lib.rs");
    for call in ["lifecycle::handoff_socket(", "lifecycle::handoff_ack_file("] {
        let mut n = 0;
        for (i, _) in lib.match_indices(call) {
            n += 1;
            let arg = &lib[i + call.len()..i + call.len() + 22];
            assert!(
                arg.starts_with("instance_identifier()"),
                "{call} takes `{arg}` — must be instance_identifier()"
            );
        }
        assert!(n >= 1, "{call} not found in lib.rs");
    }
    assert!(lib.contains("channel::current().identifier"));
}

#[test]
fn rss_growth_verdict_matches_the_soak_rule() {
    assert!(!lifecycle::rss_growth_exceeds(
        &[500, 900, 1000, 1050, 1090],
        3,
        10.0
    ));
    assert!(lifecycle::rss_growth_exceeds(
        &[500, 900, 1000, 1050, 1101],
        3,
        10.0
    ));
    assert!(!lifecycle::rss_growth_exceeds(&[1000, 2000], 3, 10.0));
}
