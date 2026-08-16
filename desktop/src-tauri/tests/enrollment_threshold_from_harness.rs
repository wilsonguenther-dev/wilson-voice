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
    EmbeddingModelId, EnrollmentBands, MatchResult, SpeakerProfile, TargetFar, TuningError,
};

/// The embedder every fixture profile below was enrolled under, and the one the
/// probe centroids come from. A stand-in for `catalog.json`'s pinned sha256.
fn model() -> EmbeddingModelId {
    EmbeddingModelId::new("sha256-fixture-embedder")
}

#[path = "support/bands.rs"]
mod bands;

/// The measured `(auto_confirm, new_voice_floor)` split, pinned once and never
/// recomputed here.
///
/// `None` = not yet measured. See the header. To fill it: run YV126's
/// `cargo test --test meeting_eval tune_enrollment_band -- --nocapture` on a
/// corpus-equipped machine with an embedder, paste the printed pair, and record
/// the sweep — with the provenance record `docs/yap23-eer-status.md` specifies —
/// in the backlog note the way YV93 recorded its WER arms.
// TODO(YV122+YV124): pin once a backend exists and the anti-alias EER is measured.
const PINNED_BANDS: Option<(f32, f32)> = None;

/// The false-accept budget these tests place the auto-confirm edge against.
///
/// A POLICY, stated at the call site, never a constant in the crate: how often
/// the app may name a stranger with nobody in the loop is a product decision,
/// and `TargetFar` exists so that decision cannot be smuggled into
/// `speaker_profiles.rs` as a tuned number. 1 impostor in 20 is what the
/// twenty-pair hand-worked sample below can express — see
/// `a_budget_finer_than_the_sample_can_express_is_refused`.
fn budget(rate: f64) -> TargetFar {
    TargetFar::new(rate).expect("a rate between 0 and chance")
}

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
            assert!(
                !shipped.is_empty(),
                "PINNED_BANDS is Some({auto}, {floor}) but nothing in the shipping \
                 crate carries it — the measurement exists and the app is not using it"
            );
            // The pin is not self-certifying either: it has to be the pair the
            // recorded run produced. Same rule as the status block — a number
            // with no provenance is the thing this item refuses.
            let block = bands::eer_status_block();
            let provenance = bands::parse_provenance(&block).unwrap_or_else(|e| {
                panic!(
                    "PINNED_BANDS is Some, so docs/yap23-eer-status.md must carry the run that \
                     produced it, and it does not ({e})"
                )
            });
            assert!(
                (provenance.auto_confirm - auto as f64).abs() < 1e-4
                    && (provenance.new_voice_floor - floor as f64).abs() < 1e-4,
                "the pinned pair ({auto}, {floor}) is not the pair the recorded run produced \
                 ({}, {})",
                provenance.auto_confirm,
                provenance.new_voice_floor
            );
            eprintln!("enrollment bands: auto_confirm={auto:.4} new_voice_floor={floor:.4}");
        }
    }
}

/// The derivation itself, against a hand-worked OVERLAPPING distribution.
///
/// Twenty pairs a side, so the sample can express a rate of `1/20 = 0.05`:
///
/// * impostor — eighteen scores from `0.10` to `0.27`, plus two confusable ones
///   at `0.60` and `0.62`.
/// * genuine — two weak pairs at `0.55` and `0.58`, plus eighteen from `0.70`
///   to `0.87`.
///
/// Worked on paper before it was run:
///
/// * The two error curves cross between `0.58` and `0.60`: above `0.59` two
///   impostors (`0.60`, `0.62`) are still accepted (FAR `0.10`) and two genuine
///   pairs (`0.55`, `0.58`) are already refused (FRR `0.10`). So the equal-error
///   point is `0.59` at EER `0.10`, and that is where the **new-voice floor**
///   goes — the item's spec, verbatim.
/// * With a 5 % false-accept budget the **auto-confirm** edge is the lowest
///   operating point above the floor that admits at most one impostor in twenty:
///   `0.60` still admits two, `0.61` admits one. So `0.61`, achieved FAR `0.05`,
///   which is also exactly this sample's resolution.
#[test]
fn bands_bracket_the_overlap_between_the_two_distributions() {
    let mut impostor: Vec<f32> = (0..18).map(|i| 0.10 + 0.01 * i as f32).collect();
    impostor.extend([0.60, 0.62]);
    let mut genuine: Vec<f32> = vec![0.55, 0.58];
    genuine.extend((0..18).map(|i| 0.70 + 0.01 * i as f32));

    let tuned = bands_from_distribution(&sims(&genuine), &sims(&impostor), budget(0.05))
        .expect("twenty pairs a side, separable");

    eprintln!(
        "hand-worked 20/20: eer={:.4} @ {:.4} → floor={:.4} auto_confirm={:.4} \
         far={:.4}(res {:.4}) frr@floor={:.4} frr@auto={:.4}(res {:.4})",
        tuned.eer,
        tuned.eer_threshold.get(),
        tuned.bands.new_voice_floor().get(),
        tuned.bands.auto_confirm().get(),
        tuned.far_at_auto_confirm,
        tuned.far_resolution,
        tuned.frr_at_new_voice_floor,
        tuned.frr_at_auto_confirm,
        tuned.frr_resolution,
    );

    assert!((tuned.eer - 0.10).abs() < 1e-9, "{:?}", tuned.eer);
    assert!(
        (tuned.bands.new_voice_floor().get() - 0.59).abs() < 1e-4,
        "the floor is the equal-error point, not an order statistic: {:?}",
        tuned.bands
    );
    assert!(
        (tuned.eer_threshold.get() - tuned.bands.new_voice_floor().get()).abs() < 1e-6,
        "and it IS the EER threshold, not merely near it"
    );
    assert!(
        (tuned.bands.auto_confirm().get() - 0.61).abs() < 1e-4,
        "the auto-confirm edge is the lowest point above the floor inside the FAR budget: {:?}",
        tuned.bands
    );
    assert!(
        (tuned.far_at_auto_confirm - 0.05).abs() < 1e-9,
        "one impostor in twenty, which is the budget AND the resolution"
    );
    assert!(tuned.far_at_auto_confirm <= tuned.target_far + 1e-12);
    assert!(
        (tuned.frr_at_new_voice_floor - 0.10).abs() < 1e-9,
        "the floor costs the two weak genuine pairs — an EER floor is not free, and \
         printing it is the honest half"
    );
    assert!((tuned.far_resolution - 0.05).abs() < 1e-9);
    assert!((tuned.frr_resolution - 0.05).abs() < 1e-9);
    assert_eq!((tuned.genuine, tuned.impostor), (20, 20));
}

/// The other topology: a clean gap, where the sample says nothing in between.
///
/// genuine `{0.95, 0.90, 0.80}`, impostor `{0.30, 0.20, 0.10}`. Every threshold
/// in `(0.30, 0.80]` scores zero errors, so the equal-error sweep reports the
/// midpoint of the gap — `0.55` — and the floor goes there. The auto-confirm
/// edge is then the lowest operating point above it, `0.80`, because there is no
/// measured point between the two: a threshold placed anywhere inside a gap the
/// sample never produced a score in is a guess about unobserved data, and the
/// honest thing to do with the whole gap is ASK.
#[test]
fn a_cleanly_separated_distribution_asks_about_the_gap_rather_than_guessing_inside_it() {
    let genuine = sims(&[0.95, 0.90, 0.80]);
    let impostor = sims(&[0.30, 0.20, 0.10]);
    let tuned = bands_from_distribution(&genuine, &impostor, budget(0.34))
        .expect("cleanly separable");

    assert_eq!(tuned.eer, 0.0, "a clean gap has no errors to trade");
    assert!(
        (tuned.bands.new_voice_floor().get() - 0.55).abs() < 1e-4,
        "{:?}",
        tuned.bands
    );
    assert!(
        (tuned.bands.auto_confirm().get() - 0.80).abs() < 1e-4,
        "{:?}",
        tuned.bands
    );
    assert_eq!(tuned.far_at_auto_confirm, 0.0);
    assert_eq!(tuned.frr_at_new_voice_floor, 0.0);

    // And a score inside the gap gets ASKED about rather than decided.
    let roster = [SpeakerProfile {
        id: "p1".into(),
        display_name: "Jeisil".into(),
        is_me: false,
        embedding_model: model(),
        centroids: vec![Centroid::new(
            "laptop_mic_near",
            Embedding::new(vec![1.0, 0.0, 0.0]),
        )],
    }];
    // cos([1,1,0], [1,0,0]) = 0.7071 — inside the gap.
    assert!(matches!(
        match_cluster(
            &Embedding::new(vec![1.0, 1.0, 0.0]),
            &model(),
            &roster,
            tuned.bands
        ),
        MatchResult::Suggested { .. }
    ));
}

/// **The finding this rule replaced, measured rather than argued.**
///
/// The first shipped rule put `auto_confirm` at `max(impostor) + ε` and
/// `new_voice_floor` at `min(genuine)`. Both are extreme order statistics, both
/// are monotone in sample size, and the review finding's consequence is the one
/// asserted here: the suggest band WIDENS as the corpus grows, and a single
/// confusable pair pushes auto-confirm to the ceiling, after which
/// `MatchResult::Known` never fires again and an enrolled speaker is asked "who
/// is this?" in every meeting forever.
///
/// The comparison is run against the same growing sample, so it is a
/// measurement of the two rules and not a claim about them. `old_rule_edges`
/// below is the deleted implementation, kept only so this test can falsify it.
#[test]
fn band_edges_do_not_diverge_as_the_sample_grows() {
    /// The rule this item shipped first, verbatim, for comparison only.
    fn old_rule_edges(genuine: &[CosineSimilarity], impostor: &[CosineSimilarity]) -> (f32, f32) {
        let highest_impostor = impostor
            .iter()
            .map(|s| s.get())
            .fold(f32::NEG_INFINITY, f32::max)
            + 1e-4;
        let lowest_genuine = genuine.iter().map(|s| s.get()).fold(f32::INFINITY, f32::min);
        if highest_impostor >= lowest_genuine {
            (highest_impostor, lowest_genuine)
        } else {
            (lowest_genuine, highest_impostor)
        }
    }

    // A deterministic generator: three uniforms summed, so the tails get more
    // extreme with N the way a real score distribution's do. No rand crate, no
    // seed drift, same numbers on every machine.
    struct Lcg(u64);
    impl Lcg {
        fn unit(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 40) as f32) / (1u64 << 24) as f32
        }
        fn score(&mut self, mean: f32, spread: f32) -> f32 {
            let u = self.unit() + self.unit() + self.unit();
            mean + spread * (u - 1.5) / 1.5
        }
    }

    // ONE pool, and each sample size is a PREFIX of it — a corpus that grew,
    // which is the situation the finding is about. Nested samples are also what
    // make the comparison airtight: `max(impostor)` can then only rise and
    // `min(genuine)` can only fall, by construction rather than by luck.
    let mut rng = Lcg(0x5eed_1129);
    let pool_genuine: Vec<CosineSimilarity> = (0..640)
        .map(|_| CosineSimilarity::new(rng.score(0.65, 0.25)))
        .collect();
    let pool_impostor: Vec<CosineSimilarity> = (0..640)
        .map(|_| CosineSimilarity::new(rng.score(0.50, 0.25)))
        .collect();
    let sample = |n: usize| (&pool_genuine[..n], &pool_impostor[..n]);

    let mut new_widths = Vec::new();
    let mut old_widths = Vec::new();
    for n in [40usize, 160, 640] {
        let (genuine, impostor) = sample(n);
        let tuned = bands_from_distribution(genuine, impostor, budget(0.05))
            .expect("overlapping but separable");
        let (old_auto, old_floor) = old_rule_edges(genuine, impostor);
        let new_width = tuned.bands.auto_confirm().get() - tuned.bands.new_voice_floor().get();
        let old_width = old_auto - old_floor;
        eprintln!(
            "n={n:4}  MEASURED floor={:.4} auto={:.4} width={new_width:.4} (far {:.4})   \
             ORDER-STATISTIC floor={old_floor:.4} auto={old_auto:.4} width={old_width:.4}",
            tuned.bands.new_voice_floor().get(),
            tuned.bands.auto_confirm().get(),
            tuned.far_at_auto_confirm,
        );
        new_widths.push(new_width);
        old_widths.push(old_width);
    }

    assert!(
        old_widths[2] > old_widths[0],
        "the order-statistic rule is supposed to widen with N; if it did not, this \
         comparison proves nothing: {old_widths:?}"
    );
    assert!(
        new_widths[2] <= new_widths[0] + 0.02,
        "the measured rule's suggest band must not widen as the corpus grows: {new_widths:?}"
    );
    assert!(
        new_widths[2] < old_widths[2],
        "at the largest sample the measured band must be the tighter one: \
         {new_widths:?} vs {old_widths:?}"
    );

    // The consequence, concretely: ONE confusable impostor pair.
    let (genuine, impostor) = sample(640);
    let genuine = genuine.to_vec();
    let mut impostor = impostor.to_vec();
    impostor.push(CosineSimilarity::new(0.98));
    let tuned = bands_from_distribution(&genuine, &impostor, budget(0.05))
        .expect("one confusable pair does not stop a quantile");
    let (old_auto, _) = old_rule_edges(&genuine, &impostor);
    let enrolled_again = CosineSimilarity::new(0.90);
    eprintln!(
        "one confusable impostor at 0.98: MEASURED auto={:.4} (a 0.90 match is Known) — \
         ORDER-STATISTIC auto={old_auto:.4} (a 0.90 match is asked about, forever)",
        tuned.bands.auto_confirm().get()
    );
    assert!(
        enrolled_again.get() >= tuned.bands.auto_confirm().get(),
        "a strong match must still auto-confirm: {:?}",
        tuned.bands
    );
    assert!(
        enrolled_again.get() < old_auto,
        "the finding's failure mode, reproduced: under the order-statistic rule the same \
         strong match no longer clears auto-confirm ({old_auto})"
    );
}

/// A tuning run that cannot tell the two populations apart must refuse.
#[test]
fn an_indistinguishable_distribution_produces_no_bands() {
    let overlapping = sims(&[0.5, 0.5, 0.5, 0.5]);
    let err = bands_from_distribution(&overlapping, &overlapping, budget(0.25))
        .expect_err("identical distributions are chance");
    assert!(
        matches!(err, TuningError::Indistinguishable { eer } if eer >= 0.5),
        "{err:?}"
    );
    assert!(
        matches!(
            bands_from_distribution(&[], &sims(&[0.1]), budget(0.25)),
            Err(TuningError::EmptyDistribution)
        ),
        "an EER over no genuine trials is not a number"
    );
    assert!(matches!(
        bands_from_distribution(&sims(&[0.9]), &[], budget(0.25)),
        Err(TuningError::EmptyDistribution)
    ));
}

/// A budget the corpus cannot express is refused, not rounded.
///
/// YV124's saturated-EER lesson on the other tail: 20 impostor pairs can show
/// `0`, `0.05`, `0.10`… and nothing in between, so an edge "meeting 1 %" would
/// be meeting a number that sample never had.
#[test]
fn a_budget_finer_than_the_sample_can_express_is_refused() {
    let genuine = sims(&[0.80, 0.85, 0.90]);
    let impostor = sims(&[0.10, 0.20, 0.30]);
    let err = bands_from_distribution(&genuine, &impostor, budget(0.01))
        .expect_err("three impostor pairs cannot express 1%");
    assert!(
        matches!(
            err,
            TuningError::TargetFarBelowResolution { far_resolution, .. }
                if (far_resolution - 1.0 / 3.0).abs() < 1e-9
        ),
        "{err:?}"
    );
    // And the policy type itself refuses a rate that is not one.
    assert!(matches!(
        TargetFar::new(0.0),
        Err(TuningError::TargetFarOutOfRange { .. })
    ));
    assert!(matches!(
        TargetFar::new(0.5),
        Err(TuningError::TargetFarOutOfRange { .. })
    ));
    assert!(TargetFar::new(0.05).is_ok());
}

/// Impostors at the ceiling of the similarity line leave nothing above them.
///
/// Four of twenty impostor pairs score a perfect `1.0` — duplicated enrollment
/// clips, or two profiles that are secretly the same person. The distributions
/// are still mostly separable, so the chance refusal does not fire, and the run
/// must fail for the RIGHT reason: meeting a 1-in-20 FAR budget would need an
/// edge above `1.0`, and there is no such similarity.
#[test]
fn an_impostor_at_the_ceiling_leaves_no_room_for_an_auto_confirm_edge() {
    let genuine: Vec<f32> = (0..20).map(|_| 0.9).collect();
    let mut impostor: Vec<f32> = (0..16).map(|i| 0.10 + 0.01 * i as f32).collect();
    impostor.extend([1.0, 1.0, 1.0, 1.0]);
    let err = bands_from_distribution(&sims(&genuine), &sims(&impostor), budget(0.05))
        .expect_err("nothing can sit above 1.0");
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

    let tuned = bands_from_distribution(&genuine, &impostor, budget(0.05))
        .expect("three separable speakers are tunable");
    eprintln!(
        "tuned from 9 genuine / 27 impostor pairs: auto_confirm={:.4} \
         new_voice_floor={:.4} eer={:.4} far={:.4} (far_res={:.4} frr_res={:.4})",
        tuned.bands.auto_confirm().get(),
        tuned.bands.new_voice_floor().get(),
        tuned.eer,
        tuned.far_at_auto_confirm,
        tuned.far_resolution,
        tuned.frr_resolution,
    );

    // Enrol speaker 0 from its first take, then match a LATER take of the same
    // speaker against the bands the sweep produced.
    let roster = [SpeakerProfile {
        id: "spk_0".into(),
        display_name: "Speaker 0".into(),
        is_me: false,
        embedding_model: model(),
        centroids: vec![Centroid::new("laptop_mic_near", wobble(0, 1))],
    }];
    assert!(
        matches!(
            match_cluster(&wobble(0, 2), &model(), &roster, tuned.bands),
            MatchResult::Known { .. }
        ),
        "a later take of an enrolled speaker must clear the measured auto-confirm edge"
    );
    assert_eq!(
        match_cluster(&wobble(2, 1), &model(), &roster, tuned.bands),
        MatchResult::New,
        "a different speaker must fall under the measured floor"
    );

    // **And the same enrolled speaker, under a DIFFERENT embedder, is not a
    // match at all.** This is the hazard the whole `TargetFar` derivation
    // exists to bound, arriving by a route no threshold can see: the vectors
    // are byte-identical here, so the score is 1.0 and the decision would be
    // `Known` — a name written on a stranger with nobody in the loop — if the
    // roster's `embedding_model` were not checked. Two 192-dim embedders
    // (CAM++ and any other wespeaker export, or the same catalog id
    // re-vendored to different weights) put vectors in unrelated spaces, and a
    // cosine across two spaces is a number, not a similarity.
    let other_embedder = [SpeakerProfile {
        embedding_model: EmbeddingModelId::new("sha256-a-different-192-dim-model"),
        ..roster[0].clone()
    }];
    assert!(
        matches!(
            match_cluster(&wobble(0, 1), &model(), &other_embedder, tuned.bands),
            MatchResult::New
        ),
        "an identical vector from another embedding space must never auto-confirm"
    );
    assert!(
        matches!(
            match_cluster(&wobble(0, 1), &other_embedder[0].embedding_model, &other_embedder, tuned.bands),
            MatchResult::Known { .. }
        ),
        "…and the same roster under its own model still matches, so it is the guard          that changed the answer and not the fixture"
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
/// non-vacuous there in both directions — including against the review probe
/// that defeated its first version.
#[test]
fn no_tuned_enrollment_band_ships_in_the_crate() {
    let sites = bands::tuned_band_sites();
    assert!(
        sites.is_empty(),
        "a tuned enrollment band has appeared in src/:\n  {}",
        sites.join("\n  ")
    );
}

/// The seal, asserted where a scanner cannot see it: the shipping crate exports
/// no way to hand-place a band.
///
/// `EnrollmentBands::for_test` is the only literal constructor and it is behind
/// `cfg(any(test, feature = "test-bands"))`, which `cargo build --release` does
/// not turn on. This test can USE it (it is a test), so the visible half of the
/// seal is checked in source: the checked constructor must be private, and the
/// literal one must carry the cfg. A shipping caller that tried either is a
/// compile error before any of the nets above are consulted.
#[test]
fn the_only_literal_constructor_is_sealed_behind_a_test_cfg() {
    let src = std::fs::read_to_string(
        bands::repo_root().join("desktop/src-tauri/src/speaker_profiles.rs"),
    )
    .expect("read speaker_profiles.rs");
    assert!(
        src.contains("    fn from_measured_edges(") && !src.contains("pub fn from_measured_edges("),
        "the checked constructor must stay private to the module, or bands_from_distribution \
         stops being the only shipping producer"
    );
    assert!(
        src.contains("#[cfg(any(test, feature = \"test-bands\"))]\n    pub fn for_test("),
        "the literal constructor must carry the test-only cfg immediately above it"
    );
    // …and it works, which is why every other test in this repo can build bands.
    assert!(EnrollmentBands::for_test(
        CosineSimilarity::new(0.9),
        CosineSimilarity::new(0.5)
    )
    .is_ok());
}
