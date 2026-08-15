//! Matrix rows 12 and `12b` — **macOS older than 14.4**.
//!
//! Required behaviour (plan §6): "Notetaker surfaces are visible but disabled
//! with a plain sentence explaining the requirement. Mic-only meeting recording
//! can still be offered."
//!
//! That is two claims with two different truth values today, so it is two
//! cells, exactly as rows 5 and 17 already split:
//!
//!   * **12 — `Test`.** The gate runs. `meeting_asr::meeting_availability_for`
//!     is consulted by the shipping `notetaker_status` command for both capture
//!     modes, it refuses system audio below 14.4 with one sentence naming the
//!     requirement, and — the load-bearing half — it can never refuse mic-only
//!     recording, on any OS, which is what keeps 22-A's macOS 12 floor.
//!   * **`12b` — `PolicyOnly`.** No frontend file invokes that command, so the
//!     sentence reaches no surface: on a macOS 13 Mac the app today shows
//!     nothing rather than a disabled control with a reason. YV102 (#125)
//!     brings the Settings step that renders it.
//!
//! This file is the matrix's own row, not a second copy of
//! `meeting_availability_144_gate.rs`: that file is YV101's exhaustive version
//! table, and this one asserts the two things the *matrix row* publishes — the
//! floor is intact, and the sentence has no home yet.

use wilson_voice_lib::meeting_asr::{
    meeting_availability, meeting_availability_for, MeetingCapture, MeetingUnavailable,
};
use wilson_voice_lib::meeting_matrix::{Coverage, ROWS};
use wilson_voice_lib::os_version_gate::{self, OsVersion};

#[path = "support/callsite.rs"]
mod callsite;

/// The English model the shipped catalog carries, so the OS is the only axis
/// varying below.
const ENGLISH_MODEL: &str = "handy-computer/parakeet-unified-en-0.6b-gguf";

fn os(text: &str) -> OsVersion {
    OsVersion::parse(text).unwrap_or_else(|| panic!("'{text}' should parse"))
}

/// Row 12, positive half: below 14.4 the system-audio track is refused, and the
/// refusal is one sentence that names the requirement and this Mac.
#[test]
fn below_14_4_the_system_audio_track_is_refused_with_a_sentence_that_names_the_requirement() {
    for text in ["12.0", "13.0", "13.6", "14.0", "14.3"] {
        let verdict = meeting_availability_for(
            MeetingCapture::MicPlusSystemAudio,
            Some(ENGLISH_MODEL),
            Some("en"),
            os(text),
        );
        let Err(MeetingUnavailable::RequiresMacOS14_4 { found }) = verdict else {
            panic!("macOS {text} must not be offered a system-audio track: {verdict:?}");
        };
        assert_eq!(found, os(text));

        let sentence = MeetingUnavailable::RequiresMacOS14_4 { found }.message();
        assert!(
            sentence.contains("14.4"),
            "the sentence must name the requirement: {sentence}"
        );
        assert!(
            sentence.contains(text),
            "…and this Mac's own version, so it is an explanation rather than a policy: {sentence}"
        );
        assert!(
            sentence.contains("microphone"),
            "…and what still works, because this refusal is the one with no next step on the \
             machine: {sentence}"
        );
    }
}

/// **Row 12's load-bearing half.** The gate must never touch mic-only
/// recording, on any OS — including an OS version that could not be read at
/// all. A gate that leaked here would turn "system audio needs a newer Mac"
/// into "meetings do not work on your Mac" for every pre-14.4 user, with a
/// green build and no error anywhere.
#[test]
fn mic_only_recording_is_never_gated_by_this_row() {
    let versions = [
        "12.0", "13.0", "13.6", "14.0", "14.3", "14.4", "14.10", "15.0", "26.0",
    ];
    for text in versions {
        assert_eq!(
            meeting_availability_for(
                MeetingCapture::MicOnly,
                Some(ENGLISH_MODEL),
                Some("en"),
                os(text)
            ),
            Ok(()),
            "22-A mic-only recording must not regress on macOS {text}"
        );
    }
    assert_eq!(
        meeting_availability_for(
            MeetingCapture::MicOnly,
            Some(ENGLISH_MODEL),
            Some("en"),
            OsVersion::UNKNOWN
        ),
        Ok(()),
        "an unreadable OS version fails closed for the TAP, never for the microphone"
    );
    // The convenience door the rest of the app calls is the mic-only one, so it
    // cannot produce this refusal on the machine running the suite either —
    // whatever that machine's OS turns out to be.
    let here = meeting_availability(Some(ENGLISH_MODEL), Some("en"));
    assert!(
        !matches!(here, Err(MeetingUnavailable::RequiresMacOS14_4 { .. })),
        "`meeting_availability` is the mic-only door and must never carry the tap's gate: \
         {here:?} on macOS {}",
        OsVersion::current()
    );
}

/// At and above the floor the gate stops being the reason for anything — so a
/// refusal on 14.4+ is a model or language problem, and says so.
#[test]
fn at_and_above_14_4_the_gate_is_no_longer_the_answer() {
    for text in ["14.4", "14.5", "14.10", "15.0", "26.0"] {
        assert_eq!(
            meeting_availability_for(
                MeetingCapture::MicPlusSystemAudio,
                Some(ENGLISH_MODEL),
                Some("en"),
                os(text)
            ),
            Ok(()),
            "macOS {text} clears the tap floor"
        );
        // No model: still refused, but for the honest reason.
        assert_eq!(
            meeting_availability_for(
                MeetingCapture::MicPlusSystemAudio,
                None,
                Some("en"),
                os(text)
            ),
            Err(MeetingUnavailable::NoModel)
        );
    }
}

/// The requirement text is declared once, in `os_version_gate`, and the
/// Notetaker's sentence quotes it — so the published row and the running app
/// cannot drift the way two copies of the 3 h cap once could.
#[test]
fn the_requirement_is_declared_in_exactly_one_place() {
    let sentence = MeetingUnavailable::RequiresMacOS14_4 { found: os("13.6") }.message();
    assert!(
        sentence.contains(os_version_gate::SYSTEM_AUDIO_REQUIREMENT),
        "the availability sentence must quote `os_version_gate::SYSTEM_AUDIO_REQUIREMENT` rather \
         than carry its own copy of the number: {sentence}"
    );
}

/// **Row `12b`'s absence half.** The gate's verdict is computed on every
/// `notetaker_status` call and rendered by nothing: no frontend file invokes
/// that command, and `system_audio_setup` — YV102's Settings step, the surface
/// that would carry the sentence — does not exist in `src/`.
#[test]
fn the_sentence_reaches_no_surface_yet_so_row_12b_is_not_wired() {
    let found = callsite::call_sites("system_audio_setup", &[]);
    assert!(
        found.is_empty(),
        "{}",
        callsite::promote_the_row("12b", "system_audio_setup", &found)
    );

    // The frontend half, checked directly rather than inferred: if some `.ts`
    // or `.tsx` starts invoking `notetaker_status`, the sentence has a surface
    // and row `12b` is a `Test` row.
    let web = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("desktop/")
        .join("src");
    let mut callers = Vec::new();
    let mut stack = vec![web];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read desktop/src").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e == "ts" || e == "tsx" || e == "js")
                && std::fs::read_to_string(&path)
                    .map(|b| b.contains("notetaker_status"))
                    .unwrap_or(false)
            {
                callers.push(path.display().to_string());
            }
        }
    }
    assert!(
        callers.is_empty(),
        "TRIPWIRE — matrix row 12b: `notetaker_status` is now invoked from {callers:?}, so the \
         14.4 sentence finally has a surface. Promote row 12b to `Coverage::Test` and assert what \
         that surface renders."
    );
}

#[test]
fn the_published_cells_split_the_gate_from_its_missing_surface() {
    let gate = ROWS.iter().find(|r| r.id == "12").expect("row 12");
    assert_eq!(
        gate.coverage,
        Coverage::Test {
            test: "matrix_row12_macos_144_gate.rs",
            subject: "meeting_availability_for",
            subject_module: "meeting_asr.rs",
        }
    );

    let surface = ROWS.iter().find(|r| r.id == "12b").expect("row 12b");
    assert_eq!(
        surface.coverage,
        Coverage::PolicyOnly {
            test: "matrix_row12_macos_144_gate.rs",
            wiring_pr: Some("#125 (YV102)"),
            absent_call_site: "system_audio_setup",
        }
    );
    let cell = surface.coverage.cell();
    assert!(cell.contains("Policy only"), "{cell}");
    assert!(cell.contains("#125 (YV102)"), "{cell}");
}
