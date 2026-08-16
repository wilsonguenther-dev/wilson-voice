//! YV131 acceptance 3 — evidence toward it. **The criterion itself stays OPEN.**
//!
//! Read the two paragraphs below and the one at the end before quoting any
//! number from this file as the criterion being met. It is not. The spec writes
//! acceptance 3 as a MANUAL check against a real recording; everything here runs
//! over simulated filters, and `speaker_asnorm` has no caller outside test
//! binaries, so no enrolment and no `Suggested` prompt exists end to end for a
//! real recording to be run through.
//!
//! The spec's third criterion is written as a manual check: *"an enrolled
//! profile from a laptop-mic recording is offered as `Suggested` (not silently
//! missed as `New`) when the same person appears in a later AirPods-recorded
//! meeting."* The first draft of this item substituted something else for it and
//! said so quietly — it shipped a RANKING, and a ranking only reorders
//! candidates that some other gate already admitted, so a voice whose
//! cross-device cosine fell under a fixed band was still missed as `New`. The
//! headline capability was not delivered by anything that shipped.
//!
//! This file asks the question the spec asks — *is the right person offered, or
//! missed?* — of two held-out SIMULATED conditions, and compares the shipped
//! [`NormalizedBand`] against the fairest possible cosine band: one chosen by
//! the same rule, on the same tuning split, at the same equal-error operating
//! point. Nothing here is a synthetic geometry; every
//! trial is a real LibriSpeech speaker embedded by the real CAM++.
//!
//! **The honest gap, restated where it is easiest to miss.** The condition
//! change is a SIMULATED filter, not a device. No AirPods were recorded and no
//! volunteer was asked to speak. This turns "the mechanism helps under a
//! simulated shift" into something a test binary can falsify; it does not turn
//! it into "the mechanism helps Wilson", which still needs hardware, a person,
//! and the enrolment path this module is not yet wired to.
//!
//! **And what the mechanism is.** The shipped normalization is enrollment-side
//! only, so the decision below is `cos >= mu_e + band * sigma_e` — a per-profile
//! ABSOLUTE cosine band, condition-blind. What it buys is that one band means
//! the same strictness for every enrolled person, which is why its false-reject
//! rate moves less when the channel does. It does not follow the microphone.
//! See `speaker_asnorm`'s header and `docs/yap23-asnorm-measurement.md`.

mod asnorm_arm;

use asnorm_arm::{eer, measurement, Arm, Scores};
use wilson_voice_lib::speaker_asnorm::{ImpostorCohort, NormalizedScore};

/// The clean arm is the one an enrolment is taken under; the report channel is
/// the one the same person turns up on later.
fn arms(m: &serde_json::Value) -> (Arm, Arm) {
    let report = m["report_subset"].as_str().unwrap();
    (
        Arm::load(m, &format!("{report}|A clean (control)")),
        Arm::load(m, &format!("{report}|{}", m["report_channel"].as_str().unwrap())),
    )
}

/// A cosine band chosen exactly the way the normalized one was: the equal-error
/// crossing on the tuning split, at the tuning channel. This is the comparison
/// that matters — not "AS-norm versus no decision at all", but "AS-norm versus
/// the best fixed cosine band the same procedure would have produced".
fn cosine_band_from_the_tuning_split(m: &serde_json::Value) -> f64 {
    let tune = Arm::load(
        m,
        &format!("{}|{}", m["tune_subset"].as_str().unwrap(), m["report_channel"].as_str().unwrap()),
    );
    eer(&tune.raw_scores()).1
}

#[test]
fn one_band_means_one_thing_under_two_conditions() {
    // The property that makes a single shipped band possible at all: a score
    // has to mean the same thing when the recording condition moves. Measured
    // as the SPREAD of the false-reject rate across two held-out conditions,
    // with one band held fixed — which is exactly what a user experiences as
    // "it recognises me at my desk but not on the couch".
    let m = measurement();
    let (clean, shifted) = arms(&m);
    let band = m["admission_band"].as_f64().unwrap();
    let cosine = cosine_band_from_the_tuning_split(&m);

    let n = |a: &Arm| a.as_norm_scores();
    let r = |a: &Arm| a.raw_scores();

    let norm_gap = (n(&clean).frr(band) - n(&shifted).frr(band)).abs();
    let raw_gap = (r(&clean).frr(cosine) - r(&shifted).frr(cosine)).abs();

    println!(
        "normalized band {band:.3}: FRR {:.1}% clean / {:.1}% shifted (spread {:.1} pp)",
        n(&clean).frr(band) * 100.0,
        n(&shifted).frr(band) * 100.0,
        norm_gap * 100.0
    );
    println!(
        "cosine band     {cosine:.3}: FRR {:.1}% clean / {:.1}% shifted (spread {:.1} pp)",
        r(&clean).frr(cosine) * 100.0,
        r(&shifted).frr(cosine) * 100.0,
        raw_gap * 100.0
    );

    // Note what is and is not being asserted. Neither band TRACKS the condition
    // — the shipped normalization never reads the test embedding, so both are
    // absolute cosine bands and the normalized one is simply chosen per profile
    // (`cos >= mu_e + band * sigma_e`). What must hold is that removing the
    // per-profile offset leaves a band whose false-reject rate is more STABLE
    // when the channel moves, because inter-speaker variance is no longer part
    // of the decision. That is the item's premise; condition tracking is not.
    assert!(
        norm_gap < raw_gap,
        "a per-profile normalized band must hold its false-reject rate steadier \
         across a channel change than a single shared cosine band, or the item's \
         premise is wrong: {:.3} vs {:.3}",
        norm_gap,
        raw_gap
    );
}

#[test]
fn the_normalized_band_beats_a_cosine_band_on_the_cross_device_arm() {
    // "Lower FRR" on its own proves nothing — any band can miss fewer true
    // speakers by admitting more strangers. So the comparison is made at a
    // MATCHED false-accept rate, and the matching is done in the cosine band's
    // favour: it is handed exactly the normalized band's false-accept budget,
    // chosen with knowledge of this held-out arm, which the normalized band was
    // not allowed. If it still misses more of the same people, the difference is
    // the decision rule and not the operating point.
    let m = measurement();
    let (_, shifted) = arms(&m);
    let band = m["admission_band"].as_f64().unwrap();
    let tuned_cosine = cosine_band_from_the_tuning_split(&m);

    let normed = shifted.as_norm_scores();
    let raw = shifted.raw_scores();
    let (nf, nr) = (normed.far(band), normed.frr(band));

    // The most permissive cosine band whose FAR does not exceed the normalized
    // band's: sweep the observed impostor scores downward until FAR would.
    let mut candidates: Vec<f64> = raw.impostor.clone();
    candidates.sort_by(f64::total_cmp);
    candidates.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let matched = candidates
        .iter()
        .copied()
        .filter(|c| raw.far(*c) <= nf)
        .fold(f64::INFINITY, f64::min);
    assert!(matched.is_finite(), "no cosine band reaches this false-accept rate");
    let (mf, mr) = (raw.far(matched), raw.frr(matched));

    println!(
        "{}:\n  cosine band tuned the same way ({tuned_cosine:.3}): FAR {:.2}% FRR {:.2}%\n  \
         cosine band matched to the normalized FAR ({matched:.3}): FAR {:.2}% FRR {:.2}%\n  \
         normalized band ({band:.3}): FAR {:.2}% FRR {:.2}%",
        shifted.name,
        raw.far(tuned_cosine) * 100.0,
        raw.frr(tuned_cosine) * 100.0,
        mf * 100.0,
        mr * 100.0,
        nf * 100.0,
        nr * 100.0
    );

    assert!(mf <= nf, "the matched band must not exceed the normalized FAR budget");
    assert!(
        nr < mr,
        "at the same false-accept rate the normalized band must miss fewer true \
         speakers, or this item has moved an operating point rather than \
         improved a decision: FRR {nr} vs {mr}"
    );
}

#[test]
fn cross_condition_matches_a_cosine_band_misses_are_recovered_and_counted() {
    // The spec's sentence, as a number. Every genuine trial here is the same
    // person under a changed recording condition. The ones a fixed cosine band
    // drops are precisely the "silently missed as New" case; the ones the
    // normalized band admits are the ones this item exists to rescue.
    let m = measurement();
    let (_, shifted) = arms(&m);
    let band = m["admission_band"].as_f64().unwrap();
    let cosine = cosine_band_from_the_tuning_split(&m);

    let normed = shifted.as_norm_scores();
    let raw = shifted.raw_scores();
    assert_eq!(normed.genuine.len(), raw.genuine.len());

    let mut recovered = 0;
    let mut newly_lost = 0;
    for (n, r) in normed.genuine.iter().zip(&raw.genuine) {
        match (*r < cosine, *n < band) {
            (true, false) => recovered += 1,  // missed as New before, Suggested now
            (false, true) => newly_lost += 1, // the honest other direction
            _ => {}
        }
    }
    println!(
        "{}: {recovered} of {} genuine cross-condition trials that a fixed cosine \
         band misses are admitted by the normalized band; {newly_lost} go the \
         other way",
        shifted.name,
        raw.genuine.len()
    );

    assert!(
        recovered > 0,
        "not one cross-condition match is rescued, which is the entire claim"
    );
    assert!(
        recovered > newly_lost,
        "the exchange has to be a net gain, not a reshuffle: {recovered} \
         recovered against {newly_lost} lost"
    );
}

#[test]
fn the_published_operating_point_is_what_the_shipped_band_actually_does() {
    // The band, the cohort manifest and the ladder are three files that can
    // disagree. This is where they cannot: the band the decoder reads is
    // applied, here, to the committed trials, and has to reproduce the FAR and
    // FRR the ladder publishes for every arm whose primitives are committed.
    let m = measurement();
    let cohort = ImpostorCohort::shipped().expect("shipped cohort decodes");
    let band = cohort.admission_band();
    assert!(
        (band.get() as f64 - m["admission_band"].as_f64().unwrap()).abs() < 1e-5,
        "the compiled-in band and the measurement transcript disagree"
    );

    for name in m["primitives"].as_object().unwrap().keys() {
        let arm = Arm::load(&m, name);
        let s = arm.as_norm_scores();
        // Through the shipped predicate, not through a re-implementation of it.
        let admitted = |v: &[f64]| {
            v.iter().filter(|x| band.admits(NormalizedScore::new(**x as f32))).count() as f64
                / v.len() as f64
        };
        let far = admitted(&s.impostor);
        let frr = 1.0 - admitted(&s.genuine);
        let cell = &m["ladder"][name];
        assert!(
            (far - cell["band_far"].as_f64().unwrap()).abs() < 1e-6
                && (frr - cell["band_frr"].as_f64().unwrap()).abs() < 1e-6,
            "{name}: published FAR/FRR {}/{} but the shipped band gives {far}/{frr}",
            cell["band_far"],
            cell["band_frr"]
        );
    }
}

#[test]
fn the_band_answers_suggested_above_it_and_new_voice_below_it() {
    // The direction of the decision, pinned. Everything else in this file
    // measures how WELL the band separates; nothing measured which side of it
    // means "this is them" — a mutation that swapped `Suggested` and `NewVoice`
    // left the whole suite green, which is a coverage gap and not a disclosure.
    use wilson_voice_lib::diarize_metrics::CosineSimilarity;
    use wilson_voice_lib::speaker_asnorm::{rank_within_meeting, Candidate, Suggestion};

    let cohort = ImpostorCohort::shipped().unwrap();
    let band = cohort.admission_band();
    let mut centroid = vec![0.0f32; cohort.dim()];
    centroid[0] = 1.0;
    let stats = cohort.statistics(&centroid).expect("statistics");

    // Two raw scores placed on either side of the band in the band's own unit,
    // derived from the cohort rather than guessed, so this cannot drift when
    // the cohort is regenerated.
    let for_z = |z: f32| CosineSimilarity::new(stats.mean() + z * stats.std_dev());
    let (well_above, well_below) = (band.get() + 3.0, band.get() - 3.0);
    assert!(
        for_z(well_above).get() < 1.0 && for_z(well_below).get() > -1.0,
        "the probe scores must survive the cosine clamp to be probes at all"
    );

    let ask = |raw| {
        let c = [Candidate {
            profile_id: "wilson".into(),
            raw,
            centroid: Some(centroid.clone()),
        }];
        let r = rank_within_meeting(&c, Some(&cohort));
        matches!(r.suggestion(band), Suggestion::Suggested(_))
    };

    assert!(ask(for_z(well_above)), "a score well above the band must be Suggested");
    assert!(!ask(for_z(well_below)), "a score well below the band must be a New voice");
}

#[test]
fn the_band_is_a_decision_and_the_degrade_paths_do_not_make_one() {
    // The band is only reachable when there is a normalized score to compare.
    // Every degrade path in this module hands the question back to the caller's
    // absolute band rather than answering it — "I could not tell" and "this is
    // a stranger" are different answers, and a prompt that confuses them asks
    // the user to name a voice the app never actually recognised.
    use wilson_voice_lib::diarize_metrics::CosineSimilarity;
    use wilson_voice_lib::speaker_asnorm::{rank_within_meeting, Candidate, Suggestion};

    let cohort = ImpostorCohort::shipped().unwrap();
    let band = cohort.admission_band();
    // A centroid pointed somewhere real in the cohort's own space, so the
    // non-degraded case below is a genuine normalization and not an accident.
    let mut centroid = vec![0.0f32; cohort.dim()];
    centroid[0] = 1.0;
    let candidate = Candidate {
        profile_id: "wilson".into(),
        raw: CosineSimilarity::new(0.9),
        centroid: Some(centroid),
    };
    let narrow = Candidate {
        profile_id: "wilson".into(),
        raw: CosineSimilarity::new(0.9),
        centroid: Some(vec![1.0, 0.0, 0.0]),
    };

    // No cohort at all.
    let no_cohort = rank_within_meeting(std::slice::from_ref(&candidate), None);
    assert!(matches!(no_cohort.suggestion(band), Suggestion::NoNormalizedOpinion { .. }));

    // A centroid from another embedder's space — the wrong width.
    let wrong_width = rank_within_meeting(std::slice::from_ref(&narrow), Some(&cohort));
    assert!(matches!(
        wrong_width.suggestion(band),
        Suggestion::NoNormalizedOpinion { .. }
    ));

    // And a real normalized ranking DOES answer, one way or the other.
    let ranking = rank_within_meeting(std::slice::from_ref(&candidate), Some(&cohort));
    let answered = ranking.suggestion(band);
    assert!(
        matches!(answered, Suggestion::Suggested(_) | Suggestion::NewVoice),
        "a normalized ranking must reach a decision, got {answered:?}"
    );
}

/// Compile-time company for [`Scores`], which is otherwise only used through
/// closures above; keeps the import honest rather than allow-listed away.
#[test]
fn a_scores_distribution_reports_both_error_rates() {
    let s = Scores { genuine: vec![1.0, 2.0], impostor: vec![-1.0, 0.0] };
    assert_eq!(s.far(0.5), 0.0);
    assert_eq!(s.frr(1.5), 0.5);
}
