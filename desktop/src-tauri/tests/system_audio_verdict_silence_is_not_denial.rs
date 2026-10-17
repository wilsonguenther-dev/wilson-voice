//! yap-meetings-fix (C / NT3) — silence is not a denial.
//!
//! The old stop-path verdict called any tap that heard nothing for 3 s
//! "looks denied". 3 of Wilson's 4 meetings carried that sentence — one of them
//! an in-person meeting that never needed system audio. The verdict now reads
//! whether ANOTHER PROCESS ran output (review-opus B1: never the device, which
//! Yap's own tap aggregate keeps running), demands >= 60 s of it, and demands two
//! consecutive meetings before it writes anything; and it never stops Yap from
//! trying the tap again.

use std::time::Duration;

use wilson_voice_lib::meeting::RtCapture;
use wilson_voice_lib::meetings::{MeetingKind, SetupVerdict, SystemAudioSetup};
use wilson_voice_lib::os_version_gate::SystemAudioGate;
use wilson_voice_lib::syscapture::{
    fold_denial_streak, meeting_permission_verdict, other_process_output_running,
    system_audio_unconfirmed_note, track_b_plan, track_b_plan_for_kind, MeetingTap,
    OutputActivitySource, OutputEvidence, OwnProcesses, ProcessOutputActivity,
    SystemAudioPermission, TapDelivery, TapFormat, TapPumpThread, TrackBPlan, LOOKS_DENIED_MESSAGE,
    OUTPUT_RAN_FOR_DENIAL, SYSTEM_AUDIO_UNCONFIRMED_NO_PROBE,
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
    assert_eq!(
        v,
        SystemAudioPermission::LooksDenied,
        "this meeting is evidence"
    );
    let step = fold_denial_streak(0, v);
    assert_eq!(
        step.write, None,
        "a muted call is one meeting of exactly this"
    );
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
        let plan =
            track_b_plan_for_kind(SystemAudioGate::Available, &row(v), MeetingKind::InPerson);
        assert_eq!(plan, TrackBPlan::RoomOnly);
        assert!(!plan.attaches());
        assert_eq!(
            plan.badge(),
            None,
            "no badge, no error for a room recording"
        );
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
    assert!(
        ctl.contains("meeting_verdict()"),
        "stop reads the meeting verdict"
    );
    assert!(ctl.contains("fold_denial_streak"), "stop folds the streak");
    assert!(
        !ctl.contains(".permission(ran_for)"),
        "the 3-second silence verdict is gone from the stop path"
    );
}

// ── review-opus B1 · evidence from OTHER processes' output only ────────────

/// An injected per-process activity source: a fixed answer every sample.
struct FixedSource(Option<Vec<ProcessOutputActivity>>);

impl OutputActivitySource for FixedSource {
    fn sample(&self) -> Option<Vec<ProcessOutputActivity>> {
        self.0.clone()
    }
}

const YAP: i32 = 4_100;
const OTHER_APP: i32 = 7_777;

fn proc(pid: i32, parent: i32, running: bool) -> ProcessOutputActivity {
    ProcessOutputActivity {
        pid,
        parent_pid: Some(parent),
        running_output: running,
    }
}

/// Fold `secs` one-second samples from `source` into a fresh meeting's evidence.
fn meeting_evidence(source: &dyn OutputActivitySource, secs: u32) -> OutputEvidence {
    let own = OwnProcesses::new(YAP);
    let mut evidence = OutputEvidence::default();
    for _ in 0..secs {
        let sample = source.sample();
        evidence.observe(sample.as_deref(), &own, Duration::from_secs(1));
    }
    evidence
}

/// Two consecutive meetings over the same evidence, as the stop path folds them.
fn two_meetings(evidence: &OutputEvidence) -> (SystemAudioPermission, Option<SetupVerdict>, bool) {
    let v = meeting_permission_verdict(&zeros(), evidence.other_output_ran);
    let first = fold_denial_streak(0, v);
    let second = fold_denial_streak(first.streak, v);
    (
        v,
        first.write.or(second.write),
        first.badge_meeting || second.badge_meeting,
    )
}

#[test]
fn yaps_own_device_activity_never_counts_as_evidence() {
    // The reviewer's probe: Yap's own aggregate IO is the ONLY output running,
    // for a whole hour, with a silent tap. Under the device-level probe this
    // was 3600 s of "output ran"; now it is zero.
    let source = FixedSource(Some(vec![proc(YAP, 1, true)]));
    let evidence = meeting_evidence(&source, 3_600);
    assert_eq!(evidence.other_output_ran, Duration::ZERO);
    assert_eq!(evidence.samples, 3_600, "the samples were readable");
    let (v, write, badge) = two_meetings(&evidence);
    assert_eq!(v, SystemAudioPermission::Unknown);
    assert_eq!(write, None, "Yap's own IO must never persist a denial");
    assert!(!badge);
}

#[test]
fn yaps_sidecars_output_never_counts_as_evidence() {
    // A sidecar Yap spawned (parent = Yap) playing output is still Yap.
    let source = FixedSource(Some(vec![proc(YAP, 1, true), proc(9_001, YAP, true)]));
    let evidence = meeting_evidence(&source, 600);
    assert_eq!(evidence.other_output_ran, Duration::ZERO);
    assert_eq!(two_meetings(&evidence).1, None);
}

#[test]
fn another_apps_output_with_a_silent_tap_is_evidence() {
    let source = FixedSource(Some(vec![proc(YAP, 1, true), proc(OTHER_APP, 1, true)]));
    let evidence = meeting_evidence(&source, 120);
    assert_eq!(evidence.other_output_ran, Duration::from_secs(120));
    let (v, write, badge) = two_meetings(&evidence);
    assert_eq!(v, SystemAudioPermission::LooksDenied);
    assert_eq!(
        write,
        Some(SetupVerdict::LooksDenied),
        "two meetings of other-app output with a silent tap is the evidence"
    );
    assert!(badge);
}

#[test]
fn an_idle_other_app_is_not_evidence() {
    // Zoom connected to CoreAudio but not playing: running_output = 0.
    let own = OwnProcesses::new(YAP);
    let sample = [proc(YAP, 1, true), proc(OTHER_APP, 1, false)];
    assert_eq!(
        other_process_output_running(Some(&sample), &own),
        Some(false)
    );
}

#[test]
fn property_unavailable_is_unknown_and_nothing_is_persisted() {
    // Below macOS 14.4, or the process list unreadable: no evidence at all.
    let source = FixedSource(None);
    let evidence = meeting_evidence(&source, 3_600);
    assert_eq!(evidence.other_output_ran, Duration::ZERO);
    assert_eq!(evidence.samples, 0);
    assert_eq!(evidence.unavailable, 3_600);
    assert_eq!(
        other_process_output_running(None, &OwnProcesses::new(YAP)),
        None,
        "unreadable is unknown, never \"nothing played\""
    );
    let (v, write, badge) = two_meetings(&evidence);
    assert_eq!(v, SystemAudioPermission::Unknown);
    assert_eq!(write, None, "nothing is persisted from no evidence");
    assert!(!badge);
    let note = system_audio_unconfirmed_note(&evidence);
    assert_eq!(note, SYSTEM_AUDIO_UNCONFIRMED_NO_PROBE);
    assert!(note.starts_with("Couldn't confirm system audio"));
    assert!(!note.contains("not granted"));
}

#[test]
fn the_meeting_tap_folds_process_activity_and_excludes_yap() {
    let tap = MeetingTap::new(
        std::sync::Arc::new(RtCapture::new(48_000, 2)),
        TapFormat {
            sample_rate: 48_000,
            channels: 2,
        },
        || {},
    );
    let own = OwnProcesses::new(YAP);
    for _ in 0..120 {
        tap.observe_process_activity(Some(&[proc(YAP, 1, true)]), &own, Duration::from_secs(1));
    }
    assert_eq!(tap.output_was_running(), Duration::ZERO);
    assert_eq!(tap.meeting_verdict(), SystemAudioPermission::Unknown);
    for _ in 0..60 {
        tap.observe_process_activity(
            Some(&[proc(YAP, 1, true), proc(OTHER_APP, 1, true)]),
            &own,
            Duration::from_secs(1),
        );
    }
    assert_eq!(tap.output_was_running(), Duration::from_secs(60));
    assert_eq!(tap.meeting_verdict(), SystemAudioPermission::LooksDenied);
    tap.stop();
}

#[test]
fn the_pump_thread_reads_the_injected_source_and_never_counts_yap() {
    let tap = MeetingTap::new(
        std::sync::Arc::new(RtCapture::new(48_000, 2)),
        TapFormat {
            sample_rate: 48_000,
            channels: 2,
        },
        || {},
    );
    let source = std::sync::Arc::new(FixedSource(Some(vec![proc(YAP, 1, true)])));
    let mut pump = TapPumpThread::spawn_with_source(
        std::sync::Arc::clone(&tap),
        source,
        OwnProcesses::new(YAP),
    );
    std::thread::sleep(Duration::from_millis(2_300));
    pump.finish();
    let evidence = tap.output_evidence();
    assert!(
        evidence.samples >= 1,
        "the pump sampled the injected source"
    );
    assert_eq!(evidence.other_output_ran, Duration::ZERO);
    assert_eq!(tap.meeting_verdict(), SystemAudioPermission::Unknown);
}

#[test]
fn the_denial_sentence_states_evidence_not_fact() {
    assert!(LOOKS_DENIED_MESSAGE.contains("while other apps were playing sound"));
    assert!(!LOOKS_DENIED_MESSAGE.contains("is not granted"));
    assert!(LOOKS_DENIED_MESSAGE.contains("System Settings"));
}

#[test]
fn the_pump_no_longer_reads_the_device_running_state() {
    let src =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/syscapture.rs")).unwrap();
    assert!(
        !src.contains("kAudioDevicePropertyDeviceIsRunningSomewhere,"),
        "the device-level probe Yap's own aggregate satisfies must not be imported"
    );
    assert!(src.contains("kAudioProcessPropertyIsRunningOutput"));
}

/// Live probe against this Mac's CoreAudio (not hermetic, so ignored by
/// default): on macOS 14.4+ the process list must be readable, and every
/// entry carries a pid. Run with `--ignored` on a real Mac.
#[test]
#[ignore]
fn live_core_audio_process_list_is_readable() {
    use wilson_voice_lib::syscapture::CoreAudioProcessActivity;
    let sample = CoreAudioProcessActivity.sample();
    if SystemAudioGate::Available == wilson_voice_lib::os_version_gate::system_audio_gate_now() {
        let sample = sample.expect("macOS 14.4+: the process object list is readable");
        eprintln!(
            "live: {} CoreAudio client processes: {sample:?}",
            sample.len()
        );
        assert!(sample.iter().all(|p| p.pid > 0));
    } else {
        assert!(sample.is_none(), "below 14.4 there is no evidence at all");
    }
}
