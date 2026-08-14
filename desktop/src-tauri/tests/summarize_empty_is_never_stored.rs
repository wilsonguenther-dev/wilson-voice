//! YV97 regression guard — an empty summary is never a summary, and never a
//! write.
//!
//! The defect this file pins, in the exact shape it was reproduced in review:
//! the model answers every MAP pass with output the grammar happily admits and
//! that carries NOTHING — `{"narrative":"","actions":[],"decisions":[],
//! "questions":[]}` — which is what a broken prompt, a wedged model, or a
//! context the transcript overflowed all look like from the outside. Every chunk
//! "succeeded", so the `extracts.is_empty()` guard added in round 2 never fired;
//! `merge_extracts` produced nothing, REDUCE narrated nothing, and
//! `render_summary` fell through to its honest placeholder
//! "_No summary could be produced for this meeting._" — which
//! `summarize_meeting_blocking` then wrote straight over `meetings.summary`.
//!
//! In a local app with no undo and no version history, re-summarizing a meeting
//! while the model was having a bad day DESTROYED the summary you already had,
//! and the UI reported success. Round 2 fixed the half of this where every MAP
//! pass ERRORED; this is the same data loss arriving through a legal answer
//! instead of a failed one, so the guard belongs on the CONTENT of the result,
//! not on how the calls went.
//!
//! The rule these tests hold the code to: a finished summary carrying no
//! narrative and no items is `Err`, and nothing on the storage path can turn it
//! into a row.

mod support;

use support::{map_answer, segments, StubModel};
use wilson_voice_lib::summarize::{summarize_segments, Generated, SummaryError};

/// Every chunk answers with a well-formed, entirely empty schema.
fn silent() -> StubModel {
    StubModel::new(|req| {
        if req.mode == "reduce" {
            Ok(String::new())
        } else {
            Ok(map_answer("", &[], &[], &[]))
        }
    })
}

/// A grammar-legal empty answer is a failure, not a summary.
///
/// Asserted as `Err` rather than as "the markdown is not the placeholder",
/// because the placeholder string is not the defect — RETURNING it as success
/// is. A `Result` the caller has to unwrap is the only version of this the
/// caller cannot ignore.
#[test]
fn a_grammar_legal_empty_map_answer_is_an_error_not_a_stored_summary() {
    let segments = segments();

    let err = summarize_segments(&segments, &silent())
        .expect_err("a summary carrying nothing is not a summary");
    assert_eq!(
        err.tag(),
        "empty",
        "the empty result reports itself as empty, not as a protocol failure"
    );

    // The two ways to get nothing stay distinguishable — that distinction is
    // what round 2's guard bought, and this fix must not collapse it: one says
    // the sidecar is broken, the other says it answered and said nothing.
    let dead = StubModel::new(|_| Err(SummaryError::Protocol));
    assert_eq!(
        summarize_segments(&segments, &dead)
            .expect_err("a dead sidecar is still an error")
            .tag(),
        "protocol"
    );
}

/// An answer whose only content is a truncation NOTE is still nothing.
///
/// Without this the guard would be trivially defeated: `render_summary` emits
/// "_Part of this meeting did not fit …_" whenever a pass was cut, so an empty
/// summary from a truncated run renders as non-empty markdown while carrying no
/// narrative and no items. Storing that over a good summary loses exactly as
/// much as storing the placeholder did.
#[test]
fn a_summary_whose_only_content_is_a_truncation_note_is_still_empty() {
    let truncating = StubModel::truncating(|req| {
        if req.mode == "reduce" {
            Ok(Generated {
                text: String::new(),
                truncated: true,
            })
        } else {
            Ok(Generated {
                text: map_answer("", &[], &[], &[]),
                truncated: true,
            })
        }
    });
    assert_eq!(
        summarize_segments(&segments(), &truncating)
            .expect_err("a truncation note is not a summary")
            .tag(),
        "empty"
    );
}

/// The guard is not over-broad: a thin summary is still a real one.
///
/// One action item and no narrative at all is a legitimate outcome for a short,
/// mostly-silent meeting, and it is worth storing — the failure this file
/// guards is "nothing", not "not much".
#[test]
fn a_summary_with_one_item_and_no_narrative_is_still_stored() {
    let sparse = StubModel::new(|req| {
        if req.mode == "reduce" {
            return Ok(String::new());
        }
        let labels = support::labels_in(&req.text);
        let bodies = support::bodies_in(&req.text);
        let clause: String = bodies[0]
            .split_whitespace()
            .take(9)
            .collect::<Vec<_>>()
            .join(" ");
        Ok(map_answer(
            "",
            &[(clause.as_str(), labels[0].as_str())],
            &[],
            &[],
        ))
    });
    let summary =
        summarize_segments(&segments(), &sparse).expect("one grounded item is a real summary");
    assert!(summary.narrative.trim().is_empty());
    assert_eq!(summary.actions.len(), 1);
    assert!(
        summary.markdown.contains("### Action items"),
        "the one item reaches the stored markdown"
    );
}

/// The storage path itself is gated, not merely the pipeline that feeds it.
///
/// A source tripwire, in the same spirit as `crash.rs`'s no-network scan: the
/// ONE place in the app that writes `meetings.summary` must refuse an empty
/// result before it writes, so a future caller that builds a `MeetingSummary`
/// some other way (a partial re-summarize, a resumed job) cannot reintroduce the
/// data loss by skipping `summarize_segments`' guard.
#[test]
fn the_only_write_of_meetings_summary_refuses_an_empty_result_first() {
    const LIB: &str = include_str!("../src/lib.rs");

    let writes: Vec<&str> = LIB
        .lines()
        .filter(|l| l.contains("set_meeting_summary(") && !l.trim_start().starts_with("///"))
        .collect();
    assert_eq!(
        writes.len(),
        1,
        "meetings.summary is written in exactly one place; found {writes:?}"
    );

    let body_start = LIB
        .find("fn summarize_meeting_blocking")
        .expect("the summary command's body");
    let body = &LIB[body_start..];
    let write_at = body
        .find("set_meeting_summary(")
        .expect("the write is in this function");
    let guard_at = body
        .find("summary.is_empty()")
        .expect("the write is gated on the summary carrying something");
    assert!(
        guard_at < write_at,
        "the empty-result refusal has to come BEFORE the write, or it guards nothing"
    );
}
