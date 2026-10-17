//! yap-meetings-fix (C / NT3) — silence is not a denial.
//!
//! The old stop-path verdict called any tap that heard nothing for 3 s
//! "looks denied". 3 of Wilson's 4 meetings carried that sentence — one of them
//! an in-person meeting that never needed system audio. The verdict now reads
//! whether the DEFAULT OUTPUT ran, demands >= 60 s of it, and demands two
//! consecutive meetings before it writes anything; and it never stops Yap from
//! trying the tap again.

use std::time::Duration;

use wilson_voice_lib::meetings::{MeetingKind, SetupVerdict, SystemAudioSetup};
use wilson_voice_lib::os_version_gate::SystemAudioGate;
use wilson_voice_lib::syscapture::{
    fold_denial_streak, meeting_permission_verdict, track_b_plan, track_b_plan_for_kind,
    SystemAudioPermission, TapDelivery, TrackBPlan, LOOKS_DENIED_MESSAGE, OUTPUT_RAN_FOR_DENIAL,
};

fn zeros() -> TapDelivery {
    TapDelivery {
        callbacks: 10_000,
        frames: 5_000_000,
        ever_nonzero: false,
    }
}

fn row(v: SetupVerdict) -> SystemAudioSetup {
    SystemAudioSetup::from_row(Some(SystemAudioSetup::encode(v, "2026-10-16T00:00:00Z")))
}

#[test]
fn a_silent_output_is_unknown_and_writes_nothing() {
    // An hour of zeros while NOTHING played: the quiet Mac.
    let v = meeting_permission_verdict(&zeros(), Duration::ZERO);
    assert_eq!(v, SystemAudioPermission::Unknown);
    let step = fold_denial_streak(0, v);
    assert_eq!(step.write, None);
    assert!(!step.badge_meeting);
    // Even after a prior suspicious meeting, a silent one writes nothing.
    assert_eq!(fold_denial_streak(1, v).write, None);
}

#[test]
fn output_ran_but_under_sixty_seconds_is_unknown() {
    let v = meeting_permission_verdict(&zeros(), Duration::from_secs(59));
    assert_eq!(v, SystemAudioPermission::Unknown);
    assert_eq!(fold_denial_streak(1, v).write, None);
}

#[test]
fn one_meeting_of_output_with_a_silent_tap_is_not_a_denial() {
    let v = meeting_permission_verdict(&zeros(), OUTPUT_RAN_FOR_DENIAL);
    assert_eq!(v, SystemAudioPermission::LooksDenied, "this meeting is evidence");
    let step = fold_denial_streak(0, v);
    assert_eq!(step.write, None, "a muted call is one meeting of exactly this");
    assert!(!step.badge_meeting);
    assert_eq!(step.streak, 1);
}

#[test]
fn two_consecutive_meetings_of_output_with_a_silent_tap_look_denied() {
    let v = meeting_permission_verdict(&zeros(), Duration::from_secs(600));
    let first = fold_denial_streak(0, v);
    let second = fold_denial_streak(first.streak, v);
    assert_eq!(second.write, Some(SetupVerdict::LooksDenied));
    assert!(second.badge_meeting);
}

#[test]
fn a_quiet_meeting_between_two_suspicious_ones_breaks_the_streak() {
    let suspicious = meeting_permission_verdict(&zeros(), Duration::from_secs(600));
    let quiet = meeting_permission_verdict(&zeros(), Duration::ZERO);
    let a = fold_denial_streak(0, suspicious);
    let b = fold_denial_streak(a.streak, quiet);
    let c = fold_denial_streak(b.streak, suspicious);
    assert_eq!(c.write, None, "not consecutive");
}

#[test]
fn any_delivered_sample_is_granted_whatever_the_output_did() {
    let mut d = zeros();
    d.ever_nonzero = true;
    for output in [Duration::ZERO, Duration::from_secs(3600)] {
        let v = meeting_permission_verdict(&d, output);
        assert_eq!(v, SystemAudioPermission::Granted);
        let step = fold_denial_streak(5, v);
        assert_eq!(step.write, Some(SetupVerdict::Granted));
        assert_eq!(step.streak, 0);
    }
}

#[test]
fn a_looks_denied_row_still_attempts_the_tap_next_meeting() {
    let plan = track_b_plan(SystemAudioGate::Available, &row(SetupVerdict::LooksDenied));
    assert!(plan.attaches(), "the verdict is never sticky: {plan:?}");
    assert_eq!(
        plan,
        TrackBPlan::AttachWithBadge {
            badge: LOOKS_DENIED_MESSAGE
        },
        "LooksDenied changes the BADGE, never the attempt"
    );
    assert_eq!(plan.tracks(), 2);
    for kind in [MeetingKind::Virtual, MeetingKind::Unknown] {
        assert!(track_b_plan_for_kind(
            SystemAudioGate::Available,
            &row(SetupVerdict::LooksDenied),
            kind
        )
        .attaches());
    }
}

#[test]
fn an_in_person_meeting_never_attaches_and_says_nothing() {
    for v in [
        SetupVerdict::Granted,
        SetupVerdict::Ran,
        SetupVerdict::LooksDenied,
        SetupVerdict::NotRun,
    ] {
        let plan = track_b_plan_for_kind(SystemAudioGate::Available, &row(v), MeetingKind::InPerson);
        assert_eq!(plan, TrackBPlan::RoomOnly);
        assert!(!plan.attaches());
        assert_eq!(plan.badge(), None, "no badge, no error for a room recording");
        assert_eq!(plan.tracks(), 1);
    }
}

#[test]
fn the_stop_path_uses_the_output_aware_verdict() {
    let ctl = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/meeting_control.rs"
    ))
    .unwrap();
    assert!(ctl.contains("meeting_verdict()"), "stop reads the meeting verdict");
    assert!(ctl.contains("fold_denial_streak"), "stop folds the streak");
    assert!(
        !ctl.contains(".permission(ran_for)"),
        "the 3-second silence verdict is gone from the stop path"
    );
}
