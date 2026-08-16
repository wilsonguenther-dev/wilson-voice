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
//! on real audio — that is a measurement and it belongs in `meeting_eval.rs`
//! against fixture (f), where `fixture_f_binary_fallback_der` now makes it
//! (DER 0.3489 at clustering distance 0.80, acceptance band 0.75). The 0.1524
//! figure quoted around this item is a CEILING from ground truth and not a
//! score anything reaches.
//!
//! And what this file proves about independence is the PARENT's half only. The
//! child clusters in order to segment, so the distance moves the turn set and
//! binary mode inherits whatever merges that causes;
//! `the_childs_segmentation_moves_with_the_distance_and_binary_mode_inherits_it`
//! is that half, and a review was right that an earlier revision stated the
//! independence without it.

mod support;

use std::path::Path;

use support::diarize_stub::{
    stub_returning, stub_segmenting_by_distance, stub_with_body, StubTurn, STUB_MIN_EMBED,
};
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
    let mut ids: Vec<i64> = segments.iter().filter_map(|s| s.cluster_index).collect();
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
            STUB_MIN_EMBED,
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
        STUB_MIN_EMBED,
    )
    .expect("the stub answers");
    pool.shutdown();

    assert_eq!(
        distinct_labels(&out),
        vec![ENROLLED_CLUSTER, EVERYONE_ELSE_CLUSTER]
    );
    let enrolled: Vec<f64> = out
        .iter()
        .filter(|s| s.cluster_index == Some(ENROLLED_CLUSTER))
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
///
/// **What this test deliberately holds still, and why that is a limit and not a
/// cheat.** The stub answers the same twelve turns at every distance, so the
/// only thing varying across the three arms is the PARENT's arithmetic — which
/// is the subject. The real child's turn set is not fixed: it clusters in order
/// to segment, so the distance moves the boundaries too, and binary mode
/// inherits whatever merges that produces. That half is a different claim with
/// a different test —
/// [`the_childs_segmentation_moves_with_the_distance_and_binary_mode_inherits_it`]
/// — and the two together are the honest statement. An earlier revision of this
/// item stated the independence without that qualifier, and a review was right
/// to call it an over-claim: a distance-invariant stub cannot see the child's
/// half, so this test could never have been the evidence for it.
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
            STUB_MIN_EMBED,
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
            STUB_MIN_EMBED,
        )
        .expect("the stub answers");
        pool.shutdown();

        let enrolled: Vec<f64> = binary
            .iter()
            .filter(|s| s.cluster_index == Some(ENROLLED_CLUSTER))
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
        STUB_MIN_EMBED,
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
        STUB_MIN_EMBED,
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
                STUB_MIN_EMBED,
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

/// **The other half of the reframe's honesty, and the one an earlier revision
/// of this item got wrong.**
///
/// `binary_mode_is_not_bounded_by_what_the_n_way_pass_did` proves the PARENT's
/// clustering decides nothing about binary mode's labels. It cannot prove that
/// binary mode is independent of the distance, because the distance also goes
/// to the child — and sherpa clusters in order to SEGMENT, so the turn set that
/// comes back moves with it. YV122's own
/// `a_two_voice_track_diarizes_and_a_tighter_distance_never_merges_more` prints
/// that movement on real audio.
///
/// The consequence is the thing that must not be quietly dropped: when a loose
/// distance merges two speakers' adjacent turns into ONE output segment, binary
/// mode gets one unit carrying two voices and puts one label on it. No per-turn
/// rule can undo that. So the error binary mode makes IS a function of the
/// distance, and the eval harness has to tune the distance for the 2-class task
/// rather than borrow full clustering's.
///
/// This drives the shipped `cluster_track` in binary mode against a stub that
/// segments the way the real child does — two turns at a tight distance, one
/// merged turn at a loose one — and asserts:
///
/// 1. the two arms really did come back with different turn sets (or the test
///    is vacuous, and a `cutoff` that never separated them would make it so);
/// 2. binary mode passed those boundaries through unchanged in both arms;
/// 3. the loose arm labels the merged unit ONCE, so one of the two voices in it
///    is attributed wrongly — the error class the old doc comment disclaimed.
#[test]
fn the_childs_segmentation_moves_with_the_distance_and_binary_mode_inherits_it() {
    let mic = Path::new("/tmp/does-not-need-to-exist.wav");
    let enrolled = enrolled_profile();
    let voice = |speaker: usize| {
        let mut embedding = vec![0.0f32; 6];
        embedding[speaker] = 1.0;
        embedding
    };

    // TIGHT: the enrolled voice's turn and a stranger's turn, separately.
    let tight = vec![
        StubTurn::new(0.0, 8.0, 0, voice(ENROLLED_VOICE)),
        StubTurn::new(8.0, 16.0, 1, voice(0)),
    ];
    // LOOSE: the same 16 seconds, but the child merged the two into one turn
    // and embedded the merged span — which, being two voices averaged, is
    // exactly what a merged embedding looks like.
    let merged: Vec<f32> = voice(ENROLLED_VOICE)
        .iter()
        .zip(voice(0).iter())
        .map(|(a, b)| (a + b) / 2.0)
        .collect();
    let loose = vec![StubTurn::new(0.0, 16.0, 0, merged)];

    let cutoff = 0.50f64;
    let run = |distance: f32| {
        let pool = DiarizePool::new(
            stub_segmenting_by_distance(loose.clone(), tight.clone(), cutoff),
            READY,
        );
        let out = cluster_track(
            &pool,
            MeetingTracks {
                mic_wav: mic,
                system_wav: None,
            },
            MeetingKind::InPerson,
            CosineDistance::new(distance),
            TargetMode::EnrolledVsEveryoneElse(enrolled.clone()),
            STUB_MIN_EMBED,
        )
        .expect("the stub answers");
        pool.shutdown();
        out
    };

    let tight_out = run(0.30);
    let loose_out = run(0.80);

    // 1 — non-vacuity: the child really did answer differently.
    let spans = |out: &[DiarizedSegment]| -> Vec<(f64, f64)> {
        out.iter()
            .map(|s| (s.start_seconds, s.end_seconds))
            .collect()
    };
    assert_ne!(
        spans(&tight_out),
        spans(&loose_out),
        "the stub answered the same turns at both distances — this test would \
         then be independent by construction, which is the exact defect it exists \
         to close"
    );

    // 2 — the boundaries are the CHILD's, passed through untouched.
    assert_eq!(spans(&tight_out), vec![(0.0, 8.0), (8.0, 16.0)]);
    assert_eq!(spans(&loose_out), vec![(0.0, 16.0)]);

    // 3 — and the merged unit gets exactly one label, so 8 of the 16 seconds
    // are attributed to the wrong class no matter what the band is.
    assert_eq!(
        tight_out
            .iter()
            .filter(|s| s.cluster_index == Some(ENROLLED_CLUSTER))
            .map(|s| s.duration())
            .sum::<f64>(),
        8.0,
        "at the tight distance the enrolled voice's 8 s is its own turn"
    );
    assert_eq!(
        loose_out.len(),
        1,
        "at the loose distance there is one unit and therefore one label — \
         binary mode cannot recover a boundary the child did not draw"
    );
    let loose_label = loose_out[0].cluster_index;
    assert!(
        loose_label == Some(ENROLLED_CLUSTER) || loose_label == Some(EVERYONE_ELSE_CLUSTER),
        "the merged unit still gets one of the two classes"
    );
    // Whichever class it landed in, 8 s of the other voice went with it.
    assert_eq!(
        loose_out[0].duration(),
        16.0,
        "8 s of the {} class is inside this one label — the error the distance \
         caused, which no acceptance band can undo",
        if loose_label == Some(ENROLLED_CLUSTER) {
            "everyone-else"
        } else {
            "enrolled"
        }
    );
}

/// **A turn nothing was measured on is `None`, not "everyone else".**
///
/// YV122's per-turn `min_embed` floor makes a partly-embedded pass the normal
/// case: a turn whose un-overlapped part is under the floor comes back from the
/// child with an EMPTY embedding, and on the corpus's own fixtures that is
/// several turns in every pass. Two wrong answers are available for such a turn
/// and this test rules out both.
///
/// Answering `EVERYONE_ELSE_CLUSTER` would be a claim — "this is not the
/// instructor" — about audio nobody compared to anything, and it is what falls
/// out of scoring an empty vector against the centroid (cosine of nothing lands
/// under any band). Refusing the whole pass would throw away the turns that DID
/// carry evidence. Both modes therefore answer per turn, and the unmeasured one
/// is `None`.
#[test]
fn a_turn_with_no_embedding_is_unattributed_rather_than_everyone_else() {
    let mic = Path::new("/tmp/does-not-need-to-exist.wav");
    let voice = |speaker: usize| {
        let mut embedding = vec![0.0f32; 6];
        embedding[speaker] = 1.0;
        embedding
    };
    // Three turns: the enrolled voice, a stranger, and one the child could not
    // embed (empty vector — what a sub-floor span comes back as).
    let turns = vec![
        StubTurn::new(0.0, 8.0, 0, voice(ENROLLED_VOICE)),
        StubTurn::new(8.0, 16.0, 1, voice(0)),
        StubTurn::new(16.0, 17.0, 2, Vec::new()),
    ];

    let pool = DiarizePool::new(stub_returning(turns.clone()), READY);
    let binary = cluster_track(
        &pool,
        MeetingTracks {
            mic_wav: mic,
            system_wav: None,
        },
        MeetingKind::InPerson,
        CosineDistance::new(SEPARATING),
        TargetMode::EnrolledVsEveryoneElse(enrolled_profile()),
        STUB_MIN_EMBED,
    )
    .expect("a pass with SOME evidence is answered, not refused");
    pool.shutdown();

    assert_eq!(
        binary
            .iter()
            .map(|s| s.cluster_index)
            .collect::<Vec<Option<i64>>>(),
        vec![Some(ENROLLED_CLUSTER), Some(EVERYONE_ELSE_CLUSTER), None],
        "the enrolled turn, the stranger's turn, and the turn nobody measured"
    );
    // The short turn is still THERE — it is real speech at a real time, and a
    // pass that dropped it would silently shorten the meeting.
    assert_eq!(binary.len(), 3);
    assert_eq!(binary[2].start_seconds, 16.0);

    // Full clustering answers the same way about the same turn: the two that
    // carry evidence are clustered by this build's threshold, the third is not
    // invented into cluster 0.
    let pool = DiarizePool::new(stub_returning(turns), READY);
    let full = cluster_track(
        &pool,
        MeetingTracks {
            mic_wav: mic,
            system_wav: None,
        },
        MeetingKind::InPerson,
        CosineDistance::new(SEPARATING),
        TargetMode::FullClustering,
        STUB_MIN_EMBED,
    )
    .expect("the stub answers");
    pool.shutdown();
    assert_eq!(
        full.iter()
            .map(|s| s.cluster_index)
            .collect::<Vec<Option<i64>>>(),
        vec![Some(0), Some(1), None],
        "one short turn must not send the whole pass to the child's own cluster ids"
    );
}
