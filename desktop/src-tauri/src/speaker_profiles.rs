//! yap-1106-voiceprints — speaker profiles: one saved voiceprint per named
//! person, so a name sticks across meetings (fix for #151, see #152).
//!
//! A re-port of the parked `feat/yv128` (schema) and `feat/yv129` (enrollment
//! matching) branches onto current code, written fresh rather than rebased.
//!
//! ## What a voiceprint is here
//!
//! A `speaker_profiles` row holds the CENTROID of a person's speaker
//! embeddings: the L2-normalised mean of every L2-normalised sample in
//! `speaker_samples`. A turn is compared against each centroid by cosine
//! SIMILARITY ([`CosineSimilarity`], never a bare `f32` — merged finding #20's
//! unit split lives in `diarize_metrics`).
//!
//! ## The two bands
//!
//! * `auto` — at or above it the turn is labelled with the name ("Probably
//!   Alex — voice match 0.82").
//! * `ask` — at or above it (and below `auto`) the app suggests "Is this
//!   Alex?".
//! * below `ask` the voice is a new unknown speaker.
//!
//! The defaults are PROVISIONAL ([`PROVISIONAL_AUTO_SIMILARITY`] is the ONE
//! named 0.70 constant decisions.meeting-questions binds FB-22a, FB-22b and
//! this item to). They are meant to be replaced by [`MatchBands::tuned`] run
//! over the scored genuine/impostor population of #152; until that number is
//! measured on Yap's corpus they stay marked provisional, here and in
//! docs/ANALYZER-RESEARCH.md.
//!
//! ## Never across embedder digests
//!
//! Every profile records the sha256 of the embedding model that produced it
//! (`embedder_digest`, the pinned catalog digest — see
//! docs/ANALYZER-RESEARCH.md "## Pinned versions"). Vectors from two models
//! live in unrelated spaces, so a profile from another digest is never
//! compared: the matcher returns [`VoiceMatch::ReEnrollNeeded`] and the UI says
//! the voice has to be enrolled again.
//!
//! ## Turn bounds
//!
//! Only single-speaker turns are embedded, using the word-exact bounds of
//! `meeting_asr::build_speaker_turns` (ENGINEERING MUSTS (a)), trimmed by
//! [`EMBED_BOUNDARY_GUARD_SECONDS`] at both ends so the audio nearest a switch
//! never feeds a voiceprint.
//!
//! Everything here is local: profiles and samples live in the Yap database on
//! this Mac and are never sent anywhere ("Leaves this Mac: No").

use crate::diarize::EnrolledSpeaker;
use crate::diarize_metrics::{cosine_similarity, enrollment_eer, CosineSimilarity};
use crate::meeting_asr::SpeakerTurn;
use serde::{Deserialize, Serialize};

/// Migration 11 — the voiceprint tables, and the name a past meeting keeps.
///
/// `speaker_samples.profile_id` cascades: deleting a profile deletes its
/// samples (the delete path also deletes them explicitly, so the cascade is a
/// backstop, not the mechanism). `meeting_id` is NULL for a sample taken from
/// dictation audio (the "You" enrollment) and becomes NULL if the meeting is
/// deleted — the centroid already absorbed it.
///
/// `meeting_segments.speaker_profile_id` links a segment to the profile it was
/// labelled with; `speaker_name` is that name as PLAIN TEXT, so deleting the
/// profile unlinks the segment but the past meeting still reads "Alex".
pub const MIGRATION_11_SPEAKER_PROFILES: &str = r#"
CREATE TABLE IF NOT EXISTS speaker_profiles (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  display_name TEXT NOT NULL,
  is_me INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  embedder_digest TEXT NOT NULL,
  centroid BLOB NOT NULL,
  n_samples INTEGER NOT NULL DEFAULT 0,
  last_heard_at TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_speaker_profiles_one_me
  ON speaker_profiles(is_me) WHERE is_me = 1;

CREATE TABLE IF NOT EXISTS speaker_samples (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  profile_id INTEGER NOT NULL REFERENCES speaker_profiles(id) ON DELETE CASCADE,
  meeting_id TEXT REFERENCES meetings(id) ON DELETE SET NULL,
  embedding BLOB NOT NULL,
  seconds REAL NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_speaker_samples_profile ON speaker_samples(profile_id);

ALTER TABLE meeting_segments ADD COLUMN speaker_profile_id INTEGER;
ALTER TABLE meeting_segments ADD COLUMN speaker_name TEXT;
"#;

/// PROVISIONAL — the `auto` band: at or above this cosine similarity a turn is
/// labelled with the profile's name. The ONE named 0.70 threshold shared by
/// FB-22a / FB-22b / voiceprints (decisions.meeting-questions). To be replaced
/// by [`MatchBands::tuned`] on #152's scored population.
pub const PROVISIONAL_AUTO_SIMILARITY: f32 = 0.70;

/// PROVISIONAL — the `ask` band: at or above this (and below `auto`) the app
/// asks "Is this Alex?". Below it the speaker is a new unknown.
pub const PROVISIONAL_ASK_SIMILARITY: f32 = 0.55;

/// Audio this close to a turn boundary never feeds an embedding (ENGINEERING
/// MUSTS (a): "embeddings skip the 0.2 s nearest a boundary"). The one
/// definition lives with the turn builder.
pub const EMBED_BOUNDARY_GUARD_SECONDS: f64 = crate::meeting_asr::TURN_EMBED_GUARD_SECONDS;

/// The `settings_kv` key holding the "enroll my voice from dictation" answer:
/// `yes` / `no`; absent = never asked. Asked once, opt-in.
pub const ENROLL_ME_SETTING: &str = "voiceprints.enroll_me";

/// Eval-only overrides of the two bands (never read by a shipped default).
pub const EVAL_AUTO_ENV: &str = "YAP_EVAL_VOICEPRINT_AUTO";
pub const EVAL_ASK_ENV: &str = "YAP_EVAL_VOICEPRINT_ASK";

/// The Settings → Voices page's privacy line (the approved design calls the
/// page "Voices"). Kept here so the label and the
/// fact it states are one greppable place.
pub const LEAVES_THIS_MAC: &str = "Leaves this Mac: No";

/// The two similarity bands a match is decided in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchBands {
    pub auto: CosineSimilarity,
    pub ask: CosineSimilarity,
}

impl MatchBands {
    /// `ask` must not be above `auto`, or the "ask" band would be empty and
    /// unreachable values would look like a decision.
    pub fn new(auto: CosineSimilarity, ask: CosineSimilarity) -> Result<Self, String> {
        if ask.get() > auto.get() {
            return Err(format!(
                "the ask band ({}) is above the auto band ({})",
                ask.get(),
                auto.get()
            ));
        }
        Ok(Self { auto, ask })
    }

    /// The provisional defaults.
    pub fn provisional() -> Self {
        Self {
            auto: CosineSimilarity::new(PROVISIONAL_AUTO_SIMILARITY),
            ask: CosineSimilarity::new(PROVISIONAL_ASK_SIMILARITY),
        }
    }

    /// The provisional defaults, unless an eval sets [`EVAL_AUTO_ENV`] /
    /// [`EVAL_ASK_ENV`]. An unparsable or inverted override is ignored (and
    /// logged) rather than shipping a broken band.
    pub fn from_env_or_provisional() -> Self {
        let base = Self::provisional();
        let read = |key: &str| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.trim().parse::<f32>().ok())
        };
        let (auto, ask) = (read(EVAL_AUTO_ENV), read(EVAL_ASK_ENV));
        if auto.is_none() && ask.is_none() {
            return base;
        }
        let auto = auto.map(CosineSimilarity::new).unwrap_or(base.auto);
        let ask = ask.map(CosineSimilarity::new).unwrap_or(base.ask);
        match Self::new(auto, ask) {
            Ok(b) => {
                log::info!(
                    "speaker_profiles bands_override auto={:.3} ask={:.3}",
                    b.auto.get(),
                    b.ask.get()
                );
                b
            }
            Err(e) => {
                log::warn!("speaker_profiles bands_override_rejected reason={e:?}");
                base
            }
        }
    }

    /// Tune the bands on a scored population (#152): `ask` = the equal-error
    /// threshold of [`enrollment_eer`]; `auto` = the lowest threshold at which
    /// at most `max_auto_far` of the impostor scores are at or above it (never
    /// below `ask`).
    ///
    /// # Panics
    /// If either distribution is empty (as [`enrollment_eer`] does).
    pub fn tuned(
        genuine: &[CosineSimilarity],
        impostor: &[CosineSimilarity],
        max_auto_far: f64,
    ) -> Self {
        let eer = enrollment_eer(genuine, impostor);
        let ask = eer.threshold_at_eer;
        let mut desc: Vec<f32> = impostor.iter().map(|s| s.get()).collect();
        desc.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        let allowed = (max_auto_far.clamp(0.0, 1.0) * desc.len() as f64).floor() as usize;
        let auto = match desc.get(allowed) {
            // Just above the first impostor that must be rejected.
            Some(&score) => CosineSimilarity::new(score + 1e-4),
            None => ask,
        };
        let auto = if auto.get() < ask.get() { ask } else { auto };
        Self { auto, ask }
    }
}

/// One saved voiceprint, as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerProfile {
    pub id: i64,
    pub display_name: String,
    pub is_me: bool,
    pub created_at: String,
    pub updated_at: String,
    pub embedder_digest: String,
    /// Never serialised to the webview: a voiceprint has no business in the UI.
    #[serde(skip)]
    pub centroid: Vec<f32>,
    pub n_samples: i64,
    pub last_heard_at: Option<String>,
}

impl SpeakerProfile {
    /// The binary-mode target `diarize::TargetMode` takes, accepting at the
    /// `auto` band.
    pub fn to_enrolled(&self, bands: MatchBands) -> EnrolledSpeaker {
        EnrolledSpeaker::new(self.id, self.centroid.clone(), bands.auto)
    }
}

/// What a turn's embedding matched. Every arm is a SUGGESTION with its reason
/// (the similarity), never a fact.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VoiceMatch {
    /// At or above `auto`: label it.
    #[serde(rename_all = "camelCase")]
    Auto {
        profile_id: i64,
        display_name: String,
        similarity: CosineSimilarity,
    },
    /// Between `ask` and `auto`: "Is this Alex?".
    #[serde(rename_all = "camelCase")]
    Ask {
        profile_id: i64,
        display_name: String,
        similarity: CosineSimilarity,
    },
    /// Below `ask` against every comparable profile (or none enrolled).
    #[serde(rename_all = "camelCase")]
    Unknown { best: Option<CosineSimilarity> },
    /// Profiles exist but every one was made with a different embedding model
    /// (or dimension). Never compared; they need to be enrolled again.
    #[serde(rename_all = "camelCase")]
    ReEnrollNeeded { stale_profiles: usize },
}

impl VoiceMatch {
    /// The short tag the telemetry line carries.
    pub fn outcome(&self) -> &'static str {
        match self {
            VoiceMatch::Auto { .. } => "auto",
            VoiceMatch::Ask { .. } => "ask",
            VoiceMatch::Unknown { .. } => "unknown",
            VoiceMatch::ReEnrollNeeded { .. } => "reenroll",
        }
    }
}

/// Can `profile` be compared with an embedding of this digest and length?
pub fn comparable(profile: &SpeakerProfile, embedder_digest: &str, dim: usize) -> bool {
    profile.embedder_digest == embedder_digest
        && profile.centroid.len() == dim
        && profile.n_samples > 0
        && dim > 0
}

/// Match one embedding against the saved profiles.
///
/// Profiles of another `embedder_digest` (or dimension) are skipped, never
/// compared; if that leaves none while some exist, the answer is
/// [`VoiceMatch::ReEnrollNeeded`]. The best similarity wins; a tie goes to the
/// lower profile id so the answer is deterministic.
pub fn match_voice(
    embedding: &[f32],
    embedder_digest: &str,
    profiles: &[SpeakerProfile],
    bands: MatchBands,
) -> VoiceMatch {
    let mut best: Option<(&SpeakerProfile, CosineSimilarity)> = None;
    let mut stale = 0usize;
    for p in profiles {
        // A profile that has unlearned every sample has no voice to compare;
        // it is neither a match nor a stale model.
        if p.n_samples <= 0 || p.centroid.is_empty() {
            continue;
        }
        if !comparable(p, embedder_digest, embedding.len()) {
            stale += 1;
            continue;
        }
        let s = cosine_similarity(embedding, &p.centroid);
        let better = match best {
            None => true,
            Some((bp, bs)) => s.get() > bs.get() || (s.get() == bs.get() && p.id < bp.id),
        };
        if better {
            best = Some((p, s));
        }
    }
    match best {
        None if stale > 0 => VoiceMatch::ReEnrollNeeded {
            stale_profiles: stale,
        },
        None => VoiceMatch::Unknown { best: None },
        Some((p, s)) if s.get() >= bands.auto.get() => VoiceMatch::Auto {
            profile_id: p.id,
            display_name: p.display_name.clone(),
            similarity: s,
        },
        Some((p, s)) if s.get() >= bands.ask.get() => VoiceMatch::Ask {
            profile_id: p.id,
            display_name: p.display_name.clone(),
            similarity: s,
        },
        Some((_, s)) => VoiceMatch::Unknown { best: Some(s) },
    }
}

/// The suggestion line for a match ("Probably Alex — voice match 0.82"), or
/// `None` for an unknown voice.
pub fn suggestion_text(m: &VoiceMatch) -> Option<String> {
    match m {
        VoiceMatch::Auto {
            display_name,
            similarity,
            ..
        } => Some(format!(
            "Probably {display_name} — voice match {:.2}",
            similarity.get()
        )),
        VoiceMatch::Ask {
            display_name,
            similarity,
            ..
        } => Some(format!(
            "Is this {display_name}? Voice match {:.2}",
            similarity.get()
        )),
        VoiceMatch::ReEnrollNeeded { .. } => {
            Some("The voice model changed, so saved voices need to be enrolled again.".to_string())
        }
        VoiceMatch::Unknown { .. } => None,
    }
}

/// L2-normalised copy; an all-zero vector stays zero.
pub fn normalized(v: &[f32]) -> Vec<f32> {
    let norm = v
        .iter()
        .map(|x| (*x as f64) * (*x as f64))
        .sum::<f64>()
        .sqrt();
    if norm <= 0.0 || !norm.is_finite() {
        return vec![0.0; v.len()];
    }
    v.iter().map(|x| ((*x as f64) / norm) as f32).collect()
}

/// Fold one more sample into a centroid built from `n` samples: the
/// normalised weighted mean of the normalised vectors.
pub fn fold_into_centroid(centroid: &[f32], n: i64, sample: &[f32]) -> Result<Vec<f32>, String> {
    if sample.is_empty() {
        return Err("an empty embedding is not a voice sample".into());
    }
    if n <= 0 || centroid.is_empty() {
        return Ok(normalized(sample));
    }
    merge_centroids(centroid, n, &normalized(sample), 1)
}

/// The centroid of two sample sets of size `na` and `nb`.
pub fn merge_centroids(a: &[f32], na: i64, b: &[f32], nb: i64) -> Result<Vec<f32>, String> {
    if a.len() != b.len() {
        return Err(format!(
            "voiceprints of different dimensions ({} and {}) cannot be combined",
            a.len(),
            b.len()
        ));
    }
    let (wa, wb) = (na.max(0) as f64, nb.max(0) as f64);
    if wa + wb <= 0.0 {
        return Ok(normalized(a));
    }
    let a = normalized(a);
    let b = normalized(b);
    let mean: Vec<f32> = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (((*x as f64) * wa + (*y as f64) * wb) / (wa + wb)) as f32)
        .collect();
    Ok(normalized(&mean))
}

/// `f32` little-endian, the BLOB layout of `centroid` and `embedding`.
pub fn encode_embedding(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

pub fn decode_embedding(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err(format!(
            "a voiceprint BLOB of {} bytes is not f32s",
            bytes.len()
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

/// The audio window worth embedding inside `[start, end]`: trimmed by
/// [`EMBED_BOUNDARY_GUARD_SECONDS`] at both ends, `None` if what is left is
/// shorter than `min_seconds`.
pub fn embed_window_bounds(start: f64, end: f64, min_seconds: f64) -> Option<(f64, f64)> {
    if !start.is_finite() || !end.is_finite() {
        return None;
    }
    let s = start + EMBED_BOUNDARY_GUARD_SECONDS;
    let e = end - EMBED_BOUNDARY_GUARD_SECONDS;
    if e - s >= min_seconds.max(0.0) && e > s {
        Some((s, e))
    } else {
        None
    }
}

/// The embedding window of a diarized turn. Only a turn attributed to ONE
/// cluster is a single-speaker turn; an unattributed one is never embedded.
/// The trim is the turn's own [`SpeakerTurn::embedding_window`].
pub fn embed_window(turn: &SpeakerTurn, min_seconds: f64) -> Option<(f64, f64)> {
    turn.cluster_index?;
    let (s, e) = turn.embedding_window()?;
    (e - s >= min_seconds.max(0.0)).then_some((s, e))
}

/// The embedding-model digest a voiceprint made today stores: the catalog's
/// pinned sha256 of the embedding model (docs/ANALYZER-RESEARCH.md
/// "## Pinned versions"; `tests/supply_chain.rs` holds them equal).
pub fn pinned_embedder_digest() -> Option<String> {
    crate::models::diarize_model_for_role(crate::models::DiarizeModelRole::Embedding)
        .map(|m| m.file.sha256.clone())
}

/// A display name as stored: trimmed, non-empty, at most 80 characters.
pub fn clean_display_name(name: &str) -> Result<String, String> {
    let t = name.trim();
    if t.is_empty() {
        return Err("A speaker needs a name.".into());
    }
    Ok(t.chars().take(80).collect())
}

// ── yap-1106-who-is-this — the "Who was this?" cards ─────────────────────────
//
// After a meeting is diarized, each cluster with enough speech that matches no
// saved voice becomes a card (design meetings.html §B, approved 2026-11-06):
// at most [`MAX_VOICE_CARDS`], most speaking time first, a
// [`VOICE_CLIP_SECONDS`] clip cut inside ONE word-exact turn (so a clip never
// crosses a speaker switch), "Who was this?", Save / That's me / Not sure /
// Ignore this voice. Asked only AFTER the meeting, never during it.

/// Migration 12 — one row per (meeting, track, cluster): the cluster's voice
/// (the normalised mean of its turn embeddings, NULL when the pass produced
/// none or the person chose "Not sure" / "Ignore") and what the person decided.
/// Deleting the meeting deletes its rows; the voiceprint a Save fed lives on
/// in `speaker_samples`.
pub const MIGRATION_12_CLUSTER_VOICES: &str = r#"
CREATE TABLE IF NOT EXISTS meeting_cluster_voices (
  meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  track INTEGER NOT NULL,
  cluster_index INTEGER NOT NULL,
  embedder_digest TEXT,
  embedding BLOB,
  seconds REAL NOT NULL DEFAULT 0,
  state TEXT NOT NULL DEFAULT 'unknown',
  profile_id INTEGER,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (meeting_id, track, cluster_index)
);
"#;

/// A cluster needs at least this much speech before Yap asks who it was
/// (spec §9: "each cluster with ≥ 20 s of speech").
pub const UNKNOWN_VOICE_MIN_SECONDS: f64 = 20.0;
/// Never more than five cards (design: "five at most").
pub const MAX_VOICE_CARDS: usize = 5;
/// The clip length (design: "a 3-second clip").
pub const VOICE_CLIP_SECONDS: f64 = 3.0;

/// What the person decided about one cluster. `NotSure` leaves "Speaker N"
/// and keeps nothing; `Ignored` never asks about this voice in this meeting
/// again and keeps nothing either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClusterVoiceState {
    Unknown,
    Named,
    Me,
    NotSure,
    Ignored,
}

impl ClusterVoiceState {
    pub fn as_str(self) -> &'static str {
        match self {
            ClusterVoiceState::Unknown => "unknown",
            ClusterVoiceState::Named => "named",
            ClusterVoiceState::Me => "me",
            ClusterVoiceState::NotSure => "not_sure",
            ClusterVoiceState::Ignored => "ignored",
        }
    }

    pub fn parse(s: &str) -> ClusterVoiceState {
        match s {
            "named" => ClusterVoiceState::Named,
            "me" => ClusterVoiceState::Me,
            "not_sure" => ClusterVoiceState::NotSure,
            "ignored" => ClusterVoiceState::Ignored,
            _ => ClusterVoiceState::Unknown,
        }
    }
}

/// One stored cluster voice (the embedding never goes to the webview).
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterVoice {
    pub track: i64,
    pub cluster_index: i64,
    pub embedder_digest: Option<String>,
    pub embedding: Option<Vec<f32>>,
    pub seconds: f64,
    pub state: ClusterVoiceState,
}

/// One transcript row of a cluster, as the card picker sees it: a word-exact
/// turn (diarize-wire splits rows at every speaker switch).
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterLine {
    pub segment_id: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
    /// Already labelled with a saved voice.
    pub labelled: bool,
}

/// The clearest clip of a cluster: the LONGEST of its turns, cut to at most
/// [`VOICE_CLIP_SECONDS`] from that turn's first word. It never leaves the
/// turn, so it never crosses a speaker switch. `None` when no turn has length.
pub fn clearest_clip(lines: &[ClusterLine]) -> Option<(f64, f64, usize)> {
    let mut best: Option<(usize, f64)> = None;
    for (i, l) in lines.iter().enumerate() {
        let len = l.end_seconds - l.start_seconds;
        if !len.is_finite() || len <= 0.0 {
            continue;
        }
        if best.is_none_or(|(_, b)| len > b) {
            best = Some((i, len));
        }
    }
    best.map(|(i, len)| {
        let l = &lines[i];
        (
            l.start_seconds,
            l.start_seconds + len.min(VOICE_CLIP_SECONDS),
            i,
        )
    })
}

/// One "Who was this?" card, as the webview gets it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceCard {
    pub track: i64,
    pub cluster_index: i64,
    /// Talk seconds of the cluster's turns in this meeting.
    pub seconds: f64,
    pub lines: usize,
    /// The line the clip comes from (design: "the line it came from").
    pub line: String,
    pub clip_start: Option<f64>,
    pub clip_end: Option<f64>,
    /// "Same person as Alex?" — an `ask`-band (or `auto`-band, until the
    /// auto-label item labels those itself) match, with its reason.
    pub suggestion: Option<VoiceSuggestion>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSuggestion {
    pub profile_id: i64,
    pub display_name: String,
    pub similarity: CosineSimilarity,
}

/// One cluster's inputs to [`pick_voice_cards`].
#[derive(Debug, Clone)]
pub struct ClusterCandidate {
    pub track: i64,
    pub cluster_index: i64,
    pub state: ClusterVoiceState,
    pub lines: Vec<ClusterLine>,
    /// The cluster voice's match against the saved profiles; `None` when the
    /// pass stored no voice (the card can still be named from text).
    pub voice_match: Option<VoiceMatch>,
    /// The profile id of "You", so a voice that sounds like you is never asked
    /// about (design: "You are never asked about yourself").
    pub me_profile_id: Option<i64>,
}

/// Which clusters get a card: still `Unknown`, no line already labelled,
/// at least [`UNKNOWN_VOICE_MIN_SECONDS`] of speech, not "You"; most speaking
/// time first (ties: track, then cluster), at most [`MAX_VOICE_CARDS`].
pub fn pick_voice_cards(candidates: &[ClusterCandidate]) -> Vec<VoiceCard> {
    let mut cards: Vec<VoiceCard> = Vec::new();
    for c in candidates {
        if c.state != ClusterVoiceState::Unknown || c.lines.iter().any(|l| l.labelled) {
            continue;
        }
        let seconds: f64 = c
            .lines
            .iter()
            .map(|l| (l.end_seconds - l.start_seconds).max(0.0))
            .sum();
        if seconds < UNKNOWN_VOICE_MIN_SECONDS {
            continue;
        }
        let suggestion = match &c.voice_match {
            Some(VoiceMatch::Auto {
                profile_id,
                display_name,
                similarity,
            })
            | Some(VoiceMatch::Ask {
                profile_id,
                display_name,
                similarity,
            }) => {
                if Some(*profile_id) == c.me_profile_id {
                    continue;
                }
                Some(VoiceSuggestion {
                    profile_id: *profile_id,
                    display_name: display_name.clone(),
                    similarity: *similarity,
                })
            }
            _ => None,
        };
        let clip = clearest_clip(&c.lines);
        cards.push(VoiceCard {
            track: c.track,
            cluster_index: c.cluster_index,
            seconds,
            lines: c.lines.len(),
            line: clip
                .map(|(_, _, i)| c.lines[i].text.clone())
                .unwrap_or_default(),
            clip_start: clip.map(|(s, _, _)| s),
            clip_end: clip.map(|(_, e, _)| e),
            suggestion,
        });
    }
    cards.sort_by(|a, b| {
        b.seconds
            .partial_cmp(&a.seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.track.cmp(&b.track))
            .then(a.cluster_index.cmp(&b.cluster_index))
    });
    cards.truncate(MAX_VOICE_CARDS);
    cards
}

/// What naming a cluster targets.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NameTarget {
    /// An existing saved voice (a suggestion accepted, or picked from the list).
    #[serde(rename_all = "camelCase")]
    Profile { profile_id: i64 },
    /// A new name typed into the card.
    #[serde(rename_all = "camelCase")]
    NewName { name: String },
    /// "That's me".
    Me,
}

/// What [`crate::db::Database::name_meeting_cluster`] did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedCluster {
    pub profile_id: i64,
    pub display_name: String,
    /// Transcript lines relabelled in this meeting.
    pub relabelled: usize,
    /// A voice sample went into the voiceprint (false = named from text only,
    /// e.g. the audio was deleted before the pass stored a voice).
    pub sample_added: bool,
}

/// The display name "That's me" saves.
pub const ME_DISPLAY_NAME: &str = "You";
