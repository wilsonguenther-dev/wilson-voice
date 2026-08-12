//! YV97 acceptance — "a constrained request produces well-formed JSON matching
//! the actions/decisions/questions schema, and a malformed/truncated grammar
//! output is parsed defensively without panicking."
//!
//! Two halves, and it is worth being exact about which is which:
//!
//! * **The chain (finding #17).** A grammar sampler cannot be built without a
//!   resident model, so what is asserted here is everything up to that call: a
//!   summarize request carries a grammar, the request's [`sampler_plan`] is
//!   `GrammarThenGreedy` rooted at the grammar's own start symbol, and a plain
//!   rewrite still plans bare greedy. The sidecar builds
//!   `chain_simple([grammar, greedy])` from exactly that plan
//!   (`yap-polish/src/main.rs`), and `cargo test -p yap-polish` compiles and
//!   covers the selection.
//! * **The parse.** Fully exercised: well-formed output must deserialize into
//!   the schema, and every malformed shape a constrained decode can actually
//!   produce — cut mid-array, cut mid-string, fenced, empty, garbage, wrong
//!   types — must yield a defensible answer rather than a panic. The grammar
//!   guarantees SHAPE, never COMPLETION: a decode that hits its token budget
//!   stops in the middle of a legal string.

mod support;

use support::{labels_in, map_answer, segments, StubModel, SEGMENT_TEXTS};
use wilson_voice_lib::polish_protocol::{
    sampler_plan, PolishRequest, SamplerPlan, GRAMMAR_ROOT, KIND_SUMMARIZE, SUMMARIZE_MAP,
};
use wilson_voice_lib::summarize::{map_grammar, parse_map_output, summarize_segments};

#[test]
fn a_constrained_request_carries_a_grammar_and_plans_grammar_then_greedy() {
    let labels: Vec<String> = (1..=3).map(|i| format!("seg_{i:04}")).collect();
    let grammar = map_grammar(&labels).expect("labels make a grammar");

    // The grammar is rooted where the sampler will look for its start symbol.
    assert!(
        grammar.contains(&format!("{GRAMMAR_ROOT} ::=")),
        "the grammar must define the root the sampler is built with"
    );
    // The schema the acceptance names, spelled out in the grammar itself.
    for field in ["narrative", "actions", "decisions", "questions", "segment"] {
        assert!(grammar.contains(field), "the grammar covers `{field}`");
    }

    let req = PolishRequest::summarize(
        1,
        SUMMARIZE_MAP,
        288,
        30_000,
        "seg_0001: hello".to_string(),
        Some(grammar.clone()),
    );
    assert_eq!(req.kind, KIND_SUMMARIZE);
    assert_eq!(
        sampler_plan(&req),
        SamplerPlan::GrammarThenGreedy {
            // `grammar_text` trims — a trailing newline is not a rule.
            grammar: grammar.trim().to_string(),
            root: GRAMMAR_ROOT.to_string()
        },
        "a constrained request must plan the grammar AHEAD of greedy"
    );

    // An ordinary rewrite is untouched: the shared-sampler bug fixed here must
    // not become a grammar every dictation now pays for.
    let rewrite = PolishRequest::polish(2, "email", "default", 96, 1200, "hey".to_string());
    assert_eq!(sampler_plan(&rewrite), SamplerPlan::Greedy);
}

#[test]
fn a_constrained_pass_produces_well_formed_schema_json() {
    // The stub answers with exactly what the grammar admits, citing ids from the
    // chunk it was handed.
    let model = StubModel::new(|req| {
        let labels = labels_in(&req.text);
        Ok(map_answer(
            "The onboarding review should move before the release goes out.",
            &[("Move the onboarding review", labels[0].as_str())],
            &[("Shipping without the calendar work", labels[1].as_str())],
            &[("Who owns the escalation path", labels[2].as_str())],
        ))
    });

    let summary = summarize_segments(&segments(), &model).expect("a summary");
    assert_eq!(summary.actions.len(), 1);
    assert_eq!(summary.decisions.len(), 1);
    assert_eq!(summary.questions.len(), 1);
    assert_eq!(summary.actions[0].text, "Move the onboarding review");
    assert_eq!(summary.actions[0].segment, "seg_0001");
    assert_eq!(summary.dropped_items, 0);
    assert!(summary.markdown.contains("### Action items"));

    // The request that produced it really was constrained.
    let map = model.stage(SUMMARIZE_MAP);
    assert_eq!(map.len(), 1);
    assert!(map[0].grammar_text().is_some());
}

#[test]
fn malformed_and_truncated_output_is_parsed_defensively() {
    let labels: Vec<String> = (1..=3).map(|i| format!("seg_{i:04}")).collect();
    let source = SEGMENT_TEXTS
        .iter()
        .enumerate()
        .map(|(i, t)| format!("seg_{:04}: {t}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");

    // (a) Cut mid-array — the decode hit its token budget. The item that CLOSED
    // survives; the half-written one does not.
    let truncated = concat!(
        r#"{"narrative":"The team reviewed the release plan.","actions":["#,
        r#"{"text":"Move the onboarding review","segment":"seg_0001"},"#,
        r#"{"text":"Book the room for"#
    );
    let out = parse_map_output(truncated, &labels, &source);
    assert_eq!(out.actions.len(), 1, "the closed item survives");
    assert_eq!(out.actions[0].segment, "seg_0001");
    assert_eq!(out.narrative, "The team reviewed the release plan.");

    // (b) Cut mid-narrative, before any list exists at all.
    let out = parse_map_output(
        r#"{"narrative":"The team reviewed the release plan. Then they"#,
        &labels,
        &source,
    );
    assert_eq!(
        out.narrative, "The team reviewed the release plan.",
        "an unterminated narrative keeps only what closed at a sentence"
    );
    assert!(out.actions.is_empty());

    // (c) A fence the grammar forbids, from a path where the grammar did not
    // apply (an older sidecar, a grammar this build refused).
    let fenced = format!(
        "```json\n{}\n```",
        map_answer(
            "Short recap of the release call.",
            &[("Move the onboarding review", "seg_0001")],
            &[],
            &[],
        )
    );
    let out = parse_map_output(&fenced, &labels, &source);
    assert_eq!(out.actions.len(), 1);

    // (d) Shapes that carry nothing usable must be empty answers, never panics.
    for junk in [
        "",
        "   ",
        "I'm sorry, I can't help with that.",
        "{",
        "}}}}",
        r#"{"narrative":42,"actions":"everything"}"#,
        r#"{"actions":[{"text":"no id here"}]}"#,
        r#"{"actions":[{"segment":"seg_0001"}]}"#,
        "\u{0}\u{1}\u{2}",
    ] {
        let out = parse_map_output(junk, &labels, &source);
        assert!(
            out.actions.is_empty() && out.decisions.is_empty() && out.questions.is_empty(),
            "junk must yield no items: {junk:?}"
        );
    }

    // (e) An item whose own text contains braces and the word "actions" must not
    // confuse the recovery scan.
    let tricky = r#"{"narrative":"n","actions":[{"text":"Fix the {actions} counter","segment":"seg_0002"}],"decisions":[],"questions":[]}"#;
    let out = parse_map_output(tricky, &labels, &source);
    assert_eq!(out.actions.len(), 1);
    assert_eq!(out.actions[0].text, "Fix the {actions} counter");
    assert!(out.decisions.is_empty() && out.questions.is_empty());
}
