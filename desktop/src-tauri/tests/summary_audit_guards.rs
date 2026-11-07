//! yap-audit-notes-guards — notes never ship a re-transcript or a wrong owner.
//!
//! Synthetic transcripts (no real meeting text) reproducing each failure the
//! 2026-10-26 meeting audit found on PR 250's notes, one test per guard, plus
//! a clean planted fixture that must still yield its planted actions.

mod support;

use support::{labels_in, segments_from, StubModel};
use wilson_voice_lib::meeting_echo::word_trigram_containment;
use wilson_voice_lib::meetings::{MeetingSegment, MIC_TRACK};
use wilson_voice_lib::summarize::{
    action_has_commitment, apply_item_guards, attribute_owners, backfill_key_points,
    decision_has_marker, echo_owner, even_shrink, fallback_title, headline_source,
    is_verbatim_narrative, mic_is_single_voice, narrative_containment, question_is_open,
    strip_speaker_tag, summarize_segments, transcript_lines, validate_narrative, SegmentSpeaker,
    SummaryError, SummaryItem, TokenCounter, TrackOwner, TranscriptLine, MAX_ITEMS_PER_SEGMENT,
    MIC_SHARED_OWNER, NO_SUMMARY_VERBATIM, VERBATIM_NARRATIVE_MAX,
};

const FIXTURE: &[&str] = &[
    "Okay so this is the contract review for the Liberty University pilot.",
    "Jeisil will send the signed contract to Liberty by Friday.",
    "Wilson to book the onboarding call with the Liberty team next week.",
    "We decided the pilot starts with two hundred students.",
    "Is the pilot price still open for the second semester?",
];

/// The planted re-transcript: lines 1-3 copied word for word.
const VERBATIM: &str = "Jeisil will send the signed contract to Liberty by Friday. Wilson to book the onboarding call with the Liberty team next week. We decided the pilot starts with two hundred students.";

/// A real summary of the same meeting: its words, not its sentences.
const CLEAN: &str = "The Liberty University pilot starts with two hundred students and Jeisil will send the signed contract.";

fn plain() -> String {
    FIXTURE.join("\n")
}

fn map_answer(req: &wilson_voice_lib::polish_protocol::PolishRequest, narrative: &str) -> String {
    let ids = labels_in(&req.text);
    let id = |i: usize| ids.get(i).cloned().unwrap_or_else(|| ids[0].clone());
    format!(
        r#"{{"narrative":"{narrative}","actions":[{{"text":"Jeisil will send the signed contract to Liberty by Friday.","segment":"{}"}},{{"text":"Wilson to book the onboarding call with the Liberty team next week.","segment":"{}"}}],"decisions":[{{"text":"We decided the pilot starts with two hundred students.","segment":"{}"}}],"questions":[{{"text":"Is the pilot price still open for the second semester?","segment":"{}"}}]}}"#,
        id(1),
        id(2),
        id(3),
        id(4)
    )
}

fn model_with(narrative: &'static str, headline_summary: &'static str) -> StubModel {
    StubModel::with_headline(move |req| {
        Ok(match req.mode.as_str() {
            "map" => map_answer(req, narrative),
            "reduce" => narrative.to_string(),
            "headline" => format!(
                r#"{{"title":"Liberty pilot contract review","key_points":["The pilot starts with two hundred students.","Jeisil will send the signed contract to Liberty by Friday."],"summary":"{headline_summary}"}}"#
            ),
            other => panic!("unexpected stage {other}"),
        })
    })
}

// ── DO 1: VERBATIM_NARRATIVE_MAX on the real render path ────────────────────

#[test]
fn the_planted_verbatim_fixture_is_over_the_ceiling() {
    let measured = narrative_containment(&plain(), VERBATIM);
    println!("MEASURED verbatim 4-gram containment={measured:.3} max={VERBATIM_NARRATIVE_MAX}");
    assert!(measured > VERBATIM_NARRATIVE_MAX, "{measured}");
    let clean = narrative_containment(&plain(), CLEAN);
    println!("MEASURED clean 4-gram containment={clean:.3} max={VERBATIM_NARRATIVE_MAX}");
    assert!(clean <= VERBATIM_NARRATIVE_MAX, "{clean}");
    assert!(is_verbatim_narrative(&plain(), VERBATIM));
    assert_eq!(
        validate_narrative(&plain(), VERBATIM),
        None,
        "v8_verbatim refuses it"
    );
}

#[test]
fn a_verbatim_summary_is_refused_and_says_so() {
    // Both the MAP narrative and the headline paragraph are re-transcripts.
    let model = model_with(VERBATIM, VERBATIM);
    let s = summarize_segments(&segments_from(FIXTURE), &model).expect("items still make notes");
    let md = &s.markdown;
    println!("{md}");
    assert!(
        md.contains(&format!("### Summary\n\n{NO_SUMMARY_VERBATIM}")),
        "the refusal sentence, exactly: {md}"
    );
    assert!(
        !md.contains(
            "Wilson to book the onboarding call with the Liberty team next week. We decided"
        ),
        "{md}"
    );
    assert!(
        s.narrative.trim().is_empty(),
        "no paragraph, never the MAP fallback: {}",
        s.narrative
    );
    assert_eq!(
        NO_SUMMARY_VERBATIM,
        "No summary this time: the notes model could only repeat the transcript."
    );
}

#[test]
fn a_clean_planted_fixture_still_yields_its_planted_actions() {
    let model = model_with(CLEAN, "");
    let s = summarize_segments(&segments_from(FIXTURE), &model).expect("a summary");
    let md = &s.markdown;
    println!("{md}");
    assert!(
        md.contains("### Summary\n\nThe Liberty University pilot"),
        "{md}"
    );
    assert!(!md.contains(NO_SUMMARY_VERBATIM), "{md}");
    let texts: Vec<&str> = s.actions.iter().map(|a| a.text.as_str()).collect();
    assert!(
        texts.contains(&"Jeisil will send the signed contract to Liberty by Friday."),
        "{texts:?}"
    );
    assert!(
        texts.contains(&"Wilson to book the onboarding call with the Liberty team next week."),
        "{texts:?}"
    );
    assert_eq!(s.decisions.len(), 1, "{:?}", s.decisions);
    assert_eq!(s.questions.len(), 1, "{:?}", s.questions);
}

// ── DO 2: speaker tags ──────────────────────────────────────────────────────

#[test]
fn leading_speaker_tags_are_stripped() {
    assert_eq!(
        strip_speaker_tag("Me: we shipped the pilot"),
        "we shipped the pilot"
    );
    assert_eq!(strip_speaker_tag("Them: send it over"), "send it over");
    assert_eq!(
        strip_speaker_tag("speaker 2: book the call"),
        "book the call"
    );
    assert_eq!(strip_speaker_tag("Speaker: book the call"), "book the call");
    assert_eq!(
        strip_speaker_tag("Pricing: forty nine dollars"),
        "Pricing: forty nine dollars"
    );
    let source = plain();
    let tagged = format!("Me: {CLEAN}");
    let kept = validate_narrative(&source, &tagged).expect("a clean narrative survives");
    assert!(!kept.starts_with("Me:"), "{kept}");
}

// ── DO 3, 4, 5: sections, per-segment cap, kinds ────────────────────────────

fn item(text: &str, seg: &str) -> SummaryItem {
    SummaryItem {
        text: text.to_string(),
        segment: seg.to_string(),
        start_seconds: 0.0,
        speaker: None,
    }
}

#[test]
fn one_item_lives_in_exactly_one_section() {
    let lines = transcript_lines(&segments_from(FIXTURE));
    let line = "We decided Jeisil will send the signed contract by Friday?";
    let mut actions = vec![item(
        "Jeisil will send the signed contract by Friday.",
        "seg_0001",
    )];
    let mut decisions = vec![item(
        "jeisil will send the signed contract by friday",
        "seg_0001",
    )];
    let mut questions = vec![item(line, "seg_0001")];
    apply_item_guards(&mut actions, &mut decisions, &mut questions, &lines);
    assert_eq!(actions.len(), 1);
    assert!(decisions.is_empty(), "{decisions:?}");
    assert!(
        questions.is_empty() || questions.iter().all(|q| q.text == line),
        "{questions:?}"
    );
}

#[test]
fn at_most_two_items_cite_one_segment_and_the_strongest_stay() {
    let lines = transcript_lines(&segments_from(FIXTURE));
    let mut actions: Vec<SummaryItem> = (0..10)
        .map(|i| {
            item(
                &format!("The contract draft number {i} has to be checked."),
                "seg_0001",
            )
        })
        .collect();
    actions.insert(
        4,
        item(
            "Jeisil will send the signed contract to Liberty by Friday.",
            "seg_0001",
        ),
    );
    let (mut d, mut q) = (Vec::new(), Vec::new());
    apply_item_guards(&mut actions, &mut d, &mut q, &lines);
    assert_eq!(actions.len(), MAX_ITEMS_PER_SEGMENT, "{actions:?}");
    assert!(
        actions
            .iter()
            .any(|a| a.text.starts_with("Jeisil will send")),
        "{actions:?}"
    );
}

#[test]
fn feelings_are_not_actions_and_statements_are_not_questions() {
    for not_action in [
        "It's really dope.",
        "I'm excited about the launch.",
        "That was a great call.",
    ] {
        assert!(!action_has_commitment(not_action), "{not_action}");
    }
    for action in [
        "I'll send the deck tonight.",
        "Jeisil will send the contract.",
        "Send the deck.",
        "Jeisil, send the deck.",
    ] {
        assert!(action_has_commitment(action), "{action}");
    }
    assert!(!question_is_open(
        "The pilot price is forty dollars.",
        "The pilot price is forty dollars."
    ));
    assert!(question_is_open("Is the pilot price still open?", ""));
    assert!(!decision_has_marker(
        "The weather was nice.",
        "The weather was nice."
    ));
    assert!(decision_has_marker(
        "Drop the free trial.",
        "We decided to drop the free trial."
    ));
    let lines = transcript_lines(&segments_from(FIXTURE));
    let mut actions = vec![
        item("It's really dope.", "seg_0000"),
        item(
            "Jeisil will send the signed contract to Liberty by Friday.",
            "seg_0001",
        ),
    ];
    let mut decisions = vec![item("The weather was nice.", "seg_0000")];
    let mut questions = vec![item("The pilot price is forty dollars.", "seg_0000")];
    apply_item_guards(&mut actions, &mut decisions, &mut questions, &lines);
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert!(decisions.is_empty() && questions.is_empty());
}

// ── DO 6: title ─────────────────────────────────────────────────────────────

#[test]
fn the_fallback_title_is_never_transcript_words() {
    use chrono::TimeZone;
    let at = chrono::Utc
        .with_ymd_and_hms(2026, 11, 7, 22, 50, 0)
        .unwrap();
    assert_eq!(fallback_title(Some(at)), "Meeting, Saturday 22:50");
    assert_eq!(fallback_title::<chrono::Utc>(None), "Meeting notes");
    let model = StubModel::with_headline(|req| match req.mode.as_str() {
        "headline" => Err(SummaryError::Protocol),
        "map" => Ok(map_answer(req, CLEAN)),
        _ => Ok(CLEAN.to_string()),
    });
    let s = summarize_segments(&segments_from(FIXTURE), &model).expect("a summary");
    assert!(s.markdown.starts_with("## Meeting, "), "{}", s.markdown);
    assert!(!s.markdown.starts_with("## Okay so this"), "{}", s.markdown);
}

// ── DO 8: no padded key points ──────────────────────────────────────────────

#[test]
fn key_points_are_never_padded_from_the_transcript() {
    let lines = transcript_lines(&segments_from(FIXTURE));
    let full = plain();
    assert!(backfill_key_points(Vec::new(), &lines, &[], &full).is_empty());
    let one = backfill_key_points(
        vec!["The pilot starts with two hundred students.".into()],
        &lines,
        &[],
        &full,
    );
    assert_eq!(one.len(), 1, "{one:?}");
    let model = StubModel::with_headline(|req| match req.mode.as_str() {
        "headline" => Err(SummaryError::Protocol),
        "map" => Ok(map_answer(req, CLEAN)),
        _ => Ok(CLEAN.to_string()),
    });
    let s = summarize_segments(&segments_from(FIXTURE), &model).expect("a summary");
    assert!(
        !s.markdown.contains("### Key points"),
        "zero points, no section: {}",
        s.markdown
    );
}

// ── DO 9, 10: echo owner and mic-track owner ────────────────────────────────

fn seg(i: usize, track: i64, start: f64, end: f64, text: &str) -> MeetingSegment {
    let mut s = segments_from(&[text]).remove(0);
    s.id = format!("segment-{i}");
    s.track = track;
    s.start_seconds = start;
    s.end_seconds = end;
    s
}

#[test]
fn an_echoed_mic_line_is_them_and_a_mic_action_is_someone_here() {
    let segments = vec![
        seg(
            0,
            1,
            10.0,
            13.0,
            "Jeisil will send the signed contract to Liberty by Friday.",
        ),
        seg(
            1,
            MIC_TRACK,
            11.0,
            14.0,
            "Jeisil will send the signed contract to Liberty by Friday.",
        ),
        seg(
            2,
            MIC_TRACK,
            40.0,
            43.0,
            "I will book the onboarding call with the Liberty team.",
        ),
    ];
    assert!(word_trigram_containment(&segments[1].text, &segments[0].text) >= 0.3);
    assert_eq!(echo_owner(&segments[1], &segments), TrackOwner::Them);
    assert_eq!(echo_owner(&segments[2], &segments), TrackOwner::Mic);
    assert!(!mic_is_single_voice(&segments));
    let label = |t: i64| {
        SegmentSpeaker::Anonymous(if t == MIC_TRACK {
            "Me".into()
        } else {
            "Them".into()
        })
    };
    let lines: Vec<TranscriptLine> = segments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            TranscriptLine::new(i, s.start_seconds, s.text.clone())
                .with_speaker(Some(label(s.track)))
        })
        .collect();
    let mut actions = vec![
        SummaryItem {
            speaker: Some("Me".into()),
            ..item(&segments[1].text, &lines[1].label)
        },
        SummaryItem {
            speaker: Some("Me".into()),
            ..item(&segments[2].text, &lines[2].label)
        },
        SummaryItem {
            speaker: Some("Them".into()),
            ..item(&segments[0].text, &lines[0].label)
        },
    ];
    attribute_owners(&mut actions, &mut [], &mut [], &segments, &lines);
    assert_eq!(
        actions[0].speaker.as_deref(),
        Some("Them"),
        "echo of the far side"
    );
    assert_eq!(
        actions[1].speaker.as_deref(),
        Some(MIC_SHARED_OWNER),
        "never a guessed Me"
    );
    assert_eq!(actions[2].speaker.as_deref(), Some("Them"));
    assert_eq!(MIC_SHARED_OWNER, "Someone here");
}

// ── DO 12: the Summary input covers the whole meeting ───────────────────────

struct OverBudget;
impl TokenCounter for OverBudget {
    fn count_tokens(&self, text: &str) -> Result<usize, SummaryError> {
        Ok(text.split_whitespace().count() * 100 + 1_000_000)
    }
}

#[test]
fn a_commitment_in_the_last_chunk_reaches_the_summary_input() {
    let filler = "The team walked through the slides and talked about the general plan for the quarter in some detail. ".repeat(6);
    let mut narratives: Vec<String> = (0..40)
        .map(|i| format!("Part {i} recap. {filler}"))
        .collect();
    narratives.push(format!(
        "Priya will send the signed contract to Liberty on Friday. {filler}"
    ));
    let lines = transcript_lines(&segments_from(FIXTURE));
    let input = headline_source(&lines, &narratives, &OverBudget);
    assert!(
        input.contains("Priya will send the signed contract"),
        "last chunk missing"
    );
    assert!(input.contains("Part 0 recap."), "first chunk missing");
    let shrunk = even_shrink(&narratives, 750);
    assert!(shrunk.contains("Priya will send"), "{shrunk}");
}
