//! yap-audit-notes-strategy — arm E, ITEMS FIRST and the Summary LAST, is the
//! shipped notes strategy, and its honest fallback copy is on the render path.
//!
//! Stub model only (no sidecar, no GGUF). The three fallback renderings each
//! have a test: no Summary -> "Topics" and no paragraph; failed items ->
//! "Lines that sounded like commitments", never "Action items"; every card
//! ends with the footer.

mod support;

use support::{labels_in, segments_from, StubModel};
use wilson_voice_lib::summarize::{
    cited_window, commitment_lines, ground_items_in_window, invented_name,
    items_first_verbatim_ratio, lemma_grounded, map_grammar, map_grammar_items_first,
    narrative_containment, notes_strategy, recover_topic, render_notes_items_first,
    summarize_segments, transcript_lines, validate_items_first_summary, validate_topic,
    CommitmentLine, Headline, NotesStrategy, SummaryItem, TopicLabel, COMMITMENT_LINES_HEADING,
    NOTES_FOOTER, NOTES_STRATEGY, NO_SUMMARY_VERBATIM, SUMMARY_VERBATIM_CEILING,
    SUMMARY_VERBATIM_SLACK_NGRAMS, TOPICS_HEADING,
};

const FIXTURE: &[&str] = &[
    "Okay so this is the contract review for the Liberty University pilot.",
    "Jeisil will send the signed contract to Liberty by Friday.",
    "Maybe someone could look at the onboarding deck at some point.",
    "We decided the pilot starts with two hundred students.",
    "Is the pilot price still open for the second semester?",
];

fn plain() -> String {
    FIXTURE.join("\n")
}

fn headline() -> Headline {
    Headline {
        title: "Liberty pilot contract review".to_string(),
        ..Headline::default()
    }
}

fn item(text: &str, seg: &str, at: f64) -> SummaryItem {
    SummaryItem {
        text: text.to_string(),
        segment: seg.to_string(),
        start_seconds: at,
        speaker: None,
    }
}

fn topics() -> Vec<TopicLabel> {
    vec![TopicLabel {
        label: "Liberty pilot contract review".to_string(),
        start_seconds: 0.0,
    }]
}

/// MAP answers with a topic label, one good action, one action that names an
/// owner the meeting never said (it must not survive as an action), a
/// decision and a question.
fn map_answer(req: &wilson_voice_lib::polish_protocol::PolishRequest) -> String {
    let ids = labels_in(&req.text);
    let id = |i: usize| ids.get(i).cloned().unwrap_or_else(|| ids[0].clone());
    format!(
        r#"{{"topic":"Liberty University pilot contract review","actions":[{{"text":"Jeisil will send the signed contract to Liberty by Friday.","segment":"{}"}},{{"text":"Priya will send the onboarding deck.","segment":"{}"}}],"decisions":[{{"text":"We decided the pilot starts with two hundred students.","segment":"{}"}}],"questions":[{{"text":"Is the pilot price still open for the second semester?","segment":"{}"}}]}}"#,
        id(1),
        id(2),
        id(3),
        id(4)
    )
}

fn model(summary: &'static str) -> StubModel {
    StubModel::with_headline(move |req| {
        Ok(match req.mode.as_str() {
            "map" => map_answer(req),
            "reduce" => summary.to_string(),
            "headline" => r#"{"title":"Liberty pilot contract review","key_points":["The pilot starts with two hundred students.","Jeisil will send the signed contract to Liberty by Friday."],"summary":"Ignored under items first."}"#.to_string(),
            other => panic!("unexpected stage {other}"),
        })
    })
}

#[test]
fn items_first_is_the_shipped_default() {
    assert_eq!(NOTES_STRATEGY, NotesStrategy::E);
    assert_eq!(
        notes_strategy(),
        NotesStrategy::E,
        "nothing in this binary overrides it"
    );
    assert_eq!(NotesStrategy::E.tag(), "E");
    assert_eq!(SUMMARY_VERBATIM_CEILING, 0.5);
}

#[test]
fn the_items_first_grammar_swaps_the_narrative_for_a_short_topic() {
    let labels = vec!["seg_0001".to_string(), "seg_0002".to_string()];
    let e = map_grammar_items_first(&labels).expect("a grammar");
    let a = map_grammar(&labels).expect("a grammar");
    assert!(e.contains("\\\"topic\\\""), "{e}");
    assert!(!e.contains("\\\"narrative\\\""), "{e}");
    assert!(e.contains("topic ::="), "{e}");
    for label in &labels {
        assert!(
            e.contains(label.as_str()),
            "the id allowlist is unchanged: {e}"
        );
    }
    // The item rules are the same rules.
    let rule = |g: &str, name: &str| g.lines().find(|l| l.starts_with(name)).map(str::to_string);
    assert_eq!(rule(&e, "item ::="), rule(&a, "item ::="));
    assert_eq!(rule(&e, "evid ::="), rule(&a, "evid ::="));
}

#[test]
fn a_topic_label_is_three_to_eight_grounded_words() {
    let src = plain();
    assert_eq!(
        recover_topic(r#"{"topic":"Liberty pilot contract review","actions":[]}"#).as_deref(),
        Some("Liberty pilot contract review")
    );
    assert_eq!(recover_topic(r#"{"topic":"cut off mid"#), None);
    assert_eq!(
        validate_topic(&src, "Liberty pilot contract review.").as_deref(),
        Some("Liberty pilot contract review")
    );
    assert_eq!(validate_topic(&src, "Pilot"), None, "too short");
    assert_eq!(
        validate_topic(
            &src,
            "the pilot and the contract and the students and the price"
        ),
        None,
        "too long"
    );
    assert_eq!(
        validate_topic(&src, "Pricing for Acme Corp renewal"),
        None,
        "invented name"
    );
}

#[test]
fn paraphrase_grounds_at_lemma_level_and_names_stay_hard_gated() {
    let said = "We discussed the plans and agreed on the pricing.";
    assert_eq!(
        lemma_grounded(said, "discussing plan, agreeing pricing"),
        1.0
    );
    assert_eq!(
        invented_name(said, "They discussed the plans with Priya."),
        Some("Priya".to_string())
    );
    assert_eq!(
        invented_name(said, "We discussed the plans."),
        None,
        "a sentence opener is not a name"
    );
}

#[test]
fn items_are_grounded_against_the_cited_segment_plus_or_minus_one() {
    let lines = transcript_lines(&segments_from(FIXTURE));
    let l = |i: usize| lines[i].label.clone();
    let window = cited_window(&lines, &l(0)).expect("a window");
    assert!(window.contains("contract review") && window.contains("Jeisil will send"));
    assert!(!window.contains("onboarding deck"), "+-1 only: {window}");
    let mut actions = vec![
        item(
            "Jeisil will send the signed contract to Liberty by Friday.",
            &l(1),
            4.0,
        ),
        // Grounded in the transcript, but not near the line it cites.
        item("The second semester pilot price is open.", &l(1), 4.0),
        item("Priya will send the onboarding deck.", &l(2), 8.0),
    ];
    let failed = ground_items_in_window(&mut actions, &lines);
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(failed.len(), 2, "{failed:?}");
    let quoted = commitment_lines(&failed, &lines);
    assert_eq!(quoted.len(), 2, "{quoted:?}");
    assert_eq!(quoted[1].quote, FIXTURE[2], "the transcript's own words");
    assert_eq!(quoted[1].start_seconds, 8.0);
}

#[test]
fn the_summary_gate_refuses_invented_facts_and_re_transcripts() {
    let src = plain();
    assert!(validate_items_first_summary(
        &src,
        "The Liberty pilot starts with two hundred students and the contract goes out this week."
    )
    .is_some());
    assert!(
        validate_items_first_summary(&src, "Priya will run the Liberty pilot.").is_none(),
        "invented name"
    );
    assert!(
        validate_items_first_summary(&src, "The pilot costs 400 dollars.").is_none(),
        "invented digits"
    );
    assert!(
        validate_items_first_summary(&src, "Jeisil will send the signed contract to Liberty by Friday. We decided the pilot starts with two hundred students.").is_none(),
        "a re-transcript"
    );
}

/// Bounce fix (loop three, batch 1): the guards' clean planted Summary, a
/// one-sentence paraphrase that joins two items, lifts 7 of its 13 distinct
/// 4-grams (raw 0.54) because it must name the meeting ("the Liberty
/// University pilot") and the items it is written from are transcript lines.
/// One lifted 4-gram is forgiven, so it judges 6 of 13 and is kept; two lines
/// pasted together (13 of 16) still judge 12 of 16 and are refused.
#[test]
fn the_summary_gate_forgives_one_lifted_phrase_and_no_more() {
    let src = plain();
    assert_eq!(SUMMARY_VERBATIM_SLACK_NGRAMS, 1);
    let clean = "The Liberty University pilot starts with two hundred students and Jeisil will send the signed contract.";
    let raw = narrative_containment(&src, clean);
    let judged = items_first_verbatim_ratio(&src, clean);
    println!("MEASURED clean raw={raw:.3} judged={judged:.3} max={SUMMARY_VERBATIM_CEILING}");
    assert!((raw - 7.0 / 13.0).abs() < 1e-9, "{raw}");
    assert!(raw > SUMMARY_VERBATIM_CEILING, "{raw}");
    assert!((judged - 6.0 / 13.0).abs() < 1e-9, "{judged}");
    assert_eq!(
        validate_items_first_summary(&src, clean).as_deref(),
        Some(clean)
    );

    let pasted = "Jeisil will send the signed contract to Liberty by Friday. We decided the pilot starts with two hundred students.";
    let judged = items_first_verbatim_ratio(&src, pasted);
    println!("MEASURED pasted judged={judged:.3} max={SUMMARY_VERBATIM_CEILING}");
    assert!((judged - 12.0 / 16.0).abs() < 1e-9, "{judged}");
    assert!(validate_items_first_summary(&src, pasted).is_none());

    // Under four words there is no 4-gram to judge.
    assert_eq!(
        items_first_verbatim_ratio(&src, "Liberty pilot review."),
        0.0
    );
    // The allowance never goes negative: one lifted 4-gram of four judges 0.
    assert_eq!(
        items_first_verbatim_ratio(&src, "The Liberty University pilot starts next term."),
        0.0
    );
}

// ── the three fallback renderings ───────────────────────────────────────────

#[test]
fn fallback_no_summary_shows_topics_and_no_paragraph() {
    let md = render_notes_items_first(
        &headline(),
        &[],
        "",
        &[item(
            "Jeisil will send the signed contract to Liberty by Friday.",
            "seg_0002",
            4.0,
        )],
        &[],
        &[],
        &topics(),
        &[],
        0,
        1,
        false,
        false,
    );
    println!("{md}");
    assert!(!md.contains("### Summary"), "no paragraph: {md}");
    assert!(
        md.contains(&format!(
            "### {TOPICS_HEADING}\n\n- Liberty pilot contract review (00:00:00)"
        )),
        "{md}"
    );
    let topics_at = md.find("### Topics").unwrap();
    let actions_at = md.find("### Action items").unwrap();
    assert!(
        topics_at < actions_at,
        "Topics stand where the Summary would: {md}"
    );
    assert!(md.ends_with(NOTES_FOOTER), "{md}");

    // A refused re-transcript says so, and Topics still follow.
    let refused = render_notes_items_first(
        &headline(),
        &[],
        "",
        &[],
        &[],
        &[],
        &topics(),
        &[],
        0,
        1,
        false,
        true,
    );
    assert!(refused.contains(NO_SUMMARY_VERBATIM), "{refused}");
    assert!(refused.contains("### Topics"), "{refused}");
}

#[test]
fn fallback_failed_items_are_quoted_commitment_lines_never_action_items() {
    let commitments = vec![CommitmentLine {
        quote: FIXTURE[2].to_string(),
        start_seconds: 65.0,
        track: Some("Them".to_string()),
    }];
    let md = render_notes_items_first(
        &headline(),
        &[],
        "The Liberty pilot contract was reviewed.",
        &[],
        &[],
        &[],
        &topics(),
        &commitments,
        0,
        1,
        false,
        false,
    );
    println!("{md}");
    assert!(!md.contains("### Action items"), "{md}");
    assert!(
        md.contains(&format!(
            "### {COMMITMENT_LINES_HEADING}\n\n- \"Maybe someone could look at the onboarding deck at some point.\" (Them, 00:01:05)"
        )),
        "{md}"
    );
    assert!(
        md.contains("### Summary\n\nThe Liberty pilot contract was reviewed."),
        "{md}"
    );
    assert!(
        !md.contains("### Topics"),
        "a Summary passed, so no Topics: {md}"
    );
    assert!(md.ends_with(NOTES_FOOTER), "{md}");
}

#[test]
fn fallback_every_card_ends_with_the_footer() {
    assert_eq!(
        NOTES_FOOTER,
        "Written on this Mac by a small model. Check owners against the transcript."
    );
    let empty = render_notes_items_first(
        &Headline::default(),
        &[],
        "",
        &[],
        &[],
        &[],
        &[],
        &[],
        0,
        1,
        false,
        false,
    );
    assert!(empty.ends_with(NOTES_FOOTER), "{empty}");
    let s = summarize_segments(&segments_from(FIXTURE), &model("The Liberty pilot starts with two hundred students and the contract goes out this week.")).expect("notes");
    assert!(s.markdown.ends_with(NOTES_FOOTER), "{}", s.markdown);
}

// ── end to end through the shipped default ──────────────────────────────────

#[test]
fn the_summary_is_one_pass_over_the_surviving_items_and_topics() {
    let m = model(
        "The Liberty pilot starts with two hundred students and the contract goes out this week.",
    );
    let s = summarize_segments(&segments_from(FIXTURE), &m).expect("notes");
    let md = &s.markdown;
    println!("{md}");
    // MAP asked for a topic, not a narrative.
    let map = m.stage("map");
    assert!(!map.is_empty());
    assert!(map[0]
        .grammar_text()
        .unwrap_or_default()
        .contains("topic ::="));
    // ONE Summary pass; its input is items + topics, never transcript lines.
    let reduce = m.stage("reduce");
    assert_eq!(reduce.len(), 1, "one Summary pass");
    let sent = &reduce[0].text;
    assert!(
        sent.contains("Topics:") && sent.contains("Liberty University pilot contract review"),
        "{sent}"
    );
    assert!(
        sent.contains("Jeisil will send the signed contract to Liberty by Friday."),
        "{sent}"
    );
    assert!(
        sent.contains("We decided the pilot starts with two hundred students."),
        "{sent}"
    );
    assert!(!sent.contains("seg_"), "no transcript ids: {sent}");
    assert!(
        !sent.contains("Priya"),
        "a failed item never reaches the Summary: {sent}"
    );
    // The headline's own paragraph is not the Summary under arm E.
    assert!(!md.contains("Ignored under items first."), "{md}");
    assert!(
        md.contains("### Summary\n\nThe Liberty pilot starts"),
        "{md}"
    );
    // The invented-owner action is a quoted line, not an action item.
    assert!(
        s.actions.iter().all(|a| !a.text.contains("Priya")),
        "{:?}",
        s.actions
    );
    assert!(
        md.contains(&format!("### {COMMITMENT_LINES_HEADING}")),
        "{md}"
    );
    assert!(
        md.contains("\"Maybe someone could look at the onboarding deck at some point.\""),
        "{md}"
    );
    assert_eq!(s.decisions.len(), 1, "{:?}", s.decisions);
    assert_eq!(s.questions.len(), 1, "{:?}", s.questions);
    assert!(md.ends_with(NOTES_FOOTER), "{md}");
}

#[test]
fn a_refused_summary_leaves_topics_not_a_paragraph() {
    // The Summary pass hands back the transcript itself.
    let m = model("Jeisil will send the signed contract to Liberty by Friday. We decided the pilot starts with two hundred students. Is the pilot price still open for the second semester?");
    let s = summarize_segments(&segments_from(FIXTURE), &m).expect("items still make notes");
    let md = &s.markdown;
    println!("{md}");
    assert!(s.narrative.trim().is_empty(), "{}", s.narrative);
    assert!(md.contains(NO_SUMMARY_VERBATIM), "{md}");
    assert!(
        md.contains("### Topics\n\n- Liberty University pilot contract review (00:00:00)"),
        "{md}"
    );
    assert!(md.ends_with(NOTES_FOOTER), "{md}");
}
