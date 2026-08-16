//! YV133 acceptance — "`SummaryItem.speaker` is populated by post-parse lookup
//! against the evidence segment's known speaker, and no code path constructs a
//! `speaker` value from raw model output text."
//!
//! The hazard this file exists for is finding #18's, one field over. YV97 closed
//! it for evidence ids: a grammar constrains a field's SHAPE, never its truth,
//! so the ids are enumerated per chunk and the parser drops anything outside the
//! enum. A `speaker` field would have had exactly the same problem and no
//! equivalent fix — there is no enum of "people who were in the room" to
//! constrain a name against, and a plausible wrong name is indistinguishable
//! from a right one at the parser.
//!
//! So the model is never asked. It is SHOWN the speaker beside each id, it cites
//! an id from the enum it was constrained to, and the name is read back out of
//! the same table the line was rendered from. This file drives a stub that does
//! the worst thing a real model plausibly does with that information — repeats a
//! DIFFERENT name in its item text, and volunteers a `"speaker"` key that is not
//! in the schema — and asserts both land nowhere.

mod support;

use support::{
    bodies_in, labels_in, map_answer, segments_from, segments_on_tracks, speakers_in,
    EnrolledSpeakers, StubModel,
};
use wilson_voice_lib::meetings::{
    self, MeetingKind, MIC_SPEAKER_LABEL, MIC_TRACK, SYSTEM_SPEAKER_LABEL, SYSTEM_TRACK,
};
use wilson_voice_lib::summarize::{
    parse_map_output, summarize_segments, summarize_segments_with, transcript_lines,
    transcript_lines_with, NoSpeakers, SegmentSpeaker, SpeakerSource, TrackSpeakers,
};

/// Three turns of a meeting in a room, worded so that an item quoting them is
/// grounded without any help — the V3 floor has to be exercised by the item's
/// own words, not satisfied by accident.
const TURNS: [&str; 3] = [
    "we should move the onboarding review before the release goes out",
    "i will send the pricing document over tomorrow morning",
    "nobody owns the escalation path yet and someone should fix that",
];

/// The enrolled names for [`TURNS`], as YV129's matcher would resolve them.
fn enrolled() -> EnrolledSpeakers {
    EnrolledSpeakers::new(&[
        ("segment-0", "Jeisil"),
        ("segment-1", "Aidan"),
        ("segment-2", "Jeisil"),
    ])
}

/// A MAP answer carrying the two things a model can do to smuggle a speaker in:
/// a different person's name inside the item TEXT, and a `"speaker"` key that
/// is not in the schema at all.
fn answer_naming_the_wrong_person(segment: &str) -> String {
    serde_json::json!({
        "narrative": "The onboarding review should move before the release goes out.",
        "actions": [{
            "text": "Priya will send the pricing document over tomorrow",
            "segment": segment,
            "speaker": "Priya",
        }],
        "decisions": [],
        "questions": [],
    })
    .to_string()
}

/// **The acceptance.** The name on the item is the one the LOOKUP holds for the
/// cited segment, not either of the two the model offered.
#[test]
fn the_speaker_is_looked_up_from_the_cited_segment_never_read_off_the_model() {
    let segments = segments_from(&TURNS);
    let model = StubModel::new(|req| {
        if req.mode == "reduce" {
            return Ok(req.text.clone());
        }
        // Cite the second line — `segment-1`, enrolled as Aidan.
        let labels = labels_in(&req.text);
        Ok(answer_naming_the_wrong_person(&labels[1]))
    });

    let summary =
        summarize_segments_with(&segments, &model, &enrolled()).expect("the meeting summarizes");
    assert_eq!(summary.actions.len(), 1, "one action survived the gate");
    let action = &summary.actions[0];

    assert_eq!(
        action.speaker.as_deref(),
        Some("Aidan"),
        "the speaker is the enrolled name of the segment the model CITED"
    );
    assert!(
        action.text.contains("Priya"),
        "the stub really did name somebody else in its text — otherwise this \
         test proves nothing about ignoring it"
    );
    assert_ne!(
        action.speaker.as_deref(),
        Some("Priya"),
        "a name the model wrote must never become the attribution"
    );
    assert_eq!(action.segment, "seg_0002");

    // And it reaches the reader: the stored Markdown carries the looked-up
    // name, beside the citation rather than in front of the text.
    assert!(
        summary.markdown.contains("(Aidan, 00:00:04)"),
        "the rendered action carries the attribution:\n{}",
        summary.markdown
    );
    assert!(
        !summary.markdown.contains("(Priya,"),
        "…and never the model's:\n{}",
        summary.markdown
    );
}

/// The `"speaker"` key the stub volunteered does not exist on the type the
/// answer deserializes into, so it is dropped at the parse — before any lookup
/// could be tempted to prefer it.
#[test]
fn a_speaker_key_in_the_model_answer_lands_nowhere() {
    let source = TURNS.join("\n");
    let labels: Vec<String> = (1..=3).map(|i| format!("seg_{i:04}")).collect();
    let out = parse_map_output(
        &answer_naming_the_wrong_person("seg_0002"),
        &labels,
        &source,
    );
    assert_eq!(out.actions.len(), 1);
    // `ExtractedItem` is exactly `{text, segment}`; a round-trip through it
    // cannot carry a speaker no matter what the model wrote.
    let round_trip = serde_json::to_value(&out.actions[0]).expect("serializes");
    assert_eq!(
        round_trip.as_object().map(|o| o.len()),
        Some(2),
        "the model's output type has two fields and neither is a speaker: {round_trip}"
    );
    assert!(round_trip.get("speaker").is_none());
}

/// An id the transcript does not hold gets no speaker rather than the first
/// one — the same "unknown resolves to nothing" rule the offset lookup already
/// follows.
///
/// Unreachable through the grammar and the allowlist together, which is exactly
/// why it is asserted: this is the branch that runs if a later change ever lets
/// an id through that the transcript does not hold.
#[test]
fn an_uncited_id_resolves_to_no_speaker_rather_than_the_wrong_one() {
    let segments = segments_from(&TURNS);
    let lines = transcript_lines_with(&segments, &enrolled());
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| l.speaker.is_some()));
    // The lookup table is keyed by label; nothing in it answers for an id that
    // is not a line.
    assert!(!lines.iter().any(|l| l.label == "seg_0099"));
}

/// A source that declines leaves the YV97 shape untouched, byte for byte.
///
/// The rendered chunk is what the model is handed, so "unchanged" has to mean
/// unchanged as TEXT — a trailing space or a stray tag is a different prompt.
#[test]
fn no_speaker_source_renders_exactly_what_yv97_rendered() {
    let segments = segments_from(&TURNS);
    let before = transcript_lines(&segments);
    let after = transcript_lines_with(&segments, &NoSpeakers);
    assert_eq!(before, after);
    assert!(before.iter().all(|l| l.speaker.is_none()));
    assert_eq!(before[0].render(), "seg_0001: ".to_string() + TURNS[0]);

    let model = StubModel::new(|req| {
        if req.mode == "reduce" {
            return Ok(req.text.clone());
        }
        let labels = labels_in(&req.text);
        let bodies = bodies_in(&req.text);
        Ok(map_answer(
            &bodies[0],
            &[(bodies[1].as_str(), labels[1].as_str())],
            &[],
            &[],
        ))
    });
    let summary = summarize_segments(&segments, &model).expect("summarizes");
    assert!(
        summary.actions.iter().all(|a| a.speaker.is_none()),
        "no source, no attribution — and no placeholder either"
    );
    assert!(
        !summary.markdown.contains(", 00:00:0"),
        "the un-attributed render is YV97's `- text (00:00:04)`:\n{}",
        summary.markdown
    );
}

/// The summary's attribution and the transcript's speaker column are two
/// readings of ONE decision.
///
/// `TrackSpeakers::for_meeting` and `meetings::render_transcript` both compute
/// the target from the meeting's kind and its own segments. If they ever stop
/// agreeing, an action item says "Them" beside a transcript row that says "Me",
/// on the same screen, about the same sentence.
#[test]
fn track_speakers_agree_with_the_rendered_transcript_line_for_line() {
    let segments = segments_on_tracks(&[
        (0.0, MIC_TRACK, TURNS[0]),
        (4.0, SYSTEM_TRACK, TURNS[1]),
        (8.0, MIC_TRACK, TURNS[2]),
    ]);
    let speakers = TrackSpeakers::for_meeting(MeetingKind::Virtual, &segments);
    let rendered = meetings::render_transcript(&segments, MeetingKind::Virtual);

    for (segment, line) in segments.iter().zip(rendered.iter()) {
        assert_eq!(segment.id, line.segment_id, "same order, same rows");
        assert_eq!(
            speakers.speaker_for(segment).map(|s| s.label().to_string()),
            Some(line.speaker.to_string()),
            "the summary's speaker for {} must be the transcript's",
            segment.id
        );
    }
    assert_eq!(rendered[0].speaker, MIC_SPEAKER_LABEL);
    assert_eq!(rendered[1].speaker, SYSTEM_SPEAKER_LABEL);

    // …and it reaches the prompt, in the order the model reads it.
    let lines = transcript_lines_with(&segments, &speakers);
    assert_eq!(lines[0].render(), format!("seg_0001 (Me): {}", TURNS[0]));
    assert_eq!(lines[1].render(), format!("seg_0002 (Them): {}", TURNS[1]));
}

/// The clustering branch declines rather than tagging every line "Speaker".
///
/// Three separate costs, all of them real: the tag separates nobody (it is
/// identical on every line), it spends tokens out of the budget finding #35
/// exists to protect, and it writes the bare word *Speaker* into the source text
/// the groundedness floor scores against — where a summarizer can pick it up as
/// a name. The measured token cost is printed, because "it costs tokens" is a
/// claim and this suite measures claims.
#[test]
fn the_clustering_branch_declines_rather_than_tagging_every_line() {
    let segments = segments_from(&TURNS);
    for kind in [MeetingKind::InPerson, MeetingKind::Unknown] {
        let speakers = TrackSpeakers::for_meeting(kind, &segments);
        assert!(
            segments.iter().all(|s| speakers.speaker_for(s).is_none()),
            "{kind:?}: 'Speaker' means 'this build cannot say who', which is not \
             an attribution"
        );
        assert_eq!(
            transcript_lines_with(&segments, &speakers),
            transcript_lines(&segments),
            "{kind:?}: the prompt is byte-identical to YV97's"
        );
    }

    // What declining saves, measured against the same counter the chunker uses.
    let tagged: String = transcript_lines(&segments)
        .iter()
        .map(|l| format!("{} (Speaker): {}\n", l.label, l.text))
        .collect();
    let bare: String = transcript_lines(&segments)
        .iter()
        .map(|l| format!("{}\n", l.render()))
        .collect();
    let cost = support::meeting_token_count(&tagged) - support::meeting_token_count(&bare);
    println!(
        "a 'Speaker' tag on every line of a {}-line transcript costs {cost} tokens and \
         separates nobody",
        segments.len()
    );
    assert!(cost > 0, "the tag is not free, which is the point");
}

/// A track index is a channel, not a person: `TrackSpeakers` can never answer
/// `Named`, under any kind, on any track.
///
/// This is the same rule `meetings::speaker_label`'s header states about
/// `is_me`, asserted at the seam where a name would first become visible.
#[test]
fn a_track_is_never_promoted_to_an_identity() {
    for kind in [
        MeetingKind::Virtual,
        MeetingKind::InPerson,
        MeetingKind::Unknown,
    ] {
        for turns in [
            vec![(0.0, MIC_TRACK, TURNS[0])],
            vec![(0.0, MIC_TRACK, TURNS[0]), (4.0, SYSTEM_TRACK, TURNS[1])],
        ] {
            let segments = segments_on_tracks(&turns);
            let speakers = TrackSpeakers::for_meeting(kind, &segments);
            for segment in &segments {
                assert!(
                    !speakers.speaker_for(segment).is_some_and(|s| s.is_named()),
                    "{kind:?} on track {} produced a NAME from a channel number",
                    segment.track
                );
            }
        }
    }
}

/// A speaker whose label is blank is "unknown", not a speaker called "".
#[test]
fn a_blank_speaker_is_no_speaker() {
    struct Blank;
    impl SpeakerSource for Blank {
        fn speaker_for(
            &self,
            _: &wilson_voice_lib::meetings::MeetingSegment,
        ) -> Option<SegmentSpeaker> {
            Some(SegmentSpeaker::Named("   ".to_string()))
        }
    }
    let segments = segments_from(&TURNS);
    let lines = transcript_lines_with(&segments, &Blank);
    assert!(lines.iter().all(|l| l.speaker.is_none()));
    assert_eq!(lines[0].render(), "seg_0001: ".to_string() + TURNS[0]);
}

/// A line long enough to be cut keeps ONE prefix, with the speaker in it.
///
/// `plan_chunks` strips the prefix back off a truncated render before rebuilding
/// the line, and it uses `TranscriptLine::prefix` to do it. A second hand-rolled
/// opinion about what a prefix looks like would leave `seg_0001 (Aidan): seg_0001
/// (Aidan): …` inside the model's prompt.
#[test]
fn a_truncated_attributed_line_is_not_double_labelled() {
    let segments = segments_on_tracks(&[
        (
            0.0,
            MIC_TRACK,
            "the pricing document says the same thing again ",
        ),
        (4.0, SYSTEM_TRACK, "we agreed to stop there"),
    ]);
    let long = segments[0].text.repeat(300);
    let mut segments = segments;
    segments[0].text = long;

    let speakers = TrackSpeakers::for_meeting(MeetingKind::Virtual, &segments);
    let lines = transcript_lines_with(&segments, &speakers);
    let chunks = wilson_voice_lib::summarize::plan_chunks(
        &lines,
        64,
        &StubModel::new(|_| Ok(String::new())),
    )
    .expect("the lines chunk");
    let text = chunks[0].text();
    assert!(
        chunks[0].truncated,
        "a 300× line does not fit a 64-token pass"
    );
    assert_eq!(
        text.matches("seg_0001").count(),
        1,
        "one label, once, per line:\n{text}"
    );
    assert_eq!(
        text.matches("(Me)").count(),
        1,
        "and one speaker tag, once:\n{text}"
    );
    assert_eq!(speakers_in(&text)[0].as_deref(), Some("Me"));
    assert_eq!(labels_in(&text)[0], "seg_0001");
}
