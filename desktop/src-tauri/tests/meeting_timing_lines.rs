//! yap-audit-meeting-timings — the lines that explain a slow meeting.
//!
//! Audit issues #10 and #16: a 31-minute meeting took 4:01 to transcribe
//! (RTF 0.13) where a 24-minute one took 0:28 (RTF 0.019), and the log could not
//! say why; a notes run logged one line per rejection and no phase timings; and
//! a mid-meeting input retune logged "0 samples captured so far" — the
//! dictation take's counter, not the meeting's.
//!
//! Everything here is pure: the formatters are called on values, and the two
//! behavioural checks drive the shipped driver / capture with stubs. Nothing
//! scrapes a log, and no real meeting data is read.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::Duration;

use wilson_voice_lib::asr_engine::{TimedKind, TimedTranscript};
use wilson_voice_lib::meeting::{
    input_format_change_line, InputRetune, MeetingCapture, RetuneCapture,
};
use wilson_voice_lib::meeting_asr::{
    never_quit, BoundaryKind, ChunkAsr, ChunkOutcome, ChunkStatus, ChunkTiming, MeetingAsr,
    MeetingAsrConfig, MeetingProgress, MemoryWindows, NoDemand, ProgressStore,
};
use wilson_voice_lib::meeting_energy::{BatteryReading, MeetingDiagnostics, ThermalState};
use wilson_voice_lib::meeting_pipeline::{
    chunk_timing_line, real_time_factor, track_rtf_line, TrackTiming,
};
use wilson_voice_lib::summarize::{summary_run_line, SummaryRunTimings};

// ── 1. the per-track RTF line ───────────────────────────────────────────────

#[test]
fn the_rtf_line_has_the_audited_shape_and_numbers() {
    // The audit's slow meeting: 31 min of audio, 4:01 of wall.
    let t = TrackTiming {
        track: 1,
        audio_seconds: 1860.0,
        wall_seconds: 241.0,
        decode_seconds: 236.4,
        chunks: 62,
        resumed: 0,
        preempted: 2,
        yields: 3,
        interrupted: false,
    };
    let line = track_rtf_line("m-31", &t);
    assert!(
        line.starts_with(
            "meeting rtf track=1 audio_s=1860.0 wall_s=241.0 rtf=0.130 chunks=62 preempted=2 "
        ),
        "{line}"
    );
    assert!(line.contains(" decode_s=236.4 "), "{line}");
    assert!(line.contains(" yields=3 "), "{line}");
    assert!(line.contains(" resumed=0 "), "{line}");
    assert!(line.ends_with(" meeting=m-31"), "{line}");
    assert!(!line.contains("interrupted"), "{line}");
}

#[test]
fn the_fast_meeting_reads_as_fast_and_an_interrupted_track_says_so() {
    // The audit's fast meeting: 24 min in 0:28 → 0.019.
    let fast = TrackTiming {
        track: 0,
        audio_seconds: 1440.0,
        wall_seconds: 28.0,
        chunks: 48,
        ..TrackTiming::default()
    };
    assert!(track_rtf_line("m-24", &fast).contains(" rtf=0.019 "));

    let cut = TrackTiming {
        interrupted: true,
        ..fast
    };
    assert!(track_rtf_line("m-24", &cut).ends_with(" interrupted=true"));
}

#[test]
fn an_rtf_with_no_audio_is_not_a_number() {
    assert_eq!(real_time_factor(10.0, 0.0), None);
    assert_eq!(real_time_factor(f64::NAN, 10.0), None);
    let r = real_time_factor(241.0, 1860.0).expect("a real ratio");
    assert!((r - 0.12957).abs() < 1e-4, "{r}");
    let empty = TrackTiming {
        track: 0,
        wall_seconds: 0.4,
        ..TrackTiming::default()
    };
    assert!(track_rtf_line("m", &empty).contains(" rtf=n/a "));
}

// ── 2. the per-chunk DEBUG line ─────────────────────────────────────────────

#[test]
fn the_chunk_line_names_track_index_audio_decode_and_boundary() {
    let line = chunk_timing_line(
        "m-31",
        1,
        &ChunkTiming {
            index: 17,
            audio_seconds: 32.04,
            decode_ms: 3875,
            boundary: BoundaryKind::FixedClock,
            status: ChunkStatus::Done,
        },
    );
    assert_eq!(
        line,
        "meeting chunk track=1 index=17 audio_s=32.0 decode_ms=3875 boundary=fixed_clock \
         status=done meeting=m-31"
    );
    let failed = chunk_timing_line(
        "m",
        0,
        &ChunkTiming {
            index: 0,
            audio_seconds: 30.0,
            decode_ms: 0,
            boundary: BoundaryKind::Silence,
            status: ChunkStatus::AsrFailed,
        },
    );
    assert!(
        failed.contains("boundary=silence status=asr_failed"),
        "{failed}"
    );
}

/// A decoder that takes a measurable time per window.
struct SlowStub;
const STUB_DECODE: Duration = Duration::from_millis(12);
impl ChunkAsr for SlowStub {
    fn transcribe_timed(&self, _samples: &[f32]) -> Result<TimedTranscript, String> {
        std::thread::sleep(STUB_DECODE);
        Ok(TimedTranscript {
            text: "words".to_string(),
            kind: TimedKind::None,
            segments: Vec::new(),
            words: Vec::new(),
        })
    }
}

/// A ledger that keeps everything in memory and records every timing it is told.
#[derive(Default)]
struct RecordingStore {
    progress: RefCell<MeetingProgress>,
    timings: Vec<(String, ChunkTiming)>,
    recorded: Vec<usize>,
}
impl ProgressStore for RecordingStore {
    fn load(&self, _key: &str) -> Result<MeetingProgress, String> {
        Ok(self.progress.borrow().clone())
    }
    fn record_chunk(&mut self, _key: &str, chunk: &ChunkOutcome) -> Result<(), String> {
        let mut p = self.progress.borrow_mut();
        p.processed_through_seconds = chunk.content_end_seconds;
        p.chunks.push(chunk.clone());
        self.recorded.push(chunk.index);
        Ok(())
    }
    fn record_timing(&mut self, key: &str, timing: &ChunkTiming) {
        self.timings.push((key.to_string(), *timing));
    }
}

#[test]
fn the_shipped_driver_reports_one_timing_per_recorded_window() {
    // 95 s of audio at the meeting rate → several 30 s windows.
    let audio = MemoryWindows::at_meeting_rate(vec![0.0f32; 16_000 * 95]);
    let mut store = RecordingStore::default();
    let never = never_quit();
    let transcript = MeetingAsr {
        audio: &audio,
        vad: None,
        asr: &SlowStub,
        demand: &NoDemand,
        store: &mut store,
        quit: &never,
        config: MeetingAsrConfig::default(),
    }
    .run("m-test.t0")
    .expect("run");

    assert!(
        transcript.chunks_decoded >= 3,
        "{}",
        transcript.chunks_decoded
    );
    // One timing per recorded chunk, in the same order, for the same key.
    let indices: Vec<usize> = store.timings.iter().map(|(_, t)| t.index).collect();
    assert_eq!(indices, store.recorded);
    for (key, t) in &store.timings {
        assert_eq!(key, "m-test.t0");
        assert!(
            t.decode_ms >= STUB_DECODE.as_millis() as u64,
            "decode_ms {} is not the decoder's time",
            t.decode_ms
        );
        assert!(t.audio_seconds > 25.0 && t.audio_seconds < 40.0, "{t:?}");
        assert_eq!(t.status, ChunkStatus::Done);
    }
    // The last window ends at the end of the audio, which is not a cut.
    assert_eq!(store.timings.last().unwrap().1.boundary, BoundaryKind::Edge);
    assert_ne!(
        store.timings.first().unwrap().1.boundary,
        BoundaryKind::Edge
    );
}

// ── 3. the notes-run line ───────────────────────────────────────────────────

#[test]
fn the_summary_run_line_counts_rejections_by_reason_in_one_line() {
    let mut rejections = BTreeMap::new();
    rejections.insert("v3_ungrounded", 5);
    rejections.insert("v2_runaway", 2);
    let line = summary_run_line(&SummaryRunTimings {
        model_load_ms: Some(812),
        map_passes: 4,
        map_failed: 1,
        map_ms: 31_200,
        reduce_passes: 1,
        reduce_failed: 0,
        reduce_ms: 6_100,
        rejections,
        outcome: "ok",
    });
    assert_eq!(
        line,
        "summary run: outcome=ok model_load_ms=812 map_passes=4 map_failed=1 map_ms=31200 \
         reduce_passes=1 reduce_failed=0 reduce_ms=6100 rejections=7 v2_runaway=2 v3_ungrounded=5"
    );
}

#[test]
fn a_run_with_no_load_and_no_rejections_says_so() {
    let line = summary_run_line(&SummaryRunTimings {
        outcome: "empty",
        ..SummaryRunTimings::default()
    });
    assert!(
        line.starts_with("summary run: outcome=empty model_load_ms=n/a "),
        "{line}"
    );
    assert!(line.ends_with(" rejections=0"), "{line}");
}

// ── 4. the retune line and the diagnostics blob ─────────────────────────────

#[test]
fn a_retune_during_a_meeting_reports_the_meetings_seconds() {
    let line = input_format_change_line(
        24_000,
        1,
        16_000,
        1,
        RetuneCapture::Meeting {
            seconds: 1803.25,
            dictation_samples: 0,
        },
    );
    assert_eq!(
        line,
        "YV92 input format changed mid-capture: 24000Hz/1ch → 16000Hz/1ch \
         (meeting capture: 1803.2 s captured so far)"
    );
    assert!(!line.contains("0 samples"), "{line}");

    let both = input_format_change_line(
        48_000,
        2,
        24_000,
        1,
        RetuneCapture::Meeting {
            seconds: 60.0,
            dictation_samples: 32_000,
        },
    );
    assert!(
        both.contains("meeting capture: 60.0 s captured so far"),
        "{both}"
    );
    assert!(both.contains("dictation take: 32000 samples"), "{both}");

    let dictation = input_format_change_line(
        48_000,
        2,
        24_000,
        1,
        RetuneCapture::Dictation { samples: 4_800 },
    );
    assert!(
        dictation.contains("(dictation take: 4800 samples captured so far;"),
        "{dictation}"
    );
}

#[test]
fn a_meeting_capture_banks_every_retune_with_its_old_and_new_format() {
    let capture = MeetingCapture::new(48_000, 2, None);
    assert!(capture.input_retunes().is_empty());
    capture.retune(24_000, 1);
    capture.retune(16_000, 1);
    let retunes = capture.input_retunes();
    assert_eq!(retunes.len(), 2);
    assert_eq!(
        (retunes[0].from_rate, retunes[0].from_channels),
        (48_000, 2)
    );
    assert_eq!((retunes[0].to_rate, retunes[0].to_channels), (24_000, 1));
    assert_eq!((retunes[1].from_rate, retunes[1].to_rate), (24_000, 16_000));
    assert_eq!(retunes[0].track, 0);
    assert!(retunes[0].at_seconds >= 0.0 && retunes[0].at_seconds <= retunes[1].at_seconds);
}

#[test]
fn the_diagnostics_blob_carries_the_retunes_and_older_blobs_still_parse() {
    let mut d = MeetingDiagnostics::start(ThermalState::Nominal, BatteryReading::default(), false);
    let retune = InputRetune {
        at_seconds: 1800.5,
        captured_seconds: 1799.9,
        track: 0,
        from_rate: 48_000,
        from_channels: 1,
        to_rate: 16_000,
        to_channels: 1,
    };
    d.record_input_retunes(&[retune]);
    let json = d.to_json();
    assert!(json.contains("\"inputRetunes\":[{"), "{json}");
    assert!(json.contains("\"atSeconds\":1800.5"), "{json}");
    assert!(json.contains("\"fromRate\":48000"), "{json}");
    assert!(json.contains("\"toRate\":16000"), "{json}");
    let back: MeetingDiagnostics = serde_json::from_str(&json).expect("round-trip");
    assert_eq!(back.input_retunes, vec![retune]);

    // A blob written before this field existed has no `inputRetunes` key.
    let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
    old.as_object_mut().unwrap().remove("inputRetunes");
    let parsed: MeetingDiagnostics = serde_json::from_value(old).expect("old blob parses");
    assert!(parsed.input_retunes.is_empty());
}
