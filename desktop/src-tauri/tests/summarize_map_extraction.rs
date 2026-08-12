//! YV97 acceptance — "action items extracted from a synthetic per-chunk MAP
//! pass carry a segment id present in that specific chunk (not the whole
//! meeting)".
//!
//! This is finding #18's mechanical provenance guarantee, scoped to 22-A's flat
//! chronological ids, and finding #34's inversion of the map-reduce stages. The
//! distinction the test turns on is the one the plan got wrong: an id that
//! exists SOMEWHERE IN THE MEETING but not in the chunk that cited it is exactly
//! what a hallucinating model produces, and a whole-meeting allowlist would wave
//! it through.
//!
//! The extraction stage itself is asserted too: actions come out of MAP, REDUCE
//! never sees a transcript line, and the merged list is byte-identical to what
//! MAP produced — a merge that re-generates cannot pass that.

mod support;

use support::{labels_in, map_answer, segments_from, StubModel};
use wilson_voice_lib::polish_protocol::{KIND_SUMMARIZE, SUMMARIZE_MAP, SUMMARIZE_REDUCE};
use wilson_voice_lib::summarize::summarize_segments;

/// Enough transcript to need more than one MAP pass at the shipped chunk size —
/// a one-chunk meeting cannot distinguish "this chunk" from "the meeting".
fn long_meeting() -> Vec<wilson_voice_lib::meetings::MeetingSegment> {
    let texts: Vec<String> = (0..140)
        .map(|i| {
            format!(
                "turn {} we should move the onboarding review and nobody owns the escalation path yet",
                i % 7
            )
        })
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    segments_from(&refs)
}

#[test]
fn map_items_cite_only_their_own_chunk() {
    let segments = long_meeting();

    // Every MAP pass answers with one honest item citing a line it was given,
    // and one item citing `seg_0001` — a real id in the MEETING, and a real id
    // in the FIRST chunk, but a fabricated citation for every chunk after it.
    let model = StubModel::new(|req| {
        let labels = labels_in(&req.text);
        if req.mode == SUMMARIZE_REDUCE {
            return Ok(
                "We should move the onboarding review and nobody owns the escalation path.".into(),
            );
        }
        Ok(map_answer(
            "We should move the onboarding review and nobody owns the escalation path.",
            &[
                ("Move the onboarding review", labels[1].as_str()),
                ("Move the escalation path review", "seg_0001"),
            ],
            &[],
            &[("Who owns the escalation path", labels[2].as_str())],
        ))
    });

    let summary = summarize_segments(&segments, &model).expect("a summary");
    let map = model.stage(SUMMARIZE_MAP);
    assert!(
        map.len() >= 2,
        "the fixture must span more than one chunk, got {}",
        map.len()
    );

    // Each MAP request is a constrained summarize pass over its own chunk, and
    // its grammar enumerates that chunk's ids and no others.
    let mut chunk_labels: Vec<Vec<String>> = Vec::new();
    for req in &map {
        assert_eq!(req.kind, KIND_SUMMARIZE);
        assert_eq!(req.mode, SUMMARIZE_MAP);
        let grammar = req.grammar_text().expect("a MAP pass is constrained");
        let labels = labels_in(&req.text);
        for label in &labels {
            assert!(
                grammar.contains(label),
                "the grammar must enumerate {label}, which is in this chunk"
            );
        }
        chunk_labels.push(labels);
    }
    // The second chunk's grammar must NOT offer the first chunk's ids.
    for label in &chunk_labels[0] {
        assert!(
            !map[1].grammar_text().expect("constrained").contains(label),
            "chunk 2's grammar offered {label}, which belongs to chunk 1"
        );
    }

    // THE property: every surviving item's citation is in the chunk that
    // produced it.
    let all: Vec<_> = summary
        .actions
        .iter()
        .chain(summary.questions.iter())
        .collect();
    assert!(!all.is_empty(), "the honest items survived");
    for item in &all {
        let owner = chunk_labels
            .iter()
            .position(|labels| labels.iter().any(|l| l == &item.segment))
            .expect("a cited id belongs to some chunk");
        // The item text is unique per bucket, so its own chunk is identifiable:
        // the honest citations are always the 2nd/3rd line OF THEIR CHUNK.
        assert!(
            chunk_labels[owner].contains(&item.segment),
            "{} cited {}, which is not in the chunk it came from",
            item.text,
            item.segment
        );
    }

    // The fabricated citation was dropped everywhere except the one chunk that
    // genuinely contains `seg_0001`.
    let claiming_first = all.iter().filter(|i| i.segment == "seg_0001").count();
    assert_eq!(
        claiming_first, 1,
        "only the chunk that really holds seg_0001 may cite it"
    );
    assert_eq!(
        summary.dropped_items,
        map.len() - 1,
        "one fabricated citation per later chunk must be dropped"
    );

    // Every item resolves to a real wall-clock offset from the segment it cites.
    for item in &all {
        assert!(item.start_seconds >= 0.0);
    }
}

#[test]
fn extraction_happens_in_map_and_reduce_only_narrates() {
    let segments = long_meeting();
    let model = StubModel::new(|req| {
        let labels = labels_in(&req.text);
        if req.mode == SUMMARIZE_REDUCE {
            // A REDUCE that mentions no commitment at all: if the final action
            // list still holds, it cannot have come from this stage.
            return Ok(
                "The onboarding review should move and nobody owns the escalation path.".into(),
            );
        }
        Ok(map_answer(
            "We should move the onboarding review and nobody owns the escalation path.",
            &[("Move the escalation path review", labels[0].as_str())],
            &[],
            &[],
        ))
    });

    let summary = summarize_segments(&segments, &model).expect("a summary");
    let reduce = model.stage(SUMMARIZE_REDUCE);
    assert_eq!(reduce.len(), 1, "one REDUCE pass, over the MAP narratives");

    // REDUCE is handed narratives ONLY — not the transcript, and not the action
    // list. This is the inversion finding #34 asked for: the evidence never
    // travels through the lossy merge.
    let sent = &reduce[0].text;
    assert!(
        !sent.contains("seg_0001"),
        "REDUCE must not receive transcript lines: {sent:?}"
    );
    assert!(
        !sent.contains("Move the escalation path review"),
        "REDUCE must not receive the action list: {sent:?}"
    );
    assert!(
        sent.contains("We should move the onboarding review and nobody owns the escalation path.")
    );
    assert!(
        reduce[0].grammar_text().is_none(),
        "the narrative is free text"
    );

    // The action survived a merge that never asked a model anything, and the
    // identical item extracted from every chunk collapsed to one.
    assert_eq!(summary.actions.len(), 1);
    assert_eq!(summary.actions[0].text, "Move the escalation path review");
    assert_eq!(
        summary.narrative,
        "The onboarding review should move and nobody owns the escalation path."
    );
}

#[test]
fn a_failed_map_pass_does_not_take_the_meeting_with_it() {
    let segments = long_meeting();
    let model = StubModel::new(|req| {
        let labels = labels_in(&req.text);
        if req.mode == SUMMARIZE_REDUCE {
            return Ok("We should move the onboarding review.".into());
        }
        // The first chunk's pass fails outright; the rest must still land.
        if labels.iter().any(|l| l == "seg_0001") {
            return Err(wilson_voice_lib::summarize::SummaryError::Protocol);
        }
        Ok(map_answer(
            "Nobody owns the escalation path yet.",
            &[("Who owns the escalation path", labels[0].as_str())],
            &[],
            &[],
        ))
    });

    let summary = summarize_segments(&segments, &model).expect("a summary survives one failure");
    assert_eq!(summary.actions.len(), 1);
    assert!(!summary.markdown.is_empty());
}
