//! YV131 acceptance 2 — two enrolled profiles both near threshold for one
//! cluster: ranking picks the closer one, rather than returning both as
//! independent suggestions requiring two separate decisions about one voice.
//!
//! The spec: "two enrolled profiles both near-threshold for one cluster;
//! ranking selects the closer one as `Suggested`, not both as independent
//! `Suggested` results requiring two separate user decisions for one voice."
//!
//! This is the half of finding #21 that is not about normalization at all. An
//! absolute per-candidate test answers "does each of these people clear 0.70?"
//! and, in a real meeting where several enrolled voices are plausible, answers
//! yes more than once for a single cluster — which reaches the user as several
//! questions about one person. Ranking answers the question that was actually
//! being asked: which of them is this.

use wilson_voice_lib::diarize_metrics::{cosine_similarity, CosineSimilarity};
use wilson_voice_lib::speaker_asnorm::{
    rank_within_meeting, Candidate, ImpostorCohort, RankingBasis,
};

const DIM: usize = 16;
const EMBEDDER: &str = "test-embedder-digest";

fn unit(components: &[(usize, f32)]) -> Vec<f32> {
    let mut v = vec![0.0f32; DIM];
    for (i, w) in components {
        v[*i] = *w;
    }
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!(norm > 0.0);
    v.iter().map(|x| x / norm).collect()
}

/// Strangers who share the meeting's recording condition (dimension 1) to
/// varying degrees, each with their own identity direction.
///
/// The overlap is the point, and it is easy to get wrong: a first cut of this
/// helper put the cohort in dimensions the candidates never touch, so every
/// cohort score was exactly zero, every normalized score came out `0.0`, and the
/// ranking silently became "input order". The module now degrades loudly on a
/// spread-less cohort (`RankingBasis::RawCosine` with a reason), and
/// `the_cohort_actually_has_spread_here` below asserts this helper does not
/// re-create the condition it was written to avoid.
fn cohort() -> ImpostorCohort {
    let rows: Vec<Vec<f32>> = (6..14)
        .map(|i| unit(&[(i, 1.0), (1, 0.55 + 0.05 * (i as f32 - 6.0))]))
        .collect();
    ImpostorCohort::from_rows(rows, 3, EMBEDDER).expect("cohort builds")
}

#[test]
fn the_cohort_actually_has_spread_here() {
    let cohort = cohort();
    let cluster = unit(&[(1, 1.0), (2, 0.55)]);
    let stats = cohort.statistics(&cluster).expect("statistics");
    assert!(
        stats.std_dev() > 1e-3,
        "a cohort with no spread against the cluster normalizes every candidate \
         to the same number and ranks nothing: sigma {:.3e}",
        stats.std_dev()
    );
    assert!(stats.mean() > 0.0, "and it must actually overlap: mean {:.4}", stats.mean());
}

fn candidates(cluster: &[f32], profiles: &[(&str, Vec<f32>)]) -> Vec<Candidate> {
    profiles
        .iter()
        .map(|(id, centroid)| Candidate {
            profile_id: (*id).to_string(),
            raw: cosine_similarity(centroid, cluster),
            centroid: Some(centroid.clone()),
        })
        .collect()
}

#[test]
fn within_meeting_ranking_prefers_best_candidate() {
    // One cluster, two plausible people. Both are near enough that an absolute
    // band would admit them; one is closer.
    let cluster = unit(&[(1, 1.0), (2, 0.55)]);
    let profiles = vec![
        ("aidan", unit(&[(1, 1.0), (2, 0.30)])),
        ("jeisil", unit(&[(1, 1.0), (2, 0.52)])),
    ];
    let cands = candidates(&cluster, &profiles);

    // The precondition that makes this a ranking question rather than a
    // filtering one: both candidates are close, and close to each other.
    let (lo, hi) = (cands[0].raw.get(), cands[1].raw.get());
    assert!(lo > 0.90 && hi > 0.90, "both must be plausible: {lo:.4} / {hi:.4}");
    assert!((hi - lo).abs() < 0.05, "and near each other: {lo:.4} / {hi:.4}");

    let cohort = cohort();
    let ranking = rank_within_meeting(&cluster, &cands, Some(&cohort));

    // ONE answer for one voice.
    assert_eq!(
        ranking.best().map(|c| c.profile_id.as_str()),
        Some("jeisil"),
        "the closer profile is the suggestion: {:?}",
        ranking.ordered
    );
    assert_eq!(
        ranking.ordered.len(),
        2,
        "every candidate is still ranked and available to the user"
    );
    assert_eq!(
        ranking.ordered[1].profile_id, "aidan",
        "the runner-up is ranked, not suggested"
    );
    assert!(
        ranking.margin().unwrap() > 0.0,
        "a ranking with no margin is a tie, not a suggestion"
    );
}

#[test]
fn six_plausible_profiles_still_produce_one_suggestion() {
    // The plan's six-person meeting: six enrolled people, one cluster. The
    // failure this replaces answers "yes" up to six times for this one voice.
    let cluster = unit(&[(1, 1.0), (2, 0.50)]);
    let profiles: Vec<(&str, Vec<f32>)> = vec![
        ("wilson", unit(&[(1, 1.0), (2, 0.20)])),
        ("jeisil", unit(&[(1, 1.0), (2, 0.49)])),
        ("aidan", unit(&[(1, 1.0), (2, 0.31)])),
        ("shayden", unit(&[(1, 1.0), (2, 0.26)])),
        ("caleb", unit(&[(1, 1.0), (2, 0.36)])),
        ("frank", unit(&[(1, 1.0), (2, 0.41)])),
    ];
    let cands = candidates(&cluster, &profiles);
    assert!(
        cands.iter().all(|c| c.raw.get() > 0.90),
        "all six are plausible under an absolute band — that is the point"
    );

    let cohort = cohort();
    let ranking = rank_within_meeting(&cluster, &cands, Some(&cohort));

    assert_eq!(ranking.ordered.len(), 6);
    assert_eq!(
        ranking.best().map(|c| c.profile_id.as_str()),
        Some("jeisil"),
        "one suggestion for one voice: {:?}",
        ranking.ordered
    );
}

#[test]
fn ranking_is_deterministic_and_stable_on_ties() {
    // Two identical centroids cannot be ordered by score, and a ranking that
    // reordered them run to run would make the UI flicker between two names for
    // the same voice. Stable sort, so input order decides.
    let cluster = unit(&[(1, 1.0)]);
    let twin = unit(&[(1, 1.0), (2, 0.3)]);
    let cands = vec![
        Candidate { profile_id: "first".into(), raw: cosine_similarity(&twin, &cluster), centroid: Some(twin.clone()) },
        Candidate { profile_id: "second".into(), raw: cosine_similarity(&twin, &cluster), centroid: Some(twin.clone()) },
    ];
    let cohort = cohort();
    for _ in 0..8 {
        let r = rank_within_meeting(&cluster, &cands, Some(&cohort));
        assert_eq!(r.best().unwrap().profile_id, "first");
        assert_eq!(r.margin().unwrap(), 0.0, "a tie has zero margin, honestly reported");
    }
}

#[test]
fn no_cohort_still_ranks_and_says_why() {
    // The degrade path, per the epic's governing principle: a scoring
    // refinement never costs the user their answer. Without a cohort the
    // ranking is raw cosine — the same answer the codebase would have given
    // before this item existed — and it is LABELLED as such rather than passed
    // off as normalized.
    let cluster = unit(&[(1, 1.0), (2, 0.55)]);
    let profiles = vec![
        ("aidan", unit(&[(1, 1.0), (2, 0.30)])),
        ("jeisil", unit(&[(1, 1.0), (2, 0.52)])),
    ];
    let cands = candidates(&cluster, &profiles);

    let ranking = rank_within_meeting(&cluster, &cands, None);
    assert_eq!(ranking.basis, RankingBasis::RawCosine);
    assert!(ranking.degraded_because.is_some(), "a degrade is always explained");
    assert_eq!(ranking.best().unwrap().profile_id, "jeisil");
    assert!(
        ranking.ordered.iter().all(|c| c.normalized.is_none()),
        "nothing may carry a normalized score when no cohort was used"
    );
}

#[test]
fn a_wrong_width_cohort_degrades_instead_of_scoring_nonsense() {
    // A cohort from a different embedder, or a different model width, is not a
    // slightly worse cohort — it is a different coordinate system, and cosine
    // similarity across the two is a well-formed float with no meaning. It must
    // degrade, not produce a confident wrong ranking.
    let cluster = unit(&[(1, 1.0), (2, 0.55)]);
    let cands = candidates(
        &cluster,
        &[("aidan", unit(&[(1, 1.0), (2, 0.30)])), ("jeisil", unit(&[(1, 1.0), (2, 0.52)]))],
    );
    let narrow = ImpostorCohort::from_rows(vec![vec![1.0, 0.0, 0.0, 0.0]; 6], 3, EMBEDDER).unwrap();

    let ranking = rank_within_meeting(&cluster, &cands, Some(&narrow));
    assert_eq!(ranking.basis, RankingBasis::RawCosine);
    assert!(ranking
        .degraded_because
        .as_deref()
        .unwrap()
        .contains("dim"), "the reason names the width mismatch: {:?}", ranking.degraded_because);
    assert_eq!(ranking.best().unwrap().profile_id, "jeisil");
}

#[test]
fn an_unnormalizable_candidate_never_outranks_a_measured_one() {
    // A candidate without a retained centroid cannot be normalized. Ranking it
    // by its raw score alongside normalized ones would compare two different
    // units — the exact hazard `NormalizedScore` exists to prevent — so it
    // sorts below every measured candidate while staying visible to the user.
    let cluster = unit(&[(1, 1.0), (2, 0.55)]);
    let good = unit(&[(1, 1.0), (2, 0.30)]);
    let cands = vec![
        Candidate {
            profile_id: "no-centroid".into(),
            raw: CosineSimilarity::new(0.999),
            centroid: None,
        },
        Candidate {
            profile_id: "measured".into(),
            raw: cosine_similarity(&good, &cluster),
            centroid: Some(good),
        },
    ];
    let cohort = cohort();
    let ranking = rank_within_meeting(&cluster, &cands, Some(&cohort));

    assert_eq!(
        ranking.best().unwrap().profile_id,
        "measured",
        "a 0.999 raw score must not beat a normalized one in a different unit"
    );
    assert_eq!(ranking.ordered[1].profile_id, "no-centroid");
    assert!(ranking.ordered[1].normalized.is_none());
}

#[test]
fn an_unmeasurable_runner_up_has_no_margin_rather_than_an_infinite_one() {
    // The winner is normalized; the runner-up is not. The gap between a
    // z-score and nothing is not a number, and reporting `+inf` for it would
    // tell a caller the suggestion is maximally safe precisely when the least
    // is known about its competition.
    let cluster = unit(&[(1, 1.0), (2, 0.55)]);
    let good = unit(&[(1, 1.0), (2, 0.52)]);
    let cands = vec![
        Candidate {
            profile_id: "measured".into(),
            raw: cosine_similarity(&good, &cluster),
            centroid: Some(good),
        },
        Candidate {
            profile_id: "no-centroid".into(),
            raw: CosineSimilarity::new(0.98),
            centroid: None,
        },
    ];
    let cohort = cohort();
    let ranking = rank_within_meeting(&cluster, &cands, Some(&cohort));

    assert_eq!(ranking.basis, RankingBasis::AsNorm);
    assert_eq!(ranking.best().unwrap().profile_id, "measured");
    assert_eq!(
        ranking.margin(),
        None,
        "an unmeasurable runner-up leaves the margin unmeasurable, not infinite"
    );

    // And the ordinary case still reports a real, finite number.
    let both = candidates(
        &cluster,
        &[("aidan", unit(&[(1, 1.0), (2, 0.30)])), ("jeisil", unit(&[(1, 1.0), (2, 0.52)]))],
    );
    let ok = rank_within_meeting(&cluster, &both, Some(&cohort));
    let m = ok.margin().expect("two measured candidates have a margin");
    assert!(m.is_finite() && m > 0.0, "margin {m}");
}
