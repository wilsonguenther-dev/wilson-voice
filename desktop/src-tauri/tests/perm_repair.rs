//! yap-p0-perm-repair — one truthful probe per service, StaleIdentity, Repair.
//!
//! Pure decisions only: no TCC read, no TCC write, no tccutil exec, no window
//! server. The live calls are exercised by the running app, never by a test.

use std::cell::Cell;
use std::time::Duration;
use wilson_voice_lib::mic_auth::MicAuth;
use wilson_voice_lib::perm_probe::{
    self, build_probe_report, classify, granted_phase, ledger_entry, needs_relaunch, phase_message,
    repair_argv, repair_pane_url, repair_plan, reprobe, ProbeState, RawAnswer, RawProbes,
    RepairPhase, Service, REPROBE_INTERVAL, REPROBE_TIMEOUT,
};
use wilson_voice_lib::perm_truth::{SigningSignature, TrustLedger};
use wilson_voice_lib::permissions::{self, GrantState};

const BUNDLE: &str = "com.wilsonguenther.wilson-voice";

fn sig(authority: &str, cdhash: &str, version: &str) -> SigningSignature {
    SigningSignature {
        authority: authority.into(),
        cdhash: cdhash.into(),
        version: version.into(),
    }
}

fn dev_id() -> SigningSignature {
    sig(
        "Developer ID Application: Wilson Guenther (VHYV8C2JNU)",
        "ca745694f2cc18904b90ea372ab28b8204dffd1f",
        "0.9.1",
    )
}

fn apple_dev() -> SigningSignature {
    sig(
        "Apple Development: Wilson Guenther (5K3X7Q2ABC)",
        "1111111111111111111111111111111111111111",
        "0.8.0",
    )
}

fn ledger_with(service: Service, s: SigningSignature) -> TrustLedger {
    let mut l = TrustLedger::default();
    match service {
        Service::Accessibility => l.accessibility = Some(s),
        Service::ListenEvent => l.input_monitoring = Some(s),
        // No ledger slot: the last-run signature is the only evidence.
        Service::PostEvent | Service::Microphone => {}
    }
    l
}

#[test]
fn probe_matrix_maps_every_combination() {
    let running = dev_id();
    let mut cases = 0;
    for service in Service::ALL {
        for raw in [RawAnswer::Granted, RawAnswer::Denied] {
            for ledger_kind in ["none", "same", "different"] {
                for last_run_same in [true, false] {
                    let ledger = match ledger_kind {
                        "none" => TrustLedger::default(),
                        "same" => ledger_with(service, running.clone()),
                        _ => ledger_with(service, apple_dev()),
                    };
                    let last_run = if last_run_same {
                        running.clone()
                    } else {
                        apple_dev()
                    };
                    let got = classify(
                        raw,
                        ledger_entry(&ledger, service),
                        Some(&last_run),
                        &running,
                    );
                    let has_slot = matches!(service, Service::Accessibility | Service::ListenEvent);
                    let want = match raw {
                        RawAnswer::Granted => ProbeState::Granted,
                        _ if !last_run_same => ProbeState::StaleIdentity,
                        _ if ledger_kind == "different" && has_slot => ProbeState::StaleIdentity,
                        _ => ProbeState::Denied,
                    };
                    assert_eq!(
                        got, want,
                        "{service:?} raw={raw:?} ledger={ledger_kind} last_run_same={last_run_same}"
                    );
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 4 * 2 * 3 * 2);
    // NotDetermined / Unknown never become stale: nobody refused anything.
    assert_eq!(
        classify(
            RawAnswer::NotDetermined,
            None,
            Some(&apple_dev()),
            &dev_id()
        ),
        ProbeState::NotDetermined
    );
    assert_eq!(
        classify(RawAnswer::Unknown, None, Some(&apple_dev()), &dev_id()),
        ProbeState::Unknown
    );
    // A version bump under the same Developer ID is NOT an identity change.
    let bumped = sig(&dev_id().authority, "2222", "0.9.2");
    assert_eq!(
        classify(RawAnswer::Denied, None, Some(&bumped), &dev_id()),
        ProbeState::Denied
    );
    assert_eq!(perm_probe::team_of(&dev_id().authority), Some("VHYV8C2JNU"));
}

#[test]
fn stale_identity_without_ledger_after_signature_change() {
    // A FUTURE identity change, caught by the last-run file: some launch of a
    // build that carries this PR ran under Apple Development, and the next one
    // runs under Developer ID, with no ledger entry. This is NOT Wilson's
    // current state: his ledger (permission-signature.json) already records
    // both grants under Developer ID 0.9.1, and last-run-signature.json does
    // not exist until a build with this PR has launched once, so the
    // Apple Development -> Developer ID change that already happened can
    // never be classified StaleIdentity. On his machine a denial reads Denied
    // and Repair is still offered (canRepair includes "denied").
    let raw = RawProbes {
        accessibility: false,
        listen_event_cg: false,
        listen_event_hid: GrantState::Denied,
        post_event: false,
        microphone: MicAuth::Authorized,
    };
    let report = build_probe_report(raw, &TrustLedger::default(), Some(&apple_dev()), &dev_id());
    let state = report.accessibility.state;
    println!("perm_repair: ledger=none last_run=changed probe=denied state={state:?}");
    assert_eq!(state, ProbeState::StaleIdentity);
    assert_eq!(report.listen_event.state, ProbeState::StaleIdentity);
    assert_eq!(report.post_event.state, ProbeState::StaleIdentity);
    // A granted service is never stale, whatever the identity history.
    assert_eq!(report.microphone.state, ProbeState::Granted);
    assert!(report.signature_changed);
}

#[test]
fn ledger_same_team_update_denied_is_denied_not_stale() {
    // Wilson's REAL ledger (permission-signature.json, read 2026-10-18): both
    // grants recorded under Developer ID 0.9.1, cdhash ca7456…. A later build
    // under the SAME Developer ID has a new cdhash and version, but TCC pins a
    // Developer ID grant to identifier + team (the designated requirement), so
    // a denial after the update is a real denial — never "an older Yap".
    let running = sig(
        &dev_id().authority,
        "3333333333333333333333333333333333333333",
        "0.9.2",
    );
    let ledger = TrustLedger {
        accessibility: Some(dev_id()),
        input_monitoring: Some(dev_id()),
    };
    for last_run in [None, Some(dev_id()), Some(running.clone())] {
        for service in [Service::Accessibility, Service::ListenEvent] {
            let state = classify(
                RawAnswer::Denied,
                ledger_entry(&ledger, service),
                last_run.as_ref(),
                &running,
            );
            println!("perm_repair: same-team update ledger=0.9.1 running=0.9.2 {service:?} probe=denied state={state:?}");
            assert_eq!(
                state,
                ProbeState::Denied,
                "{service:?} last_run={last_run:?}"
            );
        }
    }
    // A ledger under a different authority / team is still a stale identity.
    assert_eq!(
        classify(RawAnswer::Denied, Some(&apple_dev()), None, &running),
        ProbeState::StaleIdentity
    );
    // Ad-hoc signatures have no team: their designated requirement IS the
    // cdhash, so a cdhash change under "ad-hoc" is an identity change.
    let adhoc_old = sig(
        "ad-hoc",
        "4444444444444444444444444444444444444444",
        "0.9.1",
    );
    let adhoc_new = sig(
        "ad-hoc",
        "5555555555555555555555555555555555555555",
        "0.9.1",
    );
    assert_eq!(
        classify(RawAnswer::Denied, Some(&adhoc_old), None, &adhoc_new),
        ProbeState::StaleIdentity
    );
    assert_eq!(
        classify(RawAnswer::Denied, Some(&adhoc_new), None, &adhoc_new),
        ProbeState::Denied
    );
}

#[test]
fn stale_identity_from_ledger_different_signature() {
    let ledger = TrustLedger {
        accessibility: Some(apple_dev()),
        input_monitoring: Some(apple_dev()),
    };
    // Same signature last run (or no last run at all): the ledger alone decides.
    for last_run in [Some(dev_id()), None] {
        let raw = RawProbes {
            accessibility: false,
            listen_event_cg: false,
            listen_event_hid: GrantState::Denied,
            post_event: true,
            microphone: MicAuth::Denied,
        };
        let r = build_probe_report(raw, &ledger, last_run.as_ref(), &dev_id());
        assert_eq!(r.accessibility.state, ProbeState::StaleIdentity);
        assert_eq!(r.listen_event.state, ProbeState::StaleIdentity);
        // No ledger slot for the microphone and no identity change: Denied.
        assert_eq!(r.microphone.state, ProbeState::Denied);
        assert!(!r.signature_changed);
    }
    // The ledger saw it work under THIS signature: a plain denial (revoked).
    let same = TrustLedger {
        accessibility: Some(dev_id()),
        input_monitoring: None,
    };
    assert_eq!(
        classify(
            RawAnswer::Denied,
            same.accessibility.as_ref(),
            None,
            &dev_id()
        ),
        ProbeState::Denied
    );
}

#[test]
fn repair_argv_is_own_bundle_id_never_all() {
    for service in Service::ALL {
        let argv = repair_argv(service, BUNDLE);
        assert_eq!(
            argv,
            vec![
                "/usr/bin/tccutil".to_string(),
                "reset".to_string(),
                service.tcc_name().to_string(),
                BUNDLE.to_string()
            ]
        );
        assert!(!argv.iter().any(|a| a == "All"), "{argv:?}");
        assert_eq!(argv.len(), 4, "a bundle id is always passed");
    }
    assert_eq!(
        repair_argv(Service::ListenEvent, "com.apple.calculator").join(" "),
        "/usr/bin/tccutil reset ListenEvent com.apple.calculator"
    );
    for (s, name) in [
        (Service::Accessibility, "Accessibility"),
        (Service::ListenEvent, "ListenEvent"),
        (Service::PostEvent, "PostEvent"),
        (Service::Microphone, "Microphone"),
    ] {
        assert_eq!(s.tcc_name(), name);
        assert_eq!(Service::parse(s.key()), Some(s));
    }
    assert_eq!(Service::parse("All"), None);
}

#[test]
fn repair_pane_url_per_service() {
    let base = "x-apple.systempreferences:com.apple.preference.security?";
    assert_eq!(
        repair_pane_url(Service::Accessibility),
        format!("{base}Privacy_Accessibility")
    );
    assert_eq!(
        repair_pane_url(Service::ListenEvent),
        format!("{base}Privacy_ListenEvent")
    );
    assert_eq!(
        repair_pane_url(Service::Microphone),
        format!("{base}Privacy_Microphone")
    );
    // PostEvent clients are listed under Accessibility.
    assert_eq!(
        repair_pane_url(Service::PostEvent),
        format!("{base}Privacy_Accessibility")
    );
    // Every pane is in the open_privacy_pane allow-list (its first candidate).
    for s in Service::ALL {
        assert_eq!(
            permissions::open_privacy_pane_urls(s.pane_name())[0],
            repair_pane_url(s)
        );
    }
}

#[test]
fn reprobe_stops_on_granted() {
    let calls = Cell::new(0u32);
    let slept = Cell::new(Duration::ZERO);
    let mut phases = Vec::new();
    let end = reprobe(
        Service::Accessibility,
        || {
            calls.set(calls.get() + 1);
            if calls.get() >= 3 {
                ProbeState::Granted
            } else {
                ProbeState::StaleIdentity
            }
        },
        |d| slept.set(slept.get() + d),
        |p| phases.push(p),
    );
    assert_eq!(end, RepairPhase::Fixed);
    assert_eq!(calls.get(), 3, "stops the moment the probe flips");
    assert_eq!(slept.get(), Duration::from_secs(3));
    assert_eq!(phases, vec![RepairPhase::Waiting, RepairPhase::Fixed]);
    assert_eq!(
        phase_message(Service::Accessibility, RepairPhase::Waiting),
        "Waiting for you to switch Yap on…"
    );
    // Microphone and PostEvent end on Fixed too.
    assert_eq!(granted_phase(Service::Microphone), RepairPhase::Fixed);
    assert_eq!(granted_phase(Service::PostEvent), RepairPhase::Fixed);
}

#[test]
fn reprobe_times_out_at_120s() {
    let calls = Cell::new(0u32);
    let slept = Cell::new(Duration::ZERO);
    let mut phases = Vec::new();
    let end = reprobe(
        Service::Microphone,
        || {
            calls.set(calls.get() + 1);
            ProbeState::Denied
        },
        |d| {
            assert_eq!(d, Duration::from_secs(1), "re-probe every 1 s");
            slept.set(slept.get() + d);
        },
        |p| phases.push(p),
    );
    assert_eq!(end, RepairPhase::TimedOut);
    assert_eq!(REPROBE_INTERVAL, Duration::from_secs(1));
    assert_eq!(REPROBE_TIMEOUT, Duration::from_secs(120));
    assert_eq!(calls.get(), 120);
    assert_eq!(slept.get(), Duration::from_secs(120));
    assert_eq!(phases, vec![RepairPhase::Waiting, RepairPhase::TimedOut]);
}

#[test]
fn banner_and_onboarding_share_one_probe() {
    let raw = RawProbes {
        accessibility: false,
        listen_event_cg: false,
        listen_event_hid: GrantState::Denied,
        post_event: false,
        microphone: MicAuth::Authorized,
    };
    let probe = build_probe_report(raw, &TrustLedger::default(), Some(&apple_dev()), &dev_id());
    let report = permissions::report_from_probe(
        probe.clone(),
        false,
        GrantState::Unknown,
        true,
        "ok".into(),
    );
    // The banner reads `report.probe`; onboarding reads `report.grants` and
    // `report.accessibility` — all from ONE probe value.
    assert_eq!(report.probe, probe);
    assert!(!report.accessibility);
    let ax_row = report
        .grants
        .iter()
        .find(|g| g.key == permissions::GRANT_ACCESSIBILITY)
        .expect("accessibility row");
    assert_eq!(ax_row.status, GrantState::Denied);
    assert_eq!(report.probe.accessibility.state, ProbeState::StaleIdentity);
    let mic_row = report
        .grants
        .iter()
        .find(|g| g.key == permissions::GRANT_MICROPHONE)
        .expect("microphone row");
    assert_eq!(mic_row.status, GrantState::Authorized);
    assert_eq!(report.microphone_status, "authorized");
    // The wire form the frontend branches on.
    let json = serde_json::to_value(&report).expect("serialize");
    assert_eq!(json["probe"]["accessibility"]["state"], "stale_identity");
    assert_eq!(json["probe"]["listenEvent"]["service"], "listen_event");
}

#[test]
fn accessibility_repair_also_resets_post_event() {
    let plan = repair_plan(Service::Accessibility, BUNDLE);
    assert_eq!(plan.len(), 2);
    assert_eq!(plan[0], repair_argv(Service::Accessibility, BUNDLE));
    assert_eq!(plan[1], repair_argv(Service::PostEvent, BUNDLE));
    // Other services reset exactly themselves.
    assert_eq!(
        repair_plan(Service::ListenEvent, BUNDLE),
        vec![repair_argv(Service::ListenEvent, BUNDLE)]
    );
    assert_eq!(
        repair_plan(Service::Microphone, BUNDLE),
        vec![repair_argv(Service::Microphone, BUNDLE)]
    );
}

#[test]
fn listen_event_repair_says_relaunch() {
    assert!(needs_relaunch(Service::ListenEvent));
    assert!(!needs_relaunch(Service::Accessibility));
    assert_eq!(granted_phase(Service::ListenEvent), RepairPhase::Relaunch);
    assert_eq!(
        phase_message(Service::ListenEvent, RepairPhase::Relaunch),
        "Granted — quit and reopen Yap to finish"
    );
    let mut phases = Vec::new();
    let end = reprobe(
        Service::ListenEvent,
        || ProbeState::Granted,
        |_| {},
        |p| phases.push(p),
    );
    // Never "Fixed" for Input Monitoring: it needs the relaunch.
    assert_eq!(end, RepairPhase::Relaunch);
    assert!(!phases.contains(&RepairPhase::Fixed));
    let ev = serde_json::to_value(perm_probe::RepairEvent::new(
        Service::ListenEvent,
        RepairPhase::Relaunch,
    ))
    .expect("serialize");
    assert_eq!(ev["phase"], "relaunch");
    assert_eq!(ev["service"], "listen_event");
}

/// Fix round 2 (PR #228 blocking finding): Accessibility granted, IOHID
/// ListenEvent Denied, fn tap delivering. The health row says Input Monitoring
/// is fine, so the banner must not offer a ListenEvent Repair.
#[test]
fn working_tap_under_accessibility_is_not_a_listen_event_repair() {
    let raw = RawProbes {
        accessibility: true,
        listen_event_cg: false,
        listen_event_hid: GrantState::Denied,
        post_event: true,
        microphone: MicAuth::Authorized,
    };
    let probe = build_probe_report(raw, &TrustLedger::default(), None, &dev_id());
    assert_eq!(
        probe.listen_event.state,
        ProbeState::Denied,
        "raw probe alone says denied"
    );
    for tap_delivering in [true, false] {
        let report = permissions::report_from_probe(
            probe.clone(),
            tap_delivering,
            GrantState::Unknown,
            true,
            "ok".into(),
        );
        let im_row = report
            .grants
            .iter()
            .find(|g| g.key == permissions::GRANT_INPUT_MONITORING)
            .expect("input monitoring row");
        let expect_row = if tap_delivering {
            GrantState::Authorized
        } else {
            GrantState::Unknown
        };
        assert_eq!(im_row.status, expect_row);
        assert_ne!(report.probe.listen_event.state, ProbeState::Denied);
        assert_ne!(report.probe.listen_event.state, ProbeState::StaleIdentity);
        assert!(report.probe.listen_event.note.is_some());
        // The raw answers are kept.
        assert_eq!(report.probe.listen_event.raw, RawAnswer::Denied);
        assert_eq!(report.probe.listen_event_hid, GrantState::Denied);
        let json = serde_json::to_value(&report).expect("serialize");
        assert_ne!(json["probe"]["listenEvent"]["state"], "denied");
    }
}

/// The claim "the grant rows and report.probe can never disagree", checked over
/// every input: the banner offers a ListenEvent Repair exactly when the Input
/// Monitoring grant row is Denied, and reads Granted exactly when it is
/// Authorized.
#[test]
fn listen_event_banner_agrees_with_grant_row_for_every_input() {
    let hids = [
        GrantState::Authorized,
        GrantState::Denied,
        GrantState::Unknown,
    ];
    let lasts = [None, Some(apple_dev())];
    for ax in [true, false] {
        for cg in [true, false] {
            for hid in hids {
                for tap in [true, false] {
                    for last in &lasts {
                        let raw = RawProbes {
                            accessibility: ax,
                            listen_event_cg: cg,
                            listen_event_hid: hid,
                            post_event: ax,
                            microphone: MicAuth::Authorized,
                        };
                        let probe = build_probe_report(
                            raw,
                            &TrustLedger::default(),
                            last.as_ref(),
                            &dev_id(),
                        );
                        let report = permissions::report_from_probe(
                            probe,
                            tap,
                            GrantState::Unknown,
                            true,
                            "ok".into(),
                        );
                        let row = report
                            .grants
                            .iter()
                            .find(|g| g.key == permissions::GRANT_INPUT_MONITORING)
                            .expect("input monitoring row")
                            .status;
                        let state = report.probe.listen_event.state;
                        let repairable =
                            matches!(state, ProbeState::Denied | ProbeState::StaleIdentity);
                        let case = format!(
                            "ax={ax} cg={cg} hid={hid:?} tap={tap} last={}",
                            last.is_some()
                        );
                        assert_eq!(
                            repairable,
                            row == GrantState::Denied,
                            "{case}: {state:?} vs {row:?}"
                        );
                        assert_eq!(
                            state == ProbeState::Granted,
                            row == GrantState::Authorized,
                            "{case}: {state:?} vs {row:?}"
                        );
                        assert_eq!(report.input_monitoring, row, "{case}");
                    }
                }
            }
        }
    }
}

// ── fix-ax-probe-refresh ──────────────────────────────────────────────────

/// Wilson 2026-10-19 (Yap Test, combined 3095c4d): every Repair on the
/// "Accessibility is off" row logged ONLY `tccutil reset PostEvent`. That row
/// was the PostEvent row wearing the Accessibility label. Every row now resets
/// its OWN service first, and says which services it reset.
#[test]
fn every_row_resets_the_service_it_names_first() {
    for s in Service::ALL {
        let plan = repair_plan(s, BUNDLE);
        assert_eq!(plan[0], repair_argv(s, BUNDLE), "{s:?}: its own service first");
        assert_eq!(perm_probe::reset_services(s)[0], format!("kTCCService{}", s.tcc_name()));
    }
    assert_eq!(
        perm_probe::reset_services(Service::Accessibility),
        vec!["kTCCServiceAccessibility", "kTCCServicePostEvent"]
    );
    assert_eq!(perm_probe::reset_services(Service::PostEvent), vec!["kTCCServicePostEvent"]);
    assert_eq!(
        perm_probe::repair_summary(Service::Accessibility, "Yap Test"),
        "Reset kTCCServiceAccessibility and kTCCServicePostEvent for Yap Test."
    );
    // A PostEvent row can never again read as "Accessibility is off".
    assert_eq!(Service::Accessibility.label(), "Accessibility");
    assert_ne!(Service::PostEvent.label(), "Accessibility");
    // ...though its grant still lives in the Accessibility list.
    assert_eq!(Service::PostEvent.pane_name(), "Accessibility");
}

/// The 16:08:20Z state: kTCCServiceAccessibility allowed, CGPreflightPostEventAccess
/// false. Text goes in via AX, so nothing is broken and no Repair row shows.
#[test]
fn post_event_no_under_a_live_accessibility_grant_raises_no_row() {
    let report = |ax: bool, post: bool| {
        let raw = RawProbes {
            accessibility: ax,
            listen_event_cg: true,
            listen_event_hid: GrantState::Authorized,
            post_event: post,
            microphone: MicAuth::Authorized,
        };
        let probe = build_probe_report(raw, &TrustLedger::default(), None, &dev_id());
        permissions::report_from_probe(probe, true, GrantState::Unknown, true, "ok".into())
    };
    let granted = report(true, false);
    assert_eq!(granted.probe.accessibility.state, ProbeState::Granted);
    assert_eq!(granted.probe.post_event.state, ProbeState::Unknown);
    assert_eq!(granted.probe.post_event.raw, RawAnswer::Denied, "raw answer kept");
    assert_eq!(permissions::repair_row_bits(&granted.probe), 0);
    // With Accessibility off PostEvent is the bridge's grant and stays repairable.
    let off = report(false, false);
    assert_eq!(off.probe.post_event.state, ProbeState::Denied);
    assert_ne!(permissions::repair_row_bits(&off.probe), 0);
}

/// Finding 1, reproduced: flip the live AX probe and the watcher's snapshot
/// MUST change (that is what emits `permission_changed`, which re-reads the
/// banner), and the shown Repair rows go to none.
#[test]
fn flipping_the_probe_is_a_transition_and_clears_the_banner_rows() {
    let snap = |ax: bool| {
        let raw = RawProbes {
            accessibility: ax,
            listen_event_cg: true,
            listen_event_hid: GrantState::Authorized,
            post_event: false,
            microphone: MicAuth::Authorized,
        };
        let probe = build_probe_report(raw, &TrustLedger::default(), None, &dev_id());
        let r = permissions::report_from_probe(probe, true, GrantState::Unknown, true, "ok".into());
        permissions::GrantSnapshot::from_report(&r)
    };
    let mut gate = permissions::TransitionGate::new();
    assert_eq!(gate.observe(snap(false)), None, "baseline is silent");
    let before = snap(false);
    assert!(before.has_denial(), "a shown row keeps the 5 s re-read alive");
    assert_ne!(before.repair_rows, 0);
    let after = gate.observe(snap(true)).expect("AX flip must emit permission_changed");
    assert_eq!(after.repair_rows, 0, "no Repair row once Accessibility is on");
    assert_eq!(gate.observe(snap(true)), None, "and only once");
}

#[test]
fn phase_and_manual_copy_name_the_running_product() {
    use wilson_voice_lib::perm_truth::product_name_from_bundle_path;
    assert_eq!(product_name_from_bundle_path("/Applications/Yap Test.app"), "Yap Test");
    assert_eq!(product_name_from_bundle_path("/Applications/Yap.app/"), "Yap");
    assert_eq!(product_name_from_bundle_path("/Users/w/target/debug/wilson-voice"), "Yap");
    assert_eq!(
        perm_probe::phase_message_for(Service::Accessibility, RepairPhase::Waiting, "Yap Test"),
        "Waiting for you to switch Yap Test on…"
    );
    let timed = perm_probe::phase_message_for(Service::ListenEvent, RepairPhase::TimedOut, "Yap Test");
    assert!(timed.starts_with("Yap Test is still not allowed in Input Monitoring"), "{timed}");
    let steps = perm_probe::manual_steps_for(Service::PostEvent, "/Applications/Yap Test.app", "Yap Test");
    assert!(steps.contains("→ Accessibility, select Yap Test"), "{steps}");
}

#[test]
fn report_lines_name_the_test_product() {
    let raw = RawProbes {
        accessibility: false,
        listen_event_cg: false,
        listen_event_hid: GrantState::Denied,
        post_event: false,
        microphone: MicAuth::Denied,
    };
    let probe = build_probe_report(raw, &TrustLedger::default(), None, &dev_id());
    let mut r = permissions::report_from_probe(probe, false, GrantState::Unknown, true, "ok".into());
    permissions::name_product_in_report(&mut r, "Yap Test");
    assert_eq!(r.product_name, "Yap Test");
    let all: Vec<&str> = r.grants.iter().map(|g| g.detail.as_str()).collect();
    assert!(all.iter().any(|d| d.contains("Yap Test")), "{all:?}");
    for d in &all {
        assert!(!d.replace("Yap Test", "").contains("Yap"), "bare Yap left in: {d}");
    }
}
