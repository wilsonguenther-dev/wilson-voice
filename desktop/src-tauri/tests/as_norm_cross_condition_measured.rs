//! YV131 — the measured arm: AS-norm's benefit is recomputed here, through
//! YV120's harness, from committed per-trial scores.
//!
//! The eval discipline this backlog is built on says a number in a PR body is
//! not evidence. So the generator commits every trial score it produced, and
//! this test recomputes the two equal error rates with
//! `diarize_metrics::enrollment_eer` — the same function YV129's bands come out
//! of — rather than asserting against a number a script printed and a human
//! copied. If AS-norm ever stops beating raw cosine on this arm, this test goes
//! red, and the claim in `speaker_asnorm.rs`'s module docs stops being true in
//! the same commit.
//!
//! # What the arm is, precisely
//!
//! * Voices: LibriSpeech `dev-other`, 33 real speakers, CC-BY-4.0.
//! * Cohort: LibriSpeech `test-clean`, 40 different speakers. Disjoint.
//! * Design (symmetric AS-norm, K=20, two-condition cohort): chosen on
//!   `dev-clean`, a third disjoint set, and frozen before `dev-other` was
//!   scored once.
//! * Channel: `B1 headset->laptop, mild` — a SIMULATED channel change, defined
//!   in `scripts/yv131-build-impostor-cohort.py`. No AirPods were recorded and
//!   no volunteer was asked to speak; the honest name for this is a simulated
//!   condition shift applied to real speech, and it is labelled that way
//!   everywhere it appears.
//!
//! The tune/report split is the part that makes the number mean anything. A
//! design selected and reported on one split is not measured, it is fitted —
//! and this item's first attempt did exactly that, reporting a 15% relative
//! gain that vanished to -3% when a held-out split was finally scored. The
//! numbers below are the ones that survived that.

use std::path::PathBuf;
use wilson_voice_lib::diarize_metrics::{enrollment_eer, CosineSimilarity};
use wilson_voice_lib::speaker_asnorm::{normalized_score_eer, NormalizedScore};

fn measurement() -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/yap23-asnorm-measurement.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is committed evidence and must exist: {e}", path.display()));
    serde_json::from_str(&text).expect("measurement json parses")
}

fn raw_scores(v: &serde_json::Value, key: &str) -> Vec<f64> {
    v["report_trials"][key]
        .as_array()
        .unwrap_or_else(|| panic!("report_trials.{key} is an array"))
        .iter()
        .map(|s| s.as_f64().expect("trial score is a number"))
        .collect()
}

/// Raw cosine trials go through the COSINE-typed entry point, and AS-norm
/// trials go through the NORMALIZED-typed one. Deliberately not the same
/// function: `enrollment_eer` clamps its inputs to `[-1, 1]`, which is correct
/// for a cosine similarity and destructive for a z-score.
fn cosine(v: &serde_json::Value, key: &str) -> Vec<CosineSimilarity> {
    raw_scores(v, key)
        .into_iter()
        .map(|s| CosineSimilarity::new(s as f32))
        .collect()
}

fn normalized(v: &serde_json::Value, key: &str) -> Vec<NormalizedScore> {
    raw_scores(v, key)
        .into_iter()
        .map(|s| NormalizedScore::new(s as f32))
        .collect()
}

#[test]
fn as_norm_beats_raw_cosine_on_the_held_out_arm() {
    let m = measurement();

    let raw = enrollment_eer(&cosine(&m, "raw_genuine"), &cosine(&m, "raw_impostor"));
    let normed = normalized_score_eer(
        &normalized(&m, "as_norm_genuine"),
        &normalized(&m, "as_norm_impostor"),
    );

    println!(
        "held-out {} / {}: raw EER {:.2}% ({} genuine, {} impostor) -> AS-norm EER {:.2}%",
        m["report_subset"].as_str().unwrap(),
        m["report_channel"].as_str().unwrap(),
        raw.eer * 100.0,
        raw.genuine,
        raw.impostor,
        normed.eer * 100.0,
    );

    assert!(
        normed.eer < raw.eer,
        "AS-norm must reduce the equal error rate on the held-out arm: raw \
         {:.4} vs AS-norm {:.4}",
        raw.eer,
        normed.eer
    );

    // The size of the effect, not just its sign. A 1% relative improvement
    // would technically satisfy the assertion above while being indistinguishable
    // from noise on 66 genuine trials; the measured figure is ~20%.
    let relative = (raw.eer - normed.eer) / raw.eer;
    assert!(
        relative > 0.10,
        "the improvement must be more than a rounding artefact: {:.1}% relative",
        relative * 100.0
    );
}

#[test]
fn the_arm_is_large_enough_to_mean_something() {
    let m = measurement();
    let raw = enrollment_eer(&cosine(&m, "raw_genuine"), &cosine(&m, "raw_impostor"));
    assert!(
        raw.genuine >= 60 && raw.impostor >= 2000,
        "an EER over a handful of trials is a coin flip with a decimal point: \
         {} genuine, {} impostor",
        raw.genuine,
        raw.impostor
    );
    assert_eq!(
        normalized(&m, "as_norm_genuine").len(),
        raw.genuine,
        "both scorings must cover the SAME trials, or the comparison is between \
         two different experiments"
    );
    assert_eq!(normalized(&m, "as_norm_impostor").len(), raw.impostor);
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
    // the separate function necessary.
    let m = measurement();
    let all: Vec<f64> = raw_scores(&m, "as_norm_genuine")
        .into_iter()
        .chain(raw_scores(&m, "as_norm_impostor"))
        .collect();

    let outside = all.iter().filter(|s| **s < -1.0 || **s > 1.0).count();
    let fraction = outside as f64 / all.len() as f64;
    println!(
        "{outside}/{} AS-norm scores ({:.1}%) lie outside the cosine range; \
         min {:.3}, max {:.3}",
        all.len(),
        fraction * 100.0,
        all.iter().cloned().fold(f64::INFINITY, f64::min),
        all.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
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
}

#[test]
fn the_full_ladder_is_committed_not_only_the_flattering_rung() {
    // Reporting only the channel where a technique wins is how a measurement
    // becomes marketing. The generator commits every rung, including the
    // far-field ones where the benefit shrinks to almost nothing, and this
    // asserts they are all still there.
    let m = measurement();
    let ladder = m["ladder"].as_object().expect("ladder");
    let report_subset = m["report_subset"].as_str().unwrap();

    let held_out: Vec<_> = ladder
        .iter()
        .filter(|(k, _)| k.starts_with(report_subset))
        .collect();
    assert!(
        held_out.len() >= 5,
        "the held-out ladder must carry every channel, got {}",
        held_out.len()
    );

    let mut improved = 0;
    for (name, cell) in ladder {
        let raw = cell["raw_eer"].as_f64().unwrap();
        let normed = cell["as_norm_eer"].as_f64().unwrap();
        println!("{name:44} raw {:.2}%  AS-norm {:.2}%", raw * 100.0, normed * 100.0);
        if normed < raw {
            improved += 1;
        }
    }
    assert_eq!(
        improved,
        ladder.len(),
        "AS-norm was ahead in every measured cell when this landed; if that has \
         changed, the module docs' claim has to change with it"
    );
}
