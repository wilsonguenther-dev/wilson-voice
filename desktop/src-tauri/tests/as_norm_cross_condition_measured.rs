//! YV131 — the measured arm: every published number, recomputed here from
//! primitives, through the arithmetic that ships.
//!
//! The eval discipline this backlog is built on says a number in a PR body is
//! not evidence. The first draft of this item honoured the letter of that and
//! not the spirit: it committed the finished per-trial SCORES, so the tests
//! recomputed an equal error rate over numbers a Python script had already
//! finished computing. The half of the formula this item is named for could be
//! deleted from `src/` without a single test noticing.
//!
//! So the transcript now commits the PRIMITIVES — speaker ids, each side's
//! top-K cohort mean and standard deviation, the raw cosine — and this file
//! rebuilds the scores through `speaker_asnorm::as_norm_score` and sweeps them
//! through YV120's `eer_sweep`. Change the formula and these numbers move.
//!
//! # What the arm is, precisely
//!
//! * Voices: LibriSpeech `dev-other`, 33 real speakers, CC-BY-4.0.
//! * Cohort: LibriSpeech `test-clean`, 40 different speakers. Disjoint.
//! * Design — cohort variant, normalization form and K — chosen on `dev-clean`,
//!   a third disjoint set, by the rule committed in the sweep, and frozen before
//!   `dev-other` was scored.
//! * Channel: `B1 headset->laptop, mild` — a SIMULATED channel change, defined
//!   in `scripts/yv131-build-impostor-cohort.py`. No AirPods were recorded and
//!   no volunteer was asked to speak; the honest name for this is a simulated
//!   condition shift applied to real speech, and it is labelled that way
//!   everywhere it appears.
//!
//! # And the size of the effect, with an interval on it
//!
//! An EER delta with no uncertainty attached is a number of unknown size. This
//! item's first draft reported `15.15% -> 12.12%` over 66 genuine trials — two
//! trials of movement — and pinned `relative > 0.10` as a permanent CI gate on
//! it. A reviewer's paired bootstrap put the 95% interval at `[0.000, 0.061]`.
//!
//! Two things changed. The arm was widened (six test segments per speaker
//! instead of two), and the interval is now computed here, resampling SPEAKERS
//! rather than trials, published beside the point estimate, and asserted at the
//! bound it actually supports instead of a round number chosen for how it reads.

mod asnorm_arm;

use asnorm_arm::{bootstrap_eer_delta, eer, harness_eer, measurement, Arm};

/// Resamples for the published interval. Enough that the 2.5% and 97.5% order
/// statistics are stable to the third decimal on a rerun; small enough that the
/// whole file is a second of CI.
const RESAMPLES: usize = 2000;

/// Fixed, so the interval this repo publishes is the same interval on every
/// machine. A bootstrap that moves between CI runs is not a measurement.
const SEED: u64 = 0x5956_3133_31_u64;

fn held_out(m: &serde_json::Value) -> Arm {
    let name = format!(
        "{}|{}",
        m["report_subset"].as_str().unwrap(),
        m["report_channel"].as_str().unwrap()
    );
    Arm::load(m, &name)
}

#[test]
fn the_fast_sweep_agrees_with_the_harness_on_every_committed_arm() {
    // `asnorm_arm::eer` is a rank-bucket sweep and YV120's `eer_sweep` is the
    // authority. The fast one exists only so a 2,000-resample bootstrap
    // finishes; it is worth nothing unless the two agree, so they are held
    // equal on every real distribution this file uses before either is trusted.
    let m = measurement();
    for name in m["primitives"].as_object().expect("primitives").keys() {
        let arm = Arm::load(&m, name);
        for (label, s) in [("raw", arm.raw_scores()), ("as-norm", arm.as_norm_scores())] {
            let fast = eer(&s).0;
            let authority = harness_eer(&s);
            assert!(
                (fast - authority).abs() < 1e-12,
                "{name} / {label}: fast sweep {fast} vs eer_sweep {authority}"
            );
        }
    }
}

#[test]
fn the_committed_ladder_is_what_the_shipped_formula_computes() {
    // The ladder is a summary table, and a summary table is exactly the kind of
    // artefact that keeps its old numbers after the code under it changes. For
    // every arm whose primitives are committed, the published cell is
    // recomputed here through `as_norm_score` and must match.
    let m = measurement();
    let ladder = m["ladder"].as_object().expect("ladder");
    for name in m["primitives"].as_object().unwrap().keys() {
        let arm = Arm::load(&m, name);
        let cell = &ladder[name];
        let raw = eer(&arm.raw_scores()).0;
        let normed = eer(&arm.as_norm_scores()).0;
        assert!(
            (raw - cell["raw_eer"].as_f64().unwrap()).abs() < 1e-9,
            "{name}: published raw EER {} but the committed trials give {raw}",
            cell["raw_eer"]
        );
        assert!(
            (normed - cell["as_norm_eer"].as_f64().unwrap()).abs() < 1e-9,
            "{name}: published AS-norm EER {} but the shipped formula gives {normed}",
            cell["as_norm_eer"]
        );
    }
}

#[test]
fn as_norm_beats_raw_cosine_on_the_held_out_arm() {
    let m = measurement();
    let arm = held_out(&m);
    let raw = arm.raw_scores();
    let normed = arm.as_norm_scores();
    let (raw_eer, _) = eer(&raw);
    let (as_eer, _) = eer(&normed);

    println!(
        "held-out {}: raw EER {:.2}% ({} genuine, {} impostor, {} speakers) -> AS-norm EER {:.2}%",
        arm.name,
        raw_eer * 100.0,
        raw.genuine.len(),
        raw.impostor.len(),
        arm.speakers.len(),
        as_eer * 100.0,
    );

    assert!(
        as_eer < raw_eer,
        "AS-norm must reduce the equal error rate on the held-out arm: raw \
         {raw_eer:.4} vs AS-norm {as_eer:.4}"
    );
}

#[test]
fn the_effect_is_published_with_a_speaker_level_interval() {
    // The assertion this test used to make was `relative > 0.10` — a hard CI
    // gate on an effect whose confidence interval touched zero. It is replaced
    // by the bound the interval actually supports, and the interval is printed
    // so the PR transcript carries it rather than leaving a reader to compute
    // it, which is how the old number survived review.
    let m = measurement();
    let arm = held_out(&m);
    let ci = bootstrap_eer_delta(&arm, RESAMPLES, SEED);

    println!(
        "held-out EER reduction {:.2} pp, speaker-level bootstrap 95% CI \
         [{:.2}, {:.2}] pp over {} resamples of {} speakers, P(delta <= 0) = {:.1}%",
        ci.point * 100.0,
        ci.lo * 100.0,
        ci.hi * 100.0,
        ci.resamples,
        arm.speakers.len(),
        ci.p_le_zero * 100.0,
    );

    assert!(ci.point > 0.0, "the point estimate must favour AS-norm: {}", ci.point);
    assert!(
        ci.lo > 0.0,
        "the 95% interval must exclude zero, or this item's headline is a claim \
         about noise: [{:.4}, {:.4}]",
        ci.lo,
        ci.hi
    );
    assert!(
        ci.p_le_zero < 0.025,
        "P(delta <= 0) = {:.3}, which is not a separable effect",
        ci.p_le_zero
    );

    // The published document has to carry this interval verbatim. A number that
    // lives only in a test log is a number the next reader will not find.
    let doc = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/yap23-asnorm-measurement.md"),
    )
    .expect("the measurement doc is committed");
    let quoted = format!("[{:.2}, {:.2}]", ci.lo * 100.0, ci.hi * 100.0);
    assert!(
        doc.contains(&quoted),
        "docs/yap23-asnorm-measurement.md must quote the interval this test \
         computes, and it does not carry `{quoted}`"
    );
}

#[test]
fn the_arm_is_large_enough_to_mean_something() {
    let m = measurement();
    let arm = held_out(&m);
    let raw = arm.raw_scores();
    assert!(
        raw.genuine.len() >= 150 && raw.impostor.len() >= 5000,
        "an EER over a handful of trials is a coin flip with a decimal point: \
         {} genuine, {} impostor",
        raw.genuine.len(),
        raw.impostor.len()
    );
    assert!(
        arm.speakers.len() >= 30,
        "the independent unit is the SPEAKER, and {} of them is a thin interval",
        arm.speakers.len()
    );
    assert_eq!(
        arm.as_norm_scores().genuine.len(),
        raw.genuine.len(),
        "both scorings must cover the SAME trials, or the comparison is between \
         two different experiments"
    );
}

#[test]
fn the_measured_scores_do_not_fit_in_the_cosine_range() {
    // Why `normalized_score_eer` exists instead of reusing `enrollment_eer`.
    //
    // Honest about the strength of this: on THIS arm, routing the AS-norm
    // scores through the cosine-typed entry point happens to produce the same
    // EER, because clamping is monotone non-decreasing and the crossing point
    // lands inside [-1, 1] where the order survives. The mutation log records
    // that as a disclosed green rather than pretending otherwise.
    //
    // What is NOT a matter of luck is the input: the overwhelming majority of
    // these scores are outside the range a `CosineSimilarity` can hold, so the
    // clamp fuses them into ties at the boundary and throws the distribution
    // away. Nothing guarantees the crossing stays in the surviving region for
    // the next cohort, channel, or corpus. This test pins the fact that makes
    // the separate function necessary — and it applies to the BAND too, which
    // is a z-score a `CosineSimilarity` could not hold either.
    let m = measurement();
    let arm = held_out(&m);
    let s = arm.as_norm_scores();
    let all: Vec<f64> = s.genuine.iter().chain(&s.impostor).copied().collect();

    let outside = all.iter().filter(|s| **s < -1.0 || **s > 1.0).count();
    let fraction = outside as f64 / all.len() as f64;
    println!(
        "{outside}/{} AS-norm scores ({:.1}%) lie outside the cosine range; \
         min {:.3}, max {:.3}; the shipped band is {:.3}",
        all.len(),
        fraction * 100.0,
        all.iter().cloned().fold(f64::INFINITY, f64::min),
        all.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        m["admission_band"].as_f64().unwrap(),
    );
    assert!(
        fraction > 0.5,
        "if normalized scores ever fit inside [-1, 1], this whole guard is \
         moot — but they do not: only {:.1}% are outside",
        fraction * 100.0
    );

    // The concrete damage a clamp does, measured: distinct values collapse.
    let distinct = |v: &[f64]| {
        let mut s: Vec<f64> = v.to_vec();
        s.sort_by(f64::total_cmp);
        s.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        s.len()
    };
    let clamped: Vec<f64> = all.iter().map(|s| s.clamp(-1.0, 1.0)).collect();
    assert!(
        distinct(&clamped) * 2 < distinct(&all),
        "clamping must visibly destroy the distribution: {} distinct values \
         become {}",
        distinct(&all),
        distinct(&clamped)
    );
}

#[test]
fn the_tune_and_report_splits_are_different_corpora() {
    // The guard against the failure this item actually made once: choosing the
    // design and reporting it on the same speakers.
    let m = measurement();
    let tune = m["tune_subset"].as_str().expect("tune_subset");
    let report = m["report_subset"].as_str().expect("report_subset");
    assert_ne!(
        tune, report,
        "a design chosen and reported on one split is fitted, not measured"
    );
    assert_eq!(
        m["design_choice"]["chosen_on"].as_str().unwrap(),
        tune,
        "the design sweep must have run on the tuning split"
    );
}

#[test]
fn the_design_sweep_is_committed_and_the_shipped_design_is_its_argmin() {
    // The gap an adversarial review found in the first draft: K was a tuned
    // number with no tuning transcript anywhere, a hard-coded module constant
    // with no sweep mode, and two committed files disagreed about which split
    // had chosen it. Now the whole sweep is committed and the shipped design has
    // to be its argmin under the rule the sweep itself states — so a reader can
    // check the pick by looking, and a future retune cannot quietly not happen.
    let m = measurement();
    let sweep = m["design_sweep"]["results"].as_array().expect("design sweep results");
    assert!(sweep.len() >= 20, "a sweep of {} cells is not a sweep", sweep.len());

    let ks: std::collections::BTreeSet<u64> =
        sweep.iter().map(|r| r["k"].as_u64().unwrap()).collect();
    let forms: std::collections::BTreeSet<&str> =
        sweep.iter().map(|r| r["form"].as_str().unwrap()).collect();
    assert!(ks.len() >= 5, "K was swept over {} values", ks.len());
    assert!(
        forms.len() >= 3,
        "the normalization FORM is a design choice too and has to be swept, not \
         assumed from a citation: {forms:?}"
    );

    let shipped_k = m["top_k"].as_u64().unwrap();
    let shipped_form = m["normalization_form"].as_str().unwrap();
    let shipped_cohort = m["cohort_variant"].as_str().unwrap();

    let best = sweep
        .iter()
        .map(|r| r["as_norm_eer"].as_f64().unwrap())
        .fold(f64::INFINITY, f64::min);
    let shipped_cell = sweep
        .iter()
        .find(|r| {
            r["k"].as_u64().unwrap() == shipped_k
                && r["form"].as_str().unwrap() == shipped_form
                && r["cohort"].as_str().unwrap() == shipped_cohort
        })
        .expect("the shipped design must appear in the sweep that chose it");
    assert!(
        (shipped_cell["as_norm_eer"].as_f64().unwrap() - best).abs() < 1e-12,
        "the shipped design (cohort={shipped_cohort}, form={shipped_form}, \
         K={shipped_k}) scores {} on the tuning split, but {best} was available \
         — either the rule was not applied or the transcript is stale",
        shipped_cell["as_norm_eer"]
    );

    // And the manifest the app compiles in has to agree with the transcript.
    let manifest: serde_json::Value =
        serde_json::from_str(wilson_voice_lib::speaker_asnorm::shipped_manifest_json()).unwrap();
    assert_eq!(manifest["top_k"].as_u64().unwrap(), shipped_k);
    assert_eq!(manifest["normalization_form"].as_str().unwrap(), shipped_form);
}

#[test]
fn the_full_ladder_is_committed_not_only_the_flattering_rung() {
    // Reporting only the channel where a technique wins is how a measurement
    // becomes marketing. The generator commits every rung, including the
    // far-field ones where the benefit shrinks to almost nothing, and this
    // asserts they are all still there — with the band's operating point on
    // each, because an EER is a property of a distribution and a shipped band
    // is a property of a decision.
    let m = measurement();
    let ladder = m["ladder"].as_object().expect("ladder");
    let report_subset = m["report_subset"].as_str().unwrap();

    let held_out: Vec<_> = ladder.iter().filter(|(k, _)| k.starts_with(report_subset)).collect();
    assert!(
        held_out.len() >= 5,
        "the held-out ladder must carry every channel, got {}",
        held_out.len()
    );

    // The shape of the result, asserted as the shape it actually is: AS-norm
    // wins on every SHIFTED channel and LOSES on the matched-condition control,
    // where there is no condition shift to correct for and the normalization is
    // pure added variance. The first draft of this test asserted "ahead in every
    // cell", which was true of an arm half this size and is not true here. The
    // trade is the finding, so the trade is what gets pinned.
    let mut shifted_wins = 0;
    let mut shifted_cells = 0;
    let mut control_cells = 0;
    for (name, cell) in ladder {
        let raw = cell["raw_eer"].as_f64().unwrap();
        let normed = cell["as_norm_eer"].as_f64().unwrap();
        println!(
            "{name:46} raw {:.2}%  AS-norm {:.2}%  band FAR {:.2}% FRR {:.2}%",
            raw * 100.0,
            normed * 100.0,
            cell["band_far"].as_f64().unwrap() * 100.0,
            cell["band_frr"].as_f64().unwrap() * 100.0,
        );
        assert!(cell["band_far"].is_number() && cell["band_frr"].is_number());
        if name.contains("|A clean") {
            control_cells += 1;
            continue;
        }
        shifted_cells += 1;
        if normed < raw {
            shifted_wins += 1;
        }
    }
    assert_eq!(control_cells, 2, "both splits must publish the matched-condition control");
    assert_eq!(
        shifted_wins, shifted_cells,
        "AS-norm has to win every channel where there IS a condition shift — \
         that is the only thing this item claims"
    );
}
