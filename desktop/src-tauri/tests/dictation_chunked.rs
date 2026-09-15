//! Y3-B — the dictation path decodes long takes in WINDOWS.
//!
//! What this file is defending, in one sentence: before Y3-B a take was one
//! `TranscriptionManager::transcribe` call under a 120 s `TRANSCRIBE_TIMEOUT`,
//! so a take long enough to trip that wall lost EVERYTHING — there was no
//! partial result to keep, and the wall did not move when the take got longer.
//!
//! The four properties below are the whole item:
//!
//! 1. a take short enough to fit in one window takes the byte-identical
//!    single-call path it always took (so no formatting fixture and no YV66
//!    gate corpus shifts under us),
//! 2. a long take's seams neither duplicate nor drop a word,
//! 3. a failed window in the MIDDLE costs that window, not the take, and
//! 4. the timeout is spent PER WINDOW, so a twelve-window take is not subject
//!    to a 120 s total.
//!
//! Plus the structural one the item asks for by name: there is exactly ONE
//! chunker, and dictation and meetings resolve to the same function — asserted
//! by address and by a source sweep, not by a comment.
//!
//! The stub engine is the meeting suite's `RampDecoder` (`tests/support/`),
//! whose fixture audio carries its own clock: sample `i` holds `i / 16000`, so
//! a decoder handed a bare `&[f32]` can say exactly which second of the take it
//! was given, and a merge bug cannot hide behind a call count.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wilson_voice_lib::asr_engine::TimedTranscript;
use wilson_voice_lib::meeting_asr::{self, ChunkConfig, MEETING_RATE};
use wilson_voice_lib::transcription::{
    dictation_window_planner, DictationChunking, Transcriber, TranscriptionManager, WindowPlanner,
    DICTATION_CHUNK_SAMPLE_RATE, DICTATION_CHUNK_THRESHOLD_SECONDS,
};

#[path = "support/meeting.rs"]
mod support;
use support::{expected_words, window_bounds, word_at, RampDecoder};

const IDLE: Duration = Duration::from_secs(600);
const IDLE_CHECK: Duration = Duration::from_secs(600);

/// Which entry point the manager used for one decode. The byte-identical test
/// turns on this: the short path must still be the PLAIN `transcribe`, not a
/// one-window timed decode that merely produces the same string.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Call {
    Plain,
    Timed,
}

#[derive(Default)]
struct Seen {
    calls: Mutex<Vec<(Call, f64, f64)>>,
}

impl Seen {
    fn record(&self, kind: Call, bounds: (f64, f64)) {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((kind, bounds.0, bounds.1));
    }
    fn snapshot(&self) -> Vec<(Call, f64, f64)> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// A `Transcriber` that hears the ramp. `fail_at` is the 0-based DECODE index
/// (not the window index) that returns an error, so a test can kill exactly one
/// window in the middle of a take.
struct RampEngine {
    decoder: RampDecoder,
    seen: Arc<Seen>,
    decodes: Arc<AtomicUsize>,
    fail_at: Option<usize>,
    per_chunk_delay: Duration,
}

impl Transcriber for RampEngine {
    fn transcribe(
        &mut self,
        samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<String, String> {
        let bounds = window_bounds(samples);
        self.seen.record(Call::Plain, bounds);
        self.decodes.fetch_add(1, Ordering::AcqRel);
        Ok(self.decoder.transcript_for(bounds.0, bounds.1).text)
    }

    fn transcribe_timed(
        &mut self,
        samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<TimedTranscript, String> {
        let bounds = window_bounds(samples);
        self.seen.record(Call::Timed, bounds);
        let n = self.decodes.fetch_add(1, Ordering::AcqRel);
        if !self.per_chunk_delay.is_zero() {
            std::thread::sleep(self.per_chunk_delay);
        }
        if Some(n) == self.fail_at {
            return Err("stub engine refused this window".to_string());
        }
        Ok(self.decoder.transcript_for(bounds.0, bounds.1))
    }
}

struct Rig {
    manager: TranscriptionManager,
    seen: Arc<Seen>,
    decodes: Arc<AtomicUsize>,
}

fn rig(timed: bool, timeout: Duration, fail_at: Option<usize>, delay: Duration) -> Rig {
    let seen = Arc::new(Seen::default());
    let decodes = Arc::new(AtomicUsize::new(0));
    let loader_seen = seen.clone();
    let loader_decodes = decodes.clone();
    let manager = TranscriptionManager::with_loader(
        Arc::new(move |_p: &Path| {
            Ok(Box::new(RampEngine {
                decoder: RampDecoder::new(timed),
                seen: loader_seen.clone(),
                decodes: loader_decodes.clone(),
                fail_at,
                per_chunk_delay: delay,
            }) as Box<dyn Transcriber>)
        }),
        IDLE,
        IDLE_CHECK,
        timeout,
    );
    // `load` checks the file EXISTS before it calls the loader, so the stub
    // needs a real (empty) path to point at.
    let stub_path = std::env::temp_dir().join("yap-y3b-stub-model.gguf");
    std::fs::write(&stub_path, b"stub").expect("stub model file");
    manager.load("stub", &stub_path).expect("stub load");
    Rig {
        manager,
        seen,
        decodes,
    }
}

/// A take whose samples ARE its clock — the same ramp the meeting fixtures use,
/// as a plain `Vec<f32>` because dictation holds its take in memory.
fn ramp_take(seconds: f64) -> Vec<f32> {
    let n = (seconds * MEETING_RATE as f64) as usize;
    (0..n).map(|i| i as f32 / MEETING_RATE as f32).collect()
}

/// Twelve windows of five seconds each — the "chunk 9 of 12" shape the item
/// names, small enough that a test is milliseconds rather than minutes.
fn twelve_window_plan() -> DictationChunking {
    DictationChunking {
        threshold_seconds: 10.0,
        config: ChunkConfig {
            target_seconds: 5.0,
            min_seconds: 4.0,
            max_seconds: 6.0,
            overlap_seconds: 1.0,
            min_silence_seconds: 0.2,
        },
        sample_rate: DICTATION_CHUNK_SAMPLE_RATE,
    }
}

/// The 16-bit mono fixture WAV as f32, hand-parsed so this test depends on no
/// decoder it is not testing.
fn fixture_wav() -> Vec<f32> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/quick-brown-fox-16k.wav");
    let bytes = std::fs::read(&path).expect("fixture wav");
    let mut i = 12; // past "RIFF" + size + "WAVE"
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let size =
            u32::from_le_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]) as usize;
        let body = i + 8;
        if id == b"data" {
            let end = (body + size).min(bytes.len());
            return bytes[body..end]
                .chunks_exact(2)
                .map(|p| i16::from_le_bytes([p[0], p[1]]) as f32 / 32768.0)
                .collect();
        }
        i = body + size + (size % 2);
    }
    panic!("no data chunk in the fixture wav");
}

// ---------------------------------------------------------------------------

#[test]
fn single_window_take_is_byte_identical_to_the_unchunked_path() {
    let samples = fixture_wav();
    assert!(!samples.is_empty(), "fixture wav decoded to no samples");
    let seconds = samples.len() as f64 / DICTATION_CHUNK_SAMPLE_RATE as f64;
    assert!(
        seconds < DICTATION_CHUNK_THRESHOLD_SECONDS,
        "the fixture ({seconds:.2}s) must sit under the {DICTATION_CHUNK_THRESHOLD_SECONDS:.0}s \
         threshold or this test proves nothing"
    );

    let rig = rig(true, Duration::from_secs(5), None, Duration::ZERO);
    let direct = rig
        .manager
        .transcribe(samples.clone(), None, None)
        .expect("unchunked decode");
    let via_take = rig
        .manager
        .transcribe_take(samples.clone(), None, None)
        .expect("take decode");

    // Same bytes out.
    assert_eq!(
        via_take.text, direct,
        "the short path must not change a byte"
    );
    assert!(!via_take.windowed, "a short take must not be windowed");
    assert_eq!(via_take.chunks_total, 1);
    assert_eq!(via_take.chunks_failed, 0);
    assert!(!via_take.degraded);
    assert!(via_take.degraded_reason.is_none());

    // Same PATH in: two plain decodes over the identical buffer, no timed
    // decode and no second window anywhere.
    let calls = rig.seen.snapshot();
    assert_eq!(calls.len(), 2, "one decode each, got {calls:?}");
    assert_eq!(calls[0].0, Call::Plain);
    assert_eq!(
        calls[0], calls[1],
        "the take path must hand the engine the exact same buffer as the direct call"
    );
}

#[test]
fn seams_do_not_duplicate_or_drop_words() {
    // The SHIPPED geometry (30 s windows, 2 s overlap) on a 90 s take, with the
    // threshold lowered so the windowing actually runs: three windows, two
    // seams, one word per second, all of them unique.
    let seconds = 90usize;
    let plan = DictationChunking {
        threshold_seconds: 10.0,
        ..DictationChunking::default()
    };
    let rig = rig(true, Duration::from_secs(5), None, Duration::ZERO);
    let take = rig
        .manager
        .transcribe_take_with(ramp_take(seconds as f64), None, None, &plan)
        .expect("windowed decode");

    assert!(take.windowed);
    assert!(take.chunks_total > 1, "the take must actually have split");
    assert_eq!(take.chunks_failed, 0);
    assert!(!take.degraded, "reason: {:?}", take.degraded_reason);
    // Exact text: a duplicate at a seam or a word eaten by one shows up here.
    assert_eq!(take.text, expected_words(seconds).join(" "));
}

#[test]
fn a_failed_middle_chunk_keeps_the_earlier_chunks() {
    // Chunk 9 of 12 dies. The item's whole point: that costs chunk 9, not the
    // eleven minutes in front of it.
    let plan = twelve_window_plan();
    let rig = rig(true, Duration::from_secs(5), Some(8), Duration::ZERO);
    let take = rig
        .manager
        .transcribe_take_with(ramp_take(60.0), None, None, &plan)
        .expect("a partly failed take still returns");

    assert_eq!(take.chunks_total, 12, "expected the 12-window shape");
    assert_eq!(take.chunks_failed, 1);
    assert!(take.degraded, "a lost window must mark the take degraded");
    assert!(take.degraded_reason.is_some());

    let words: Vec<&str> = take.text.split_whitespace().collect();
    assert!(!words.is_empty(), "the take must not come back empty");
    // Everything before the failed window survived, in order.
    for second in 0..40usize {
        assert!(
            words.contains(&word_at(second).as_str()),
            "word {second} from before the failure was lost"
        );
    }
    // And the take did not stop at the failure: windows after it are here too.
    assert!(
        words.contains(&word_at(55).as_str()),
        "the windows AFTER the failed one were dropped"
    );
    // The failed window's own content is the only hole.
    assert!(
        !words.contains(&word_at(42).as_str()),
        "the failed window's audio cannot have produced text"
    );
}

#[test]
fn per_chunk_deadline_not_per_take() {
    // The deadline is spent per WINDOW. Twelve windows at 200 ms each is 2.4 s
    // of decoding against a 300 ms budget: under the old per-take wall this
    // take was dead, and under the new one every window is comfortably inside
    // its own budget.
    let per_chunk = Duration::from_millis(300);
    let delay = Duration::from_millis(200);
    let plan = twelve_window_plan();
    let rig = rig(true, per_chunk, None, delay);

    let started = Instant::now();
    let take = rig
        .manager
        .transcribe_take_with(ramp_take(60.0), None, None, &plan)
        .expect("a 12-window take must not be subject to a per-take wall");
    let elapsed = started.elapsed();

    assert_eq!(take.chunks_total, 12);
    assert_eq!(take.chunks_failed, 0, "reason: {:?}", take.degraded_reason);
    assert!(!take.degraded);
    assert_eq!(rig.decodes.load(Ordering::Acquire), 12);
    assert!(
        elapsed > per_chunk,
        "the take ran {elapsed:?}, which is not longer than the per-chunk budget \
         {per_chunk:?} — this test is not proving anything"
    );
    assert!(
        elapsed >= delay * 12,
        "every window must actually have decoded ({elapsed:?} < {:?})",
        delay * 12
    );
}

#[test]
fn chunker_has_exactly_one_implementation() {
    // (1) By address: the function dictation plans with IS the one the meeting
    // path falls back to, not a copy that happens to agree today.
    let dictation: WindowPlanner = dictation_window_planner();
    let meeting: WindowPlanner = meeting_asr::plan_windows_fixed;
    assert_eq!(
        dictation as usize, meeting as usize,
        "dictation and meetings must resolve to the same planner"
    );

    // (2) By source sweep: nothing else in the crate defines a window planner,
    // a window-from-boundaries walk, or a seam merge. The pattern is the `fn`
    // DEFINITION, and the scope is every module in src/ — a second chunker
    // living in, say, `lib.rs` would be caught by this and not by (1).
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut defs: Vec<(String, String)> = Vec::new();
    let mut transcription = String::new();
    let mut walk = vec![src.clone()];
    while let Some(dir) = walk.pop() {
        for entry in std::fs::read_dir(&dir).expect("read src") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                walk.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let body = std::fs::read_to_string(&path).expect("read rs");
            let name = path
                .strip_prefix(&src)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            for needle in [
                "fn plan_windows_fixed(",
                "fn windows_from_boundaries(",
                "fn merge_timed_reporting(",
            ] {
                if body.contains(needle) {
                    defs.push((needle.to_string(), name.clone()));
                }
            }
            if name == "transcription.rs" {
                transcription = body;
            }
        }
    }
    // Prove the pattern can match the shape we fear before trusting its absence.
    assert!(
        defs.iter()
            .any(|(n, f)| n == "fn plan_windows_fixed(" && f == "meeting_asr.rs"),
        "the sweep pattern found nothing at all — it is not testing what it claims"
    );
    for needle in [
        "fn plan_windows_fixed(",
        "fn windows_from_boundaries(",
        "fn merge_timed_reporting(",
    ] {
        let files: Vec<&str> = defs
            .iter()
            .filter(|(n, _)| n == needle)
            .map(|(_, f)| f.as_str())
            .collect();
        assert_eq!(
            files,
            vec!["meeting_asr.rs"],
            "`{needle}` must be defined exactly once, in meeting_asr.rs; found {files:?}"
        );
    }

    // (3) And the dictation path CALLS it rather than open-coding the geometry
    // or the merge.
    assert!(!transcription.is_empty(), "transcription.rs was not swept");
    for call in [
        "meeting_asr::plan_windows_fixed",
        "meeting_asr::ChunkOutcome::from_transcript",
        "meeting_asr::ChunkOutcome::failed",
        "meeting_asr::assemble",
    ] {
        assert!(
            transcription.contains(call),
            "the dictation path must call `{call}` instead of reimplementing it"
        );
    }
}
