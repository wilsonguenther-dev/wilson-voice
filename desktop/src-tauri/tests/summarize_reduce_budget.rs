//! YV97 regression — REDUCE is bounded in meeting length, and every truncation
//! reaches the reader.
//!
//! The defect this pins down: `reduce_narrative` joined EVERY chunk narrative
//! into one request, with no `count_tokens`, no `fit_to_budget` and no fold. Each
//! MAP narrative is capped at 250 words, so a three-hour meeting (YV91's hard
//! cap, ~27 chunks) reached thousands of tokens against the sidecar's REDUCE
//! budget of `n_ctx - (SUMMARY_REDUCE_MAX_OUT + 1)`. The sidecar cut the tail,
//! answered `truncated:true`, and the parent only `log::warn!`d it — so the
//! stored "summary of this meeting" described the FRONT of the meeting and said
//! nothing about the rest.
//!
//! Both halves are asserted here, and both are stated as differentials rather
//! than as claims about implementation:
//!
//! * the naive join is RECOMPUTED from the same MAP answers and shown to blow the
//!   budget the folded passes stay inside — an implementation that quietly went
//!   back to joining would fail this;
//! * a stub that reports `truncated:true` is shown to change the STORED MARKDOWN,
//!   not just a log line nobody who reads a summary ever sees.

mod support;

use support::{bodies_in, map_answer, meeting_token_count, segments_from, StubModel};
use wilson_voice_lib::meetings::MeetingSegment;
use wilson_voice_lib::polish_protocol::{SUMMARIZE_MAP, SUMMARIZE_REDUCE};
use wilson_voice_lib::summarize::{
    summarize_segments, Generated, MAX_NARRATIVE_WORDS, REDUCE_CHUNK_TOKENS,
};

/// The note `render_summary` owes the reader when a pass did not fit.
const COVERAGE_NOTE: &str = "did not fit the model in one pass";

/// A meeting long enough to need many MAP passes at the shipped chunk size —
/// meeting-shaped for the same reason as the chunking fixture: proper nouns,
/// disfluencies and em dashes are what push real transcripts past the
/// 1.3-tokens/word proxy.
fn long_meeting(lines_wanted: usize) -> Vec<MeetingSegment> {
    const NAMES: [&str; 6] = [
        "Priya",
        "Okonkwo",
        "Delacroix-Bell",
        "Yusuf",
        "Ratnayake",
        "O'Shaughnessy",
    ];
    const BODY: [&str; 6] = [
        "we should move the onboarding review before the release",
        "the staging rollout is blocked on the migration",
        "nobody owns the escalation path yet",
        "the customer wants the export in markdown",
        "we are not shipping the calendar integration this cycle",
        "that number came from the wrong dashboard",
    ];
    let texts: Vec<String> = (0..lines_wanted)
        .map(|i| {
            format!(
                "{}: um, {} — I mean {} — sort of {}",
                NAMES[i % NAMES.len()],
                BODY[i % BODY.len()],
                NAMES[(i + 3) % NAMES.len()],
                BODY[(i + 4) % BODY.len()],
            )
        })
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    segments_from(&refs)
}

/// A chunk narrative at the shipped cap, drawn from the chunk's OWN words so it
/// clears the V3 groundedness floor. A short stub narrative would make this test
/// pass for the wrong reason: the whole question is what happens when 27 of these
/// arrive at REDUCE together.
fn narrative_for(chunk_text: &str) -> String {
    bodies_in(chunk_text)
        .join(" ")
        .split_whitespace()
        .take(MAX_NARRATIVE_WORDS)
        .collect::<Vec<_>>()
        .join(" ")
}

fn scripted() -> StubModel {
    StubModel::new(|req| {
        if req.mode == SUMMARIZE_REDUCE {
            // A merge that keeps the front of what it was handed — words already
            // in the meeting, so the merged narrative stays grounded.
            return Ok(req
                .text
                .split_whitespace()
                .take(180)
                .collect::<Vec<_>>()
                .join(" "));
        }
        Ok(map_answer(&narrative_for(&req.text), &[], &[], &[]))
    })
}

#[test]
fn reduce_never_receives_an_unbounded_prompt() {
    let segments = long_meeting(700);
    let model = scripted();
    let summary = summarize_segments(&segments, &model).expect("a long meeting summarizes");

    let map = model.stage(SUMMARIZE_MAP);
    let reduce = model.stage(SUMMARIZE_REDUCE);
    assert!(
        map.len() >= 8,
        "the fixture must be long enough to matter, got {} MAP passes",
        map.len()
    );

    // THE property: every REDUCE pass was sized against the real tokenizer.
    for (i, req) in reduce.iter().enumerate() {
        let real = meeting_token_count(&req.text);
        assert!(
            real <= REDUCE_CHUNK_TOKENS,
            "REDUCE pass {i} was handed {real} tokens against a {REDUCE_CHUNK_TOKENS} budget"
        );
    }

    // The differential. Recompute what the naive join WOULD have sent — the same
    // MAP answers, concatenated — and show it does not fit. Without this the
    // assertion above is satisfied by any fixture that happened to be small.
    let joined: String = map
        .iter()
        .map(|req| narrative_for(&req.text))
        .collect::<Vec<_>>()
        .join("\n\n");
    let joined_tokens = meeting_token_count(&joined);
    assert!(
        joined_tokens > REDUCE_CHUNK_TOKENS,
        "the fixture's narratives join to {joined_tokens} tokens, inside the \
         {REDUCE_CHUNK_TOKENS} budget — it no longer distinguishes a fold from a join"
    );

    // A fold, not one oversized request: more passes than groups fit at once.
    assert!(
        reduce.len() > 1,
        "{} narratives went into a single REDUCE request",
        map.len()
    );

    // And because it fitted, nothing had to be dropped — the summary makes no
    // coverage disclaimer, because there is nothing to disclaim.
    assert!(!summary.truncated, "a folded REDUCE fits and cuts nothing");
    assert_eq!(summary.truncated_chunks, 0);
    assert!(!summary.markdown.contains(COVERAGE_NOTE));
    assert!(summary.narrative.split_whitespace().count() <= MAX_NARRATIVE_WORDS);
    assert!(!summary.narrative.trim().is_empty());

    println!(
        "summarize_reduce_budget: {} MAP passes → {} folded REDUCE passes; \
         the naive join would have been {joined_tokens} tokens against {REDUCE_CHUNK_TOKENS}",
        map.len(),
        reduce.len()
    );
}

#[test]
fn a_truncated_map_pass_is_rendered_not_only_logged() {
    let segments = long_meeting(200);
    // The sidecar answers, but says it had to cut the prompt to fit.
    let model = StubModel::truncating(|req| {
        if req.mode == SUMMARIZE_REDUCE {
            return Ok(Generated::whole(
                req.text
                    .split_whitespace()
                    .take(120)
                    .collect::<Vec<_>>()
                    .join(" "),
            ));
        }
        Ok(Generated {
            text: map_answer(&narrative_for(&req.text), &[], &[], &[]),
            truncated: true,
        })
    });

    let summary = summarize_segments(&segments, &model).expect("a summary");
    assert!(
        summary.truncated,
        "the sidecar's truncated:true must survive into the summary"
    );
    // The stored Markdown is the only thing a user ever reads.
    assert!(
        summary.markdown.contains(COVERAGE_NOTE),
        "the summary must say it may not cover the whole meeting:\n{}",
        summary.markdown
    );
    // Distinct from the existing input-line note, which counts something else.
    assert_eq!(
        summary.truncated_chunks, 0,
        "no transcript LINE was over-long here — this is the model's own cut"
    );
    assert!(!summary.markdown.contains("were too long for one pass"));
}

#[test]
fn a_truncated_reduce_pass_is_rendered_too() {
    let segments = long_meeting(200);
    let model = StubModel::truncating(|req| {
        if req.mode == SUMMARIZE_REDUCE {
            return Ok(Generated {
                text: req
                    .text
                    .split_whitespace()
                    .take(120)
                    .collect::<Vec<_>>()
                    .join(" "),
                truncated: true,
            });
        }
        Ok(Generated::whole(map_answer(
            &narrative_for(&req.text),
            &[],
            &[],
            &[],
        )))
    });

    let summary = summarize_segments(&segments, &model).expect("a summary");
    assert!(summary.truncated);
    assert!(summary.markdown.contains(COVERAGE_NOTE));
}

#[test]
fn a_single_chunk_meeting_still_skips_reduce_entirely() {
    // The cheap path must survive the fold: nothing to merge, no model call, no
    // disclaimer.
    let segments = long_meeting(3);
    let model = scripted();
    let summary = summarize_segments(&segments, &model).expect("a summary");
    assert_eq!(summary.chunks, 1);
    assert!(model.stage(SUMMARIZE_REDUCE).is_empty());
    assert!(!summary.truncated);
    assert!(!summary.narrative.trim().is_empty());
}
