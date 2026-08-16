//! YV131 acceptance 1 — AS-norm favours the true match across a condition
//! change, where the raw absolute comparison does not.
//!
//! The spec's wording for this test: "a synthetic scenario with one profile
//! enrolled under `laptop_mic_near` and tested against an embedding captured
//! under a different synthetic condition asserts the AS-norm-adjusted score
//! correctly favors the true match over a same-condition impostor, where a raw
//! absolute-threshold comparison (asserted as a baseline in the same test) would
//! not."
//!
//! # The geometry, and why it is the honest one
//!
//! Building this test surfaced the structural fact that eventually decided what
//! the module ships. Within ONE cluster, every candidate is scored against the
//! SAME test embedding, so the test-side term of symmetric AS-norm is identical
//! for all of them — a monotone affine map of the raw score, which cannot change
//! who wins. **Only the enrollment-side term can reorder candidates.** A test
//! that claimed otherwise would be claiming something arithmetically impossible;
//! a first draft of this file did, and had to be rebuilt.
//!
//! The design sweep later removed the test side entirely (it lost on the tuning
//! split, see the module header), so what is left is exactly what this scenario
//! was already built on: HUBNESS. An identity direction, a condition direction,
//! and the observation that a profile enrolled under the same condition as the
//! test recording sits close to *every stranger ever recorded under that
//! condition* — not because it is the right person, but because they share a
//! microphone. That shared, non-identity mass is what the cohort measures and
//! what the normalization subtracts. The true match, enrolled on a different
//! device, has less of it and is penalised for it by a raw comparison.

use wilson_voice_lib::diarize_metrics::{cosine_similarity, CosineSimilarity};
use wilson_voice_lib::speaker_asnorm::{
    as_norm_score, rank_within_meeting, Candidate, ImpostorCohort, NormalizedBand, RankingBasis,
};

const DIM: usize = 24;
const EMBEDDER: &str = "test-embedder-digest";

// Two orthogonal "recording condition" directions, and one identity direction
// per person. Orthogonal because the point of the scenario is that condition
// and identity are separate things that raw cosine adds together and cannot
// tell apart afterwards.
const AIRPODS: usize = 0;
const LAPTOP_MIC_NEAR: usize = 1;
const WILSON: usize = 2;
const JEISIL: usize = 3;
const STRANGERS: std::ops::Range<usize> = 4..14;

/// `identity_weight` of the vector is who you are; the rest is what recorded
/// you. A weight of 0.75 is a distinctive voice on a close microphone; 0.35 is
/// a voice the channel is doing most of the talking for.
fn embed(identity: usize, condition: usize, identity_weight: f32) -> Vec<f32> {
    assert!((0.0..=1.0).contains(&identity_weight));
    let mut v = vec![0.0f32; DIM];
    v[identity] = identity_weight;
    v[condition] = (1.0 - identity_weight * identity_weight).sqrt();
    v
}

/// Strangers on both devices. The cohort has to span conditions for the
/// adaptive top-K to have anything condition-appropriate to select — with only
/// one condition represented, half of every ranking is normalized against
/// voices that share nothing with it.
fn cohort() -> ImpostorCohort {
    let mut rows = Vec::new();
    for (n, identity) in STRANGERS.enumerate() {
        // A spread of identity weights, so the cohort scores have a standard
        // deviation to divide by. A cohort whose members all score identically
        // measures nothing.
        let weight = 0.30 + 0.015 * n as f32;
        rows.push(embed(identity, AIRPODS, weight));
        rows.push(embed(identity, LAPTOP_MIC_NEAR, weight));
    }
    ImpostorCohort::from_rows(rows, 4, NormalizedBand::new(0.0), EMBEDDER).expect("cohort builds")
}

/// Wilson enrolled on the laptop microphone, distinctive: identity carries.
fn wilson_enrolled() -> Vec<f32> {
    embed(WILSON, LAPTOP_MIC_NEAR, 0.75)
}

/// The cluster under test: Wilson again, later, on AirPods. Same identity
/// direction, different condition direction — the cross-device case.
fn wilson_on_airpods() -> Vec<f32> {
    embed(WILSON, AIRPODS, 0.75)
}

/// Jeisil, enrolled on AirPods. A different person who shares the test
/// recording's condition, and whose profile is therefore close to every
/// stranger on that device too.
fn jeisil_enrolled() -> Vec<f32> {
    embed(JEISIL, AIRPODS, 0.35)
}

#[test]
fn as_norm_improves_cross_condition_matching() {
    let cluster = wilson_on_airpods();
    let raw_true = cosine_similarity(&wilson_enrolled(), &cluster);
    let raw_impostor = cosine_similarity(&jeisil_enrolled(), &cluster);

    // ---- baseline: the raw comparison gets this WRONG ----------------------
    // Asserted, not assumed. If the geometry ever stopped being a case where
    // raw cosine fails, the AS-norm assertion below would prove nothing, and
    // this test would pass for the wrong reason. It fails here instead.
    assert!(
        raw_impostor.get() > raw_true.get(),
        "the scenario must be one where raw cosine picks the impostor, else the \
         AS-norm assertion proves nothing: true {:.4} vs impostor {:.4}",
        raw_true.get(),
        raw_impostor.get()
    );

    // ---- AS-norm gets it right --------------------------------------------
    let cohort = cohort();
    let as_true = as_norm_score(
        raw_true,
        &cohort.statistics(&wilson_enrolled()).expect("enrolled stats"),
    );
    let as_impostor = as_norm_score(
        raw_impostor,
        &cohort.statistics(&jeisil_enrolled()).expect("enrolled stats"),
    );

    assert!(
        as_true.get() > as_impostor.get(),
        "AS-norm must favour the true match across the condition change: true \
         {:.4} vs impostor {:.4} (raw was {:.4} vs {:.4})",
        as_true.get(),
        as_impostor.get(),
        raw_true.get(),
        raw_impostor.get()
    );
}

#[test]
fn the_whole_ranking_flips_not_just_the_pair() {
    // The same scenario through the shipped entry point, because a pairwise
    // score comparison is not what the app calls — `rank_within_meeting` is.
    let cluster = wilson_on_airpods();
    let candidates = vec![
        Candidate {
            profile_id: "jeisil".into(),
            raw: cosine_similarity(&jeisil_enrolled(), &cluster),
            centroid: Some(jeisil_enrolled()),
        },
        Candidate {
            profile_id: "wilson".into(),
            raw: cosine_similarity(&wilson_enrolled(), &cluster),
            centroid: Some(wilson_enrolled()),
        },
    ];

    let without = rank_within_meeting(&candidates, None);
    assert_eq!(without.basis, RankingBasis::RawCosine);
    assert_eq!(
        without.best().unwrap().profile_id,
        "jeisil",
        "baseline: raw cosine ranks the same-device impostor first"
    );

    let cohort = cohort();
    let with = rank_within_meeting(&candidates, Some(&cohort));
    assert_eq!(with.basis, RankingBasis::AsNorm);
    assert_eq!(
        with.best().unwrap().profile_id,
        "wilson",
        "AS-norm ranks the true match first: {:?}",
        with.ordered
    );
    assert!(with.margin().unwrap() > 0.0, "the winner must be strictly ahead");
}

#[test]
fn as_norm_is_not_a_cosine_similarity() {
    // The unit discipline, exercised rather than asserted in prose: a normalized
    // score routinely lands outside [-1, 1], which is why it has its own type
    // and cannot be handed to an enrollment band expressed in cosine.
    let cohort = cohort();
    let stats = cohort.statistics(&wilson_enrolled()).expect("stats");
    let score = as_norm_score(CosineSimilarity::new(1.0), &stats);
    assert!(
        score.get() > 1.0,
        "a perfect match should exceed the cosine range once normalized, got {}",
        score.get()
    );
}

#[test]
fn a_hub_profile_loses_the_advantage_it_never_earned() {
    // The mechanism, isolated from the ranking: the impostor's raw score is
    // high because his profile is close to EVERY stranger on that device, and
    // the cohort is what makes that visible. Measured as the gap between his
    // raw score and what a stranger scores — the quantity AS-norm divides by.
    let cohort = cohort();
    let cluster = wilson_on_airpods();

    let jeisil_stats = cohort.statistics(&jeisil_enrolled()).unwrap();
    let wilson_stats = cohort.statistics(&wilson_enrolled()).unwrap();

    assert!(
        jeisil_stats.mean() > wilson_stats.mean(),
        "the same-device profile must be the one sitting closer to the cohort: \
         jeisil {:.4} vs wilson {:.4}",
        jeisil_stats.mean(),
        wilson_stats.mean()
    );

    let raw_true = cosine_similarity(&wilson_enrolled(), &cluster).get();
    let raw_impostor = cosine_similarity(&jeisil_enrolled(), &cluster).get();
    assert!(
        raw_true - wilson_stats.mean() > raw_impostor - jeisil_stats.mean(),
        "measured against its own cohort, the true match is further above a \
         stranger than the impostor is: {:.4} vs {:.4}",
        raw_true - wilson_stats.mean(),
        raw_impostor - jeisil_stats.mean()
    );
}
