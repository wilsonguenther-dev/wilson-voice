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
//! What this file proves is that the smaller question is genuinely SMALLER: a
//! six-cluster raw pass becomes exactly two labels, the turn boundaries are
//! untouched, and — the load-bearing one —
//! `binary_mode_is_not_bounded_by_what_the_n_way_pass_did` shows the answer is
//! the same whether the clustering pass merged all twelve turns into one
//! cluster or split them into twelve, because binary mode never looks at a
//! cluster id. An earlier cut of this item DID relabel cluster ids, which made
//! it a post-hoc rename of the very pass it exists to replace; that test is the
//! one that would have caught it.
//!
//! What this file deliberately does not prove is that binary mode scores better
//! on real audio — that is a measurement, it belongs in `meeting_eval.rs`
//! against fixture (f), and it is blocked on a sidecar with an inference
//! backend in it (YV122). The 0.1524 figure quoted around this item is a
//! CEILING from ground truth, not a score anything here has reached. See
//! `meeting_eval::fixture_f_binary_fallback_der`.

mod support;

use std::path::Path;

use support::diarize_stub::{stub_returning, stub_with_body, StubTurn};
use wilson_voice_lib::diarize::{
    cluster_track, label_against_enrolled, DiarizeError, DiarizePool, DiarizedSegment,
    EnrolledSpeaker, MeetingTracks, TargetMode, ENROLLED_CLUSTER, ERR_NO_EMBEDDINGS,
    EVERYONE_ELSE_CLUSTER,
};
use wilson_voice_lib::diarize_metrics::{cosine_similarity, CosineDistance, CosineSimilarity};
use wilson_voice_lib::meetings::{MeetingKind, MIC_TRACK, SYSTEM_TRACK};

const READY: std::time::Duration = std::time::Duration::from_secs(10);
/// Wide enough that the two turns of one voice always merge, tight enough that
/// two different voices never do. Not a tuned number and not shipped as one: it
/// is a property of the synthetic geometry below, which
/// `assert_the_fixture_really_holds_six_voices` re-measures.
const SEPARATING: f32 = 0.30;
/// The acceptance band the *caller* hands binary mode in these tests. Also not
/// a tuned number and also not shipped as one — YV129 measures the real band
/// against YV120's `enrollment_eer`. Here it is chosen from the fixture's own
/// geometry, which `assert_the_fixture_really_holds_six_voices` re-measures:
/// a voice matches itself at ≥0.99 and matches every other voice at ≤0.14.
const ACCEPTING: f32 = 0.70;
/// Which of the six synthetic voices is enrolled: the FOURTH. Not the first and
/// not the loudest, so a mode that quietly used "cluster 0" or "the biggest
/// cluster" would put the wrong turns in `ENROLLED_CLUSTER`.
const ENROLLED_VOICE: usize = 3;

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

/// The enrolled profile for `ENROLLED_VOICE`, as a caller would supply it: a
/// centroid (the mean of that voice's own turns, L2-normalised, which is what
/// YV128's `speaker_profiles` will store) and an acceptance band.
fn enrolled_profile() -> EnrolledSpeaker {
    let turns = six_speakers();
    let mine: Vec<&StubTurn> = turns
        .iter()
        .enumerate()
        .filter(|(i, _)| i / 2 == ENROLLED_VOICE)
        .map(|(_, t)| t)
        .collect();
    let dim = mine[0].embedding.len();
    let mut centroid = vec![0.0f32; dim];
    for turn in &mine {
        for (slot, value) in centroid.iter_mut().zip(&turn.embedding) {
            *slot += value / mine.len() as f32;
        }
    }
    let norm = centroid.iter().map(|v| v * v).sum::<f32>().sqrt();
    for slot in centroid.iter_mut() {
        *slot /= norm;
    }
    EnrolledSpeaker::new(7, centroid, CosineSimilarity::new(ACCEPTING))
}

/// The two turns of the enrolled voice, by start time.
fn enrolled_turn_starts() -> Vec<f64> {
    six_speakers()
        .iter()
        .enumerate()
        .filter(|(i, _)| i / 2 == ENROLLED_VOICE)
        .map(|(_, t)| t.start)
        .collect()
}

/// The fixture is what it claims to be: two turns of each of six voices, with
/// every within-speaker pair inside the threshold and every cross-speaker pair
/// well outside it — AND the enrolled centroid separating the same way in the
/// similarity unit binary mode actually uses. A fixture that quietly held five
/// voices, or whose centroid sat between two of them, would make the headline
/// assertions below pass for the wrong reason.
#[test]
fn assert_the_fixture_really_holds_six_voices() {
    let turns = six_speakers();
    assert_eq!(turns.len(), 12);
    let threshold = f64::from(SEPARATING);
    for (i, a) in turns.iter().enumerate() {
        for (j, b) in turns.iter().enumerate().skip(i + 1) {
            let distance = f64::from(
                CosineDistance::from_similarity(cosine_similarity(&a.embedding, &b.embedding))
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

    // The centroid separates in SIMILARITY, which is the unit binary mode
    // decides in — the distance spread above says nothing about it on its own.
    let profile = enrolled_profile();
    for (i, turn) in turns.iter().enumerate() {
        let similarity = f64::from(cosine_similarity(&turn.embedding, &profile.centroid).get());
        if i / 2 == ENROLLED_VOICE {
            assert!(
                similarity > 0.99,
                "turn {i} is the enrolled voice but only {similarity:.3} from its own centroid"
            );
        } else {
            assert!(
                similarity < 0.15,
                "turn {i} is a different voice but sits {similarity:.3} from the enrolled \
                 centroid — the band below would be separating nothing"
            );
        }
    }
    assert!(
        (0.15..0.99).contains(&f64::from(ACCEPTING)),
        "the acceptance band must sit strictly inside the fixture's own gap, or these \
         tests pass regardless of what the code does"
    );
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
            TargetMode::EnrolledVsEveryoneElse(enrolled_profile()),
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
        // apart that it cannot do, so answering the smaller question may not
        // re-cut the timeline.
        for (turn, segment) in turns.iter().zip(&out) {
            assert!((turn.start - segment.start_seconds).abs() < 1e-6);
            assert!((turn.end - segment.end_seconds).abs() < 1e-6);
        }
    }
}

/// Binary mode's two labels are the DOCUMENTED two, and the enrolled turns are
/// the ones that SOUND like the enrolled profile — not the first cluster, not
/// the biggest.
#[test]
fn the_enrolled_label_lands_on_the_voice_the_profile_describes() {
    let turns = six_speakers();
    let mic = Path::new("/tmp/does-not-need-to-exist.wav");

    let pool = DiarizePool::new(stub_returning(turns.clone()), READY);
    let out = cluster_track(
        &pool,
        MeetingTracks {
            mic_wav: mic,
            system_wav: None,
        },
        MeetingKind::InPerson,
        CosineDistance::new(SEPARATING),
        TargetMode::EnrolledVsEveryoneElse(enrolled_profile()),
    )
    .expect("the stub answers");
    pool.shutdown();

    assert_eq!(
        distinct_labels(&out),
        vec![ENROLLED_CLUSTER, EVERYONE_ELSE_CLUSTER]
    );
    let enrolled: Vec<f64> = out
        .iter()
        .filter(|s| s.cluster_index == ENROLLED_CLUSTER)
        .map(|s| s.start_seconds)
        .collect();
    assert_eq!(
        enrolled,
        enrolled_turn_starts(),
        "the enrolled label must sit on the enrolled voice's two turns"
    );
}

/// **The load-bearing test of finding #5's reframe.**
///
/// If binary mode were the N-way pass with its labels rewritten, its answer
/// would move whenever the clustering moved — and clustering on a six-person
/// far-field room is exactly what the mechanism ceiling says is unreliable, so
/// the "smaller question" would be no smaller at all. Here the SAME twelve
/// turns are clustered three ways by choosing three absurd distance
/// thresholds: 1.5 merges every voice into one cluster (the fixture's voices
/// are orthogonal, so their pairwise distance is exactly 1.0 and complete
/// linkage needs a threshold above it), 0.001 splits each of the twelve turns
/// into its own, and `SEPARATING` gets it right. Binary mode returns the
/// identical two labels on the identical two turns in all three.
///
/// Under the relabel design this test is unsatisfiable: at 1.5 there is only
/// cluster 0 to name, so the enrolled cluster is either everybody or nobody.
#[test]
fn binary_mode_is_not_bounded_by_what_the_n_way_pass_did() {
    let turns = six_speakers();
    let mic = Path::new("/tmp/does-not-need-to-exist.wav");

    for (distance, expected_n_way_clusters, note) in [
        (1.5f32, 1usize, "everything in one cluster"),
        (0.001, 12, "every turn its own cluster"),
        (SEPARATING, 6, "the clustering that actually works"),
    ] {
        // First: prove the N-way pass really did collapse/shatter, so the arm
        // below is not passing because all three thresholds behaved the same.
        let pool = DiarizePool::new(stub_returning(turns.clone()), READY);
        let n_way = cluster_track(
            &pool,
            MeetingTracks {
                mic_wav: mic,
                system_wav: None,
            },
            MeetingKind::InPerson,
            CosineDistance::new(distance),
            TargetMode::FullClustering,
        )
        .expect("the stub answers");
        pool.shutdown();
        assert_eq!(
            distinct_labels(&n_way).len(),
            expected_n_way_clusters,
            "{note}: the N-way pass did not do what this arm needs it to do"
        );

        // Then: binary mode, same turns, same threshold, unmoved.
        let pool = DiarizePool::new(stub_returning(turns.clone()), READY);
        let binary = cluster_track(
            &pool,
            MeetingTracks {
                mic_wav: mic,
                system_wav: None,
            },
            MeetingKind::InPerson,
            CosineDistance::new(distance),
            TargetMode::EnrolledVsEveryoneElse(enrolled_profile()),
        )
        .expect("the stub answers");
        pool.shutdown();

        let enrolled: Vec<f64> = binary
            .iter()
            .filter(|s| s.cluster_index == ENROLLED_CLUSTER)
            .map(|s| s.start_seconds)
            .collect();
        assert_eq!(
            enrolled,
            enrolled_turn_starts(),
            "{note}: binary mode moved with the clustering, so it is reading cluster ids"
        );
        assert_eq!(
            distinct_labels(&binary),
            vec![ENROLLED_CLUSTER, EVERYONE_ELSE_CLUSTER],
            "{note}"
        );
    }
}

/// Binary mode is a decision per turn, and it behaves at its edges: a profile
/// nobody in the room matches leaves one label (everyone else), and an empty
/// pass stays empty. The acceptance says "exactly 2 labels" for a six-cluster
/// input — it must not be read as "always fabricate two".
#[test]
fn binary_mode_never_invents_a_speaker_who_was_not_there() {
    let raw: Vec<wilson_voice_lib::diarize_protocol::DiarizeSegment> = six_speakers()
        .iter()
        .map(|t| wilson_voice_lib::diarize_protocol::DiarizeSegment {
            start: t.start,
            end: t.end,
            cluster: t.child_cluster,
            embedding: t.embedding.clone(),
        })
        .collect();

    let matched = label_against_enrolled(MIC_TRACK, &raw, &enrolled_profile()).expect("embeddings");
    assert_eq!(
        distinct_labels(&matched),
        vec![ENROLLED_CLUSTER, EVERYONE_ELSE_CLUSTER]
    );

    // A voice nobody in the room is. Six orthogonal axes are all taken by the
    // six fixture voices, so the stranger is the bisector of two of them: it
    // sits ~0.707 from each of those, ~0.0 from the other four, and therefore
    // under any band above 0.71. `assert_the_stranger_really_is_a_stranger`
    // below re-measures that instead of trusting this comment.
    let mut mixed = vec![0.0f32; 6];
    mixed[0] = std::f32::consts::FRAC_1_SQRT_2;
    mixed[1] = std::f32::consts::FRAC_1_SQRT_2;
    let stranger = EnrolledSpeaker::new(99, mixed, CosineSimilarity::new(0.95));
    for turn in &raw {
        assert!(
            cosine_similarity(&turn.embedding, &stranger.centroid).get() < 0.95,
            "the stranger is not a stranger — this arm would pass vacuously"
        );
    }
    let unmatched = label_against_enrolled(MIC_TRACK, &raw, &stranger).expect("embeddings");
    assert_eq!(distinct_labels(&unmatched), vec![EVERYONE_ELSE_CLUSTER]);

    assert!(label_against_enrolled(MIC_TRACK, &[], &enrolled_profile())
        .expect("an empty pass is not an error")
        .is_empty());
}

/// Without embeddings there is nothing to compare, and binary mode says so
/// instead of guessing.
///
/// Full clustering has an honest degraded path here — it forwards the child's
/// own cluster ids and logs that it did — because "these turns group together"
/// survives without the parent's arithmetic. "This turn is Samantha" does not:
/// a cluster id carries no identity, so keying the enrolled class off one would
/// be a guess wearing a name. The refusal is what lets the caller leave the
/// transcript unattributed.
#[test]
fn binary_mode_refuses_when_there_is_nothing_to_compare() {
    let mic = Path::new("/tmp/does-not-need-to-exist.wav");
    // A body no `StubTurn` can express: turns with no embedding field at all,
    // which is what a backend that clusters internally would send.
    let body = r#"{"start":0.0,"end":8.0,"cluster":0},{"start":10.0,"end":18.0,"cluster":1}"#;

    let pool = DiarizePool::new(stub_with_body(body.to_string()), READY);
    let refused = cluster_track(
        &pool,
        MeetingTracks {
            mic_wav: mic,
            system_wav: None,
        },
        MeetingKind::InPerson,
        CosineDistance::new(SEPARATING),
        TargetMode::EnrolledVsEveryoneElse(enrolled_profile()),
    );
    pool.shutdown();
    assert_eq!(
        refused,
        Err(DiarizeError::Refused(ERR_NO_EMBEDDINGS.to_string())),
        "binary mode must refuse, not fall back to cluster ids"
    );

    // The same pass under full clustering is NOT an error — it degrades, and
    // that asymmetry is the point.
    let pool = DiarizePool::new(stub_with_body(body.to_string()), READY);
    let degraded = cluster_track(
        &pool,
        MeetingTracks {
            mic_wav: mic,
            system_wav: None,
        },
        MeetingKind::InPerson,
        CosineDistance::new(SEPARATING),
        TargetMode::FullClustering,
    )
    .expect("full clustering forwards the child's ids");
    pool.shutdown();
    assert_eq!(distinct_labels(&degraded), vec![0, 1]);

    // An empty centroid is the same refusal from the other side: a caller that
    // has a profile row but no voice for it yet.
    let empty = EnrolledSpeaker::new(1, Vec::new(), CosineSimilarity::new(ACCEPTING));
    assert_eq!(
        label_against_enrolled(MIC_TRACK, &[], &empty),
        Err(DiarizeError::Refused(ERR_NO_EMBEDDINGS.to_string()))
    );
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
        (
            MeetingKind::InPerson,
            None,
            MIC_TRACK,
            "in person: the mic carries the room",
        ),
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
        // Both modes, because the track choice is the kind branch's and neither
        // mode may reach into it.
        for mode in [
            TargetMode::FullClustering,
            TargetMode::EnrolledVsEveryoneElse(enrolled_profile()),
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
                mode,
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
}
