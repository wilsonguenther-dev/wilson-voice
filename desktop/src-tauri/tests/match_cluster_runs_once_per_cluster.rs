//! YV129 — the identity question is asked once per CLUSTER, never once per
//! utterance, turn or segment.
//!
//! This is the audit's explicit demand ("the confidence threshold must not spam
//! a 6-person class") turned into a number a machine checks. The fixture below
//! is a six-speaker classroom recording in the shape fixture (f) has: six
//! clusters carrying forty-eight segments between them. A matcher that ran per
//! segment would be called forty-eight times and would put dozens of "who is
//! this?" prompts on the screen for a recording with six people in it.
//!
//! **Why the count is instrumented rather than inferred from the output.**
//! `match_meeting_clusters` returns one decision per cluster whatever it does
//! inside — an implementation that matched every segment and took a vote would
//! return the same six decisions and pass a test that only looked at the answer.
//! So `match_meeting_clusters_with` takes the per-cluster matcher as a
//! parameter, and this file passes one that increments a counter. The seam
//! exists for this test, and its doc comment says so.

use std::cell::Cell;

use wilson_voice_lib::diarize_metrics::CosineSimilarity;
use wilson_voice_lib::speaker_profiles::{
    match_cluster, match_meeting_clusters, match_meeting_clusters_with, ClusterSummary, Embedding,
    EnrollmentBands, MatchResult, SpeakerProfile,
};

/// Six speakers, forty-eight segments — the shape of a real classroom pass
/// after YV126's clustering has run and before any name has been attached.
const SPEAKERS: usize = 6;
const SEGMENTS: usize = 48;

/// A unit vector pointing along axis `k` of a six-dimensional space: six
/// mutually orthogonal "voices", which is all this test needs from an
/// embedding. Real CAM++ vectors are 192-dimensional; the count is what is
/// under test here, not the geometry.
fn voice(k: usize) -> Embedding {
    let mut v = vec![0.0f32; SPEAKERS];
    v[k] = 1.0;
    Embedding::new(v)
}

/// The forty-eight segments, each already attributed to one of six clusters —
/// what `meeting_segments.cluster_index` holds once YV126 has written it.
fn segments() -> Vec<(usize, f64)> {
    (0..SEGMENTS)
        .map(|i| (i % SPEAKERS, 4.0 + (i % 5) as f64))
        .collect()
}

/// Fold the segments into six clusters, the way the diarization pass hands them
/// to the matcher.
fn clusters() -> Vec<ClusterSummary> {
    let segs = segments();
    (0..SPEAKERS)
        .map(|k| {
            let mine: Vec<&(usize, f64)> = segs.iter().filter(|(c, _)| *c == k).collect();
            ClusterSummary {
                cluster_index: k as i64,
                label: format!("Speaker {}", k + 1),
                centroid: voice(k),
                speech_seconds: mine.iter().map(|(_, d)| *d).sum(),
                turns: mine.len(),
            }
        })
        .collect()
}

fn bands() -> EnrollmentBands {
    EnrollmentBands::for_test(CosineSimilarity::new(0.90), CosineSimilarity::new(0.50))
        .expect("well-ordered test bands")
}

/// The acceptance criterion: six clusters, six calls.
#[test]
fn match_cluster_runs_once_per_cluster() {
    let clusters = clusters();
    assert_eq!(clusters.len(), SPEAKERS);
    assert_eq!(
        segments().len(),
        SEGMENTS,
        "the fixture must actually contain dozens of segments, or `6` is not a \
         smaller number than anything"
    );

    let calls = Cell::new(0usize);
    let decisions = match_meeting_clusters_with(&clusters, |centroid| {
        calls.set(calls.get() + 1);
        match_cluster(centroid, &[], bands())
    });

    assert_eq!(
        calls.get(),
        SPEAKERS,
        "the matcher ran {} times over {SPEAKERS} clusters — a six-person class \
         must cost six questions, not one per utterance",
        calls.get()
    );
    assert_eq!(decisions.len(), SPEAKERS);

    // The number this is smaller THAN, asserted rather than implied: a
    // per-segment matcher over the same meeting runs eight times as often.
    let per_segment = Cell::new(0usize);
    for (cluster_index, _) in segments() {
        per_segment.set(per_segment.get() + 1);
        let _ = match_cluster(&voice(cluster_index), &[], bands());
    }
    assert_eq!(per_segment.get(), SEGMENTS);
    assert!(
        calls.get() * 8 == per_segment.get(),
        "the fixture's whole point is the ratio: {} cluster calls against {} \
         segments",
        calls.get(),
        per_segment.get()
    );
}

/// The shipped entry point delegates to the counted one, so the count above is
/// a fact about what the app does and not about a test-only path.
#[test]
fn the_shipped_entry_point_is_the_one_that_was_counted() {
    let clusters = clusters();
    let roster = [SpeakerProfile {
        id: "p1".into(),
        display_name: "Jeisil".into(),
        is_me: false,
        centroids: vec![wilson_voice_lib::speaker_profiles::Centroid::new(
            "laptop_mic_near",
            voice(2),
        )],
    }];

    let shipped = match_meeting_clusters(&clusters, &roster, bands());
    let counted =
        match_meeting_clusters_with(&clusters, |c| match_cluster(c, &roster, bands()));
    assert_eq!(shipped, counted);

    // And the one enrolled voice is found, exactly once, in the cluster that
    // carries it — not in the five that do not.
    let named: Vec<i64> = shipped
        .iter()
        .filter(|d| d.result.profile_id() == Some("p1"))
        .map(|d| d.cluster.cluster_index)
        .collect();
    assert_eq!(named, vec![2]);
    assert!(matches!(
        shipped[2].result,
        MatchResult::Known { ref profile_id, .. } if profile_id == "p1"
    ));
    for other in [0, 1, 3, 4, 5] {
        assert_eq!(
            shipped[other].result,
            MatchResult::New,
            "an orthogonal voice is nobody we know"
        );
    }
}

/// Every decision carries the cluster it came from, in order.
///
/// Without this, "one call per cluster" would be satisfiable by an
/// implementation that matched cluster 0 six times.
#[test]
fn each_decision_is_bound_to_its_own_cluster() {
    let clusters = clusters();
    let decisions = match_meeting_clusters(&clusters, &[], bands());
    let seen: Vec<i64> = decisions.iter().map(|d| d.cluster.cluster_index).collect();
    assert_eq!(seen, (0..SPEAKERS as i64).collect::<Vec<_>>());
    for (d, c) in decisions.iter().zip(clusters.iter()) {
        assert_eq!(&d.cluster, c, "a decision must not rewrite its cluster");
    }
}
