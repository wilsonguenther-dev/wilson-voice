//! YV93 — the Notetaker is English-only, and says so.
//!
//! Plan finding #38: Notetaker is English end to end (Parakeet EN,
//! `lang_detect:false`) while the app's existing "Language I speak" picker
//! implies a path that will not exist for meetings. Without a gate, a user who
//! set that picker to Spanish records a lecture and gets a page of plausible
//! English nonsense — the worst possible failure, because it looks like output.
//!
//! Shape copied from the app's other capability gates: one enum, one honest
//! message, no silent no-op. The gate is checked against the REAL bundled
//! catalog, so a future catalog edit that drops `languages` from the English
//! model — or adds a model whose language list does not include English —
//! shows up here.

use wilson_voice_lib::meeting_asr::{meeting_availability, MeetingUnavailable};
use wilson_voice_lib::models;

const ENGLISH_MODEL: &str = "handy-computer/parakeet-unified-en-0.6b-gguf";

#[test]
fn the_shipped_english_model_is_allowed() {
    assert!(meeting_availability(Some(ENGLISH_MODEL), None).is_ok());
    assert!(meeting_availability(Some(ENGLISH_MODEL), Some("en")).is_ok());
    // Regional variants of the picker's value are still English.
    for code in ["en-US", "en_GB", "EN", " en "] {
        assert!(
            meeting_availability(Some(ENGLISH_MODEL), Some(code)).is_ok(),
            "'{code}' should be English"
        );
    }
}

#[test]
fn a_non_english_spoken_language_disables_the_notetaker_with_a_message() {
    let blocked = meeting_availability(Some(ENGLISH_MODEL), Some("es"))
        .expect_err("Spanish is refused, not transcribed");
    assert_eq!(
        blocked,
        MeetingUnavailable::SpokenLanguageNotEnglish {
            language: "es".into()
        }
    );
    let message = blocked.message();
    assert!(message.contains("English-only"), "{message}");
    assert!(
        message.contains("Language I speak"),
        "the message has to name the setting to change: {message}"
    );
}

#[test]
fn a_model_that_cannot_do_english_disables_the_notetaker() {
    let blocked = MeetingUnavailable::ModelNotEnglish {
        model_id: "vendor/some-de-model".into(),
        model_name: "Some German Model".into(),
        languages: vec!["de".into()],
    };
    let message = blocked.message();
    assert!(message.contains("English-only"), "{message}");
    assert!(message.contains("Some German Model"), "{message}");
    assert!(
        message.contains("Settings"),
        "the message has to say where to fix it: {message}"
    );

    // …and the rule that produces it, applied to the whole shipped catalog:
    // every model that declares languages without English must be refused, and
    // every model that declares English must be allowed.
    for model in &models::catalog().models {
        let verdict = meeting_availability(Some(&model.id), None);
        let does_english = model.languages.iter().any(|l| l.to_lowercase() == "en");
        assert_eq!(
            verdict.is_ok(),
            does_english || model.languages.is_empty(),
            "catalog model '{}' (languages {:?}) got the wrong verdict",
            model.id,
            model.languages
        );
    }
}

#[test]
fn the_catalog_still_declares_what_the_gate_reads() {
    let english = models::catalog_model(ENGLISH_MODEL).expect("the shipped English model");
    assert!(
        english.languages.iter().any(|l| l == "en"),
        "the gate reads `languages`; the catalog stopped declaring it"
    );
}

#[test]
fn no_model_and_an_unknown_model_are_both_refused_honestly() {
    assert_eq!(
        meeting_availability(None, None),
        Err(MeetingUnavailable::NoModel)
    );
    assert_eq!(
        meeting_availability(Some("   "), None),
        Err(MeetingUnavailable::NoModel)
    );
    let unknown = meeting_availability(Some("someone/side-loaded.gguf"), None)
        .expect_err("an unknown model is not waved through");
    assert!(matches!(unknown, MeetingUnavailable::UnknownModel { .. }));
    assert!(
        unknown.message().contains("someone/side-loaded.gguf"),
        "{}",
        unknown.message()
    );
}

/// The user-visible copy, in one place, printed so a reviewer can read it
/// without running the app (the Meetings surface itself lands in YV95).
/// Asserted, not just printed: one sentence-per-line, no jargon leaking out of
/// the enum, and every message says what to do next.
#[test]
fn every_refusal_says_what_to_do_next() {
    let cases = [
        MeetingUnavailable::NoModel,
        MeetingUnavailable::UnknownModel {
            model_id: "someone/side-loaded.gguf".into(),
        },
        MeetingUnavailable::ModelNotEnglish {
            model_id: "vendor/some-de-model".into(),
            model_name: "Some German Model".into(),
            languages: vec!["de".into(), "fr".into()],
        },
        MeetingUnavailable::SpokenLanguageNotEnglish {
            language: "es".into(),
        },
    ];
    println!("Notetaker empty-state copy (YV93):");
    for case in &cases {
        let message = case.message();
        println!("  - {message}");
        assert!(!message.contains('_'), "enum jargon leaked: {message}");
        assert!(
            message.contains("Settings") || message.contains("Language I speak"),
            "no next step: {message}"
        );
        assert!(message.ends_with('.'), "not a sentence: {message}");
    }
}
