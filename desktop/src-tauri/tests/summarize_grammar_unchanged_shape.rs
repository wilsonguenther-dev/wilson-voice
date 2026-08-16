//! YV133 acceptance — "`map_grammar`'s enum still enumerates only `evid`
//! (segment ids), not speaker names — speaker is never part of what the grammar
//! constrains, only what gets looked up afterward, keeping YV97's existing
//! hallucination guard exactly as strict as it was."
//!
//! The whole of YV133 is one sentence: the model reads speakers and never writes
//! them. This file is the second half of that sentence, asserted in the four
//! places a speaker could have leaked into the write path —
//!
//! * the GBNF itself (a `speaker ::=` rule, or a name in the `evid` enum);
//! * `Chunk::labels`, which is both the enum the grammar is built over and the
//!   allowlist the parser enforces — a label that carried its speaker would put
//!   the speaker inside the enum by the back door;
//! * `ExtractedItem`, the type a MAP answer deserializes into;
//! * `summarize.rs` itself, where the lookup happens.
//!
//! The first three are asserted on values. The fourth is asserted on the SOURCE,
//! the same way `meeting_kind_branch.rs` asserts `speaker_label` decides on the
//! target: "no code path constructs a speaker from model output" is a claim
//! about which expression feeds a field, and no runtime fixture can distinguish
//! a looked-up name from an identical name that was copied out of the answer.

mod support;

use support::{segments_from, EnrolledSpeakers};
use wilson_voice_lib::summarize::{
    map_grammar, plan_chunks, transcript_lines, transcript_lines_with, ExtractedItem,
};

const TURNS: [&str; 3] = [
    "we should move the onboarding review before the release goes out",
    "i will send the pricing document over tomorrow morning",
    "nobody owns the escalation path yet and someone should fix that",
];

const SUMMARIZE_RS: &str = include_str!("../src/summarize.rs");

/// The rule names YV97 shipped, in order. A grammar that grew a rule grew a
/// field, and a field is a thing a model fills in.
const YV97_RULES: [&str; 7] = ["root", "items", "item", "string", "char", "evid", "ws"];

fn rule_names(grammar: &str) -> Vec<String> {
    grammar
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            l.split("::=")
                .next()
                .expect("every rule has a name")
                .trim()
                .to_string()
        })
        .collect()
}

/// The grammar built from an attributed transcript is **byte-identical** to the
/// grammar built from the same transcript with no speakers at all.
#[test]
fn the_grammar_is_byte_identical_with_and_without_speakers() {
    let segments = segments_from(&TURNS);
    let enrolled = EnrolledSpeakers::new(&[
        ("segment-0", "Jeisil"),
        ("segment-1", "Aidan"),
        ("segment-2", "Jeisil"),
    ]);
    let counter = support::StubModel::new(|_| Ok(String::new()));

    let bare = plan_chunks(&transcript_lines(&segments), 4000, &counter).expect("chunks");
    let attributed =
        plan_chunks(&transcript_lines_with(&segments, &enrolled), 4000, &counter).expect("chunks");

    // The prompts differ — that is the feature.
    assert_ne!(bare[0].text(), attributed[0].text());
    assert!(attributed[0].text().contains("seg_0001 (Jeisil): "));

    // The labels, the enum and the allowlist do not.
    assert_eq!(bare[0].labels(), attributed[0].labels());
    assert_eq!(
        bare[0].labels(),
        vec!["seg_0001", "seg_0002", "seg_0003"],
        "a label is an id and nothing else"
    );
    let bare_grammar = map_grammar(&bare[0].labels()).expect("a grammar");
    let attributed_grammar = map_grammar(&attributed[0].labels()).expect("a grammar");
    assert_eq!(
        bare_grammar, attributed_grammar,
        "speakers reached the prompt and not the grammar"
    );
    for name in ["Jeisil", "Aidan", "speaker", "Speaker", "Me", "Them"] {
        assert!(
            !attributed_grammar.contains(name),
            "{name:?} must not appear anywhere in the grammar:\n{attributed_grammar}"
        );
    }
}

/// The rule set is YV97's, exactly — no new rule, no renamed one.
#[test]
fn the_grammar_has_no_rule_yv97_did_not_ship() {
    let labels: Vec<String> = (1..=4).map(|i| format!("seg_{i:04}")).collect();
    let grammar = map_grammar(&labels).expect("labels make a grammar");
    assert_eq!(rule_names(&grammar), YV97_RULES.to_vec());
    assert!(grammar
        .contains(r#"evid ::= "\"seg_0001\"" | "\"seg_0002\"" | "\"seg_0003\"" | "\"seg_0004\"""#));
    // The item schema is still exactly two fields.
    assert!(grammar.contains(
        r#"item ::= "{" ws "\"text\"" ws ":" ws string ws "," ws "\"segment\"" ws ":" ws evid ws "}""#
    ));
}

/// `ExtractedItem` — the type the model's own answer parses into — has no
/// speaker field, so there is nothing for an unconstrained decode to aim at.
#[test]
fn the_models_output_type_has_no_speaker_field() {
    let json = serde_json::to_value(ExtractedItem {
        text: "Move the onboarding review".to_string(),
        segment: "seg_0001".to_string(),
    })
    .expect("serializes");
    // Sorted, because `serde_json`'s map is a `BTreeMap` here — the assertion
    // is about WHICH fields exist, and key order is not one of them.
    let mut fields: Vec<&str> = json
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(fields, vec!["segment", "text"]);

    // And the declaration says so, so a later edit that adds one has to delete
    // this assertion on purpose rather than pass it by accident.
    let decl = SUMMARIZE_RS
        .split("pub struct ExtractedItem {")
        .nth(1)
        .expect("ExtractedItem is declared here")
        .split('}')
        .next()
        .expect("the struct body closes");
    assert!(
        !decl.contains("speaker"),
        "a speaker field on the model's output type is a slot a model can aim \
         at; YV133 fills SummaryItem.speaker instead:\n{decl}"
    );
}

/// No code path constructs a speaker from model output.
///
/// `SummaryItem.speaker` is assigned in exactly one place in this module, and
/// the expression it is assigned from is the evidence lookup — never anything
/// reached through the parsed item, which in `summarize_segments_with` is bound
/// as `i`.
#[test]
fn the_only_speaker_assignment_reads_the_lookup() {
    let assignments: Vec<&str> = SUMMARIZE_RS
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("speaker:") && !l.starts_with("speaker: Option"))
        .collect();
    assert_eq!(
        assignments,
        vec![
            "speaker: None,",                                      // TranscriptLine::new
            "speaker: cited.and_then(|(_, _, who)| who.clone()),", // the lookup
        ],
        "every construction of a speaker value in summarize.rs, in order"
    );
    assert!(
        !SUMMARIZE_RS.contains("i.speaker"),
        "nothing may read a speaker off a parsed item"
    );
    assert!(
        !SUMMARIZE_RS.contains("item.speaker.unwrap") && !SUMMARIZE_RS.contains("answer.speaker"),
        "nor off a raw answer"
    );
}

/// The parser's allowlist is unchanged: a citation is matched against the bare
/// id, so an answer that echoed the rendered prefix back is dropped exactly as
/// an invented id is.
#[test]
fn a_citation_that_echoes_the_speaker_tag_is_dropped_like_any_other_bad_id() {
    let segments = segments_from(&TURNS);
    let enrolled = EnrolledSpeakers::new(&[("segment-0", "Jeisil")]);
    let lines = transcript_lines_with(&segments, &enrolled);
    let counter = support::StubModel::new(|_| Ok(String::new()));
    let chunks = plan_chunks(&lines, 4000, &counter).expect("chunks");
    let labels = chunks[0].labels();
    let source = chunks[0].text();

    let answer = serde_json::json!({
        "narrative": "",
        "actions": [{ "text": "Move the onboarding review", "segment": "seg_0001 (Jeisil)" }],
        "decisions": [],
        "questions": [],
    })
    .to_string();
    let out = wilson_voice_lib::summarize::parse_map_output(&answer, &labels, &source);
    assert!(out.actions.is_empty(), "the id allowlist is exact");
    assert_eq!(out.dropped, 1);
}
