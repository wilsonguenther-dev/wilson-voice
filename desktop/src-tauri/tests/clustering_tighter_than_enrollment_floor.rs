//! YV129 — clustering must be **strictly tighter** than the identity decision
//! it feeds. Merged finding #20's second half, as an executable check.
//!
//! `sherpa_onnx::FastClusteringConfig.threshold` is a cosine **distance**
//! (smaller is more similar, default `0.5`). The plan's enrollment bands are
//! cosine **similarities** (larger is more similar). As specified — clustering
//! at `0.5` distance, a new-voice floor at `0.55` similarity, i.e. `0.45`
//! distance — clustering was LOOSER than enrollment: it would merge two voices
//! into one cluster that the matcher, handed the same pair, would refuse to call
//! the same person. The merged cluster's centroid is then the average of two
//! people, and every downstream decision inherits it.
//!
//! Nothing in a codebase of bare `f32`s would ever have said so, which is why
//! YV120 put the two units in the type system and why the check below can only
//! be called with them the right way round.
//!
//! # Why this asserts a FUNCTION rather than a pair of shipped constants
//!
//! The backlog's wording is `CosineDistance::from_similarity(new_voice_floor) >
//! clustering_threshold` over the shipped values. Neither value ships on this
//! base, deliberately and in both cases for the same reason: YV126 made its
//! clustering distance a parameter with no default anywhere in the crate
//! (`no_tuned_clustering_constant_ships_in_the_crate`), and this item does the
//! same for the bands, because there is no inference backend to measure either
//! against. Asserting an inequality between two numbers that do not exist would
//! be a test of nothing.
//!
//! So the invariant is a shipped function with both units in its signature, and
//! this file drives it over real values in both directions — including the
//! plan's own pair, which it must reject. The day a call site supplies both
//! numbers, the check binds there. Same shape as YV124's `arm_is_not_worse`: a
//! gate written as a function precisely so it can be executed on a machine that
//! cannot yet run the measurement around it.

use wilson_voice_lib::diarize_metrics::{CosineDistance, CosineSimilarity};
use wilson_voice_lib::speaker_profiles::{
    check_clustering_tighter_than_enrollment, EnrollmentBands,
};

fn bands(auto: f32, floor: f32) -> EnrollmentBands {
    EnrollmentBands::for_test(CosineSimilarity::new(auto), CosineSimilarity::new(floor))
        .expect("well-ordered bands")
}

/// The acceptance criterion, both ways round.
#[test]
fn clustering_tighter_than_enrollment_floor() {
    let b = bands(0.90, 0.55);
    let floor_as_distance = CosineDistance::from_similarity(b.new_voice_floor());
    assert!(
        (floor_as_distance.get() - 0.45).abs() < 1e-6,
        "a 0.55 similarity floor is a 0.45 distance"
    );

    // Tighter: clustering merges only what is closer than the floor.
    check_clustering_tighter_than_enrollment(CosineDistance::new(0.40), &b)
        .expect("0.40 distance is tighter than a 0.45-distance floor");

    // The plan's own pair — sherpa's 0.5 default against a 0.55 floor.
    let violation = check_clustering_tighter_than_enrollment(CosineDistance::new(0.50), &b)
        .expect_err(
            "the plan's specified pair is the exact inversion finding #20 is about \
             and must not pass",
        );
    assert!((violation.clustering.get() - 0.50).abs() < 1e-6);
    assert!((violation.floor_as_distance.get() - 0.45).abs() < 1e-6);
    assert!(
        violation.to_string().contains("not tighter"),
        "the failure has to say what is wrong: {violation}"
    );
}

/// Equality is a violation, not a pass.
///
/// At exactly the floor, clustering merges the pair enrollment would refuse —
/// the boundary is the interesting case and `>` is deliberate.
#[test]
fn clustering_exactly_at_the_floor_is_rejected() {
    let b = bands(0.90, 0.55);
    assert!(
        check_clustering_tighter_than_enrollment(CosineDistance::new(0.45), &b).is_err(),
        "0.45 distance IS the floor; merging there is the inversion at its boundary"
    );
    assert!(
        check_clustering_tighter_than_enrollment(CosineDistance::new(0.4499), &b).is_ok(),
        "a hair inside the floor is the tightest legal setting"
    );
}

/// The ordering holds across the whole unit interval, not just at one pair.
///
/// A single hand-picked pair can be satisfied by a check that compares the
/// wrong things and happens to agree; sweeping both axes cannot.
#[test]
fn the_ordering_is_a_property_of_the_two_units_not_of_one_lucky_pair() {
    for floor_pct in 1..100 {
        let floor = floor_pct as f32 / 100.0;
        let b = bands(1.0, floor);
        let boundary = 1.0 - floor;
        for dist_pct in 0..200 {
            let d = dist_pct as f32 / 100.0;
            let ok = check_clustering_tighter_than_enrollment(CosineDistance::new(d), &b).is_ok();
            assert_eq!(
                ok,
                boundary > d,
                "floor={floor} distance={d}: clustering must be legal exactly when \
                 it is strictly tighter than {boundary}"
            );
        }
    }
}

/// The units cannot be swapped by accident — that is the whole point of YV120's
/// newtypes, and this records it as an executed fact rather than a claim.
///
/// `check_clustering_tighter_than_enrollment(CosineSimilarity, &bands)` does not
/// compile; there is no `From<f32>` for either newtype; the ONE conversion is
/// `CosineDistance::from_similarity`. What is asserted here is the conversion's
/// direction, which is the part a compiler cannot check.
#[test]
fn the_one_conversion_runs_the_direction_finding_20_requires() {
    for pct in -100..=100 {
        let s = CosineSimilarity::new(pct as f32 / 100.0);
        let d = CosineDistance::from_similarity(s);
        assert!(
            (d.get() - (1.0 - s.get())).abs() < 1e-6,
            "distance is 1 - similarity, at {}",
            s.get()
        );
        assert!(
            (CosineSimilarity::from_distance(d).get() - s.get()).abs() < 1e-6,
            "and it round-trips"
        );
    }
}
