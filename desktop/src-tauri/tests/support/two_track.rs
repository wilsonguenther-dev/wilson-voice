//! YV109 — the harness's own HOST-TIME reference for a two-track meeting, and
//! the ordering vocabulary the fixture gate and the phase-closing E2E share.
//!
//! ## Why a reference lives in the harness at all
//!
//! Two producers, two crystals. The mic is clocked by the input device and the
//! tap by the aggregate's main sub-device (OS-2), and each track's finalized wav
//! is written at a NOMINAL 16 kHz whatever its device's true rate was. So a span
//! at local second `t` on the tap and a span at local second `t` on the mic are
//! two readings of two unrelated clocks, and comparing them directly — which is
//! all anything could do before this phase — orders a conversation by accident.
//!
//! The only shared clock either track ever saw is `mach_absolute_time`, carried
//! per callback in [`CaptureAnchor::host_ns`](wilson_voice_lib::rtring::CaptureAnchor)
//! and persisted about once a second as an
//! [`IndexRecord`]. This module turns one track's record sequence into the
//! affine map from ITS local seconds onto that shared clock, and it does so
//! INDEPENDENTLY of any shipped merge — which is the point. An eval harness that
//! scores the implementation with the implementation measures nothing; YV90's
//! own WER and seam metrics are hand-rolled here for exactly that reason, and
//! `the_harness_geometry_is_the_shipped_geometry` is how such a mirror is kept
//! from drifting away from the code it scores.
//!
//! ## What this is NOT
//!
//! It is not YV107. The shipped cross-track merge (`HostTimeline`, per-track
//! true-rate logging into the meeting's `diagnostics`) is PR #130, still in
//! review and **not on `main` at the time this landed** — which is why nothing
//! here is imported from the library and why the E2E beside it is honest about
//! rebasing the two tracks with this reference rather than with a shipped
//! function that does not exist yet. When #130 merges, the gates that use this
//! module become a cross-check of `HostTimeline` against an independent
//! implementation instead of the only implementation, and that is a strictly
//! better position than deleting it.
//!
//! ## The one thing this reference refuses to do
//!
//! A track that LOST audio has silence spliced into its finalized wav
//! ([`plan_silence_splices`]), so a local sample position in the wav is no
//! longer the same number as the `spilled_samples` the index records count. The
//! correct key is the FINALIZED position — spilled plus the silence spliced at
//! or before that record — and getting that wrong is exactly the defect YV107's
//! own review found and fixed (a device stall freezes both counters while the
//! splice is large, so several splices share one `at_sample` and the naive
//! attribution mis-times everything after it by the length of the stall).
//! Re-deriving that here would be forking YV107. So instead
//! [`TrackTimeline::from_records`] REFUSES a lossy record sequence outright,
//! and both callers assert their tracks are clean. A harness that quietly
//! mis-times a lossy track would be worse than one that will not score it.

#![allow(dead_code)] // each test binary uses a different subset

use std::path::Path;

use wilson_voice_lib::meeting::{plan_silence_splices, IndexRecord, TARGET_RATE};
use wilson_voice_lib::meetings::{MeetingSegment, TranscriptLine};

/// The horizon every residual in this phase is quoted at: the meeting hard cap
/// (`meeting::MEETING_HARD_CAP`, 3 h). A fixture is minutes long; the number
/// that matters is where its measured clock error would have put a word at the
/// end of the longest meeting Yap will record, which is an extrapolation of a
/// measured RATE rather than a measurement of a 3-hour recording nobody will
/// sit through in CI.
pub const RESIDUAL_HORIZON_SECONDS: f64 = 3.0 * 60.0 * 60.0;

/// The budget YV107's spec sets and this harness re-asserts end to end: 50 ms
/// of residual cross-track offset at [`RESIDUAL_HORIZON_SECONDS`]. Well under a
/// syllable, which is the unit that decides whether "Me" or "Them" said
/// something first.
pub const RESIDUAL_BUDGET_MS: f64 = 50.0;

/// One track's affine map from its own finalized-wav seconds onto the shared
/// host clock: `host = origin_seconds + local_seconds * TARGET_RATE * seconds_per_sample`.
///
/// `seconds_per_sample` is the reciprocal of the track's TRUE sample rate — the
/// rate its device actually ran at, which is not 16 000 and is not the rate the
/// wav header claims.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackTimeline {
    origin_seconds: f64,
    seconds_per_sample: f64,
    /// How many index records the fit was made from. `0` for an exact,
    /// constructed timeline (the ground truth a residual is measured against).
    records: usize,
}

impl TrackTimeline {
    /// The ground truth a fixture declares: where this track's local sample 0
    /// sat on the shared host clock, and the rate its device really ran at.
    pub fn exact(origin_seconds: f64, true_rate_hz: f64) -> TrackTimeline {
        assert!(true_rate_hz > 0.0, "a rate is positive");
        TrackTimeline {
            origin_seconds,
            seconds_per_sample: 1.0 / true_rate_hz,
            records: 0,
        }
    }

    /// The timeline as the pipeline can actually know it: a least-squares fit of
    /// `host_seconds` against finalized sample position over EVERY index record
    /// the track wrote.
    ///
    /// Least squares over every record rather than first-and-last, and the
    /// reason is the noisy case rather than this phase's fixtures. An index
    /// record's `host_ns` is the last drained anchor's capture timestamp while
    /// its sample counters run through the END of that callback, so every
    /// record carries a timing offset of one callback. On a synthetic stream
    /// that offset is constant and the records are exactly collinear — an
    /// endpoint estimator would agree to the last bit, and saying otherwise
    /// here would be a claim these fixtures do not measure. On a real device it
    /// is not constant (variable block sizes, a coalesced callback after a
    /// scheduling hiccup), and two endpoints inherit that undivided while a fit
    /// over N records divides it down. The constant part of the offset is not
    /// removed by either: it lands in the ORIGIN, is different per track when
    /// the two devices use different callback sizes, and shows up honestly in
    /// the cross-track residual the gates print.
    ///
    /// # Panics
    ///
    /// If the record sequence describes a track that lost audio — see this
    /// module's header. The finalized position of a lossy track is YV107's
    /// `finalized_positions`, not this file's.
    pub fn from_records(records: &[IndexRecord]) -> TrackTimeline {
        assert!(
            records.len() >= 2,
            "a rate cannot be measured from {} record(s)",
            records.len()
        );
        let splices = plan_silence_splices(records);
        assert!(
            splices.is_empty(),
            "this track lost audio ({} splice(s)) — a finalized sample position \
             is no longer its `spilled_samples`, and deriving the corrected one \
             here would fork YV107's `finalized_positions`",
            splices.len()
        );
        Self::fit(records)
    }

    /// The pre-22-B assumption, as a timeline: every track ran at exactly
    /// 16 000 Hz. Origin still measured, rate assumed. This is the mutation the
    /// whole fixture exists to redden.
    pub fn nominal_rate(records: &[IndexRecord]) -> TrackTimeline {
        assert!(!records.is_empty(), "an origin needs a record");
        let seconds_per_sample = 1.0 / TARGET_RATE as f64;
        let r = records[0];
        TrackTimeline {
            origin_seconds: ns(r.host_ns) - r.spilled_samples as f64 * seconds_per_sample,
            seconds_per_sample,
            records: records.len(),
        }
    }

    fn fit(records: &[IndexRecord]) -> TrackTimeline {
        let n = records.len() as f64;
        let xs: Vec<f64> = records.iter().map(|r| r.spilled_samples as f64).collect();
        let ys: Vec<f64> = records.iter().map(|r| ns(r.host_ns)).collect();
        let mean_x = xs.iter().sum::<f64>() / n;
        let mean_y = ys.iter().sum::<f64>() / n;
        let mut sxx = 0.0f64;
        let mut sxy = 0.0f64;
        for (x, y) in xs.iter().zip(ys.iter()) {
            sxx += (x - mean_x) * (x - mean_x);
            sxy += (x - mean_x) * (y - mean_y);
        }
        assert!(
            sxx > 0.0,
            "every record reports the same sample position — nothing to fit"
        );
        let seconds_per_sample = sxy / sxx;
        assert!(
            seconds_per_sample > 0.0,
            "the fitted rate is not positive: the records run backwards"
        );
        TrackTimeline {
            origin_seconds: mean_y - seconds_per_sample * mean_x,
            seconds_per_sample,
            records: records.len(),
        }
    }

    /// This track's local second `local_seconds`, on the shared host clock.
    pub fn host_seconds(&self, local_seconds: f64) -> f64 {
        self.origin_seconds + local_seconds * TARGET_RATE as f64 * self.seconds_per_sample
    }

    /// The device's measured true rate, in Hz.
    pub fn measured_rate(&self) -> f64 {
        1.0 / self.seconds_per_sample
    }

    /// Where this track thinks local second 0 sat on the shared clock.
    pub fn origin_seconds(&self) -> f64 {
        self.origin_seconds
    }

    /// Signed drift against the truth at `horizon_seconds` of LOCAL time:
    /// positive means this timeline places a word later than it was spoken.
    pub fn drift_seconds_at(&self, truth: &TrackTimeline, horizon_seconds: f64) -> f64 {
        self.host_seconds(horizon_seconds) - truth.host_seconds(horizon_seconds)
    }

    /// The same, in milliseconds and unsigned — one track's own residual.
    pub fn residual_ms_at(&self, truth: &TrackTimeline, horizon_seconds: f64) -> f64 {
        self.drift_seconds_at(truth, horizon_seconds).abs() * 1000.0
    }
}

/// How far apart Me and Them slide by `horizon_seconds`, in milliseconds.
///
/// This — not either track's own residual — is the number a two-track ordering
/// gate has to hold: a common error that moves BOTH tracks by the same amount
/// re-times the transcript but never reorders it, while a differential error of
/// one syllable puts an answer before the question that prompted it.
pub fn cross_track_residual_ms(
    mic: (&TrackTimeline, &TrackTimeline),
    system: (&TrackTimeline, &TrackTimeline),
    horizon_seconds: f64,
) -> f64 {
    let mic_drift = mic.0.drift_seconds_at(mic.1, horizon_seconds);
    let sys_drift = system.0.drift_seconds_at(system.1, horizon_seconds);
    (mic_drift - sys_drift).abs() * 1000.0
}

fn ns(host_ns: u64) -> f64 {
    host_ns as f64 / 1_000_000_000.0
}

// ---------------------------------------------------------------------------
// Reading a live journal's own index records back off disk
// ---------------------------------------------------------------------------

/// Every track's persisted [`IndexRecord`] sequence, read out of the journal's
/// marker and sidecars exactly as `finalize_meeting_marker` reads them.
///
/// Called BEFORE `finalize`, because finalize consumes the marker and removes
/// the sidecars. Reading the real files rather than keeping a copy of what was
/// fed in is deliberate: the E2E's claim is that the numbers a merge needs
/// SURVIVED to disk, and a test that rebases from its own in-memory ground
/// truth would pass on a journal that persisted nothing.
pub fn read_index_sidecars(dir: &Path, journal_id: &str) -> Vec<Vec<IndexRecord>> {
    let marker = dir.join(format!("{journal_id}.meeting.in_progress.json"));
    let raw = std::fs::read_to_string(&marker)
        .unwrap_or_else(|e| panic!("the journal marker is on disk at {}: {e}", marker.display()));
    let meta: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("the marker is JSON: {e}"));
    let tracks = meta
        .get("tracks")
        .and_then(|t| t.as_array())
        .expect("the marker names its tracks")
        .clone();
    tracks
        .iter()
        .map(|t| {
            let path = t
                .get("index")
                .and_then(|p| p.as_str())
                .expect("every track names its own index sidecar");
            read_index_jsonl(Path::new(path))
        })
        .collect()
}

/// [`read_index_sidecars`], but waiting for the journal's writer thread to have
/// flushed at least `at_least` records per track.
///
/// The writer flushes on its own 250 ms timer, so reading immediately after the
/// last block races it. A poll with a deadline rather than a sleep: a fixed
/// sleep is either too short on a loaded machine or wasted time on an idle one,
/// and this one fails with the counts it actually saw.
pub fn wait_for_index_records(
    dir: &Path,
    journal_id: &str,
    tracks: usize,
    at_least: usize,
) -> Vec<Vec<IndexRecord>> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let records = read_index_sidecars(dir, journal_id);
        let ready = records.len() == tracks && records.iter().all(|r| r.len() >= at_least);
        if ready || std::time::Instant::now() > deadline {
            assert_eq!(records.len(), tracks, "one index sidecar per track");
            for (t, r) in records.iter().enumerate() {
                assert!(
                    r.len() >= at_least,
                    "track {t} persisted {} index records, expected at least {at_least}",
                    r.len()
                );
            }
            return records;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// The same records as a committed JSON ARRAY — how a corpus fixture carries
/// them, because the eval corpus holds `wav`/`txt`/`json` only. Every field is
/// the journal's own; nothing is recomputed on the way in.
pub fn read_index_array_json(path: &Path) -> Vec<IndexRecord> {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} is part of the fixture: {e}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()));
    value
        .as_array()
        .unwrap_or_else(|| panic!("{} is an array of index records", path.display()))
        .iter()
        .map(|v| IndexRecord {
            host_ns: v.get("host_ns").and_then(|x| x.as_u64()).expect("host_ns"),
            captured_samples: v
                .get("captured_samples")
                .and_then(|x| x.as_u64())
                .expect("captured_samples"),
            spilled_samples: v
                .get("spilled_samples")
                .and_then(|x| x.as_u64())
                .expect("spilled_samples"),
        })
        .collect()
}

/// The sidecar's on-disk format: one JSON object per line, partial last line
/// tolerated (that is what a crash leaves).
pub fn read_index_jsonl(path: &Path) -> Vec<IndexRecord> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            Some(IndexRecord {
                host_ns: value.get("host_ns")?.as_u64()?,
                captured_samples: value.get("captured_samples")?.as_u64()?,
                spilled_samples: value.get("spilled_samples")?.as_u64()?,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Ordering — what the gate actually scores
// ---------------------------------------------------------------------------

/// One transcript row on the way into the render, before it is a DB row: the
/// track it came from and where it sits on the SHARED clock.
#[derive(Debug, Clone, PartialEq)]
pub struct HostSpan {
    pub track: i64,
    pub host_start_seconds: f64,
    pub host_end_seconds: f64,
    pub text: String,
}

/// `Me:word` / `Them:word`, in rendered order — the sequence a fixture declares
/// as ground truth and the gate compares against.
///
/// Only the words the fixture named as markers are kept. Ordinary filler words
/// are not scoreable: they are not unique, and a merge that reordered two of
/// them would be indistinguishable from a decoder that heard them in the other
/// order.
pub fn marker_sequence(lines: &[TranscriptLine], markers: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for line in lines {
        for word in line.text.split_whitespace() {
            let word = word
                .trim_matches(|c: char| !c.is_ascii_alphanumeric())
                .to_ascii_lowercase();
            if markers.iter().any(|m| *m == word) {
                out.push(format!("{}:{}", line.speaker, word));
            }
        }
    }
    out
}

/// Every place a rendered transcript goes backwards in time, as
/// `(index, previous_start, this_start)`.
///
/// The render sorts, so on its own output this is always empty — which is the
/// point: it is asserted on the render's INPUT ordering too, where a merge that
/// failed to rebase the two clocks shows up as rows the sort had to move.
pub fn out_of_order(starts: &[f64]) -> Vec<(usize, f64, f64)> {
    let mut out = Vec::new();
    for (i, pair) in starts.windows(2).enumerate() {
        if pair[1] < pair[0] {
            out.push((i + 1, pair[0], pair[1]));
        }
    }
    out
}

/// [`HostSpan`]s as the stored rows the render reads, ordered by insertion so
/// the render's own sort is the thing under test.
pub fn segments_from_host_spans(meeting_id: &str, spans: &[HostSpan]) -> Vec<MeetingSegment> {
    spans
        .iter()
        .enumerate()
        .map(|(i, s)| MeetingSegment {
            id: format!("{meeting_id}-seg-{i}"),
            meeting_id: meeting_id.to_string(),
            start_seconds: s.host_start_seconds,
            end_seconds: s.host_end_seconds,
            text: s.text.clone(),
            confidence: None,
            created_at: chrono::Utc::now(),
            track: s.track,
        })
        .collect()
}
