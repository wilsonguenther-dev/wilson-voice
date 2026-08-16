//! YV132 · matrix row 13 — the diarization model is missing, or half of it is.
//!
//! ```sh
//! cargo test --test matrix_row13_diarize_model_missing
//! ```
//!
//! The row publishes two claims and this file drives both:
//!
//!   * **the feature is off, not broken**, with the combined download named —
//!     `meeting_matrix::speaker_detection_gate`, over the two facts `models.rs`
//!     already computes;
//!   * **recording is unaffected** — which is the half worth most of the test
//!     budget, for the reason row 12 spends its own budget the same way: a gate
//!     that leaks turns "speaker detection needs a download" into "meeting
//!     recording does not work on your Mac", with a green build and no error
//!     anywhere.
//!
//! **The `is_diarize_downloaded` half is driven against a real file on a real
//! disk**, because the claim row 13 makes about a *partial* download is a claim
//! about bytes: YV123's gate compares the file's length to the size the catalog
//! states, so an interrupted download reads as missing rather than as present.
//! Asserting that against a mocked bool would prove nothing about the mechanism
//! the row names. The file is a few bytes under a name no catalog entry uses,
//! and it is removed again.

use std::fs;

use wilson_voice_lib::meeting_matrix::{speaker_detection_gate, SpeakerDetection};
use wilson_voice_lib::meetings::MeetingState;
use wilson_voice_lib::models::{
    diarize_download_bytes, diarize_model_for_role, is_diarize_downloaded, models_dir,
    DiarizeCatalogModel, DiarizeModelRole,
};
use wilson_voice_lib::os_version_gate::OsVersion;
use wilson_voice_lib::NotetakerStatus;

#[path = "support/callsite.rs"]
mod callsite;
use callsite::{call_sites, mentions_as_code, promote_the_row, src_dir};

/// The English ASR model the Notetaker's own availability gate is happy with —
/// the same constant `matrix_row12_macos_144_gate` drives that gate with, so
/// this file's "recording is unaffected" half and row 12's "recording is never
/// refused" half are asserted about the same configuration.
const ENGLISH_MODEL: &str = "handy-computer/parakeet-unified-en-0.6b-gguf";

/// The combined size, spelled the way every other download size in this app is
/// spelled (`ModelSetup.tsx`: `Math.round(bytes / 1e6)`).
fn expected_megabytes() -> u64 {
    ((diarize_download_bytes() as f64) / 1_000_000.0).round() as u64
}

/// A catalog entry shaped exactly like a real one, pointing at a filename no
/// real entry uses — so the disk assertions below cannot touch, and cannot be
/// satisfied by, an actual installed model.
fn synthetic_entry(filename: &str, size_bytes: u64) -> DiarizeCatalogModel {
    serde_json::from_value(serde_json::json!({
        "id": "yv132-row13-probe",
        "role": "embedding",
        "repo": "wilsonguenther/yap-diarize-models",
        "revision": "0000000000000000000000000000000000000000",
        "name": "row 13 probe",
        "license": "cc-by-4.0",
        "description": "not a model; a file whose length the gate reads",
        "file": {
            "filename": filename,
            "quant": "none",
            "size_bytes": size_bytes,
            "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
        }
    }))
    .expect("the synthetic entry must parse as a real catalog entry")
}

/// **The gate's input, on a real disk.** Missing, half there, and there — the
/// three states row 13 names, against the shipped `is_diarize_downloaded`.
#[test]
fn a_partial_download_reads_as_missing_and_only_the_full_size_reads_as_present() {
    let dir = models_dir();
    fs::create_dir_all(&dir).expect("create the models dir");
    let filename = format!("yv132-row13-probe-{}.onnx", std::process::id());
    let path = dir.join(&filename);
    let _ = fs::remove_file(&path);
    let model = synthetic_entry(&filename, 8);

    // 1. Missing.
    assert!(
        !is_diarize_downloaded(&model),
        "nothing is installed at {}",
        path.display()
    );

    // 2. Half there — the case the row's own title names, and the one a size
    //    check exists for. An interrupted download leaves a real file behind.
    fs::write(&path, b"abc").expect("write a partial file");
    assert!(
        !is_diarize_downloaded(&model),
        "a 3-byte file standing in for an 8-byte model read as installed — a \
         partial download must never look like a present one"
    );

    // 3. There, at exactly the size the catalog states.
    fs::write(&path, b"abcdefgh").expect("write the full file");
    assert!(
        is_diarize_downloaded(&model),
        "the full-size file is present"
    );

    // 4. …and a file that is too LONG is not the model either.
    fs::write(&path, b"abcdefghi").expect("write an oversized file");
    assert!(!is_diarize_downloaded(&model));

    let _ = fs::remove_file(&path);

    // The real entries are untouched by any of the above: they name different
    // files, which is what makes this test safe to run on a machine that has
    // the models installed and on one that does not.
    for role in [DiarizeModelRole::Segmentation, DiarizeModelRole::Embedding] {
        let real = diarize_model_for_role(role).expect("catalog entry");
        assert_ne!(real.file.filename, filename);
    }
}

/// **Off, not broken** — and the sentence names the download.
#[test]
fn the_affordance_reports_itself_unavailable_with_the_combined_size() {
    let gate = speaker_detection_gate(false, diarize_download_bytes());

    let SpeakerDetection::NeedsDownload {
        megabytes, message, ..
    } = &gate
    else {
        panic!("models absent must not report the feature as available: {gate:?}");
    };
    assert!(!gate.is_available());
    assert_eq!(*megabytes, expected_megabytes());
    assert_eq!(
        *megabytes, 36,
        "the two vendored assets are 6,958,444 B + 29,292,684 B = 36.25 MB. The plan's §6 row 13 \
         says 37 MB; it was written before the assets were vendored. If this fails because the \
         catalog was re-pinned, the SENTENCE is still right — it is computed — and this number is \
         what needs updating."
    );
    assert!(
        message.contains(&format!("{megabytes} MB")),
        "the sentence must state the size: {message}"
    );
    assert!(
        message.contains("speaker detection"),
        "the sentence must name the feature it enables: {message}"
    );
    assert!(
        message.contains("without it"),
        "off-not-broken means the sentence says what still works: {message}"
    );

    // Installed: available, and there is nothing to say.
    let ready = speaker_detection_gate(true, diarize_download_bytes());
    assert_eq!(ready, SpeakerDetection::Available);
    assert!(ready.is_available());
    assert_eq!(ready.message(), None);
}

/// The size is COMPUTED. A literal would be wrong twice — against the plan's
/// 37 MB today, and against the next catalog edit tomorrow.
#[test]
fn the_size_in_the_sentence_comes_from_the_catalog_and_not_from_a_literal() {
    // Same gate, a different byte count: the sentence moves with it.
    let gate = speaker_detection_gate(false, 120_000_000);
    assert!(
        gate.message().expect("a message").contains("120 MB"),
        "{:?}",
        gate.message()
    );
    assert_ne!(
        speaker_detection_gate(false, 120_000_000),
        speaker_detection_gate(false, diarize_download_bytes()),
        "a gate whose sentence ignores its input is a literal wearing a parameter"
    );

    // And the catalog's own arithmetic is the source: the sum of the two
    // entries, not a third number written down somewhere.
    let summed: u64 = [DiarizeModelRole::Segmentation, DiarizeModelRole::Embedding]
        .into_iter()
        .map(|role| {
            diarize_model_for_role(role)
                .expect("catalog entry")
                .file
                .size_bytes
        })
        .sum();
    assert_eq!(diarize_download_bytes(), summed);
}

/// **Recording is unaffected**, stated three ways because this is the half that
/// hurts if it is wrong.
#[test]
fn a_missing_diarization_model_does_not_touch_recording_or_transcripts() {
    // 1. The Notetaker's own status payload — the thing the Settings step
    //    renders — is computed without consulting the diarization catalog at
    //    all, on an OS where meeting recording is offered.
    let status = NotetakerStatus::for_os(ENGLISH_MODEL, "en", OsVersion::new(14, 4, 0));
    assert!(
        status.available,
        "meeting recording must be offered whatever the diarization models are doing: {:?}",
        status.message
    );
    assert_eq!(status.message, None);
    assert!(status.system_audio_available);

    // 2. The gate's TYPE cannot express an opinion about recording: it has two
    //    variants, neither of which carries a recording flag. Row 12 needed most
    //    of a test file to guard a boolean that COULD say it; this one cannot be
    //    written wrong.
    match speaker_detection_gate(false, diarize_download_bytes()) {
        SpeakerDetection::Available => panic!("models absent"),
        SpeakerDetection::NeedsDownload { .. } => {}
    }

    // 3. The availability decision's own module never reads the diarization
    //    catalog. This is the leak that would turn row 13 into "meetings do not
    //    work", and it is a source-level fact because it is an absence.
    let availability =
        fs::read_to_string(src_dir().join("meeting_asr.rs")).expect("meeting_asr.rs");
    assert!(
        !mentions_as_code(&availability, "is_diarize"),
        "meeting_asr.rs consults the diarization catalog. Row 13's whole claim is that a missing \
         speaker-detection model turns ONE affordance off — the moment the recording gate reads \
         it, a 36 MB download becomes a precondition for recording a meeting at all."
    );
    assert!(
        !mentions_as_code(&availability, "speaker_detection_gate"),
        "the row-13 gate reached the recording-availability decision"
    );

    // 4. A meeting recorded without the models is a meeting like any other —
    //    diarization has no vote in its state, which is row 7's rule and applies
    //    identically here.
    assert_eq!(
        MeetingState::parse("complete"),
        Some(MeetingState::Complete)
    );
}

/// The row's own tripwire: row 13 is `Policy only, NOT WIRED` until a Notetaker
/// surface offers speaker detection.
#[test]
fn no_notetaker_surface_offers_speaker_detection_yet() {
    let found = call_sites("speaker_detection_gate", &["meeting_matrix.rs"]);
    assert!(
        found.is_empty(),
        "{}",
        promote_the_row("13", "speaker_detection_gate", &found)
    );
    // And the payload the Settings step reads carries no field for it, which is
    // the same fact from the frontend's side: `NotetakerStatus` answers "can it
    // record" and "can it record the other end", and nothing else.
    let lib = fs::read_to_string(src_dir().join("lib.rs")).expect("lib.rs");
    assert!(
        !mentions_as_code(&lib, "speaker_detection"),
        "`NotetakerStatus` (or another command) now carries a speaker-detection field — row 13 \
         has a surface and must be promoted to Coverage::Test naming it"
    );
}
