//! YV131 — cross-device embedding drift: AS-norm against a shipped impostor
//! cohort, and within-meeting relative ranking instead of an absolute per-
//! utterance threshold.
//!
//! Merged audit finding #21 is the case Wilson asked about directly: the same
//! person, once on a laptop microphone and once on AirPods, will not reliably
//! match against a fixed cosine-similarity threshold, because the absolute
//! distribution of cosine scores moves with the recording condition. Multi-
//! centroid profiles (YV128) are half the answer. This module is the other
//! half, and it is the finding's own prescription, in its own words: "AS-norm
//! against a small shipped impostor cohort to convert an absolute threshold
//! into a relative one; within a meeting, rank candidates rather than threshold
//! absolutely."
//!
//! # The two mechanisms
//!
//! **(1) AS-norm.** An incoming embedding is scored against a small shipped
//! cohort of voices that are definitely nobody's enrolled profile, and the
//! target score is expressed in units of that impostor distribution:
//! *how far above a stranger is this, measured in strangers?* A shift in the
//! recording condition moves the target score and the cohort scores together,
//! so the ratio survives what the raw number does not.
//!
//! **(2) Within-meeting ranking.** When several enrolled profiles are plausible
//! for one cluster, they are ranked against each other and the best one is
//! offered — once. The right question in a six-person meeting is "which of
//! these people is this", not "does each of them independently clear 0.70",
//! which asks the user six questions about one voice.
//!
//! # What this module does NOT contain
//!
//! **No threshold.** Not one. [`NormalizedScore`] is deliberately not a
//! [`CosineSimilarity`] and cannot be compared to one — the accept/reject bands
//! stay where YV129 put them, measured, in cosine units, and this module only
//! decides ORDER. `tests/as_norm_ships_no_tuned_threshold.rs` enforces that by
//! scanning this file. The one tuned number here, `top_k` in the cohort
//! manifest, is how many cohort scores enter a mean and a standard deviation;
//! it decides nothing on its own, and it was chosen on one LibriSpeech split
//! and reported on another (docs/yap23-asnorm-measurement.md).
//!
//! # Measured, on the harness, not quoted
//!
//! Held out on LibriSpeech `dev-other`, 33 speakers the design was never tuned
//! against, through a simulated headset-to-laptop channel: equal error rate
//! **15.15% raw → 12.12% AS-norm**, a 20% relative reduction, and AS-norm is
//! ahead in all ten split-by-channel cells measured. The full ladder, including
//! the far-field conditions where the benefit shrinks toward nothing, is in
//! `docs/yap23-asnorm-measurement.md`; the per-trial scores are committed, and
//! `tests/as_norm_cross_condition_measured.rs` recomputes both numbers through
//! YV120's `enrollment_eer` rather than trusting the line you just read.

use crate::diarize_metrics::{cosine_similarity, eer_sweep, CosineSimilarity, EerSweep};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// The unit
// ---------------------------------------------------------------------------

/// An AS-norm-adjusted score: how far above the impostor cohort this trial sits,
/// in standard deviations of the cohort's own score distribution.
///
/// This is a SEPARATE type from [`CosineSimilarity`] on purpose, and the reason
/// is the same one YV120 gave for splitting distance from similarity. A cosine
/// similarity lives in `[-1, 1]` and an enrollment band is a number inside it. A
/// normalized score is a z-score: unbounded, routinely negative, and `0.6` means
/// "a bit above average for a stranger", not "a fairly good match". There is no
/// conversion between the two types, because there is no conversion between the
/// two quantities — passing one where the other belongs is a compile error here
/// rather than a silent accuracy regression six months from now.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct NormalizedScore(f32);

impl NormalizedScore {
    pub fn new(value: f32) -> Self {
        Self(value)
    }

    /// The raw number. One greppable escape from the type, same as YV120's.
    pub fn get(self) -> f32 {
        self.0
    }
}

// ---------------------------------------------------------------------------
// The shipped cohort
// ---------------------------------------------------------------------------

/// The cohort payload: `count * dim` little-endian `f32`, row-major, every row
/// already L2-normalized. Compiled in, the same posture as `catalog.json` and
/// the sha256 trust anchors — a cohort that had to be downloaded would be a
/// cohort the app could be missing, and "AS-norm, when the network worked" is
/// not a mechanism.
///
/// The actual number, since "small enough to embed in the binary" is a number
/// and not a feeling: **163,840 bytes** — 80 entries (40 LibriSpeech speakers
/// under 2 conditions) at 512 `f32` each. That is 0.16 MB against a CAM++ model
/// the user downloads at 29.3 MB.
const COHORT_BIN: &[u8] = include_bytes!("../assets/yv131-impostor-cohort.bin");

/// The manifest describing [`COHORT_BIN`] — digest, dim, count, provenance.
/// Parsed rather than hard-coded so the two cannot drift silently;
/// `tests/as_norm_cohort_is_provenanced.rs` re-hashes the payload against it.
const COHORT_JSON: &str = include_str!("../assets/yv131-impostor-cohort.json");

/// The compiled-in cohort payload, exposed so that
/// `tests/as_norm_cohort_is_provenanced.rs` can re-hash the ACTUAL bytes rather
/// than assert against a copy of the manifest's own claim about them.
pub fn shipped_payload() -> &'static [u8] {
    COHORT_BIN
}

/// The compiled-in manifest text, same reason.
pub fn shipped_manifest_json() -> &'static str {
    COHORT_JSON
}

#[derive(Debug, Clone, Deserialize)]
struct CohortManifest {
    /// sha256 of the payload. The decoder does not verify it — hashing 160 KB on
    /// every app start to check a constant against another constant compiled
    /// from the same commit would be theatre. It is verified once, in CI, by
    /// `tests/as_norm_cohort_is_provenanced.rs`, which is the moment the two
    /// could actually disagree: when someone regenerates one and commits the
    /// other.
    #[allow(dead_code)]
    sha256: String,
    bytes: usize,
    count: usize,
    dim: usize,
    top_k: usize,
    embedder: CohortEmbedder,
}

#[derive(Debug, Clone, Deserialize)]
struct CohortEmbedder {
    model_sha256: String,
}

/// Why a cohort could not be used. Every one of these degrades to raw-cosine
/// ranking rather than failing a meeting: the user's transcript is never held
/// hostage to a scoring refinement (the governing principle §6 this epic
/// applies to every degrade path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CohortError {
    /// The manifest does not describe the payload sitting next to it.
    ManifestMismatch(String),
    /// The embedder that produced the live embedding is not the one that
    /// produced the cohort. Two embedding spaces are not comparable, and
    /// scoring across them produces confident nonsense rather than an error.
    WrongEmbedder { cohort: String, live: String },
    /// The live embedding is not the cohort's width.
    DimMismatch { cohort: usize, live: usize },
    /// A zero-norm embedding carries no direction to score.
    Degenerate,
}

impl std::fmt::Display for CohortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ManifestMismatch(why) => write!(f, "impostor cohort manifest mismatch: {why}"),
            Self::WrongEmbedder { cohort, live } => write!(
                f,
                "impostor cohort was built with embedder {cohort} but the live embedding came from {live}"
            ),
            Self::DimMismatch { cohort, live } => {
                write!(f, "impostor cohort is {cohort}-dim, live embedding is {live}-dim")
            }
            Self::Degenerate => write!(f, "zero-norm embedding has no direction to score"),
        }
    }
}

impl std::error::Error for CohortError {}

/// A small, fixed set of voices that are definitely not any enrolled speaker.
#[derive(Debug, Clone)]
pub struct ImpostorCohort {
    rows: Vec<Vec<f32>>,
    dim: usize,
    top_k: usize,
    embedder_sha256: String,
}

impl ImpostorCohort {
    /// Decode the compiled-in cohort, verifying it against its manifest.
    ///
    /// Returns `Err` rather than panicking, because every caller already has a
    /// correct thing to do without a cohort: rank on raw cosine and say so.
    pub fn shipped() -> Result<Self, CohortError> {
        let manifest: CohortManifest = serde_json::from_str(COHORT_JSON)
            .map_err(|e| CohortError::ManifestMismatch(format!("unparseable manifest: {e}")))?;
        Self::decode(COHORT_BIN, &manifest)
    }

    fn decode(payload: &[u8], manifest: &CohortManifest) -> Result<Self, CohortError> {
        if payload.len() != manifest.bytes {
            return Err(CohortError::ManifestMismatch(format!(
                "manifest says {} bytes, payload is {}",
                manifest.bytes,
                payload.len()
            )));
        }
        if manifest.count == 0 || manifest.dim == 0 {
            return Err(CohortError::ManifestMismatch(
                "a cohort of no voices, or of zero-width voices, is not a cohort".into(),
            ));
        }
        if manifest.count * manifest.dim * 4 != payload.len() {
            return Err(CohortError::ManifestMismatch(format!(
                "{} entries x {} dims x 4 bytes != {} bytes",
                manifest.count,
                manifest.dim,
                payload.len()
            )));
        }
        if manifest.top_k == 0 || manifest.top_k > manifest.count {
            return Err(CohortError::ManifestMismatch(format!(
                "top_k {} is not in 1..={}",
                manifest.top_k, manifest.count
            )));
        }
        let mut rows = Vec::with_capacity(manifest.count);
        for row in payload.chunks_exact(manifest.dim * 4) {
            rows.push(
                row.chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect::<Vec<f32>>(),
            );
        }
        Ok(Self {
            rows,
            dim: manifest.dim,
            top_k: manifest.top_k,
            embedder_sha256: manifest.embedder.model_sha256.clone(),
        })
    }

    /// Build a cohort in memory. Tests use this; nothing in the app does, which
    /// is why it takes the same invariants the decoder enforces rather than
    /// trusting its caller.
    pub fn from_rows(rows: Vec<Vec<f32>>, top_k: usize, embedder_sha256: impl Into<String>) -> Result<Self, CohortError> {
        let dim = rows.first().map(|r| r.len()).unwrap_or(0);
        if rows.is_empty() || dim == 0 {
            return Err(CohortError::ManifestMismatch(
                "a cohort of no voices, or of zero-width voices, is not a cohort".into(),
            ));
        }
        if let Some(bad) = rows.iter().find(|r| r.len() != dim) {
            return Err(CohortError::DimMismatch { cohort: dim, live: bad.len() });
        }
        if top_k == 0 || top_k > rows.len() {
            return Err(CohortError::ManifestMismatch(format!(
                "top_k {} is not in 1..={}",
                top_k,
                rows.len()
            )));
        }
        Ok(Self { rows, dim, top_k, embedder_sha256: embedder_sha256.into() })
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    /// How many of the nearest cohort scores enter the normalization. This is
    /// the "adaptive" in adaptive score normalization: the statistics come from
    /// the strangers who actually sound like this trial, not from the whole
    /// cohort, most of which is irrelevant to any given voice.
    pub fn top_k(&self) -> usize {
        self.top_k
    }

    pub fn embedder_sha256(&self) -> &str {
        &self.embedder_sha256
    }

    /// Refuse a cohort built by a different embedder.
    ///
    /// Two speaker-embedding models put voices in unrelated coordinate systems.
    /// Cosine similarity between them is a well-formed float and a meaningless
    /// one, which is worse than an error — this turns it into an error.
    pub fn require_embedder(&self, live_model_sha256: &str) -> Result<(), CohortError> {
        if self.embedder_sha256 == live_model_sha256 {
            Ok(())
        } else {
            Err(CohortError::WrongEmbedder {
                cohort: self.embedder_sha256.clone(),
                live: live_model_sha256.to_string(),
            })
        }
    }

    /// Score one embedding against every cohort voice and summarize the top-K.
    ///
    /// Computed ONCE per profile centroid and once per cluster — not per
    /// utterance, not per turn. A six-person meeting with four enrolled
    /// profiles costs ten of these, total.
    pub fn statistics(&self, embedding: &[f32]) -> Result<CohortStatistics, CohortError> {
        if embedding.len() != self.dim {
            return Err(CohortError::DimMismatch { cohort: self.dim, live: embedding.len() });
        }
        if embedding.iter().all(|v| *v == 0.0) {
            return Err(CohortError::Degenerate);
        }
        let mut scores: Vec<f32> = self
            .rows
            .iter()
            .map(|row| cosine_similarity(embedding, row).get())
            .collect();
        // Descending, so the top-K are the most similar strangers.
        scores.sort_by(|a, b| b.total_cmp(a));
        let top = &scores[..self.top_k];
        let n = top.len() as f64;
        let mean = top.iter().map(|s| *s as f64).sum::<f64>() / n;
        let var = top.iter().map(|s| (*s as f64 - mean).powi(2)).sum::<f64>() / n;
        Ok(CohortStatistics { mean: mean as f32, std_dev: var.sqrt() as f32, k: top.len() })
    }
}

/// Below this, a cohort's top-K scores are one number with rounding on it, and
/// dividing by their spread turns floating-point noise into an enormous score.
/// Not a tuned threshold — it is the width of the arithmetic, and nothing about
/// speaker identity moves if it changes by an order of magnitude either way.
const DEGENERATE_SPREAD: f32 = 1e-6;

/// The mean and (population) standard deviation of one embedding's top-K cohort
/// scores — everything AS-norm needs from the cohort, in two floats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CohortStatistics {
    mean: f32,
    std_dev: f32,
    k: usize,
}

impl CohortStatistics {
    pub fn mean(&self) -> f32 {
        self.mean
    }

    pub fn std_dev(&self) -> f32 {
        self.std_dev
    }

    pub fn k(&self) -> usize {
        self.k
    }

    /// One side of the symmetric normalization.
    ///
    /// A cohort whose top-K scores are all but identical has no spread to
    /// measure against, and dividing by it would turn floating-point noise into
    /// an enormous score. That case contributes `0.0` — "indistinguishable from
    /// a stranger" — which is the honest reading of a degenerate cohort, and
    /// leaves the other side of the average to carry the trial.
    fn z(&self, raw: f32) -> f32 {
        if self.std_dev < DEGENERATE_SPREAD {
            0.0
        } else {
            (raw - self.mean) / self.std_dev
        }
    }
}

/// Symmetric adaptive score normalization.
///
/// `0.5 * [ (s - mu_e)/sigma_e + (s - mu_t)/sigma_t ]`, the standard AS-norm of
/// Matejka et al. (Interspeech 2017), with the enrollment side and the test side
/// each normalized by their own top-K cohort statistics.
///
/// Both sides are load-bearing and they close different holes. The
/// ENROLLMENT-side term removes hubness: some enrolled centroids sit in a dense
/// part of the space and score high against everybody, so they win rankings they
/// should lose. The TEST-side term is the cross-condition one this item exists
/// for: a recording condition that depresses every score depresses the cohort
/// scores too, and dividing it out is what turns "is this ≥ 0.70" into "is this
/// meaningfully closer than a stranger, in this exact condition".
pub fn as_norm_score(
    raw: CosineSimilarity,
    enrollment: &CohortStatistics,
    test: &CohortStatistics,
) -> NormalizedScore {
    let s = raw.get();
    NormalizedScore::new(0.5 * (enrollment.z(s) + test.z(s)))
}

/// The equal-error point of a labelled distribution of AS-NORM scores.
///
/// This exists so that nobody reaches for [`crate::diarize_metrics::enrollment_eer`]
/// with normalized scores. That function takes [`CosineSimilarity`], whose
/// constructor CLAMPS to `[-1, 1]` — and an AS-norm score is a z-score that
/// routinely lands at `-12` or `+3`. Passing them through it would fuse every
/// strong match into a tie at `1.0`, move the crossing point, and report a
/// wrong EER without a single error message. The unit discipline is not
/// decoration: this is the concrete bug it prevents, and it was a live bug in
/// this item's first draft.
pub fn normalized_score_eer(genuine: &[NormalizedScore], impostor: &[NormalizedScore]) -> EerSweep {
    let g: Vec<f64> = genuine.iter().map(|s| s.get() as f64).collect();
    let i: Vec<f64> = impostor.iter().map(|s| s.get() as f64).collect();
    eer_sweep(&g, &i)
}

// ---------------------------------------------------------------------------
// Within-meeting relative ranking
// ---------------------------------------------------------------------------

/// One enrolled profile in the running for one cluster.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub profile_id: String,
    /// The best raw cosine similarity across this profile's centroids —
    /// YV128's multi-centroid best-match, already reduced to one number.
    pub raw: CosineSimilarity,
    /// The centroid that produced `raw`, needed for the enrollment side of the
    /// normalization. `None` degrades this candidate to raw ordering rather
    /// than dropping it.
    pub centroid: Option<Vec<f32>>,
}

/// What the ordering was computed from — carried out of the ranking so a caller
/// (or a log line, or a support bundle) can tell a normalized decision from a
/// degraded one without guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankingBasis {
    /// AS-norm against the shipped cohort.
    AsNorm,
    /// Raw cosine similarity: no usable cohort. Still a correct ranking, just a
    /// less condition-robust one.
    RawCosine,
}

/// One ranked candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct RankedCandidate {
    pub profile_id: String,
    pub raw: CosineSimilarity,
    /// Present only when the ranking basis is [`RankingBasis::AsNorm`] and this
    /// candidate had a centroid to normalize.
    pub normalized: Option<NormalizedScore>,
}

/// The outcome of ranking every plausible profile against one cluster.
#[derive(Debug, Clone, PartialEq)]
pub struct Ranking {
    /// Best first. Ties keep the caller's input order, so a ranking is
    /// deterministic for a given input rather than dependent on sort internals.
    pub ordered: Vec<RankedCandidate>,
    pub basis: RankingBasis,
    /// Why the basis degraded, when it did. `None` on the AS-norm path.
    pub degraded_because: Option<String>,
}

impl Ranking {
    /// The one candidate to offer the user for this cluster.
    ///
    /// The whole point of ranking: a cluster produces ONE suggestion, not one
    /// per profile that happened to clear a threshold. Six enrolled people and
    /// one voice is one question.
    pub fn best(&self) -> Option<&RankedCandidate> {
        self.ordered.first()
    }

    /// How far clear of the runner-up the winner is, in whatever unit the
    /// ranking was computed in. A caller that wants to refuse a near-tie has
    /// the number to refuse it with; this module does not refuse anything,
    /// because that would be a threshold.
    pub fn margin(&self) -> Option<f32> {
        let (a, b) = (self.ordered.first()?, self.ordered.get(1)?);
        Some(match self.basis {
            RankingBasis::AsNorm => {
                a.normalized?.get() - b.normalized.map(|n| n.get()).unwrap_or(f32::NEG_INFINITY)
            }
            RankingBasis::RawCosine => a.raw.get() - b.raw.get(),
        })
    }
}

/// Rank enrolled profiles against one cluster, relative to each other.
///
/// This replaces "test each candidate independently against an absolute floor"
/// with "ask which of them this is". The two differ exactly when several
/// profiles are plausible — which is the normal case in a real meeting, and the
/// case where the absolute test asks the user one question per plausible person
/// about a single voice.
///
/// The cohort is optional and its absence is not an error: with no cohort, or
/// with one built by a different embedder, the ranking falls back to raw cosine
/// and records why. The user still gets a ranked suggestion; it is simply
/// computed the way it would have been before this item existed.
pub fn rank_within_meeting(
    cluster_embedding: &[f32],
    candidates: &[Candidate],
    cohort: Option<&ImpostorCohort>,
) -> Ranking {
    let raw_only = |why: Option<String>| -> Ranking {
        let mut ordered: Vec<RankedCandidate> = candidates
            .iter()
            .map(|c| RankedCandidate {
                profile_id: c.profile_id.clone(),
                raw: c.raw,
                normalized: None,
            })
            .collect();
        // `sort_by` is stable, so equal scores keep input order.
        ordered.sort_by(|a, b| b.raw.get().total_cmp(&a.raw.get()));
        Ranking { ordered, basis: RankingBasis::RawCosine, degraded_because: why }
    };

    let Some(cohort) = cohort else {
        return raw_only(Some("no impostor cohort supplied".into()));
    };
    let test_stats = match cohort.statistics(cluster_embedding) {
        Ok(s) => s,
        Err(e) => return raw_only(Some(e.to_string())),
    };
    // A cohort with no spread against this cluster normalizes every candidate to
    // the same number, and a ranking in which every score is identical is not a
    // ranking — it silently becomes "whatever order the sort happened to leave
    // them in". Degrading here is what makes that failure loud: it is how this
    // very test file caught a cohort that was orthogonal to the space its
    // candidates lived in, which had produced a confident, meaningless order.
    if test_stats.std_dev() < DEGENERATE_SPREAD {
        return raw_only(Some(format!(
            "impostor cohort has no spread against this cluster (sigma {:.2e})",
            test_stats.std_dev()
        )));
    }

    let mut ordered = Vec::with_capacity(candidates.len());
    for c in candidates {
        let normalized = c
            .centroid
            .as_deref()
            .and_then(|centroid| cohort.statistics(centroid).ok())
            .map(|enrollment_stats| as_norm_score(c.raw, &enrollment_stats, &test_stats));
        ordered.push(RankedCandidate {
            profile_id: c.profile_id.clone(),
            raw: c.raw,
            normalized,
        });
    }

    // A candidate with no normalized score sorts below every candidate that has
    // one, rather than being silently compared in the wrong unit. It keeps its
    // place in the list — the user may still pick it — but it cannot outrank a
    // measured candidate on a number that means something else.
    ordered.sort_by(|a, b| match (a.normalized, b.normalized) {
        (Some(x), Some(y)) => y.get().total_cmp(&x.get()),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.raw.get().total_cmp(&a.raw.get()),
    });

    Ranking { ordered, basis: RankingBasis::AsNorm, degraded_because: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_cohort_decodes_and_matches_its_manifest() {
        let cohort = ImpostorCohort::shipped().expect("shipped cohort decodes");
        assert_eq!(cohort.dim(), 512, "CAM++ measured 512 wide in YV122");
        assert!(cohort.len() >= 20, "a cohort of {} is not a cohort", cohort.len());
        assert!(cohort.top_k() <= cohort.len());
        for row in &cohort.rows {
            let norm: f32 = row.iter().map(|v| v * v).sum::<f32>().sqrt();
            assert!((norm - 1.0).abs() < 1e-3, "cohort row is not L2-normalized: {norm}");
        }
    }

    #[test]
    fn a_dim_mismatch_is_an_error_not_a_score() {
        let cohort = ImpostorCohort::shipped().unwrap();
        assert_eq!(
            cohort.statistics(&[1.0, 0.0, 0.0]),
            Err(CohortError::DimMismatch { cohort: 512, live: 3 })
        );
    }

    #[test]
    fn a_different_embedder_is_refused() {
        let cohort = ImpostorCohort::shipped().unwrap();
        assert!(cohort.require_embedder(cohort.embedder_sha256()).is_ok());
        assert!(matches!(
            cohort.require_embedder("0000000000000000000000000000000000000000000000000000000000000000"),
            Err(CohortError::WrongEmbedder { .. })
        ));
    }

    #[test]
    fn zero_spread_contributes_nothing_rather_than_infinity() {
        let stats = CohortStatistics { mean: 0.5, std_dev: 0.0, k: 4 };
        assert_eq!(stats.z(0.9), 0.0);
        let s = as_norm_score(CosineSimilarity::new(0.9), &stats, &stats);
        assert_eq!(s.get(), 0.0);
        assert!(s.get().is_finite());
    }
}
