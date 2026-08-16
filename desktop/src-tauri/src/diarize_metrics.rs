//! YV120 — the diarization eval metrics (DER, JER, enrollment-EER) and the two
//! cosine units that must never be mixed.
//!
//! This lands BEFORE any diarization code exists, for the same reason YV90's
//! WER/seam harness landed before any capture code did: the epic plan's finding
//! #16 is that every accuracy number in it is quoted from a third party and
//! never measured on this pipeline, and its acceptance criteria are
//! unfalsifiable without one. There has to be a harness before there is
//! anything to tune, so the harness is item one.
//!
//! Everything here is PURE: labeled interval data in, numbers out. No sidecar,
//! no model bytes, no audio hardware, no `sherpa-onnx`. That is what makes the
//! metrics themselves testable today, against hand-worked examples whose right
//! answer was computed on paper — a metric nobody has checked cannot falsify
//! anything it is later pointed at.
//!
//! ## Why this is a library module and not only a test binary
//!
//! The item's file list names `tests/diarization_metrics.rs`, and that file
//! exists — it is where every one of these functions is held to a hand-worked
//! answer. The DEFINITIONS live here, in the crate, because the unit discipline
//! below is only worth anything if it reaches the shipped code:
//!
//! * YV126's `diarize.rs::cluster_track` takes a [`CosineDistance`], not an
//!   `f32`. A newtype defined inside a test binary cannot be the type on a
//!   `src/` function signature, so a test-only definition would force YV126 to
//!   declare a second one — which is the exact mixed-unit hazard this item was
//!   written to make impossible.
//! * Four separate test binaries (YV122's embed smoke, YV124's anti-alias EER
//!   arm, YV126's clustering tune, YV129's enrollment bands) each need these
//!   functions. Rust integration tests are separate crates; they can share a
//!   `tests/support/` module or a library module, and only the library module
//!   also reaches `src/`.
//!
//! Same argument `meeting_asr` and `resample` are already public for: the eval
//! harness must score the SHIPPED thing, never a copy of it.
//!
//! ## The unit discipline (merged finding #20)
//!
//! `sherpa_onnx::FastClusteringConfig.threshold` is a **distance** — smaller
//! means more similar, and its default is 0.5. The plan's enrollment bands
//! (0.70 / 0.55, quoted from OpenWhispr's blog) are cosine **similarities** —
//! larger means more similar. As specified, clustering was LOOSER than the
//! identity decision it feeds, and nothing in a codebase of bare `f32`s would
//! ever have said so.
//!
//! So the two live in the type system: [`CosineDistance`] and
//! [`CosineSimilarity`] are distinct newtypes, with exactly one conversion
//! between them ([`CosineDistance::from_similarity`] and its inverse), and no
//! arithmetic that mixes them. Passing a similarity where a distance belongs is
//! a compile error everywhere in this crate, not a silent accuracy regression
//! that shows up months later as "diarization is a bit worse than the blog
//! said".
//!
//! ## What is deliberately NOT here
//!
//! No thresholds. Not one. Every threshold in this epic is an OUTPUT of these
//! functions measured against a fixture (YV126 for clustering, YV129 for the
//! enrollment bands), never an input copied from a vendor blog. A `const` in
//! this file with a number in it would be the whole failure this item exists to
//! prevent.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// The two cosine units
// ---------------------------------------------------------------------------

/// Cosine SIMILARITY: `1.0` is the same direction, `-1.0` the opposite.
/// **Larger is more similar.** This is the unit an enrollment decision is made
/// in ("is this the voice we enrolled?").
///
/// Constructed through [`CosineSimilarity::new`], which clamps to `[-1.0, 1.0]`
/// rather than rejecting: a dot product of two unit vectors legitimately comes
/// back as `1.0000001` in `f32`, and an eval harness that panicked on floating
/// point noise would be useless.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CosineSimilarity(f32);

/// Cosine DISTANCE: `0.0` is identical, `2.0` is opposite.
/// **Smaller is more similar.** This is the unit sherpa-onnx's clustering runs
/// in, and therefore the unit every clustering threshold in this codebase is
/// expressed in.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CosineDistance(f32);

impl CosineSimilarity {
    pub const MIN: f32 = -1.0;
    pub const MAX: f32 = 1.0;

    /// Clamped into `[-1.0, 1.0]`.
    pub fn new(value: f32) -> Self {
        Self(value.clamp(Self::MIN, Self::MAX))
    }

    /// The raw number. Named `get` rather than exposing the field so that every
    /// escape from the type is one greppable call.
    pub fn get(self) -> f32 {
        self.0
    }

    /// The one conversion, in the other direction: `1.0 - distance`.
    pub fn from_distance(distance: CosineDistance) -> Self {
        Self::new(1.0 - distance.get())
    }
}

impl CosineDistance {
    pub const MIN: f32 = 0.0;
    pub const MAX: f32 = 2.0;

    /// Clamped into `[0.0, 2.0]`.
    pub fn new(value: f32) -> Self {
        Self(value.clamp(Self::MIN, Self::MAX))
    }

    pub fn get(self) -> f32 {
        self.0
    }

    /// The ONE conversion point between the two units: `1.0 - similarity`.
    ///
    /// Exercised by `cosine_distance_and_similarity_conversion_round_trips` and
    /// by the boundary in `diarize.rs` where a wire `f32` becomes a typed
    /// threshold — nowhere else. Every other call site in this crate is a
    /// compile-time-enforced single unit.
    pub fn from_similarity(similarity: CosineSimilarity) -> Self {
        Self::new(1.0 - similarity.get())
    }
}

/// Does this embedding carry no direction at all?
///
/// The companion of [`cosine_similarity`]'s zero-norm convention, and the
/// reason it exists as a named predicate rather than an inline check: `0.0`
/// coming back from `cosine_similarity` means **absent information**, but
/// `CosineDistance::from_similarity(0.0)` is `1.0`, which in the distance unit
/// means **far**. Any caller that ranks or partitions by distance therefore has
/// to ask this question BEFORE it measures, or it will read "we know nothing
/// about this vector" as "this vector is further away than anything else" —
/// which is how a single all-zero embedding hijacks farthest-pair seeding.
///
/// The rule is exactly the one [`cosine_similarity`] guards with (`norm <= 0`),
/// accumulated in `f64` for the same reason, so the two can never disagree
/// about which vectors are degenerate.
pub fn is_zero_norm(v: &[f32]) -> bool {
    let mut norm = 0.0f64;
    for x in v {
        norm += (*x as f64) * (*x as f64);
    }
    norm <= 0.0
}

/// The cosine similarity of two embeddings.
///
/// Zero-norm input (a silent utterance, an all-zero embedding) returns `0.0`
/// rather than `NaN`: an orthogonal, maximally uninformative answer is the
/// honest one, and a `NaN` propagating into an EER sweep would sort as neither
/// genuine nor impostor. A caller that needs to tell "uninformative" apart from
/// "orthogonal" — anything ranking by distance — must ask [`is_zero_norm`]
/// first; this function cannot say it in its return type.
///
/// # Panics
/// If the two embeddings are of different length — comparing a 192-dim CAM++
/// vector against anything else is a bug at the call site, not a low score.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> CosineSimilarity {
    assert_eq!(
        a.len(),
        b.len(),
        "cosine similarity of embeddings with different dimensions"
    );
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += (*x as f64) * (*y as f64);
        na += (*x as f64) * (*x as f64);
        nb += (*y as f64) * (*y as f64);
    }
    if na <= 0.0 || nb <= 0.0 {
        return CosineSimilarity::new(0.0);
    }
    CosineSimilarity::new((dot / (na.sqrt() * nb.sqrt())) as f32)
}

// ---------------------------------------------------------------------------
// RTTM-shaped ground truth
// ---------------------------------------------------------------------------

/// One speaker turn, in the shape `pyannote.metrics` and `dscore` — the two
/// standard external DER scorers — consume, so a future cross-check against
/// either costs a `to_rttm_line` call and nothing else.
///
/// Times are seconds from the start of the recording. `end_seconds` is
/// exclusive; a zero-length turn contributes nothing to any metric here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RttmTurn {
    pub speaker_id: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
}

impl RttmTurn {
    pub fn new(speaker_id: impl Into<String>, start_seconds: f64, end_seconds: f64) -> Self {
        Self {
            speaker_id: speaker_id.into(),
            start_seconds,
            end_seconds,
        }
    }

    pub fn duration(&self) -> f64 {
        (self.end_seconds - self.start_seconds).max(0.0)
    }

    /// The NIST RTTM line for this turn: the format `md-eval.pl`, `dscore` and
    /// `pyannote.metrics` all read.
    ///
    /// `SPEAKER <file> 1 <onset> <duration> <NA> <NA> <speaker> <NA> <NA>`
    pub fn to_rttm_line(&self, file_id: &str) -> String {
        format!(
            "SPEAKER {} 1 {:.3} {:.3} <NA> <NA> {} <NA> <NA>",
            file_id,
            self.start_seconds,
            self.duration(),
            self.speaker_id
        )
    }

    /// Parse the same format back. Unknown record types (`SPKR-INFO`, comments)
    /// are skipped rather than failing — an RTTM written by an external tool
    /// carries them, and refusing to read one would defeat the whole point of
    /// speaking the standard format.
    pub fn parse_rttm(text: &str) -> Vec<RttmTurn> {
        let mut out = Vec::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 8 || !f[0].eq_ignore_ascii_case("SPEAKER") {
                continue;
            }
            let (Ok(onset), Ok(dur)) = (f[3].parse::<f64>(), f[4].parse::<f64>()) else {
                continue;
            };
            out.push(RttmTurn::new(f[7], onset, onset + dur));
        }
        out
    }
}

/// Every turn of one speaker, merged into disjoint intervals.
///
/// A speaker who is annotated twice over the same second is speaking once — a
/// scorer that summed the two would report more reference speech than the
/// recording contains and quietly deflate every rate computed against it.
fn merged_spans(turns: &[RttmTurn], speaker: &str) -> Vec<(f64, f64)> {
    let mut spans: Vec<(f64, f64)> = turns
        .iter()
        .filter(|t| t.speaker_id == speaker && t.duration() > 0.0)
        .map(|t| (t.start_seconds, t.end_seconds))
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        match out.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

/// The distinct speaker ids in `turns`, in first-appearance order made stable by
/// a sort — the mapping below is a search over these, and a scorer whose answer
/// depended on JSON key order would not be a scorer.
fn speakers_of(turns: &[RttmTurn]) -> Vec<String> {
    let mut ids: Vec<String> = turns
        .iter()
        .filter(|t| t.duration() > 0.0)
        .map(|t| t.speaker_id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn overlap(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    let mut total = 0.0;
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        let lo = a[i].0.max(b[j].0);
        let hi = a[i].1.min(b[j].1);
        if hi > lo {
            total += hi - lo;
        }
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    total
}

fn union_duration(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    let total_a: f64 = a.iter().map(|(s, e)| e - s).sum();
    let total_b: f64 = b.iter().map(|(s, e)| e - s).sum();
    total_a + total_b - overlap(a, b)
}

/// How many speakers a side may have before [`optimal_speaker_mapping`] stops
/// searching exhaustively and falls back to a greedy pass.
///
/// A six-person classroom (fixture (f)) is the largest thing yap23 targets, so
/// twelve is a guard rather than a limit anything here reaches — and the
/// fallback reports itself in [`SpeakerMapping::exact`] instead of silently
/// returning a worse-than-optimal error rate.
pub const EXACT_MAPPING_LIMIT: usize = 12;

#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerMapping {
    /// `(reference speaker, hypothesis speaker)`, one entry per MAPPED
    /// reference speaker. Reference speakers with no counterpart are absent.
    pub pairs: Vec<(String, String)>,
    /// Total overlapped speech the mapping accounts for, in seconds.
    pub overlapped_seconds: f64,
    /// False when the speaker count exceeded [`EXACT_MAPPING_LIMIT`] and the
    /// greedy fallback ran.
    pub exact: bool,
}

impl SpeakerMapping {
    fn hypothesis_for(&self, reference: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(r, _)| r == reference)
            .map(|(_, h)| h.as_str())
    }
}

/// The one-to-one reference→hypothesis speaker mapping that maximises total
/// overlapped speech — what `md-eval.pl` calls the optimal mapping, and the
/// reason a hypothesis that got every boundary right but named the speakers
/// `spk_0`/`spk_1` scores 0 % error rather than 100 %.
///
/// Exact by dynamic programming over subsets of hypothesis speakers while both
/// sides are small ([`EXACT_MAPPING_LIMIT`]); greedy beyond that, which
/// [`SpeakerMapping::exact`] reports.
pub fn optimal_speaker_mapping(reference: &[RttmTurn], hypothesis: &[RttmTurn]) -> SpeakerMapping {
    let refs = speakers_of(reference);
    let hyps = speakers_of(hypothesis);
    let ref_spans: Vec<Vec<(f64, f64)>> = refs
        .iter()
        .map(|s| merged_spans(reference, s))
        .collect::<Vec<_>>();
    let hyp_spans: Vec<Vec<(f64, f64)>> = hyps
        .iter()
        .map(|s| merged_spans(hypothesis, s))
        .collect::<Vec<_>>();
    let cost: Vec<Vec<f64>> = ref_spans
        .iter()
        .map(|r| hyp_spans.iter().map(|h| overlap(r, h)).collect())
        .collect();

    if refs.is_empty() || hyps.is_empty() {
        return SpeakerMapping {
            pairs: Vec::new(),
            overlapped_seconds: 0.0,
            exact: true,
        };
    }

    if refs.len() <= EXACT_MAPPING_LIMIT && hyps.len() <= EXACT_MAPPING_LIMIT {
        let (pairs, total) = exact_mapping(&refs, &hyps, &cost);
        return SpeakerMapping {
            pairs,
            overlapped_seconds: total,
            exact: true,
        };
    }

    let (pairs, total) = greedy_mapping(&refs, &hyps, &cost);
    SpeakerMapping {
        pairs,
        overlapped_seconds: total,
        exact: false,
    }
}

/// Assignment by DP over subsets of hypothesis speakers: `best[k][mask]` is the
/// most overlap obtainable after deciding the first `k` reference speakers,
/// having used exactly the hypothesis speakers in `mask`. Reference speakers may
/// be left unmapped (the `k+1, mask` transition), which is what makes it correct
/// when the hypothesis found fewer speakers than there are.
fn exact_mapping(
    refs: &[String],
    hyps: &[String],
    cost: &[Vec<f64>],
) -> (Vec<(String, String)>, f64) {
    let n = refs.len();
    let m = hyps.len();
    let masks = 1usize << m;
    let neg = f64::NEG_INFINITY;
    let mut best = vec![neg; (n + 1) * masks];
    // `choice[k][mask]` = the hypothesis index reference `k` took, or `usize::MAX`
    // for "left unmapped", recorded so the winning assignment can be read back.
    let mut choice = vec![usize::MAX; (n + 1) * masks];
    let at = |k: usize, mask: usize| k * masks + mask;
    best[at(0, 0)] = 0.0;

    for k in 0..n {
        for mask in 0..masks {
            let here = best[at(k, mask)];
            if here == neg {
                continue;
            }
            // Leave reference speaker `k` unmapped.
            if here > best[at(k + 1, mask)] {
                best[at(k + 1, mask)] = here;
                choice[at(k + 1, mask)] = usize::MAX;
            }
            for (j, gain) in cost[k].iter().enumerate() {
                if mask & (1 << j) != 0 {
                    continue;
                }
                let next = mask | (1 << j);
                let value = here + gain;
                if value > best[at(k + 1, next)] {
                    best[at(k + 1, next)] = value;
                    choice[at(k + 1, next)] = j;
                }
            }
        }
    }

    let mut end_mask = 0usize;
    let mut total = neg;
    for mask in 0..masks {
        if best[at(n, mask)] > total {
            total = best[at(n, mask)];
            end_mask = mask;
        }
    }

    let mut pairs = Vec::new();
    let mut mask = end_mask;
    for k in (0..n).rev() {
        let taken = choice[at(k + 1, mask)];
        if taken != usize::MAX {
            pairs.push((refs[k].clone(), hyps[taken].clone()));
            mask &= !(1 << taken);
        }
    }
    pairs.reverse();
    // A pairing worth zero overlap is not a pairing — it would let JER call two
    // strangers a match and report `1.0` for a speaker that was simply missed.
    pairs.retain(|(r, h)| {
        let ri = refs.iter().position(|x| x == r).expect("ref index");
        let hi = hyps.iter().position(|x| x == h).expect("hyp index");
        cost[ri][hi] > 0.0
    });
    (pairs, total.max(0.0))
}

/// Highest-overlap pair first, then the next available — the standard fallback
/// when the exact search is too wide. Only reachable above
/// [`EXACT_MAPPING_LIMIT`] speakers.
fn greedy_mapping(
    refs: &[String],
    hyps: &[String],
    cost: &[Vec<f64>],
) -> (Vec<(String, String)>, f64) {
    let mut candidates: Vec<(f64, usize, usize)> = Vec::new();
    for (i, row) in cost.iter().enumerate() {
        for (j, &value) in row.iter().enumerate() {
            if value > 0.0 {
                candidates.push((value, i, j));
            }
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut used_ref = vec![false; refs.len()];
    let mut used_hyp = vec![false; hyps.len()];
    let mut pairs = Vec::new();
    let mut total = 0.0;
    for (value, i, j) in candidates {
        if used_ref[i] || used_hyp[j] {
            continue;
        }
        used_ref[i] = true;
        used_hyp[j] = true;
        total += value;
        pairs.push((refs[i].clone(), hyps[j].clone()));
    }
    pairs.sort();
    (pairs, total)
}

// ---------------------------------------------------------------------------
// DER
// ---------------------------------------------------------------------------

/// The NIST-style split of diarization error, in SECONDS.
///
/// * `miss` — reference speech no hypothesis speaker covered.
/// * `false_alarm` — hypothesis speech no reference speaker was speaking under.
/// * `confusion` — speech both sides found, attributed to the wrong speaker
///   under the optimal mapping.
/// * `total` — total reference speaker time, counting overlapped speech once
///   PER SPEAKER (two people talking for one second is two seconds of reference
///   speaker time). This is md-eval's denominator, and it is why a DER can
///   exceed 1.0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DerReport {
    pub miss: f64,
    pub false_alarm: f64,
    pub confusion: f64,
    pub total: f64,
}

impl DerReport {
    /// `(miss + false_alarm + confusion) / total`. An empty reference scores
    /// `0.0` when the hypothesis is empty too, and `1.0` when it is not — a rate
    /// over nothing is otherwise a divide by zero dressed up as an accuracy
    /// number.
    pub fn rate(&self) -> f64 {
        if self.total <= 0.0 {
            return if self.false_alarm > 0.0 { 1.0 } else { 0.0 };
        }
        (self.miss + self.false_alarm + self.confusion) / self.total
    }
}

/// Diarization Error Rate over a shared timeline, with **no collar and no
/// overlap forgiveness**.
///
/// Both omissions are deliberate. A collar exists to forgive boundary jitter in
/// HUMAN annotation; this epic's ground truth is synthetic and exact to the
/// sample, so a collar here would only hide real boundary error. Overlap
/// forgiveness would hide precisely the failure fixture (f) exists to expose —
/// sherpa's pipeline drops overlapped frames before embedding (merged finding
/// #5), and a scorer that also dropped them would rate that as perfect.
pub fn der(reference: &[RttmTurn], hypothesis: &[RttmTurn]) -> DerReport {
    let mapping = optimal_speaker_mapping(reference, hypothesis);
    let refs = speakers_of(reference);
    let hyps = speakers_of(hypothesis);
    let ref_spans: Vec<Vec<(f64, f64)>> = refs.iter().map(|s| merged_spans(reference, s)).collect();
    let hyp_spans: Vec<Vec<(f64, f64)>> =
        hyps.iter().map(|s| merged_spans(hypothesis, s)).collect();
    // Reference speaker index → mapped hypothesis speaker index.
    let mapped: Vec<Option<usize>> = refs
        .iter()
        .map(|r| {
            mapping
                .hypothesis_for(r)
                .and_then(|h| hyps.iter().position(|x| x == h))
        })
        .collect();

    let mut points: Vec<f64> = Vec::new();
    for spans in ref_spans.iter().chain(hyp_spans.iter()) {
        for (start, end) in spans {
            points.push(*start);
            points.push(*end);
        }
    }
    points.sort_by(f64::total_cmp);
    points.dedup_by(|a, b| (*a - *b).abs() < 1e-12);

    let mut report = DerReport {
        miss: 0.0,
        false_alarm: 0.0,
        confusion: 0.0,
        total: 0.0,
    };
    // Every elementary interval lies wholly inside or wholly outside each span
    // (its endpoints ARE the span boundaries), so the midpoint decides it — and
    // decides it without an epsilon comparison against a boundary that dedup may
    // have moved by 1e-13.
    let active = |spans: &[(f64, f64)], lo: f64, hi: f64| {
        let mid = (lo + hi) / 2.0;
        spans.iter().any(|(s, e)| *s <= mid && mid < *e)
    };
    for window in points.windows(2) {
        let (lo, hi) = (window[0], window[1]);
        let duration = hi - lo;
        if duration <= 0.0 {
            continue;
        }
        let ref_on: Vec<usize> = (0..refs.len())
            .filter(|i| active(&ref_spans[*i], lo, hi))
            .collect();
        let hyp_on: Vec<usize> = (0..hyps.len())
            .filter(|j| active(&hyp_spans[*j], lo, hi))
            .collect();
        let correct = ref_on
            .iter()
            .filter(|i| mapped[**i].is_some_and(|j| hyp_on.contains(&j)))
            .count();
        let n_ref = ref_on.len();
        let n_hyp = hyp_on.len();
        report.total += n_ref as f64 * duration;
        report.miss += n_ref.saturating_sub(n_hyp) as f64 * duration;
        report.false_alarm += n_hyp.saturating_sub(n_ref) as f64 * duration;
        report.confusion += (n_ref.min(n_hyp) - correct) as f64 * duration;
    }
    report
}

// ---------------------------------------------------------------------------
// JER
// ---------------------------------------------------------------------------

/// Jaccard Error Rate — the per-speaker-averaged metric DER's denominator
/// hides.
///
/// DER weights every second equally, so one speaker who talks for 90 % of a
/// meeting dominates it: a hypothesis that gets the loud speaker perfect and
/// loses the quiet one entirely still scores a respectable DER. JER scores each
/// REFERENCE speaker separately (`1 - |ref ∩ hyp| / |ref ∪ hyp|` against their
/// mapped hypothesis speaker) and averages, so the lost speaker costs a full
/// `1.0` regardless of how little they said. `jer_exposes_a_speaker_der_hides`
/// is that difference as a test.
///
/// This is the DIHARD definition (`dscore`): averaged over reference speakers,
/// with an unmapped reference speaker scoring `1.0`.
pub fn jer(reference: &[RttmTurn], hypothesis: &[RttmTurn]) -> f64 {
    let refs = speakers_of(reference);
    if refs.is_empty() {
        return if speakers_of(hypothesis).is_empty() {
            0.0
        } else {
            1.0
        };
    }
    let mapping = optimal_speaker_mapping(reference, hypothesis);
    let mut sum = 0.0;
    for r in &refs {
        let ref_spans = merged_spans(reference, r);
        let Some(h) = mapping.hypothesis_for(r) else {
            sum += 1.0;
            continue;
        };
        let hyp_spans = merged_spans(hypothesis, h);
        let union = union_duration(&ref_spans, &hyp_spans);
        if union <= 0.0 {
            sum += 1.0;
            continue;
        }
        sum += 1.0 - overlap(&ref_spans, &hyp_spans) / union;
    }
    sum / refs.len() as f64
}

// ---------------------------------------------------------------------------
// Enrollment EER
// ---------------------------------------------------------------------------

/// The equal-error point of a genuine/impostor similarity distribution.
///
/// `threshold_at_eer` is the number YV129's auto-confirm / suggest / new-voice
/// bands are placed around — MEASURED here, never copied from a blog post that
/// measured a different pipeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EerReport {
    /// The equal error rate, `(FAR + FRR) / 2` at the crossing point.
    pub eer: f64,
    /// The similarity threshold at that point. "Accept if `score >= threshold`."
    pub threshold_at_eer: CosineSimilarity,
    /// False-accept rate at the reported threshold: impostors scoring at or
    /// above it.
    pub far_at_eer: f64,
    /// False-reject rate at the reported threshold: genuine pairs scoring below
    /// it.
    pub frr_at_eer: f64,
    pub genuine: usize,
    pub impostor: usize,
}

/// Sweep the equal-error point over a labeled genuine/impostor distribution.
///
/// Candidate thresholds are every observed score plus the midpoint between
/// consecutive distinct scores, so the crossing is found between two clusters
/// rather than being pinned to a sample that happens to sit in one. The
/// decision rule is `accept if score >= threshold`, which is the rule the
/// enrollment matcher will use, so the threshold this returns can be shipped
/// verbatim.
///
/// # Panics
/// If either distribution is empty. An "equal error rate" over no errors and no
/// trials is not a number, and returning `0.5` for it would let an empty eval
/// arm look like a measured coin flip.
pub fn enrollment_eer(
    genuine_scores: &[CosineSimilarity],
    impostor_scores: &[CosineSimilarity],
) -> EerReport {
    assert!(
        !genuine_scores.is_empty() && !impostor_scores.is_empty(),
        "an EER needs both a genuine and an impostor distribution: got {} genuine, {} impostor",
        genuine_scores.len(),
        impostor_scores.len()
    );
    let genuine: Vec<f64> = genuine_scores.iter().map(|s| s.get() as f64).collect();
    let impostor: Vec<f64> = impostor_scores.iter().map(|s| s.get() as f64).collect();

    let mut observed: Vec<f64> = genuine.iter().chain(impostor.iter()).copied().collect();
    observed.sort_by(f64::total_cmp);
    observed.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let mut candidates: Vec<f64> = Vec::with_capacity(observed.len() * 2 + 2);
    candidates.push(observed[0] - 1e-6);
    for pair in observed.windows(2) {
        candidates.push(pair[0]);
        candidates.push((pair[0] + pair[1]) / 2.0);
    }
    candidates.push(*observed.last().expect("non-empty"));
    candidates.push(observed.last().expect("non-empty") + 1e-6);

    let mut best = EerReport {
        eer: 1.0,
        threshold_at_eer: CosineSimilarity::new(0.0),
        far_at_eer: 1.0,
        frr_at_eer: 1.0,
        genuine: genuine.len(),
        impostor: impostor.len(),
    };
    let mut best_gap = f64::INFINITY;
    for threshold in candidates {
        let far =
            impostor.iter().filter(|s| **s >= threshold).count() as f64 / impostor.len() as f64;
        let frr = genuine.iter().filter(|s| **s < threshold).count() as f64 / genuine.len() as f64;
        let gap = (far - frr).abs();
        let eer = (far + frr) / 2.0;
        // Tie-break on the lower error rate, so an interval of thresholds that
        // all cross equally reports the best point in it rather than the first.
        if gap < best_gap - 1e-12 || ((gap - best_gap).abs() <= 1e-12 && eer < best.eer) {
            best_gap = gap;
            best = EerReport {
                eer,
                threshold_at_eer: CosineSimilarity::new(threshold as f32),
                far_at_eer: far,
                frr_at_eer: frr,
                genuine: genuine.len(),
                impostor: impostor.len(),
            };
        }
    }
    best
}
