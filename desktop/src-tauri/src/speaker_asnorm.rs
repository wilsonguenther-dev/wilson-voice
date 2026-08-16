//! YV131 — cross-device embedding drift: AS-norm against a shipped impostor
//! cohort, and within-meeting relative ranking instead of an absolute per-
//! utterance threshold.
//!
//! Merged audit finding #21 is the case Wilson asked about directly: the same
//! person, once on a laptop microphone and once on AirPods, will not reliably
//! match against a fixed cosine-similarity threshold, because the absolute
//! distribution of cosine scores moves with the recording condition. Multi-
//! centroid profiles (YV128) are half the answer. This module answers the
//! finding, whose prescription — in its own words — was: "AS-norm against a
//! small shipped impostor cohort to convert an absolute threshold into a
//! relative one; within a meeting, rank candidates rather than threshold
//! absolutely."
//!
//! **It does not deliver that prescription as written, and the next section
//! says exactly what it delivers instead.** The ranking half is here in full.
//! The "convert an absolute threshold into a relative one" half is here only in
//! the sense of *relative to the enrolled profile*, never *relative to the
//! recording condition*: the term that would have done the latter was swept
//! against the harness and lost, so it does not ship. Reading the quotation
//! above as a description of the code is the mistake three earlier revisions of
//! this file, its PR body and its changelog entry all made.
//!
//! # What the shipped form actually is — read this before the claims
//!
//! **The shipped normalization is enrollment-side only, and it is therefore
//! CONDITION-BLIND.** `as_norm_score(raw, enrollment)` reads the raw cosine and
//! the ENROLLED centroid's cohort statistics. Nothing about the live recording
//! enters it. For a fixed profile `μₑ` and `σₑ` are constants, so
//! [`Ranking::suggestion`] is algebraically
//!
//! ```text
//! accept  ⟺  cos ≥ μₑ + band · σₑ
//! ```
//!
//! — a **per-profile absolute cosine band**, computing the same number whatever
//! microphone the cluster was recorded on. Whoever reads this module next should
//! have that fact before they have any of its numbers, because an earlier
//! revision of this header, of the PR body and of the user-facing changelog all
//! described the *test-side* term that the design sweep deleted, and a
//! maintainer acting on that description would look for condition tracking that
//! is not here.
//!
//! # The two mechanisms
//!
//! **(1) AS-norm, enrollment side.** A profile's score is expressed in units of
//! how *that profile* scores against a small shipped cohort of voices that are
//! definitely nobody's enrolled speaker. What that removes is the per-PROFILE
//! offset — **hubness**: some enrolled centroids sit in a dense part of the
//! space and score high against everybody, so a single fixed cosine band is a
//! different strictness for each enrolled person. Dividing that offset out is
//! what lets ONE band mean the same thing across enrolled people. It is not what
//! lets a band follow a microphone; nothing here follows a microphone.
//!
//! The cross-condition benefit this item was written for is real and measured,
//! and this is the honest account of where it comes from: removing the
//! inter-speaker component of score variance shrinks the FRR *spread* between a
//! matched channel and a shifted one (21.5 pp for a cosine band tuned the same
//! way, 4.9 pp for this one), because the decision no longer inherits each
//! speaker's own offset on top of the channel's. The score does not adapt to the
//! test condition. It is a better-calibrated absolute band, per person.
//!
//! **(2) Within-meeting ranking.** When several enrolled profiles are plausible
//! for one cluster, they are ranked against each other and the best one is
//! offered — once. The right question in a six-person meeting is "which of
//! these people is this", not "does each of them independently clear a floor",
//! which asks the user six questions about one voice.
//!
//! # Which normalization, and how that was decided
//!
//! The literature form of AS-norm is SYMMETRIC — an enrollment-side term and a
//! test-side term, averaged (Matejka et al., Interspeech 2017) — and the epic
//! plan asks for the test side by name ("the impostor-score distribution for
//! that specific recording condition"). This module ships the **enrollment side
//! only**, and that is a measurement, not a shortcut.
//!
//! An adversarial review of the first draft found that the test-side term had
//! no observable consequence anywhere in the shipped code: within one cluster
//! every candidate is scored against the same test embedding, so the term is
//! identical across them — a monotone affine map of the raw score, which cannot
//! reorder anything — and deleting it left every test green. So all three forms
//! (enrollment-only, test-only, symmetric) were swept against YV120's harness on
//! the tuning split, across two cohort variants and every K, and the transcript
//! is committed in `docs/yap23-asnorm-measurement.json`. Enrollment-only won,
//! and it won on the arm this item exists for. Symmetric was 0.4 points of EER
//! behind; test-only was 2 points behind.
//!
//! Half a formula that no test can falsify does not ship because a paper uses
//! it. That is the whole point of the eval discipline this backlog runs on:
//! numbers are outputs of the harness, and so are designs.
//!
//! # The band, and why this module ships one
//!
//! An earlier draft of this item shipped **no** accept/reject band: it
//! normalized, it ranked, and it left admission in cosine units where YV129
//! measured it. That was the wrong call, and the reason is the finding itself.
//! Ranking only ever reorders candidates something else already admitted, so a
//! laptop-mic profile whose cross-device cosine falls under a fixed band is
//! still missed as `New` — which is exactly the failure finding #21 describes
//! and exactly the case Wilson asked about. Moving admission into per-profile
//! calibrated units is the deliverable, and a band nothing consumes is not a
//! band. (Note what that sentence does NOT say: the band is not relative to the
//! test condition. It is one band per enrolled profile, in that profile's own
//! calibrated units, and it is the same number for every microphone.)
//!
//! So there is one band, [`NormalizedBand`], in AS-norm units. It is **tuned**,
//! and this module says so rather than hiding behind "we ship no thresholds":
//! it is the equal-error crossing of the AS-norm score distribution on
//! LibriSpeech `dev-clean`, frozen there, and reported on `dev-other`. It
//! travels in the cohort manifest beside `top_k` and the distinctness gate —
//! all three tuned numbers carry the split, the rule and the operating point
//! that produced them, because a band whose provenance is a code comment is a
//! band nobody can re-derive. See `docs/yap23-asnorm-measurement.md`.
//!
//! [`NormalizedScore`] is still deliberately not a [`CosineSimilarity`], and
//! there is still no conversion between them: the band is compared to the score
//! in the unit the score is measured in, which is the entire point.
//!
//! # Where this is wired: nowhere yet, and that is a fact not a hedge
//!
//! `grep -rn speaker_asnorm desktop --include='*.rs'` outside `tests/` returns
//! exactly one line — `pub mod speaker_asnorm;` in `lib.rs`. The spec's
//! integration point is `speaker_profiles.rs::match_cluster` (YV129, "extended,
//! not forked"), and that function is not in this tree: YV126/128/129/130 are
//! unmerged and `main` is at YV125. So there is no enrollment, no `Suggested`
//! prompt reaching a user, and no end-to-end cross-device path — this module is
//! a measured scoring component waiting for its caller, and every number below
//! is measured on the harness rather than on a running app. Anything that reads
//! like a shipped user-visible behaviour anywhere else should be read against
//! this paragraph.
//!
//! # Measured, on the harness, not quoted
//!
//! Held out on LibriSpeech `dev-other` — speakers the design was never tuned
//! against — through a simulated headset-to-laptop channel, AS-norm reduces the
//! equal error rate, with a speaker-level bootstrap interval published beside
//! the point estimate because an EER delta without an interval is a number of
//! unknown size. It also **costs** a little in the matched-condition control,
//! where there is no condition shift to correct for; that rung is published too,
//! and the full ladder — including the far-field ones where the benefit shrinks
//! toward nothing — is in `docs/yap23-asnorm-measurement.md`.
//!
//! `tests/as_norm_cross_condition_measured.rs` does not trust any of that: the
//! measurement JSON commits the raw PRIMITIVES (speaker ids, per-side cohort
//! statistics, raw cosines) and the tests recompute every published number
//! through [`as_norm_score`] — the arithmetic that actually ships — and through
//! YV120's `eer_sweep`. Change the formula and the published numbers move and
//! the test goes red. That is the property the first draft lacked, and it is why
//! the transcript commits primitives rather than finished scores.

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

/// The accept/reject band, in the same units as [`NormalizedScore`].
///
/// A separate type from the score for the reason YV120 gives everywhere else: a
/// band and an observation are not interchangeable even when they share a unit,
/// and the compiler is a cheaper reviewer than a person. There is deliberately
/// no `From<CosineSimilarity>` — a cosine band cannot be converted into this
/// one, it has to be MEASURED in this unit, which is the work
/// `scripts/yv131-build-impostor-cohort.py` does on the tuning split.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct NormalizedBand(f32);

impl NormalizedBand {
    pub fn new(value: f32) -> Self {
        Self(value)
    }

    pub fn get(self) -> f32 {
        self.0
    }

    /// `accept if score >= band` — the same decision rule YV120's `eer_sweep`
    /// sweeps, so the band it reports can be shipped verbatim without anybody
    /// re-deriving which side of the comparison is which.
    pub fn admits(self, score: NormalizedScore) -> bool {
        score.get() >= self.0
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
/// and not a feeling: **159,744 bytes** — 78 entries at 512 `f32` each (40
/// LibriSpeech speakers under 2 conditions, minus the 2 rows the distinctness
/// gate dropped). That is 0.16 MB against a CAM++ model the user downloads at
/// 29.3 MB. `tests/as_norm_cohort_is_provenanced.rs` checks the payload against
/// the manifest rather than against this sentence, which is why this sentence
/// was allowed to go stale at 163,840 / 80 for one revision.
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
    /// Where the three tuned numbers came from. Only the admission band is read
    /// at runtime; the rest is the provenance
    /// `tests/as_norm_cohort_is_provenanced.rs` checks, and it lives beside the
    /// value rather than in a doc somebody can forget to update.
    tuning: CohortTuning,
    embedder: CohortEmbedder,
}

#[derive(Debug, Clone, Deserialize)]
struct CohortTuning {
    admission: CohortAdmission,
}

#[derive(Debug, Clone, Deserialize)]
struct CohortAdmission {
    normalized_band: f32,
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
    band: NormalizedBand,
    embedder_sha256: String,
}

impl ImpostorCohort {
    /// Decode the compiled-in cohort, verifying it against its manifest.
    ///
    /// Returns `Err` rather than panicking, because every caller already has a
    /// correct thing to do without a cohort: rank on raw cosine and say so.
    ///
    /// **Decode once per session and hold it**, rather than calling this per
    /// cluster. It parses the manifest and allocates the whole 159,744-byte
    /// payload into rows each time — cheap, but not free, and pointlessly
    /// repeated for a constant. The per-cluster cost that is genuinely
    /// unavoidable is [`ImpostorCohort::statistics`], which is also computed
    /// once per cluster and once per profile centroid, never per utterance.
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
        if !manifest.tuning.admission.normalized_band.is_finite() {
            return Err(CohortError::ManifestMismatch(
                "the admission band is not a finite number".into(),
            ));
        }
        Ok(Self {
            rows,
            dim: manifest.dim,
            top_k: manifest.top_k,
            band: NormalizedBand::new(manifest.tuning.admission.normalized_band),
            embedder_sha256: manifest.embedder.model_sha256.clone(),
        })
    }

    /// Build a cohort in memory. Tests use this; nothing in the app does, which
    /// is why it takes the same invariants the decoder enforces rather than
    /// trusting its caller.
    pub fn from_rows(
        rows: Vec<Vec<f32>>,
        top_k: usize,
        band: NormalizedBand,
        embedder_sha256: impl Into<String>,
    ) -> Result<Self, CohortError> {
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
        Ok(Self { rows, dim, top_k, band, embedder_sha256: embedder_sha256.into() })
    }

    /// The measured accept/reject band this cohort was tuned with.
    ///
    /// It belongs to the cohort, not to the code, for the same reason `top_k`
    /// does: it was measured against THESE rows, and a band that can drift from
    /// the cohort it was measured against is a silent accuracy regression.
    pub fn admission_band(&self) -> NormalizedBand {
        self.band
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
    /// Rebuild one side's statistics from numbers computed elsewhere.
    ///
    /// This exists for the measured arm. `docs/yap23-asnorm-measurement.json`
    /// commits the PRIMITIVES a trial was built from — each side's top-K mean
    /// and standard deviation, and the raw cosine — rather than the finished
    /// AS-norm score, so `tests/as_norm_cross_condition_measured.rs` can push
    /// them back through the SHIPPED [`as_norm_score`] and recompute every
    /// published number. Committing finished scores instead, which the first
    /// draft did, leaves the shipped arithmetic untested by the one arm that
    /// exists to justify it.
    pub fn new(mean: f32, std_dev: f32, k: usize) -> Self {
        Self { mean, std_dev, k }
    }

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

/// Adaptive score normalization, enrollment side.
///
/// `(s - mu_e) / sigma_e`, where `mu_e` and `sigma_e` are the enrolled
/// centroid's top-K impostor-cohort statistics: *how many strangers above a
/// stranger is this profile's score?*
///
/// What it corrects is **hubness**, and nothing else. Some enrolled centroids
/// sit in a dense part of the embedding space and score high against everybody;
/// a raw cosine cannot tell that apart from being the right person. Dividing it
/// out does two things at once: it lets the right profile win a ranking it was
/// losing on its rival's offset alone (candidates in one cluster share the
/// cluster, but each carries its OWN `μₑ`/`σₑ`, which is why normalization can
/// reorder them), and it removes the per-profile offset that makes a FIXED
/// cosine band a different strictness for each enrolled person. The second is
/// what makes [`NormalizedBand`] possible.
///
/// **It does not correct for the recording condition, and cannot.** `raw` is the
/// only argument that carries anything about the live audio, and it is not used
/// to compute `μₑ` or `σₑ` — those are properties of the enrolled centroid
/// against the fixed shipped cohort, identical for every microphone. This is a
/// per-profile recalibration of an absolute cosine band, not a condition-adaptive
/// score. The cross-condition numbers in `docs/yap23-asnorm-measurement.md` are
/// what that recalibration buys; they are not evidence of condition tracking.
///
/// **The test-side term of symmetric AS-norm is deliberately absent**, and the
/// module header explains why at length: it was swept against the harness, on
/// the tuning split, in both directions and at every K, and it lost. Adding it
/// back is a design change that has to beat `docs/yap23-asnorm-measurement.json`
/// before it is a design change worth making.
pub fn as_norm_score(raw: CosineSimilarity, enrollment: &CohortStatistics) -> NormalizedScore {
    NormalizedScore::new(enrollment.z(raw.get()))
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

/// What to do with one cluster, once its candidates are ranked and the winner
/// has been asked whether it clears the band.
///
/// This is the type that gives this module an admission opinion at all, rather
/// than leaving it a reordering of whatever some other gate already admitted.
/// The spec's third acceptance criterion is the case where a profile enrolled on
/// a laptop microphone is **missed as `New`** when the same person turns up on
/// AirPods; what moves that case is a band calibrated per profile instead of one
/// fixed cosine number shared by everybody — not a score that adapts to the
/// microphone, which is not what ships. See [`Ranking::suggestion`] for the
/// algebra, and note that acceptance 3 is a MANUAL check against a real
/// recording that has not been run: the automated arm is four simulated
/// channels.
#[derive(Debug, Clone, PartialEq)]
pub enum Suggestion<'a> {
    /// The best candidate cleared the band. Offer it — once, for the whole
    /// cluster.
    Suggested(&'a RankedCandidate),
    /// Nobody cleared it. This is a new voice, and saying so is a decision, not
    /// a failure to make one.
    NewVoice,
    /// The ranking degraded to raw cosine (no cohort, wrong embedder, wrong
    /// width, no spread), so there is no normalized score to compare and this
    /// module has no admission opinion. The caller falls back to the absolute
    /// cosine band it used before this item existed. Carried as its own variant
    /// rather than folded into `NewVoice`, because "this is a stranger" and "I
    /// could not tell" are different answers and a user-facing prompt should not
    /// confuse them.
    NoNormalizedOpinion { because: String },
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
    ///
    /// `None` when there is no comparable pair to measure: fewer than two
    /// candidates, or a runner-up that could not be normalized. That last case
    /// is the one worth stating — the obvious implementation subtracts negative
    /// infinity and reports a margin of `+inf`, which reads as total confidence
    /// about a candidate nothing is known about. An unmeasurable runner-up
    /// means the gap is unmeasurable, and saying so is the honest answer; the
    /// winner is still ranked first either way.
    pub fn margin(&self) -> Option<f32> {
        let (a, b) = (self.ordered.first()?, self.ordered.get(1)?);
        match self.basis {
            RankingBasis::AsNorm => Some(a.normalized?.get() - b.normalized?.get()),
            RankingBasis::RawCosine => Some(a.raw.get() - b.raw.get()),
        }
    }

    /// Admit the winner, or call the cluster a new voice.
    ///
    /// The accept/reject decision this item exists to move out of raw cosine
    /// units into per-profile calibrated ones.
    ///
    /// **What this is, stated so nobody has to derive it.** The score being
    /// compared is `(cos − μₑ)/σₑ` with `μₑ`/`σₑ` fixed by the enrolled centroid
    /// (see [`as_norm_score`]), so this test is exactly
    /// `cos ≥ μₑ + band · σₑ`: an absolute cosine band, chosen per profile. It
    /// is **condition-blind** — the cohort strangers are scored against the
    /// ENROLLMENT centroid, and their scores do not move when the cluster's
    /// microphone does. An earlier version of this comment claimed the band was
    /// expressed relative to strangers recorded on the cluster's own device;
    /// that described the test-side term the design sweep deleted, and it was
    /// wrong about the code beneath it.
    ///
    /// **What it buys, measured rather than argued.** Because the per-profile
    /// offset is divided out, one band is the same strictness for every enrolled
    /// person, and the FRR gap between a matched channel and a shifted one
    /// narrows (21.5 pp → 4.9 pp) — inter-speaker variance leaves the decision,
    /// the channel shift itself does not.
    /// `tests/as_norm_admits_across_conditions.rs` measures how often this
    /// rescues a match a fixed cosine band drops, on the held-out split, at a
    /// published false-accept rate.
    ///
    /// Only the WINNER is tested against the band. A cluster is one voice, so it
    /// gets one question — testing every candidate independently is precisely
    /// the "six questions about one voice" behaviour this item's second
    /// mechanism replaced.
    pub fn suggestion(&self, band: NormalizedBand) -> Suggestion<'_> {
        if self.basis == RankingBasis::RawCosine {
            return Suggestion::NoNormalizedOpinion {
                because: self
                    .degraded_because
                    .clone()
                    .unwrap_or_else(|| "ranking is not in normalized units".into()),
            };
        }
        match self.ordered.first() {
            // `rank_within_meeting` degrades to `RawCosine` when NOTHING could
            // be normalized, and sorts normalized candidates ahead of
            // un-normalized ones, so an `AsNorm` winner always carries a score.
            // The `None` arm is defensive rather than reachable: an invariant a
            // reader has to reconstruct from two other functions is not one to
            // answer a user-facing question on, and "I could not tell" is the
            // only honest answer if it is ever violated.
            Some(best) => match best.normalized {
                Some(score) if band.admits(score) => Suggestion::Suggested(best),
                Some(_) => Suggestion::NewVoice,
                None => Suggestion::NoNormalizedOpinion {
                    because: "no candidate could be normalized against the cohort".into(),
                },
            },
            None => Suggestion::NewVoice,
        }
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
/// computed the way it would have been before this item existed — and
/// [`Ranking::suggestion`] then declines to make an admission decision rather
/// than making one in the wrong unit.
///
/// **It does not take the cluster embedding**, and that is the whole reason the
/// resulting decision is condition-blind. It used to, for the test-side
/// normalization term the design sweep removed, and carrying a parameter the
/// score no longer reads would be an invitation to believe it still does. The
/// raw cosine each candidate carries is the only thing about the cluster this
/// function needs; computing it — and checking the live embedding against
/// [`ImpostorCohort::require_embedder`] — belongs to the caller that has the
/// embedding in the first place.
///
/// What normalization changes here is the ORDER, and it can change it because
/// each candidate brings its own centroid and therefore its own `μₑ`/`σₑ`. The
/// shared raw cosine cancels out of a comparison only if the two candidates
/// happen to have identical cohort statistics, which enrolled people do not.
pub fn rank_within_meeting(candidates: &[Candidate], cohort: Option<&ImpostorCohort>) -> Ranking {
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

    let mut ordered = Vec::with_capacity(candidates.len());
    let mut first_error: Option<String> = None;
    let mut normalized_count = 0usize;
    for c in candidates {
        let stats = match c.centroid.as_deref() {
            None => None,
            Some(centroid) => match cohort.statistics(centroid) {
                // A cohort with no spread against this centroid normalizes it to
                // a constant, and a score that is the same number whatever the
                // trial was is not a score. Degrading is what makes that failure
                // loud: it is how this module's own tests caught a cohort
                // orthogonal to the space its candidates lived in, which had been
                // producing a confident, meaningless order. `CohortStatistics::z`
                // returns 0.0 there, which is the right value and the wrong
                // silence.
                Ok(s) if s.std_dev() < DEGENERATE_SPREAD => {
                    return raw_only(Some(format!(
                        "impostor cohort has no spread against profile `{}` (sigma {:.2e})",
                        c.profile_id,
                        s.std_dev()
                    )));
                }
                Ok(s) => Some(s),
                Err(e) => {
                    first_error.get_or_insert(e.to_string());
                    None
                }
            },
        };
        if stats.is_some() {
            normalized_count += 1;
        }
        ordered.push(RankedCandidate {
            profile_id: c.profile_id.clone(),
            raw: c.raw,
            normalized: stats.map(|s| as_norm_score(c.raw, &s)),
        });
    }
    // Not one candidate could be normalized — a cohort of the wrong width, or
    // no retained centroids at all. Calling that an AS-norm ranking would be
    // labelling raw cosine as something it is not, and `Ranking::suggestion`
    // would then admit or reject on a band nothing was measured against.
    if normalized_count == 0 {
        return raw_only(Some(first_error.unwrap_or_else(|| {
            "no candidate carried a centroid to normalize against the cohort".into()
        })));
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
        // 512 is the pinned model's own answer, not a downstream claim: the file
        // catalog.json pins by sha256 `c46fad10…` declares ONNX metadata
        // `output_dim = 512` and a graph output `embs [B, 512]`. Reproduce with
        // `onnx.load(...)` over the digest-verified download. (This assertion
        // used to cite "YV122 measured 512"; YV122 is unmerged and cannot have.)
        assert_eq!(cohort.dim(), 512, "the pinned CAM++ ONNX declares output_dim = 512");
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
    fn the_shipped_band_is_finite_and_comes_from_the_manifest() {
        let cohort = ImpostorCohort::shipped().unwrap();
        let manifest: serde_json::Value = serde_json::from_str(COHORT_JSON).unwrap();
        assert!(cohort.admission_band().get().is_finite());
        assert_eq!(
            cohort.admission_band().get(),
            manifest["tuning"]["admission"]["normalized_band"].as_f64().unwrap() as f32,
            "the band the app admits with must be the one the manifest recorded \
             the tuning transcript for"
        );
    }

    #[test]
    fn a_degraded_ranking_has_no_admission_opinion() {
        // The distinction the enum exists for: with no cohort there is no
        // normalized score, so the answer is "ask the caller's absolute band",
        // never "this is a stranger".
        let ranking = rank_within_meeting(
            &[Candidate {
                profile_id: "a".into(),
                raw: CosineSimilarity::new(0.9),
                centroid: Some(vec![1.0, 0.0, 0.0]),
            }],
            None,
        );
        assert!(matches!(
            ranking.suggestion(NormalizedBand::new(0.0)),
            Suggestion::NoNormalizedOpinion { .. }
        ));
    }

    #[test]
    fn zero_spread_contributes_nothing_rather_than_infinity() {
        let stats = CohortStatistics { mean: 0.5, std_dev: 0.0, k: 4 };
        assert_eq!(stats.z(0.9), 0.0);
        let s = as_norm_score(CosineSimilarity::new(0.9), &stats);
        assert_eq!(s.get(), 0.0);
        assert!(s.get().is_finite());
    }
}
