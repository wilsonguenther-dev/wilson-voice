//! YV133 acceptance — "on a fixture with named speakers, produces at least one
//! action item whose rendered text includes an owner name resolved from
//! enrollment, not 'Speaker 2' or a chronological label."
//!
//! ## What this test can and cannot claim, stated before the code
//!
//! The backlog sequences YV133 after YV125–131, and the criterion above says
//! "post-YV129 enrollment". On `main` today YV120, YV121, YV123, YV125 and YV132
//! are merged and **YV126–131 are not**: there is no `speaker_profiles` table,
//! no clustering, and no enrollment match. So the fixture's names come from the
//! seam rather than from a database — `support::EnrolledSpeakers`, a hand-built
//! [`SpeakerSource`] with the exact shape YV129's matcher will have.
//!
//! That makes this a real test of everything YV133 owns — the prompt, the
//! grammar, the parse, the lookup, the render — and **not** a claim that the
//! shipped app can produce a named speaker today. It cannot, and
//! `the_app_cannot_reach_a_named_speaker_yet` asserts exactly that, out loud,
//! rather than leaving a green suite to imply otherwise. That is the same
//! posture YV132 took when it published three matrix rows whose honest cell was
//! "the app cannot reach this".
//!
//! Unlike `summarize_e2e.rs` this file needs no corpus: the property under test
//! is a property of the pipeline over a transcript, and the transcript is right
//! here. It runs in CI, every time.

mod support;

use support::{bodies_in, labels_in, map_answer, segments_on_tracks, EnrolledSpeakers, StubModel};
use wilson_voice_lib::meetings::{MeetingKind, MIC_TRACK};
use wilson_voice_lib::summarize::{summarize_segments_with, SpeakerSource, TrackSpeakers};

/// Six turns of a three-person conversation in one room — the IRL case merged
/// finding #4 promoted, on a single mic track, which is why every turn is
/// [`MIC_TRACK`] and none of it could be told apart by a channel number.
const TURNS: [(f64, i64, &str); 6] = [
    (
        0.0,
        MIC_TRACK,
        "we should move the onboarding review before the release goes out",
    ),
    (
        9.0,
        MIC_TRACK,
        "i will send the pricing document over tomorrow morning",
    ),
    (
        17.0,
        MIC_TRACK,
        "nobody owns the escalation path yet and someone should fix that",
    ),
    (
        26.0,
        MIC_TRACK,
        "we are shipping without the calendar work this cycle and that is settled",
    ),
    (
        34.0,
        MIC_TRACK,
        "can somebody confirm the onboarding review moved",
    ),
    (
        41.0,
        MIC_TRACK,
        "i will confirm the onboarding review with the release owner",
    ),
];

/// Two enrolled voices and one that is not — the shape YV129's matcher produces
/// on a real room, where somebody in it has never been enrolled.
fn enrolled() -> EnrolledSpeakers {
    EnrolledSpeakers::new(&[
        ("segment-0", "Jeisil"),
        ("segment-1", "Aidan"),
        ("segment-3", "Jeisil"),
        ("segment-5", "Aidan"),
        // segment-2 and segment-4 are the unenrolled voice: no name, and no guess.
    ])
}

/// A stand-in model that answers strictly out of the chunk it was handed: the
/// narrative is the first line, each item is a clause of a later line, and every
/// citation is that line's own id. Grounded by construction, so what is being
/// measured is the pipeline and not the stub.
fn scripted() -> StubModel {
    StubModel::new(|req| {
        if req.mode == "reduce" {
            return Ok(req
                .text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" "));
        }
        let labels = labels_in(&req.text);
        let bodies = bodies_in(&req.text);
        let clause = |i: usize| -> String {
            bodies
                .get(i)
                .map(|b| b.split_whitespace().take(9).collect::<Vec<_>>().join(" "))
                .unwrap_or_default()
        };
        Ok(map_answer(
            &clause(0),
            &[
                (clause(1).as_str(), labels[1 % labels.len()].as_str()),
                (clause(5).as_str(), labels[5 % labels.len()].as_str()),
            ],
            &[(clause(3).as_str(), labels[3 % labels.len()].as_str())],
            &[(clause(4).as_str(), labels[4 % labels.len()].as_str())],
        ))
    })
}

#[test]
fn summarize_e2e_speaker_attributed() {
    let segments = segments_on_tracks(&TURNS);
    let summary = summarize_segments_with(&segments, &scripted(), &enrolled())
        .expect("the fixture summarizes");

    println!(
        "---- stored summary ----\n{}\n------------------------",
        summary.markdown
    );

    // **The acceptance.** At least one action carries a real person's name,
    // resolved from enrollment through the id the model cited.
    let named: Vec<&wilson_voice_lib::summarize::SummaryItem> = summary
        .actions
        .iter()
        .filter(|a| a.speaker.is_some())
        .collect();
    assert!(
        !named.is_empty(),
        "no action carried a speaker:\n{}",
        summary.markdown
    );
    let owners: Vec<&str> = named.iter().filter_map(|a| a.speaker.as_deref()).collect();
    assert!(
        owners.iter().any(|o| *o == "Aidan" || *o == "Jeisil"),
        "the owner is an enrolled name, not a voice number: {owners:?}"
    );
    for owner in &owners {
        assert!(
            !owner.starts_with("Speaker") && !owner.starts_with("seg_"),
            "{owner:?} is a placeholder or a chronological label, not an owner"
        );
    }

    // It reaches the reader in the stored Markdown, beside the citation.
    let action_lines: Vec<&str> = summary
        .markdown
        .lines()
        .skip_while(|l| !l.starts_with("### Action items"))
        .filter(|l| l.starts_with("- "))
        .collect();
    assert!(!action_lines.is_empty());
    assert!(
        action_lines
            .iter()
            .any(|l| l.contains("(Aidan, ") || l.contains("(Jeisil, ")),
        "a rendered action names its speaker: {action_lines:?}"
    );
    for line in &action_lines {
        assert!(
            !line.contains("seg_"),
            "a chronological label must never render to the reader: {line}"
        );
        assert!(!line.contains("Speaker "), "nor a voice number: {line}");
    }

    // The unenrolled voice stays unenrolled: nothing is attributed to a segment
    // the lookup does not hold, in either direction.
    for item in summary
        .actions
        .iter()
        .chain(&summary.decisions)
        .chain(&summary.questions)
    {
        let index: usize = item.segment["seg_".len()..]
            .parse::<usize>()
            .expect("an id")
            - 1;
        let expected = enrolled().speaker_for(&segments[index]);
        assert_eq!(
            item.speaker.as_deref(),
            expected.as_ref().map(|s| s.label()),
            "{} was attributed to {:?} but the lookup says {:?}",
            item.segment,
            item.speaker,
            expected.as_ref().map(|s| s.label())
        );
    }
    let unattributed = summary
        .questions
        .iter()
        .filter(|q| q.speaker.is_none())
        .count();
    println!(
        "summarize_e2e_speaker_attributed: {} action(s), {} decision(s), {} question(s); \
         {} item(s) cite the unenrolled voice and carry NO name",
        summary.actions.len(),
        summary.decisions.len(),
        summary.questions.len(),
        unattributed
    );
    assert!(
        unattributed > 0,
        "the fixture's unenrolled voice must exercise the no-name path — a demo \
         that only walks the easy road proves less"
    );
}

/// The honesty gate. **Nothing in the shipped app can produce a named speaker
/// yet**, and this test fails the day that changes so the claim above cannot go
/// stale silently.
///
/// `TrackSpeakers` is the only `SpeakerSource` the app constructs (`lib.rs`,
/// `summarize_meeting_blocking`). It answers from a channel number, so it can
/// only ever be `Anonymous` — YV128's `speaker_profiles` table and YV129's
/// matcher are what make the test above reachable from the app, and neither
/// exists on `main`.
#[test]
fn the_app_cannot_reach_a_named_speaker_yet() {
    // Both shapes, because they take different branches and only one of them
    // produces a label at all. The IRL fixture above is single-track, so every
    // kind resolves to `ClusterTrackA` and the source declines — a test built
    // only on that would still pass if `TrackSpeakers` started calling track 0
    // by somebody's name on a two-track call.
    let two_track: Vec<(f64, i64, &str)> = TURNS
        .iter()
        .enumerate()
        .map(|(i, (at, _, text))| {
            (
                *at,
                if i % 2 == 0 {
                    MIC_TRACK
                } else {
                    wilson_voice_lib::meetings::SYSTEM_TRACK
                },
                *text,
            )
        })
        .collect();
    for turns in [TURNS.to_vec(), two_track] {
        let segments = segments_on_tracks(&turns);
        for kind in [
            MeetingKind::Virtual,
            MeetingKind::InPerson,
            MeetingKind::Unknown,
        ] {
            let speakers = TrackSpeakers::for_meeting(kind, &segments);
            assert!(
                segments
                    .iter()
                    .all(|s| !speakers.speaker_for(s).is_some_and(|v| v.is_named())),
                "{kind:?} produced a name from a track index"
            );
        }
    }
    // And the one configuration that DOES label — a virtual call with a live
    // second track — labels with the two anonymous words YV108 shipped, so the
    // claim "the app cannot name anybody" is asserted where a name would show.
    let live_call = segments_on_tracks(&[
        (0.0, MIC_TRACK, TURNS[0].2),
        (9.0, wilson_voice_lib::meetings::SYSTEM_TRACK, TURNS[1].2),
    ]);
    let speakers = TrackSpeakers::for_meeting(MeetingKind::Virtual, &live_call);
    let labels: Vec<String> = live_call
        .iter()
        .filter_map(|s| speakers.speaker_for(s))
        .map(|s| s.label().to_string())
        .collect();
    assert_eq!(labels, vec!["Me", "Them"]);
    let lib = include_str!("../src/lib.rs");
    assert!(
        lib.contains("summarize::TrackSpeakers::for_meeting("),
        "the app wires the track source…"
    );
    assert!(
        !lib.contains("speaker_profiles"),
        "…and there is no enrolled-profile source to wire yet (YV128/YV129)"
    );
    println!(
        "the app's only speaker source is TrackSpeakers (Me/Them on a live \
         two-track call, nothing on the clustering branch); enrolled names \
         become reachable with YV128–130"
    );
}
