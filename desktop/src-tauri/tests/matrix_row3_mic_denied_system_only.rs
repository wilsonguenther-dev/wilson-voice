//! Matrix row 3 — **mic permission denied ⇒ a system-audio-only meeting**.
//!
//! Required behaviour (plan §6): a system-audio-only meeting is valid for a
//! webinar or a lecture stream. Badge it.
//!
//! Row 3 is the only one of YV105's five that **nobody is bringing**. Rows 1, 2
//! and 14 are waiting on open PRs; this one is waiting on a change to the
//! capture session that no branch contains: `MeetingSession::start` holds a
//! single `CaptureStream`, and a `hold()` that returns `Err` ends the attempt —
//! which is right for 22-A, where the mic is the only source, and stops being
//! right the moment there are two.
//!
//! So the decision is written where nothing else owns it
//! (`meeting_matrix::meeting_start_plan`), tested here, and published as
//! `PolicyOnly` with `wiring_pr: None` — the same shape, and the same honesty,
//! as rows `5b`, 16 and `17b`. `matrix_coverage`'s absence tripwire covers this
//! row directly because it has no owning PR.

use wilson_voice_lib::meeting::{CaptureStream, ExternalStream, SessionConfig};
use wilson_voice_lib::meeting_matrix::{
    meeting_start_plan, Coverage, MeetingStartPlan, SourceAvailability, ROWS,
};

#[path = "support/callsite.rs"]
mod callsite;

use SourceAvailability::{Denied, Ready, Unsupported};

/// **The row itself.** The mic is refused; the call's own audio is not. The
/// meeting starts.
#[test]
fn a_denied_mic_with_a_working_tap_still_records_the_meeting() {
    let plan = meeting_start_plan(Denied, Ready);
    let MeetingStartPlan::Start {
        mic,
        system_audio,
        badge,
    } = plan
    else {
        panic!(
            "matrix row 3: a lecture stream recorded with the mic denied is a real meeting, and \
             this refused it: {plan:?}"
        );
    };
    assert!(!mic, "the mic was denied — it cannot be a track");
    assert!(system_audio, "the tap opened and is the whole recording");
    assert_eq!(plan.tracks(), 1);

    let badge = badge.expect("the plan's own word for row 3 is `badge it`");
    assert!(
        badge.contains("microphone"),
        "the badge has to name the track that is missing, or it is decoration: {badge}"
    );
    assert!(
        badge.contains("recording normally"),
        "…and it has to say what IS being recorded, so a user does not read a missing track as a \
         failed meeting: {badge}"
    );
}

/// The mirror image — row 1's fallback — decided by the same function, because
/// two functions is how the two rows end up disagreeing about the case where
/// both sources are gone.
#[test]
fn a_denied_tap_with_a_working_mic_is_22as_ordinary_meeting_badged() {
    let plan = meeting_start_plan(Ready, Denied);
    let MeetingStartPlan::Start {
        mic,
        system_audio,
        badge,
    } = plan
    else {
        panic!("a denied tap must never cost the user their microphone recording: {plan:?}");
    };
    assert!(mic);
    assert!(!system_audio);
    let badge = badge.expect("row 1 is badged too");
    assert!(badge.contains("System audio"), "{badge}");
    assert!(badge.contains("permission"), "{badge}");

    // …and the sentence for a Mac that simply cannot do it must NOT say
    // permission, because there is no permission to grant and no Settings pane
    // that would help. Row 12's refusal arrives here as `Unsupported`.
    let unsupported = meeting_start_plan(Ready, Unsupported);
    let MeetingStartPlan::Start {
        badge: Some(sentence),
        ..
    } = unsupported
    else {
        panic!("{unsupported:?}");
    };
    assert!(
        !sentence.contains("permission"),
        "a pre-14.4 Mac has no system-audio permission to be missing; telling the user it does \
         sends them to a Settings pane that cannot help: {sentence}"
    );
    assert!(sentence.contains("cannot capture"), "{sentence}");
}

/// Both sources present is the two-track meeting YV106 builds, and it is the
/// **only** case with no badge — a meeting that is quietly missing half of what
/// the user expected is the failure rows 1 and 3 are both about.
#[test]
fn only_a_complete_meeting_goes_unbadged() {
    let both = meeting_start_plan(Ready, Ready);
    assert_eq!(
        both,
        MeetingStartPlan::Start {
            mic: true,
            system_audio: true,
            badge: None,
        }
    );
    assert_eq!(both.tracks(), 2);

    for (mic, tap) in [
        (Ready, Denied),
        (Ready, Unsupported),
        (Denied, Ready),
        (Unsupported, Ready),
    ] {
        let plan = meeting_start_plan(mic, tap);
        let MeetingStartPlan::Start { badge, .. } = plan else {
            panic!("{mic:?}/{tap:?} refused: {plan:?}");
        };
        assert!(
            badge.is_some(),
            "a meeting missing a track and saying nothing about it is the bug: {mic:?}/{tap:?}"
        );
    }
}

/// A meeting starts if **either** source opened, and refuses only when neither
/// did — and the refusal names what to do, because "recording failed" is not a
/// reason.
#[test]
fn the_only_honest_refusal_is_when_nothing_could_be_opened() {
    for mic in [Ready, Denied, Unsupported] {
        for tap in [Ready, Denied, Unsupported] {
            let plan = meeting_start_plan(mic, tap);
            let should_start = mic.is_ready() || tap.is_ready();
            assert_eq!(
                plan.starts(),
                should_start,
                "{mic:?}/{tap:?} produced {plan:?}"
            );
            match plan {
                MeetingStartPlan::Start { .. } => assert!(plan.tracks() >= 1),
                MeetingStartPlan::Refuse { reason } => {
                    assert_eq!(plan.tracks(), 0);
                    assert!(
                        reason.contains("microphone") || reason.contains("input device"),
                        "the refusal must name the source the user can act on: {reason}"
                    );
                }
            }
        }
    }
}

/// The seam this row hands off to. `ExternalStream` — already shipped,
/// documented for exactly this purpose — is what a system-audio-only session
/// holds, because the tap owns its own IOProc and there is no cpal stream to
/// hold open. Row 3's fallback needs no new stream type invented for it, and
/// asserting that here is what stops the wiring item from growing one.
#[test]
fn the_system_audio_only_session_already_has_a_stream_type_to_hold() {
    let external = ExternalStream;
    assert!(
        external.hold().is_ok(),
        "a tap-only session holds nothing and must not fail trying"
    );
    external.release();

    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("yv105-row3");
    let config = SessionConfig {
        stream: std::sync::Arc::new(ExternalStream),
        tracks: meeting_start_plan(Denied, Ready).tracks(),
        ..SessionConfig::new(&dir, 48_000, 2)
    };
    assert_eq!(
        config.tracks, 1,
        "the plan's track count is what the session opens journals for"
    );
}

/// **The absence half**, and for this row it is the whole point: nothing calls
/// `meeting_start_plan`, so a denied mic today still ends the meeting.
#[test]
fn meeting_start_plan_reaches_no_call_site_so_row_3_is_not_wired() {
    let found = callsite::call_sites("meeting_start_plan", &["meeting_matrix.rs"]);
    assert!(
        found.is_empty(),
        "{}",
        callsite::promote_the_row("3", "meeting_start_plan", &found)
    );
}

#[test]
fn the_published_cell_admits_nobody_owns_this_one() {
    let row = ROWS.iter().find(|r| r.id == "3").expect("row 3");
    assert_eq!(
        row.coverage,
        Coverage::PolicyOnly {
            test: "matrix_row3_mic_denied_system_only.rs",
            wiring_pr: None,
            absent_call_site: "meeting_start_plan",
        }
    );
    let cell = row.coverage.cell();
    assert!(cell.contains("NOT WIRED"), "{cell}");
    assert!(cell.contains("nothing calls"), "{cell}");
}
