//! yap-call-1 — the call offer's policy table. Pure: no GUI, no Mac, no
//! audio. Every transition of `CallOfferMachine::step` is listed here, and the
//! last tests prove the ONE property the item exists for: nothing except
//! `UserChoice::Record` with the LIVE offer id ever yields `StartRecording`,
//! and nothing ever stops a recording but the user's Stop.

use wilson_voice_lib::call_offer::{
    content_protected_for, may_stamp_detection, never_rows, offer_start_decision, parse_policy,
    policy_for, policy_json, set_app_policy, AppPolicy, CallOfferMachine, DetectedVia, Effect,
    Input, OfferStart, Phase, PolicyMap, Sighting, UserChoice, ALREADY_RECORDING_MSG,
    CALL_ENDED_QUIET_MS, CALL_OFFER_POLICY_KEY, CALL_OFFER_WINDOW, OFFER_COUNTDOWN_MS,
};
use wilson_voice_lib::call_signals::{catalog, CallCandidate, Via};

fn zoom() -> Sighting {
    Sighting {
        key: "app:us.zoom.xos:4242".into(),
        bundle_id: "us.zoom.xos".into(),
        name: "Zoom".into(),
        copy: "Zoom call — record it?".into(),
        your_side_only: false,
        site_line: None,
        via: DetectedVia::MicInUse,
    }
}

fn seen(s: Option<Sighting>, now_ms: u64) -> Input {
    Input::Seen {
        sighting: s,
        now_ms,
    }
}

fn user(choice: UserChoice, id: &str) -> Input {
    Input::User {
        choice,
        offer_id: id.to_string(),
    }
}

fn tick(now_ms: u64, running: Option<bool>) -> Input {
    Input::Tick {
        now_ms,
        app_input_running: running,
    }
}

fn offering(m: &mut CallOfferMachine, policy: &PolicyMap) -> String {
    assert_eq!(
        m.step(seen(Some(zoom()), 1_000), policy),
        vec![Effect::ShowOffer]
    );
    m.live_offer_id().expect("live id").to_string()
}

fn starts(fx: &[Effect]) -> bool {
    fx.iter()
        .any(|e| matches!(e, Effect::StartRecording { .. }))
}

fn stops(fx: &[Effect]) -> bool {
    fx.iter().any(|e| matches!(e, Effect::StopRecording))
}

// ─── the table ──────────────────────────────────────────────────────────────

#[test]
fn offer_policy_offers_with_a_timer() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    let id = offering(&mut m, &policy);
    assert_eq!(m.phase, Phase::Offering);
    assert!(id.starts_with("app:us.zoom.xos:4242#"));
    let v = m.view(1_000);
    assert_eq!(v.countdown_ms_left, OFFER_COUNTDOWN_MS);
    assert_eq!(v.copy.as_deref(), Some("Zoom call — record it?"));
    assert_eq!(OFFER_COUNTDOWN_MS, 10_000);
    // The same call seen again is ONE offer (no second banner).
    assert_eq!(m.step(seen(Some(zoom()), 2_000), &policy), vec![]);
    assert_eq!(m.live_offer_id(), Some(id.as_str()));
}

#[test]
fn never_policy_offers_nothing() {
    let mut policy = PolicyMap::new();
    policy.insert("us.zoom.xos".into(), AppPolicy::Never);
    let mut m = CallOfferMachine::default();
    assert_eq!(m.step(seen(Some(zoom()), 1_000), &policy), vec![]);
    assert_eq!(m.phase, Phase::Idle);
    assert!(m.live_offer_id().is_none());
}

#[test]
fn expiry_shows_the_chip_and_records_nothing() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    offering(&mut m, &policy);
    assert_eq!(
        m.step(tick(1_000 + OFFER_COUNTDOWN_MS - 1, None), &policy),
        vec![]
    );
    let fx = m.step(tick(1_000 + OFFER_COUNTDOWN_MS, None), &policy);
    assert_eq!(fx, vec![Effect::ShowChip]);
    assert_eq!(m.phase, Phase::Chip);
    // The detector's own wake also expires it.
    let mut m2 = CallOfferMachine::default();
    offering(&mut m2, &policy);
    assert_eq!(
        m2.step(seen(Some(zoom()), 1_000 + OFFER_COUNTDOWN_MS), &policy),
        vec![Effect::ShowChip]
    );
    // Not now == expiry.
    let mut m3 = CallOfferMachine::default();
    let id = offering(&mut m3, &policy);
    assert_eq!(
        m3.step(user(UserChoice::NotNow, &id), &policy),
        vec![Effect::ShowChip]
    );
    // The chip stays for as long as the candidate lives.
    for t in 0..100u64 {
        assert!(m.step(tick(20_000 + t * 1_000, None), &policy).is_empty());
        assert!(m
            .step(seen(Some(zoom()), 20_000 + t * 1_000), &policy)
            .is_empty());
    }
    assert_eq!(m.phase, Phase::Chip);
}

#[test]
fn chip_click_records_with_the_live_id() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    let id = offering(&mut m, &policy);
    m.step(tick(1_000 + OFFER_COUNTDOWN_MS, None), &policy);
    let fx = m.step(user(UserChoice::Record, &id), &policy);
    assert_eq!(
        fx,
        vec![Effect::StartRecording {
            offer_id: id.clone(),
            bundle_id: "us.zoom.xos".into(),
            title: "Zoom call".into(),
            detected_via: DetectedVia::MicInUse,
        }]
    );
    assert_eq!(m.phase, Phase::Recording);
}

#[test]
fn stale_or_invented_id_is_rejected() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    // Nothing live at all.
    let fx = m.step(user(UserChoice::Record, "app:us.zoom.xos:4242#1"), &policy);
    assert!(matches!(fx.as_slice(), [Effect::Rejected { .. }]));
    let id = offering(&mut m, &policy);
    // Invented.
    let fx = m.step(user(UserChoice::Record, "app:evil:1#9"), &policy);
    assert!(matches!(fx.as_slice(), [Effect::Rejected { .. }]));
    // Stale: the call ended and a new one began — the old id no longer starts.
    m.step(seen(None, 5_000), &policy);
    m.step(seen(Some(zoom()), 6_000), &policy);
    assert_ne!(m.live_offer_id(), Some(id.as_str()));
    let fx = m.step(user(UserChoice::Record, &id), &policy);
    assert!(matches!(fx.as_slice(), [Effect::Rejected { .. }]));
    assert_eq!(m.phase, Phase::Offering);
}

#[test]
fn candidate_gone_withdraws_offer_and_chip() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    offering(&mut m, &policy);
    assert_eq!(m.step(seen(None, 2_000), &policy), vec![Effect::Withdraw]);
    assert_eq!(m.phase, Phase::Idle);
    offering(&mut m, &policy);
    m.step(tick(1_000 + OFFER_COUNTDOWN_MS, None), &policy);
    assert_eq!(m.phase, Phase::Chip);
    assert_eq!(m.step(seen(None, 30_000), &policy), vec![Effect::Withdraw]);
    assert!(m.live_offer_id().is_none());
    // Nothing to withdraw twice.
    assert_eq!(m.step(seen(None, 31_000), &policy), vec![]);
}

#[test]
fn a_second_call_by_the_same_app_is_a_new_candidate() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    let first = offering(&mut m, &policy);
    m.step(user(UserChoice::NotNow, &first), &policy);
    m.step(seen(None, 60_000), &policy); // running_input dropped
    assert_eq!(
        m.step(seen(Some(zoom()), 61_000), &policy),
        vec![Effect::ShowOffer]
    );
    let second = m.live_offer_id().unwrap().to_string();
    assert_ne!(first, second);
}

#[test]
fn never_answer_saves_and_withdraws() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    let id = offering(&mut m, &policy);
    let fx = m.step(user(UserChoice::Never, &id), &policy);
    assert_eq!(
        fx,
        vec![
            Effect::SaveNever {
                bundle_id: "us.zoom.xos".into()
            },
            Effect::Withdraw
        ]
    );
    assert!(!starts(&fx));
}

#[test]
fn ended_20s_prompts_and_never_stops() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    let id = offering(&mut m, &policy);
    m.step(user(UserChoice::Record, &id), &policy);
    assert_eq!(m.step(tick(10_000, Some(true)), &policy), vec![]);
    assert_eq!(m.step(tick(11_000, Some(false)), &policy), vec![]);
    // Unreadable is never evidence of an end; it resets the quiet clock.
    assert_eq!(m.step(tick(12_000, None), &policy), vec![]);
    assert_eq!(m.step(tick(13_000, Some(false)), &policy), vec![]);
    assert_eq!(
        m.step(
            tick(13_000 + CALL_ENDED_QUIET_MS - 1_000, Some(false)),
            &policy
        ),
        vec![]
    );
    let fx = m.step(tick(13_000 + CALL_ENDED_QUIET_MS, Some(false)), &policy);
    assert_eq!(fx, vec![Effect::ShowEndPrompt]);
    assert!(!stops(&fx));
    assert_eq!(m.phase, Phase::EndPrompt);
    // Silence forever after: still only the prompt, never a stop.
    for t in 0..120u64 {
        assert!(!stops(
            &m.step(tick(40_000 + t * 1_000, Some(false)), &policy)
        ));
    }
    // Keep going: back to recording, and no second prompt for this call.
    assert_eq!(
        m.step(user(UserChoice::KeepGoing, &id), &policy),
        vec![Effect::HideEndPrompt]
    );
    for t in 0..120u64 {
        assert!(m
            .step(tick(200_000 + t * 1_000, Some(false)), &policy)
            .is_empty());
    }
    assert_eq!(CALL_ENDED_QUIET_MS, 20_000);
}

#[test]
fn stop_on_the_end_prompt_is_the_only_stop() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    let id = offering(&mut m, &policy);
    m.step(user(UserChoice::Record, &id), &policy);
    m.step(tick(1_000, Some(false)), &policy);
    m.step(tick(1_000 + CALL_ENDED_QUIET_MS, Some(false)), &policy);
    assert_eq!(
        m.step(user(UserChoice::Stop, &id), &policy),
        vec![Effect::StopRecording]
    );
    // The recording stopped: the machine goes idle and this call is spent.
    m.step(Input::RecordingStopped, &policy);
    assert_eq!(m.phase, Phase::Idle);
    assert_eq!(m.step(seen(Some(zoom()), 90_000), &policy), vec![]);
}

#[test]
fn a_failed_start_returns_to_the_chip() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    let id = offering(&mut m, &policy);
    m.step(user(UserChoice::Record, &id), &policy);
    assert_eq!(m.step(Input::StartFailed, &policy), vec![Effect::ShowChip]);
    assert_eq!(m.phase, Phase::Chip);
    assert_eq!(m.live_offer_id(), Some(id.as_str()));
}

/// The property, exhaustively over a sequence of every non-Record input in
/// every phase: NO transition except `UserChoice::Record` yields
/// `StartRecording` (expiry, withdraw, offer, never, ended), and only
/// `UserChoice::Stop` ever yields `StopRecording`.
#[test]
fn only_user_choice_record_yields_start_recording() {
    let mut never = PolicyMap::new();
    never.insert("us.zoom.xos".into(), AppPolicy::Never);
    let empty = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    let mut now = 0u64;
    let mut records = 0;
    for round in 0..6u64 {
        let id = m.live_offer_id().unwrap_or("none").to_string();
        let inputs = vec![
            seen(Some(zoom()), now),
            tick(now + OFFER_COUNTDOWN_MS, None),
            tick(now + OFFER_COUNTDOWN_MS * 3, Some(false)),
            tick(now + OFFER_COUNTDOWN_MS * 6, Some(false)),
            user(UserChoice::NotNow, &id),
            user(UserChoice::KeepGoing, &id),
            user(UserChoice::Never, "stale#0"),
            Input::StartFailed,
            Input::MeetingStartedElsewhere,
            seen(None, now + 1),
            seen(Some(zoom()), now + 2),
        ];
        for (i, input) in inputs.into_iter().enumerate() {
            let policy = if (round + i as u64).is_multiple_of(5) {
                &never
            } else {
                &empty
            };
            let fx = m.step(input, policy);
            assert!(!starts(&fx), "round {round} input {i}: {fx:?}");
            assert!(!stops(&fx), "round {round} input {i}: {fx:?}");
        }
        // Then the user does press Record — and only that starts.
        if let Some(live) = m.live_offer_id().map(str::to_string) {
            if matches!(m.phase, Phase::Offering | Phase::Chip) {
                let fx = m.step(user(UserChoice::Record, &live), &empty);
                assert!(starts(&fx));
                records += 1;
                m.step(Input::RecordingStopped, &empty);
                m.step(seen(None, now + 3), &empty);
            }
        }
        now += 1_000_000;
    }
    assert!(records > 0, "the table must exercise a real Record");
}

// ─── fix-b1-1: a meeting started any other way ──────────────────────────────

#[test]
fn offering_then_meeting_started_elsewhere_withdraws() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    offering(&mut m, &policy);
    let fx = m.step(Input::MeetingStartedElsewhere, &policy);
    assert_eq!(fx, vec![Effect::Withdraw]);
    assert!(!starts(&fx));
    assert!(!stops(&fx));
    assert_eq!(m.phase, Phase::Idle);
    assert!(m.live_offer_id().is_none());
    // The countdown that was running can no longer turn into a chip.
    assert_eq!(
        m.step(tick(1_000 + OFFER_COUNTDOWN_MS * 2, None), &policy),
        vec![]
    );
    assert_eq!(m.view(1_000 + OFFER_COUNTDOWN_MS).phase, Phase::Idle);
}

#[test]
fn chip_then_meeting_started_elsewhere_withdraws() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    let id = offering(&mut m, &policy);
    assert_eq!(
        m.step(user(UserChoice::NotNow, &id), &policy),
        vec![Effect::ShowChip]
    );
    assert_eq!(m.phase, Phase::Chip);
    let fx = m.step(Input::MeetingStartedElsewhere, &policy);
    assert_eq!(fx, vec![Effect::Withdraw]);
    assert!(!starts(&fx));
    assert_eq!(m.phase, Phase::Idle);
    // The chip's id is dead: pressing it now cannot stamp the manual meeting.
    let late = m.step(user(UserChoice::Record, &id), &policy);
    assert!(!starts(&late), "{late:?}");
    assert!(matches!(late.as_slice(), [Effect::Rejected { .. }]));
}

#[test]
fn the_call_is_spent_after_the_manual_recording_stops() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    offering(&mut m, &policy);
    assert_eq!(
        m.step(Input::MeetingStartedElsewhere, &policy),
        vec![Effect::Withdraw]
    );
    // The manual recording stops; the same Zoom call is still Seen.
    assert_eq!(m.step(Input::RecordingStopped, &policy), vec![]);
    let fx = m.step(seen(Some(zoom()), 60_000), &policy);
    assert!(!fx.contains(&Effect::ShowOffer), "{fx:?}");
    assert_eq!(fx, vec![]);
    assert_eq!(m.phase, Phase::Idle);
    // Mic input drops (the call ended): the next call is a new offer.
    m.step(seen(None, 61_000), &policy);
    assert_eq!(
        m.step(seen(Some(zoom()), 62_000), &policy),
        vec![Effect::ShowOffer]
    );
}

#[test]
fn recording_or_end_prompt_ignore_meeting_started_elsewhere() {
    let policy = PolicyMap::new();
    // Idle: nothing to withdraw.
    let mut idle = CallOfferMachine::default();
    assert_eq!(idle.step(Input::MeetingStartedElsewhere, &policy), vec![]);
    assert_eq!(idle.phase, Phase::Idle);
    // Recording (the offer's own recording): untouched.
    let mut m = CallOfferMachine::default();
    let id = offering(&mut m, &policy);
    assert!(starts(&m.step(user(UserChoice::Record, &id), &policy)));
    assert_eq!(m.phase, Phase::Recording);
    assert_eq!(m.step(Input::MeetingStartedElsewhere, &policy), vec![]);
    assert_eq!(m.phase, Phase::Recording);
    assert_eq!(m.live_offer_id(), Some(id.as_str()));
    // EndPrompt: untouched too.
    m.step(tick(100_000, Some(false)), &policy);
    assert_eq!(
        m.step(tick(100_000 + CALL_ENDED_QUIET_MS, Some(false)), &policy),
        vec![Effect::ShowEndPrompt]
    );
    assert_eq!(m.step(Input::MeetingStartedElsewhere, &policy), vec![]);
    assert_eq!(m.phase, Phase::EndPrompt);
}

/// [Record] on the chip while a manual meeting already records: the machine
/// goes Recording → (StartFailed) Chip → (MeetingStartedElsewhere) withdrawn
/// and spent — the sequence `apply_call_offer_effects` drives.
#[test]
fn record_while_a_meeting_already_records_withdraws_and_spends() {
    let policy = PolicyMap::new();
    let mut m = CallOfferMachine::default();
    let id = offering(&mut m, &policy);
    assert!(starts(&m.step(user(UserChoice::Record, &id), &policy)));
    assert_eq!(m.step(Input::StartFailed, &policy), vec![Effect::ShowChip]);
    assert_eq!(
        m.step(Input::MeetingStartedElsewhere, &policy),
        vec![Effect::Withdraw]
    );
    assert_eq!(m.phase, Phase::Idle);
    assert_eq!(m.step(seen(Some(zoom()), 50_000), &policy), vec![]);
}

#[test]
fn already_recording_never_calls_set_meeting_detection() {
    assert_eq!(offer_start_decision(true), OfferStart::AlreadyRecording);
    assert_eq!(offer_start_decision(false), OfferStart::Begin);
    assert!(!may_stamp_detection(true, true));
    assert!(!may_stamp_detection(true, false));
    assert!(!may_stamp_detection(false, false), "lost the start race");
    assert!(may_stamp_detection(false, true));
    assert_eq!(ALREADY_RECORDING_MSG, "A meeting is already recording");

    // The StartRecording branch reads is_recording() BEFORE begin_meeting,
    // and set_meeting_detection only runs behind may_stamp_detection.
    let lib = include_str!("../src/lib.rs");
    let body = &lib[lib
        .find("fn apply_call_offer_effects(")
        .expect("apply_call_offer_effects")..];
    let body = &body[..body.find("\n}\n").expect("end of fn")];
    let arm = &body[body
        .find("Effect::StartRecording {")
        .expect("StartRecording arm")..];
    let already = arm
        .find("c.is_recording()")
        .expect("is_recording read first");
    let decide = arm
        .find("call_offer::offer_start_decision(already)")
        .expect("decision");
    let begin = arm.find("begin_meeting_from(").expect("begin_meeting_from");
    let gate = arm
        .find("call_offer::may_stamp_detection(")
        .expect("stamp gate");
    let stamp = arm.find("set_meeting_detection(").expect("stamp");
    assert!(
        already < decide && decide < begin,
        "checked before starting"
    );
    assert!(gate < stamp, "the stamp sits behind may_stamp_detection");
    assert_eq!(arm.matches("set_meeting_detection(").count(), 1);
    let refused = &arm[arm
        .find("None | Some(Ok((_, false)))")
        .expect("refused arm")..];
    let refused = &refused[..refused.find("Some(Ok((status, true)))").expect("next arm")];
    assert!(!refused.contains("set_meeting_detection"));
    assert!(refused.contains("call_offer::ALREADY_RECORDING_MSG"));
    assert!(refused.contains("call_offer::Input::MeetingStartedElsewhere"));
    assert!(arm.contains("StartOrigin::CallOffer"));
}

/// Every non-offer start withdraws the offer: begin_meeting (pill, Meetings
/// tab, tray Upcoming row, calendar offer), toggle_meeting (tray, ⌃⌘M), the
/// detector's meeting-recording branch, and the calendar raising its offer.
#[test]
fn every_other_start_drives_meeting_started_elsewhere() {
    let lib = include_str!("../src/lib.rs");
    let detect = include_str!("../src/meeting_detect.rs");
    let fn_body = |src: &'static str, sig: &str| -> &'static str {
        let b = &src[src.find(sig).unwrap_or_else(|| panic!("{sig}"))..];
        &b[..b.find("\n}\n").expect("end of fn")]
    };
    let begin = fn_body(lib, "fn begin_meeting_from(");
    let call = begin
        .find("call_offer_meeting_started_elsewhere(app, \"begin_meeting\")")
        .expect("begin_meeting withdraws");
    let guard = begin
        .find("if origin == StartOrigin::Elsewhere")
        .expect("the offer's own start is skipped");
    assert!(guard < call);
    assert!(fn_body(lib, "fn begin_meeting(").contains("StartOrigin::Elsewhere"));
    assert!(fn_body(lib, "fn toggle_meeting(")
        .contains("call_offer_meeting_started_elsewhere(app, \"toggle\")"));
    let helper = fn_body(lib, "pub(crate) fn call_offer_meeting_started_elsewhere(");
    assert!(helper.contains("call_offer::Input::MeetingStartedElsewhere"));

    let eval = fn_body(detect, "fn evaluate(app");
    let rec = &eval[eval.find("if meeting_recording").expect("recording branch")..];
    let rec = &rec[..rec.find("return;").expect("early return")];
    assert!(rec.contains("crate::call_offer_meeting_started_elsewhere(app, \"meeting_recording\")"));
    let cal = &eval[eval.find("if calendar_raised").expect("calendar branch")..];
    let cal = &cal[..cal.find("} else {").expect("else")];
    assert!(cal.contains("crate::call_offer_meeting_started_elsewhere(app, \"calendar_offer\")"));
}

// ─── per-app memory ─────────────────────────────────────────────────────────

#[test]
fn policy_has_exactly_two_states_and_defaults_to_offer() {
    let map = parse_policy(Some(
        r#"{"us.zoom.xos":"never","com.apple.FaceTime":"offer","x":"always_offer","y":"ask"}"#,
    ));
    assert_eq!(policy_for(&map, "us.zoom.xos"), AppPolicy::Never);
    assert_eq!(policy_for(&map, "US.ZOOM.XOS"), AppPolicy::Never);
    assert_eq!(policy_for(&map, "com.apple.facetime"), AppPolicy::Offer);
    // Anything but the two states is dropped (= offer).
    assert!(!map.contains_key("x") && !map.contains_key("y"));
    assert_eq!(policy_for(&map, "unknown.app"), AppPolicy::Offer);
    assert!(parse_policy(None).is_empty());
    assert!(parse_policy(Some("not json")).is_empty());
    assert_eq!(parse_policy(Some(&policy_json(&map))), map);
    assert_eq!(CALL_OFFER_POLICY_KEY, "call_offer_policy_v1");
}

#[test]
fn never_persists_in_settings_kv_and_reset_restores_offer() {
    let db = wilson_voice_lib::db::Database::open_in_memory().expect("db");
    set_app_policy(&db, "us.zoom.xos", Some(AppPolicy::Never)).expect("save");
    let raw = db.setting_get(CALL_OFFER_POLICY_KEY).expect("row");
    assert!(raw.contains("\"us.zoom.xos\":\"never\""), "{raw}");
    let map = wilson_voice_lib::call_offer::load_policy(&db);
    assert_eq!(policy_for(&map, "us.zoom.xos"), AppPolicy::Never);
    let rows = never_rows(&map, catalog());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "Zoom");
    let map = set_app_policy(&db, "us.zoom.xos", None).expect("reset");
    assert_eq!(policy_for(&map, "us.zoom.xos"), AppPolicy::Offer);
    assert!(never_rows(&wilson_voice_lib::call_offer::load_policy(&db), catalog()).is_empty());
}

#[test]
fn meeting_row_stores_detected_app_and_via() {
    let db = wilson_voice_lib::db::Database::open_in_memory().expect("db");
    assert_eq!(
        db.schema_version().unwrap(),
        wilson_voice_lib::meetings::SCHEMA_VERSION
    );
    // The columns exist (an UPDATE naming a missing column is an error).
    db.set_meeting_detection("no-such-meeting", "us.zoom.xos", "mic_in_use")
        .expect("detected_app / detected_via columns");
    assert_eq!(DetectedVia::MicInUse.as_str(), "mic_in_use");
    assert_eq!(DetectedVia::Title.as_str(), "title");
    assert_eq!(DetectedVia::Calendar.as_str(), "calendar");
    assert_eq!(DetectedVia::Manual.as_str(), "manual");
}

// ─── copy ───────────────────────────────────────────────────────────────────

fn candidate(bundle: &str, name: &str, via: Via) -> CallCandidate {
    CallCandidate {
        bundle_id: bundle.into(),
        name: name.into(),
        confidence: 90,
        via,
        pid: 7,
    }
}

#[test]
fn sighting_copy_comes_from_offer_copy() {
    let z = Sighting::from_candidate(&candidate("us.zoom.xos", "Zoom", Via::Bundle), catalog());
    assert_eq!(z.copy, "Zoom call — record it?");
    assert!(!z.your_side_only);
    let f = Sighting::from_candidate(
        &candidate("com.apple.FaceTime", "FaceTime", Via::Daemon),
        catalog(),
    );
    assert_eq!(f.copy, "FaceTime call — record your side?");
    assert!(f.your_side_only);
    let b = Sighting::from_candidate(
        &candidate("com.google.Chrome", "Google Chrome", Via::HelperParent),
        catalog(),
    );
    assert_eq!(b.copy, "Browser call — record it?");
    assert_eq!(b.site_line.as_deref(), Some("site unknown"));
    let meet = Sighting::from_candidate(
        &candidate("com.apple.Safari", "Google Meet", Via::WebkitTitle),
        catalog(),
    );
    assert_eq!(meet.copy, "Google Meet call — record it?");
    assert_eq!(meet.via, DetectedVia::Title);
    // FaceTime's recording title says "your side".
    let mut m = CallOfferMachine::default();
    let p = PolicyMap::new();
    m.step(seen(Some(f), 0), &p);
    let id = m.live_offer_id().unwrap().to_string();
    let fx = m.step(user(UserChoice::Record, &id), &p);
    assert!(
        matches!(&fx[..], [Effect::StartRecording { title, .. }] if title == "FaceTime call (your side)")
    );
}

// ─── content protection (no GUI launch) ─────────────────────────────────────

/// WF-16 changed the pill's half of this: the static rule still protects only
/// the offer window, and the pill / Scratchpad are hidden only while a meeting
/// records (`tests/capture_hide_in_meetings.rs`).
#[test]
fn offer_window_is_always_content_protected_and_the_pill_only_in_meetings() {
    assert_eq!(CALL_OFFER_WINDOW, "call-offer");
    assert!(content_protected_for("call-offer"));
    assert!(!content_protected_for("pill"));
    assert!(!content_protected_for("float"));
    assert!(!content_protected_for("main"));
    assert!(!content_protected_for("scratchpad"));

    let lib = include_str!("../src/lib.rs");
    let float = include_str!("../src/float_pill.rs");
    let offer = include_str!("../src/call_offer.rs");
    // The offer window builder passes its label through the one rule.
    let body = &lib[lib
        .find("fn ensure_call_offer_window(")
        .expect("offer builder")..];
    let body = &body[..body.find("\n}\n").expect("end of fn")];
    assert!(body.contains("call_offer::CALL_OFFER_WINDOW"));
    assert!(body.contains("call_offer::apply_content_protection(&win)"));
    // The pill window is built through the same rule (false for "float"
    // outside a meeting), and never switches protection on by itself.
    assert!(float.contains("crate::call_offer::apply_content_protection(&win)"));
    assert!(!float.contains("set_content_protected"));
    assert!(!float.contains("content_protected(true)"));
    // Nothing but the one helper calls set_content_protected.
    assert!(!lib.contains(".set_content_protected("));
    assert_eq!(offer.matches(".set_content_protected(").count(), 1);
    assert!(offer.contains("set_content_protected(protect)"));
}

#[test]
fn no_auto_record_or_auto_stop_anywhere() {
    for src in [
        include_str!("../src/call_offer.rs"),
        include_str!("../src/lib.rs"),
        include_str!("../src/meeting_detect.rs"),
    ] {
        for bad in ["auto_record", "autoRecord", "auto_stop", "autoStop"] {
            assert!(!src.contains(bad), "{bad}");
        }
    }
}
