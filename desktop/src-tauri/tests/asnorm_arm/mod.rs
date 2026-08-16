//! YV131 — the measured arm, rebuilt from committed primitives.
//!
//! `docs/yap23-asnorm-measurement.json` does not commit finished AS-norm
//! scores. It commits the PRIMITIVES each trial was computed from — the speaker
//! ids, each side's top-K cohort mean and standard deviation, and the raw
//! cosine — and everything published about this item is recomputed from them,
//! here, through the SHIPPED [`wilson_voice_lib::speaker_asnorm::as_norm_score`].
//!
//! That is the difference between evidence and a screenshot. The first draft of
//! this item committed the finished scores, which meant the shipped formula
//! could be changed — a whole term deleted from it — without a single test
//! noticing: the published numbers had been computed by a Python script that
//! still had the old one. Now the numbers move when the formula moves, and
//! every test that asserts a published number goes red with it.
//!
//! The transcript still carries each trial's TEST-side cohort statistics even
//! though the shipped normalization no longer reads them, so that the forms the
//! design sweep rejected stay recomputable from the same evidence.
//!
//! # Why the EER is computed twice
//!
//! [`Arm::eer`] is a rank-bucket sweep, and YV120's `eer_sweep` is the
//! authority. They are held equal on every real arm before the fast one is used
//! for anything (`crossings_agree_with_the_harness`), because the fast one is
//! only here to make a 2,000-resample bootstrap finish: `eer_sweep` rescans
//! every trial per candidate threshold, which is the right shape for one
//! measurement and the wrong shape for four million.
//!
//! The two are exactly equivalent, not approximately. Sweeping `accept if score
//! >= t` over the distinct observed values reaches every achievable (FAR, FRR)
//! pair: for a midpoint between consecutive values `v_j < v_{j+1}`,
//! `P(impostor >= mid) = P(impostor >= v_{j+1})` and `P(genuine < mid) =
//! P(genuine < v_{j+1})`, so `eer_sweep`'s midpoint candidates are duplicates of
//! candidates already in the set.

#![allow(dead_code)] // each test binary uses a different subset

use std::path::PathBuf;

use wilson_voice_lib::diarize_metrics::{eer_sweep, CosineSimilarity};
use wilson_voice_lib::speaker_asnorm::{as_norm_score, CohortStatistics, NormalizedScore};

/// The committed measurement transcript.
pub fn measurement() -> serde_json::Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/yap23-asnorm-measurement.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} is committed evidence and must exist: {e}", path.display()));
    serde_json::from_str(&text).expect("measurement json parses")
}

/// One test segment: whose it is, and what it scored against every enrolment.
pub struct Trial {
    pub truth: i64,
    pub segment: usize,
    pub test_stats: CohortStatistics,
    /// Raw cosine against each speaker's enrolment, aligned to [`Arm::speakers`].
    pub raw: Vec<CosineSimilarity>,
}

/// One (split, channel) cell, with the speaker structure intact.
pub struct Arm {
    pub name: String,
    pub speakers: Vec<i64>,
    pub enroll_stats: Vec<CohortStatistics>,
    pub trials: Vec<Trial>,
}

/// A labelled score distribution.
#[derive(Clone, Default)]
pub struct Scores {
    pub genuine: Vec<f64>,
    pub impostor: Vec<f64>,
}

impl Scores {
    pub fn far(&self, band: f64) -> f64 {
        self.impostor.iter().filter(|s| **s >= band).count() as f64 / self.impostor.len() as f64
    }

    pub fn frr(&self, band: f64) -> f64 {
        self.genuine.iter().filter(|s| **s < band).count() as f64 / self.genuine.len() as f64
    }
}

fn stats(v: &serde_json::Value) -> CohortStatistics {
    let a = v.as_array().expect("stats triple");
    CohortStatistics::new(
        a[0].as_f64().expect("mean") as f32,
        a[1].as_f64().expect("std dev") as f32,
        a[2].as_u64().expect("k") as usize,
    )
}

impl Arm {
    /// Load one arm by its `subset|channel` key.
    pub fn load(m: &serde_json::Value, name: &str) -> Self {
        let a = m["primitives"][name].as_object().unwrap_or_else(|| {
            panic!("primitives for `{name}` are committed evidence and must exist")
        });
        let speakers: Vec<i64> = a["speakers"]
            .as_array()
            .expect("speakers")
            .iter()
            .map(|s| s.as_i64().expect("speaker id"))
            .collect();
        let enroll_stats: Vec<CohortStatistics> =
            a["enroll_stats"].as_array().expect("enroll_stats").iter().map(stats).collect();
        assert_eq!(
            enroll_stats.len(),
            speakers.len(),
            "one enrolment statistic per speaker, aligned by position"
        );
        let trials: Vec<Trial> = a["tests"]
            .as_array()
            .expect("tests")
            .iter()
            .map(|t| {
                let raw: Vec<CosineSimilarity> = t["raw"]
                    .as_array()
                    .expect("raw")
                    .iter()
                    .map(|s| CosineSimilarity::new(s.as_f64().expect("cosine") as f32))
                    .collect();
                assert_eq!(raw.len(), speakers.len(), "one cosine per enrolled speaker");
                Trial {
                    truth: t["truth"].as_i64().expect("truth"),
                    segment: t["segment"].as_u64().expect("segment") as usize,
                    test_stats: stats(&t["test_stats"]),
                    raw,
                }
            })
            .collect();
        Self { name: name.to_string(), speakers, enroll_stats, trials }
    }

    /// The raw cosine distribution — the baseline this item has to beat.
    pub fn raw_scores(&self) -> Scores {
        let mut s = Scores::default();
        for t in &self.trials {
            for (i, raw) in t.raw.iter().enumerate() {
                let dst = if self.speakers[i] == t.truth { &mut s.genuine } else { &mut s.impostor };
                dst.push(raw.get() as f64);
            }
        }
        s
    }

    /// The AS-norm distribution, computed by the arithmetic that ships.
    ///
    /// `test_stats` is committed in the transcript and deliberately not read
    /// here: the design sweep removed the test-side term, and the primitives
    /// keep it so a reader can recompute the forms that lost.
    pub fn as_norm_scores(&self) -> Scores {
        let mut s = Scores::default();
        for t in &self.trials {
            for (i, raw) in t.raw.iter().enumerate() {
                let n = as_norm_score(*raw, &self.enroll_stats[i]);
                let dst = if self.speakers[i] == t.truth { &mut s.genuine } else { &mut s.impostor };
                dst.push(n.get() as f64);
            }
        }
        s
    }

    /// The same distribution as [`NormalizedScore`]s, for the typed API.
    pub fn as_norm_typed(&self) -> (Vec<NormalizedScore>, Vec<NormalizedScore>) {
        let s = self.as_norm_scores();
        (
            s.genuine.iter().map(|v| NormalizedScore::new(*v as f32)).collect(),
            s.impostor.iter().map(|v| NormalizedScore::new(*v as f32)).collect(),
        )
    }

    /// Trials grouped by the speaker they came from — the unit the bootstrap
    /// resamples. Every trial belongs to exactly one group (its test speaker),
    /// so the partition is exact and nothing is counted twice.
    pub fn by_speaker(&self, pick: impl Fn(&Self) -> Scores) -> Vec<Scores> {
        let all = pick(self);
        let mut out: Vec<Scores> = self.speakers.iter().map(|_| Scores::default()).collect();
        let index = |id: i64| self.speakers.iter().position(|s| *s == id).expect("known speaker");
        let (mut g, mut i) = (0usize, 0usize);
        for t in &self.trials {
            let slot = &mut out[index(t.truth)];
            for k in 0..t.raw.len() {
                if self.speakers[k] == t.truth {
                    slot.genuine.push(all.genuine[g]);
                    g += 1;
                } else {
                    slot.impostor.push(all.impostor[i]);
                    i += 1;
                }
            }
        }
        assert_eq!((g, i), (all.genuine.len(), all.impostor.len()));
        out
    }
}

/// Equal error rate over a labelled distribution — the fast, rank-bucket form.
///
/// Exactly equivalent to YV120's `eer_sweep` (see the module header); held to it
/// by `crossings_agree_with_the_harness` before it is used for anything.
pub fn eer(s: &Scores) -> (f64, f64) {
    assert!(!s.genuine.is_empty() && !s.impostor.is_empty());
    let mut values: Vec<f64> = s.genuine.iter().chain(&s.impostor).copied().collect();
    values.sort_by(f64::total_cmp);
    values.dedup_by(|a, b| (*a - *b).abs() < 1e-12);

    let bucket = |v: f64| values.partition_point(|x| *x < v - 1e-12);
    let mut gc = vec![0usize; values.len() + 1];
    let mut ic = vec![0usize; values.len() + 1];
    for v in &s.genuine {
        gc[bucket(*v)] += 1;
    }
    for v in &s.impostor {
        ic[bucket(*v)] += 1;
    }

    let (ng, ni) = (s.genuine.len() as f64, s.impostor.len() as f64);
    // Candidate j means "accept if score >= values[j]"; j == len means a band
    // above every observation, which rejects everything.
    let mut best = (1.0f64, 0.0f64);
    let mut best_gap = f64::INFINITY;
    let mut below_g = 0usize; // genuine strictly below values[j]
    let mut at_or_above_i = s.impostor.len(); // impostors at or above values[j]
    for j in 0..=values.len() {
        if j > 0 {
            below_g += gc[j - 1];
            at_or_above_i -= ic[j - 1];
        }
        let far = at_or_above_i as f64 / ni;
        let frr = below_g as f64 / ng;
        let gap = (far - frr).abs();
        let rate = (far + frr) / 2.0;
        if gap < best_gap - 1e-12 || ((gap - best_gap).abs() <= 1e-12 && rate < best.0) {
            best_gap = gap;
            best = (rate, if j < values.len() { values[j] } else { values[values.len() - 1] });
        }
    }
    best
}

/// The same sweep through YV120's harness, for the agreement check.
pub fn harness_eer(s: &Scores) -> f64 {
    eer_sweep(&s.genuine, &s.impostor).eer
}

/// xorshift64*, so the published interval is the same interval on every machine
/// and every run. A bootstrap whose answer moves between CI runs is not a
/// measurement, and pulling in a seeded-RNG crate for eleven lines of shift-xor
/// is a dependency this repo does not need.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn next_below(&mut self, n: usize) -> usize {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize) % n
    }
}

/// The result of a speaker-level bootstrap over an EER difference.
pub struct Interval {
    pub point: f64,
    pub lo: f64,
    pub hi: f64,
    pub p_le_zero: f64,
    pub resamples: usize,
}

/// Resample SPEAKERS, not trials, and report the interval on `raw EER − AS-norm EER`.
///
/// The unit matters and the first draft of this item got it wrong by not
/// reporting one at all. Six test segments of one voice are not six independent
/// observations, and the 32 impostor trials one segment generates are not 32:
/// the independent thing this arm samples is the SPEAKER. So a resample draws
/// speakers with replacement and takes each drawn speaker's whole block of
/// trials — genuine and impostor together, since both are generated by that
/// speaker's audio.
///
/// What it does not model, stated rather than implied: the dependence through
/// the CANDIDATE side, where one enrolment appears in every other speaker's
/// impostor trials. Each trial belongs to exactly one block here — its test
/// speaker's — so the partition is exact, but the interval is still narrower
/// than a two-way cluster bootstrap would give.
pub fn bootstrap_eer_delta(arm: &Arm, resamples: usize, seed: u64) -> Interval {
    let point = eer(&arm.raw_scores()).0 - eer(&arm.as_norm_scores()).0;

    // Two thousand resamples of a 25,000-trial arm is four thousand equal-error
    // sweeps, and the obvious implementation — rebuild the score vectors, sort
    // them — takes a minute of CI to answer a question with one number in it.
    // Every resample draws from the SAME finite pool of scores, so each trial's
    // position in the sorted order can be computed once: `Bucketed` holds the
    // distinct values, and a resample becomes two counting arrays and one
    // linear sweep. Same arithmetic, same answer, no sorting in the loop.
    let raw = Bucketed::new(arm, &arm.by_speaker(|a| a.raw_scores()));
    let asn = Bucketed::new(arm, &arm.by_speaker(|a| a.as_norm_scores()));

    let mut rng = Rng::new(seed);
    let mut deltas = Vec::with_capacity(resamples);
    let n = arm.speakers.len();
    let (mut rs, mut as_) = (raw.scratch(), asn.scratch());
    for _ in 0..resamples {
        let picks: Vec<usize> = (0..n).map(|_| rng.next_below(n)).collect();
        deltas.push(raw.eer_of(&picks, &mut rs) - asn.eer_of(&picks, &mut as_));
    }
    deltas.sort_by(f64::total_cmp);
    let q = |p: f64| deltas[((deltas.len() - 1) as f64 * p).round() as usize];
    Interval {
        point,
        lo: q(0.025),
        hi: q(0.975),
        p_le_zero: deltas.iter().filter(|d| **d <= 0.0).count() as f64 / deltas.len() as f64,
        resamples,
    }
}

/// One metric's trials, reduced to positions in a sorted list of distinct
/// values, grouped by speaker.
struct Bucketed {
    values: usize,
    genuine: Vec<Vec<u32>>,
    impostor: Vec<Vec<u32>>,
}

/// Reusable counting arrays, so a resample allocates nothing.
struct Scratch {
    g: Vec<u32>,
    i: Vec<u32>,
}

impl Bucketed {
    fn new(arm: &Arm, blocks: &[Scores]) -> Self {
        let mut values: Vec<f64> = blocks
            .iter()
            .flat_map(|b| b.genuine.iter().chain(&b.impostor))
            .copied()
            .collect();
        values.sort_by(f64::total_cmp);
        values.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        let at = |v: f64| values.partition_point(|x| *x < v - 1e-12) as u32;
        assert_eq!(blocks.len(), arm.speakers.len());
        Self {
            values: values.len(),
            genuine: blocks.iter().map(|b| b.genuine.iter().map(|v| at(*v)).collect()).collect(),
            impostor: blocks.iter().map(|b| b.impostor.iter().map(|v| at(*v)).collect()).collect(),
        }
    }

    fn scratch(&self) -> Scratch {
        Scratch { g: vec![0; self.values + 1], i: vec![0; self.values + 1] }
    }

    fn eer_of(&self, picks: &[usize], s: &mut Scratch) -> f64 {
        s.g.fill(0);
        s.i.fill(0);
        let (mut ng, mut ni) = (0usize, 0usize);
        for p in picks {
            for b in &self.genuine[*p] {
                s.g[*b as usize] += 1;
                ng += 1;
            }
            for b in &self.impostor[*p] {
                s.i[*b as usize] += 1;
                ni += 1;
            }
        }
        let (ngf, nif) = (ng as f64, ni as f64);
        let mut best = 1.0f64;
        let mut best_gap = f64::INFINITY;
        let (mut below_g, mut above_i) = (0usize, ni);
        for j in 0..=self.values {
            if j > 0 {
                below_g += s.g[j - 1] as usize;
                above_i -= s.i[j - 1] as usize;
            }
            let (far, frr) = (above_i as f64 / nif, below_g as f64 / ngf);
            let gap = (far - frr).abs();
            let rate = (far + frr) / 2.0;
            if gap < best_gap - 1e-12 || ((gap - best_gap).abs() <= 1e-12 && rate < best) {
                best_gap = gap;
                best = rate;
            }
        }
        best
    }
}
