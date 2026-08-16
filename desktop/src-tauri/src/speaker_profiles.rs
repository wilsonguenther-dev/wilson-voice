//! YV129 — the enrollment decision: is this cluster somebody we already know?
//!
//! Two things live here, and they are the same item because the second is the
//! answer to the first being wrong in the plan.
//!
//! ## 1. Not one threshold ships in this file
//!
//! The epic plan quoted `0.70` / `0.55` cosine similarity as the auto-confirm /
//! suggest bands. Those numbers come from a third-party blog post (OpenWhispr)
//! measuring a **different** pipeline: a different embedder, a different
//! resampler, a different segmentation front end. Merged finding #21's closing
//! instruction, and this backlog's standing rule, is that every threshold in
//! yap23 is an OUTPUT of YV120's harness measured against a fixture, never an
//! input copied from a vendor's blog.
//!
//! So [`EnrollmentBands`] is a **parameter with no default anywhere in this
//! crate**. There is no `const AUTO_CONFIRM`, no `const NEW_VOICE_FLOOR`, and
//! `EnrollmentBands` has no `Default`. The only way to obtain one is
//! [`bands_from_distribution`], which derives both edges from a measured
//! genuine/impostor distribution through [`crate::diarize_metrics::enrollment_eer`].
//! `tests/enrollment_threshold_from_harness.rs` asserts the absence, exactly as
//! YV126 asserts it for the clustering distance.
//!
//! **That absence is a TYPE property first and a scan second.** The first
//! version of this gate was a name-based grep, and a review probe walked
//! straight through it: rename the constants (`OPENWHISPR_HI` / `OPENWHISPR_LO`)
//! and put one function between the literals and the constructor, and both
//! threshold gates stayed green. So the literal constructor is now sealed —
//! `EnrollmentBands::from_measured_edges` is private to this module and
//! [`EnrollmentBands::for_test`] exists only under `cfg(test)` or the
//! `test-bands` feature (which the package's own dev-dependency turns on for
//! `cargo test` and nothing turns on for `cargo build --release`). That
//! LITERAL probe fails to COMPILE in a shipping build.
//!
//! **The second door was serde, and sealing the constructor did nothing to
//! it.** A derived `Deserialize` is a public producer that needs no
//! constructor: with it, a shipping
//! `serde_json::from_str::<EnrollmentBands>("{\"autoConfirm\":0.70,\"newVoiceFloor\":0.55}")`
//! compiled clean, put the exact vendor pair in the release binary, and skipped
//! [`BandError::Inverted`] on the way — a second review probe, and the reason
//! [`EnrollmentBands`] now derives `Serialize` only. The same derive on
//! [`crate::diarize_metrics::CosineSimilarity`] was bypassing its clamp for the
//! same reason; that one is hand-written now rather than dropped, because a
//! score genuinely travels inward.
//!
//! `tests/support/bands.rs` then asserts the same properties by TYPE rather
//! than by name: it walks `src/**/*.rs` for every construction of an
//! [`EnrollmentBands`] or a [`ChipFloor`] and requires each one to sit inside a
//! measured producer, it resolves one level of `const` indirection before
//! calling a constructor argument literal-free, it flags **any** non-endpoint
//! decimal in an `f32`/`f64` `const` in this file whatever it is called, and it
//! flags a band arriving as DATA — a `Deserialize` derive or hand-written impl
//! on a band type, or a deserializer call in any function that names one,
//! however the call is wrapped.
//!
//! ## The rule that places the two edges, and why it is not the extrema
//!
//! [`bands_from_distribution`] places `new_voice_floor` at the harness's
//! **equal-error operating point** and `auto_confirm` at the lowest operating
//! point above it that meets a caller-supplied **false-accept budget**
//! ([`TargetFar`]). The first shipped version used `min(genuine)` and
//! `max(impostor) + ε` instead, and a review finding killed it for the right
//! reason: both are extreme order statistics and both are monotone in sample
//! size, so more measurement bought a strictly worse operating point — the
//! suggest band widened with every added pair, one confusable impostor pushed
//! auto-confirm toward `1.0`, and [`MatchResult::Known`] would then never fire,
//! i.e. an enrolled speaker asked "who is this?" in every meeting forever. A
//! quantile does not do that: it converges as `N` grows, and
//! `enrollment_threshold_from_harness.rs::band_edges_do_not_diverge_as_the_sample_grows`
//! measures both rules side by side to show which one moves.
//!
//! **On this base the bands are still unmeasured, and that is enforced rather
//! than remembered.** OS-8 requires the anti-alias EER to be measured *before*
//! the enrollment thresholds are tuned, or those thresholds permanently encode
//! the aliasing. YV124 instrumented that measurement and could not take it —
//! `yap-diarize` answers `no_backend` until YV122 lands, so there are no CAM++
//! embeddings on any machine. `docs/yap23-eer-status.md` mirrors YV124's own
//! record of that, and `tests/enrollment_thresholds_refuse_an_unmeasured_eer.rs`
//! fails the build if a tuned band constant ever appears in this crate while
//! that mirror still reads `EER: UNMEASURED`.
//!
//! ## 2. One question per cluster — never one per utterance
//!
//! A six-person classroom recording contains hundreds of segments and dozens of
//! speaker turns. Asking "who is this?" per utterance, or per turn, would put
//! dozens of prompts in front of a user for a recording with six people in it,
//! which is the failure mode this item was written to prevent.
//!
//! [`match_cluster`] therefore takes a **cluster centroid**, not a segment and
//! not a turn, and [`match_meeting_clusters`] calls it exactly once per cluster.
//! By the time a cluster reaches here it has already survived YV126's ranking
//! and floor, so the population is a handful of named-worthy voices rather than
//! every raw cluster the diarizer emitted — and [`who_is_this_chips`] applies
//! the same floor a second time on the way to the screen, so the batching
//! property holds even if it is handed a raw list.
//!
//! ## The chip row is a completed-meeting affordance, and that is mechanical
//!
//! The plan's F2 flow is an inline chip row in the **meeting-detail** view:
//! `Speaker 2 → [Jeisil ▾] [Aidan] [+ new]`. Never a modal, never during a
//! recording. [`who_is_this_chips`] refuses — [`ChipRowError::MeetingStillLive`]
//! — when handed a meeting that has not ended, and
//! `tests/who_is_this_never_modal_never_live.rs` additionally asserts that no
//! symbol in this file appears as code anywhere in `meeting.rs`, the live
//! capture module. The affordance does not merely go unused mid-meeting; it is
//! not reachable from there.
//!
//! ## What this file deliberately does NOT own
//!
//! * **Persistence.** The `speaker_profiles` table, its migration and the
//!   centroid-update math are YV128's (PR #143, open at the time this landed).
//!   The types here are the in-memory view a stored row deserializes into —
//!   including that row's `embedding_model` column, which [`SpeakerProfile`]
//!   carries rather than drops, because a matcher that cannot say WHICH
//!   embedder made a stored vector cannot say what a cosine against it means.
//!   Nothing in this file touches SQL.
//! * **Clustering.** Producing a [`ClusterSummary`] from audio is YV126's
//!   `diarize::cluster_track` (PR #141, open). This file starts where that ends.
//! * **Correction.** Reassign / merge / split, and the `locked` rule, are
//!   YV130's. What this file honours is the half of that rule it can:
//!   [`who_is_this_chips`] never re-offers a cluster the user has already
//!   answered.

use serde::{Deserialize, Serialize};

use crate::diarize_metrics::{cosine_similarity, enrollment_eer, CosineDistance, CosineSimilarity};

// ---------------------------------------------------------------------------
// The stored shapes, as this file sees them
// ---------------------------------------------------------------------------

/// One speaker embedding — a CAM++ vector, 192-dimensional as the model
/// reports it.
///
/// A newtype rather than a bare `Vec<f32>` for the same reason the cosine units
/// are newtypes: the two things in this codebase shaped like a float vector are
/// an embedding and a window of audio samples, and confusing them is a silent
/// wrong answer rather than a crash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Embedding(Vec<f32>);

impl Embedding {
    pub fn new(values: Vec<f32>) -> Self {
        Self(values)
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }

    pub fn dim(&self) -> usize {
        self.0.len()
    }

    /// True when every component is zero — a silent utterance, or an embedder
    /// that answered without looking. [`cosine_similarity`] returns `0.0` for
    /// these rather than `NaN`, so they score as maximally uninformative rather
    /// than poisoning a sweep, but a caller that wants to drop them can ask.
    pub fn is_degenerate(&self) -> bool {
        self.0.iter().all(|v| *v == 0.0)
    }
}

/// One enrolled centroid, under one recording condition.
///
/// `condition_key` is YV128's ("laptop_mic_near", "airpods", …): the same
/// person through a different microphone at a different distance produces a
/// measurably different embedding, and one averaged centroid across both is
/// worse than two. This file only reads the key — it scores across **every**
/// centroid a profile holds and reports which one won, so a profile enrolled on
/// a laptop still matches on AirPods if either centroid is close enough.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Centroid {
    pub condition_key: String,
    pub vector: Embedding,
}

impl Centroid {
    pub fn new(condition_key: impl Into<String>, vector: Embedding) -> Self {
        Self {
            condition_key: condition_key.into(),
            vector,
        }
    }
}

/// WHICH embedder produced a vector — the pinned sha256 of the model file, not
/// its catalog id.
///
/// Cosine similarity between two embeddings is only a statement about voices
/// when both came from the **same** model. Two different 192-dimensional
/// embedders (CAM++ and any other wespeaker export) produce vectors of the same
/// width in unrelated spaces, and comparing them yields a number — often a high
/// one, ~1.0 for a voice the roster has never heard — with no meaning at all.
/// Above the auto-confirm edge that number writes a name on a stranger with
/// nobody in the loop, which is exactly the false-accept rate [`TargetFar`]
/// exists to bound.
///
/// **The digest, not the id.** `catalog.json`'s `id` does not change when the
/// bytes behind it are re-vendored (YV123 moved the weights to a Wilson-owned
/// mirror without renaming anything), so an id comparison would call two
/// different models the same model on precisely the occasion that matters. The
/// pinned `sha256` — the value the downloader already verifies and the binary
/// already carries as a trust anchor — changes exactly when the weights do.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EmbeddingModelId(String);

impl EmbeddingModelId {
    /// From a pinned digest string, as a stored profile row carries it.
    pub fn new(digest: impl Into<String>) -> Self {
        Self(digest.into())
    }

    /// The id of the embedding model a catalog entry names: the sha256 of the
    /// FILE THE SIDECAR LOADS — the extracted file's digest when the entry is
    /// an archive, the downloaded file's when it is a plain `.onnx`.
    ///
    /// `None` for a segmentation entry: segmentation produces no embeddings, so
    /// asking it for an embedding-model id is a bug at the call site rather
    /// than a value.
    ///
    /// This is the constructor a matching call site must use, applied to the
    /// entry that was handed to `diarize::DiarizePool::load_models` — the model
    /// identity has to come from the LOAD path, never from the profile being
    /// compared against, or the check is circular and always passes.
    pub fn of_loaded_embedder(model: &crate::models::DiarizeCatalogModel) -> Option<Self> {
        if model.role != crate::models::DiarizeModelRole::Embedding {
            return None;
        }
        Some(Self::new(match &model.archive {
            Some(archive) => archive.extracted_sha256.clone(),
            None => model.file.sha256.clone(),
        }))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EmbeddingModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One enrolled person.
///
/// `is_me` is a flag on a PROFILE, never a track index — merged finding #4's
/// correction, already spelled out in `meetings::speaker_label`'s doc comment.
/// Wilson is matched by his voice like anybody else; the flag only says which
/// enrolled row is his, so a surface can style it.
///
/// `embedding_model` is the [`EmbeddingModelId`] every centroid on this profile
/// was produced by — YV128's `speaker_profiles.embedding_model` column, in the
/// in-memory view. A profile carries one because a stored vector outlives the
/// model that made it: re-vendored weights, a catalog swap, or a second 192-dim
/// embedder all leave the row looking exactly as valid as it did yesterday.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerProfile {
    pub id: String,
    pub display_name: String,
    pub is_me: bool,
    pub embedding_model: EmbeddingModelId,
    pub centroids: Vec<Centroid>,
}

impl SpeakerProfile {
    /// The best score this profile can offer for `embedding`, across ALL of its
    /// centroids, and the `condition_key` of the one that won.
    ///
    /// `None` when this profile cannot answer the question at all:
    ///
    /// * it has no centroids — an enrolled name with no voice behind it yet,
    ///   which must never be *suggested* for anything; or
    /// * `probe_model` is not the model its centroids came from. A score across
    ///   two embedding spaces is not a weak score, it is not a score, and the
    ///   only safe thing to do with it is not to have it. The cluster is then
    ///   reported as [`MatchResult::New`] and a human is asked — a profile
    ///   enrolled under different weights is unusable evidence, not a stranger.
    ///
    /// # Panics
    /// If a centroid's dimension differs from `embedding`'s. Comparing a CAM++
    /// vector against something else is a bug at the call site, and a low score
    /// would hide it. Note the model guard above fires FIRST, so the panic is
    /// reserved for two vectors that claim the same model and disagree about
    /// its width — a real corruption, not an ordinary model change.
    pub fn best_match(
        &self,
        embedding: &Embedding,
        probe_model: &EmbeddingModelId,
    ) -> Option<(CosineSimilarity, &str)> {
        if &self.embedding_model != probe_model {
            return None;
        }
        self.centroids
            .iter()
            .map(|c| {
                (
                    cosine_similarity(embedding.as_slice(), c.vector.as_slice()),
                    c.condition_key.as_str(),
                )
            })
            .max_by(|a, b| a.0.get().total_cmp(&b.0.get()))
    }
}

// ---------------------------------------------------------------------------
// The bands — measured, never quoted
// ---------------------------------------------------------------------------

/// The three-band split of the cosine-similarity line: auto-confirm above
/// `auto_confirm`, suggest between the two edges, a new voice below
/// `new_voice_floor`.
///
/// **Unconstructible from literals in a shipping build.** The checked
/// constructor is private to this module, so [`bands_from_distribution`] is the
/// only path in the crate that yields one; [`EnrollmentBands::for_test`] exists
/// only under `cfg(test)` or the `test-bands` feature. There is deliberately no
/// `Default` and no `const` instance: see the module header.
///
/// **And no `Deserialize`.** `Serialize` alone, on purpose: serde's derive is a
/// second public producer that needs no constructor, so a sealed
/// `from_measured_edges` plus a derived `Deserialize` still let a shipping
/// `serde_json::from_str::<EnrollmentBands>(r#"{"autoConfirm":0.70,
/// "newVoiceFloor":0.55}"#)` put the vendor pair into a release binary — and it
/// bypassed [`BandError::Inverted`] too, producing the state this type claims
/// is impossible. A measured band only ever needs to travel OUTWARD (to the
/// UI, to a log, to `docs/`), which `Serialize` covers; nothing in this crate
/// deserializes one, and the chip row's TypeScript carries its own type
/// (`src/meetings/speakerChips.ts`). If an inward path is ever genuinely
/// wanted, write it by hand so it routes through `from_measured_edges` and
/// returns a serde error on `Inverted`, the way
/// [`crate::diarize_metrics::CosineSimilarity`] now routes through its clamp.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollmentBands {
    auto_confirm: CosineSimilarity,
    new_voice_floor: CosineSimilarity,
}

/// Why a proposed pair of band edges is not a usable split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandError {
    /// `auto_confirm` is at or below `new_voice_floor` — there is no "suggest"
    /// region between them, and the two decisions would contradict each other.
    Inverted,
}

impl std::fmt::Display for BandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BandError::Inverted => write!(
                f,
                "the auto-confirm edge must sit strictly above the new-voice floor"
            ),
        }
    }
}

impl std::error::Error for BandError {}

impl EnrollmentBands {
    /// The one checked constructor, **private to this module**.
    ///
    /// Private is the point: with no `pub` constructor in a shipping build, the
    /// only exported path to an `EnrollmentBands` is
    /// [`bands_from_distribution`], and "the bands are an output of the
    /// harness" stops being a convention a scanner has to police by name.
    ///
    /// `auto_confirm` must sit **strictly above** `new_voice_floor`. Equal edges
    /// are rejected rather than collapsed into a two-way decision: a build that
    /// silently stopped ever suggesting anything would look like a tuning
    /// result rather than the mistake it is.
    fn from_measured_edges(
        auto_confirm: CosineSimilarity,
        new_voice_floor: CosineSimilarity,
    ) -> Result<Self, BandError> {
        if auto_confirm.get() <= new_voice_floor.get() {
            return Err(BandError::Inverted);
        }
        Ok(Self {
            auto_confirm,
            new_voice_floor,
        })
    }

    /// Two literal edges, for tests and for tests only.
    ///
    /// Compiled under `cfg(test)` (this file's own unit tests) or the
    /// `test-bands` feature, which `Cargo.toml`'s self dev-dependency turns on
    /// for `cargo test` and which nothing turns on for `cargo build`. A
    /// shipping call site cannot name it — the review probe that defeated the
    /// name-based scan (`EnrollmentBands::new(CosineSimilarity::new(OPENWHISPR_HI), …)`
    /// behind a renamed constant and one function) is now a compile error in
    /// the release build, before any scanner is consulted.
    #[cfg(any(test, feature = "test-bands"))]
    pub fn for_test(
        auto_confirm: CosineSimilarity,
        new_voice_floor: CosineSimilarity,
    ) -> Result<Self, BandError> {
        Self::from_measured_edges(auto_confirm, new_voice_floor)
    }

    pub fn auto_confirm(&self) -> CosineSimilarity {
        self.auto_confirm
    }

    /// The floor below which a cluster is a NEW voice: nothing enrolled is
    /// close enough to be worth suggesting.
    pub fn new_voice_floor(&self) -> CosineSimilarity {
        self.new_voice_floor
    }
}

/// What a tuning run produced, together with what it could and could not
/// resolve.
///
/// The resolution fields exist because YV124 learned the lesson the hard way: a
/// finite genuine/impostor sample can only express error rates in steps of
/// `1/n`, and an EER of `0.0000` on eighteen genuine pairs is a floor, not a
/// measurement. A caller that prints a band without printing the resolution
/// underneath it is publishing a number it cannot support.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TunedBands {
    pub bands: EnrollmentBands,
    /// The measured equal error rate of the distribution the bands came from.
    pub eer: f64,
    /// The similarity at the equal-error point — which is exactly where
    /// [`EnrollmentBands::new_voice_floor`] was placed, per the item's spec
    /// ("compute the EER-optimal threshold and set the bands around it").
    pub eer_threshold: CosineSimilarity,
    /// The false-accept budget the caller asked the auto-confirm edge to meet.
    /// Policy, supplied by the call site — see [`TargetFar`].
    pub target_far: f64,
    /// The observed false-accept rate AT the auto-confirm edge, on this sample:
    /// impostor pairs that would have been given a name with no human in the
    /// loop. `<= target_far` by construction; read it together with
    /// `far_resolution`, the smallest non-zero value it could have taken.
    pub far_at_auto_confirm: f64,
    /// The observed false-REJECT rate at the auto-confirm edge: genuine pairs
    /// that clear the floor but not the auto-confirm edge, i.e. the ones that
    /// still cost the user a chip. This is the price of the FAR budget, and it
    /// is reported rather than left to be discovered on a screen.
    pub frr_at_auto_confirm: f64,
    /// The observed false-reject rate at the new-voice floor, on this sample:
    /// genuine pairs that would have been called a stranger. At the equal-error
    /// point this is `eer`-shaped by definition, not zero — read it together
    /// with `frr_resolution`.
    pub frr_at_new_voice_floor: f64,
    /// The smallest non-zero FAR this impostor sample can express: `1/impostor`.
    pub far_resolution: f64,
    /// The smallest non-zero FRR this genuine sample can express: `1/genuine`.
    pub frr_resolution: f64,
    pub genuine: usize,
    pub impostor: usize,
}

/// Why a distribution could not produce bands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TuningError {
    /// One or both sides of the distribution is empty. An EER over no trials is
    /// not a number.
    EmptyDistribution,
    /// The two distributions are indistinguishable — the equal-error point is a
    /// coin flip or worse. Bands derived from chance are noise wearing a
    /// measurement's clothes.
    ///
    /// Note this is a *definition* (0.5 is chance for a two-class decision), not
    /// a tuned acceptance bar. Deciding that, say, an EER of 0.12 is too poor to
    /// ship would itself be a number that has to come from a measurement, so
    /// this function reports the EER and refuses only at chance.
    Indistinguishable { eer: f64 },
    /// No operating point above the equal-error floor meets the requested
    /// false-accept budget without leaving the similarity line: the impostors
    /// that would have to be excluded sit at or above `1.0`, and nothing can be
    /// placed above them.
    NoRoomAboveImpostors { highest_impostor: f32 },
    /// The requested false-accept budget is finer than this sample can express.
    ///
    /// A FAR of 1% cannot be measured against 48 impostor pairs: the smallest
    /// non-zero rate they can show is `1/48 = 0.021`, so an edge "meeting" 1%
    /// would be meeting a number the corpus never had. Enlarge the corpus or
    /// loosen the budget; do not round. This is YV124's saturated-EER lesson
    /// applied to the other tail.
    TargetFarBelowResolution { target_far: f64, far_resolution: f64 },
    /// A FAR budget has to be a rate strictly between `0` and chance.
    TargetFarOutOfRange { target_far: f64 },
    /// The two measured edges landed on the same number, so there is no
    /// suggest region between them and the split is a single point. A distinct
    /// error rather than a silently collapsed two-way decision: a build that
    /// stopped ever asking would look like a tuning result.
    Degenerate { edge: f32 },
}

impl std::fmt::Display for TuningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TuningError::EmptyDistribution => write!(
                f,
                "a tuning run needs both a genuine and an impostor distribution"
            ),
            TuningError::Indistinguishable { eer } => write!(
                f,
                "genuine and impostor scores are indistinguishable (EER {eer:.4} at or above chance)"
            ),
            TuningError::NoRoomAboveImpostors { highest_impostor } => write!(
                f,
                "the highest impostor scored {highest_impostor:.4}; no operating point above the \
                 equal-error floor meets the false-accept budget without leaving the similarity line"
            ),
            TuningError::TargetFarBelowResolution {
                target_far,
                far_resolution,
            } => write!(
                f,
                "a false-accept budget of {target_far:.4} is finer than this sample can express: \
                 the smallest non-zero FAR {} impostor pairs can show is {far_resolution:.4}",
                (1.0 / far_resolution).round() as u64
            ),
            TuningError::TargetFarOutOfRange { target_far } => write!(
                f,
                "a false-accept budget must be a rate strictly between 0 and chance (0.5), not \
                 {target_far}"
            ),
            TuningError::Degenerate { edge } => write!(
                f,
                "both measured edges landed on {edge:.4}; there is no suggest region between them"
            ),
        }
    }
}

impl std::error::Error for TuningError {}

/// The false-accept budget the auto-confirm edge is placed to meet.
///
/// **This is policy, not a measurement, and it belongs to the CALLER.** How
/// often the app may put a name on a stranger with nobody in the loop is a
/// product decision; where that rate lands on the similarity line is a
/// measurement. Keeping the two apart is why this is a parameter rather than a
/// constant in this file — a `const TARGET_FAR` here would be a tuned number
/// with no measurement behind it, which is the shape of thing this whole item
/// exists to refuse, and `tests/support/bands.rs` scans for it by type.
///
/// A budget is only meaningful down to the sample's resolution: see
/// [`TuningError::TargetFarBelowResolution`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetFar(f64);

impl TargetFar {
    /// # Errors
    /// [`TuningError::TargetFarOutOfRange`] unless `0 < rate < 0.5`. A budget of
    /// zero is unreachable on a finite sample (it would claim something about
    /// impostors nobody measured) and a budget at chance is not a budget.
    pub fn new(rate: f64) -> Result<Self, TuningError> {
        if !rate.is_finite() || rate <= 0.0 || rate >= 0.5 {
            return Err(TuningError::TargetFarOutOfRange { target_far: rate });
        }
        Ok(Self(rate))
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

/// Every operating point the sample can distinguish, ascending.
///
/// The observed scores plus the midpoints between consecutive distinct ones,
/// plus one point above the highest — the same candidate construction
/// [`enrollment_eer`] sweeps, so the floor and the auto-confirm edge are chosen
/// from the same grid and the ROC they are read off is one curve, not two.
fn operating_points(genuine: &[CosineSimilarity], impostor: &[CosineSimilarity]) -> Vec<f64> {
    let mut observed: Vec<f64> = genuine
        .iter()
        .chain(impostor.iter())
        .map(|s| s.get() as f64)
        .collect();
    observed.sort_by(f64::total_cmp);
    observed.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let mut out = Vec::with_capacity(observed.len() * 2 + 1);
    for pair in observed.windows(2) {
        out.push(pair[0]);
        out.push((pair[0] + pair[1]) / 2.0);
    }
    let highest = *observed.last().expect("a non-empty distribution");
    out.push(highest);
    out.push(highest + 1e-6);
    out
}

/// The observed false-accept rate of `edge`: impostors the decision rule
/// `accept if score >= edge` would let through.
fn far_at(impostor: &[CosineSimilarity], edge: CosineSimilarity) -> f64 {
    impostor.iter().filter(|s| s.get() >= edge.get()).count() as f64 / impostor.len() as f64
}

/// The observed false-reject rate of `edge`: genuine pairs it would refuse.
fn frr_at(genuine: &[CosineSimilarity], edge: CosineSimilarity) -> f64 {
    genuine.iter().filter(|s| s.get() < edge.get()).count() as f64 / genuine.len() as f64
}

/// Derive both band edges from a MEASURED genuine/impostor distribution.
///
/// This is the only producer of an [`EnrollmentBands`] anywhere in a shipping
/// build, and it is what the item means by "thresholds are an OUTPUT of the
/// harness".
///
/// ## The rule, and why it is not the extrema
///
/// * **`new_voice_floor` = the equal-error operating point** that
///   [`enrollment_eer`] measured — the item's spec, verbatim: *"compute the
///   EER-optimal threshold and set the auto-confirm/suggest/new bands around
///   it."* Below it, the sample says a match is more likely wrong than right.
/// * **`auto_confirm` = the LOWEST operating point strictly above the floor
///   whose observed FAR is within `target_far`.** Lowest, because anything
///   higher rejects genuine pairs for no measured gain; within the budget,
///   because "auto-confirm" means nobody is asked, so the only honest way to
///   place it is against the rate of strangers it would silently name.
///
/// The first version of this function used `min(genuine)` and
/// `max(impostor) + ε`. A review finding killed that, correctly: both are
/// extreme order statistics, both are monotone in sample size (max(impostor)
/// can only rise, min(genuine) can only fall), so the suggest band widened and
/// the Known/New regions shrank as the corpus GREW. On a realistic CAM++
/// distribution with overlapping tails one confusable pair pushes auto-confirm
/// toward `1.0`, [`MatchResult::Known`] never fires, and an enrolled speaker is
/// asked "who is this?" in every meeting forever — which contradicts
/// [`who_is_this_chips`]'s own rule 3. A quantile of the impostor distribution
/// converges instead of diverging; `band_edges_do_not_diverge_as_the_sample_grows`
/// measures both rules on the same growing sample and prints the two curves.
///
/// ## What is reported alongside, and why
///
/// `far_at_auto_confirm` / `frr_at_auto_confirm` / `frr_at_new_voice_floor` are
/// the ACHIEVED rates on the sample the bands came from, and
/// `far_resolution` / `frr_resolution` (`1/impostor`, `1/genuine`) are the
/// smallest non-zero rates that sample can express. A band printed without its
/// resolution underneath it is a number nobody can support — YV124's
/// saturated-EER lesson, applied to this item's numbers.
///
/// # Errors
/// [`TuningError`] — see its variants. Every one of them is a condition under
/// which a returned pair of bands would be fiction.
pub fn bands_from_distribution(
    genuine: &[CosineSimilarity],
    impostor: &[CosineSimilarity],
    target_far: TargetFar,
) -> Result<TunedBands, TuningError> {
    if genuine.is_empty() || impostor.is_empty() {
        return Err(TuningError::EmptyDistribution);
    }
    let report = enrollment_eer(genuine, impostor);
    if report.eer >= 0.5 {
        return Err(TuningError::Indistinguishable { eer: report.eer });
    }

    let far_resolution = 1.0 / impostor.len() as f64;
    let frr_resolution = 1.0 / genuine.len() as f64;
    if target_far.get() < far_resolution {
        return Err(TuningError::TargetFarBelowResolution {
            target_far: target_far.get(),
            far_resolution,
        });
    }

    // The floor is the measured equal-error point. The edge is chosen from the
    // same grid, above the floor, on the achieved FAR of the f32 that will
    // actually ship — not of the f64 candidate — so the reported rate is the
    // rate the matcher will produce.
    let new_voice_floor = report.threshold_at_eer;
    let auto_confirm = operating_points(genuine, impostor)
        .into_iter()
        .map(|t| CosineSimilarity::new(t as f32))
        .find(|edge| {
            edge.get() > new_voice_floor.get() && far_at(impostor, *edge) <= target_far.get() + 1e-12
        });
    let Some(auto_confirm) = auto_confirm else {
        return Err(TuningError::NoRoomAboveImpostors {
            highest_impostor: impostor
                .iter()
                .map(|s| s.get())
                .fold(f32::NEG_INFINITY, f32::max),
        });
    };

    let bands = EnrollmentBands::from_measured_edges(auto_confirm, new_voice_floor).map_err(
        |_: BandError| TuningError::Degenerate {
            edge: auto_confirm.get(),
        },
    )?;

    Ok(TunedBands {
        bands,
        eer: report.eer,
        eer_threshold: report.threshold_at_eer,
        target_far: target_far.get(),
        far_at_auto_confirm: far_at(impostor, bands.auto_confirm()),
        frr_at_auto_confirm: frr_at(genuine, bands.auto_confirm()),
        frr_at_new_voice_floor: frr_at(genuine, bands.new_voice_floor()),
        far_resolution,
        frr_resolution,
        genuine: genuine.len(),
        impostor: impostor.len(),
    })
}

/// Turn labeled utterances into the genuine/impostor distribution
/// [`bands_from_distribution`] needs.
///
/// Every unordered pair of utterances is scored once: **genuine** when the two
/// labels match, **impostor** when they do not. That is the harness's own
/// definition of the two populations, and it is the whole bridge between "a
/// fixture with named speakers" and "a measured band" — which is what the item
/// means by tuning FROM the harness rather than from a blog post.
///
/// The caller supplies the utterances. Fixture (e) (three people, near-field,
/// no overlap) is the intended source once an embedder exists: slice its WAV by
/// the ground-truth RTTM, embed each span, and hand the labeled vectors here.
/// That call site is `meeting_eval`'s sweep, which YV126 owns — this function
/// is the part of it that can be executed today, on any machine, with no model.
///
/// # Panics
/// If two embeddings differ in dimension — see [`cosine_similarity`].
pub fn labeled_pair_scores(
    labeled: &[(String, Embedding)],
) -> (Vec<CosineSimilarity>, Vec<CosineSimilarity>) {
    let mut genuine = Vec::new();
    let mut impostor = Vec::new();
    for (i, (label_a, a)) in labeled.iter().enumerate() {
        for (label_b, b) in labeled.iter().skip(i + 1) {
            let score = cosine_similarity(a.as_slice(), b.as_slice());
            if label_a == label_b {
                genuine.push(score);
            } else {
                impostor.push(score);
            }
        }
    }
    (genuine, impostor)
}

/// The two thresholds are in different units and must be ordered, not compared.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderingViolation {
    /// The clustering threshold, as configured.
    pub clustering: CosineDistance,
    /// The new-voice floor converted into the clustering unit — the number the
    /// clustering threshold has to stay under.
    pub floor_as_distance: CosineDistance,
}

impl std::fmt::Display for OrderingViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "clustering distance {:.4} is not tighter than the new-voice floor \
             ({:.4} as a distance): clustering would merge two voices that \
             enrollment would refuse to call the same person",
            self.clustering.get(),
            self.floor_as_distance.get()
        )
    }
}

impl std::error::Error for OrderingViolation {}

/// Merged finding #20's second half, as a check rather than a paragraph.
///
/// `sherpa_onnx::FastClusteringConfig.threshold` is a **distance** (smaller is
/// more similar, default `0.5`); the enrollment bands are **similarities**
/// (larger is more similar). As the plan specified them — clustering at `0.5`
/// distance, new-voice floor at `0.55` similarity, i.e. `0.45` distance —
/// clustering was LOOSER than the identity decision it feeds. That is
/// incoherent: clustering would happily merge two voices into one cluster that
/// the enrollment matcher, handed the same pair, would refuse to call the same
/// person, and the merged cluster's centroid is then an average of two people.
///
/// So: clustering must be **strictly tighter** — a smaller distance — than the
/// new-voice floor expressed in the same unit. Both YV120 newtypes are in the
/// signature, so this can only be called with the units the right way round.
///
/// This function is where the ordering is enforced; it is not asserted against
/// a pair of shipped constants because **neither number ships yet** (YV126's
/// clustering distance and this item's bands are both parameters with no
/// default in the crate, for the reasons in the module header). The call site
/// that first supplies both is where it binds.
pub fn check_clustering_tighter_than_enrollment(
    clustering: CosineDistance,
    bands: &EnrollmentBands,
) -> Result<(), OrderingViolation> {
    let floor_as_distance = CosineDistance::from_similarity(bands.new_voice_floor());
    if floor_as_distance.get() > clustering.get() {
        Ok(())
    } else {
        Err(OrderingViolation {
            clustering,
            floor_as_distance,
        })
    }
}

// ---------------------------------------------------------------------------
// The match, once per cluster
// ---------------------------------------------------------------------------

/// What the matcher concluded about one cluster.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MatchResult {
    /// At or above the auto-confirm edge: the name is written, no question is
    /// asked. `condition_key` names the centroid that won, which is what makes
    /// a cross-device match legible rather than mysterious.
    #[serde(rename_all = "camelCase")]
    Known {
        profile_id: String,
        score: CosineSimilarity,
        condition_key: String,
    },
    /// Between the two edges: the best candidate is offered, pre-selected, and
    /// a human decides.
    #[serde(rename_all = "camelCase")]
    Suggested {
        profile_id: String,
        score: CosineSimilarity,
        condition_key: String,
    },
    /// Below the new-voice floor, or nothing is enrolled at all: this is
    /// somebody we do not know.
    New,
}

impl MatchResult {
    pub fn profile_id(&self) -> Option<&str> {
        match self {
            MatchResult::Known { profile_id, .. } | MatchResult::Suggested { profile_id, .. } => {
                Some(profile_id)
            }
            MatchResult::New => None,
        }
    }

    /// True when a human still has to answer something. [`MatchResult::Known`]
    /// is the only outcome that asks nothing.
    pub fn needs_a_human(&self) -> bool {
        !matches!(self, MatchResult::Known { .. })
    }
}

/// Match ONE cluster against the enrolled roster.
///
/// **One call per cluster.** Not per utterance, not per turn, not per segment —
/// see the module header, and `tests/match_cluster_runs_once_per_cluster.rs`,
/// which counts.
///
/// The score is the best any centroid of any profile offers (see
/// [`SpeakerProfile::best_match`]), so a person enrolled under three
/// microphones is matched on whichever of the three this recording resembles.
/// Ties break on the profile id, so the answer is stable across runs rather
/// than dependent on roster order — a wobbling suggestion is worse than a wrong
/// one, because the user cannot tell it is the same question.
///
/// **`probe_model` is which embedder produced `cluster_centroid`**, and it must
/// come from the model the sidecar was told to load
/// ([`EmbeddingModelId::of_loaded_embedder`] applied to the catalog entry
/// `DiarizePool::load_models` was handed) — never from a profile, which would
/// make the comparison circular. Profiles enrolled under any other embedder are
/// skipped: a cosine between two embedding spaces is a meaningless number that
/// runs ~1.0 as readily as ~0.0, and above the auto-confirm edge a meaningless
/// number is a name written on a stranger with nobody in the loop.
pub fn match_cluster(
    cluster_centroid: &Embedding,
    probe_model: &EmbeddingModelId,
    profiles: &[SpeakerProfile],
    bands: EnrollmentBands,
) -> MatchResult {
    let best = profiles
        .iter()
        .filter_map(|p| {
            p.best_match(cluster_centroid, probe_model)
                .map(|(s, k)| (p, s, k))
        })
        .max_by(|a, b| {
            a.1.get()
                .total_cmp(&b.1.get())
                .then_with(|| b.0.id.cmp(&a.0.id))
        });

    let Some((profile, score, condition_key)) = best else {
        return MatchResult::New;
    };

    if score.get() >= bands.auto_confirm().get() {
        MatchResult::Known {
            profile_id: profile.id.clone(),
            score,
            condition_key: condition_key.to_string(),
        }
    } else if score.get() >= bands.new_voice_floor().get() {
        MatchResult::Suggested {
            profile_id: profile.id.clone(),
            score,
            condition_key: condition_key.to_string(),
        }
    } else {
        MatchResult::New
    }
}

/// One clustered voice in a finished meeting: everything the identity decision
/// and the chip row need, and nothing about the audio it came from.
///
/// `speech_seconds` and `turns` are YV126's ranking inputs, carried here so the
/// chip row can apply the same floor a second time on the way to the screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSummary {
    pub cluster_index: i64,
    /// What the transcript currently calls this voice — `Speaker 2`, and the
    /// text the chip row echoes so the two surfaces name the same thing.
    pub label: String,
    pub centroid: Embedding,
    pub speech_seconds: f64,
    pub turns: usize,
}

/// One cluster and what the matcher said about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterDecision {
    pub cluster: ClusterSummary,
    pub result: MatchResult,
}

/// Match every cluster in a finished meeting — exactly once each.
///
/// `probe_model` is the embedder that produced THIS meeting's centroids; see
/// [`match_cluster`] for why it comes from the load path and not from the
/// roster.
pub fn match_meeting_clusters(
    clusters: &[ClusterSummary],
    probe_model: &EmbeddingModelId,
    profiles: &[SpeakerProfile],
    bands: EnrollmentBands,
) -> Vec<ClusterDecision> {
    match_meeting_clusters_with(clusters, |centroid| {
        match_cluster(centroid, probe_model, profiles, bands)
    })
}

/// [`match_meeting_clusters`] with the per-cluster matcher injected.
///
/// The seam exists so the once-per-cluster claim can be COUNTED rather than
/// read: `tests/match_cluster_runs_once_per_cluster.rs` passes a closure that
/// increments, over a fixture whose six clusters hold dozens of segments, and
/// asserts six. A test that could only inspect the output would pass just as
/// happily against an implementation that matched every segment and voted.
pub fn match_meeting_clusters_with(
    clusters: &[ClusterSummary],
    mut matcher: impl FnMut(&Embedding) -> MatchResult,
) -> Vec<ClusterDecision> {
    clusters
        .iter()
        .map(|cluster| ClusterDecision {
            result: matcher(&cluster.centroid),
            cluster: cluster.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The chip row
// ---------------------------------------------------------------------------

/// The ranking floor a cluster must clear to be worth a name.
///
/// YV126's replacement for the plan's `cluster count > max(8, attendees×2)`
/// hard reject, which misfires on exactly the case this backlog prioritises: a
/// manually-started six-person room legitimately produces ten to fifteen raw
/// clusters, and rejecting the whole pass throws away a good diarization.
///
/// Both edges are **parameters with no default in this crate**, for the same
/// reason the bands are: `30 s / 3 turns` appears in the backlog as a design
/// sketch, and the value that ships has to come from a measured run against
/// fixture (f). Everything below the floor rolls into one "Other" bucket
/// ([`ChipRow::rolled_into_other`]) instead of becoming a chip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChipFloor {
    pub min_speech_seconds: f64,
    pub min_turns: usize,
}

impl ChipFloor {
    pub fn new(min_speech_seconds: f64, min_turns: usize) -> Self {
        Self {
            min_speech_seconds,
            min_turns,
        }
    }

    pub fn admits(&self, cluster: &ClusterSummary) -> bool {
        cluster.speech_seconds >= self.min_speech_seconds && cluster.turns >= self.min_turns
    }
}

/// One name the user can pick for a cluster.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipCandidate {
    pub profile_id: String,
    pub display_name: String,
}

/// The pre-selected candidate on a chip: the matcher's best guess, with the
/// score that made it a guess rather than an answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipSuggestion {
    pub profile_id: String,
    pub display_name: String,
    pub score: CosineSimilarity,
}

/// One "who is this?" question — `Speaker 2 → [Jeisil ▾] [Aidan] [+ new]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerChip {
    pub cluster_index: i64,
    /// The label the transcript uses for this voice, echoed verbatim so the
    /// chip and the transcript cannot name the same cluster differently.
    pub cluster_label: String,
    /// Pre-selected when the matcher had a candidate between the bands; `None`
    /// when it did not, in which case this is a straight "who is this?".
    pub suggested: Option<ChipSuggestion>,
    /// The rest of the roster, in roster order, minus whoever is already
    /// suggested. Never scored — offering a ranked list of everyone would turn
    /// one question into a leaderboard.
    pub alternatives: Vec<ChipCandidate>,
    /// `+ new` is always available: the roster is never assumed complete.
    pub allow_new: bool,
    /// Seconds of speech behind this cluster, so the row can say why this voice
    /// is worth naming and the busiest unknown sorts first.
    pub speech_seconds: f64,
}

/// The whole row, plus what it deliberately did not ask about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipRow {
    pub chips: Vec<SpeakerChip>,
    /// Clusters that did not clear [`ChipFloor`] — the "Other" bucket. Counted
    /// rather than listed, because the number is the honest thing to show
    /// ("3 quieter voices") and the list would be the spam.
    pub rolled_into_other: usize,
}

/// Why a chip row could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipRowError {
    /// The meeting has not ended. The chip row is a meeting-detail affordance;
    /// asking "who is this?" while a recording is running is the modal,
    /// mid-meeting interruption the plan's F2 flow rules out.
    MeetingStillLive,
}

impl std::fmt::Display for ChipRowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChipRowError::MeetingStillLive => write!(
                f,
                "the who-is-this row is a completed-meeting affordance; this meeting has not ended"
            ),
        }
    }
}

impl std::error::Error for ChipRowError {}

/// Build the inline "who is this?" row for a **finished** meeting.
///
/// The batching rule, which is the item:
///
/// 1. `meeting_ended` false ⇒ [`ChipRowError::MeetingStillLive`]. There is no
///    mid-meeting path, and `who_is_this_never_modal_never_live` proves the
///    live capture module cannot even name this function.
/// 2. One chip per **cluster**, never per segment or per turn.
/// 3. [`MatchResult::Known`] clusters get no chip — the auto-confirm band
///    exists precisely so a confident match asks nothing.
/// 4. A cluster whose index appears in `answered` gets no chip. Offered once,
///    inline, and never again — including the `locked` assignments YV130's
///    correction UX writes, which is the half of YV128's `locked` rule this
///    item can honour today.
/// 5. Clusters below `floor` roll into [`ChipRow::rolled_into_other`]. This is
///    what keeps a six-person far-field classroom to a handful of questions
///    rather than one per detected voice change.
/// 6. The remainder sorts by speech time, descending, tie-broken by cluster
///    index: the voice that talked most is the one worth naming first.
pub fn who_is_this_chips(
    meeting_ended: bool,
    decisions: &[ClusterDecision],
    answered: &[i64],
    roster: &[SpeakerProfile],
    floor: ChipFloor,
) -> Result<ChipRow, ChipRowError> {
    if !meeting_ended {
        return Err(ChipRowError::MeetingStillLive);
    }

    let mut chips = Vec::new();
    let mut rolled_into_other = 0usize;

    for decision in decisions {
        if answered.contains(&decision.cluster.cluster_index) {
            continue;
        }
        if !decision.result.needs_a_human() {
            continue;
        }
        if !floor.admits(&decision.cluster) {
            rolled_into_other += 1;
            continue;
        }

        let suggested = match &decision.result {
            MatchResult::Suggested {
                profile_id, score, ..
            } => roster
                .iter()
                .find(|p| &p.id == profile_id)
                .map(|p| ChipSuggestion {
                    profile_id: p.id.clone(),
                    display_name: p.display_name.clone(),
                    score: *score,
                }),
            _ => None,
        };
        let suggested_id = suggested.as_ref().map(|s| s.profile_id.clone());
        chips.push(SpeakerChip {
            cluster_index: decision.cluster.cluster_index,
            cluster_label: decision.cluster.label.clone(),
            alternatives: roster
                .iter()
                .filter(|p| Some(&p.id) != suggested_id.as_ref())
                .map(|p| ChipCandidate {
                    profile_id: p.id.clone(),
                    display_name: p.display_name.clone(),
                })
                .collect(),
            suggested,
            allow_new: true,
            speech_seconds: decision.cluster.speech_seconds,
        });
    }

    chips.sort_by(|a, b| {
        b.speech_seconds
            .total_cmp(&a.speech_seconds)
            .then_with(|| a.cluster_index.cmp(&b.cluster_index))
    });

    Ok(ChipRow {
        chips,
        rolled_into_other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bands(auto: f32, floor: f32) -> EnrollmentBands {
        EnrollmentBands::for_test(CosineSimilarity::new(auto), CosineSimilarity::new(floor))
            .expect("well-ordered test bands")
    }

    /// The digest of the embedder these fixtures pretend to have been enrolled
    /// under. A stand-in for `catalog.json`'s pinned sha256 — what matters to
    /// these tests is only that two different models are two different strings.
    fn campp() -> EmbeddingModelId {
        EmbeddingModelId::new("sha256-campp")
    }

    fn profile(id: &str, name: &str, vectors: &[(&str, [f32; 3])]) -> SpeakerProfile {
        profile_from(id, name, campp(), vectors)
    }

    fn profile_from(
        id: &str,
        name: &str,
        embedding_model: EmbeddingModelId,
        vectors: &[(&str, [f32; 3])],
    ) -> SpeakerProfile {
        SpeakerProfile {
            id: id.to_string(),
            display_name: name.to_string(),
            is_me: false,
            embedding_model,
            centroids: vectors
                .iter()
                .map(|(k, v)| Centroid::new(*k, Embedding::new(v.to_vec())))
                .collect(),
        }
    }

    #[test]
    fn bands_reject_an_inverted_pair() {
        assert_eq!(
            EnrollmentBands::for_test(CosineSimilarity::new(0.4), CosineSimilarity::new(0.6)),
            Err(BandError::Inverted)
        );
        assert_eq!(
            EnrollmentBands::for_test(CosineSimilarity::new(0.5), CosineSimilarity::new(0.5)),
            Err(BandError::Inverted),
            "equal edges leave no suggest region and must not pass as a split"
        );
    }

    #[test]
    fn a_profile_scores_across_every_centroid_not_just_the_first() {
        let p = profile(
            "p1",
            "Jeisil",
            &[("laptop", [1.0, 0.0, 0.0]), ("airpods", [0.0, 1.0, 0.0])],
        );
        let (score, key) = p
            .best_match(&Embedding::new(vec![0.0, 1.0, 0.0]), &campp())
            .expect("a profile with centroids matches");
        assert!(score.get() > 0.99, "the airpods centroid is an exact match");
        assert_eq!(key, "airpods");
    }

    #[test]
    fn an_enrolled_name_with_no_voice_behind_it_is_never_suggested() {
        let empty = SpeakerProfile {
            id: "p1".into(),
            display_name: "Aidan".into(),
            is_me: false,
            embedding_model: campp(),
            centroids: vec![],
        };
        assert_eq!(
            match_cluster(
                &Embedding::new(vec![1.0, 0.0, 0.0]),
                &campp(),
                &[empty],
                bands(0.8, 0.5)
            ),
            MatchResult::New
        );
    }

    #[test]
    fn the_three_bands_are_the_three_outcomes() {
        let roster = [profile("p1", "Jeisil", &[("laptop", [1.0, 0.0, 0.0])])];
        let b = bands(0.90, 0.50);
        // cos = 1.0
        assert!(matches!(
            match_cluster(&Embedding::new(vec![1.0, 0.0, 0.0]), &campp(), &roster, b),
            MatchResult::Known { .. }
        ));
        // cos = 1/sqrt(2) ≈ 0.707 — between the edges
        assert!(matches!(
            match_cluster(&Embedding::new(vec![1.0, 1.0, 0.0]), &campp(), &roster, b),
            MatchResult::Suggested { .. }
        ));
        // cos = 0.0 — orthogonal
        assert!(matches!(
            match_cluster(&Embedding::new(vec![0.0, 1.0, 0.0]), &campp(), &roster, b),
            MatchResult::New
        ));
    }

    #[test]
    fn ties_break_on_profile_id_so_the_answer_does_not_depend_on_roster_order() {
        let a = profile("aaa", "A", &[("laptop", [1.0, 0.0, 0.0])]);
        let z = profile("zzz", "Z", &[("laptop", [1.0, 0.0, 0.0])]);
        let target = Embedding::new(vec![1.0, 0.0, 0.0]);
        let forward = match_cluster(&target, &campp(), &[a.clone(), z.clone()], bands(0.9, 0.5));
        let backward = match_cluster(&target, &campp(), &[z, a], bands(0.9, 0.5));
        assert_eq!(forward, backward);
        assert_eq!(forward.profile_id(), Some("aaa"));
    }

    /// **A profile enrolled under a different embedder is not a candidate.**
    ///
    /// Two 192-dim embedders produce vectors in unrelated spaces, so the cosine
    /// between them is a number rather than a similarity — and it is as happy
    /// to come out at 1.0 for a voice nobody has heard as at 0.0. Without the
    /// guard, this fixture (identical vectors, different model ids) scores 1.0
    /// and returns `Known`: a name written on a stranger with nobody in the
    /// loop, which is exactly the false accept `TargetFar` exists to bound.
    #[test]
    fn a_profile_from_another_embedder_is_skipped_rather_than_scored() {
        let other = EmbeddingModelId::new("sha256-some-other-192-dim-model");
        let roster = [profile_from(
            "p1",
            "Jeisil",
            other.clone(),
            &[("laptop", [1.0, 0.0, 0.0])],
        )];
        let identical = Embedding::new(vec![1.0, 0.0, 0.0]);

        assert_eq!(
            roster[0].best_match(&identical, &campp()),
            None,
            "a cosine across two embedding spaces is not a weak score, it is not a score"
        );
        assert_eq!(
            match_cluster(&identical, &campp(), &roster, bands(0.9, 0.5)),
            MatchResult::New,
            "an unusable roster asks a human; it never auto-confirms on a meaningless 1.0"
        );
        // …and the SAME roster under its own model is the auto-confirm it would
        // have been, so the guard is what changed the answer and not the fixture.
        assert!(matches!(
            match_cluster(&identical, &other, &roster, bands(0.9, 0.5)),
            MatchResult::Known { .. }
        ));
    }

    /// The guard fires before the dimension assert, so an ordinary model change
    /// is a skip rather than a panic in the middle of a meeting.
    #[test]
    fn a_model_mismatch_is_checked_before_the_dimension_panic() {
        let roster = [profile_from(
            "p1",
            "Jeisil",
            EmbeddingModelId::new("sha256-256-dim-resnet34"),
            &[("laptop", [1.0, 0.0, 0.0])],
        )];
        // 8 dims against 3: `cosine_similarity` would panic if it were reached.
        let wider = Embedding::new(vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(
            match_cluster(&wider, &campp(), &roster, bands(0.9, 0.5)),
            MatchResult::New
        );
    }

    /// The id is the pinned DIGEST of the file the sidecar loads, and only an
    /// embedding entry has one.
    #[test]
    fn the_embedding_model_id_is_the_catalogs_pinned_digest() {
        let embedding =
            crate::models::diarize_model_for_role(crate::models::DiarizeModelRole::Embedding)
                .expect("the catalog ships an embedding entry");
        let id = EmbeddingModelId::of_loaded_embedder(embedding)
            .expect("an embedding entry has an embedding-model id");
        assert_eq!(
            id.as_str(),
            embedding.file.sha256,
            "the plain .onnx entry's id is the digest of the bytes that were verified"
        );
        assert_ne!(
            id.as_str(),
            embedding.id,
            "the catalog ID does not change when the bytes are re-vendored, which is \
             precisely the case this guard exists for"
        );

        let segmentation =
            crate::models::diarize_model_for_role(crate::models::DiarizeModelRole::Segmentation)
                .expect("the catalog ships a segmentation entry");
        assert_eq!(
            EmbeddingModelId::of_loaded_embedder(segmentation),
            None,
            "segmentation produces no embeddings; asking it for an embedder id is a bug"
        );
    }

    #[test]
    fn the_ordering_check_accepts_a_tighter_clustering_and_rejects_a_looser_one() {
        // floor 0.55 similarity == 0.45 distance; clustering at 0.40 is tighter.
        let b = bands(0.90, 0.55);
        assert!(check_clustering_tighter_than_enrollment(CosineDistance::new(0.40), &b).is_ok());
        // The plan's own pair: sherpa's 0.5 default against a 0.55 floor.
        let violation = check_clustering_tighter_than_enrollment(CosineDistance::new(0.50), &b)
            .expect_err("0.50 distance is looser than a 0.45-distance floor");
        assert!((violation.floor_as_distance.get() - 0.45).abs() < 1e-6);
    }
}
