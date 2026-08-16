//! YV129 — the enrollment bands come out of YV120's harness, or they do not
//! exist.
//!
//! The epic plan's `0.70` / `0.55` cosine-similarity bands are quoted from a
//! third-party blog post measuring a different pipeline: a different embedder, a
//! different resampler, a different segmentation front end. Merged finding #21's
//! closing instruction, and this backlog's standing rule, is that every
//! threshold in yap23 is an OUTPUT of the harness, measured against a fixture.
//!
//! This file is the golden-value pin the item asks for, in the posture
//! `WER_GATE` already established: the number is tuned ONCE, written down, and
//! asserted here as a literal — never re-derived at test time, because a test
//! that recomputes its own expectation cannot notice the computation changing.
//!
//! # The pin is `None` on this base, and that is the measurement, not a gap
//!
//! [`PINNED_BANDS`] is `None` because the bands are unmeasurable here and it
//! would be dishonest to fill it:
//!
//! * `yap-diarize` answers `no_backend` until YV122 (PR #137) merges, so there
//!   is no CAM++ embedder on any machine — the genuine/impostor distribution
//!   fixture (e) is supposed to yield cannot be computed at all.
//! * Even with a backend, OS-8 forbids tuning until YV124's anti-alias EER is
//!   measured, which `enrollment_thresholds_refuse_an_unmeasured_eer.rs`
//!   enforces against `docs/yap23-eer-status.md`.
//!
//! So what ships is the derivation and the pin, both falsifiable today:
//! `speaker_profiles::bands_from_distribution` and `labeled_pair_scores` are
//! exercised here against hand-worked distributions whose right answer was
//! computed on paper, and against a full labeled-utterances → pairs → bands →
//! matched-cluster loop. Filling [`PINNED_BANDS`] is then a one-line edit
//! against a printed sweep, and the assertions below turn from "nothing is
//! pinned" into "the crate agrees with what was measured".
//!
//! **The corpus-side sweep is deliberately NOT added here.** YV126 (PR #141,
//! open) already extends `meeting_eval.rs` with `tune_enrollment_band` — a
//! `SIMILARITY_SWEEP` over fixture (f) that prints `DIARIZER_ABSENT` and sets
//! nothing on a backend-less machine. A second sweep from this item would be a
//! duplicate that collides on merge; the right seam is that YV126's sweep calls
//! `bands_from_distribution`, which is why that function ships in the crate
//! rather than in a test binary.

use wilson_voice_lib::diarize_metrics::CosineSimilarity;
use wilson_voice_lib::speaker_profiles::{
    bands_from_distribution, labeled_pair_scores, match_cluster, Centroid, Embedding,
    EnrollmentBands, MatchResult, SpeakerProfile, TuningError,
};

#[path = "support/bands.rs"]
mod bands;

/// The measured `(auto_confirm, new_voice_floor)` split, pinned once and never
/// recomputed here.
///
/// `None` = not yet measured. See the header. To fill it: run YV126's
/// `cargo test --test meeting_eval tune_enrollment_band -- --nocapture` on a
/// corpus-equipped machine with an embedder, paste the printed pair, and record
/// the sweep in the backlog note the way YV93 recorded its WER arms.
// TODO(YV122+YV124): pin once a backend exists and the anti-alias EER is measured.
const PINNED_BANDS: Option<(f32, f32)> = None;

fn sims(values: &[f32]) -> Vec<CosineSimilarity> {
    values.iter().copied().map(CosineSimilarity::new).collect()
}

/// The item's acceptance criterion: the SHIPPED bands match the measured split.
///
/// While nothing is measured, the shipped set is empty and the pin is `None`,
/// and this asserts they agree on that. The two ways it can go red are the two
/// ways the item can be got wrong: a band appears in the crate without a
/// measurement behind it, or a measurement is pinned and the crate does not
/// carry it.
#[test]
fn enrollment_threshold_from_harness() {
    let shipped = bands::tuned_band_sites();
    match PINNED_BANDS {
        None => {
            assert!(
                shipped.is_empty(),
                "PINNED_BANDS is None — no enrollment band has been measured — but \
                 the shipping crate carries {}:\n  {}\n\nEither pin the measured \
                 pair here (with the sweep that produced it recorded in the yap23 \
                 backlog note), or take the threshold back out. A number in the \
                 crate with no measurement behind it is the vendor-blog threshold \
                 this whole backlog was sequenced to prevent.",
                shipped.len(),
                shipped.join("\n  ")
            );
            eprintln!(
                "enrollment bands: UNMEASURED (no inference backend until YV122; \
                 OS-8 also forbids tuning until YV124's EER is measured). \
                 0 tuned band sites in src/."
            );
        }
        Some((auto, floor)) => {
            let bands = EnrollmentBands::new(
                CosineSimilarity::new(auto),
                CosineSimilarity::new(floor),
            )
            .expect("a pinned pair must be well-ordered");
            assert!(
                !shipped.is_empty(),
                "PINNED_BANDS is Some({auto}, {floor}) but nothing in the shipping \
                 crate carries it — the measurement exists and the app is not using it"
            );
            eprintln!(
                "enrollment bands: auto_confirm={:.4} new_voice_floor={:.4}",
                bands.auto_confirm().get(),
                bands.new_voice_floor().get()
            );
        }
    }
}

/// The derivation itself, against a hand-worked OVERLAPPING distribution.
///
/// genuine `{0.90, 0.60, 0.50}`, impostor `{0.75, 0.30, 0.10}`. The two edges
/// are `min(genuine) = 0.50` and `max(impostor) + ε = 0.7501`, and since the
/// clouds overlap the suggest band is exactly that overlap. Worked on paper:
///
/// * `0.90` (genuine) ≥ `0.7501` ⇒ auto-confirmed.
/// * `0.75` (the worst IMPOSTOR) sits just below the auto-confirm edge ⇒ asked
///   about, never silently given a name. This is the property the edge exists
///   for and the one a round number would not have.
/// * `0.50` (the weakest GENUINE pair) sits exactly on the floor ⇒ still
///   suggested, never dropped to "new voice".
#[test]
fn bands_bracket_the_overlap_between_the_two_distributions() {
    let genuine = sims(&[0.90, 0.60, 0.50]);
    let impostor = sims(&[0.75, 0.30, 0.10]);
    let tuned = bands_from_distribution(&genuine, &impostor).expect("separable enough to tune");

    assert!(
        (tuned.bands.new_voice_floor().get() - 0.50).abs() < 1e-4,
        "the floor is min(genuine): {:?}",
        tuned.bands
    );
    assert!(
        tuned.bands.auto_confirm().get() > 0.75 && tuned.bands.auto_confirm().get() < 0.7502,
        "the auto-confirm edge sits just above the highest impostor: {:?}",
        tuned.bands
    );
    assert_eq!(
        tuned.far_at_auto_confirm, 0.0,
        "no measured impostor may be auto-confirmed"
    );
    assert_eq!(
        tuned.frr_at_new_voice_floor, 0.0,
        "no measured genuine pair may be called a new voice"
    );
    // The resolution of those two zeroes, which is what makes them honest:
    // three pairs a side, so the smallest non-zero error either could show is
    // 1/3. YV124's saturated-EER lesson, applied to this item's numbers.
    assert!((tuned.far_resolution - 1.0 / 3.0).abs() < 1e-9);
    assert!((tuned.frr_resolution - 1.0 / 3.0).abs() < 1e-9);
    assert_eq!((tuned.genuine, tuned.impostor), (3, 3));
}

/// The other topology: a clean gap, where the two edges swap sides.
///
/// genuine `{0.95, 0.90, 0.80}`, impostor `{0.30, 0.20, 0.10}`. Now
/// `max(impostor) + ε = 0.3001` is BELOW `min(genuine) = 0.80`, and the suggest
/// band is the gap the sample never produced a score in — which is the honest
/// place to ask, because a threshold anywhere inside it is a guess about data
/// nobody measured. A rule that always assigned the floor to the equal-error
/// point would have put both edges in the middle of that gap and auto-confirmed
/// scores the sample says nothing about.
#[test]
fn a_cleanly_separated_distribution_asks_about_the_gap_rather_than_guessing_inside_it() {
    let genuine = sims(&[0.95, 0.90, 0.80]);
    let impostor = sims(&[0.30, 0.20, 0.10]);
    let tuned = bands_from_distribution(&genuine, &impostor).expect("cleanly separable");

    assert!((tuned.bands.auto_confirm().get() - 0.80).abs() < 1e-4, "{:?}", tuned.bands);
    assert!(
        tuned.bands.new_voice_floor().get() > 0.30 && tuned.bands.new_voice_floor().get() < 0.3002,
        "{:?}",
        tuned.bands
    );
    assert_eq!(tuned.eer, 0.0, "a clean gap has no errors to trade");
    // And a score inside the gap gets ASKED about rather than decided.
    let roster = [SpeakerProfile {
        id: "p1".into(),
        display_name: "Jeisil".into(),
        is_me: false,
        centroids: vec![Centroid::new(
            "laptop_mic_near",
            Embedding::new(vec![1.0, 0.0, 0.0]),
        )],
    }];
    // cos([1,1,0], [1,0,0]) = 0.7071 — inside the gap.
    assert!(matches!(
        match_cluster(&Embedding::new(vec![1.0, 1.0, 0.0]), &roster, tuned.bands),
        MatchResult::Suggested { .. }
    ));
}

/// A tuning run that cannot tell the two populations apart must refuse.
#[test]
fn an_indistinguishable_distribution_produces_no_bands() {
    let overlapping = sims(&[0.5, 0.5, 0.5, 0.5]);
    let err = bands_from_distribution(&overlapping, &overlapping)
        .expect_err("identical distributions are chance");
    assert!(
        matches!(err, TuningError::Indistinguishable { eer } if eer >= 0.5),
        "{err:?}"
    );
    assert!(
        matches!(
            bands_from_distribution(&[], &sims(&[0.1])),
            Err(TuningError::EmptyDistribution)
        ),
        "an EER over no genuine trials is not a number"
    );
    assert!(matches!(
        bands_from_distribution(&sims(&[0.9]), &[]),
        Err(TuningError::EmptyDistribution)
    ));
}

/// An impostor at the ceiling of the similarity line leaves no room above it.
///
/// One impostor in five scores a perfect `1.0` — a duplicated enrollment clip,
/// or two profiles that are secretly the same person. The distributions are
/// still mostly separable (EER `0.10`, so the chance refusal does not fire), and
/// the run must fail for the RIGHT reason: there is no similarity above `1.0`
/// for an auto-confirm edge to sit at, so any pair of bands returned here would
/// admit that impostor.
#[test]
fn an_impostor_at_the_ceiling_leaves_no_room_for_an_auto_confirm_edge() {
    let genuine = sims(&[0.9, 0.9, 0.9, 0.9, 0.9]);
    let impostor = sims(&[1.0, 0.1, 0.1, 0.1, 0.1]);
    let err =
        bands_from_distribution(&genuine, &impostor).expect_err("nothing can sit above 1.0");
    assert!(
        matches!(err, TuningError::NoRoomAboveImpostors { .. }),
        "must fail on the ceiling, not on chance: {err:?}"
    );
}

/// The whole loop, end to end, on a machine with no model: labeled utterances →
/// pair scores → bands → a matched cluster.
///
/// This is what "tuned FROM the harness" means as a mechanism rather than a
/// sentence. The utterances below stand in for fixture (e)'s three near-field
/// speakers: each person is a direction, each utterance is that direction with a
/// little per-take wobble, which is the structure a real embedder produces
/// (same speaker ≈ same direction, different speaker ≈ a different one).
///
/// **These vectors are not CAM++ output and no number here is a claim about the
/// model** — the same caveat YV124 attached to its synthetic arm. What IS
/// established is that the pipeline runs and that its output is usable: the
/// bands it derives correctly auto-confirm a fourth utterance from a person
/// already enrolled, and correctly refuse one from a stranger.
#[test]
fn the_full_tuning_loop_runs_from_labeled_utterances_to_a_matched_cluster() {
    // Three speakers × three takes, in a five-dimensional space.
    let wobble = |axis: usize, take: usize| {
        let mut v = vec![0.0f32; 5];
        v[axis] = 1.0;
        v[(axis + 1) % 5] = 0.10 * (take as f32 + 1.0);
        Embedding::new(v)
    };
    let labeled: Vec<(String, Embedding)> = (0..3)
        .flat_map(|axis| (0..3).map(move |take| (format!("spk_{axis}"), wobble(axis, take))))
        .collect();

    let (genuine, impostor) = labeled_pair_scores(&labeled);
    assert_eq!(
        (genuine.len(), impostor.len()),
        (9, 27),
        "3 speakers × 3 takes = 36 unordered pairs: 3×C(3,2) genuine, the rest impostor"
    );

    let tuned = bands_from_distribution(&genuine, &impostor)
        .expect("three separable speakers are tunable");
    eprintln!(
        "tuned from 9 genuine / 27 impostor pairs: auto_confirm={:.4} \
         new_voice_floor={:.4} eer={:.4} (far_res={:.4} frr_res={:.4})",
        tuned.bands.auto_confirm().get(),
        tuned.bands.new_voice_floor().get(),
        tuned.eer,
        tuned.far_resolution,
        tuned.frr_resolution
    );

    // Enrol speaker 0 from its first take, then match a LATER take of the same
    // speaker against the bands the sweep produced.
    let roster = [SpeakerProfile {
        id: "spk_0".into(),
        display_name: "Speaker 0".into(),
        is_me: false,
        centroids: vec![Centroid::new("laptop_mic_near", wobble(0, 1))],
    }];
    assert!(
        matches!(
            match_cluster(&wobble(0, 2), &roster, tuned.bands),
            MatchResult::Known { .. }
        ),
        "a later take of an enrolled speaker must clear the measured auto-confirm edge"
    );
    assert_eq!(
        match_cluster(&wobble(2, 1), &roster, tuned.bands),
        MatchResult::New,
        "a different speaker must fall under the measured floor"
    );
}

/// The pairing rule is the harness's definition, not a convenience.
#[test]
fn every_unordered_pair_is_scored_once_and_labeled_by_whether_the_speakers_match() {
    let e = |k: usize| {
        let mut v = vec![0.0f32; 3];
        v[k] = 1.0;
        Embedding::new(v)
    };
    let (genuine, impostor) = labeled_pair_scores(&[
        ("a".to_string(), e(0)),
        ("a".to_string(), e(0)),
        ("b".to_string(), e(1)),
    ]);
    assert_eq!(genuine.len(), 1, "one same-label pair");
    assert_eq!(impostor.len(), 2, "two cross-label pairs");
    assert!((genuine[0].get() - 1.0).abs() < 1e-6);
    assert!(impostor.iter().all(|s| s.get().abs() < 1e-6));

    let (g, i) = labeled_pair_scores(&[("a".to_string(), e(0))]);
    assert!(g.is_empty() && i.is_empty(), "one utterance makes no pairs");
}

/// The bands are a PARAMETER. Nothing in the crate hands you a default.
///
/// The scanner behind this is the same one
/// `enrollment_thresholds_refuse_an_unmeasured_eer.rs` uses, and it is proved
/// non-vacuous there in both directions.
#[test]
fn no_tuned_enrollment_band_ships_in_the_crate() {
    let sites = bands::tuned_band_sites();
    assert!(
        sites.is_empty(),
        "a tuned enrollment band has appeared in src/:\n  {}",
        sites.join("\n  ")
    );
}
