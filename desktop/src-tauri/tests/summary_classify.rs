//! #250 round 2, defect 3 — the real 1.5B run filed a question and a
//! decision under Action items, skipped plainly-said lines, and swapped two
//! citations. A post-parse classifier re-buckets each line by its own words,
//! and `summarize::ground_extract` re-cites and backfills from the chunk's own lines.

mod support;

use support::{labels_in, segments_from, StubModel};
use wilson_voice_lib::polish_protocol::PolishRequest;
use wilson_voice_lib::summarize::{classify_item, clear_cancel, summarize_segments, Bucket};

const FIXTURE: &[&str] = &[
    "Okay, this is the pricing review for the Drivia consulting tier.",
    "We agreed the starter package stays at forty nine dollars a month.",
    "Jeisil will send the updated pricing doc to the team by Friday.",
    "Wilson to book the demo with Liberty University next week.",
    "The open question is whether annual billing gets a discount.",
    "We also decided to drop the free trial for enterprise accounts.",
];

/// The real-model MAP answer's bucketing from the PR #250 run, verbatim: the
/// question and the "decided to drop" line came back as ACTIONS.
fn misfiled_map(req: &PolishRequest) -> String {
    let ids = labels_in(&req.text);
    let id = |i: usize| ids.get(i).cloned().unwrap_or_else(|| ids[0].clone());
    format!(
        r#"{{"narrative":"The team reviewed pricing for the Drivia consulting tier.","actions":[{{"text":"Jeisil will send the updated pricing doc to the team by Friday","segment":"{}"}},{{"text":"Wilson to book the demo with Liberty University next week","segment":"{}"}},{{"text":"The open question is whether annual billing gets a discount","segment":"{}"}},{{"text":"We also decided to drop the free trial for enterprise accounts","segment":"{}"}}],"decisions":[{{"text":"The starter package will remain at forty nine dollars a month","segment":"{}"}}],"questions":[]}}"#,
        id(2),
        id(3),
        id(4),
        id(5),
        id(1)
    )
}

fn reduce_or_headline(req: &PolishRequest) -> Option<String> {
    match req.mode.as_str() {
        "reduce" => Some("The team reviewed pricing for the Drivia consulting tier.".into()),
        "headline" => Some(r#"{"title":"Drivia consulting pricing review","key_points":["The starter package stays at forty nine dollars a month.","The free trial is dropped for enterprise accounts.","Jeisil will send the updated pricing doc by Friday."]}"#.into()),
        _ => None,
    }
}

const NOTE: &str = "did not fit the model";

// ── defect 3: questions and decisions out of Action items ───────────────────

#[test]
fn the_real_runs_misfiled_lines_are_reclassified() {
    use Bucket::*;
    for (from, text, want) in [
        (Actions, "Jeisil will send the updated pricing doc to the team by Friday", Some(Actions)),
        (Actions, "Wilson to book the demo with Liberty University next week", Some(Actions)),
        (Actions, "The open question is whether annual billing gets a discount", Some(Questions)),
        (Actions, "We also decided to drop the free trial for enterprise accounts", Some(Decisions)),
        (Decisions, "The starter package will remain at forty-nine dollars a month", Some(Decisions)),
        (Actions, "We agreed the starter package stays at forty nine dollars a month", Some(Decisions)),
        // Owner + verb outranks a question word later in the line.
        (Questions, "Jeisil will check whether annual billing gets a discount", Some(Actions)),
        (Actions, "Send the deck to the team", Some(Actions)),
        (Actions, "I'll draft the renewal email", Some(Actions)),
        (Decisions, "Should we offer annual billing", Some(Questions)),
        (Actions, "Does annual billing get a discount?", Some(Questions)),
        // An "action item" with no commitment at all is not one.
        (Actions, "Pricing for the Drivia consulting tier", None),
        (Decisions, "Drop the free trial for enterprise accounts", Some(Decisions)),
    ] {
        assert_eq!(classify_item(from, text), want, "{text}");
    }
}

#[test]
fn the_fixture_notes_file_each_line_under_the_right_heading() {
    clear_cancel();
    let model = StubModel::with_headline(|req| {
        Ok(reduce_or_headline(req).unwrap_or_else(|| misfiled_map(req)))
    });
    let s = summarize_segments(&segments_from(FIXTURE), &model).expect("a summary");
    let md = &s.markdown;
    println!("{md}");
    let section = |h: &str| -> String {
        md.split(&format!("{h}\n\n"))
            .nth(1)
            .unwrap_or_else(|| panic!("no {h} in {md}"))
            .split("\n\n")
            .next()
            .unwrap()
            .to_string()
    };
    let actions = section("### Action items");
    assert!(actions.contains("Jeisil will send"), "{actions}");
    assert!(actions.contains("Wilson to book"), "{actions}");
    assert!(!actions.contains("open question"), "{actions}");
    assert!(!actions.contains("decided"), "{actions}");
    let decisions = section("### Decisions");
    assert!(decisions.contains("decided to drop the free trial"), "{decisions}");
    assert!(decisions.contains("will remain at forty nine"), "{decisions}");
    let questions = section("### Open questions");
    assert!(questions.contains("whether annual billing"), "{questions}");
    assert!(!md.contains(NOTE), "{md}");
    let headings: Vec<&str> = md.lines().filter(|l| l.starts_with('#')).collect();
    assert_eq!(
        headings,
        [
            "## Drivia consulting pricing review",
            "### Summary",
            "### Key points",
            "### Action items",
            "### Decisions",
            "### Open questions",
        ]
    );
}

/// The real-model MAP answer from round 2's tuned prompt, verbatim apart from
/// the ids: it skipped Jeisil's commitment and the starter-price decision, and
/// cited the question's line for the free-trial decision and vice versa.
fn round2_real_map(req: &PolishRequest) -> String {
    let ids = labels_in(&req.text);
    format!(
        r#"{{"narrative":"The pricing review for the Drivia consulting tier is complete.","actions":[],"decisions":[{{"text":"Wilson will book a demo with Liberty University next week.","segment":"{}"}},{{"text":"The free trial for enterprise accounts will be dropped.","segment":"{}"}}],"questions":[{{"text":"The open question is whether annual billing gets a discount.","segment":"{}"}}]}}"#,
        ids[3], ids[4], ids[5]
    )
}

#[test]
fn skipped_lines_are_backfilled_and_swapped_citations_corrected() {
    clear_cancel();
    let model = StubModel::with_headline(|req| {
        Ok(reduce_or_headline(req).unwrap_or_else(|| round2_real_map(req)))
    });
    let s = summarize_segments(&segments_from(FIXTURE), &model).expect("a summary");
    println!("{}", s.markdown);
    let texts = |items: &[wilson_voice_lib::summarize::SummaryItem]| -> Vec<String> {
        items.iter().map(|i| i.text.clone()).collect()
    };
    // Actions: both commitments, in meeting order — Jeisil's backfilled verbatim.
    let actions = texts(&s.actions);
    assert_eq!(actions.len(), 2, "{actions:?}");
    assert!(actions[0].starts_with("Jeisil will send"), "{actions:?}");
    assert!(actions[1].starts_with("Wilson will book"), "{actions:?}");
    // Decisions: the starter price (backfilled) and the free trial.
    let decisions = texts(&s.decisions);
    assert_eq!(decisions.len(), 2, "{decisions:?}");
    assert!(decisions[0].contains("starter package"), "{decisions:?}");
    assert!(decisions[1].contains("free trial"), "{decisions:?}");
    // Citations: the free-trial decision is the 6th line (20 s), the question
    // the 5th (16 s) — the model had them swapped.
    let free_trial = s.decisions.iter().find(|d| d.text.contains("free trial")).unwrap();
    let question = &s.questions[0];
    assert!(free_trial.start_seconds > question.start_seconds, "{free_trial:?} {question:?}");
    assert_eq!(s.questions.len(), 1);
}

#[test]
fn backfill_never_adds_a_line_without_a_clear_marker() {
    clear_cancel();
    let quiet: &[&str] = &[
        "I will say the numbers looked fine this quarter.",
        "Pricing came up again for the consulting tier.",
    ];
    let model = StubModel::with_headline(|req| {
        Ok(reduce_or_headline(req).unwrap_or_else(|| {
            r#"{"narrative":"Pricing came up again for the consulting tier.","actions":[],"decisions":[],"questions":[]}"#.to_string()
        }))
    });
    let s = summarize_segments(&segments_from(quiet), &model).expect("a summary");
    assert!(s.actions.is_empty() && s.decisions.is_empty() && s.questions.is_empty(), "{s:?}");
}
