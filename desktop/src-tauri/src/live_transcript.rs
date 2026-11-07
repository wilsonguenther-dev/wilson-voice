//! yap-notetaker-notes — the light live transcript.
//!
//! While a meeting records, a rolling decode every [`LIVE_TICK`] reads the
//! newest audio of each track straight from the journal's spill files and
//! shows the last few lines under "You" / "Call" in the Meetings view
//! ([`LIVE_EVENT`], drawn by `src/meetings/LiveTranscript.tsx`). The finished
//! transcript replaces it after Stop: the worker emits an empty, non-recording
//! snapshot as it exits, and the stored segments are the record.
//!
//! **It is light on purpose.** A meeting that shares the Mac with dictation
//! must never make a dictation wait, so the decode:
//!
//! * never LOADS a model — it rides whatever engine is already warm and
//!   answers "busy" when the slot is empty or leased out ([`LiveDecoder`]);
//! * yields the whole tick to any dictation take (recording, waiting for, or
//!   holding the engine), and checks again before every track;
//! * decodes at most [`LIVE_WINDOW_SECONDS`] per track per tick, skipping
//!   ahead rather than catching up, and skips silence entirely.
//!
//! Nothing here is stored and nothing leaves the Mac: the lines live in this
//! worker's memory and in one Tauri event to the app's own window.
//!
//! ## Who is talking, live (yap-1106-live-whos-talking)
//!
//! Each decoded window is also embedded ONCE by the diarization sidecar
//! (`diarize::DiarizePool::embed`, the same child the post-meeting pass uses,
//! loaded once and kept warm for the meeting) and matched against the
//! ENROLLED voiceprints only (`speaker_profiles::match_voice`). The line then
//! reads the person's name ("Alex"), "Someone new", or the track's own word
//! ("You" / "Call"). There is no live clustering: the post-meeting pass is the
//! source of truth and replaces every live label. CPU budget: a window with
//! under [`LIVE_SPEAKER_MIN_SPEECH_SECONDS`] of speech is never embedded, a
//! track embeds at most once per window, and a sidecar that is not ready (no
//! models, no voiceprints, a failed load) leaves the line as You/Call silently.

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::diarize_metrics::cosine_similarity;
use crate::meetings::{speaker_label, DiarizationTarget};
use crate::speaker_profiles::{
    clean_display_name, fold_into_centroid, match_voice, merge_centroids, suggestion_text,
    MatchBands, SpeakerProfile, VoiceMatch,
};

/// The Tauri event the Meetings view listens on.
pub const LIVE_EVENT: &str = "meeting:live";
/// How often the rolling decode runs.
pub const LIVE_TICK: Duration = Duration::from_secs(10);
/// The most audio one track decodes per tick. Older audio than this is skipped:
/// a live view wants the newest words, and the final transcript has the rest.
pub const LIVE_WINDOW_SECONDS: usize = 12;
/// Less new audio than this waits for the next tick instead of decoding a word.
pub const LIVE_MIN_SECONDS: usize = 2;
/// Lines kept per track.
pub const LIVE_LINES_PER_TRACK: usize = 4;
/// RMS under this is silence and is never decoded (a silent window is where a
/// small model invents "Thank you.").
pub const LIVE_SILENCE_RMS: f32 = 0.004;
const RATE: usize = 16_000;
/// The live label of a voice that matched no enrolled profile.
pub const SOMEONE_NEW: &str = "Someone new";
/// Less speech than this in a window is never embedded (the CPU budget, and a
/// short span is where a voice match is noise).
pub const LIVE_SPEAKER_MIN_SPEECH_SECONDS: f32 = 2.0;
/// The frame the speech count is measured in: 100 ms.
const SPEECH_FRAME: usize = RATE / 10;

/// Who one live line is. `name: None` means "the track's own word" (You /
/// Call, or nothing on a mic-only meeting) — the fallback whenever no voice
/// was matched. `hint` is the reason behind a name ("Probably Alex — voice
/// match 0.82"): every live label is a suggestion, never a fact.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveVoice {
    pub name: Option<String>,
    pub hint: Option<String>,
    /// yap-1106-fb-22a — the live voice this line belongs to (a [`LiveRoster`]
    /// index) when "Ask who's talking during meetings" is on. Lets a name
    /// given live relabel every earlier line of that voice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<usize>,
}

/// One track's lines as the view draws them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTrack {
    pub track: usize,
    /// "You" / "Call" on a two-track meeting; `None` on a mic-only one, which
    /// shows its single track with no speaker label at all.
    pub speaker: Option<String>,
    pub lines: Vec<String>,
    /// One entry per line, in the same order: who said it, live.
    pub voices: Vec<LiveVoice>,
}

/// The payload of [`LIVE_EVENT`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSnapshot {
    pub recording: bool,
    pub tracks: Vec<LiveTrack>,
}

impl LiveSnapshot {
    /// What the worker emits on its way out: the live view clears.
    pub fn stopped() -> Self {
        Self {
            recording: false,
            tracks: Vec::new(),
        }
    }
}

/// The live label for `track` of a `tracks`-track recording. Two tracks are
/// the call shape (mic + the call's own audio), so the mic is "You" and the
/// tap is "Call" — the same words the stored transcript uses. One track gets
/// no label.
pub fn live_speaker(track: usize, tracks: usize) -> Option<String> {
    (tracks >= 2).then(|| speaker_label(track as i64, DiarizationTarget::MicIsMe).to_string())
}

/// Read every i16 sample appended to a spill since `from_byte`, keeping at
/// most `max_samples` of the newest. Returns the samples as f32 and the byte
/// offset the next read starts from. A spill is little-endian i16 mono at
/// 16 kHz (`meeting::MEETING_SPILL_EXT`).
pub fn read_spill_tail(
    path: &Path,
    from_byte: u64,
    max_samples: usize,
) -> std::io::Result<(Vec<f32>, u64)> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len() & !1;
    if len <= from_byte {
        return Ok((Vec::new(), from_byte.min(len)));
    }
    let max_bytes = (max_samples as u64) * 2;
    let start = if len - from_byte > max_bytes {
        len - max_bytes
    } else {
        from_byte & !1
    };
    f.seek(SeekFrom::Start(start))?;
    let mut buf = vec![0u8; (len - start) as usize];
    f.read_exact(&mut buf)?;
    let samples = buf
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect();
    Ok((samples, len))
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Seconds of speech in a window: the 100 ms frames whose RMS clears
/// [`LIVE_SILENCE_RMS`].
pub fn speech_seconds(samples: &[f32]) -> f32 {
    let frames = samples
        .chunks(SPEECH_FRAME)
        .filter(|f| f.len() == SPEECH_FRAME && rms(f) >= LIVE_SILENCE_RMS)
        .count();
    frames as f32 * (SPEECH_FRAME as f32 / RATE as f32)
}

/// One live speaker embedding. `Ok(None)` means "the sidecar is not ready"
/// and is a silent fallback to You/Call, never an error.
pub trait LiveVoiceEmbedder {
    fn embed(&self, samples_16k: &[f32]) -> Result<Option<Vec<f32>>, String>;
}

/// The live label for one match on `track` of a `tracks`-track meeting.
///
/// * `auto` band — the profile's name, with the reason as the hint;
/// * compared and below every band — "Someone new", except on the mic of a
///   call, which stays "You" (the mic is the person at this Mac);
/// * anything else (the `ask` band, nothing comparable, a re-enroll) — the
///   track's own word. The post-meeting pass asks the questions; live never
///   claims a name it is unsure of.
pub fn live_voice_label(m: &VoiceMatch, track: usize, tracks: usize) -> LiveVoice {
    match m {
        VoiceMatch::Auto { display_name, .. } => LiveVoice {
            name: Some(display_name.clone()),
            hint: suggestion_text(m),
            ..LiveVoice::default()
        },
        VoiceMatch::Unknown { best: Some(_) } if !(tracks >= 2 && track == 0) => LiveVoice {
            name: Some(SOMEONE_NEW.to_string()),
            hint: None,
            ..LiveVoice::default()
        },
        _ => LiveVoice::default(),
    }
}

/// What the live matcher compares against: the enrolled voiceprints, the
/// embedder digest they must share, and the bands.
pub struct LiveSpeakers<'a> {
    pub embedder: &'a dyn LiveVoiceEmbedder,
    pub profiles: &'a [SpeakerProfile],
    pub digest: &'a str,
    pub bands: MatchBands,
}

impl LiveSpeakers<'_> {
    /// Who spoke this window: at most ONE embed, none under
    /// [`LIVE_SPEAKER_MIN_SPEECH_SECONDS`] of speech, none while a dictation
    /// wants the Mac.
    pub fn label(
        &self,
        track: usize,
        tracks: usize,
        samples: &[f32],
        dictation_wants_engine: &dyn Fn() -> bool,
    ) -> LiveVoice {
        let t0 = Instant::now();
        match self.embed_once(track, samples, dictation_wants_engine, false) {
            Some((embedding, _)) => {
                let m = match_voice(&embedding, self.digest, self.profiles, self.bands);
                log::info!(
                    "live_speaker matched={} track={track} ms={}",
                    m.outcome(),
                    t0.elapsed().as_millis()
                );
                live_voice_label(&m, track, tracks)
            }
            None => LiveVoice::default(),
        }
    }

    /// The ONE embed of a window, with every skip rule of [`Self::label`]:
    /// none without voiceprints (unless `without_profiles`, the live
    /// question's own clustering), none under
    /// [`LIVE_SPEAKER_MIN_SPEECH_SECONDS`], none while a dictation wants the
    /// Mac. Returns the embedding and the window's speech seconds.
    pub fn embed_once(
        &self,
        track: usize,
        samples: &[f32],
        dictation_wants_engine: &dyn Fn() -> bool,
        without_profiles: bool,
    ) -> Option<(Vec<f32>, f32)> {
        if (self.profiles.is_empty() && !without_profiles) || dictation_wants_engine() {
            return None;
        }
        let speech = speech_seconds(samples);
        if speech < LIVE_SPEAKER_MIN_SPEECH_SECONDS {
            log::debug!("live_speaker skip reason=short_speech track={track} speech_s={speech:.1}");
            return None;
        }
        match self.embedder.embed(samples) {
            Ok(Some(embedding)) => Some((embedding, speech)),
            Ok(None) => None,
            Err(e) => {
                log::warn!("live_speaker failure track={track} reason={e}");
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// yap-1106-fb-22a — "Who's talking right now?", live and OFF by default.
//
// With Settings → Meetings → "Ask who's talking during meetings" on, every
// embedded window of the call side (never the mic of a call: that is the
// person at this Mac) joins a live voice in a [`LiveRoster`]. A voice that no
// voiceprint claims and that has spoken about [`LIVE_ASK_MIN_SPEECH_SECONDS`]
// raises ONE [`LiveAsk`] for the pill; the answer comes back through
// [`post_answer`]. Live labels stay provisional: the post-meeting pass is the
// source of truth, and a name given live seeds a voiceprint so that pass
// keeps it. No embed is added: the roster reuses the one embed per track per
// window that [`LiveSpeakers`] already makes.
// ---------------------------------------------------------------------------

/// The Tauri event the pill listens on for a live question.
pub const LIVE_ASK_EVENT: &str = "meeting:live-ask";
/// A voice is asked about after about this much of its speech.
pub const LIVE_ASK_MIN_SPEECH_SECONDS: f32 = 5.0;
/// At or above this a window is the same live voice; between the ask band
/// and this, a known or just-named person gets "Same person as John?".
pub const LIVE_SAME_PERSON_BELOW: f32 = 0.70;
/// The hint on a line whose name was given during the meeting.
pub const LIVE_NAMED_HINT: &str = "You named them during the meeting";

/// Who a "Same person as …?" question points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameTarget {
    /// An enrolled voiceprint.
    Profile(i64),
    /// A voice named earlier in this meeting.
    Cluster(usize),
}

/// One live question for the pill. Every arm names the live voice
/// (`cluster`) the answer goes back to.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LiveAsk {
    /// "Who's talking right now?"
    #[serde(rename_all = "camelCase")]
    New {
        cluster: usize,
        label: String,
        speech_seconds: f32,
    },
    /// "Same person as John? 62% sure"
    #[serde(rename_all = "camelCase")]
    SamePerson {
        cluster: usize,
        label: String,
        speech_seconds: f32,
        name: String,
        similarity: f32,
    },
}

/// One voice heard live in this meeting.
#[derive(Debug, Clone)]
pub struct LiveCluster {
    pub centroid: Vec<f32>,
    pub n: i64,
    pub speech_seconds: f32,
    /// A name given live, or the voiceprint's name when a profile claimed it.
    pub name: Option<String>,
    pub profile_id: Option<i64>,
    /// Asked about already (or never to be asked): at most once per meeting.
    pub asked: bool,
    /// "Yes, John" folded this voice into another live voice.
    pub merged_into: Option<usize>,
    /// The best known/just-named person under [`LIVE_SAME_PERSON_BELOW`].
    /// Frozen once `asked` is set: it is the person the card named.
    pub candidate: Option<(SameTarget, String, f32)>,
}

/// What a name given live hands the post-meeting side: a voiceprint seed.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSeed {
    pub cluster: usize,
    pub name: String,
    pub centroid: Vec<f32>,
    pub speech_seconds: f32,
}

/// What a "Same person as John?" answer did.
#[derive(Debug, Clone, PartialEq)]
pub enum SameOutcome {
    /// "Yes" on an enrolled voiceprint: this sound becomes a sample of it.
    AddedToProfile {
        profile_id: i64,
        sample: Vec<f32>,
        speech_seconds: f32,
    },
    /// "Yes" on a voice named this meeting: the two are one speaker now.
    MergedInto { cluster: usize },
    /// "No, someone new": a speaker of its own.
    NewSpeaker,
}

/// An answer from the pill, queued for the live worker.
#[derive(Debug, Clone, PartialEq)]
pub enum LiveAnswer {
    Name { cluster: usize, name: String },
    Same { cluster: usize, yes: bool },
    Skip { cluster: usize, reason: String },
}

/// What applying an answer leaves for the worker to store.
#[derive(Debug, Clone, PartialEq)]
pub enum AnswerEffect {
    Seed(LiveSeed),
    Same(SameOutcome),
    Skipped,
}

/// The live voices of one meeting.
#[derive(Debug, Clone)]
pub struct LiveRoster {
    pub clusters: Vec<LiveCluster>,
    /// A copy of the enrolled voiceprints; "Yes" folds a sample in here too,
    /// so the rest of this meeting matches John's phone voice.
    pub profiles: Vec<SpeakerProfile>,
    pub bands: MatchBands,
    pub ask_enabled: bool,
}

impl LiveRoster {
    pub fn new(profiles: Vec<SpeakerProfile>, bands: MatchBands, ask_enabled: bool) -> Self {
        Self {
            clusters: Vec::new(),
            profiles,
            bands,
            ask_enabled,
        }
    }

    /// The voice a cluster resolves to after any "Yes" merges.
    pub fn root(&self, mut cluster: usize) -> usize {
        let mut hops = 0;
        while let Some(next) = self.clusters.get(cluster).and_then(|c| c.merged_into) {
            cluster = next;
            hops += 1;
            if hops > self.clusters.len() {
                break;
            }
        }
        cluster
    }

    /// The name lines of `cluster` carry, if any.
    pub fn display_name(&self, cluster: usize) -> Option<String> {
        self.clusters
            .get(self.root(cluster))
            .and_then(|c| c.name.clone())
    }

    /// Distinct live speakers (merged voices count once).
    pub fn speakers(&self) -> usize {
        self.clusters
            .iter()
            .filter(|c| c.merged_into.is_none())
            .count()
    }

    /// The pill's word for an unnamed voice.
    pub fn label(cluster: usize) -> String {
        format!("Speaker {}", cluster + 1)
    }

    /// Put one embedded window into a live voice and say whether to ask.
    /// `m` is that window's match against the voiceprints.
    pub fn observe(
        &mut self,
        embedding: &[f32],
        speech: f32,
        m: &VoiceMatch,
    ) -> (usize, Option<LiveAsk>) {
        if let VoiceMatch::Auto {
            profile_id,
            display_name,
            ..
        } = m
        {
            // A voiceprint claims it: one live voice per profile, never asked.
            if let Some(i) = self
                .clusters
                .iter()
                .position(|c| c.merged_into.is_none() && c.profile_id == Some(*profile_id))
            {
                self.join(i, embedding, speech);
                return (i, None);
            }
            self.clusters.push(LiveCluster {
                centroid: normalized_or_empty(embedding),
                n: 1,
                speech_seconds: speech,
                name: Some(display_name.clone()),
                profile_id: Some(*profile_id),
                asked: true,
                merged_into: None,
                candidate: None,
            });
            return (self.clusters.len() - 1, None);
        }
        let best = self
            .clusters
            .iter()
            .enumerate()
            .filter(|(_, c)| c.merged_into.is_none() && c.centroid.len() == embedding.len())
            .map(|(i, c)| (i, cosine_similarity(embedding, &c.centroid).get()))
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)));
        let idx = match best {
            Some((i, s)) if s >= LIVE_SAME_PERSON_BELOW => {
                self.join(i, embedding, speech);
                i
            }
            _ => {
                self.clusters.push(LiveCluster {
                    centroid: normalized_or_empty(embedding),
                    n: 1,
                    speech_seconds: speech,
                    name: None,
                    profile_id: None,
                    asked: false,
                    merged_into: None,
                    candidate: None,
                });
                self.clusters.len() - 1
            }
        };
        // fix-l3b1-2: once a voice is asked about, its candidate is frozen:
        // "Yes, John" must merge into the person the card showed, never a
        // closer person a later window found after the question went out.
        if !self.clusters[idx].asked {
            self.note_candidate(idx, embedding, m);
        }
        let c = &mut self.clusters[idx];
        if !self.ask_enabled
            || c.asked
            || c.name.is_some()
            || c.speech_seconds < LIVE_ASK_MIN_SPEECH_SECONDS
        {
            return (idx, None);
        }
        c.asked = true;
        let ask = match c.candidate.clone() {
            Some((_, name, similarity)) => LiveAsk::SamePerson {
                cluster: idx,
                label: Self::label(idx),
                speech_seconds: c.speech_seconds,
                name,
                similarity,
            },
            None => LiveAsk::New {
                cluster: idx,
                label: Self::label(idx),
                speech_seconds: c.speech_seconds,
            },
        };
        (idx, Some(ask))
    }

    fn join(&mut self, i: usize, embedding: &[f32], speech: f32) {
        let c = &mut self.clusters[i];
        if let Ok(next) = fold_into_centroid(&c.centroid, c.n, embedding) {
            c.centroid = next;
            c.n += 1;
        }
        c.speech_seconds += speech;
    }

    /// Remember the closest known or just-named person in the ask band
    /// (at or above `bands.ask`, under [`LIVE_SAME_PERSON_BELOW`]).
    fn note_candidate(&mut self, idx: usize, embedding: &[f32], m: &VoiceMatch) {
        let floor = self.bands.ask.get();
        let mut best: Option<(SameTarget, String, f32)> = match m {
            VoiceMatch::Ask {
                profile_id,
                display_name,
                similarity,
            } if similarity.get() < LIVE_SAME_PERSON_BELOW => Some((
                SameTarget::Profile(*profile_id),
                display_name.clone(),
                similarity.get(),
            )),
            _ => None,
        };
        for (j, other) in self.clusters.iter().enumerate() {
            if j == idx || other.merged_into.is_some() || other.centroid.len() != embedding.len() {
                continue;
            }
            let Some(name) = other.name.clone() else {
                continue;
            };
            let s = cosine_similarity(embedding, &other.centroid).get();
            if s >= floor && s < LIVE_SAME_PERSON_BELOW && best.as_ref().is_none_or(|b| s > b.2) {
                best = Some((SameTarget::Cluster(j), name, s));
            }
        }
        if let Some(b) = best {
            let c = &mut self.clusters[idx];
            if c.candidate.as_ref().is_none_or(|old| b.2 > old.2) {
                c.candidate = Some(b);
            }
        }
    }

    /// A name typed live: the voice is that person for the rest of the meeting.
    pub fn name(&mut self, cluster: usize, name: &str) -> Result<LiveSeed, String> {
        let name = clean_display_name(name)?;
        let root = self.root(cluster);
        let c = self
            .clusters
            .get_mut(root)
            .ok_or_else(|| "That voice is no longer in this meeting.".to_string())?;
        c.name = Some(name.clone());
        c.asked = true;
        c.candidate = None;
        Ok(LiveSeed {
            cluster: root,
            name,
            centroid: c.centroid.clone(),
            speech_seconds: c.speech_seconds,
        })
    }

    /// "Yes, John" / "No, someone new" on a [`LiveAsk::SamePerson`].
    pub fn confirm_same(&mut self, cluster: usize, yes: bool) -> Result<SameOutcome, String> {
        let c = self
            .clusters
            .get_mut(cluster)
            .ok_or_else(|| "That voice is no longer in this meeting.".to_string())?;
        c.asked = true;
        let candidate = c.candidate.take();
        if !yes {
            return Ok(SameOutcome::NewSpeaker);
        }
        let Some((target, name, _)) = candidate else {
            return Err("There is no one to compare that voice with.".into());
        };
        let (sample, n, speech) = (c.centroid.clone(), c.n, c.speech_seconds);
        match target {
            SameTarget::Profile(profile_id) => {
                let c = &mut self.clusters[cluster];
                c.name = Some(name);
                c.profile_id = Some(profile_id);
                if let Some(p) = self.profiles.iter_mut().find(|p| p.id == profile_id) {
                    if let Ok(next) = fold_into_centroid(&p.centroid, p.n_samples, &sample) {
                        p.centroid = next;
                        p.n_samples += 1;
                    }
                }
                Ok(SameOutcome::AddedToProfile {
                    profile_id,
                    sample,
                    speech_seconds: speech,
                })
            }
            SameTarget::Cluster(j) => {
                let into = self.root(j);
                if into == cluster {
                    return Err("That voice cannot be merged into itself.".into());
                }
                let t = &mut self.clusters[into];
                if let Ok(next) = merge_centroids(&t.centroid, t.n, &sample, n) {
                    t.centroid = next;
                    t.n += n;
                }
                t.speech_seconds += speech;
                self.clusters[cluster].merged_into = Some(into);
                Ok(SameOutcome::MergedInto { cluster: into })
            }
        }
    }

    /// Skip, or 20 s with no answer: the voice stays "Speaker N" for the
    /// after-meeting cards and is never asked about again in this call.
    pub fn skip(&mut self, cluster: usize) -> Result<(), String> {
        let c = self
            .clusters
            .get_mut(cluster)
            .ok_or_else(|| "That voice is no longer in this meeting.".to_string())?;
        c.asked = true;
        c.candidate = None;
        Ok(())
    }
}

fn normalized_or_empty(v: &[f32]) -> Vec<f32> {
    fold_into_centroid(&[], 0, v).unwrap_or_default()
}

/// Answers waiting for the live worker (it drains them every 250 ms).
static ANSWERS: std::sync::Mutex<Vec<LiveAnswer>> = std::sync::Mutex::new(Vec::new());

/// Queue an answer from the pill. Refused when no live worker runs.
pub fn post_answer(a: LiveAnswer) -> Result<(), String> {
    if !RUNNING.load(Ordering::SeqCst) {
        return Err("No meeting is recording now.".into());
    }
    ANSWERS
        .lock()
        .map_err(|_| "The live meeting could not take that answer.".to_string())?
        .push(a);
    Ok(())
}

fn take_answers() -> Vec<LiveAnswer> {
    ANSWERS
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

/// The pill's "Save" on "Who's talking right now?".
#[tauri::command]
pub fn meeting_live_name(cluster: usize, name: String) -> Result<(), String> {
    log::info!("live_ask answer kind=name cluster={cluster}");
    post_answer(LiveAnswer::Name { cluster, name })
}

/// The pill's "Yes, John" / "No, someone new".
#[tauri::command]
pub fn meeting_live_same(cluster: usize, yes: bool) -> Result<(), String> {
    log::info!("live_ask answer kind=same cluster={cluster} yes={yes}");
    post_answer(LiveAnswer::Same { cluster, yes })
}

/// The pill's "Skip", or the card sliding away after 20 s.
#[tauri::command]
pub fn meeting_live_skip(cluster: usize, reason: String) -> Result<(), String> {
    let reason: String = reason
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(24)
        .collect();
    log::info!("live_ask answer kind=skip cluster={cluster} reason={reason}");
    post_answer(LiveAnswer::Skip { cluster, reason })
}

/// One live decode. `Ok(None)` means "not now" (the engine is busy, leased
/// out, or nothing is loaded) and is a yield, never an error.
pub trait LiveDecoder {
    fn decode(&self, samples_16k: &[f32]) -> Result<Option<String>, String>;
}

/// What one tick did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickOutcome {
    /// A dictation wanted the engine; nothing was decoded.
    Yielded,
    /// `decoded` tracks produced a line.
    Ran { decoded: usize },
}

struct Cursor {
    offset: u64,
    lines: VecDeque<String>,
    voices: VecDeque<LiveVoice>,
}

/// The rolling state for one meeting.
pub struct LiveState {
    spills: Vec<PathBuf>,
    cursors: Vec<Cursor>,
    /// yap-1106-fb-22a — the live voices, when the worker attached a roster.
    roster: Option<LiveRoster>,
    /// Questions raised by the last tick, for the worker to emit.
    asks: Vec<LiveAsk>,
}

impl LiveState {
    pub fn new(spills: Vec<PathBuf>) -> Self {
        let cursors = spills
            .iter()
            .map(|_| Cursor {
                offset: 0,
                lines: VecDeque::new(),
                voices: VecDeque::new(),
            })
            .collect();
        Self {
            spills,
            cursors,
            roster: None,
            asks: Vec::new(),
        }
    }

    /// yap-1106-fb-22a — keep live voices (and ask about new ones when the
    /// roster's `ask_enabled` is on).
    pub fn with_roster(mut self, roster: LiveRoster) -> Self {
        self.roster = Some(roster);
        self
    }

    pub fn roster(&self) -> Option<&LiveRoster> {
        self.roster.as_ref()
    }

    /// Follow the setting while the meeting runs. Off stops new questions;
    /// it never forgets a name already given.
    pub fn set_ask_enabled(&mut self, on: bool) {
        if let Some(r) = self.roster.as_mut() {
            r.ask_enabled = on;
        }
    }

    /// The questions the last ticks raised, oldest first.
    pub fn take_asks(&mut self) -> Vec<LiveAsk> {
        std::mem::take(&mut self.asks)
    }

    /// Every line of a voice that now has a name carries it (a name given
    /// live, or "Yes, John" merging it into John).
    fn relabel(&mut self) {
        let Some(roster) = self.roster.as_ref() else {
            return;
        };
        for cursor in &mut self.cursors {
            for voice in cursor.voices.iter_mut() {
                let Some(name) = voice.cluster.and_then(|c| roster.display_name(c)) else {
                    continue;
                };
                if voice.name.as_deref() != Some(name.as_str()) {
                    voice.name = Some(name);
                    voice.hint = Some(LIVE_NAMED_HINT.to_string());
                }
            }
        }
    }

    /// Apply one answer from the pill. Lines relabel at once; the effect is
    /// what the worker stores (a voiceprint seed or a sample).
    pub fn apply_answer(&mut self, answer: LiveAnswer) -> Result<AnswerEffect, String> {
        let roster = self
            .roster
            .as_mut()
            .ok_or_else(|| "Live questions are off for this meeting.".to_string())?;
        let effect = match answer {
            LiveAnswer::Name { cluster, name } => AnswerEffect::Seed(roster.name(cluster, &name)?),
            LiveAnswer::Same { cluster, yes } => {
                AnswerEffect::Same(roster.confirm_same(cluster, yes)?)
            }
            LiveAnswer::Skip { cluster, reason } => {
                roster.skip(cluster)?;
                log::info!("live_ask deferred cluster={cluster} reason={reason}");
                AnswerEffect::Skipped
            }
        };
        self.relabel();
        let speakers = self.roster.as_ref().map_or(0, LiveRoster::speakers);
        match &effect {
            AnswerEffect::Seed(seed) => {
                log::info!(
                    "live_ask named cluster={} speakers={speakers}",
                    seed.cluster
                )
            }
            AnswerEffect::Same(SameOutcome::MergedInto { cluster }) => {
                log::info!("live_ask merged into={cluster} speakers={speakers}")
            }
            AnswerEffect::Same(SameOutcome::AddedToProfile { .. }) => {
                log::info!("live_ask merged into=profile speakers={speakers}")
            }
            AnswerEffect::Same(SameOutcome::NewSpeaker) => {
                log::info!("live_ask new_speaker speakers={speakers}")
            }
            AnswerEffect::Skipped => {}
        }
        Ok(effect)
    }

    /// Who said this window, with the roster: one embed (the same budget as
    /// [`LiveSpeakers::label`]), the voiceprint match, then the live voice.
    fn voice_for(
        &mut self,
        sp: &LiveSpeakers<'_>,
        track: usize,
        tracks: usize,
        samples: &[f32],
        dictation_wants_engine: &dyn Fn() -> bool,
    ) -> LiveVoice {
        let on = self.roster.as_ref().is_some_and(|r| r.ask_enabled);
        // Setting off (the default), or the mic of a call (the person at this
        // Mac): exactly the live-whos-talking path, never a question.
        if !on || (tracks >= 2 && track == 0) {
            return sp.label(track, tracks, samples, dictation_wants_engine);
        }
        let t0 = Instant::now();
        let Some((embedding, speech)) = sp.embed_once(track, samples, dictation_wants_engine, true)
        else {
            return LiveVoice::default();
        };
        let Some(roster) = self.roster.as_mut() else {
            return LiveVoice::default();
        };
        let m = match_voice(&embedding, sp.digest, &roster.profiles, sp.bands);
        let mut voice = live_voice_label(&m, track, tracks);
        let (cluster, ask) = roster.observe(&embedding, speech, &m);
        voice.cluster = Some(cluster);
        if let Some(name) = roster.display_name(cluster) {
            if voice.name.as_deref() != Some(name.as_str()) {
                voice.name = Some(name);
                voice.hint = Some(LIVE_NAMED_HINT.to_string());
            }
        }
        log::info!(
            "live_speaker matched={} track={track} cluster={cluster} ask={} ms={}",
            m.outcome(),
            ask.is_some(),
            t0.elapsed().as_millis()
        );
        if let Some(a) = ask {
            self.asks.push(a);
        }
        voice
    }

    pub fn spills(&self) -> &[PathBuf] {
        &self.spills
    }

    fn push_line(&mut self, track: usize, text: &str, voice: LiveVoice) {
        let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            return;
        }
        let cursor = &mut self.cursors[track];
        cursor.lines.push_back(line);
        cursor.voices.push_back(voice);
        while cursor.lines.len() > LIVE_LINES_PER_TRACK {
            cursor.lines.pop_front();
            cursor.voices.pop_front();
        }
    }

    pub fn snapshot(&self) -> LiveSnapshot {
        let n = self.cursors.len();
        LiveSnapshot {
            recording: true,
            tracks: self
                .cursors
                .iter()
                .enumerate()
                .map(|(track, c)| LiveTrack {
                    track,
                    speaker: live_speaker(track, n),
                    lines: c.lines.iter().cloned().collect(),
                    voices: c.voices.iter().cloned().collect(),
                })
                .collect(),
        }
    }

    /// One rolling decode over every track. `dictation_wants_engine` is asked
    /// before the tick and again before each track: a take that starts mid-tick
    /// gets the engine at the next track boundary, not after the tick.
    pub fn tick(
        &mut self,
        decoder: &dyn LiveDecoder,
        dictation_wants_engine: &dyn Fn() -> bool,
    ) -> Result<TickOutcome, String> {
        self.tick_with_speakers(decoder, None, dictation_wants_engine)
    }

    /// [`Self::tick`], plus who said each new line when `speakers` is given
    /// (one embed per track per window; see [`LiveSpeakers::label`]).
    pub fn tick_with_speakers(
        &mut self,
        decoder: &dyn LiveDecoder,
        speakers: Option<&LiveSpeakers<'_>>,
        dictation_wants_engine: &dyn Fn() -> bool,
    ) -> Result<TickOutcome, String> {
        let mut decoded = 0;
        let tracks = self.spills.len();
        for track in 0..tracks {
            if dictation_wants_engine() {
                return Ok(TickOutcome::Yielded);
            }
            let (samples, next) = read_spill_tail(
                &self.spills[track],
                self.cursors[track].offset,
                LIVE_WINDOW_SECONDS * RATE,
            )
            .map_err(|e| format!("read track {track}: {e}"))?;
            if samples.len() < LIVE_MIN_SECONDS * RATE {
                continue; // keep the cursor: this audio joins the next tick
            }
            if rms(&samples) < LIVE_SILENCE_RMS {
                self.cursors[track].offset = next;
                continue;
            }
            match decoder.decode(&samples)? {
                None => return Ok(TickOutcome::Yielded),
                Some(text) => {
                    self.cursors[track].offset = next;
                    let voice = if text.trim().is_empty() {
                        LiveVoice::default()
                    } else {
                        decoded += 1;
                        match speakers {
                            Some(sp) => {
                                self.voice_for(sp, track, tracks, &samples, dictation_wants_engine)
                            }
                            None => LiveVoice::default(),
                        }
                    };
                    self.push_line(track, &text, voice);
                }
            }
        }
        Ok(TickOutcome::Ran { decoded })
    }
}

/// The app's decoder: the warm engine, never a load.
pub struct WarmEngineLive {
    pub manager: crate::transcription::TranscriptionManager,
}

impl LiveDecoder for WarmEngineLive {
    fn decode(&self, samples_16k: &[f32]) -> Result<Option<String>, String> {
        use crate::meeting_asr::EngineDemand;
        if self.manager.dictation_pending() {
            return Ok(None);
        }
        let Some(model) = self.manager.loaded_model() else {
            return Ok(None); // nothing warm, or leased out to a decode
        };
        match self
            .manager
            .transcribe_timed_on(&model, samples_16k.to_vec(), None, None)
        {
            Ok(t) => Ok(Some(t.text)),
            // The slot changed hands between the check and the take: a yield.
            Err(e) if e.contains(crate::transcription::NO_ENGINE_LOADED) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Where the live speaker sidecar stands for this meeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidecarWarmth {
    /// Not asked yet: the first window with enough speech loads the models.
    Cold,
    /// Models loaded; stays warm for the meeting (and for the pass after it).
    Warm,
    /// Not ready this meeting (no models, a failed load, a failed embed):
    /// every line keeps You/Call, and nothing is retried.
    Off,
}

/// The app's live embedder: the shipped `yap-diarize` sidecar, one `embed`
/// call per window. The window is written as a short WAV next to the
/// meeting's own spill files and deleted right after the call, so no audio
/// lands anywhere new and nothing leaves the Mac.
pub struct SidecarLiveVoice {
    wav: PathBuf,
    warmth: std::cell::Cell<SidecarWarmth>,
}

impl SidecarLiveVoice {
    /// `dir` is the meeting's own folder (the spills' parent).
    pub fn new(dir: &Path) -> Self {
        Self {
            wav: dir.join(format!(".live-voice-{}.wav", std::process::id())),
            warmth: std::cell::Cell::new(SidecarWarmth::Cold),
        }
    }

    fn off(&self, reason: &str) -> Result<Option<Vec<f32>>, String> {
        self.warmth.set(SidecarWarmth::Off);
        log::info!("live_speaker sidecar_off reason={reason}");
        Ok(None)
    }

    fn write_window(&self, samples_16k: &[f32]) -> Result<(), String> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: RATE as u32,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&self.wav, spec).map_err(|e| format!("wav {e}"))?;
        for s in samples_16k {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            w.write_sample(v).map_err(|e| format!("wav {e}"))?;
        }
        w.finalize().map_err(|e| format!("wav {e}"))
    }
}

impl LiveVoiceEmbedder for SidecarLiveVoice {
    fn embed(&self, samples_16k: &[f32]) -> Result<Option<Vec<f32>>, String> {
        use crate::models::{
            diarize_model_for_role, diarize_model_path, is_diarize_ready, DiarizeModelRole,
        };
        let pool = crate::diarize::pool();
        match self.warmth.get() {
            SidecarWarmth::Off => return Ok(None),
            SidecarWarmth::Warm => {}
            SidecarWarmth::Cold => {
                if !is_diarize_ready() {
                    return self.off("models_missing");
                }
                let (Some(seg), Some(emb)) = (
                    diarize_model_for_role(DiarizeModelRole::Segmentation),
                    diarize_model_for_role(DiarizeModelRole::Embedding),
                ) else {
                    return self.off("models_missing");
                };
                let t0 = Instant::now();
                match pool.load_models(&diarize_model_path(seg), &diarize_model_path(emb)) {
                    Ok(dim) => {
                        self.warmth.set(SidecarWarmth::Warm);
                        log::info!(
                            "live_speaker sidecar_ready dim={dim} ms={}",
                            t0.elapsed().as_millis()
                        );
                    }
                    Err(e) => return self.off(&format!("load_{}", e.tag())),
                }
            }
        }
        if let Err(e) = self.write_window(samples_16k) {
            let _ = std::fs::remove_file(&self.wav);
            return Err(e);
        }
        // The caller's floor, the same one the post-meeting pass states (the
        // diarization crates carry no default on purpose).
        let result = pool.embed(&self.wav, crate::meeting_diarize::PROVISIONAL_SHORTEST_TURN);
        let _ = std::fs::remove_file(&self.wav);
        match result {
            Ok(v) if v.is_empty() => Ok(None),
            Ok(v) => Ok(Some(v)),
            // One failure turns the live labels off for this meeting: the
            // post-meeting pass is the record, and a retry storm is CPU the
            // dictation path would feel.
            Err(e) => self.off(&format!("embed_{}", e.tag())),
        }
    }
}

/// The enrolled voiceprints, read once when the meeting starts. Any failure
/// is an empty list: the live view then shows You/Call, as before.
fn enrolled_profiles(app: &tauri::AppHandle) -> Vec<SpeakerProfile> {
    use tauri::Manager;
    let Some(st) = app.try_state::<std::sync::Arc<crate::AppState>>() else {
        return Vec::new();
    };
    match st.db.list_speaker_profiles() {
        Ok(p) => p.into_iter().filter(|p| p.n_samples > 0).collect(),
        Err(e) => {
            log::warn!("live_speaker profiles_failure reason={e}");
            Vec::new()
        }
    }
}

/// yap-1106-fb-22a — Settings → Meetings → "Ask who's talking during
/// meetings" (`AppSettings::ask_whos_talking_live`, off by default).
fn ask_live_enabled(app: &tauri::AppHandle) -> bool {
    use tauri::Manager;
    app.try_state::<std::sync::Arc<crate::AppState>>()
        .map(|st| st.settings.lock().ask_whos_talking_live)
        .unwrap_or(false)
}

/// Store what a live answer means for later meetings and the post-meeting
/// pass: a name seeds (or feeds) that person's voiceprint, "Yes, John" adds
/// the sound to John's. Voiceprints stay on this Mac and are deletable in
/// Settings, exactly like the after-meeting cards' names.
fn store_effect(app: &tauri::AppHandle, digest: Option<&str>, effect: &AnswerEffect) {
    use tauri::Manager;
    let (Some(digest), Some(st)) = (digest, app.try_state::<std::sync::Arc<crate::AppState>>())
    else {
        log::info!("live_ask store skipped reason=no_digest_or_state");
        return;
    };
    let result = match effect {
        AnswerEffect::Seed(seed) if !seed.centroid.is_empty() => {
            let existing = st.db.list_speaker_profiles().ok().and_then(|ps| {
                ps.into_iter()
                    .find(|p| p.display_name.eq_ignore_ascii_case(&seed.name))
            });
            match existing {
                Some(p) => st
                    .db
                    .add_speaker_sample(
                        p.id,
                        digest,
                        &seed.centroid,
                        seed.speech_seconds as f64,
                        None,
                    )
                    .map(|_| "sample"),
                None => st
                    .db
                    .create_speaker_profile(
                        &seed.name,
                        false,
                        digest,
                        &seed.centroid,
                        seed.speech_seconds as f64,
                        None,
                    )
                    .map(|_| "profile"),
            }
        }
        AnswerEffect::Same(SameOutcome::AddedToProfile {
            profile_id,
            sample,
            speech_seconds,
        }) => st
            .db
            .add_speaker_sample(*profile_id, digest, sample, *speech_seconds as f64, None)
            .map(|_| "sample"),
        _ => Ok("none"),
    };
    match result {
        Ok(kind) => log::info!("live_ask store finish kind={kind}"),
        Err(e) => log::warn!("live_ask store failure reason={e}"),
    }
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static YIELDS: AtomicU64 = AtomicU64::new(0);

/// Start the live transcript for the meeting that is recording now. A no-op
/// when one is already running or no meeting is active. The worker ends by
/// itself when that meeting's capture leaves the active slot.
pub fn spawn(app: tauri::AppHandle, manager: crate::transcription::TranscriptionManager) {
    let Some(spills) = crate::meeting::active_capture().map(|c| c.spill_paths()) else {
        return;
    };
    if spills.is_empty() || RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let tracks = spills.len();
    let spawned = std::thread::Builder::new()
        .name("wv-live-transcript".into())
        .spawn(move || {
            use tauri::Emitter;
            log::info!("live_transcript start tracks={tracks}");
            // yap-1106-live-whos-talking — enrolled voices only; no digest or
            // no voiceprints means no embedding at all this meeting.
            let profiles = enrolled_profiles(&app);
            let digest = crate::speaker_profiles::pinned_embedder_digest();
            let voice_dir = spills
                .first()
                .and_then(|p| p.parent())
                .map(Path::to_path_buf)
                .unwrap_or_else(std::env::temp_dir);
            let embedder = SidecarLiveVoice::new(&voice_dir);
            // yap-1106-fb-22a — with "Ask who's talking" on, a meeting with no
            // voiceprints yet still embeds (one per track per window, the
            // same budget) so a new voice can be asked about.
            let ask_live = ask_live_enabled(&app);
            let bands = MatchBands::from_env_or_provisional();
            let speakers = match digest.as_deref() {
                Some(d) if !profiles.is_empty() || ask_live => Some(LiveSpeakers {
                    embedder: &embedder,
                    profiles: &profiles,
                    digest: d,
                    bands,
                }),
                _ => None,
            };
            drop(take_answers()); // nothing from an earlier meeting
            log::info!("live_ask start enabled={ask_live}");
            log::info!(
                "live_speaker start enrolled={} enabled={}",
                profiles.len(),
                speakers.is_some()
            );
            let mut state = LiveState::new(spills).with_roster(LiveRoster::new(
                profiles.clone(),
                bands,
                ask_live,
            ));
            let decoder = WarmEngineLive {
                manager: manager.clone(),
            };
            let mut ticks = 0u64;
            let mut failed = 0u64;
            let mut paused = false;
            let still_ours = |s: &LiveState| {
                crate::meeting::active_capture()
                    .map(|c| c.spill_paths())
                    .as_deref()
                    == Some(s.spills())
            };
            'outer: loop {
                let started = Instant::now();
                while started.elapsed() < LIVE_TICK {
                    std::thread::sleep(Duration::from_millis(250));
                    if !still_ours(&state) {
                        break 'outer;
                    }
                    // yap-1106-fb-22a — answers from the pill land within 250 ms.
                    let answers = take_answers();
                    if answers.is_empty() {
                        continue;
                    }
                    for answer in answers {
                        match state.apply_answer(answer) {
                            Ok(effect) => store_effect(&app, digest.as_deref(), &effect),
                            Err(e) => log::warn!("live_ask answer failure reason={e}"),
                        }
                    }
                    if let Err(e) = app.emit(LIVE_EVENT, state.snapshot()) {
                        log::warn!("live_transcript emit failure reason={e}");
                    }
                }
                state.set_ask_enabled(ask_live_enabled(&app));
                // yap-1106-resource-governor: at minimal the live view pauses;
                // the recording and the spill keep going underneath it.
                if crate::resource_governor::live_transcript_paused() {
                    if !paused {
                        paused = true;
                        log::info!("live_transcript paused reason=governor level=minimal");
                    }
                    continue;
                }
                if paused {
                    paused = false;
                    log::info!("live_transcript resumed reason=governor");
                }
                let demand = || {
                    use crate::meeting_asr::EngineDemand;
                    manager.dictation_pending()
                };
                let t0 = Instant::now();
                match state.tick_with_speakers(&decoder, speakers.as_ref(), &demand) {
                    Ok(TickOutcome::Yielded) => {
                        let n = YIELDS.fetch_add(1, Ordering::Relaxed) + 1;
                        log::info!("live_transcript yield reason=engine_busy yields={n}");
                    }
                    Ok(TickOutcome::Ran { decoded }) => {
                        ticks += 1;
                        log::info!(
                            "live_transcript tick finish decoded={decoded} ms={} ticks={ticks}",
                            t0.elapsed().as_millis()
                        );
                        if let Err(e) = app.emit(LIVE_EVENT, state.snapshot()) {
                            log::warn!("live_transcript emit failure reason={e}");
                        }
                        for ask in state.take_asks() {
                            if let Err(e) = app.emit(LIVE_ASK_EVENT, &ask) {
                                log::warn!("live_ask emit failure reason={e}");
                            }
                        }
                    }
                    Err(e) => {
                        // The spill vanished (the meeting is finalizing) or the
                        // engine refused: the live view is a convenience, the
                        // recording is untouched, so log and try again.
                        failed += 1;
                        log::warn!("live_transcript tick failure failures={failed} reason={e}");
                    }
                }
            }
            log::info!("live_transcript finish ticks={ticks} failures={failed}");
            if let Err(e) = app.emit(LIVE_EVENT, LiveSnapshot::stopped()) {
                log::warn!("live_transcript emit failure reason={e}");
            }
            RUNNING.store(false, Ordering::SeqCst);
        });
    if let Err(e) = spawned {
        RUNNING.store(false, Ordering::SeqCst);
        log::error!("live_transcript failure reason=spawn {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Echo(Cell<usize>);
    impl LiveDecoder for Echo {
        fn decode(&self, s: &[f32]) -> Result<Option<String>, String> {
            self.0.set(self.0.get() + 1);
            Ok(Some(format!("heard {} seconds", s.len() / RATE)))
        }
    }

    fn spill(dir: &Path, name: &str, seconds: usize, amp: i16) -> PathBuf {
        let p = dir.join(name);
        let mut bytes = Vec::new();
        for i in 0..seconds * RATE {
            let v = if i % 2 == 0 { amp } else { -amp };
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn two_tracks_are_you_and_call_one_track_is_unlabelled() {
        assert_eq!(live_speaker(0, 2).as_deref(), Some("You"));
        assert_eq!(live_speaker(1, 2).as_deref(), Some("Call"));
        assert_eq!(live_speaker(0, 1), None);
    }

    #[test]
    fn a_tick_decodes_only_the_newest_window_and_yields_to_dictation() {
        let dir = std::env::temp_dir().join(format!("yap-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = spill(&dir, "a.t0.spill.pcm", 30, 3000);
        let b = spill(&dir, "a.t1.spill.pcm", 30, 0); // silence
        let mut st = LiveState::new(vec![a, b]);
        let dec = Echo(Cell::new(0));
        // A dictation wants the engine: nothing is decoded at all.
        assert_eq!(st.tick(&dec, &|| true).unwrap(), TickOutcome::Yielded);
        assert_eq!(dec.0.get(), 0);
        // Free engine: track 0 decodes its newest 12 s, silent track 1 never decodes.
        assert_eq!(
            st.tick(&dec, &|| false).unwrap(),
            TickOutcome::Ran { decoded: 1 }
        );
        assert_eq!(dec.0.get(), 1);
        let snap = st.snapshot();
        assert_eq!(
            snap.tracks[0].lines,
            vec![format!("heard {LIVE_WINDOW_SECONDS} seconds")]
        );
        assert_eq!(snap.tracks[0].speaker.as_deref(), Some("You"));
        assert!(snap.tracks[1].lines.is_empty());
        // Nothing new on disk: the next tick decodes nothing.
        assert_eq!(
            st.tick(&dec, &|| false).unwrap(),
            TickOutcome::Ran { decoded: 0 }
        );
        assert_eq!(dec.0.get(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- yap-1106-live-whos-talking: fixture embeddings, no sidecar ----

    const DIGEST: &str = "fixture-digest";

    fn profile(id: i64, name: &str, centroid: Vec<f32>) -> SpeakerProfile {
        SpeakerProfile {
            id,
            display_name: name.to_string(),
            is_me: false,
            created_at: String::new(),
            updated_at: String::new(),
            embedder_digest: DIGEST.to_string(),
            centroid,
            n_samples: 3,
            last_heard_at: None,
        }
    }

    /// Answers every embed with the next fixture vector and counts the calls.
    /// `None` in the list is a sidecar that is not ready.
    struct Fixture {
        answers: std::cell::RefCell<VecDeque<Option<Vec<f32>>>>,
        calls: Cell<usize>,
    }

    impl Fixture {
        fn new(answers: Vec<Option<Vec<f32>>>) -> Self {
            Self {
                answers: std::cell::RefCell::new(answers.into()),
                calls: Cell::new(0),
            }
        }
    }

    impl LiveVoiceEmbedder for Fixture {
        fn embed(&self, _samples: &[f32]) -> Result<Option<Vec<f32>>, String> {
            self.calls.set(self.calls.get() + 1);
            Ok(self.answers.borrow_mut().pop_front().flatten())
        }
    }

    fn speech(seconds: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|i| if i % 2 == 0 { 0.1 } else { -0.1 })
            .collect()
    }

    fn alex_and_sam() -> Vec<SpeakerProfile> {
        vec![
            profile(1, "Alex", vec![1.0, 0.0, 0.0]),
            profile(2, "Sam", vec![0.0, 0.0, 1.0]),
        ]
    }

    #[test]
    fn live_speaker_fixture_embeddings_match_and_no_match_labels() {
        let profiles = alex_and_sam();
        let embedder = Fixture::new(vec![
            Some(vec![0.95, 0.05, 0.0]), // Alex, well above the auto band
            Some(vec![0.0, 1.0, 0.0]),   // nobody enrolled: similarity 0
            Some(vec![0.0, 1.0, 0.0]),   // nobody, but on the mic of a call
            Some(vec![0.6, 0.0, 0.8]),   // Sam at 0.80
            None,                        // the sidecar is not ready
        ]);
        let sp = LiveSpeakers {
            embedder: &embedder,
            profiles: &profiles,
            digest: DIGEST,
            bands: MatchBands::provisional(),
        };
        let window = speech(5.0);
        let free = || false;

        let alex = sp.label(1, 2, &window, &free);
        assert_eq!(alex.name.as_deref(), Some("Alex"));
        assert!(alex.hint.as_deref().unwrap().starts_with("Probably Alex"));

        let new_call = sp.label(1, 2, &window, &free);
        assert_eq!(new_call.name.as_deref(), Some(SOMEONE_NEW));
        assert_eq!(new_call.hint, None);

        // The mic of a call is the person at this Mac: an unmatched voice there stays "You".
        let mic = sp.label(0, 2, &window, &free);
        assert_eq!(mic, LiveVoice::default());

        let sam = sp.label(0, 1, &window, &free);
        assert_eq!(sam.name.as_deref(), Some("Sam"));
        assert!(sam.hint.as_deref().unwrap().contains("0.80"));

        // Not ready: silently the track's own word.
        assert_eq!(sp.label(1, 2, &window, &free), LiveVoice::default());
        assert_eq!(embedder.calls.get(), 5);
    }

    #[test]
    fn live_speaker_ask_band_and_other_digest_never_claim_a_name() {
        let m = match_voice(
            &[0.6, 0.8, 0.0],
            DIGEST,
            &alex_and_sam(),
            MatchBands::provisional(),
        );
        assert_eq!(m.outcome(), "ask"); // 0.60: between ask and auto
        assert_eq!(live_voice_label(&m, 1, 2), LiveVoice::default());
        let stale = match_voice(
            &[1.0, 0.0, 0.0],
            "another-model",
            &alex_and_sam(),
            MatchBands::provisional(),
        );
        assert_eq!(stale.outcome(), "reenroll");
        assert_eq!(live_voice_label(&stale, 1, 2), LiveVoice::default());
    }

    #[test]
    fn live_speaker_skips_short_speech_and_yields_to_dictation() {
        let profiles = alex_and_sam();
        let embedder = Fixture::new(vec![Some(vec![1.0, 0.0, 0.0])]);
        let sp = LiveSpeakers {
            embedder: &embedder,
            profiles: &profiles,
            digest: DIGEST,
            bands: MatchBands::provisional(),
        };
        // 1.5 s of speech inside 6 s of silence: under the 2 s budget.
        let mut window = vec![0.0f32; 6 * RATE];
        window.extend(speech(1.5));
        assert!(speech_seconds(&window) < LIVE_SPEAKER_MIN_SPEECH_SECONDS);
        assert_eq!(sp.label(1, 2, &window, &|| false), LiveVoice::default());
        // A dictation wants the Mac: no embed either.
        assert_eq!(sp.label(1, 2, &speech(4.0), &|| true), LiveVoice::default());
        assert_eq!(embedder.calls.get(), 0);
        // No voiceprints at all: never embeds.
        let none: Vec<SpeakerProfile> = Vec::new();
        let empty = LiveSpeakers {
            embedder: &embedder,
            profiles: &none,
            digest: DIGEST,
            bands: MatchBands::provisional(),
        };
        assert_eq!(
            empty.label(1, 2, &speech(4.0), &|| false),
            LiveVoice::default()
        );
        assert_eq!(embedder.calls.get(), 0);
    }

    #[test]
    fn a_tick_names_each_new_line_with_one_embed_per_track() {
        let dir = std::env::temp_dir().join(format!("yap-live-who-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = spill(&dir, "w.t0.spill.pcm", 20, 3000);
        let b = spill(&dir, "w.t1.spill.pcm", 20, 3000);
        let mut st = LiveState::new(vec![a, b]);
        let profiles = alex_and_sam();
        let embedder = Fixture::new(vec![
            Some(vec![0.0, 1.0, 0.0]), // mic: unmatched -> stays "You"
            Some(vec![1.0, 0.0, 0.0]), // call: Alex
        ]);
        let sp = LiveSpeakers {
            embedder: &embedder,
            profiles: &profiles,
            digest: DIGEST,
            bands: MatchBands::provisional(),
        };
        let dec = Echo(Cell::new(0));
        assert_eq!(
            st.tick_with_speakers(&dec, Some(&sp), &|| false).unwrap(),
            TickOutcome::Ran { decoded: 2 }
        );
        assert_eq!(embedder.calls.get(), 2);
        let snap = st.snapshot();
        assert_eq!(snap.tracks[0].speaker.as_deref(), Some("You"));
        assert_eq!(snap.tracks[0].voices, vec![LiveVoice::default()]);
        assert_eq!(snap.tracks[1].voices[0].name.as_deref(), Some("Alex"));
        assert_eq!(snap.tracks[1].voices.len(), snap.tracks[1].lines.len());
        // The payload the webview reads carries the name per line.
        let json = serde_json::to_value(&snap).unwrap();
        assert_eq!(json["tracks"][1]["voices"][0]["name"], "Alex");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- yap-1106-fb-22a — "Who's talking right now?" ----

    /// Append `seconds` more of speech to a spill (the meeting keeps going).
    fn grow(path: &Path, seconds: usize, amp: i16) {
        use std::io::Write;
        let mut bytes = Vec::new();
        for i in 0..seconds * RATE {
            let v = if i % 2 == 0 { amp } else { -amp };
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        f.write_all(&bytes).unwrap();
    }

    /// 62% like `[1, 0, 0]` (John's voice on his laptop): John on his phone.
    fn johns_phone() -> Vec<f32> {
        vec![0.62, (1.0f32 - 0.62 * 0.62).sqrt(), 0.0]
    }

    #[test]
    fn a_live_name_relabels_that_voices_live_lines_and_seeds_the_post_meeting_profile() {
        let dir = std::env::temp_dir().join(format!("yap-live-name-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = spill(&dir, "w.t0.spill.pcm", 20, 3000);
        let embedder = Fixture::new(vec![
            Some(vec![1.0, 0.0, 0.0]),  // John
            Some(vec![0.98, 0.2, 0.0]), // John again (above 70%: the same voice)
            Some(vec![0.0, 0.0, 1.0]),  // someone else
        ]);
        let no_profiles: Vec<SpeakerProfile> = Vec::new();
        let sp = LiveSpeakers {
            embedder: &embedder,
            profiles: &no_profiles,
            digest: DIGEST,
            bands: MatchBands::provisional(),
        };
        let mut st = LiveState::new(vec![a.clone()]).with_roster(LiveRoster::new(
            Vec::new(),
            MatchBands::provisional(),
            true,
        ));
        let dec = Echo(Cell::new(0));
        st.tick_with_speakers(&dec, Some(&sp), &|| false).unwrap();
        grow(&a, 6, 3000);
        st.tick_with_speakers(&dec, Some(&sp), &|| false).unwrap();
        grow(&a, 6, 3000);
        st.tick_with_speakers(&dec, Some(&sp), &|| false).unwrap();
        assert_eq!(
            embedder.calls.get(),
            3,
            "one embed per track per window, no more"
        );

        let asks = st.take_asks();
        assert_eq!(asks.len(), 2, "each new voice is asked about once");
        assert!(matches!(asks[0], LiveAsk::New { cluster: 0, .. }));
        let voices = st.snapshot().tracks[0].voices.clone();
        assert_eq!(
            voices.iter().map(|v| v.cluster).collect::<Vec<_>>(),
            vec![Some(0), Some(0), Some(1)]
        );

        let effect = st
            .apply_answer(LiveAnswer::Name {
                cluster: 0,
                name: "John".into(),
            })
            .unwrap();
        let AnswerEffect::Seed(seed) = effect else {
            panic!("a live name seeds the post-meeting voiceprint");
        };
        assert_eq!(seed.name, "John");
        assert_eq!(seed.cluster, 0);
        assert_eq!(seed.centroid.len(), 3);
        assert!(seed.speech_seconds >= LIVE_ASK_MIN_SPEECH_SECONDS);

        let snap = st.snapshot();
        let v = &snap.tracks[0].voices;
        assert_eq!(
            v[0].name.as_deref(),
            Some("John"),
            "his earlier lines relabel too"
        );
        assert_eq!(v[1].name.as_deref(), Some("John"));
        assert_eq!(v[0].hint.as_deref(), Some(LIVE_NAMED_HINT));
        assert_ne!(
            v[2].name.as_deref(),
            Some("John"),
            "another voice keeps its own label"
        );
        let json = serde_json::to_value(&snap).unwrap();
        assert_eq!(json["tracks"][0]["voices"][0]["cluster"], 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn yes_john_on_a_62_percent_match_adds_a_sample_and_leaves_one_speaker() {
        let mut r = LiveRoster::new(Vec::new(), MatchBands::provisional(), true);
        let none = VoiceMatch::Unknown { best: None };
        let (john, ask) = r.observe(&[1.0, 0.0, 0.0], 6.0, &none);
        assert!(matches!(ask, Some(LiveAsk::New { .. })));
        r.name(john, "John").unwrap();
        let (phone, ask) = r.observe(&johns_phone(), 6.0, &none);
        assert_ne!(phone, john);
        let Some(LiveAsk::SamePerson {
            name, similarity, ..
        }) = ask
        else {
            panic!("under 70% like John asks once: {ask:?}");
        };
        assert_eq!(name, "John");
        assert_eq!((similarity * 100.0).round() as i32, 62);
        assert_eq!(r.speakers(), 2);

        assert_eq!(
            r.confirm_same(phone, true).unwrap(),
            SameOutcome::MergedInto { cluster: john }
        );
        assert_eq!(
            r.clusters[john].n, 2,
            "the phone sound is a sample of John now"
        );
        assert_eq!(r.speakers(), 1);
        assert_eq!(r.display_name(phone).as_deref(), Some("John"));
        // Asked once: the same voice never raises another question.
        assert_eq!(r.observe(&johns_phone(), 6.0, &none).1, None);
    }

    #[test]
    fn no_someone_new_on_a_62_percent_match_yields_two_speakers() {
        let mut r = LiveRoster::new(Vec::new(), MatchBands::provisional(), true);
        let none = VoiceMatch::Unknown { best: None };
        let (john, _) = r.observe(&[1.0, 0.0, 0.0], 6.0, &none);
        r.name(john, "John").unwrap();
        let (phone, _) = r.observe(&johns_phone(), 6.0, &none);
        assert_eq!(
            r.confirm_same(phone, false).unwrap(),
            SameOutcome::NewSpeaker
        );
        assert_eq!(r.speakers(), 2);
        assert_eq!(r.clusters[john].n, 1);
        assert_eq!(r.display_name(phone), None);
    }

    #[test]
    fn yes_on_an_enrolled_voiceprint_adds_a_sample_to_that_profile() {
        let john = profile(7, "John", vec![1.0, 0.0, 0.0]);
        let mut r = LiveRoster::new(vec![john], MatchBands::provisional(), true);
        let m = match_voice(&johns_phone(), DIGEST, &r.profiles, r.bands);
        assert_eq!(m.outcome(), "ask");
        let (c, ask) = r.observe(&johns_phone(), 6.0, &m);
        assert!(matches!(ask, Some(LiveAsk::SamePerson { .. })));
        let out = r.confirm_same(c, true).unwrap();
        assert!(matches!(
            out,
            SameOutcome::AddedToProfile { profile_id: 7, .. }
        ));
        assert_eq!(r.profiles[0].n_samples, 4);
        assert_eq!(r.speakers(), 1);
        assert_eq!(r.display_name(c).as_deref(), Some("John"));
    }

    /// fix-l3b1-2 fixture: a named John cluster, an enrolled Maya profile and
    /// a new voice asked "Same person as John? 62% sure", then a later window
    /// of that voice that is 66% like Maya. Returns (roster, john, phone).
    fn asked_about_john_then_closer_to_maya() -> (LiveRoster, usize, usize) {
        let maya = profile(9, "Maya", vec![0.0, 0.0, 1.0]);
        let mut r = LiveRoster::new(vec![maya], MatchBands::provisional(), true);
        let none = VoiceMatch::Unknown { best: None };
        let (john, _) = r.observe(&[1.0, 0.0, 0.0], 6.0, &none);
        r.name(john, "John").unwrap();
        let (phone, ask) = r.observe(&johns_phone(), 6.0, &none);
        let Some(LiveAsk::SamePerson {
            name, similarity, ..
        }) = ask
        else {
            panic!("62% like John asks about John: {ask:?}");
        };
        assert_eq!(name, "John");
        assert_eq!((similarity * 100.0).round() as i32, 62);
        let maya_66 = VoiceMatch::Ask {
            profile_id: 9,
            display_name: "Maya".into(),
            similarity: crate::diarize_metrics::cosine_similarity(
                &[0.66, (1.0f32 - 0.66 * 0.66).sqrt()],
                &[1.0, 0.0],
            ),
        };
        let (again, ask) = r.observe(&johns_phone(), 6.0, &maya_66);
        assert_eq!(again, phone, "the later window is the same live voice");
        assert_eq!(ask, None, "asked once: no second question");
        (r, john, phone)
    }

    #[test]
    fn a_closer_window_after_asking_never_changes_who_the_card_named() {
        let (r, john, phone) = asked_about_john_then_closer_to_maya();
        let Some((target, name, similarity)) = r.clusters[phone].candidate.clone() else {
            panic!("the asked voice keeps the person the card named");
        };
        assert_eq!(target, SameTarget::Cluster(john));
        assert_eq!(name, "John");
        assert_eq!((similarity * 100.0).round() as i32, 62);
    }

    #[test]
    fn yes_john_merges_into_john_and_leaves_mayas_voiceprint_untouched() {
        let (mut r, john, phone) = asked_about_john_then_closer_to_maya();
        assert_eq!(
            r.confirm_same(phone, true).unwrap(),
            SameOutcome::MergedInto { cluster: john }
        );
        assert_eq!(r.profiles[0].display_name, "Maya");
        assert_eq!(
            r.profiles[0].n_samples, 3,
            "Maya's voiceprint never gets John's sample"
        );
        assert_eq!(r.clusters[phone].profile_id, None);
        assert_eq!(r.clusters[phone].merged_into, Some(john));
        assert_eq!(r.display_name(phone).as_deref(), Some("John"));
        assert_eq!(r.speakers(), 1);
    }

    #[test]
    fn a_voice_is_asked_about_after_five_seconds_and_only_once() {
        let mut r = LiveRoster::new(Vec::new(), MatchBands::provisional(), true);
        let none = VoiceMatch::Unknown { best: None };
        assert_eq!(r.observe(&[0.0, 1.0, 0.0], 3.0, &none).1, None);
        assert!(r.observe(&[0.0, 1.0, 0.0], 3.0, &none).1.is_some());
        r.skip(0).unwrap();
        assert_eq!(r.observe(&[0.0, 1.0, 0.0], 9.0, &none).1, None);
        // A voiceprint that claims the voice outright is never asked about.
        let alex = VoiceMatch::Auto {
            profile_id: 1,
            display_name: "Alex".into(),
            similarity: crate::diarize_metrics::cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]),
        };
        assert_eq!(r.observe(&[1.0, 0.0, 0.0], 12.0, &alex).1, None);
    }

    #[test]
    fn with_the_setting_off_a_full_fixture_meeting_raises_no_live_question() {
        let dir = std::env::temp_dir().join(format!("yap-live-off-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = spill(&dir, "w.t0.spill.pcm", 20, 3000);
        let b = spill(&dir, "w.t1.spill.pcm", 20, 3000);
        let profiles = alex_and_sam();
        let embedder = Fixture::new(
            (0..20)
                .map(|i| {
                    Some(if i % 3 == 0 {
                        vec![0.0, 1.0, 0.0]
                    } else {
                        johns_phone()
                    })
                })
                .collect(),
        );
        let sp = LiveSpeakers {
            embedder: &embedder,
            profiles: &profiles,
            digest: DIGEST,
            bands: MatchBands::provisional(),
        };
        // The default: the roster is attached but asking is off.
        let mut st = LiveState::new(vec![a.clone(), b.clone()]).with_roster(LiveRoster::new(
            profiles.clone(),
            MatchBands::provisional(),
            false,
        ));
        let dec = Echo(Cell::new(0));
        let mut asks = 0;
        for _ in 0..8 {
            st.tick_with_speakers(&dec, Some(&sp), &|| false).unwrap();
            asks += st.take_asks().len();
            grow(&a, 10, 3000);
            grow(&b, 10, 3000);
        }
        assert_eq!(asks, 0, "setting off: zero live cards");
        assert!(st
            .snapshot()
            .tracks
            .iter()
            .flat_map(|t| t.voices.iter())
            .all(|v| v.cluster.is_none()));
        assert!(st.roster().unwrap().clusters.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
