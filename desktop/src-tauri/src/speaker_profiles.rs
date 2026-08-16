//! YV128 — an enrolled voice, and the centroid math that keeps a profile
//! comparable to itself across the microphones it was heard on.
//!
//! Pure/impure split, the shape `sysaudio.rs` already uses for
//! `OutputAudioState::restore_plan`: everything in this file is a function of
//! its arguments. No SQLite handle, no sidecar, no model bytes, no audio
//! hardware. `db.rs` owns the rows; this owns what a row MEANS, so "does a
//! centroid stay a unit vector after the fortieth update" is a test rather than
//! an eyeball over a formula.
//!
//! ## Two corrections from the audit live in here, not in a comment
//!
//! **Finding #19 — no width is assumed anywhere.** The epic plan's schema had
//! `embedding_dim INTEGER NOT NULL DEFAULT 512`, and nothing this backlog ships
//! is 512-dim: the recommended CAM++ embedder is 192-dim and the ResNet34
//! fallback the plan considered is 256-dim. A default is a guess the schema
//! makes on behalf of a model it has never loaded, and the moment it is wrong
//! it is wrong *silently* — every downstream length check passes against the
//! guess. So [`crate::meetings::MIGRATION_5_SPEAKER_PROFILES`] declares
//! `embedding_dim` `NOT NULL` with **no default**, and the only number that can
//! reach it is the one the sidecar reported at load time
//! (`DiarizeResponse::embedding_dim`, YV121/YV122). There is no width constant
//! in this file either — grep it: `192` appears in prose only.
//!
//! **Finding #21 — one running-mean vector per person does not survive a change
//! of microphone.** This is the exact case Wilson asked about: enrol on the
//! MacBook's own mic, then be recognised on AirPods. Embeddings of one true
//! speaker under two capture conditions do not sit near each other — different
//! transducer, different distance, different noise floor — so averaging them
//! into one centroid produces a vector that is a good match for neither
//! condition. A profile therefore holds **many** centroids, keyed by a coarse
//! condition bucket ([`condition_key_for_capture`]), and matching scores a probe
//! against **every centroid of every profile** and takes the best
//! ([`best_match`]). That is what makes cross-device recognition possible
//! without asking the user to enrol again on each device.
//!
//! **A different microphone, yes. A different model, never.** "Every centroid of
//! every profile" is scoped to the profiles enrolled under the SAME embedding
//! model as the probe: [`best_match`] takes the probe's model id and skips a
//! profile whose [`SpeakerProfile::embedding_model`] differs. Width alone cannot
//! stand in for that check — two 192-dim models produce two incomparable
//! 192-dim spaces, and a cosine between them is a number with no meaning that
//! YV129 would then put a threshold on.
//!
//! ## The L2 discipline, enforced by a type rather than by remembering
//!
//! The plan's running-mean formula (`new_avg = (avg × n + emb) / (n + 1)`) is
//! silent on normalisation, and un-normalised averaging drifts a centroid's
//! MAGNITUDE — which cosine comparison is supposed to be blind to, and is,
//! right up until the drift is large enough that `f32` precision on the dot
//! product is not. Both halves are required: normalise every embedding at
//! ingestion (before it enters any average) and renormalise the result after
//! each update (the average of two unit vectors is not a unit vector).
//!
//! Neither half is a rule a future caller has to know: [`NormalizedEmbedding`]
//! is the ONLY way to hand this module a vector, its constructor is the
//! normalisation, and [`Centroid::updated_with`] returns one — so an
//! un-normalised vector cannot enter an average and an un-renormalised one
//! cannot leave it. Same posture as YV120's [`CosineDistance`]/
//! [`CosineSimilarity`] newtypes: put the invariant where the compiler can hold
//! it, not where a code review has to.
//!
//! **Including serde.** `#[derive(Deserialize)]` on a newtype is a SECOND
//! constructor — it wraps whatever the wire sent and calls nothing — so a
//! derived impl would quietly re-open every case [`NormalizedEmbedding::new`]
//! refuses (`[3.0, 4.0]` arrives with norm 5, `[]` with dim 0, `[0.0, 0.0]`
//! with norm 0) and falsify the paragraph above for any value that came in
//! over JSON. The impl is therefore hand-written and routes through `new`,
//! turning a refused vector into a serde error. `serde_deserialisation_cannot_
//! bypass_the_normalising_constructor` (in
//! `tests/speaker_centroid_l2_normalize_before_and_after.rs`) is what keeps it
//! that way when YV129 wires the sidecar's `{"embedding": [...]}` response up
//! to this type.
//!
//! [`CosineDistance`]: crate::diarize_metrics::CosineDistance
//!
//! ## What is deliberately NOT here
//!
//! **No threshold. Not one.** Whether a similarity is high enough to be a
//! person is YV129's question, and its answer is an OUTPUT of YV120's harness
//! measured against a fixture — never a number copied from a vendor blog. This
//! module ranks and reports; it never decides. `no_tuned_similarity_constant_
//! ships_in_speaker_profiles` (in `tests/speaker_multi_centroid_best_match.rs`)
//! scans this file so the rule survives the next person in a hurry — and it
//! scans VALUES, not `const` declarations, because `let band = 0.62_f32;` in
//! the middle of [`best_match`] is the same defect and contains no `const`.
//! Every numeric literal on a non-comment line above `#[cfg(test)]` must be
//! `0.0` or `1.0`, the two arithmetic identities the centroid math needs.
//! That matters most for the NEXT item: YV129 puts `match_cluster` and the
//! enrollment bands in this same file.

use serde::{Deserialize, Serialize};

use crate::diarize_metrics::{cosine_similarity, CosineSimilarity};

/// `f32` is four bytes, and a stored embedding is `embedding_dim` of them. The
/// whole blob-length invariant is this constant times a number the model
/// reported — which is why the constant is here and the number never is.
pub const BYTES_PER_F32: usize = 4;

// ---------------------------------------------------------------------------
// Condition keys — a coarse bucket, inferred, never asked
// ---------------------------------------------------------------------------

/// The Mac's own microphone: far the most common enrolment condition, and the
/// one every in-person meeting is recorded on.
pub const CONDITION_LAPTOP_MIC_NEAR: &str = "laptop_mic_near";
/// AirPods, a Bluetooth headset, anything worn. Close-mic'd and codec-coloured
/// — the condition finding #21 names as the one a single centroid fails on.
pub const CONDITION_BLUETOOTH_NEAR: &str = "bluetooth_near";
/// A wired external interface or USB mic.
pub const CONDITION_EXTERNAL_MIC_NEAR: &str = "external_mic_near";
/// The system-audio tap (YV100): remote participants, already through their own
/// microphone and a call codec before they reach this Mac. A condition of its
/// own because nothing about it resembles a room.
pub const CONDITION_SYSTEM_TAP: &str = "system_tap";
/// We could not tell. A first-class bucket, not a failure: matching scores
/// across every centroid regardless of key, so an `unknown`-keyed centroid is
/// fully usable — it simply cannot be grouped with a later recording that we
/// also could not classify. Guessing a specific bucket to avoid this word would
/// pool two genuinely different conditions into one centroid, which is the
/// failure this whole scheme exists to prevent.
pub const CONDITION_UNKNOWN: &str = "unknown";

/// Which coarse capture condition a track was recorded under.
///
/// Inferred from what the capture path already knows — `record.rs` carries the
/// CoreAudio device name for the mic track, and the system track is the tap —
/// and NEVER asked of the user: a device/distance bucket is a fact about the
/// recording, and a question about it would be a question nobody can answer
/// better than the machine that made it.
///
/// Deliberately coarse. Buckets exist to keep obviously-different conditions
/// out of one average; a finer taxonomy (per model of headphone, say) would
/// split one person's enrolment across so many centroids that each holds one
/// sample, which is a running mean that never runs.
///
/// String matching on a device name is a heuristic and is treated as one: it
/// resolves to [`CONDITION_UNKNOWN`] rather than to a plausible-looking guess
/// whenever the name says nothing, and no branch here can produce a bucket the
/// name did not evidence.
pub fn condition_key_for_capture(track: i64, device_name: &str) -> &'static str {
    if track == crate::meetings::SYSTEM_TRACK {
        return CONDITION_SYSTEM_TAP;
    }
    let name = device_name.trim().to_ascii_lowercase();
    if name.is_empty() {
        return CONDITION_UNKNOWN;
    }
    // Worn/wireless first: "MacBook Pro Microphone" and "AirPods Pro" share no
    // token, but a Bluetooth headset's name often contains a Mac model word
    // (e.g. "Beats Studio Buds — Wilson's MacBook"), so the more specific test
    // has to run first or the generic one swallows it.
    const WORN: [&str; 6] = [
        "airpod",
        "bluetooth",
        "beats",
        "headset",
        "headphone",
        "buds",
    ];
    if WORN.iter().any(|k| name.contains(k)) {
        return CONDITION_BLUETOOTH_NEAR;
    }
    const BUILT_IN: [&str; 5] = ["macbook", "built-in", "built in", "internal", "imac"];
    if BUILT_IN.iter().any(|k| name.contains(k)) {
        return CONDITION_LAPTOP_MIC_NEAR;
    }
    const EXTERNAL: [&str; 6] = ["usb", "yeti", "shure", "scarlett", "interface", "podcast"];
    if EXTERNAL.iter().any(|k| name.contains(k)) {
        return CONDITION_EXTERNAL_MIC_NEAR;
    }
    CONDITION_UNKNOWN
}

// ---------------------------------------------------------------------------
// Embeddings
// ---------------------------------------------------------------------------

/// Everything that can be wrong with a vector on its way into, or out of, a
/// profile.
///
/// Each variant carries the numbers a person needs to act on it. A stored blob
/// that is the wrong length is not a rounding problem — it is a truncated write
/// or a row written under a different model — and the message says which two
/// numbers disagreed rather than "invalid embedding".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingError {
    /// A zero-length vector: no model produces one.
    Empty,
    /// A `NaN` or infinity reached the math. Refused rather than propagated: a
    /// `NaN` inside a centroid poisons every future comparison against it and
    /// sorts as neither similar nor dissimilar.
    NotFinite,
    /// Every component is zero (or the average of a sample and a centroid
    /// cancelled exactly). There is no unit vector in that direction because
    /// there is no direction, so this is refused instead of being normalised
    /// into an arbitrary one.
    ZeroNorm,
    /// Two vectors of different widths were compared or averaged — a profile
    /// enrolled under a different embedding model, or a wrong `embedding_dim`.
    DimensionMismatch { expected: usize, actual: usize },
    /// The blob-length invariant from finding #19: `blob.len()` must equal
    /// `embedding_dim * 4`.
    BlobLength {
        embedding_dim: usize,
        expected_bytes: usize,
        actual_bytes: usize,
    },
}

impl std::fmt::Display for EmbeddingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbeddingError::Empty => write!(f, "embedding is empty"),
            EmbeddingError::NotFinite => write!(f, "embedding contains NaN or infinity"),
            EmbeddingError::ZeroNorm => {
                write!(f, "embedding has zero magnitude and cannot be normalized")
            }
            EmbeddingError::DimensionMismatch { expected, actual } => write!(
                f,
                "embedding dimension mismatch: expected {expected}, got {actual}"
            ),
            EmbeddingError::BlobLength {
                embedding_dim,
                expected_bytes,
                actual_bytes,
            } => write!(
                f,
                "embedding blob is {actual_bytes} bytes but embedding_dim {embedding_dim} \
                 needs {expected_bytes} ({embedding_dim} x {BYTES_PER_F32})"
            ),
        }
    }
}

impl std::error::Error for EmbeddingError {}

/// A speaker embedding that is known to be finite and of unit L2 norm.
///
/// The only constructor normalises, so "was this normalised before it entered
/// the average" stops being a question anyone can get wrong. Everything in this
/// module takes and returns this type; raw `&[f32]` appears exactly at the two
/// boundaries (the sidecar's response, the stored blob) and is converted there.
/// Serde is hand-written below, **not** derived: `#[derive(Deserialize)]` on a
/// newtype generates a second constructor that skips [`Self::new`] entirely, so
/// a derived `NormalizedEmbedding` could hold `[3.0, 4.0]` (norm 5), `[]`, or
/// the zero vector — every input the constructor exists to refuse. See the
/// [`Deserialize`] impl below.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedEmbedding(Vec<f32>);

impl NormalizedEmbedding {
    /// L2-normalise a raw embedding — the "before averaging" half of the
    /// discipline, applied at ingestion so no caller can skip it.
    pub fn new(values: &[f32]) -> Result<Self, EmbeddingError> {
        if values.is_empty() {
            return Err(EmbeddingError::Empty);
        }
        if values.iter().any(|v| !v.is_finite()) {
            return Err(EmbeddingError::NotFinite);
        }
        // Accumulate in f64: a 192-dim vector of large components can overflow
        // an f32 sum of squares long before any single component does.
        let norm = values
            .iter()
            .map(|v| (*v as f64) * (*v as f64))
            .sum::<f64>();
        if norm <= 0.0 {
            return Err(EmbeddingError::ZeroNorm);
        }
        let norm = norm.sqrt();
        let unit: Vec<f32> = values.iter().map(|v| (*v as f64 / norm) as f32).collect();
        if unit.iter().any(|v| !v.is_finite()) {
            return Err(EmbeddingError::NotFinite);
        }
        Ok(Self(unit))
    }

    pub fn dim(&self) -> usize {
        self.0.len()
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }

    /// The measured L2 norm — `1.0` within `f32` tolerance for any value this
    /// type can hold. Public because the invariant is worth ASSERTING in tests
    /// rather than trusting the constructor's word for it.
    pub fn l2_norm(&self) -> f64 {
        l2_norm(&self.0)
    }

    /// Little-endian `f32`s, `dim * 4` bytes. The storage encoding, in one
    /// place, so the writer and the length check can never disagree.
    pub fn to_blob(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.0.len() * BYTES_PER_F32);
        for v in &self.0 {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// The load-time invariant finding #19 asks for, asserted on **read** and
    /// not only on write: `blob.len() == embedding_dim * 4`, checked every time
    /// a row is used.
    ///
    /// A truncated or half-written BLOB is caught the moment it is read, with
    /// both numbers in the error, rather than silently producing a shorter
    /// vector that then compares against everything at a plausible-looking low
    /// score. Length is the only corruption a blob can self-report — bytes that
    /// are the right length but garbage will normalise fine, which is why the
    /// `embedding_model` column exists to invalidate a profile when the model
    /// changes.
    pub fn from_blob(bytes: &[u8], embedding_dim: usize) -> Result<Self, EmbeddingError> {
        let expected =
            embedding_dim
                .checked_mul(BYTES_PER_F32)
                .ok_or(EmbeddingError::BlobLength {
                    embedding_dim,
                    expected_bytes: usize::MAX,
                    actual_bytes: bytes.len(),
                })?;
        if embedding_dim == 0 {
            return Err(EmbeddingError::Empty);
        }
        if bytes.len() != expected {
            return Err(EmbeddingError::BlobLength {
                embedding_dim,
                expected_bytes: expected,
                actual_bytes: bytes.len(),
            });
        }
        let values: Vec<f32> = bytes
            .chunks_exact(BYTES_PER_F32)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        // Re-normalising a stored unit vector is a no-op to within f32 noise;
        // doing it anyway means the invariant holds for every value of this
        // type no matter where it came from.
        Self::new(&values)
    }
}

/// Written as the bare JSON array of its components — the same shape a derived
/// `#[serde(transparent)]` would produce. Serialising cannot break the
/// invariant (the value already holds it), so this half is a plain forward.
impl Serialize for NormalizedEmbedding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// **Deserialisation routes through [`NormalizedEmbedding::new`], and that is
/// the whole point of writing this by hand.**
///
/// A derived `Deserialize` is a second constructor. It would take the wire's
/// `Vec<f32>` and wrap it — no L2 normalisation, no finite check, no empty
/// check, no zero-norm check — which makes every claim in this module's header
/// false for any value that arrived over serde rather than through `new`. The
/// module says an un-normalised vector cannot enter an average and
/// [`Centroid::updated_with`] leans on exactly that ("`sample` is already
/// unit-length, it cannot be anything else"); a derived impl is precisely the
/// thing that makes it something else.
///
/// So: deserialise into a raw `Vec<f32>`, then hand it to the one constructor.
/// A vector the constructor refuses becomes a serde error — the ingestion
/// boundary YV129 will parse the sidecar's `{"embedding": [...]}` at, refusing
/// the same inputs `new` refuses, with the same names.
impl<'de> Deserialize<'de> for NormalizedEmbedding {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = Vec::<f32>::deserialize(deserializer)?;
        NormalizedEmbedding::new(&values).map_err(serde::de::Error::custom)
    }
}

/// The L2 norm of a raw slice, accumulated in `f64`.
///
/// Free-standing (not only a method) because the tests that prove the
/// discipline have to measure the norm of vectors that are deliberately NOT
/// normalised — the naive running mean, to show it drifts.
pub fn l2_norm(values: &[f32]) -> f64 {
    values
        .iter()
        .map(|v| (*v as f64) * (*v as f64))
        .sum::<f64>()
        .sqrt()
}

// ---------------------------------------------------------------------------
// Centroids
// ---------------------------------------------------------------------------

/// One condition's running mean for one profile: the row shape of
/// `speaker_centroids`.
///
/// `sample_count` is the running-mean denominator for THIS condition, not for
/// the profile — that is the whole point of the split. Enrolling on AirPods
/// does not dilute the laptop-mic centroid, and forty laptop-mic samples do not
/// bury the two AirPods ones.
#[derive(Debug, Clone, PartialEq)]
pub struct Centroid {
    pub condition_key: String,
    pub vector: NormalizedEmbedding,
    pub sample_count: u32,
}

impl Centroid {
    /// The first sample under a condition IS that condition's centroid.
    pub fn first(condition_key: impl Into<String>, vector: NormalizedEmbedding) -> Self {
        Self {
            condition_key: condition_key.into(),
            vector,
            sample_count: 1,
        }
    }

    /// The plan's running mean — `(avg × n + emb) / (n + 1)` — with the two
    /// normalisation steps it was silent about, and with the second one
    /// enforced by the return type.
    ///
    /// `sample` is already unit-length (it cannot be anything else), so the
    /// "before" half is discharged by the argument type. The "after" half is
    /// [`NormalizedEmbedding::new`] on the result: averaging two unit vectors
    /// yields a vector shorter than 1 (exactly 1 only when they are identical),
    /// and letting that shrink compound over forty updates is how a centroid
    /// stops being comparable to the samples it is made of.
    ///
    /// Returns [`EmbeddingError::ZeroNorm`] in the one degenerate case — a
    /// sample exactly antipodal to the centroid, whose mean is the origin —
    /// rather than inventing a direction. Real embeddings do not do this; a
    /// test fixture can, and an honest error is better than a silent zero
    /// vector that then matches everything at 0.0.
    pub fn updated_with(&self, sample: &NormalizedEmbedding) -> Result<Centroid, EmbeddingError> {
        if sample.dim() != self.vector.dim() {
            return Err(EmbeddingError::DimensionMismatch {
                expected: self.vector.dim(),
                actual: sample.dim(),
            });
        }
        let n = self.sample_count.max(1) as f64;
        let mean: Vec<f32> = self
            .vector
            .as_slice()
            .iter()
            .zip(sample.as_slice().iter())
            .map(|(avg, s)| (((*avg as f64) * n + (*s as f64)) / (n + 1.0)) as f32)
            .collect();
        Ok(Centroid {
            condition_key: self.condition_key.clone(),
            vector: NormalizedEmbedding::new(&mean)?,
            sample_count: self.sample_count.saturating_add(1),
        })
    }
}

/// An enrolled voice: the `speaker_profiles` row, without its centroids.
///
/// `embedding_dim` is the width the SIDECAR reported when it loaded the
/// embedding model, stored per profile so a later model change is detectable
/// rather than silently mixed. `embedding_model` is the catalog id for the same
/// reason: two 192-dim models produce two incomparable 192-dim spaces, so width
/// cannot stand in for space identity. That is not a caveat a caller has to
/// remember — [`best_match`] takes the probe's model id and skips any profile
/// whose `embedding_model` differs, and
/// `a_profile_of_another_model_at_the_same_width_is_skipped_not_compared`
/// (in `tests/speaker_multi_centroid_best_match.rs`) is what holds it there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakerProfile {
    pub id: String,
    pub display_name: String,
    pub embedding_dim: u32,
    pub embedding_model: String,
    /// A user-confirmed profile. `locked = 1` is never auto-overwritten by an
    /// automated pass — the rule YV129's matching and YV130's correction UX
    /// both have to respect absolutely.
    pub locked: bool,
    /// Wilson's own voice, as an ENROLLED PROFILE rather than a track index
    /// (YV125's correction, finding #4). "Me" used to be track 0 by
    /// construction, which is true only for a virtual call with a clean second
    /// track and false in every room; it is now a flag on a voice that was
    /// actually enrolled.
    pub is_me: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// A profile with every centroid it has accumulated — the unit matching reads.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileCentroids {
    pub profile: SpeakerProfile,
    pub centroids: Vec<Centroid>,
}

/// The best-scoring (profile, condition) pair for a probe embedding.
///
/// Carries the condition that won as well as the profile, because that is the
/// diagnostic that tells you WHY a match happened — "matched Wilson on his
/// bluetooth centroid" is actionable; "matched Wilson, 0.61" is not.
#[derive(Debug, Clone, PartialEq)]
pub struct CentroidMatch {
    pub profile_id: String,
    pub condition_key: String,
    pub similarity: CosineSimilarity,
}

/// Score a probe against **every centroid of every profile** and return the
/// best one, or `None` if there is nothing comparable to score against.
///
/// This is finding #21's cross-device mechanism in one function. A profile is
/// not represented by one canonical vector; it is represented by all of its
/// conditions, and the best of them wins. Enrol on a laptop mic, be recognised
/// on AirPods — because the AirPods centroid, once it exists, is scored too,
/// and until it exists the laptop one still gets its chance.
///
/// **Two skips, and the second is not implied by the first.** `probe_model` is
/// the catalog id of the embedding model that produced `probe`, and a profile
/// enrolled under a different one is SKIPPED rather than scored — a cosine
/// between two different models' outputs is arithmetic, not a similarity, no
/// matter how well the widths happen to agree. Width is checked too, per
/// centroid, because `cosine_similarity` panics on mismatched lengths by design
/// and truncating to make the shapes agree would invent a score across two
/// unrelated spaces. Neither check subsumes the other: the width check alone
/// lets CAM++ be compared against any other 192-dim embedder (and against
/// this same catalog id re-vendored to different weights), which is exactly the
/// contamination the model check exists to stop.
///
/// **No threshold.** The answer is a ranking and a number; whether that number
/// is high enough to put a name on a transcript is YV129's decision, tuned
/// against YV120's harness.
pub fn best_match(
    profiles: &[ProfileCentroids],
    probe: &NormalizedEmbedding,
    probe_model: &str,
) -> Option<CentroidMatch> {
    let mut best: Option<CentroidMatch> = None;
    for entry in profiles {
        if entry.profile.embedding_model != probe_model {
            continue;
        }
        for centroid in &entry.centroids {
            if centroid.vector.dim() != probe.dim() {
                continue;
            }
            let similarity = cosine_similarity(centroid.vector.as_slice(), probe.as_slice());
            let better = match &best {
                None => true,
                Some(current) => similarity > current.similarity,
            };
            if better {
                best = Some(CentroidMatch {
                    profile_id: entry.profile.id.clone(),
                    condition_key: centroid.condition_key.clone(),
                    similarity,
                });
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_is_the_constructor() {
        let e = NormalizedEmbedding::new(&[3.0, 4.0]).unwrap();
        assert!((e.l2_norm() - 1.0).abs() < 1e-6);
        assert!((e.as_slice()[0] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn degenerate_vectors_are_refused_not_normalized() {
        assert_eq!(NormalizedEmbedding::new(&[]), Err(EmbeddingError::Empty));
        assert_eq!(
            NormalizedEmbedding::new(&[0.0, 0.0, 0.0]),
            Err(EmbeddingError::ZeroNorm)
        );
        assert_eq!(
            NormalizedEmbedding::new(&[1.0, f32::NAN]),
            Err(EmbeddingError::NotFinite)
        );
    }

    #[test]
    fn a_blob_round_trips_and_a_short_one_is_named() {
        let e = NormalizedEmbedding::new(&[1.0, 2.0, 3.0, 4.0]).unwrap();
        let blob = e.to_blob();
        assert_eq!(blob.len(), 4 * BYTES_PER_F32);
        assert_eq!(NormalizedEmbedding::from_blob(&blob, 4).unwrap(), e);
        assert_eq!(
            NormalizedEmbedding::from_blob(&blob[..12], 4),
            Err(EmbeddingError::BlobLength {
                embedding_dim: 4,
                expected_bytes: 16,
                actual_bytes: 12
            })
        );
    }

    #[test]
    fn condition_keys_are_evidenced_never_guessed() {
        assert_eq!(
            condition_key_for_capture(crate::meetings::MIC_TRACK, "MacBook Pro Microphone"),
            CONDITION_LAPTOP_MIC_NEAR
        );
        assert_eq!(
            condition_key_for_capture(crate::meetings::MIC_TRACK, "AirPods Pro"),
            CONDITION_BLUETOOTH_NEAR
        );
        // A headset whose name also contains a Mac model word is worn, not
        // built in — the specific test runs first, on purpose.
        assert_eq!(
            condition_key_for_capture(
                crate::meetings::MIC_TRACK,
                "Beats Studio Buds - Wilson's MacBook"
            ),
            CONDITION_BLUETOOTH_NEAR
        );
        assert_eq!(
            condition_key_for_capture(crate::meetings::MIC_TRACK, "Blue Yeti"),
            CONDITION_EXTERNAL_MIC_NEAR
        );
        // Says nothing -> says nothing.
        assert_eq!(
            condition_key_for_capture(crate::meetings::MIC_TRACK, "Device 3"),
            CONDITION_UNKNOWN
        );
        assert_eq!(
            condition_key_for_capture(crate::meetings::MIC_TRACK, "   "),
            CONDITION_UNKNOWN
        );
        // The tap is its own condition whatever the input device is called.
        assert_eq!(
            condition_key_for_capture(crate::meetings::SYSTEM_TRACK, "MacBook Pro Microphone"),
            CONDITION_SYSTEM_TAP
        );
    }

    #[test]
    fn an_update_of_the_wrong_width_is_an_error_not_a_truncation() {
        let c = Centroid::first(
            CONDITION_UNKNOWN,
            NormalizedEmbedding::new(&[1.0, 0.0, 0.0]).unwrap(),
        );
        assert_eq!(
            c.updated_with(&NormalizedEmbedding::new(&[1.0, 0.0]).unwrap()),
            Err(EmbeddingError::DimensionMismatch {
                expected: 3,
                actual: 2
            })
        );
    }

    #[test]
    fn an_antipodal_sample_is_refused_rather_than_zeroed() {
        let c = Centroid::first(
            CONDITION_UNKNOWN,
            NormalizedEmbedding::new(&[1.0, 0.0]).unwrap(),
        );
        assert_eq!(
            c.updated_with(&NormalizedEmbedding::new(&[-1.0, 0.0]).unwrap()),
            Err(EmbeddingError::ZeroNorm)
        );
    }
}
