//! YV126 acceptance — the clustering threshold is a cosine DISTANCE, and it
//! decides what groups and what splits.
//!
//! ```sh
//! cargo test --test diarize_cluster_distance_threshold
//! ```
//!
//! Pure in the sense that matters: no model, no onnxruntime, no audio hardware,
//! no corpus. The embeddings are unit vectors placed at chosen angles, so the
//! cosine distance between any two of them is a number this file ASKED for
//! rather than one it discovered — which is what makes "below the threshold
//! groups, above it splits" a falsifiable claim rather than a description of
//! whatever the code happened to do.
//!
//! Every threshold here is a [`CosineDistance`]. That is not a style rule:
//! `sherpa-onnx`'s clustering threshold is a distance (smaller = more similar)
//! and the plan's enrollment bands are similarities (larger = more similar), and
//! a codebase of bare `f32`s would compare the two without a word (merged
//! finding #20). `the_public_clustering_api_takes_a_typed_distance` holds the
//! shipped signature to it.

mod support;

use std::path::Path;

use support::diarize_stub::{angle_for_distance, ray, stub_returning, stub_with_body, StubTurn};
use wilson_voice_lib::diarize::{cluster_by_distance_threshold, cluster_track, DiarizePool, MeetingTracks, TargetMode};
use wilson_voice_lib::diarize_metrics::{cosine_similarity, CosineDistance, CosineSimilarity};
use wilson_voice_lib::meetings::{MeetingKind, MIC_TRACK};

const READY: std::time::Duration = std::time::Duration::from_secs(10);

/// How many distinct clusters a labelling contains.
fn distinct(labels: &[usize]) -> usize {
    let mut seen: Vec<usize> = labels.to_vec();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

/// The mechanism, at the smallest size that can show it: two turns at a KNOWN
/// cosine distance, and a threshold swept across it.
///
/// The pair is 0.30 apart. At 0.35 it is one speaker; at 0.25 it is two. Same
/// audio, same embeddings, same code — the threshold is the whole difference,
/// which is precisely why it may not be copied from a blog that measured a
/// different embedding model.
#[test]
fn one_pair_groups_below_the_threshold_and_splits_above_it() {
    let separation = 0.30f64;
    let a = ray(0.0);
    let b = ray(angle_for_distance(separation));
    // The fixture is what it claims to be, measured rather than asserted by
    // comment: a test built on a mis-stated distance would prove nothing.
    let measured = CosineDistance::from_similarity(cosine_similarity(&a, &b));
    assert!(
        (f64::from(measured.get()) - separation).abs() < 1e-4,
        "the two rays are {} apart, not {separation}",
        measured.get()
    );

    let points: Vec<&[f32]> = vec![&a, &b];
    assert_eq!(
        distinct(&cluster_by_distance_threshold(&points, CosineDistance::new(0.35))),
        1,
        "0.30 apart, threshold 0.35: one speaker"
    );
    assert_eq!(
        distinct(&cluster_by_distance_threshold(&points, CosineDistance::new(0.25))),
        2,
        "0.30 apart, threshold 0.25: two speakers"
    );
}

/// The same claim through the SHIPPED entry point — `cluster_track`, over the
/// sidecar wire, against a stub process with zero model bytes.
///
/// The stub's own `cluster` field says every turn is cluster 0. That is
/// deliberate: the parent re-clusters from the embeddings at the threshold it
/// was given, and a stub that already agreed with the right answer could not
/// tell a working clusterer from one that forwarded the child's opinion.
#[test]
fn cluster_track_groups_and_splits_at_the_threshold_it_was_given() {
    // Three turns: two of one voice (0.10 apart), one of another (0.80 away).
    let close = angle_for_distance(0.10);
    let far = angle_for_distance(0.80);
    let turns = vec![
        StubTurn::new(0.0, 4.0, 0, ray(0.0)),
        StubTurn::new(4.5, 9.0, 0, ray(close)),
        StubTurn::new(9.5, 14.0, 0, ray(far)),
    ];

    for (threshold, expected, note) in [
        (0.35f32, vec![0i64, 0, 1], "two voices: the near pair merges, the far turn does not"),
        (0.05, vec![0, 1, 2], "a threshold under every gap: three singletons"),
        (0.95, vec![0, 0, 0], "a threshold over every gap: one cluster, correctly"),
    ] {
        let pool = DiarizePool::new(stub_returning(turns.clone()), READY);
        let mic = Path::new("/tmp/does-not-need-to-exist.wav");
        let out = cluster_track(
            &pool,
            MeetingTracks {
                mic_wav: mic,
                system_wav: None,
            },
            MeetingKind::InPerson,
            CosineDistance::new(threshold),
            TargetMode::FullClustering,
        )
        .expect("the stub answers");
        pool.shutdown();

        let got: Vec<i64> = out.iter().map(|s| s.cluster_index).collect();
        assert_eq!(got, expected, "threshold {threshold}: {note}");
        // The turn boundaries and the track survive clustering untouched — the
        // pass attributes speech, it does not re-cut it.
        assert_eq!(out.len(), turns.len());
        assert!(out.iter().all(|s| s.track == MIC_TRACK));
        assert!((out[0].start_seconds - 0.0).abs() < 1e-9);
        assert!((out[2].end_seconds - 14.0).abs() < 1e-9);
    }
}

/// The child's cluster ids are the FALLBACK, not the answer.
///
/// A backend that returns turns without embeddings still gets used — a
/// diarization with the vendor's own clustering beats no diarization — but the
/// two paths must be distinguishable, or "the tuned threshold decided this"
/// becomes unfalsifiable. Same turns, same threshold, two answers.
#[test]
fn without_embeddings_the_childs_own_clusters_are_used_and_they_differ() {
    let mic = Path::new("/tmp/does-not-need-to-exist.wav");
    let tracks = MeetingTracks {
        mic_wav: mic,
        system_wav: None,
    };
    let threshold = CosineDistance::new(0.35);

    // No `embedding` field at all: the shape a backend that only reports
    // sherpa's own assignment produces.
    let bodyless = String::from(
        r#"{"start":0.0,"end":4.0,"cluster":7},{"start":4.5,"end":9.0,"cluster":7},{"start":9.5,"end":14.0,"cluster":9}"#,
    );
    let pool = DiarizePool::new(stub_with_body(bodyless), READY);
    let fallback = cluster_track(
        &pool,
        tracks,
        MeetingKind::InPerson,
        threshold,
        TargetMode::FullClustering,
    )
    .expect("the stub answers");
    pool.shutdown();
    assert_eq!(
        fallback.iter().map(|s| s.cluster_index).collect::<Vec<_>>(),
        vec![7, 7, 9],
        "with no embeddings the child's own ids are passed through verbatim"
    );

    // The same three turns WITH embeddings that disagree with those ids: the
    // parent's threshold wins, and the answer changes.
    let turns = vec![
        StubTurn::new(0.0, 4.0, 7, ray(0.0)),
        StubTurn::new(4.5, 9.0, 7, ray(angle_for_distance(0.80))),
        StubTurn::new(9.5, 14.0, 9, ray(angle_for_distance(0.05))),
    ];
    let pool = DiarizePool::new(stub_returning(turns), READY);
    let ours = cluster_track(
        &pool,
        tracks,
        MeetingKind::InPerson,
        threshold,
        TargetMode::FullClustering,
    )
    .expect("the stub answers");
    pool.shutdown();
    assert_eq!(
        ours.iter().map(|s| s.cluster_index).collect::<Vec<_>>(),
        vec![0, 1, 0],
        "turns 1 and 3 are 0.05 apart and turn 2 is 0.80 away — the child said \
         the opposite, and the child is not who decides"
    );
}

/// The unit discipline reaches the SIGNATURE, not just the prose.
///
/// A bare `f32` threshold on this API is the whole of merged finding #20: a
/// caller with a similarity in hand would pass it, clustering would run looser
/// than the identity decision it feeds, and every number downstream would be
/// wrong by an amount nobody could see. Rust cannot grep intent, so this greps
/// the source — the same trick `meeting_kind_branch.rs` uses to hold a rule to
/// a file.
#[test]
fn the_public_clustering_api_takes_a_typed_distance() {
    let src = include_str!("../src/diarize.rs");
    for signature in [
        "pub fn cluster_by_distance_threshold(",
        "pub fn cluster_track(",
    ] {
        let at = src.find(signature).unwrap_or_else(|| panic!("{signature} is gone"));
        let body = &src[at..at + 400];
        let head = body.split(") ->").next().unwrap_or_default();
        assert!(
            head.contains("threshold: CosineDistance"),
            "{signature} no longer takes a typed cosine distance:\n{head}"
        );
        // An embedding is legitimately `&[f32]` — a vector of measurements has
        // no unit to confuse. What may never be a bare `f32` is a parameter
        // whose NAME says it is one of the two cosine quantities.
        for parameter in head.lines().filter(|l| {
            let l = l.to_ascii_lowercase();
            l.contains("threshold") || l.contains("distance") || l.contains("similarity")
        }) {
            assert!(
                !parameter.contains(": f32"),
                "{signature} takes a bare f32 where a cosine unit belongs — the \
                 mixed-unit bug is back:\n{parameter}"
            );
        }
    }

    // …and the two units really are different numbers, so none of the above is
    // passing because somebody made them aliases.
    let distance = CosineDistance::new(0.35);
    let similarity = CosineSimilarity::from_distance(distance);
    assert!((similarity.get() - 0.65).abs() < 1e-6);
    assert_ne!(distance.get(), similarity.get());
}

/// No accuracy threshold ships as a constant. Not one.
///
/// The clustering threshold is a PARAMETER of `cluster_track` precisely because
/// the only honest source for a value is a measurement against fixture (e), and
/// this tree has no inference backend to measure with yet. A `const
/// DEFAULT_CLUSTERING_DISTANCE` would be sherpa's 0.5 (or OpenWhispr's 0.70/0.55
/// read as the wrong unit) wearing a Rust type — the exact failure this
/// backlog's eval-first sequencing exists to prevent.
#[test]
fn no_tuned_clustering_constant_ships_in_the_crate() {
    let src = include_str!("../src/diarize.rs");
    for line in src.lines() {
        let code = line.split("//").next().unwrap_or_default();
        if !code.contains("const") || !code.contains("CosineDistance") {
            continue;
        }
        panic!(
            "a cosine-distance constant has appeared in diarize.rs — every \
             threshold in this epic is an OUTPUT of the eval harness, never an \
             input: {line}"
        );
    }
}
