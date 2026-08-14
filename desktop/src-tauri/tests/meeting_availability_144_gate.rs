//! YV101 — the 14.4 gate as a value of the Notetaker's existing availability
//! enum, not a second "feature unavailable" surface (plan finding OS-11(b)).
//!
//! YV93 already built the shape: one enum (`MeetingUnavailable`), one honest
//! sentence per variant, no silent no-op. The system-audio gate joins it as
//! `RequiresMacOS14_4` rather than inventing a parallel mechanism, so the
//! Meetings UI keeps one place to ask "can this run, and if not, what do I
//! tell the user".
//!
//! The property this file exists to protect is the one that is easy to break
//! and expensive to discover: **mic-only meeting recording keeps working on
//! macOS 12**. 22-A shipped to that floor. A gate that accidentally applies to
//! both capture modes would turn "system audio needs a newer Mac" into
//! "meetings do not work on your Mac", for every pre-14.4 user, with a green
//! build.

use wilson_voice_lib::meeting_asr::{
    meeting_availability, meeting_availability_for, MeetingCapture, MeetingUnavailable,
};
use wilson_voice_lib::os_version_gate::OsVersion;

/// The English model the shipped catalog carries (same constant YV93's gate
/// test uses), so the OS axis is the only thing varying below.
const ENGLISH_MODEL: &str = "handy-computer/parakeet-unified-en-0.6b-gguf";

/// Every OS version below the tap floor, including Yap's actual deployment
/// floor and the boundary's near miss.
const BELOW: &[&str] = &["12.0", "13.0", "13.6", "14.0", "14.3"];
/// Every OS version at or above it, including the ones a string compare would
/// sort wrongly.
const AT_OR_ABOVE: &[&str] = &["14.4", "14.5", "14.10", "15.0", "26.0"];

fn os(text: &str) -> OsVersion {
    OsVersion::parse(text).unwrap_or_else(|| panic!("'{text}' should parse"))
}

#[test]
fn mic_only_recording_is_available_on_every_version_including_the_macos_12_floor() {
    for text in BELOW.iter().chain(AT_OR_ABOVE) {
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
    // Even an unreadable OS version cannot disable mic-only recording — the
    // fail-closed default belongs to the tap, not to 22-A.
    assert_eq!(
        meeting_availability_for(
            MeetingCapture::MicOnly,
            Some(ENGLISH_MODEL),
            Some("en"),
            OsVersion::UNKNOWN
        ),
        Ok(())
    );
}

#[test]
fn system_audio_specifically_reports_requires_macos_14_4_below_the_floor() {
    for text in BELOW {
        let blocked = meeting_availability_for(
            MeetingCapture::MicPlusSystemAudio,
            Some(ENGLISH_MODEL),
            Some("en"),
            os(text),
        )
        .expect_err("system audio needs macOS 14.4");
        assert_eq!(
            blocked,
            MeetingUnavailable::RequiresMacOS14_4 { found: os(text) },
            "macOS {text}"
        );
    }
}

#[test]
fn system_audio_is_available_at_and_above_14_4() {
    for text in AT_OR_ABOVE {
        assert_eq!(
            meeting_availability_for(
                MeetingCapture::MicPlusSystemAudio,
                Some(ENGLISH_MODEL),
                Some("en"),
                os(text)
            ),
            Ok(()),
            "macOS {text} has the process-tap API"
        );
    }
}

/// The two verdicts on the SAME machine, which is the state the UI has to
/// render on a macOS 13 Mac: recording works, the system-audio control is
/// visible and disabled with a sentence.
#[test]
fn a_macos_13_mac_records_meetings_and_disables_only_system_audio() {
    let thirteen = os("13.6");
    assert_eq!(
        meeting_availability_for(
            MeetingCapture::MicOnly,
            Some(ENGLISH_MODEL),
            Some("en"),
            thirteen
        ),
        Ok(())
    );
    let blocked = meeting_availability_for(
        MeetingCapture::MicPlusSystemAudio,
        Some(ENGLISH_MODEL),
        Some("en"),
        thirteen,
    )
    .expect_err("no process tap on macOS 13");
    let message = blocked.message();
    println!("macOS 13 system-audio empty state: {message}");
    assert!(message.contains("macOS 14.4"), "{message}");
    assert!(
        message.contains("13.6"),
        "the user's own version: {message}"
    );
    assert!(
        message.to_lowercase().contains("microphone"),
        "must say mic-only recording still works: {message}"
    );
    assert!(!message.contains('_'), "enum jargon leaked: {message}");
    assert!(message.ends_with('.'), "not a sentence: {message}");
}

/// The public two-argument door is mic-only, on whatever OS this test runs on.
/// It is the function the Notetaker's own empty state has called since YV93 and
/// it must never start refusing for an OS reason.
#[test]
fn the_public_meeting_availability_is_the_mic_only_door() {
    assert_eq!(
        meeting_availability(Some(ENGLISH_MODEL), Some("en")),
        Ok(())
    );
    for spoken in [None, Some("en"), Some("en-US")] {
        assert!(!matches!(
            meeting_availability(Some(ENGLISH_MODEL), spoken),
            Err(MeetingUnavailable::RequiresMacOS14_4 { .. })
        ));
    }
}

/// Check order, stated as a test because it is a decision, not an accident: on
/// a machine that can never run a tap, the system-audio control says so — the
/// model and language refusals are shared with mic-only recording and the
/// Notetaker's own empty state already carries them.
#[test]
fn the_os_gate_is_the_reason_the_disabled_control_shows_on_an_old_mac() {
    let thirteen = os("13.0");
    // No model at all, macOS 13: mic-only blames the model (fixable in
    // Settings), system audio blames the OS (not fixable at all).
    assert_eq!(
        meeting_availability_for(MeetingCapture::MicOnly, None, None, thirteen),
        Err(MeetingUnavailable::NoModel)
    );
    assert_eq!(
        meeting_availability_for(MeetingCapture::MicPlusSystemAudio, None, None, thirteen),
        Err(MeetingUnavailable::RequiresMacOS14_4 { found: thirteen })
    );
}

/// The English-only gate (YV93) still applies to BOTH capture modes above 14.4.
/// The new variant must extend that gate, not shadow it.
#[test]
fn the_english_only_gate_still_applies_to_both_modes() {
    let modern = os("15.0");
    for capture in [MeetingCapture::MicOnly, MeetingCapture::MicPlusSystemAudio] {
        assert_eq!(
            meeting_availability_for(capture, Some(ENGLISH_MODEL), Some("es"), modern),
            Err(MeetingUnavailable::SpokenLanguageNotEnglish {
                language: "es".into()
            }),
            "{capture:?}"
        );
        assert_eq!(
            meeting_availability_for(capture, None, None, modern),
            Err(MeetingUnavailable::NoModel),
            "{capture:?}"
        );
        assert!(
            matches!(
                meeting_availability_for(capture, Some("someone/side-loaded.gguf"), None, modern),
                Err(MeetingUnavailable::UnknownModel { .. })
            ),
            "{capture:?}"
        );
    }
}

/// Every variant still has a message, including the new one — the enum is only
/// useful because nothing in it is allowed to be silent.
#[test]
fn every_variant_including_the_new_one_has_a_sentence() {
    let cases = [
        MeetingUnavailable::NoModel,
        MeetingUnavailable::UnknownModel {
            model_id: "someone/side-loaded.gguf".into(),
        },
        MeetingUnavailable::ModelNotEnglish {
            model_id: "vendor/some-de-model".into(),
            model_name: "Some German Model".into(),
            languages: vec!["de".into()],
        },
        MeetingUnavailable::SpokenLanguageNotEnglish {
            language: "es".into(),
        },
        MeetingUnavailable::RequiresMacOS14_4 { found: os("13.0") },
        MeetingUnavailable::RequiresMacOS14_4 {
            found: OsVersion::UNKNOWN,
        },
    ];
    for case in &cases {
        let message = case.message();
        println!("  - {message}");
        assert!(!message.trim().is_empty(), "{case:?} has no message");
        assert!(message.ends_with('.'), "not a sentence: {message}");
        assert!(!message.contains('_'), "enum jargon leaked: {message}");
    }
    // An unreadable OS version must not print "macOS 0.0" at the user.
    let unknown = MeetingUnavailable::RequiresMacOS14_4 {
        found: OsVersion::UNKNOWN,
    }
    .message();
    assert!(!unknown.contains("0.0"), "{unknown}");
}
