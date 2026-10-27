//! #250 round 2, defect 3 — the real 1.5B run filed a question and a
//! decision under Action items, skipped plainly-said lines, and swapped two
//! citations. A post-parse classifier re-buckets each line by its own words,
//! and `summarize::ground_extract` re-cites and backfills from the chunk's own lines.

mod support;

use support::{labels_in, segments_from, StubModel};
use wilson_voice_lib::polish_protocol::PolishRequest;
use wilson_voice_lib::summarize::{classify_item, summarize_segments, Bucket};

const FIXTURE: &[&str] = &[
    "Okay, this is the pricing review for the Drivia consulting tier.",
    "We agreed the starter package stays at forty nine dollars a month.",
    "Jeisil will send the updated pricing doc to the team by Friday.",
    "Wilson to book the demo with Liberty University next week.",
    "The open question is whether annual billing gets a discount.",
    "We also decided to drop the free trial for enterprise accounts.",
    // R2-B2: a bold / backfilled owner must be a name the meeting uses
    // mid-sentence (twice), as a real meeting does.
    "Thanks Jeisil, and thanks Wilson for pulling the numbers together.",
    "I'll sync with Jeisil and Wilson after the call.",
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
        (Actions, "The pricing review for the Drivia consulting tier is complete", None),
        (Actions, "Move the onboarding review", Some(Actions)),
        (Decisions, "Drop the free trial for enterprise accounts", Some(Decisions)),
    ] {
        assert_eq!(classify_item(from, text), want, "{text}");
    }
}

#[test]
fn the_fixture_notes_file_each_line_under_the_right_heading() {
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

// ── fix round 1 (review BLOCKING): a pleasantry is never a Decision ─────────

/// The Decisions (and Questions) arm used to keep any line the model filed
/// there with no marker at all, so "Thanks Jeisil, …" rendered under
/// ### Decisions. Small talk is dropped from every list; a real decision with
/// a marker — even one that opens with a pleasantry — still stands.
#[test]
fn a_pleasantry_the_model_files_as_a_decision_is_dropped() {
    use Bucket::*;
    for text in [
        "Thanks Jeisil, and thanks Wilson for pulling the numbers together.",
        "Great to have everyone here today.",
        "Nice to meet you all.",
        "Thank you all for joining.",
        "Good to see everyone again.",
        "Appreciate everyone making the time.",
    ] {
        assert_eq!(classify_item(Decisions, text), None, "{text}");
        assert_eq!(classify_item(Questions, text), None, "{text}");
    }
    // The reviewer's three probe lines, in every bucket the model may use.
    for text in [
        "Thanks Jeisil, and thanks Wilson for pulling the numbers together.",
        "Great to have everyone here today.",
        "Nice to meet you all.",
    ] {
        assert_eq!(classify_item(Actions, text), None, "{text}");
    }
    for (text, want) in [
        ("Thanks everyone, we decided to drop the free trial.", Some(Decisions)),
        ("The starter package will remain at forty-nine dollars a month", Some(Decisions)),
        ("Drop the free trial for enterprise accounts", Some(Decisions)),
        ("Annual billing moves to the new contract template", Some(Decisions)),
    ] {
        assert_eq!(classify_item(Decisions, text), want, "{text}");
    }
}

// ── fix round 2 (review BLOCKING): a pleasantry WORD is not a pleasantry ────

/// Fix round 1 dropped any unmarked Decision or Question whose FIRST word was
/// on the opener list, so a proper noun or a compound that merely starts with
/// "Good", "Great", "Morning", "Happy", "Welcome" or "Cool" vanished from the
/// notes. An ambiguous opener is small talk only when every word after it is
/// an addressee or filler word ("Good morning everyone", "Great, thanks all").
#[test]
fn a_real_line_that_opens_with_a_pleasantry_word_is_kept() {
    use Bucket::*;
    for text in [
        // The reviewers' probes.
        "Good Shepherd Health is the pilot customer.",
        "Great Lakes office closes in June.",
        "Morning standups move to Tuesdays.",
        "Morning standup moves to 9am",
        // Same shape, the rest of the ambiguous openers.
        "Happy Valley Clinic signs the annual contract.",
        "Welcome packet ships with every new account.",
        "Cool Springs is the second pilot site.",
        "Nice France office joins the rollout in May.",
        "Glad Tidings Church is on the starter tier.",
        "Lovely Lane Bakery gets the nonprofit discount.",
        "Evening classes switch to the recorded format.",
        "Afternoon office hours run through December.",
        "Awesome Inc is the reseller for Canada.",
        "Thanks to the new template, annual billing moves to net thirty.",
        "Morning standups.",
        "Great Lakes.",
    ] {
        assert_eq!(classify_item(Decisions, text), Some(Decisions), "{text}");
        assert!(classify_item(Questions, text).is_some(), "{text}");
    }
    // Small talk built from the same words is still dropped.
    for text in [
        "Good morning everyone.",
        "Morning all!",
        "Great, thanks all.",
        "Happy Friday everyone.",
        "Cool, cool.",
        "Welcome back, everyone.",
        "Sorry I'm late.",
        "Good afternoon, team.",
        "Hi everyone, hello Jeisil.",
        "Have a great weekend.",
        "Good morning, Jeisil.",
    ] {
        assert_eq!(classify_item(Decisions, text), None, "{text}");
        assert_eq!(classify_item(Questions, text), None, "{text}");
    }
}

// ── fix round 3 (both reviewers BLOCKING on 0baec25) ─────────────────────────

/// Both seats probed `classify_item` on 0baec25 with lines that open with (or
/// contain) a greeting and still carry substance; every one came back `None`
/// from the Decisions and the Questions bucket. Small talk is now a line in
/// which EVERY clause is a greeting, thanks or goodbye.
#[test]
fn a_line_with_substance_next_to_a_greeting_is_kept() {
    use Bucket::*;
    for text in [
        // review-opus, 0baec25
        "Hey team, annual billing moves to the new contract template.",
        "Hey, the pilot customer is Good Shepherd Health.",
        "The pilot moves to May, thanks for flagging it Jeisil.",
        "The thank you emails go out on Monday.",
        "Thanks to everyone's feedback the starter tier keeps the free trial.",
        "Hello Fresh is the pilot customer.",
        "Hi-Fi Audio is the second pilot site.",
        "Cheers Brewing is the venue for the launch.",
        "Thank-you cards go out with every renewal.",
        "Bye-laws now require two signatures on contracts.",
        // review-opus2, 0baec25
        "Cool, Friday it is.",
        "Great, Monday it is.",
        "Good, so we're on board.",
        "Thanks everyone, the launch moves to May.",
        "Thanks for waiting, the launch moves to May.",
        "Appreciate everyone's patience, the launch moves to May.",
        "Hey, the launch moves to May.",
        "Hi-fi mockups are due Friday.",
        "Thank-you emails go out to donors on Monday.",
        "The checkout flow gets a thank you page.",
        "Hello Fresh renews the sponsorship for Q3.",
        "Morning it is.",
        // the control both seats used
        "Sorry, the launch moves to May.",
    ] {
        assert!(classify_item(Decisions, text).is_some(), "Decisions dropped: {text}");
        assert!(classify_item(Questions, text).is_some(), "Questions dropped: {text}");
    }
}

/// The other side of the rule: small talk built from the same words, and the
/// courtesy lines review-opus listed as advisory under Action items, is still
/// nobody's decision, question or task.
#[test]
fn small_talk_is_dropped_from_every_bucket() {
    use Bucket::*;
    for text in [
        "Thanks Jeisil, and thanks Wilson for pulling the numbers together.",
        "Thank you all for joining.",
        "Thanks to everyone for coming.",
        "Thanks to all of you.",
        "Appreciate everyone making the time.",
        "Cheers everyone, see you all next week.",
        "Congrats to Jeisil on the launch.",
        "Good morning, Jeisil.",
        "Happy Friday everyone.",
        "Hi everyone, hello Jeisil.",
        "Have a great weekend.",
    ] {
        for from in [Actions, Decisions, Questions] {
            assert_eq!(classify_item(from, text), None, "{from:?}: {text}");
        }
    }
}

/// review-opus2's end-to-end probe: the MAP filed "Thanks everyone, the launch
/// moves to May" under decisions, and the stored notes had no "May" at all.
#[test]
fn a_decision_after_a_thank_you_reaches_the_stored_notes() {
    let lines: &[&str] = &[
        "Okay, quick sync on the launch for the Drivia consulting tier.",
        "Thanks everyone, the launch moves to May.",
        "Jeisil will send the updated launch plan by Friday.",
    ];
    let model = StubModel::with_headline(|req| {
        let ids = labels_in(&req.text);
        Ok(match req.mode.as_str() {
            "reduce" => "The team synced on the launch for the Drivia consulting tier.".into(),
            "headline" => r#"{"title":"Drivia consulting launch sync","key_points":["The launch moves to May.","Jeisil will send the updated launch plan by Friday.","The team synced on the launch."]}"#.into(),
            _ => format!(
                r#"{{"narrative":"The team synced on the launch for the Drivia consulting tier.","actions":[{{"text":"Jeisil will send the updated launch plan by Friday","segment":"{}"}}],"decisions":[{{"text":"Thanks everyone, the launch moves to May","segment":"{}"}}],"questions":[]}}"#,
                ids[2], ids[1]
            ),
        })
    });
    let s = summarize_segments(&segments_from(lines), &model).expect("a summary");
    println!("{}", s.markdown);
    assert!(
        s.decisions.iter().any(|d| d.text.contains("launch moves to May")),
        "{:?}",
        s.decisions
    );
    let decisions = s.markdown.split("### Decisions").nth(1).expect("a Decisions section");
    assert!(decisions.contains("May"), "{}", s.markdown);
}

// ── fix round 4 (review BLOCKING on cb35adf) ─────────────────────────────────

/// review-opus on cb35adf: these lines were KEPT on 0baec25 and DROPPED on
/// cb35adf — "appreciate" decided on the first word, a gratitude opener that
/// reached "for" / "on" dropped whatever reason followed, and "next" was
/// filler. Each is kept in the bucket(s) the reviewer named.
#[test]
fn a_decision_inside_gratitude_or_next_week_is_kept() {
    use Bucket::*;
    for (text, buckets) in [
        ("Appreciate the team pushing the launch to May.", &[Decisions, Actions][..]),
        ("Appreciate Jeisil moving the demo to Thursday.", &[Decisions, Actions][..]),
        ("Kudos on shipping the pilot two weeks early.", &[Decisions, Actions][..]),
        ("Good for next week.", &[Decisions][..]),
        ("Great, a call next week.", &[Decisions][..]),
        ("Congrats on closing Liberty University for the fall pilot.", &[Actions][..]),
    ] {
        for &from in buckets {
            assert!(classify_item(from, text).is_some(), "{from:?} dropped: {text}");
        }
    }
    // The courtesy lines built from the same openers are still small talk.
    for text in [
        "Appreciate everyone making the time.",
        "Appreciate it.",
        "Appreciate you all.",
        "Kudos everyone.",
        "Kudos on the launch.",
        "Congrats to Jeisil on the launch.",
        "Thanks Jeisil, and thanks Wilson for pulling the numbers together.",
        "Thank you all for joining.",
        "Cheers everyone, see you all next week.",
        "See you next week.",
    ] {
        for from in [Actions, Decisions, Questions] {
            assert_eq!(classify_item(from, text), None, "{from:?}: {text}");
        }
    }
}

/// The reviewer's end-to-end probe: the MAP files "Appreciate the team pushing
/// the launch to May" under decisions and the headline returns three key
/// points that do not carry it, so no backfill can rescue it. On cb35adf the
/// stored notes had no Decisions section.
#[test]
fn an_appreciate_decision_reaches_the_stored_notes_without_a_backfill() {
    let lines: &[&str] = &[
        "Okay, quick sync on the launch for the Drivia consulting tier.",
        "Appreciate the team pushing the launch to May.",
        "Jeisil will send the updated launch plan by Friday.",
    ];
    let model = StubModel::with_headline(|req| {
        let ids = labels_in(&req.text);
        Ok(match req.mode.as_str() {
            "reduce" => "The team synced on the launch for the Drivia consulting tier.".into(),
            "headline" => r#"{"title":"Drivia consulting launch sync","key_points":["Jeisil will send the updated launch plan by Friday.","The team synced on the launch.","The consulting tier was discussed."]}"#.into(),
            _ => format!(
                r#"{{"narrative":"The team synced on the launch for the Drivia consulting tier.","actions":[{{"text":"Jeisil will send the updated launch plan by Friday","segment":"{}"}}],"decisions":[{{"text":"Appreciate the team pushing the launch to May","segment":"{}"}}],"questions":[]}}"#,
                ids[2], ids[1]
            ),
        })
    });
    let s = summarize_segments(&segments_from(lines), &model).expect("a summary");
    println!("{}", s.markdown);
    assert!(
        s.decisions.iter().any(|d| d.text.contains("launch to May")),
        "{:?}",
        s.decisions
    );
    let decisions = s.markdown.split("### Decisions").nth(1).expect("a Decisions section");
    assert!(decisions.contains("May"), "{}", s.markdown);
}

// ── fix round 5 (review BLOCKING on f8fb110) ─────────────────────────────────

/// review-opus on f8fb110: the reason after a thank-you was small talk unless
/// it named a date, a name, a number or a destination — a short list of what
/// counts as substance, so a lowercase decision wrapped in gratitude was
/// dropped from Decisions and no backfill could restore it (no marker). The
/// check now fails closed: the reason is small talk only when EVERY word is
/// on the courtesy list (joining, coming, the time, help, hosting, …).
#[test]
fn a_gratitude_wrapped_decision_with_no_date_or_name_is_kept() {
    use Bucket::*;
    for text in [
        // review-opus, f8fb110 — each returned None from Decisions.
        "Thanks for approving the budget increase.",
        "Thanks everyone for agreeing on the cheaper vendor.",
        "Appreciate the team dropping the enterprise trial.",
        "Thanks for pushing the launch back a week.",
        "Thanks all for choosing the lower price tier.",
        "Thanks for cancelling the offsite.",
        // review-opus2 (advisory A4), f8fb110 — same class.
        "Thanks for signing off on the budget.",
        "Thanks for approving the new hire.",
        // the kept control from round 4
        "Appreciate the team pushing the launch to May.",
        // review-opus2 B1 on b3d048c — the round-5 allowlist spelled these
        // from its schedule words and two-sense verbs; each returned None.
        "Thanks for setting the launch back a week.",
        "Thanks for pulling the launch back a week.",
        "Thanks for pulling the launch.",
        "Thanks for making the call to have the launch today.",
        "Thanks for setting the demo for today.",
        "Thanks for putting the launch back a day.",
        "Thanks everyone for taking the launch back a week.",
        "Appreciate the team pulling the launch.",
        // review-opus advisory on b3d048c — a cancellation phrased as thanks.
        "Thanks Jeisil for pulling the demo.",
        "Thank you for pulling it.",
        "Thanks for taking the launch.",
    ] {
        assert_eq!(classify_item(Decisions, text), Some(Decisions), "Decisions dropped: {text}");
    }
    // Courtesy reasons are still small talk in every bucket.
    for text in [
        "Thanks for the help.",
        "Thanks for hosting the call.",
        "Thanks everyone for making the time.",
        "Thanks for your patience.",
        "Thanks again for coming.",
        "Thanks for the update.",
        "Appreciate your help.",
        "Congrats on the launch.",
        // the two-sense verbs with their courtesy completion stay small talk
        "Thanks Wilson for pulling the numbers together.",
        "Thanks for taking the time.",
        "Thanks for taking notes.",
        "Thanks for setting this up.",
        "Thanks for putting this together.",
    ] {
        for from in [Actions, Decisions, Questions] {
            assert_eq!(classify_item(from, text), None, "{from:?}: {text}");
        }
    }
}
