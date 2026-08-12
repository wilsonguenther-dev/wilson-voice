//! YV97 acceptance — "a summary fabricating a due date or an @address absent
//! from the source transcript is rejected/stripped, reusing the ported
//! `validate_polish` logic."
//!
//! The port is not a copy. V4 (invented digits), V5 (invented `@`-address or
//! URL), V6 (template leak / assistant preamble) and V7 (script drift) mean
//! exactly what they mean for a rewrite, and reuse the polish stage's own
//! helpers. V2 and V3 are REINTERPRETED, and it matters why: the polish
//! validator's length band and its ≥80% content-word RETENTION floor both assume
//! output ≈ input. A summary is supposed to be shorter and is supposed to drop
//! content, so retention becomes GROUNDEDNESS — the share of what the summary
//! SAYS that is actually in the source — and the band becomes a per-item
//! sentence cap plus a runaway ceiling on the narrative.
//!
//! Both halves of the acceptance are exercised: rejected (the validator returns
//! `None`) and stripped (the same item vanishes from a real MAP answer while its
//! honest neighbours survive).

mod support;

use support::{map_answer, SEGMENT_TEXTS};
use wilson_voice_lib::summarize::{
    parse_map_output, rejected_total, validate_item, validate_narrative, MAX_NARRATIVE_WORDS,
};

/// The source every claim below is checked against. No digits, no addresses —
/// so anything numeric or `@`-shaped in an output is, by construction, invented.
fn source() -> String {
    SEGMENT_TEXTS
        .iter()
        .enumerate()
        .map(|(i, t)| format!("seg_{:04}: {t}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_fabricated_due_date_or_address_is_rejected() {
    let source = source();

    // An honest item, drawn from the transcript, is kept — otherwise the gate
    // below proves nothing.
    assert_eq!(
        validate_item(&source, "Move the onboarding review"),
        Some("Move the onboarding review".to_string())
    );

    // V4 — a fabricated due date, ISOLATED: every word of this item is in its
    // source, so groundedness passes and the DIGITS are the only thing wrong.
    // That is what makes this a test of V4 rather than of V3 twice.
    const DATED: &str = "seg_0001: we agreed to move the onboarding review to march";
    assert_eq!(
        validate_item(DATED, "Move the onboarding review to march"),
        Some("Move the onboarding review to march".to_string()),
        "the same sentence without the invented number is kept"
    );
    assert_eq!(
        validate_item(DATED, "Move the onboarding review to march 14"),
        None
    );
    assert_eq!(
        validate_item(DATED, "Move the onboarding review to march at 3:30"),
        None
    );

    // V5 — an @-address, ISOLATED the same way: the source names the same
    // people and places, so only the address SHAPE is new.
    const ADDRESSED: &str = "seg_0001: send the onboarding review to wilson at drivia consulting";
    assert_eq!(
        validate_item(
            ADDRESSED,
            "Send the onboarding review to wilson at drivia consulting"
        ),
        Some("Send the onboarding review to wilson at drivia consulting".to_string())
    );
    assert_eq!(
        validate_item(
            ADDRESSED,
            "Send the onboarding review to wilson@drivia.consulting"
        ),
        None
    );
    assert_eq!(
        validate_item(
            &source,
            "Move the onboarding review at https://example.com/plan"
        ),
        None
    );

    // V3 (inverted) — words the source never contained at all.
    assert_eq!(
        validate_item(&source, "Renegotiate the vendor contract with procurement"),
        None
    );

    // V6 — template leak and assistant behaviour.
    assert_eq!(
        validate_item(&source, "<|im_start|>Move the onboarding review"),
        None
    );
    assert_eq!(
        validate_item(&source, "Sure, move the onboarding review"),
        None
    );

    // V7 — script drift (a script the meeting never used cannot be grounded
    // either; both gates refuse it, and neither may let it through).
    assert_eq!(validate_item(&source, "行動項目の確認とレビュー"), None);

    // V1/V2 — nothing, and a paragraph pretending to be one item.
    assert_eq!(validate_item(&source, "   "), None);
    let paragraph = std::iter::repeat("onboarding review")
        .take(40)
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(validate_item(&source, &paragraph), None);
}

#[test]
fn a_fabricated_item_is_stripped_while_its_neighbours_survive() {
    let source = source();
    let labels: Vec<String> = (1..=3).map(|i| format!("seg_{i:04}")).collect();
    let before = rejected_total();

    let answer = map_answer(
        "The onboarding review should move before the release goes out.",
        &[
            ("Move the onboarding review", "seg_0001"),
            ("Move the onboarding review by March 14", "seg_0001"),
            ("Send the review to wilson@drivia.consulting", "seg_0002"),
        ],
        &[("Shipping without the calendar work", "seg_0002")],
        &[("Who owns the escalation path", "seg_0003")],
    );

    let out = parse_map_output(&answer, &labels, &source);
    assert_eq!(
        out.actions,
        vec![wilson_voice_lib::summarize::ExtractedItem {
            text: "Move the onboarding review".to_string(),
            segment: "seg_0001".to_string(),
        }],
        "the fabricated date and the fabricated address are both stripped"
    );
    assert_eq!(out.decisions.len(), 1);
    assert_eq!(out.questions.len(), 1);
    assert_eq!(out.dropped, 2);
    assert!(
        rejected_total() >= before + 2,
        "the gate counts what it threw away"
    );
}

#[test]
fn the_narrative_is_gated_and_capped() {
    let source = source();

    // Grounded, short, and kept verbatim.
    let good = "The onboarding review should move before the release goes out.";
    assert_eq!(validate_narrative(&source, good), Some(good.to_string()));

    // Fabricated numbers and addresses fail the same way an item does.
    assert_eq!(
        validate_narrative(&source, "The team agreed to ship 3 features by March 14."),
        None
    );
    assert_eq!(
        validate_narrative(&source, "Follow-ups go to wilson@drivia.consulting."),
        None
    );

    // A "summary" that is really a re-transcript is refused, not trimmed —
    // trimming a runaway keeps its first half and hides the failure.
    let runaway = std::iter::repeat("the onboarding review should move")
        .take(200)
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(validate_narrative(&source, &runaway), None);

    // Between the two: grounded, long, capped at the word limit the acceptance
    // names, cut at a sentence rather than mid-clause.
    let long = std::iter::repeat("The onboarding review should move before the release goes out.")
        .take(30)
        .collect::<Vec<_>>()
        .join(" ");
    let capped = validate_narrative(&source, &long).expect("grounded prose is kept");
    let words = capped.split_whitespace().count();
    assert!(
        words <= MAX_NARRATIVE_WORDS,
        "a narrative is capped at {MAX_NARRATIVE_WORDS} words, got {words}"
    );
    assert!(
        capped.ends_with('.'),
        "the cap lands on a sentence: {capped:?}"
    );
}
