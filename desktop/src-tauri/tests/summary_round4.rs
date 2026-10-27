//! #250 round 4 — review-opus round 2's blocking findings, as fixtures.
//!
//! R2-B1: key points come FIRST, from the transcript — 3–6 distinct facts —
//!        and the Summary paragraph is written from them.
//! R2-B2: an owner is a name the meeting uses; small talk is never a task.
//! R2-B3: a true status sentence survives; prices and decimals are intact.

mod support;

use support::{labels_in, segments_from, StubModel};
use wilson_voice_lib::polish_protocol::PolishRequest;
use wilson_voice_lib::summarize::{
    action_owner, classify_item, ground_extract, point_overlap, summarize_segments,
    split_sentences, transcript_lines, transcript_names, validate_narrative, Bucket, MapExtract,
    SummaryError,
};

const FIXTURE: &[&str] = &[
    "Okay, this is the pricing review for the Drivia consulting tier.",
    "We agreed the starter package stays at forty nine dollars a month.",
    "Jeisil will send the updated pricing doc to the team by Friday.",
    "Wilson to book the demo with Liberty University next week.",
    "The open question is whether annual billing gets a discount.",
    "We also decided to drop the free trial for enterprise accounts.",
];

fn map_answer(req: &PolishRequest) -> String {
    let ids = labels_in(&req.text);
    let id = |i: usize| ids.get(i).cloned().unwrap_or_else(|| ids[0].clone());
    format!(
        r#"{{"narrative":"The team reviewed pricing for the Drivia consulting tier.","actions":[{{"text":"Jeisil will send the updated pricing doc to the team by Friday.","segment":"{}"}}],"decisions":[{{"text":"The starter package stays at forty nine dollars a month.","segment":"{}"}}],"questions":[]}}"#,
        id(2),
        id(1)
    )
}

fn key_points(md: &str) -> Vec<String> {
    md.split("### Key points\n\n")
        .nth(1)
        .map(|kp| {
            kp.lines()
                .take_while(|l| l.starts_with("- "))
                .map(|l| l.trim_start_matches("- ").to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn assert_distinct(points: &[String]) {
    for (i, a) in points.iter().enumerate() {
        assert!(!a.trim().is_empty(), "an empty key point");
        for b in &points[i + 1..] {
            assert!(point_overlap(a, b) < 0.6, "not distinct: {a:?} ~ {b:?}");
        }
    }
}

/// The round-3 real run's shape: the model's points are the Summary's own
/// sentences, one of them the action item. All kept (they are distinct
/// facts), and the Summary paragraph is the one written from them.
#[test]
fn round3_shape_keeps_three_or_more_distinct_points_and_the_summary_comes_from_them() {
    let model = StubModel::with_headline(|req| {
        Ok(match req.mode.as_str() {
            "map" => map_answer(req),
            "reduce" => "The team reviewed pricing.".to_string(),
            "headline" => r#"{"title":"Drivia consulting pricing review","key_points":["The starter package stays at forty nine dollars a month.","Jeisil will send the updated pricing doc to the team by Friday.","Wilson will book the demo with Liberty University next week.","The starter package stays at forty nine dollars a month."],"summary":"The starter package stays at forty nine dollars a month. Jeisil will send the updated pricing doc by Friday and Wilson will book the demo with Liberty University next week."}"#.to_string(),
            other => panic!("unexpected stage {other}"),
        })
    });
    let s = summarize_segments(&segments_from(FIXTURE), &model).expect("a summary");
    let points = key_points(&s.markdown);
    assert!(points.len() >= 3, "{}", s.markdown);
    assert_distinct(&points);
    // A point that is also an action item stays.
    assert!(points.iter().any(|p| p.starts_with("Jeisil will send")), "{points:?}");
    assert!(
        s.narrative.starts_with("The starter package stays at forty nine dollars a month."),
        "the Summary is written from the key points: {}",
        s.narrative
    );
    // Key points first, from the TRANSCRIPT — not from a finished Summary.
    let h = model.stage("headline");
    assert_eq!(h.len(), 1);
    assert!(h[0].text.contains("Okay, this is the pricing review"), "{}", h[0].text);
    assert!(!h[0].text.contains("seg_"), "ids are not facts: {}", h[0].text);
    assert!(model.stage("reduce").is_empty(), "no REDUCE once the summary held up");
}

/// The model returns one fact three times: backfill tops the list up from
/// the transcript's most informative sentences, verbatim.
#[test]
fn fewer_than_three_distinct_points_are_backfilled_from_the_transcript() {
    let model = StubModel::with_headline(|req| {
        Ok(match req.mode.as_str() {
            "map" => map_answer(req),
            "reduce" => "The team reviewed pricing for the Drivia consulting tier.".to_string(),
            "headline" => r#"{"title":"Pricing review","key_points":["The starter package stays at forty nine dollars a month.","The starter package stays at forty nine dollars.","Starter package: forty nine dollars a month."],"summary":""}"#.to_string(),
            other => panic!("unexpected stage {other}"),
        })
    });
    let s = summarize_segments(&segments_from(FIXTURE), &model).expect("a summary");
    let points = key_points(&s.markdown);
    assert!(points.len() >= 3, "{}", s.markdown);
    assert_distinct(&points);
    for p in &points[1..] {
        assert!(FIXTURE.contains(&p.as_str()), "backfill is verbatim transcript: {p}");
    }
    // No paragraph from the model → the MAP/REDUCE narrative stands in.
    assert_eq!(s.narrative, "The team reviewed pricing for the Drivia consulting tier.");
}

/// A failed HEADLINE call still yields ≥ 3 distinct points.
#[test]
fn a_failed_headline_call_still_backfills_three_points() {
    let model = StubModel::with_headline(|req| match req.mode.as_str() {
        "map" => Ok(map_answer(req)),
        "headline" => Err(SummaryError::Protocol),
        _ => Ok("The team reviewed pricing.".to_string()),
    });
    let s = summarize_segments(&segments_from(FIXTURE), &model).expect("a summary");
    let points = key_points(&s.markdown);
    assert!(points.len() >= 3, "{}", s.markdown);
    assert_distinct(&points);
}

/// The reviewer's seven probe lines (round 2): every one was backfilled as an
/// action item with a bold "owner" (Everything, Monday, Traffic, …).
const NOT_OWNERS: &[&str] = &[
    "Everything will be fine",
    "Monday will be busy for everyone",
    "Traffic should be lighter after six",
    "Something should change eventually",
    "Revenue will grow next quarter hopefully",
    "Lunch should be here soon",
    "Coffee will help",
];

#[test]
fn small_talk_with_will_or_should_never_becomes_an_owned_action() {
    let segments = segments_from(NOT_OWNERS);
    let lines = transcript_lines(&segments);
    let source = lines.iter().map(|l| l.render()).collect::<Vec<_>>().join("\n");
    let names = transcript_names(&lines);
    assert!(names.is_empty(), "no names in small talk: {names:?}");
    let mut extract = MapExtract::default();
    ground_extract(&mut extract, &lines, &source);
    assert!(extract.actions.is_empty(), "backfilled small talk: {:?}", extract.actions);
    for line in NOT_OWNERS {
        assert_eq!(action_owner(line, &names), None, "fake owner for {line}");
        // Even with every word on the name list, a common word is no owner.
        let everyone: Vec<String> =
            line.split_whitespace().map(str::to_string).collect();
        assert_eq!(action_owner(line, &everyone), None, "common word as owner: {line}");
        assert_ne!(classify_item(Bucket::Actions, line), Some(Bucket::Actions), "{line}");
    }
}

#[test]
fn a_real_owner_is_a_name_the_meeting_uses() {
    let segments = segments_from(&[
        "Jeisil will send the pricing doc by Friday.",
        "Coffee will help.",
        "Thanks Jeisil, and Priya too.",
        "I'll check with Jeisil after lunch.",
        "Priya will draft the follow up.",
    ]);
    let lines = transcript_lines(&segments);
    let source = lines.iter().map(|l| l.render()).collect::<Vec<_>>().join("\n");
    let names = transcript_names(&lines);
    assert_eq!(names, vec!["Jeisil".to_string()], "Priya is said once mid-sentence");
    let mut extract = MapExtract::default();
    ground_extract(&mut extract, &lines, &source);
    let texts: Vec<&str> = extract.actions.iter().map(|a| a.text.as_str()).collect();
    assert_eq!(texts, vec!["Jeisil will send the pricing doc by Friday"]);
    assert_eq!(action_owner(texts[0], &names), Some("Jeisil".into()));
    assert_eq!(action_owner("Priya will draft the follow up", &names), None);
}

#[test]
fn a_status_the_meeting_stated_is_kept_and_prices_stay_whole() {
    let source = "seg_0001: We finished the migration yesterday.\nseg_0002: We're done with the import too.\nseg_0003: Jeisil will send the pricing doc by Friday at $49.99 a seat.";
    let narrative = "The migration is finished. The import is done. Jeisil will send the pricing doc by Friday at $49.99 a seat.";
    assert_eq!(validate_narrative(source, narrative).as_deref(), Some(narrative));
    // One true sentence is a summary, never a rejection.
    let one = "seg_0001: We finished the migration yesterday.";
    assert_eq!(
        validate_narrative(one, "The migration is finished.").as_deref(),
        Some("The migration is finished.")
    );
    // Still dropped: a status nobody stated, next to a price that stays whole.
    let src = "seg_0001: Okay, this is the pricing review.\nseg_0002: Dr. Britt said the seat price is $49.99, e.g. for the starter tier, from 3.30pm today.";
    let got = validate_narrative(
        src,
        "The pricing review is complete. Dr. Britt said the seat price is $49.99, e.g. for the starter tier, from 3.30pm today.",
    );
    assert_eq!(
        got.as_deref(),
        Some("Dr. Britt said the seat price is $49.99, e.g. for the starter tier, from 3.30pm today.")
    );
}

#[test]
fn sentences_split_only_at_real_ends() {
    assert_eq!(
        split_sentences("It costs $49.99 a seat. Dr. Britt agreed, e.g. for Liberty. We meet at 3.30pm. Then 9 a.m. tomorrow works. Done! Really?"),
        vec![
            "It costs $49.99 a seat.",
            "Dr. Britt agreed, e.g. for Liberty.",
            "We meet at 3.30pm.",
            "Then 9 a.m. tomorrow works.",
            "Done!",
            "Really?",
        ]
    );
    assert_eq!(split_sentences("Version 1.2.3 ships"), vec!["Version 1.2.3 ships"]);
}

/// review-opus2 N1's probe set: none is an owner, none is backfilled, none
/// classifies as an action.
#[test]
fn review_opus2_n1_words_are_never_owners() {
    let lines_in = [
        "Everything will be ready",
        "Friday will be tight",
        "Lunch will be late",
        "Pricing will change next quarter",
        "Monday should be quiet",
        "Nobody will mind",
        "Revenue is going to climb",
    ];
    let lines = transcript_lines(&segments_from(&lines_in));
    let source = lines.iter().map(|l| l.render()).collect::<Vec<_>>().join("\n");
    let names = transcript_names(&lines);
    let mut extract = MapExtract::default();
    ground_extract(&mut extract, &lines, &source);
    assert!(extract.actions.is_empty(), "{:?}", extract.actions);
    for line in lines_in {
        let first = vec![line.split_whitespace().next().unwrap().to_string()];
        assert_eq!(action_owner(line, &names), None, "{line}");
        assert_eq!(action_owner(line, &first), None, "{line}");
        assert_ne!(classify_item(Bucket::Actions, line), Some(Bucket::Actions), "{line}");
    }
}

#[test]
fn key_points_hide_when_fewer_than_two() {
    use wilson_voice_lib::summarize::{render_notes, Headline};
    let one = Headline { title: "T".into(), key_points: vec!["Only one point here.".into()], summary: String::new() };
    let md = render_notes(&one, &[], "A summary.", &[], &[], &[], 0, 1, false);
    assert!(!md.contains("### Key points"), "{md}");
    let two = Headline { key_points: vec!["One point here.".into(), "Another fact entirely.".into()], ..one };
    let md = render_notes(&two, &[], "A summary.", &[], &[], &[], 0, 1, false);
    assert!(md.contains("### Key points"), "{md}");
}
