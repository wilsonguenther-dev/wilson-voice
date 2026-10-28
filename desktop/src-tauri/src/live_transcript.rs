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

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::meetings::{speaker_label, DiarizationTarget};

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

/// One track's lines as the view draws them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTrack {
    pub track: usize,
    /// "You" / "Call" on a two-track meeting; `None` on a mic-only one, which
    /// shows its single track with no speaker label at all.
    pub speaker: Option<String>,
    pub lines: Vec<String>,
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
            })
            .collect();
        Self { spills, cursors }
    }

    pub fn spills(&self) -> &[PathBuf] {
        &self.spills
    }

    fn push_line(&mut self, track: usize, text: &str) {
        let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            return;
        }
        let lines = &mut self.cursors[track].lines;
        lines.push_back(line);
        while lines.len() > LIVE_LINES_PER_TRACK {
            lines.pop_front();
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
        let mut decoded = 0;
        for track in 0..self.spills.len() {
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
                    if !text.trim().is_empty() {
                        decoded += 1;
                    }
                    self.push_line(track, &text);
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
            let mut state = LiveState::new(spills);
            let decoder = WarmEngineLive {
                manager: manager.clone(),
            };
            let mut ticks = 0u64;
            let mut failed = 0u64;
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
                let demand = || {
                    use crate::meeting_asr::EngineDemand;
                    manager.dictation_pending()
                };
                let t0 = Instant::now();
                match state.tick(&decoder, &demand) {
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
}
