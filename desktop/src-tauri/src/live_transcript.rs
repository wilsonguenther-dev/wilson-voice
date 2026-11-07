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

use crate::meetings::{speaker_label, DiarizationTarget};
use crate::speaker_profiles::{
    match_voice, suggestion_text, MatchBands, SpeakerProfile, VoiceMatch,
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
        },
        VoiceMatch::Unknown { best: Some(_) } if !(tracks >= 2 && track == 0) => LiveVoice {
            name: Some(SOMEONE_NEW.to_string()),
            hint: None,
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
        if self.profiles.is_empty() || dictation_wants_engine() {
            return LiveVoice::default();
        }
        let speech = speech_seconds(samples);
        if speech < LIVE_SPEAKER_MIN_SPEECH_SECONDS {
            log::debug!("live_speaker skip reason=short_speech track={track} speech_s={speech:.1}");
            return LiveVoice::default();
        }
        let t0 = Instant::now();
        match self.embedder.embed(samples) {
            Ok(Some(embedding)) => {
                let m = match_voice(&embedding, self.digest, self.profiles, self.bands);
                log::info!(
                    "live_speaker matched={} track={track} ms={}",
                    m.outcome(),
                    t0.elapsed().as_millis()
                );
                live_voice_label(&m, track, tracks)
            }
            Ok(None) => LiveVoice::default(),
            Err(e) => {
                log::warn!("live_speaker failure track={track} reason={e}");
                LiveVoice::default()
            }
        }
    }
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
        Self { spills, cursors }
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
                        speakers
                            .map(|sp| sp.label(track, tracks, &samples, dictation_wants_engine))
                            .unwrap_or_default()
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
            let speakers = match digest.as_deref() {
                Some(d) if !profiles.is_empty() => Some(LiveSpeakers {
                    embedder: &embedder,
                    profiles: &profiles,
                    digest: d,
                    bands: MatchBands::from_env_or_provisional(),
                }),
                _ => None,
            };
            log::info!(
                "live_speaker start enrolled={} enabled={}",
                profiles.len(),
                speakers.is_some()
            );
            let mut state = LiveState::new(spills);
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
                }
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
}
