//! yap-audit-vad-boundaries — the SHIPPED meeting pipeline cuts at pauses.
//!
//! Audit issue #3: `run_meeting_job` built `MeetingAsr` with `vad: None`, so
//! `plan_windows` always took the fixed-clock planner and every chunk in every
//! `*.asr-progress.json` was `fixed_clock`/`edge` — never `silence`. These tests
//! drive the production entry the worker calls (`run_meeting_job`, with a
//! `PipelineDeps` shaped exactly like lib.rs builds it) over a synthetic WAV with
//! pauses between 20 s and 40 s, then read the resume ledger the pipeline wrote.
//!
//! The VAD here is a stub behind the shipped `VoiceActivity` seam (an energy
//! gate over the real samples, so "the cut is inside a pause" is checked against
//! the audio, not against the stub's own script). The Silero model is not
//! available offline and is not what is under test: the wiring is.

mod support;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use support::{open_db, temp_dir};
use wilson_voice_lib::asr_engine::{TimedKind, TimedSpan, TimedTranscript};
use wilson_voice_lib::meeting_asr::{
    BoundaryKind, ChunkAsr, ChunkConfig, JsonProgressStore, MeetingAsrConfig, NoDemand,
    ProgressStore, VoiceActivity,
};
use wilson_voice_lib::meeting_pipeline::{
    ledger_key, run_meeting_job, MeetingEngine, MeetingProgressEvent, PipelineDeps, MIC_TRACK,
};
use wilson_voice_lib::meetings::MeetingState;
use wilson_voice_lib::vad::VoicedSpan;

const RATE: u32 = 16_000;
const SECONDS: f64 = 75.0;
/// Pauses (start, end) in the synthetic meeting. The first sits in the 25-35 s
/// search region of the first cut (target 30 s); the second in the region of the
/// second cut (~29 s + 25..35 s).
const PAUSES: [(f64, f64); 2] = [(28.0, 29.5), (58.0, 59.5)];

fn in_pause(t: f64) -> bool {
    PAUSES.iter().any(|(a, b)| t >= *a && t < *b)
}

fn write_meeting_wav(path: &Path) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("create wav");
    for i in 0..(SECONDS * RATE as f64) as usize {
        let t = i as f64 / RATE as f64;
        let v = if in_pause(t) {
            0.0
        } else {
            (t as f32 * 220.0 * std::f32::consts::TAU).sin() * 0.3
        };
        w.write_sample((v * i16::MAX as f32) as i16).unwrap();
    }
    w.finalize().unwrap();
}

/// An energy gate over 20 ms frames, reporting spans relative to the buffer.
struct EnergyVad;

impl VoiceActivity for EnergyVad {
    fn voiced_spans(&self, samples: &[f32]) -> Result<Vec<VoicedSpan>, String> {
        let frame = (RATE as usize) / 50;
        let mut spans: Vec<VoicedSpan> = Vec::new();
        for (k, chunk) in samples.chunks(frame).enumerate() {
            let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
            if rms < 0.01 {
                continue;
            }
            let start = (k * frame) as f64 / RATE as f64;
            let end = (k * frame + chunk.len()) as f64 / RATE as f64;
            match spans.last_mut() {
                Some(last) if (start - last.end_seconds).abs() < 1e-9 => last.end_seconds = end,
                _ => spans.push(VoicedSpan {
                    start_seconds: start,
                    end_seconds: end,
                }),
            }
        }
        Ok(spans)
    }
}

/// A VAD whose session is broken: every call errors.
struct BrokenVad;

impl VoiceActivity for BrokenVad {
    fn voiced_spans(&self, _samples: &[f32]) -> Result<Vec<VoicedSpan>, String> {
        Err("onnx session exploded".to_string())
    }
}

struct StubAsr;

impl ChunkAsr for StubAsr {
    fn transcribe_timed(&self, samples: &[f32]) -> Result<TimedTranscript, String> {
        let len = samples.len() as f64 / RATE as f64;
        let mut words = Vec::new();
        let mut t = 0.05;
        let mut k = 0;
        while t + 0.3 < len {
            words.push(TimedSpan {
                start_seconds: t,
                end_seconds: t + 0.3,
                text: format!("w{k}"),
            });
            t += 0.5;
            k += 1;
        }
        let text = words
            .iter()
            .map(|w| w.text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        Ok(TimedTranscript {
            text,
            kind: TimedKind::Word,
            segments: Vec::new(),
            words,
        })
    }
}

fn deps(
    db: Arc<wilson_voice_lib::db::Database>,
    dir: &Path,
    vad: wilson_voice_lib::meeting_pipeline::VadFactory,
) -> PipelineDeps {
    PipelineDeps {
        db,
        progress_dir: dir.to_path_buf(),
        vad,
        asr: Arc::new(|| Ok(MeetingEngine::new("stub-asr", Box::new(StubAsr)))),
        demand: Arc::new(NoDemand),
        quit: Arc::new(|| false),
        sink: Arc::new(|_: &MeetingProgressEvent| {}),
        after_transcript: None,
        // The SHIPPED chunk geometry (30 s target, 25-35 s search, 2 s overlap).
        config: MeetingAsrConfig {
            chunk: ChunkConfig::default(),
            yield_poll: Duration::from_millis(1),
            ..MeetingAsrConfig::default()
        },
        start_delay: Duration::ZERO,
    }
}

/// Run one meeting through `run_meeting_job` and return its ledger chunks.
fn run(
    vad: wilson_voice_lib::meeting_pipeline::VadFactory,
    name: &str,
) -> Vec<(f64, BoundaryKind)> {
    let dir = temp_dir(name);
    let db = Arc::new(open_db(&dir));
    let wav = dir.join("m.t0.wav");
    write_meeting_wav(&wav);
    let m = db.create_meeting(name, "manual").unwrap();
    db.finish_meeting(&m.id, SECONDS, Some(&wav)).unwrap();
    db.set_meeting_state(&m.id, MeetingState::Transcribing, None)
        .unwrap();

    let state = run_meeting_job(&deps(Arc::clone(&db), &dir, vad), &m.id).expect("job");
    assert_eq!(state, MeetingState::Complete, "the meeting completes");
    assert!(
        !db.list_meeting_segments(&m.id).unwrap().is_empty(),
        "segments were written"
    );

    let ledger = JsonProgressStore::new(&dir)
        .load(&ledger_key(&m.id, MIC_TRACK))
        .expect("ledger");
    // (where the chunk's content ends, the kind of cut it ends at)
    ledger
        .chunks
        .iter()
        .map(|c| (c.content_end_seconds, c.end_boundary))
        .collect()
}

#[test]
fn the_shipped_pipeline_cuts_meeting_chunks_inside_pauses() {
    let ends = run(
        Arc::new(|| Ok(Arc::new(EnergyVad) as Arc<dyn VoiceActivity + Send + Sync>)),
        "vad-boundaries-silence",
    );
    println!("chunk ends (seconds, kind): {ends:?}");
    let silence: Vec<f64> = ends
        .iter()
        .filter(|(_, k)| *k == BoundaryKind::Silence)
        .map(|(t, _)| *t)
        .collect();
    assert!(
        !silence.is_empty(),
        "at least one boundary is the silence kind: {ends:?}"
    );
    for cut in &silence {
        assert!(
            PAUSES.iter().any(|(a, b)| cut >= a && cut <= b),
            "silence cut at {cut:.3}s is not inside a pause {PAUSES:?}"
        );
    }
    // Both pauses were reachable, so both interior cuts are silence cuts.
    assert_eq!(
        silence.len(),
        2,
        "both interior cuts are silence cuts: {ends:?}"
    );
    assert_eq!(
        ends.last().map(|(t, k)| ((*t - SECONDS).abs() < 1e-6, *k)),
        Some((true, BoundaryKind::Edge)),
        "the last chunk ends at the end of the audio"
    );
}

#[test]
fn a_vad_that_errors_falls_back_to_the_fixed_clock_and_the_meeting_completes() {
    let ends = run(
        Arc::new(|| Ok(Arc::new(BrokenVad) as Arc<dyn VoiceActivity + Send + Sync>)),
        "vad-boundaries-broken",
    );
    println!("chunk ends (seconds, kind): {ends:?}");
    let interior: Vec<&(f64, BoundaryKind)> = ends
        .iter()
        .filter(|(_, k)| *k != BoundaryKind::Edge)
        .collect();
    assert!(!interior.is_empty(), "there are interior cuts: {ends:?}");
    assert!(
        interior.iter().all(|(_, k)| *k == BoundaryKind::FixedClock),
        "every interior cut is fixed_clock: {ends:?}"
    );
}

#[test]
fn a_vad_that_cannot_load_falls_back_to_the_fixed_clock_and_the_meeting_completes() {
    let ends = run(
        Arc::new(|| Err("the Silero VAD is not loaded".to_string())),
        "vad-boundaries-unloaded",
    );
    println!("chunk ends (seconds, kind): {ends:?}");
    assert!(
        ends.iter()
            .filter(|(_, k)| *k != BoundaryKind::Edge)
            .all(|(_, k)| *k == BoundaryKind::FixedClock),
        "no silence cut without a VAD: {ends:?}"
    );
}
