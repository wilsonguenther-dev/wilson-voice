//! Y4-G acceptance — **undo restores BYTE-IDENTICAL raw text through the
//! clipboard path.**
//!
//! The parity backlog's acceptance for "see what changed + undo" is exactly that
//! sentence, and it has to be ASSERTED rather than assumed, for two reasons that
//! are specific to this app:
//!
//! * The stored raw goes through SQLite and back. `raw_text` is a nullable TEXT
//!   column (YV10), and a round trip that trims, re-encodes or normalises
//!   whitespace would make undo "restore" something the user never said. So the
//!   raw here carries trailing whitespace, interior blank lines, a tab and a
//!   non-ASCII character on purpose.
//! * `snippets::append_signature` copies the configured block BYTE FOR BYTE
//!   after polish (lib.rs), which means the polished text an undo is replacing
//!   can end in a block that was never dictated. `undo_ai_edit_text` must still
//!   answer with the raw take exactly — not the raw plus the signature, not the
//!   polished minus the signature.
//!
//! What this file does NOT test: the ⌘V hop itself. `paste::copy_and_maybe_paste`
//! drives a CGEvent against the frontmost app and needs Accessibility, which is
//! machine state, not code. What IS tested is everything upstream of it — the
//! exact `&str` that path is handed — because that is where a byte can be lost.

mod support;

use support::{open_db, temp_dir};
use wilson_voice_lib::dictation::{
    run_cleanup_traced, undo_ai_edit_text, CleanupLevel, DictationMode, Style,
};
use wilson_voice_lib::snippets::{append_signature, SignatureMode};

/// A raw take with every byte an over-eager round trip likes to eat.
const RAW: &str =
    "um so the report is uh done\n\n\tand the numbers are fine — café  \nlet me know\n   ";

const SIGNATURE: &str = "Best,\nWilson\n  (sent from Yap)";

#[test]
fn undo_restores_byte_identical_raw_including_signature() {
    let dir = temp_dir("y4g-undo");
    let db = open_db(&dir);

    // Run the real pipeline over the raw take: rules only, no model on this
    // machine, which is the shipped High-with-no-sidecar shape.
    let (cleaned, trace) = run_cleanup_traced(
        RAW,
        CleanupLevel::High,
        DictationMode::Email,
        Style::default(),
        &[],
        |t| t.to_string(),
        |_| Err("no_model"),
    );
    assert_eq!(
        trace.llm_skip_reason,
        Some("no_model"),
        "the stage was asked for and produced nothing — that must be recorded"
    );

    // …then the LAST stage of the live path: the signature block, copied byte
    // for byte onto the polished text. `Cue` + a spoken "sign it" is the
    // deterministic way in — `Auto` depends on the sign-off heuristic, which is
    // not what this test is about.
    let polished = append_signature(
        &format!("{cleaned} sign it"),
        SIGNATURE,
        SignatureMode::Cue,
        DictationMode::Email,
    );
    assert!(
        polished.ends_with(SIGNATURE),
        "fixture must actually exercise the signature path; got {polished:?}"
    );
    assert!(
        !RAW.contains("Wilson"),
        "the signature must not be in the raw take, or this asserts nothing"
    );

    // The row the live path writes.
    let entry = db
        .insert_transcript_at(
            polished.clone(),
            "test".into(),
            1.0,
            1.0,
            10,
            None,
            chrono::Utc::now(),
            Some(RAW.to_string()),
        )
        .expect("insert transcript");

    // Read it BACK — the undo path reads the newest row, it does not reuse the
    // in-memory entry, so the assertion has to survive SQLite.
    let stored = db
        .list_transcripts(1, None)
        .expect("list transcripts")
        .into_iter()
        .next()
        .expect("one row");
    assert_eq!(stored.id, entry.id);

    let restored = undo_ai_edit_text(&stored.text, stored.raw_text.as_deref())
        .expect("there is an AI edit to undo");

    // The whole point, byte for byte.
    assert_eq!(
        restored, RAW,
        "undo must hand the clipboard path the raw take verbatim"
    );
    assert_eq!(restored.as_bytes(), RAW.as_bytes());
    assert!(
        restored.ends_with("\n   "),
        "trailing whitespace must survive the round trip: {restored:?}"
    );
    assert!(restored.contains('\t'), "the tab must survive");
    assert!(restored.contains("café"), "non-ASCII must survive");
    assert!(
        !restored.contains("Wilson"),
        "undo must NOT carry the signature back — it was never dictated"
    );
}

/// The signature block is the one part of a take that is copied verbatim after
/// polish, so a take whose ONLY difference from raw is the signature still has
/// something to undo — and undoing it must drop exactly that block.
#[test]
fn undo_drops_a_signature_that_was_never_dictated() {
    let raw = "thanks for the update";
    let polished = append_signature(
        &format!("{raw} sign it"),
        SIGNATURE,
        SignatureMode::Cue,
        DictationMode::Email,
    );
    assert_ne!(polished, raw);

    let restored = undo_ai_edit_text(&polished, Some(raw)).expect("signature is an AI edit");
    assert_eq!(restored, raw);
    assert_eq!(restored.as_bytes(), raw.as_bytes());
}

/// The inert case, which is the one that could blank a user's text: raw and
/// polished agree, so undo must decline rather than re-paste.
#[test]
fn undo_declines_when_the_pipeline_changed_nothing() {
    assert_eq!(undo_ai_edit_text("ship it", Some("ship it")), None);
    assert_eq!(undo_ai_edit_text("ship it", None), None);
    assert_eq!(undo_ai_edit_text("ship it", Some("   ")), None);
}
