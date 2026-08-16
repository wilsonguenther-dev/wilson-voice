//! YV120 — the diarization metrics, held to hand-worked answers.
//!
//! Zero hardware, zero corpus, zero model bytes: DER, JER and enrollment-EER
//! are computable from labeled interval data alone, which is exactly why this
//! file can and must precede the diarization sidecar (YV121) rather than follow
//! it. `cargo test --test diarization_metrics` runs in milliseconds on a fresh
//! clone with nothing installed.
//!
//! Every gate below is a NEGATIVE control as much as a positive one. A metric
//! that has only ever been run on real output cannot tell a bug in the pipeline
//! from a bug in itself, so each metric here is pinned against a two-speaker
//! example small enough to work out on paper — the arithmetic is written into
//! the test — plus at least one case built so that a plausible wrong
//! implementation fails it:
//!
//! * DER is checked against a hypothesis carrying one miss, one false alarm and
//!   one confusion at the same time, and separately against a hypothesis that
//!   is perfect except that it named the speakers differently (a scorer that
//!   matched labels by name would report 100 % error there, and does not).
//! * JER is checked against the same example, and then against the case it
//!   exists for: a meeting one loud speaker dominates, where DER reads a
//!   comfortable 0.10 for a hypothesis that lost a whole speaker and JER reads
//!   0.50.
//! * EER is checked at both endpoints — a fully separated distribution (0.0)
//!   and two identical ones (0.5) — because those are the only two points on
//!   the curve whose answer is known without measuring anything.
//!
//! The functions themselves live in `src/diarize_metrics.rs`; see that module's
//! header for why the definitions are in the library rather than in this file
//! (short version: yap23's shipped clustering API takes a `CosineDistance`, and
//! a newtype declared in a test crate cannot be the type in a `src/` signature).

use wilson_voice_lib::diarize_metrics::{
    cosine_similarity, der, enrollment_eer, is_zero_norm, jer, optimal_speaker_mapping,
    CosineDistance, CosineSimilarity, RttmTurn,
};

/// Floating point equality for numbers that came out of an interval sum.
fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() < 1e-9
}

/// The hand-worked example, shared by the DER and the JER gate so both are
/// pinned to ONE set of intervals whose arithmetic is written out once.
///
/// ```text
///  reference   A ────────────────┤            B ────────────────┤
///              0                10           10                20
///  hypothesis  h1 ──────────┤ h2 ───────────────────────┤   h3 ──┤
///              0            8  8                       18   20  22
/// ```
///
/// Optimal mapping: `A↔h1` (8 s overlapped) + `B↔h2` (8 s) = 16 s, which beats
/// every other pairing (`A↔h2` is worth 2 s and leaves `B` with `h1`'s 0 s).
/// `h3` overlaps no reference speech at all and stays unmapped.
fn hand_worked() -> (Vec<RttmTurn>, Vec<RttmTurn>) {
    let reference = vec![
        RttmTurn::new("A", 0.0, 10.0),
        RttmTurn::new("B", 10.0, 20.0),
    ];
    let hypothesis = vec![
        RttmTurn::new("h1", 0.0, 8.0),
        RttmTurn::new("h2", 8.0, 18.0),
        RttmTurn::new("h3", 20.0, 22.0),
    ];
    (reference, hypothesis)
}

/// Worked on paper, interval by interval:
///
/// | interval | reference | hypothesis | verdict |
/// |---|---|---|---|
/// | 0–8   | A | h1 (=A) | correct |
/// | 8–10  | A | h2 (=B) | **confusion**, 2 s |
/// | 10–18 | B | h2 (=B) | correct |
/// | 18–20 | B | —       | **miss**, 2 s |
/// | 20–22 | — | h3      | **false alarm**, 2 s |
///
/// Total reference speaker time is 20 s, so DER = (2 + 2 + 2) / 20 = **0.30**.
#[test]
fn der_matches_hand_worked_example() {
    let (reference, hypothesis) = hand_worked();
    let report = der(&reference, &hypothesis);
    assert!(close(report.miss, 2.0), "miss: {report:?}");
    assert!(close(report.false_alarm, 2.0), "false alarm: {report:?}");
    assert!(close(report.confusion, 2.0), "confusion: {report:?}");
    assert!(close(report.total, 20.0), "total: {report:?}");
    assert!(close(report.rate(), 0.30), "DER: {}", report.rate());
}

/// The mapping is optimal, not nominal: a hypothesis that is perfect except
/// that it calls the speakers `spk_0` / `spk_1` scores ZERO error.
///
/// This is the single most common way a hand-rolled DER is wrong, and it is
/// wrong in the direction that hides everything — a scorer that matched on
/// label names would report 1.0 for a perfect system, and every later
/// improvement would look like noise against it.
#[test]
fn der_is_zero_when_the_hypothesis_only_renamed_the_speakers() {
    let reference = vec![
        RttmTurn::new("alice", 0.0, 10.0),
        RttmTurn::new("bob", 10.0, 20.0),
        RttmTurn::new("alice", 20.0, 30.0),
    ];
    let hypothesis = vec![
        RttmTurn::new("spk_1", 0.0, 10.0),
        RttmTurn::new("spk_0", 10.0, 20.0),
        RttmTurn::new("spk_1", 20.0, 30.0),
    ];
    let report = der(&reference, &hypothesis);
    assert!(close(report.rate(), 0.0), "DER: {report:?}");
    assert!(close(jer(&reference, &hypothesis), 0.0));

    let mapping = optimal_speaker_mapping(&reference, &hypothesis);
    assert!(mapping.exact, "the exact assignment ran: {mapping:?}");
    assert_eq!(
        mapping.pairs,
        vec![
            ("alice".to_string(), "spk_1".to_string()),
            ("bob".to_string(), "spk_0".to_string())
        ]
    );
}

/// Overlapped reference speech counts once PER SPEAKER, which is md-eval's
/// denominator and the reason fixture (f) can be scored at all.
///
/// Reference: `A` speaks 0–10, `B` joins for 5–10, so total reference speaker
/// time is 15 s, not 10. A hypothesis that heard only one voice throughout is
/// missing 5 s of it: DER = 5 / 15 = 1/3. A scorer that measured the timeline
/// instead of the speaker time would say 0.0 and rate a system that cannot hear
/// overlap as perfect — the exact blindness merged finding #5 is about.
#[test]
fn der_counts_overlapped_reference_speech_once_per_speaker() {
    let reference = vec![RttmTurn::new("A", 0.0, 10.0), RttmTurn::new("B", 5.0, 10.0)];
    let hypothesis = vec![RttmTurn::new("h1", 0.0, 10.0)];
    let report = der(&reference, &hypothesis);
    assert!(close(report.total, 15.0), "total: {report:?}");
    assert!(close(report.miss, 5.0), "miss: {report:?}");
    assert!(close(report.confusion, 0.0), "confusion: {report:?}");
    assert!(close(report.rate(), 1.0 / 3.0), "DER: {}", report.rate());
}

/// Same intervals as [`der_matches_hand_worked_example`], worked per speaker:
///
/// * `A` ∩ `h1` = 8 s over `A` ∪ `h1` = 10 s → `1 - 0.8` = **0.2**
/// * `B` ∩ `h2` = 8 s over `B` ∪ `h2` = 12 s (8–20) → `1 - 8/12` = **1/3**
///
/// JER is the mean over reference speakers: `(0.2 + 1/3) / 2` = **0.2667**.
#[test]
fn jer_matches_hand_worked_example() {
    let (reference, hypothesis) = hand_worked();
    let measured = jer(&reference, &hypothesis);
    let expected = (0.2 + 1.0 / 3.0) / 2.0;
    assert!(close(measured, expected), "JER: {measured} vs {expected}");
}

/// The reason JER is in this harness at all.
///
/// One speaker holds the floor for 900 s of a 1000 s meeting; the other says
/// 100 s and is lost entirely by the hypothesis. DER reads **0.10** — a number
/// that would pass any gate written on a good day — and JER reads **0.50**,
/// because half the people in the room were not found.
#[test]
fn jer_exposes_a_speaker_der_hides() {
    let reference = vec![
        RttmTurn::new("loud", 0.0, 900.0),
        RttmTurn::new("quiet", 900.0, 1000.0),
    ];
    let hypothesis = vec![RttmTurn::new("h1", 0.0, 900.0)];
    let d = der(&reference, &hypothesis);
    assert!(close(d.rate(), 0.10), "DER: {}", d.rate());
    assert!(close(jer(&reference, &hypothesis), 0.5), "JER");
}

/// Two well-separated clusters: every genuine pair scores above every impostor
/// pair, so a threshold exists that makes both error rates zero.
#[test]
fn eer_is_near_zero_for_well_separated_scores() {
    let genuine: Vec<CosineSimilarity> = [0.82, 0.79, 0.91, 0.88, 0.75]
        .iter()
        .map(|s| CosineSimilarity::new(*s))
        .collect();
    let impostor: Vec<CosineSimilarity> = [0.11, 0.20, 0.05, 0.31, -0.04]
        .iter()
        .map(|s| CosineSimilarity::new(*s))
        .collect();
    let report = enrollment_eer(&genuine, &impostor);
    assert!(report.eer < 1e-9, "EER: {report:?}");
    assert!(close(report.far_at_eer, 0.0) && close(report.frr_at_eer, 0.0));
    // The crossing sits in the empty band between the two clusters, which is
    // where an enrollment threshold would be placed.
    let t = report.threshold_at_eer.get();
    assert!((0.31..=0.75).contains(&t), "threshold: {t}");
}

/// Two IDENTICAL distributions: no threshold can separate them, and the
/// equal-error point is a coin flip. If this reported anything materially below
/// 0.5, the sweep would be finding structure that is not in the data — the
/// failure mode that makes an unfalsifiable metric look like a good result.
#[test]
fn eer_is_near_half_for_fully_overlapping_scores() {
    let scores: Vec<CosineSimilarity> = [0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80]
        .iter()
        .map(|s| CosineSimilarity::new(*s))
        .collect();
    let report = enrollment_eer(&scores, &scores);
    assert!(close(report.eer, 0.5), "EER: {report:?}");
    assert!(close(report.far_at_eer, report.frr_at_eer), "{report:?}");
    assert_eq!((report.genuine, report.impostor), (8, 8));
}

/// The one conversion, both ways, plus the bounds.
///
/// `CosineDistance::from_similarity` is `1.0 - sim`, so the pair round-trips
/// exactly in `f32` for every value that matters, and the two ranges are what
/// the types promise: similarity `[-1, 1]`, distance `[0, 2]`.
#[test]
fn cosine_distance_and_similarity_conversion_round_trips() {
    for raw in [-1.0f32, -0.5, -0.125, 0.0, 0.25, 0.35, 0.65, 0.9, 1.0] {
        let similarity = CosineSimilarity::new(raw);
        let distance = CosineDistance::from_similarity(similarity);
        assert!(
            (distance.get() - (1.0 - raw)).abs() < 1e-6,
            "{raw} -> {distance:?}"
        );
        let back = CosineSimilarity::from_distance(distance);
        assert!((back.get() - raw).abs() < 1e-6, "{raw} -> {back:?}");
    }

    // The extremes land exactly on the declared bounds rather than one ULP past
    // them, and out-of-range input from a float dot product is clamped, not
    // rejected: an eval harness that panicked on 1.0000001 would be useless.
    assert_eq!(
        CosineDistance::from_similarity(CosineSimilarity::new(-1.0)).get(),
        CosineDistance::MAX
    );
    assert_eq!(
        CosineDistance::from_similarity(CosineSimilarity::new(1.0)).get(),
        CosineDistance::MIN
    );
    assert_eq!(
        CosineSimilarity::new(1.000_000_1).get(),
        CosineSimilarity::MAX
    );
    assert_eq!(CosineSimilarity::new(-4.0).get(), CosineSimilarity::MIN);
    assert_eq!(CosineDistance::new(9.0).get(), CosineDistance::MAX);
    assert_eq!(CosineDistance::new(-9.0).get(), CosineDistance::MIN);
}

/// Merged finding #20, as a number rather than a warning.
///
/// The plan's enrollment floor is a cosine SIMILARITY of 0.70. sherpa's
/// clustering threshold is a cosine DISTANCE, default 0.5. Reading the first as
/// if it were the second is not a rounding error — it is a threshold of
/// similarity 0.30, less than half as strict, which would merge two different
/// people into one cluster all day and never once look like a bug.
///
/// The types make that swap impossible to write. This test states the size of
/// the mistake they prevent, so the discipline has a reason attached to it and
/// not just a rule.
#[test]
fn reading_a_similarity_as_a_distance_would_halve_the_strictness() {
    let plan_floor = CosineSimilarity::new(0.70);
    let as_distance_by_mistake = CosineDistance::new(plan_floor.get());
    let what_it_would_really_mean = CosineSimilarity::from_distance(as_distance_by_mistake);
    assert!((what_it_would_really_mean.get() - 0.30).abs() < 1e-6);

    // And the correct conversion of the same number, which is what any caller
    // in this crate is forced to write.
    let correct = CosineDistance::from_similarity(plan_floor);
    assert!((correct.get() - 0.30).abs() < 1e-6);
    // Clustering must end up STRICTLY TIGHTER than the identity floor it feeds
    // (finding #20's second half). Expressed in one unit, that is an ordinary
    // comparison — which is the whole point of having one unit.
    let clustering_candidate = CosineDistance::new(0.25);
    assert!(clustering_candidate < correct);
}

/// The ground-truth format really is the standard one: a turn renders to a NIST
/// RTTM line and parses back identically, so a future cross-check against
/// `dscore` or `pyannote.metrics` costs a file write and nothing else.
#[test]
fn rttm_turns_round_trip_through_the_standard_line_format() {
    let turns = [
        RttmTurn::new("spk_a", 0.0, 4.25),
        RttmTurn::new("spk_b", 4.5, 9.125),
        RttmTurn::new("spk_a", 9.0, 12.0),
    ];
    let text: String = turns
        .iter()
        .map(|t| t.to_rttm_line("room-3-near-field"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.starts_with("SPEAKER room-3-near-field 1 0.000 4.250 <NA> <NA> spk_a <NA> <NA>"));

    let parsed = RttmTurn::parse_rttm(&text);
    assert_eq!(parsed.len(), turns.len());
    for (a, b) in parsed.iter().zip(turns.iter()) {
        assert_eq!(a.speaker_id, b.speaker_id);
        assert!(close(a.start_seconds, b.start_seconds));
        assert!(close(a.end_seconds, b.end_seconds));
    }

    // A real RTTM carries records this scorer has no use for. Skipping them is
    // what makes reading one written by an external tool possible at all.
    let with_noise = format!("SPKR-INFO room 1 <NA> <NA> <NA> unknown spk_a <NA> <NA>\n{text}\n");
    assert_eq!(RttmTurn::parse_rttm(&with_noise).len(), turns.len());
}

/// Cosine similarity itself, including the degenerate input an eval sweep will
/// eventually hand it (a silent utterance embeds to zeros, and `NaN` sorts as
/// neither genuine nor impostor).
#[test]
fn cosine_similarity_is_bounded_and_survives_a_zero_embedding() {
    let a = [1.0f32, 0.0, 0.0];
    let b = [2.0f32, 0.0, 0.0];
    let c = [0.0f32, 1.0, 0.0];
    assert!((cosine_similarity(&a, &b).get() - 1.0).abs() < 1e-6);
    assert!(cosine_similarity(&a, &c).get().abs() < 1e-6);
    assert!((cosine_similarity(&a, &[-1.0f32, 0.0, 0.0]).get() + 1.0).abs() < 1e-6);
    assert_eq!(cosine_similarity(&a, &[0.0f32; 3]).get(), 0.0);
}

/// The trap that answer sets, and the predicate that exists to defuse it.
///
/// `0.0` from `cosine_similarity` means "no information", but converted into the
/// distance unit it is `1.0` — "far" — so any caller that RANKS by distance
/// (YV130's farthest-pair split seeding, first of several) would read a silent
/// utterance as the most distinctive voice in the room. `is_zero_norm` is the
/// question those callers must ask first, and it has to agree exactly with the
/// guard inside `cosine_similarity` or the two disagree about which vectors are
/// degenerate.
#[test]
fn is_zero_norm_agrees_with_the_similarity_guard_it_defuses() {
    assert!(is_zero_norm(&[0.0f32; 3]));
    assert!(is_zero_norm(&[]), "no components is no direction");
    assert!(is_zero_norm(&[-0.0f32, 0.0, -0.0]), "signed zeros are still zero");
    assert!(!is_zero_norm(&[1.0f32, 0.0, 0.0]));
    assert!(
        !is_zero_norm(&[f32::MIN_POSITIVE, 0.0, 0.0]),
        "a tiny direction is still a direction: the f64 accumulator must not \
         round it away, or a real embedding would be discarded as degenerate"
    );

    // The agreement, stated as the property: exactly the vectors `is_zero_norm`
    // calls degenerate are the ones `cosine_similarity` answers 0.0 for against
    // a vector they are otherwise parallel to.
    for v in [
        vec![0.0f32; 3],
        vec![1.0f32, 0.0, 0.0],
        vec![f32::MIN_POSITIVE, 0.0, 0.0],
    ] {
        let parallel_similarity = cosine_similarity(&v, &[1.0f32, 0.0, 0.0]).get();
        assert_eq!(
            is_zero_norm(&v),
            parallel_similarity == 0.0,
            "disagreement about {v:?}: similarity {parallel_similarity}"
        );
    }
}

/// The unit discipline, enforced mechanically rather than by review.
///
/// YV120's own acceptance criterion is that no function here takes a bare
/// `f32`/`f64` where a [`CosineDistance`] or [`CosineSimilarity`] is meant —
/// and notes that "Rust can't grep intent". It can grep TEXT, though: this test
/// reads both source files and fails on any declaration whose NAME is
/// threshold-shaped and whose TYPE is a bare float. That turns a review
/// convention into a check that runs on every push, which is what every other
/// convention in this repo that survived did.
#[test]
fn no_threshold_shaped_declaration_takes_a_bare_float() {
    // Names that mean "a number on one side of the distance/similarity line".
    const SHAPED: [&str; 5] = ["threshold", "cosine", "similarity", "distance", "margin"];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = [
        root.join("src/diarize_metrics.rs"),
        root.join("tests/diarization_metrics.rs"),
    ];

    let mut scanned = 0usize;
    let mut saw_the_newtypes = false;
    for path in files {
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} is part of this item: {e}", path.display()));
        for (n, line) in body.lines().enumerate() {
            let trimmed = line.trim_start();
            // Prose is allowed to say "distance" next to "f32"; code is not.
            if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with('#') {
                continue;
            }
            if trimmed.contains("struct CosineDistance(f32)")
                || trimmed.contains("struct CosineSimilarity(f32)")
            {
                saw_the_newtypes = true;
                continue;
            }
            scanned += 1;
            let bare_float = trimmed.contains(": f32")
                || trimmed.contains(": f64")
                || trimmed.contains("-> f32");
            if !bare_float {
                continue;
            }
            for needle in SHAPED {
                assert!(
                    !trimmed.to_ascii_lowercase().contains(needle),
                    "{}:{}: a {needle}-shaped name typed as a bare float — \
                     use CosineDistance / CosineSimilarity so a mixed unit is a \
                     compile error:\n  {trimmed}",
                    path.display(),
                    n + 1
                );
            }
        }
    }
    // The scanner really read the files it claims to have read.
    assert!(scanned > 200, "only {scanned} code lines scanned");
    assert!(
        saw_the_newtypes,
        "the newtype declarations were not found — the scan is pointed at the wrong files"
    );
}
