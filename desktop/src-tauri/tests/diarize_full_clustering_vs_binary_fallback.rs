//! YV126 acceptance — the two target modes, and what each one can be asked for.
//!
//! ```sh
//! cargo test --test diarize_full_clustering_vs_binary_fallback
//! ```
//!
//! Merged finding #5, as a mechanism rather than a mood: pyannote-segmentation-3.0
//! caps at 3 speakers per 10 s window and 2 simultaneous, and sherpa's pipeline
//! DELETES every overlapped frame before embedding. A six-person far-field
//! classroom exceeds that by construction. The fix the audit names is not a
//! better threshold — no threshold recovers frames that were deleted — it is a
//! smaller question: "is this the enrolled voice, or is it not", which is
//! achievable far-field and is what a student wants from a lecture recording.
//!
//! What this file proves is the COLLAPSE: a six-cluster raw pass becomes exactly
//! two labels, with the turn boundaries untouched. What it deliberately does not
//! prove is that binary mode scores better on real audio — that is a
//! measurement, it belongs in `meeting_eval.rs` against fixture (f), and it is
//! blocked on a sidecar with an inference backend in it (YV122). See
//! `meeting_eval::fixture_f_binary_fallback_der`.

mod support;

use std::path::Path;

use support::diarize_stub::{stub_returning, StubTurn};
use wilson_voice_lib::diarize::{
    cluster_track, collapse_to_enrolled_vs_everyone_else, DiarizePool, DiarizedSegment,
    EnrolledSpeaker, MeetingTracks, TargetMode, ENROLLED_CLUSTER, EVERYONE_ELSE_CLUSTER,
};
use wilson_voice_lib::diarize_metrics::CosineDistance;
use wilson_voice_lib::meetings::{MeetingKind, MIC_TRACK, SYSTEM_TRACK};

const READY: std::time::Duration = std::time::Duration::from_secs(10);
/// Wide enough that the two turns of one voice always merge, tight enough that
/// two different voices never do. Not a tuned number and not shipped as one: it
/// is a property of the synthetic geometry below, which
/// `assert_the_fixture_really_holds_six_voices` re-measures.
const SEPARATING: f32 = 0.30;

/// Six voices, two turns each — a six-cluster raw pass, by construction.
///
/// One dimension per speaker (a 6-dimensional basis) rather than six angles on
/// a circle: angles WRAP, and a first draft of this fixture spaced six voices
/// 0.60 apart around 2π, which put the sixth 0.12 from the first and produced
/// five clusters. Orthogonal axes cannot wrap — every cross-speaker pair is
/// ~1.0 apart no matter how many speakers the fixture grows to.
///
/// `assert_the_fixture_really_holds_six_voices` re-measures that claim rather
/// than trusting this comment.
fn six_speakers() -> Vec<StubTurn> {
    let mut turns = Vec::new();
    for speaker in 0..6usize {
        for turn in 0..2 {
            let mut embedding = vec![0.0f32; 6];
            embedding[speaker] = 1.0;
            // A wobble WITHIN the speaker: ~0.01 in cosine distance, far inside
            // the threshold, so a clusterer that split on noise would fail here.
            if turn == 1 {
                embedding[(speaker + 1) % 6] = 0.14;
            }
            let start = (speaker as f64 * 2.0 + turn as f64) * 10.0;
            turns.push(StubTurn::new(start, start + 8.0, speaker as u32, embedding));
        }
    }
    turns
}

/// The fixture is what it claims to be: two turns of each of six voices, with
/// every within-speaker pair inside the threshold and every cross-speaker pair
/// well outside it. A fixture that quietly held five voices would make the
/// headline assertion below pass for the wrong reason.
#[test]
fn assert_the_fixture_really_holds_six_voices() {
    let turns = six_speakers();
    assert_eq!(turns.len(), 12);
    let threshold = f64::from(SEPARATING);
    for (i, a) in turns.iter().enumerate() {
        for (j, b) in turns.iter().enumerate().skip(i + 1) {
            let distance = f64::from(
                CosineDistance::from_similarity(wilson_voice_lib::diarize_metrics::cosine_similarity(
                    &a.embedding,
                    &b.embedding,
                ))
                .get(),
            );
            let same_speaker = i / 2 == j / 2;
            if same_speaker {
                assert!(distance < threshold, "turns {i},{j}: {distance:.3} apart");
            } else {
                assert!(distance > threshold, "turns {i},{j}: {distance:.3} apart");
            }
        }
    }
}

fn distinct_labels(segments: &[DiarizedSegment]) -> Vec<i64> {
    let mut ids: Vec<i64> = segments.iter().map(|s| s.cluster_index).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The table the acceptance criterion asks for: the same raw pass, both modes.
#[test]
fn the_same_six_cluster_pass_is_six_labels_or_exactly_two() {
    let turns = six_speakers();
    let mic = Path::new("/tmp/does-not-need-to-exist.wav");

    for (mode, expected_labels, note) in [
        (
            TargetMode::FullClustering,
            6usize,
            "full clustering keeps every voice it found",
        ),
        (
            TargetMode::EnrolledVsEveryoneElse(EnrolledSpeaker::matched(42, 3)),
            2,
            "binary mode is the enrolled voice and everybody else — no more",
        ),
    ] {
        let pool = DiarizePool::new(stub_returning(turns.clone()), READY);
        let out = cluster_track(
            &pool,
            MeetingTracks {
                mic_wav: mic,
                system_wav: None,
            },
            MeetingKind::InPerson,
            CosineDistance::new(SEPARATING),
            mode,
        )
        .expect("the stub answers");
        pool.shutdown();

        assert_eq!(out.len(), turns.len(), "{note}: no turn is dropped");
        assert_eq!(
            distinct_labels(&out).len(),
            expected_labels,
            "{note}; got {:?}",
            distinct_labels(&out)
        );
        // Boundaries survive both modes. The segmentation model can find WHERE
        // the speaker changed far-field; it is telling six far-field voices
        // apart that it cannot do, so collapsing the labels may not re-cut the
        // timeline.
        for (turn, segment) in turns.iter().zip(&out) {
            assert!((turn.start - segment.start_seconds).abs() < 1e-6);
            assert!((turn.end - segment.end_seconds).abs() < 1e-6);
        }
    }
}

/// Binary mode's two labels are the DOCUMENTED two, and the enrolled cluster is
/// the one the caller named — not the biggest, not the first.
#[test]
fn the_enrolled_cluster_is_the_one_the_caller_named() {
    let turns = six_speakers();
    let mic = Path::new("/tmp/does-not-need-to-exist.wav");

    // Cluster ids are assigned in first-appearance order, so speaker `n`'s turns
    // are cluster `n`. Enrol the FOURTH voice: a mode that quietly used "the
    // first cluster" or "the loudest" would put the wrong turns in
    // `ENROLLED_CLUSTER` and this would catch it.
    let pool = DiarizePool::new(stub_returning(turns.clone()), READY);
    let out = cluster_track(
        &pool,
        MeetingTracks {
            mic_wav: mic,
            system_wav: None,
        },
        MeetingKind::InPerson,
        CosineDistance::new(SEPARATING),
        TargetMode::EnrolledVsEveryoneElse(EnrolledSpeaker::matched(7, 3)),
    )
    .expect("the stub answers");
    pool.shutdown();

    assert_eq!(distinct_labels(&out), vec![ENROLLED_CLUSTER, EVERYONE_ELSE_CLUSTER]);
    let enrolled: Vec<f64> = out
        .iter()
        .filter(|s| s.cluster_index == ENROLLED_CLUSTER)
        .map(|s| s.start_seconds)
        .collect();
    assert_eq!(
        enrolled,
        vec![60.0, 70.0],
        "the enrolled label must sit on the fourth speaker's two turns"
    );
}

/// The collapse is a pure function and behaves at its edges: a cluster nobody
/// matched leaves one label (everyone else), and an empty pass stays empty. The
/// acceptance says "exactly 2 labels" for a six-cluster input — it must not be
/// read as "always fabricate two".
#[test]
fn the_collapse_never_invents_a_speaker_who_was_not_there() {
    let segments: Vec<DiarizedSegment> = (0..6)
        .map(|i| DiarizedSegment::new(MIC_TRACK, i as f64 * 10.0, i as f64 * 10.0 + 8.0, i))
        .collect();

    let matched = collapse_to_enrolled_vs_everyone_else(&segments, 2);
    assert_eq!(distinct_labels(&matched), vec![ENROLLED_CLUSTER, EVERYONE_ELSE_CLUSTER]);

    // A cluster index no turn carries: everything is "everyone else", and
    // nothing is relabelled into an enrolled bucket that had no evidence.
    let unmatched = collapse_to_enrolled_vs_everyone_else(&segments, 99);
    assert_eq!(distinct_labels(&unmatched), vec![EVERYONE_ELSE_CLUSTER]);

    assert!(collapse_to_enrolled_vs_everyone_else(&[], 0).is_empty());
}

/// The kind branch decides WHICH track gets clustered, and binary mode does not
/// change it. A virtual meeting with a live tap clusters Track B — the mixed
/// stream of every remote participant — because Track A is one voice by
/// mechanism there (YV125's `MicIsMe`), and every other configuration clusters
/// Track A.
#[test]
fn the_kind_branch_picks_the_track_and_the_mode_does_not_override_it() {
    let turns = six_speakers();
    let mic = Path::new("/tmp/mic.wav");
    let sys = Path::new("/tmp/sys.wav");

    for (kind, system_wav, expected_track, note) in [
        (MeetingKind::InPerson, None, MIC_TRACK, "in person: the mic carries the room"),
        (
            MeetingKind::Unknown,
            Some(sys),
            MIC_TRACK,
            "unknown, even WITH a tap: a tap proves audio left the speakers, \
             never that the room behind the microphone is empty",
        ),
        (
            MeetingKind::Virtual,
            Some(sys),
            SYSTEM_TRACK,
            "virtual with a live tap: the mic is one voice, the call is not",
        ),
        (
            MeetingKind::Virtual,
            None,
            MIC_TRACK,
            "virtual whose tap never delivered: acoustically the in-person case",
        ),
    ] {
        let pool = DiarizePool::new(stub_returning(turns.clone()), READY);
        let out = cluster_track(
            &pool,
            MeetingTracks {
                mic_wav: mic,
                system_wav,
            },
            kind,
            CosineDistance::new(SEPARATING),
            TargetMode::FullClustering,
        )
        .expect("the stub answers");
        pool.shutdown();
        assert!(
            out.iter().all(|s| s.track == expected_track),
            "{note}: expected track {expected_track}, got {:?}",
            out.first().map(|s| s.track)
        );
    }
}
