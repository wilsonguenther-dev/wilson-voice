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
//! true-rate logging into the meeting's `diagnostics`) is the OTHER half of
//! this module, appended below when PR #130 rebased onto this file — a fixture
//! generator, not a reference implementation. The two halves are deliberately
//! independent: nothing above this line imports `meeting_asr`, and nothing
//! below it is used to score the harness's own residual. That is what makes
//! the YV109 gates a CROSS-CHECK of `HostTimeline` against an independent
//! implementation rather than a measurement of the implementation by itself —
//! the position this module's original header said would be strictly better
//! than deleting it, now reached.
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

use wilson_voice_lib::asr_engine::{TimedKind, TimedSpan};
use wilson_voice_lib::meeting::{plan_silence_splices, IndexRecord, TARGET_RATE};
use wilson_voice_lib::meeting_asr::{
    BoundaryKind, ChunkOutcome, ChunkStatus, MergedSpan, MEETING_RATE,
};
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

// ============================================================================
// YV107 — synthetic two-track FIXTURES (a generator, not a reference)
// ============================================================================
//
// Everything below is derived from ONE model of the defect, stated once:
//
// * A track's device runs at `nominal x (1 + ppm/1e6)` real hertz.
// * `IndexRecord::captured_samples` counts what the device DELIVERED, scaled
//   onto the nominal rate — so after `t` real seconds it reads
//   `nominal x (1 + ppm/1e6) x t`, which is more (or less) than `nominal x t`.
// * `IndexRecord::host_ns` counts real nanoseconds, because it is stamped off
//   the host clock rather than derived from samples.
// * The finalized wav is what reached the disk plus what the repair put back —
//   `spilled_samples + sum` of the splices `meeting::plan_silence_splices`
//   planned up to that point (`meeting::finalized_positions`) — and the ASR
//   times it at the NOMINAL rate, so on a clean track a word spoken at real
//   second `t` is timestamped at `t x (1 + ppm/1e6)`: the drift, in one line.
//   On a track that lost audio that reduces to `captured_samples`; on a track
//   whose DEVICE stalled it does not, and `index_records_stalled` is the
//   fixture for the difference.
//
// That last line is the bug OS-2 names, and it is GENERATED here rather than
// asserted, so a test that passes because the fixture is flat cannot happen:
// `local_seconds` is what the merge is fed, and `t` is what it must recover.

/// The relative offset OS-2's acceptance criterion names. 100 ppm over the
/// 3-hour cap is 1.08 s of skew — well past the 200 ms at which an interleave
/// starts putting an answer before its question.
pub const FIXTURE_PPM: f64 = 100.0;

/// The plan's own cap, and the horizon the residual is asserted at. Aliased
/// onto [`RESIDUAL_HORIZON_SECONDS`] rather than restated: two spellings of the
/// 3-hour cap in one module is how the merge's budget and the harness's budget
/// would eventually stop being the same number.
pub const THREE_HOURS: f64 = RESIDUAL_HORIZON_SECONDS;

/// The acceptance criterion's line, in seconds — [`RESIDUAL_BUDGET_MS`] in the
/// unit this half of the module works in.
pub const RESIDUAL_BUDGET_SECONDS: f64 = RESIDUAL_BUDGET_MS / 1000.0;

/// Samples this track's device has delivered after `t` real seconds, as
/// `captured_samples` counts them.
pub fn captured_at(ppm: f64, t: f64) -> f64 {
    MEETING_RATE as f64 * (1.0 + ppm / 1e6) * t
}

/// Where a word spoken at real second `t` lands on this track's OWN timeline —
/// i.e. what the ASR will timestamp it as, because the finalized wav is timed at
/// the nominal rate. This is the drifted number the merge has to undo.
pub fn local_seconds(ppm: f64, t: f64) -> f64 {
    captured_at(ppm, t) / MEETING_RATE as f64
}

/// One index record per host second, exactly as the capture consumer writes
/// them ([`wilson_voice_lib::meeting::INDEX_INTERVAL`] is one second).
///
/// `spilled_samples` equals `captured_samples`: this fixture is about clocks,
/// not about loss, and a divergence here would be testing
/// `plan_silence_splices` instead.
pub fn index_records(ppm: f64, seconds: u64) -> Vec<IndexRecord> {
    (0..=seconds)
        .map(|k| {
            let captured = captured_at(ppm, k as f64).round() as u64;
            IndexRecord {
                host_ns: k * 1_000_000_000,
                captured_samples: captured,
                spilled_samples: captured,
            }
        })
        .collect()
}

/// The same sequence, but the journal stopped accepting for a moment: the
/// device kept delivering, the queue was full, and from `loss_at_second`
/// onwards `spilled_samples` trails `captured_samples` by `loss_samples`.
///
/// This is the fixture that separates the two COUNTERS, and it separates them
/// because the finalize does. The counter rule splices `captured − spilled`
/// samples of silence into the wav at the point the loss happened, so here —
/// and only here — the finalized track is `captured` samples long. A map keyed
/// on `spilled_samples` shifts everything after the gap by the whole length of
/// the gap, and nothing in a lossless fixture can tell the two apart.
///
/// It cannot show the other half of the finalize, and that is the point of
/// [`index_records_stalled`]: this shape can never make `finalized` differ from
/// `captured`, so a map keyed on `captured_samples` passes it, which is exactly
/// how that keying survived review.
///
/// **`spilled_samples` only ever moves forward.** A journal that lost two
/// seconds inside a one-second interval is not a thing that can happen — the
/// counter would have to run backwards — so the deficit here opens at the rate
/// the device delivers and stops at `loss_samples`, which spreads a loss longer
/// than one interval across as many intervals as it really takes.
pub fn index_records_lossy(
    ppm: f64,
    seconds: u64,
    loss_at_second: u64,
    loss_samples: u64,
) -> Vec<IndexRecord> {
    let opens_at = captured_at(ppm, loss_at_second.saturating_sub(1) as f64);
    index_records(ppm, seconds)
        .into_iter()
        .enumerate()
        .map(|(k, r)| {
            let deficit = if (k as u64) < loss_at_second {
                0
            } else {
                loss_samples.min((r.captured_samples as f64 - opens_at).max(0.0) as u64)
            };
            IndexRecord {
                spilled_samples: r.spilled_samples.saturating_sub(deficit),
                ..r
            }
        })
        .collect()
}

/// The sequence a DEVICE STALL leaves behind: `host_ns` keeps advancing while
/// **both** counters freeze, for `stall_seconds` starting at `stall_at_second`.
///
/// This is the shape [`index_records_lossy`] cannot express, and it is the one
/// that decides how the host-time map must be keyed. No callback fires during a
/// stall, so nothing counts anything: `captured − spilled` stays at zero, the
/// counter rule sees a flawless recording, and only
/// [`wilson_voice_lib::meeting::plan_silence_splices`]'s wall-clock rule
/// notices — splicing the whole shortfall into the finalized wav, which comes
/// out LONGER than `captured` by the length of the stall. Everything after it
/// therefore sits `stall_seconds` further into the wav than `captured_samples`
/// says, which is the residual a map keyed on `captured_samples` inherits.
///
/// It is also the routine case rather than the exotic one on track 1: a process
/// tap delivers nothing at all whenever the tapped app is silent.
pub fn index_records_stalled(
    ppm: f64,
    seconds: u64,
    stall_at_second: u64,
    stall_seconds: u64,
) -> Vec<IndexRecord> {
    (0..=seconds)
        .map(|k| {
            // Audio only exists for the host seconds the device was actually
            // delivering in; the stall's seconds produced none.
            let delivered = k as f64 - k.saturating_sub(stall_at_second).min(stall_seconds) as f64;
            let captured = captured_at(ppm, delivered).round() as u64;
            IndexRecord {
                host_ns: k * 1_000_000_000,
                captured_samples: captured,
                spilled_samples: captured,
            }
        })
        .collect()
}

/// The sequence a stream REOPEN leaves behind: `host_ns` restarts at zero on
/// the new stream's first callback while BOTH counters keep running.
///
/// That is not a hypothetical shape, it is the shipped one.
/// `record.rs::build_capture_stream` rebases `host_ns` to each stream build's
/// own first callback — it has to, because `cpal` only defines
/// `StreamInstant::duration_since` within one stream — and
/// [`wilson_voice_lib::meeting::MeetingCapture::retune_track`], the documented
/// handler for exactly this reopen (an AirPods swap on track 0, YV103's
/// `RebuildAggregate` on track 1), deliberately carries `captured_samples`
/// across the seam via its `captured_base`. So the wav is CONTINUOUS and the
/// clock underneath it restarts.
///
/// **`post_seconds` longer than `pre_seconds` is the case that matters**, and it
/// is the common one: a swap five minutes into a ninety-minute meeting, an
/// aggregate rebuilt minutes in with hours left to run. A map that merely DROPS
/// the backwards records recovers only until the new clock passes the old
/// maximum, and then re-times the whole remainder of the meeting by the length
/// of the pre-reopen run — which is why a fixture with a SHORT post-reopen run
/// cannot fail and is therefore not a test.
///
/// The reopen itself is instantaneous here. Real dead air across a stream
/// rebuild is audio that was never captured and is in neither clock's domain, so
/// it is a separate, bounded, non-accumulating offset — see
/// `a_reopen_is_early_by_its_own_dead_air_and_by_nothing_else`.
pub fn index_records_rebased(
    pre_ppm: f64,
    pre_seconds: u64,
    post_ppm: f64,
    post_seconds: u64,
) -> Vec<IndexRecord> {
    let mut records = index_records(pre_ppm, pre_seconds);
    let (captured, spilled) = records
        .last()
        .map(|r| (r.captured_samples, r.spilled_samples))
        .unwrap_or((0, 0));
    records.extend(
        index_records(post_ppm, post_seconds)
            .into_iter()
            .map(|r| IndexRecord {
                host_ns: r.host_ns,
                captured_samples: r.captured_samples + captured,
                spilled_samples: r.spilled_samples + spilled,
            }),
    );
    records
}

/// Where a word spoken at real second `t` lands on the timeline of a track that
/// STALLED — i.e. what the ASR will timestamp it as, given the finalize spliced
/// the stall back in as silence.
///
/// Two terms, and they are the whole point: the silence the repair inserted
/// (nominal-rate, because that is the rate the wall-clock rule counts in) plus
/// the audio the device actually delivered before and after it (drifted, at the
/// device's own rate). Generated from the same model as
/// [`index_records_stalled`] rather than asserted, so the fixture and the
/// expectation cannot quietly agree on the wrong thing.
pub fn local_seconds_stalled(ppm: f64, t: f64, stall_at: f64, stall_seconds: f64) -> f64 {
    let silence = (t - stall_at).clamp(0.0, stall_seconds);
    silence + local_seconds(ppm, t - silence)
}

/// One chunk holding `spans`, with a content range that owns all of them.
///
/// `merge_timed` keeps a span whose MIDPOINT falls inside the content range, so
/// the range is stated explicitly rather than inferred: a fixture whose chunk
/// silently dropped half its spans would make every downstream assertion pass
/// vacuously.
pub fn chunk(
    index: usize,
    content_start: f64,
    content_end: f64,
    start_boundary: BoundaryKind,
    spans: Vec<TimedSpan>,
) -> ChunkOutcome {
    let text = spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    ChunkOutcome {
        index,
        audio_start_seconds: content_start,
        content_start_seconds: content_start,
        content_end_seconds: content_end,
        start_boundary,
        end_boundary: BoundaryKind::Silence,
        status: ChunkStatus::Done,
        text,
        spans,
        timestamp_kind: TimedKind::Word,
        error: None,
    }
}

/// A word on one track's own timeline, spoken at real second `t` for
/// `duration` real seconds.
pub fn word(ppm: f64, t: f64, duration: f64, text: &str) -> TimedSpan {
    TimedSpan {
        start_seconds: local_seconds(ppm, t),
        end_seconds: local_seconds(ppm, t + duration),
        text: text.to_string(),
    }
}

/// `(speaker, text)` for every merged span, in the order the merge produced —
/// the readable form of "who spoke when".
pub fn turns(merged: &[MergedSpan]) -> Vec<(String, String)> {
    merged
        .iter()
        .map(|s| (s.speaker.clone(), s.text.clone()))
        .collect()
}

/// The worst |merged − truth| over a list of `(text, real_second)` expectations,
/// in milliseconds.
pub fn worst_residual_ms(merged: &[MergedSpan], truth: &[(&str, f64)]) -> f64 {
    let mut worst = 0.0f64;
    for (text, at) in truth {
        let span = merged
            .iter()
            .find(|s| s.text == *text)
            .unwrap_or_else(|| panic!("the merge dropped {text:?} entirely"));
        worst = worst.max((span.start_seconds - at).abs() * 1000.0);
    }
    worst
}

/// A track whose device delivers in BURSTS: `silent_for` seconds of nothing in
/// every `period` seconds, on an otherwise perfect `ppm` crystal.
///
/// This is not a fault fixture. It is the ORDINARY shape of the far side of a
/// call — a process tap gets no callbacks at all while the tapped app is silent,
/// which this PR's own documentation calls "the routine shape of the far side
/// rather than a hardware fault" — and it is the shape that made
/// `measure_true_rate` report −333,241 ppm on a crystal that never drifted, by
/// leaving every silent second in the denominator and none of its audio in the
/// numerator.
///
/// `host_ns` runs whatever the device does; `captured_samples` only advances in
/// the seconds it delivered, which is exactly what a frozen counter looks like.
pub fn index_records_bursty(
    ppm: f64,
    seconds: u64,
    period: u64,
    silent_for: u64,
) -> Vec<IndexRecord> {
    let mut delivered = 0.0f64;
    (0..=seconds)
        .map(|k| {
            // Second `k` produced audio unless it fell inside the silent window
            // of its period. Second 0 is the origin record and delivers nothing.
            if k > 0 && (k - 1) % period.max(1) >= silent_for {
                delivered += 1.0;
            }
            let captured = captured_at(ppm, delivered).round() as u64;
            IndexRecord {
                host_ns: k * 1_000_000_000,
                captured_samples: captured,
                spilled_samples: captured,
            }
        })
        .collect()
}

/// Stamp a deterministic PAIRING SLACK onto a record sequence: up to ±10 ms
/// between a record's `host_ns` and its `captured_samples`.
///
/// Every other fixture in this module is exact, which is what makes them good
/// tests of arithmetic and useless as tests of RESOLUTION. Real records are not
/// exact: `MeetingCapture::accept` stamps `host_ns` from the LAST anchor it
/// drained and counts `captured_samples` over the whole block, so the pair
/// carries up to about one callback period of slack. Inside one stretch of kept
/// intervals that slack telescopes away; across many short stretches it does
/// not, and it is the reason a chopped-up track cannot resolve a 50 ppm crystal
/// band. The generator is an LCG rather than a real RNG so a failure is
/// reproducible from the test name alone.
pub fn with_pairing_slack(records: &[IndexRecord]) -> Vec<IndexRecord> {
    records
        .iter()
        .enumerate()
        .map(|(k, record)| {
            let mut x = (k as u64)
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            x ^= x >> 33;
            let slack_ns = (x % 20_000_001) as i64 - 10_000_000;
            IndexRecord {
                host_ns: (record.host_ns as i64 + slack_ns).max(0) as u64,
                ..*record
            }
        })
        .collect()
}
