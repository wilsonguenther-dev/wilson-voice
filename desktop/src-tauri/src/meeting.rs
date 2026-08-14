//! YV91 — the meeting capture session.
//!
//! yap22-A records a meeting off the microphone Yap already owns. Five defects
//! stood between "a five-second dictation works" and "a three-hour recording
//! survives", and they land together here because they are one piece of work
//! (the panel's OS-7 framing):
//!
//! 1. **Bounded memory** (finding #1). `StreamDsp` keeps `out` (16 kHz) and
//!    `raw` (native rate) growing for the whole take. That is correct for a
//!    five-second dictation with a never-lose-the-audio fallback; for three
//!    hours on one track it is ~2.8 GB resident before post-processing even
//!    starts. A meeting therefore never retains its audio in RAM: each drained
//!    block is resampled and handed to the spill, and the buffer it went
//!    through is truncated in the same breath. Peak RSS is a test, not a hope
//!    (`tests/meeting_capture_memory.rs`, gate: < 400 MB over a synthetic 3 h).
//! 2. **A real-time-safe capture callback** (OS-7). See [`crate::rtring`]: the
//!    callback copies into a preallocated lock-free ring and returns; all DSP
//!    moves to a normal-priority consumer. Built once here for the mic path so
//!    22-B's CoreAudio tap IOProc reuses it verbatim.
//! 3. **A host-time anchor** (OS-2 / findings #12 and #3). Every callback emits
//!    `(host_ns, sample_index, frames, sample_rate, lost_frames)`; roughly once
//!    a second one of those is persisted as a journal index record. That one
//!    datum is the gap detector, the resume anchor, and — in 22-B — the
//!    cross-track alignment key.
//!
//!    A gap detector is only as good as its two numbers, and both of them are
//!    easy to compute in a way that can never disagree — which is the same as
//!    not having a detector at all. So: `spilled` advances by what the journal
//!    ACCEPTED (a rejected chunk moves it not at all), the anchor's
//!    `sample_index` counts what the device DELIVERED (never what fit in the
//!    ring), and neither number is clamped against the other on the way to
//!    disk. The two independent losses — a full journal queue and a full RT
//!    ring — each produce a divergence, and each divergence becomes spliced
//!    silence rather than a track that is quietly short and wrong after it.
//!    Both routes are tested end to end in `tests/capture_journal_recovery.rs`.
//!
//!    Those two routes share a blind spot, and closing it is what `host_ns` is
//!    FOR. Both count audio that arrived; neither can see audio that never
//!    arrived at all. When the device simply stops delivering — the HAL wedges,
//!    the machine sleeps through a stall, a USB interface drops off — the
//!    callback stops firing, so `captured` and `spilled` freeze together, agree
//!    perfectly, and a detector reading only those two says the recording is
//!    healthy while every timestamp after the stall silently shifts. So
//!    [`plan_silence_splices`] measures each interval against the WALL CLOCK
//!    the anchors carry as well: one second of host time that produced a
//!    quarter-second of audio is a gap, whoever's fault it was. Two clocks,
//!    two independent failure classes, one splice list — and a stall long
//!    enough to matter also stops the meeting through the watchdog's liveness
//!    input ([`CAPTURE_STALL_LIMIT`]) rather than recording an hour of nothing.
//! 4. **A power assertion** (OS-1, matrix row #16). See [`crate::power`]. Idle
//!    system sleep only; never the display variant.
//! 5. **Fan-out and auto-mute** (findings #2a and #9). A dictation hotkey
//!    mid-meeting must not wipe the meeting, and must not mute the call the
//!    user is listening to. The fan-out falls out of (2): the meeting sink and
//!    the dictation DSP are two consumers of the SAME drained block, so a
//!    dictation take can neither reset nor gap the meeting. Auto-mute becomes a
//!    property of the take's [`TakeContext`], and only a plain dictation has it.
//!
//! 6. **The stream itself** ([`CaptureStream`]). A meeting HOLDS the input
//!    stream open for its whole length. It has no Arm and no Disarm, so the
//!    capture worker's idle closer — which drops the cpal stream after 60
//!    seconds without a take, to put the mic indicator out — would otherwise
//!    end the audio a minute in while the session went on counting elapsed time
//!    and finalized a 60-second wav as `complete`. Riding on whatever stream a
//!    dictation left open is not a recording, it is a coincidence.
//!
//! Rounded out with the disk/battery/thermal preflight, the 60 s watchdog and
//! the 3 h hard cap (findings #27, #39; matrix rows #4 and #17).
//!
//! Everything a decision depends on is a pure function over explicit inputs —
//! [`preflight`], [`watchdog_tick`], [`auto_mute_allowed`],
//! [`plan_silence_splices`], [`parse_pmset_batt`] — so the session's behaviour
//! under a full disk, a flat battery or a dropped spill chunk is provable
//! without a microphone, a meeting, or three hours.

use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use uuid::Uuid;

use crate::power::PowerAssertion;
use crate::record::HIGH_PASS_HZ;
/// Every meeting track is stored, spilled and finalized at 16 kHz mono — the
/// rate the ASR engine consumes, so nothing downstream ever resamples again.
pub use crate::record::TARGET_RATE;
// YV92 moved the DSP primitives into their own module and put an anti-alias
// cascade in front of the interpolator. The meeting path takes the SAME
// `StreamResampler::new` the dictation path takes, so a three-hour room
// recording is decimated through the 7.2 kHz lowpass, not straight through the
// triangle kernel.
use crate::resample::{Biquad, StreamResampler};
use crate::rtring::{CaptureAnchor, SpscRing};

// ── Budgets and caps ────────────────────────────────────────────────────────

/// Matrix row #17: a meeting is capped at three hours. Past that we stop
/// cleanly (and 22-A's UI offers a continuation) rather than discovering the
/// limit as a full disk or a dead battery.
pub const MEETING_HARD_CAP: Duration = Duration::from_secs(3 * 60 * 60);
/// Warn at 2 h 45 m so the cap is never a surprise.
pub const MEETING_CAP_WARN_AT: Duration = Duration::from_secs(2 * 60 * 60 + 45 * 60);
/// Matrix row #4: re-check the world every 60 s while recording.
pub const WATCHDOG_INTERVAL: Duration = Duration::from_secs(60);
/// Matrix row #4: below 1 GB free we stop cleanly and finalize, rather than
/// writing until the filesystem errors.
pub const DISK_FLOOR_BYTES: u64 = 1024 * 1024 * 1024;
/// Spill rate for one 16 kHz mono i16 track ≈ 32 KB/s; the plan budgets
/// 64 KB/s so the two-track 22-B shape needs no new formula.
pub const DISK_BYTES_PER_SECOND: u64 = 64 * 1024;
/// Headroom on top of the recording itself, so a meeting never fills the disk
/// out from under the rest of the machine.
pub const DISK_HEADROOM_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Finding #27: refuse to start a LONG meeting on a battery below this.
pub const BATTERY_REFUSE_PERCENT: u8 = 20;
/// What counts as "long" for the battery rule.
pub const LONG_MEETING: Duration = Duration::from_secs(2 * 60 * 60);
/// Finding #12: a meeting's journal queue is far deeper than a dictation's 64,
/// because a meeting rides out disk hiccups for hours rather than seconds.
pub const MEETING_QUEUE_DEPTH: usize = 512;
/// Finding #12: flush the spill on a timer, not per callback.
pub const JOURNAL_FLUSH_INTERVAL: Duration = Duration::from_millis(250);
/// Finding #12: persist one index record about once a second.
pub const INDEX_INTERVAL: Duration = Duration::from_secs(1);
/// The smallest divergence between "what the device delivered" and "what
/// reached the spill" that counts as a GAP. One millisecond at 16 kHz: below
/// that the difference is the streaming resampler's phase (it withholds the
/// output sample whose right-hand neighbour has not arrived yet), not lost
/// audio, and splicing silence for it would be the false positive that makes
/// the gap detector useless.
pub const SPLICE_MIN_SAMPLES: u64 = 16;
/// The smallest WALL-CLOCK shortfall that counts as a stall — a quarter second.
///
/// A separate, much larger threshold than [`SPLICE_MIN_SAMPLES`] on purpose.
/// That one compares two counters advanced by the same code from the same
/// blocks, so they are exact and 1 ms of slack is generous. This one compares
/// the audio clock against the host clock: the anchor's `host_ns` is stamped
/// when the ADC captured the frames, the record is written when a block crosses
/// the cadence, and those two events are separated by a callback period plus
/// whatever the consumer's scheduler did — tens of milliseconds of legitimate
/// jitter on a busy machine. 250 ms is far above that jitter and far below any
/// stall worth reporting, and the first thing anyone does with a detector that
/// cries wolf is stop believing it.
pub const STALL_MIN_SAMPLES: u64 = TARGET_RATE as u64 / 4;
/// The most silence ONE interval may splice for a wall-clock shortfall: an hour.
///
/// A stall is bounded by the watchdog ([`CAPTURE_STALL_LIMIT`]), so an interval
/// claiming more than this is not a stall — it is a host clock that changed
/// meaning under us (a stream rebuilt mid-meeting rebases `host_ns` to its own
/// first callback, and 22-B adds a second track with its own epoch). Believing
/// such a number would turn a bad timestamp into a multi-gigabyte wav, which is
/// a worse outcome than the gap it was trying to document.
pub const MAX_STALL_SPLICE_SAMPLES: u64 = TARGET_RATE as u64 * 60 * 60;
/// OS-1: how long the meeting may consume NO blocks at all before the watchdog
/// calls the capture dead and stops cleanly.
///
/// Longer than one [`WATCHDOG_INTERVAL`] so a single late tick is never a stop,
/// short enough that a wedged HAL costs 90 seconds rather than the rest of the
/// meeting. This is the liveness half of the same failure
/// [`plan_silence_splices`]'s host-clock rule documents after the fact: the
/// splice makes a short stall honest, this stops a permanent one.
pub const CAPTURE_STALL_LIMIT: Duration = Duration::from_secs(90);
/// How much audio the RT ring holds before the consumer must have drained it.
/// 4 seconds at 48 kHz stereo — deep enough that a scheduling stall on the
/// consumer is invisible, bounded so a wedged consumer costs a fixed 1.5 MB
/// rather than the session.
pub const RING_FRAMES: usize = 48_000 * 2 * 4;
/// Anchors are tiny and arrive once per callback; a second of them is plenty.
pub const ANCHOR_RING_LEN: usize = 512;
/// The longest [`MeetingJournal::pause_handle`] can park the writer thread. A
/// test seam that can hang a build is a worse defect than the one it covers.
pub const WRITER_PAUSE_CAP: Duration = Duration::from_secs(5);

/// Marker suffix — same shape (and same startup-scan meaning) as YV63's
/// dictation journal: its presence means a capture never completed.
const MEETING_MARKER_EXT: &str = "meeting.in_progress.json";
/// One track's raw spill: little-endian i16 mono at [`TARGET_RATE`].
const MEETING_SPILL_EXT: &str = "spill.pcm";
/// The index-record sidecar (JSON lines), one line per ~second of capture.
const MEETING_INDEX_EXT: &str = "index.jsonl";

/// Bytes of free disk a meeting of `duration` requires before it may start.
/// Finding #39: a manually-started meeting has no duration estimate, so the
/// caller passes `None` and this bills against the 3 h hard cap.
pub fn required_free_bytes(duration: Duration) -> u64 {
    duration
        .as_secs()
        .saturating_mul(DISK_BYTES_PER_SECOND)
        .saturating_add(DISK_HEADROOM_BYTES)
}

// ── Preflight ───────────────────────────────────────────────────────────────

/// `NSProcessInfo.thermalState`, mapped. `Unknown` is what a non-macOS build
/// (and a probe failure) reports, and it never blocks a recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalState {
    Unknown,
    Nominal,
    Fair,
    Serious,
    Critical,
}

/// Power-source snapshot. `percent` is `None` on a desktop with no battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatterySnapshot {
    pub on_ac: bool,
    pub percent: Option<u8>,
}

impl Default for BatterySnapshot {
    fn default() -> Self {
        Self {
            on_ac: true,
            percent: None,
        }
    }
}

/// Everything the start decision depends on, as data.
#[derive(Debug, Clone)]
pub struct PreflightInputs {
    pub free_bytes: u64,
    /// `None` for a manually-started meeting — finding #39's case.
    pub duration_estimate: Option<Duration>,
    pub battery: BatterySnapshot,
    pub thermal: ThermalState,
}

/// A start that is allowed to proceed, plus anything the user should be told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightPlan {
    /// The duration this start was budgeted against (the estimate, or the cap).
    pub budgeted: Duration,
    pub required_bytes: u64,
    pub warnings: Vec<String>,
}

/// A start that is refused, and exactly why — every variant carries the real
/// number, because "not enough disk space" with no number is the failure mode
/// matrix row #4 is written against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreflightError {
    NotEnoughDisk {
        free_bytes: u64,
        required_bytes: u64,
        budgeted: Duration,
    },
    LowBattery {
        percent: u8,
        budgeted: Duration,
    },
    TooHot {
        thermal: ThermalState,
    },
    /// The input stream could not be opened or held (YV91). A meeting whose
    /// audio source refuses must fail HERE — a session that starts anyway
    /// records nothing while every indicator says it is recording.
    NoInput {
        detail: String,
    },
    /// A meeting is already recording. There is exactly one capture consumer
    /// and one global slot, so a second session would not be a second meeting —
    /// it would be a silent takeover of the first (see [`ActiveMeeting`]).
    AlreadyRecording,
}

/// Bytes as a short human string. Used inside error text that must ALSO carry
/// the exact byte count, never instead of it.
fn human_bytes(bytes: u64) -> String {
    const GB: f64 = (1024u64 * 1024 * 1024) as f64;
    const MB: f64 = (1024u64 * 1024) as f64;
    if bytes as f64 >= GB {
        format!("{:.2} GB", bytes as f64 / GB)
    } else {
        format!("{:.0} MB", bytes as f64 / MB)
    }
}

fn human_duration(d: Duration) -> String {
    let mins = d.as_secs() / 60;
    format!("{}h {:02}m", mins / 60, mins % 60)
}

impl std::fmt::Display for PreflightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreflightError::NotEnoughDisk {
                free_bytes,
                required_bytes,
                budgeted,
            } => write!(
                f,
                "Not enough disk space to record: {} free ({free_bytes} bytes), {} needed ({required_bytes} bytes) for a {} recording. Free up {} and try again.",
                human_bytes(*free_bytes),
                human_bytes(*required_bytes),
                human_duration(*budgeted),
                human_bytes(required_bytes.saturating_sub(*free_bytes)),
            ),
            PreflightError::LowBattery { percent, budgeted } => write!(
                f,
                "Battery is at {percent}% and this recording is budgeted for {} — plug in, or start a shorter recording.",
                human_duration(*budgeted)
            ),
            PreflightError::TooHot { thermal } => write!(
                f,
                "The Mac is thermally throttled ({thermal:?}) — recording now would drop audio. Let it cool down and try again."
            ),
            PreflightError::NoInput { detail } => write!(
                f,
                "The microphone could not be opened for this meeting: {detail}"
            ),
            PreflightError::AlreadyRecording => write!(
                f,
                "A meeting is already recording. Stop it before starting another."
            ),
        }
    }
}

/// May this meeting start? Pure over [`PreflightInputs`], so every refusal —
/// and every number inside it — is testable without a disk, a battery or an
/// hour of waiting.
pub fn preflight(inputs: &PreflightInputs) -> Result<PreflightPlan, PreflightError> {
    // Finding #39: with no estimate, budget against the hard cap. A manually
    // started meeting is exactly the case that has no estimate.
    let budgeted = inputs.duration_estimate.unwrap_or(MEETING_HARD_CAP);
    let required_bytes = required_free_bytes(budgeted);

    if inputs.thermal == ThermalState::Critical {
        return Err(PreflightError::TooHot {
            thermal: inputs.thermal,
        });
    }
    if inputs.free_bytes < required_bytes {
        return Err(PreflightError::NotEnoughDisk {
            free_bytes: inputs.free_bytes,
            required_bytes,
            budgeted,
        });
    }
    let mut warnings = Vec::new();
    if let Some(percent) = inputs.battery.percent {
        if !inputs.battery.on_ac && percent < BATTERY_REFUSE_PERCENT {
            if budgeted > LONG_MEETING {
                return Err(PreflightError::LowBattery { percent, budgeted });
            }
            warnings.push(format!(
                "Battery is at {percent}% — this recording may not survive to the end."
            ));
        }
    }
    if inputs.thermal == ThermalState::Serious {
        warnings
            .push("The Mac is running hot — recording will continue but may throttle.".to_string());
    }
    Ok(PreflightPlan {
        budgeted,
        required_bytes,
        warnings,
    })
}

/// Parse `pmset -g batt`. Pure, because the probe is a process and the RULE
/// must not be.
pub fn parse_pmset_batt(output: &str) -> BatterySnapshot {
    let on_ac = output.contains("'AC Power'");
    let percent = output.split_whitespace().find_map(|token| {
        let t = token.trim_end_matches([';', ',']);
        t.strip_suffix('%').and_then(|n| n.parse::<u8>().ok())
    });
    BatterySnapshot { on_ac, percent }
}

// ── Watchdog ────────────────────────────────────────────────────────────────

/// Why a recording stopped itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Matrix row #4: below [`DISK_FLOOR_BYTES`].
    LowDisk { free_bytes: u64 },
    /// Matrix row #17.
    HardCap,
    /// OS-9: the capture device errored or renegotiated its format. YV92 wires
    /// the poll of `LiveStream::has_failed` into this input; the watchdog that
    /// reads it is here.
    DeviceFailed,
    /// OS-1: the device stopped delivering. Not an ERROR from the device — an
    /// absence, which is why nothing else notices it: no callback fires, so
    /// every counter freezes in agreement and the session would otherwise go on
    /// counting elapsed time over silence it never recorded.
    CaptureStalled { seconds: u64 },
    /// Thermal state went critical mid-recording.
    TooHot,
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StopReason::LowDisk { free_bytes } => write!(
                f,
                "stopped: only {} free ({free_bytes} bytes) — below the {} floor",
                human_bytes(*free_bytes),
                human_bytes(DISK_FLOOR_BYTES)
            ),
            StopReason::HardCap => {
                write!(f, "stopped at the {} cap", human_duration(MEETING_HARD_CAP))
            }
            StopReason::DeviceFailed => write!(f, "stopped: the capture device failed"),
            StopReason::CaptureStalled { seconds } => write!(
                f,
                "stopped: the capture device delivered nothing for {seconds}s"
            ),
            StopReason::TooHot => write!(f, "stopped: the Mac went thermally critical"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogAction {
    Continue,
    /// Matrix row #17's 2 h 45 m warning — fires once.
    WarnApproachingCap,
    Stop(StopReason),
}

/// One tick's view of the world.
#[derive(Debug, Clone, Copy)]
pub struct WatchdogInputs {
    pub elapsed: Duration,
    pub free_bytes: u64,
    pub device_failed: bool,
    /// OS-1 liveness: wall time since the meeting last consumed a block of
    /// audio. A device that stalls raises no error and sets no flag — it simply
    /// stops calling back — so this is the ONLY input that can see it.
    pub since_last_block: Duration,
    pub thermal: ThermalState,
    /// Has the approaching-cap warning already been delivered?
    pub cap_warned: bool,
}

/// The 60 s watchdog rule. Pure: the ordering below IS the policy, and it is
/// ordered by urgency — a dying device or a full disk beats a cap that is still
/// minutes away.
pub fn watchdog_tick(inputs: &WatchdogInputs) -> WatchdogAction {
    if inputs.device_failed {
        return WatchdogAction::Stop(StopReason::DeviceFailed);
    }
    // Ranked with the dead device, because it IS one — the silent kind, which
    // sets no flag and returns no error. A meeting that has consumed nothing
    // for this long is not recording, and every further second it stays open is
    // a second of wall clock the finalized track will have to account for.
    if inputs.since_last_block >= CAPTURE_STALL_LIMIT {
        return WatchdogAction::Stop(StopReason::CaptureStalled {
            seconds: inputs.since_last_block.as_secs(),
        });
    }
    if inputs.free_bytes < DISK_FLOOR_BYTES {
        return WatchdogAction::Stop(StopReason::LowDisk {
            free_bytes: inputs.free_bytes,
        });
    }
    if inputs.elapsed >= MEETING_HARD_CAP {
        return WatchdogAction::Stop(StopReason::HardCap);
    }
    if inputs.thermal == ThermalState::Critical {
        return WatchdogAction::Stop(StopReason::TooHot);
    }
    if inputs.elapsed >= MEETING_CAP_WARN_AT && !inputs.cap_warned {
        return WatchdogAction::WarnApproachingCap;
    }
    WatchdogAction::Continue
}

// ── Auto-mute policy (finding #9) ───────────────────────────────────────────

/// What kind of take is asking to mute the Mac's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TakeContext {
    /// An ordinary dictation with no meeting running — YV28's original case.
    Dictation,
    /// A meeting recording.
    Meeting,
    /// A dictation hotkey pressed WHILE a meeting is recording.
    DictationDuringMeeting,
}

/// Finding #9, as one function: auto-mute is a property of the take, and only a
/// plain dictation has it.
///
/// Muting during a meeting is not a cosmetic bug. The user is on a call; YV28's
/// mute would silence the call they are recording — and, mid-meeting, silence
/// the call they are LISTENING to — with no indication of why.
pub fn auto_mute_allowed(context: TakeContext) -> bool {
    matches!(context, TakeContext::Dictation)
}

/// The context a dictation starting right now is in.
pub fn take_context() -> TakeContext {
    if meeting_capture_active() {
        TakeContext::DictationDuringMeeting
    } else {
        TakeContext::Dictation
    }
}

// ── The meeting journal (N-track, index records) ────────────────────────────

/// A meeting's lifecycle state. 22-A's `meetings` table (YV94) stores this
/// verbatim; the journal is where it is DECIDED, because the journal is the
/// only thing that survives a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingState {
    Recording,
    /// Stopped by the user, everything captured.
    Complete,
    /// Stopped by the watchdog, a crash, or a device failure — the audio up to
    /// that point is real and kept.
    Partial,
}

impl MeetingState {
    pub fn as_str(self) -> &'static str {
        match self {
            MeetingState::Recording => "recording",
            MeetingState::Complete => "complete",
            MeetingState::Partial => "partial",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "complete" => MeetingState::Complete,
            "partial" => MeetingState::Partial,
            _ => MeetingState::Recording,
        }
    }
}

/// One persisted index record (finding #12). Written about once a second.
///
/// `captured_samples` is what the DEVICE gave us (derived from the callback's
/// host-time anchors); `spilled_samples` is what actually reached the disk. The
/// two are equal in a healthy recording. When they diverge, the difference is
/// exactly the audio that was lost — and because both numbers are on disk, a
/// recovery can splice that much silence back in instead of silently shifting
/// every timestamp after it.
///
/// `host_ns` is the third number and it is not decoration: it is the anchor's
/// capture time (mach host time, rebased to the stream's first callback), and
/// it is the only one of the three that keeps running when the device stops.
/// See [`plan_silence_splices`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexRecord {
    pub host_ns: u64,
    pub captured_samples: u64,
    pub spilled_samples: u64,
}

/// A repair the recovery must make: `silence_samples` of silence belong at
/// `at_sample` in the finalized track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Splice {
    pub at_sample: u64,
    pub silence_samples: u64,
}

/// Finding #12's whole point, as a pure function: turn a sequence of index
/// records into the list of gaps that must be spliced.
///
/// Without this, one dropped chunk in a three-hour meeting shifts every
/// timestamp after it — and with them every speaker label and every evidence
/// reference downstream — with nothing anywhere recording that it happened.
///
/// A gap is found by TWO independent rules, because there are two independent
/// ways for audio to be missing and each rule is blind to the other's failure:
///
/// * **The counters.** `captured` (what the device delivered, from the anchors)
///   minus `spilled` (what the journal accepted). This catches the losses that
///   happen to audio that ARRIVED: a full RT ring, a full journal queue. Both
///   numbers move, and their difference is exact — so the threshold is one
///   millisecond ([`SPLICE_MIN_SAMPLES`]).
/// * **The wall clock.** How much audio the interval's elapsed `host_ns` says
///   should exist, minus what reached the spill. This catches the loss the
///   counters CANNOT see: a device that stops delivering. No callback fires, so
///   `captured` and `spilled` freeze in perfect agreement — and a detector
///   reading only the counters reports a clean recording while the finalized
///   track quietly loses the stall and shifts everything after it. Comparing
///   two different clocks has real jitter in it, so this rule's threshold is
///   [`STALL_MIN_SAMPLES`] (250 ms) and its per-interval ceiling is
///   [`MAX_STALL_SPLICE_SAMPLES`].
///
/// The two never stack: a stall's shortfall already contains any drop inside
/// the same interval, so the interval splices the LARGER of the two, not their
/// sum.
///
/// The wall-clock rule needs a previous record to measure from, so it is not
/// applied to the first record in the list — which is why
/// [`MeetingCapture::accept`] writes one on its very first block instead of
/// waiting out the first cadence: that record is the origin, and without it the
/// first second of every meeting would be the one second a stall could hide in.
pub fn plan_silence_splices(records: &[IndexRecord]) -> Vec<Splice> {
    let mut splices = Vec::new();
    let mut prev = IndexRecord {
        host_ns: 0,
        captured_samples: 0,
        spilled_samples: 0,
    };
    // `prev` starts as a synthetic zero so the counter rule can measure the
    // first record against the start of the recording. The host clock has no
    // such origin — record 0's `host_ns` is an arbitrary stream-relative
    // instant, not zero — so the wall-clock rule waits for a real predecessor.
    let mut have_origin = false;
    for record in records {
        let captured = record
            .captured_samples
            .saturating_sub(prev.captured_samples);
        let spilled = record.spilled_samples.saturating_sub(prev.spilled_samples);

        let dropped = captured.saturating_sub(spilled);
        let dropped = if dropped >= SPLICE_MIN_SAMPLES {
            dropped
        } else {
            0
        };

        // Monotonic `host_ns` only: a stream rebuilt mid-meeting rebases its
        // epoch, and an interval that appears to run backwards is a changed
        // clock rather than a negative gap. Skipping it loses one interval of
        // coverage; believing it would invent one.
        let stalled = if have_origin && record.host_ns > prev.host_ns {
            let elapsed_ns = (record.host_ns - prev.host_ns) as u128;
            let expected = (elapsed_ns * TARGET_RATE as u128 / 1_000_000_000) as u64;
            let short = expected.saturating_sub(spilled);
            if short >= STALL_MIN_SAMPLES {
                short.min(MAX_STALL_SPLICE_SAMPLES)
            } else {
                0
            }
        } else {
            0
        };

        // The larger, never the sum: a stall's wall-clock shortfall already
        // accounts for anything dropped inside the same interval, so adding
        // them would splice the same missing audio twice.
        let silence_samples = dropped.max(stalled);
        if silence_samples > 0 {
            splices.push(Splice {
                at_sample: prev.spilled_samples,
                silence_samples,
            });
        }
        prev = *record;
        have_origin = true;
    }
    splices
}

/// Apply the splices to a decoded track — the readable SPECIFICATION of what a
/// repair does. Production streams instead (see [`spill_to_wav`]: a three-hour
/// spill must never be materialized in RAM), and
/// `the_streaming_finalize_agrees_with_the_pure_splice_spec` is what keeps the
/// two from drifting apart.
#[cfg(test)]
fn apply_splices(samples: &[f32], splices: &[Splice]) -> Vec<f32> {
    if splices.is_empty() {
        return samples.to_vec();
    }
    let extra: u64 = splices.iter().map(|s| s.silence_samples).sum();
    let mut out = Vec::with_capacity(samples.len() + extra as usize);
    let mut cursor = 0usize;
    for splice in splices {
        let at = (splice.at_sample as usize).min(samples.len());
        if at > cursor {
            out.extend_from_slice(&samples[cursor..at]);
            cursor = at;
        }
        out.resize(out.len() + splice.silence_samples as usize, 0.0f32);
    }
    if cursor < samples.len() {
        out.extend_from_slice(&samples[cursor..]);
    }
    out
}

enum JournalMsg {
    Audio { track: usize, samples: Vec<i16> },
    State(MeetingState),
}

/// Index records waiting for the writer thread.
///
/// They do NOT ride the audio queue, and that is the whole point. The queue is
/// bounded because a meeting must ride out a disk hiccup without unbounded
/// memory — but a full queue is exactly the moment a gap is being created, and
/// sending the record that DESCRIBES the gap through the same full queue loses
/// it precisely when it matters. Instead the record lands here (a few dozen
/// bytes, once a second) and the writer drains it on its own flush timer.
type PendingIndex = Arc<Mutex<Vec<IndexRecord>>>;

/// The in-progress journal for ONE meeting, generalized over N tracks (N = 1
/// for 22-A; the two-track shape is what 22-B needs and costs nothing now).
///
/// Same additive rule as YV63's dictation journal: every failure — open, marker
/// write, spawn, full queue, write error — degrades to the behaviour that
/// existed before it and is COUNTED, never silent.
pub struct MeetingJournal {
    id: String,
    marker: PathBuf,
    spills: Vec<PathBuf>,
    index: PathBuf,
    tx: Option<mpsc::SyncSender<JournalMsg>>,
    pending_index: PendingIndex,
    writer: Option<thread::JoinHandle<()>>,
    dropped: AtomicU64,
    /// TEST SEAM — see [`MeetingJournal::pause_handle`].
    writer_pause: Arc<AtomicBool>,
    keep: bool,
}

impl MeetingJournal {
    /// Open a journal for `tracks` tracks under `dir`. `None` only if the writer
    /// thread cannot be spawned.
    pub fn start(dir: &Path, tracks: usize) -> Option<Self> {
        Self::start_with_depth(dir, tracks, MEETING_QUEUE_DEPTH)
    }

    /// Open a journal with an explicit queue depth.
    ///
    /// Public because finding #12's failure — the bounded queue REJECTING a
    /// chunk — has to be produced by the real `append` path in a test, not by
    /// hand-written index records that merely claim to be what the live journal
    /// writes. A depth of 1 plus [`MeetingJournal::pause_handle`] makes that
    /// rejection deterministic.
    pub fn start_with_depth(dir: &Path, tracks: usize, depth: usize) -> Option<Self> {
        let tracks = tracks.max(1);
        let id = Uuid::new_v4().to_string();
        let marker = dir.join(format!("{id}.{MEETING_MARKER_EXT}"));
        let index = dir.join(format!("{id}.{MEETING_INDEX_EXT}"));
        let spills: Vec<PathBuf> = (0..tracks)
            .map(|t| dir.join(format!("{id}.t{t}.{MEETING_SPILL_EXT}")))
            .collect();
        let (tx, rx) = mpsc::sync_channel::<JournalMsg>(depth.max(1));
        let writer_pause = Arc::new(AtomicBool::new(false));
        let pending_index: PendingIndex = Arc::new(Mutex::new(Vec::new()));
        let (wdir, wspills, wmarker, windex, wpause, wpending) = (
            dir.to_path_buf(),
            spills.clone(),
            marker.clone(),
            index.clone(),
            Arc::clone(&writer_pause),
            Arc::clone(&pending_index),
        );
        let Ok(writer) = thread::Builder::new()
            .name("wv-meeting-journal".into())
            .spawn(move || {
                meeting_writer_loop(&wdir, &wspills, &wmarker, &windex, rx, &wpause, &wpending)
            })
        else {
            log::warn!("YV91 meeting journal off (writer thread)");
            return None;
        };
        Some(Self {
            id,
            marker,
            spills,
            index,
            tx: Some(tx),
            pending_index,
            writer: Some(writer),
            dropped: AtomicU64::new(0),
            writer_pause,
            keep: false,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// TEST SEAM. A handle that parks this journal's writer thread, so the
    /// bounded queue fills and the next `append` is REJECTED — finding #12's
    /// failure, on demand.
    ///
    /// Without it the only way to observe a queue drop is to out-run a disk,
    /// which is a race, and a test that races is a test that is eventually
    /// deleted — or, worse, replaced by hand-written index records that only
    /// CLAIM to be what the live journal writes. The park is self-limiting
    /// ([`WRITER_PAUSE_CAP`]) so a forgotten un-park can never wedge a build.
    pub fn pause_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.writer_pause)
    }

    /// Hand one track's 16 kHz frames to the writer. Never blocks.
    ///
    /// Returns how many samples the writer ACCEPTED — 0 when the bounded queue
    /// rejected the chunk. That return value is finding #12's whole mechanism:
    /// the caller advances its `spilled` counter by what was accepted, so a
    /// rejected chunk leaves `spilled` behind `captured` and the divergence
    /// reaches the index record — and from there, the silence splice. When this
    /// returned nothing, the drop was counted here and then forgotten by every
    /// caller, which is precisely how a "detected" gap stayed invisible — so
    /// the return is `#[must_use]`: ignoring it is the bug.
    #[must_use = "advance `spilled` by what the journal ACCEPTED — ignoring this \
                  is what made a dropped chunk invisible to the gap detector"]
    pub fn append(&self, track: usize, samples: &[f32]) -> usize {
        if samples.is_empty() {
            return 0;
        }
        let Some(tx) = self.tx.as_ref() else { return 0 };
        let msg = JournalMsg::Audio {
            track,
            samples: samples.iter().map(|&s| to_i16(s)).collect(),
        };
        if tx.try_send(msg).is_err() {
            self.dropped
                .fetch_add(samples.len() as u64, Ordering::Relaxed);
            return 0;
        }
        samples.len()
    }

    /// Persist one index record (finding #12).
    ///
    /// Queued separately from the audio — see `PendingIndex`: the record that
    /// documents a drop must survive the condition that caused it.
    pub fn index(&self, record: IndexRecord) {
        self.pending_index.lock().push(record);
    }

    /// Samples this journal could not hand to the writer.
    pub fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Close the meeting: stamp the final state, wait for the writer to flush,
    /// then finalize the spills into wavs through the SAME code the crash
    /// recovery uses — so a normal stop and a recovered crash produce byte-wise
    /// identical artifacts, and the recovery path is exercised on every stop.
    pub fn finalize(mut self, state: MeetingState) -> Option<FinalizedMeeting> {
        if let Some(tx) = self.tx.as_ref() {
            let _ = tx.send(JournalMsg::State(state));
        }
        self.hang_up();
        let marker = self.marker.clone();
        self.keep = true; // the finalizer removes the marker itself
        match finalize_meeting_marker(&marker) {
            Ok(finalized) => finalized,
            Err(e) => {
                log::warn!("YV91 meeting finalize failed ({e}) — left for the next launch");
                None
            }
        }
    }

    /// Test-only: simulate the app dying mid-meeting. Leaves the marker, the
    /// spills and the index exactly as a crash leaves them.
    pub fn abandon(mut self) {
        self.keep = true;
        self.hang_up();
    }

    fn hang_up(&mut self) {
        drop(self.tx.take());
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        let dropped = self.dropped_samples();
        if dropped > 0 {
            log::warn!(
                "YV91 meeting journal fell behind: {dropped} sample(s) dropped (spliced as silence on finalize)"
            );
        }
    }
}

impl Drop for MeetingJournal {
    fn drop(&mut self) {
        self.hang_up();
        if self.keep {
            return;
        }
        let _ = std::fs::remove_file(&self.marker);
        let _ = std::fs::remove_file(&self.index);
        for spill in &self.spills {
            let _ = std::fs::remove_file(spill);
        }
    }
}

/// Append whatever index records have accumulated, oldest first, and flush —
/// the sidecar is JSON lines so a partially-written file still parses up to its
/// last complete line, which is what a crash leaves behind.
fn write_index_records(file: Option<&mut std::fs::File>, pending: &PendingIndex) {
    let Some(file) = file else { return };
    let records: Vec<IndexRecord> = {
        let mut guard = pending.lock();
        if guard.is_empty() {
            return;
        }
        std::mem::take(&mut *guard)
    };
    for record in records {
        let line = serde_json::json!({
            "host_ns": record.host_ns,
            "captured_samples": record.captured_samples,
            "spilled_samples": record.spilled_samples,
        });
        let _ = writeln!(file, "{line}");
    }
    let _ = file.flush();
}

/// The meeting journal's writer thread. Creates the spills, writes the marker
/// FIRST (that pair is the crash signal), then appends audio and index records
/// with a BATCHED flush (finding #12: a timer, not every callback).
fn meeting_writer_loop(
    dir: &Path,
    spills: &[PathBuf],
    marker: &Path,
    index: &Path,
    rx: mpsc::Receiver<JournalMsg>,
    pause: &AtomicBool,
    pending_index: &PendingIndex,
) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!("YV91 meeting journal off (recovery dir): {e}");
        return;
    }
    let mut files = Vec::with_capacity(spills.len());
    for spill in spills {
        match std::fs::File::create(spill) {
            Ok(file) => files.push(std::io::BufWriter::new(file)),
            Err(e) => {
                log::warn!("YV91 meeting journal off (spill): {e}");
                return;
            }
        }
    }
    let meta = serde_json::json!({
        "version": 1,
        "kind": "meeting",
        "started_at": chrono::Utc::now().to_rfc3339(),
        "sample_rate": TARGET_RATE,
        "state": MeetingState::Recording.as_str(),
        "index": index.to_string_lossy(),
        "tracks": spills
            .iter()
            .enumerate()
            .map(|(i, p)| serde_json::json!({ "track": i, "spill": p.to_string_lossy() }))
            .collect::<Vec<_>>(),
    });
    if let Err(e) = std::fs::write(marker, meta.to_string()) {
        log::warn!("YV91 meeting journal off (marker): {e}");
        for spill in spills {
            let _ = std::fs::remove_file(spill);
        }
        return;
    }

    let mut index_file = std::fs::File::create(index).ok();
    let mut last_flush = Instant::now();
    let mut buf: Vec<u8> = Vec::new();
    let mut final_state = MeetingState::Partial;
    loop {
        // TEST SEAM (see `MeetingJournal::pause_handle`): parked, and bounded,
        // so the queue fills deterministically and a forgotten un-pause costs a
        // few seconds rather than the build.
        if pause.load(Ordering::SeqCst) {
            let parked_at = Instant::now();
            while pause.load(Ordering::SeqCst) && parked_at.elapsed() < WRITER_PAUSE_CAP {
                thread::sleep(Duration::from_millis(1));
            }
        }
        match rx.recv_timeout(JOURNAL_FLUSH_INTERVAL) {
            Ok(JournalMsg::Audio { track, samples }) => {
                let Some(file) = files.get_mut(track) else {
                    continue;
                };
                buf.clear();
                buf.reserve(samples.len() * 2);
                for sample in samples {
                    buf.extend_from_slice(&sample.to_le_bytes());
                }
                if let Err(e) = file.write_all(&buf) {
                    log::warn!("YV91 meeting spill write stopped: {e}");
                    return;
                }
            }
            Ok(JournalMsg::State(state)) => final_state = state,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        write_index_records(index_file.as_mut(), pending_index);
        if last_flush.elapsed() >= JOURNAL_FLUSH_INTERVAL {
            for file in files.iter_mut() {
                let _ = file.flush();
            }
            last_flush = Instant::now();
        }
    }
    // The last records — including the one `MeetingCapture::close` wrote to
    // size the final splice — are written before the spills are flushed shut.
    write_index_records(index_file.as_mut(), pending_index);
    for file in files.iter_mut() {
        let _ = file.flush();
    }
    // Stamp the state the session ended in, so a startup scan can tell a
    // watchdog stop (partial, but deliberate) from a crash (partial, discovered).
    if let Ok(raw) = std::fs::read_to_string(marker) {
        if let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&raw) {
            meta["state"] = serde_json::Value::String(final_state.as_str().to_string());
            let _ = std::fs::write(marker, meta.to_string());
        }
    }
}

/// A meeting turned back into playable audio — by a normal stop, or by the
/// startup scan after a crash. Both go through the same function.
#[derive(Debug, Clone, PartialEq)]
pub struct FinalizedMeeting {
    pub id: String,
    /// One wav per track, in track order.
    pub tracks: Vec<PathBuf>,
    pub seconds: f64,
    pub state: MeetingState,
    /// How much silence had to be spliced for chunks that never reached disk.
    /// Non-zero means the recording is honest about a gap rather than shifted.
    pub spliced_silence_samples: u64,
}

fn is_meeting_marker(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(MEETING_MARKER_EXT) && lower.len() > MEETING_MARKER_EXT.len() + 1
}

/// Startup scan (matrix row #6): finalize every orphaned meeting journal into
/// playable wavs. An orphan is any marker still on disk — a normal stop
/// finalizes and removes its own.
pub fn recover_orphaned_meetings(dir: &Path) -> Vec<FinalizedMeeting> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut recovered = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        if !is_meeting_marker(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let marker = entry.path();
        match finalize_meeting_marker(&marker) {
            Ok(Some(meeting)) => recovered.push(meeting),
            Ok(None) => {}
            Err(e) => log::warn!(
                "YV91 orphaned meeting {} not recovered ({e}) — left for the next launch",
                marker.display()
            ),
        }
    }
    recovered
}

/// Read the index sidecar back. Missing/partial lines are skipped: a truncated
/// last line is what a crash leaves, and it must not cost the recording.
fn read_index_records(path: &Path) -> Vec<IndexRecord> {
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

/// Stream one track's spill into a wav, splicing silence where the index
/// records say audio was lost. Constant memory regardless of how long the
/// meeting was: a fixed read buffer in, hound's writer out.
///
/// Returns how many samples the wav holds (0 = the spill had no audio, which is
/// a stray start rather than a recording).
fn spill_to_wav(
    spill: &Path,
    wav: &Path,
    sample_rate: u32,
    splices: &[Splice],
) -> Result<u64, String> {
    let Ok(file) = std::fs::File::open(spill) else {
        return Ok(0);
    };
    if std::fs::metadata(spill).map(|m| m.len()).unwrap_or(0) < 2 {
        return Ok(0);
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::create(wav, spec).map_err(|e| format!("wav {}: {e}", wav.display()))?;
    let mut reader = BufReader::new(file);
    let mut buf = [0u8; 8192];
    // `spill_pos` indexes the SPILL (which is what the splices are keyed on);
    // `written` counts what lands in the wav, and the two diverge by exactly the
    // silence spliced in.
    let mut spill_pos: u64 = 0;
    let mut written: u64 = 0;
    let mut next_splice = 0usize;
    let mut carry: Option<u8> = None;
    loop {
        let read = reader
            .read(&mut buf)
            .map_err(|e| format!("read {}: {e}", spill.display()))?;
        if read == 0 {
            break;
        }
        let mut idx = 0usize;
        while idx < read {
            let (lo, hi) = match carry.take() {
                Some(lo) => {
                    let hi = buf[idx];
                    idx += 1;
                    (lo, hi)
                }
                None => {
                    if idx + 1 >= read {
                        carry = Some(buf[idx]);
                        break;
                    }
                    let pair = (buf[idx], buf[idx + 1]);
                    idx += 2;
                    pair
                }
            };
            while next_splice < splices.len() && splices[next_splice].at_sample == spill_pos {
                for _ in 0..splices[next_splice].silence_samples {
                    writer.write_sample(0i16).map_err(|e| e.to_string())?;
                    written += 1;
                }
                next_splice += 1;
            }
            writer
                .write_sample(i16::from_le_bytes([lo, hi]))
                .map_err(|e| e.to_string())?;
            spill_pos += 1;
            written += 1;
        }
    }
    // Any splice at (or past) the end of the spill still belongs in the wav.
    while next_splice < splices.len() {
        for _ in 0..splices[next_splice].silence_samples {
            writer.write_sample(0i16).map_err(|e| e.to_string())?;
            written += 1;
        }
        next_splice += 1;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(written)
}

/// Turn ONE meeting marker into wavs. Shared by the normal stop and the crash
/// recovery, on purpose — see [`MeetingJournal::finalize`].
fn finalize_meeting_marker(marker: &Path) -> Result<Option<FinalizedMeeting>, String> {
    let raw = std::fs::read_to_string(marker).map_err(|e| e.to_string())?;
    let meta: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let sample_rate = meta
        .get("sample_rate")
        .and_then(|v| v.as_u64())
        .filter(|r| *r > 0)
        .unwrap_or(TARGET_RATE as u64) as u32;
    // A marker whose state never advanced past `recording` is a CRASH: nobody
    // stamped it. Either way the audio is partial — only a clean stop says so.
    let state = meta
        .get("state")
        .and_then(|v| v.as_str())
        .map(MeetingState::from_str)
        .unwrap_or(MeetingState::Recording);
    let state = match state {
        MeetingState::Complete => MeetingState::Complete,
        _ => MeetingState::Partial,
    };
    let index_path = meta
        .get("index")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| marker.with_extension("index.jsonl"));
    let splices = plan_silence_splices(&read_index_records(&index_path));
    let spliced_silence_samples: u64 = splices.iter().map(|s| s.silence_samples).sum();
    // `complete` has to mean "nothing was lost", or it means nothing. A meeting
    // that needed silence spliced into it lost audio — to a full queue, a full
    // ring, or a device that stopped delivering — and the user is entitled to
    // know that from the row rather than from counting samples. YV94's list
    // shows `partial` with the quality note; a clean stop that happens to have
    // a hole in it is still a hole.
    let state = if spliced_silence_samples > 0 {
        MeetingState::Partial
    } else {
        state
    };

    let name = marker.file_name().unwrap_or_default().to_string_lossy();
    let id = name
        .strip_suffix(&format!(".{MEETING_MARKER_EXT}"))
        .unwrap_or(&name)
        .to_string();

    let tracks = meta
        .get("tracks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut wavs = Vec::new();
    let mut seconds = 0.0f64;
    for (i, track) in tracks.iter().enumerate() {
        let Some(spill) = track
            .get("spill")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
        else {
            continue;
        };
        let wav_path = marker.with_file_name(format!("{id}.t{i}.wav"));
        // Finding #1 again, at the other end: a three-hour spill is ~345 MB and
        // reading it into a `Vec<f32>` to write a wav would cost 690 MB — the
        // exact class of thing this item exists to delete. The finalize streams.
        match spill_to_wav(&spill, &wav_path, sample_rate, &splices) {
            Ok(0) => continue,
            Ok(written) => {
                seconds = seconds.max(written as f64 / sample_rate as f64);
                wavs.push(wav_path);
            }
            Err(e) => {
                // Never leave a half-written wav; the marker stays so the next
                // launch retries rather than losing hours of audio.
                let _ = std::fs::remove_file(&wav_path);
                return Err(e);
            }
        }
    }
    // Only now is the spill redundant.
    for track in tracks.iter() {
        if let Some(spill) = track.get("spill").and_then(|v| v.as_str()) {
            let _ = std::fs::remove_file(spill);
        }
    }
    let _ = std::fs::remove_file(&index_path);
    let _ = std::fs::remove_file(marker);
    if wavs.is_empty() {
        return Ok(None);
    }
    Ok(Some(FinalizedMeeting {
        id,
        tracks: wavs,
        seconds,
        state,
        spliced_silence_samples,
    }))
}

// ── The real-time capture hand-off ──────────────────────────────────────────

/// Everything the audio callback touches, and nothing else. Preallocated at
/// stream open; the callback only ever stores into it.
pub struct RtCapture {
    /// Interleaved native-rate frames, exactly as the device delivered them.
    pub samples: SpscRing<f32>,
    /// One [`CaptureAnchor`] per callback.
    pub anchors: SpscRing<CaptureAnchor>,
    channels: u16,
    sample_rate: u32,
    /// Cumulative per-channel frames the device has DELIVERED — advanced by
    /// what arrived, never by what fit.
    ///
    /// This is the number that stamps every [`CaptureAnchor`], and therefore
    /// the number the journal's `captured_samples` is derived from. Advancing
    /// it by the ACCEPTED count instead (what this used to do) shifts every
    /// anchor after a ring overrun down by exactly the amount that was lost, so
    /// `captured` collapses back onto `spilled` and the loss disappears from
    /// the index — the same gap the queue-drop path had, arriving by a second,
    /// independent route.
    delivered: AtomicU64,
    /// Cumulative per-channel frames that actually FIT in the ring. This is the
    /// consumer's clock: `capture_consumer_loop`'s `pos` advances by what it
    /// drains, so the dictation arm window must be stamped in this space or a
    /// take would be cut at the wrong frame after any overrun.
    accepted: AtomicU64,
    /// Set if a callback ever unwound. The FFI boundary in 22-B is
    /// `extern "C-unwind"`, where a panic is undefined behaviour rather than a
    /// `Result` — so the body catches, flags, and lets the watchdog decide.
    panicked: AtomicBool,
}

impl RtCapture {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            samples: SpscRing::with_capacity(RING_FRAMES),
            anchors: SpscRing::with_capacity(ANCHOR_RING_LEN),
            channels: channels.max(1),
            sample_rate,
            delivered: AtomicU64::new(0),
            accepted: AtomicU64::new(0),
            panicked: AtomicBool::new(false),
        }
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Per-channel frames the device has DELIVERED so far, whether or not they
    /// fit. The anchors — and so the gap detector — are stamped in this space.
    pub fn frames_delivered(&self) -> u64 {
        self.delivered.load(Ordering::Acquire)
    }

    /// Per-channel frames that made it into the ring. The consumer's space:
    /// the dictation arm window is opened and closed against this.
    pub fn frames_accepted(&self) -> u64 {
        self.accepted.load(Ordering::Acquire)
    }

    /// Per-channel frames lost to a full ring — `delivered - accepted`, the
    /// same loss `overruns()` reports in interleaved samples.
    pub fn frames_lost(&self) -> u64 {
        self.frames_delivered()
            .saturating_sub(self.frames_accepted())
    }

    pub fn callback_panicked(&self) -> bool {
        self.panicked.load(Ordering::Relaxed)
    }

    /// Interleaved samples the ring could not hold — the consumer fell behind.
    pub fn overruns(&self) -> u64 {
        self.samples.overruns()
    }
}

/// THE CAPTURE CALLBACK BODY (OS-7).
///
/// Three things and return: one anchor into the anchor ring, the frames into
/// the sample ring, one counter bump. No allocation, no lock, no syscall, no
/// logging, no unbounded work — `tests/meeting_capture_rt_safety.rs` installs a
/// counting global allocator and fails on a single allocation here.
///
/// `to_f32` converts the device's sample type on the way in, which is why there
/// is no scratch buffer: the scratch buffer WAS the allocation.
pub fn rt_capture_callback<T: Copy>(
    capture: &RtCapture,
    data: &[T],
    to_f32: impl Fn(T) -> f32,
    host_ns: u64,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let channels = capture.channels as usize;
        let frames = (data.len() / channels) as u32;
        // The anchor is stamped in DELIVERED space, so `sample_index + frames`
        // is what the device gave us — including anything the ring is about to
        // refuse. That is the whole point of the anchor: it is the only witness
        // to audio that existed and did not survive.
        let delivered = capture.delivered.load(Ordering::Relaxed);
        let taken = capture.accepted.load(Ordering::Relaxed);
        let accepted = capture.samples.push_mapped(data, &to_f32);
        let delivered_now = delivered + frames as u64;
        let taken_now = taken + (accepted / channels) as u64;
        // ACCEPTED first: the consumer's arm window is stamped in this space,
        // and it must never observe a frame count the ring has not published.
        capture.accepted.store(taken_now, Ordering::Release);
        capture.delivered.store(delivered_now, Ordering::Release);
        // The anchor goes LAST, carrying this callback's own loss total. It is
        // the consumer's only witness to frames that were delivered and did not
        // fit; publishing it before the push would report the previous
        // callback's loss against these frames.
        capture.anchors.push_one(CaptureAnchor {
            host_ns,
            sample_index: delivered,
            frames,
            sample_rate: capture.sample_rate,
            lost_frames: delivered_now.saturating_sub(taken_now),
        });
    }));
    if result.is_err() {
        capture.panicked.store(true, Ordering::Relaxed);
    }
}

// ── The fan-out (finding #2a) ───────────────────────────────────────────────

/// Anything that wants the drained native-rate block. The dictation path's
/// `StreamDsp` is one; the meeting is the other, and it is not reached through
/// this trait because it is not optional per-block state.
pub trait DictationSink {
    fn push_native(&mut self, interleaved: &[f32]);
}

impl DictationSink for Vec<f32> {
    fn push_native(&mut self, interleaved: &[f32]) {
        self.extend_from_slice(interleaved);
    }
}

/// Split ONE drained block between the meeting (whenever a meeting is
/// recording) and the in-flight dictation take (whenever one is armed).
///
/// This is finding #2a's fix and it is deliberately shaped as a fan-out rather
/// than as two independent captures: both consumers see the SAME block, so a
/// dictation started mid-meeting cannot reset, truncate or gap the meeting —
/// there is no shared mutable take state left for it to clobber. The meeting is
/// served FIRST and unconditionally, which is the property
/// `tests/meeting_dictation_fanout.rs` asserts.
pub fn fan_out_block(
    block: &[f32],
    anchors: &[CaptureAnchor],
    dictation: Option<&mut dyn DictationSink>,
    dictation_range: Option<std::ops::Range<usize>>,
) {
    // The meeting is served FIRST, with the WHOLE block, unconditionally. That
    // ordering is the invariant: whatever the dictation take does or does not
    // want, the meeting's audio is already accounted for.
    if let Some(meeting) = active_capture() {
        meeting.accept(block, anchors);
    }
    if let Some(sink) = dictation {
        // The take gets exactly the frames that fall inside its arm window —
        // by INDEX, never by a flag read at drain time, which would lose the
        // tail of every take still sitting in the ring at key-up.
        let slice = match dictation_range {
            Some(range) if range.end <= block.len() => &block[range],
            Some(_) => block,
            None => block,
        };
        sink.push_native(slice);
    }
}

/// The meeting's own consumer state: DSP + journal + the index cadence.
///
/// Note what is NOT here: any buffer that grows with the length of the meeting.
/// `out` is reused and truncated every block (finding #1) — the audio's only
/// home is the spill.
struct MeetingCaptureInner {
    channels: u16,
    mono: Vec<f32>,
    out: Vec<f32>,
    high_pass: Option<Biquad>,
    resampler: StreamResampler,
    journal: Option<MeetingJournal>,
    /// Cumulative 16 kHz samples the journal ACCEPTED — what is on disk.
    spilled: u64,
    /// Cumulative 16 kHz-equivalent samples the DEVICE delivered.
    captured: u64,
    /// 16 kHz-equivalent samples delivered under rates this meeting has ALREADY
    /// finished with (YV92 composition — see [`MeetingCapture::retune`]).
    ///
    /// `captured` cannot be one multiplication over a single cumulative frame
    /// count any more: an AirPods swap changes the divisor mid-meeting, and
    /// re-scaling the whole history by the NEW rate would move every second of
    /// audio recorded before the swap. So each rate epoch is converted once, at
    /// the moment it ends, and banked here.
    captured_base: u64,
    /// The native rate the CURRENT epoch's frames were captured at.
    native_rate: u32,
    /// Native-rate frames that reached this consumer IN THIS EPOCH.
    native_frames: u64,
    /// Native-rate frames the RT ring refused IN THIS EPOCH.
    lost_frames: u64,
    /// The last anchor's cumulative `lost_frames`, so the epoch total can be
    /// advanced by a DELTA. The anchors count from the ring's birth and a
    /// reopen builds a new ring, so reading the cumulative number straight off
    /// the anchor would make every reopen look like the loss un-happened.
    anchor_lost: u64,
    last_index_at: u64,
    last_host_ns: u64,
    index_written: u64,
}

/// A meeting that is currently recording. Registered globally for the duration
/// so the capture consumer can reach it without threading a handle through
/// every layer of the dictation path.
pub struct MeetingCapture {
    inner: Mutex<MeetingCaptureInner>,
    started: Instant,
    samples_16k: AtomicU64,
    blocks: AtomicU64,
    /// Milliseconds since `started` at the last accepted block. The watchdog's
    /// liveness input (OS-1) — see [`MeetingCapture::since_last_block`].
    /// Milliseconds because an `Instant` is not atomic and the watchdog must
    /// read this without taking the consumer's lock.
    last_block_ms: AtomicU64,
}

impl MeetingCapture {
    /// Public so the meeting consumer can be driven directly — by a test with
    /// a journal it controls, and (in 22-B) by a tap that owns its own track
    /// and never touches the global active-meeting slot.
    pub fn new(native_rate: u32, channels: u16, journal: Option<MeetingJournal>) -> Self {
        Self {
            inner: Mutex::new(MeetingCaptureInner {
                channels: channels.max(1),
                mono: Vec::new(),
                out: Vec::new(),
                high_pass: Biquad::high_pass(native_rate, HIGH_PASS_HZ),
                resampler: StreamResampler::new(native_rate, TARGET_RATE),
                journal,
                spilled: 0,
                captured: 0,
                captured_base: 0,
                native_rate,
                native_frames: 0,
                lost_frames: 0,
                anchor_lost: 0,
                last_index_at: 0,
                last_host_ns: 0,
                index_written: 0,
            }),
            started: Instant::now(),
            samples_16k: AtomicU64::new(0),
            blocks: AtomicU64::new(0),
            last_block_ms: AtomicU64::new(0),
        }
    }

    /// 16 kHz samples this meeting has captured. Also the meeting's clock.
    pub fn samples(&self) -> u64 {
        self.samples_16k.load(Ordering::Relaxed)
    }

    pub fn seconds(&self) -> f64 {
        self.samples() as f64 / TARGET_RATE as f64
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// 16 kHz-equivalent samples the DEVICE has delivered — the other half of
    /// the gap detector's pair (`spilled` is what reached the disk).
    pub fn captured_samples(&self) -> u64 {
        self.inner.lock().captured
    }

    /// Index records persisted so far — the gap detector's paper trail.
    pub fn index_records_written(&self) -> u64 {
        self.inner.lock().index_written
    }

    /// Blocks this meeting has consumed. A meeting's own liveness counter.
    pub fn blocks(&self) -> u64 {
        self.blocks.load(Ordering::Relaxed)
    }

    /// Wall time since this meeting last consumed a block — OS-1's liveness
    /// input, read by the watchdog once a tick.
    ///
    /// Measured from the session start until the first block arrives, on
    /// purpose: a meeting that never receives audio at all is the same failure
    /// as one that stops receiving it, and "we have not started yet" is not a
    /// state a recording gets to sit in for an hour.
    pub fn since_last_block(&self) -> Duration {
        let last = Duration::from_millis(self.last_block_ms.load(Ordering::Relaxed));
        self.started.elapsed().saturating_sub(last)
    }

    /// One drained block: downmix → high-pass → 16 kHz → spill, then FORGET it.
    pub fn accept(&self, interleaved: &[f32], anchors: &[CaptureAnchor]) {
        if interleaved.is_empty() {
            return;
        }
        let mut guard = self.inner.lock();
        // Destructured so the reused `mono`/`out` scratch buffers can be
        // borrowed at the same time as the DSP state that fills them — which is
        // what lets this whole path steady-state allocate nothing.
        let MeetingCaptureInner {
            channels,
            mono,
            out,
            high_pass,
            resampler,
            journal,
            spilled,
            captured,
            captured_base,
            native_rate,
            native_frames,
            lost_frames,
            anchor_lost,
            last_index_at,
            last_host_ns,
            index_written,
        } = &mut *guard;

        let ch = (*channels).max(1) as usize;
        mono.clear();
        if ch <= 1 {
            mono.extend_from_slice(interleaved);
        } else {
            mono.extend(
                interleaved
                    .chunks(ch)
                    .map(|c| c.iter().sum::<f32>() / c.len() as f32),
            );
        }
        if let Some(hp) = high_pass.as_mut() {
            hp.process(mono);
        }
        out.clear();
        resampler.push(mono, out);

        // Finding #12: `spilled` advances by what the journal ACCEPTED, never
        // by what we handed it. A chunk the bounded queue rejected never
        // reached the disk, so counting it here would make `captured` and
        // `spilled` agree about audio that does not exist — and a gap detector
        // whose two numbers always agree detects nothing. With no journal at
        // all there is no spill to fall behind, so the block counts in full.
        let accepted = match journal.as_ref() {
            Some(journal) => journal.append(0, out),
            None => out.len(),
        };
        *spilled += accepted as u64;
        self.samples_16k
            .fetch_add(out.len() as u64, Ordering::Relaxed);
        self.blocks.fetch_add(1, Ordering::Relaxed);
        // OS-1: the watchdog's proof that the device is still alive. A clock
        // read on the consumer thread, never on the callback.
        self.last_block_ms
            .store(self.started.elapsed().as_millis() as u64, Ordering::Relaxed);
        // The bounded-memory rule (finding #1), in one line: the block's audio
        // is on its way to disk, so nothing keeps it. `out` and `mono` survive
        // as CAPACITY only — the two buffers that used to grow for three hours
        // now have a ceiling of one callback.
        out.clear();
        mono.clear();

        // Finding #12: turn the callback's host-time anchors into a periodic
        // on-disk index record. `captured` is what the DEVICE delivered, so
        // audio the ring or the queue dropped shows up as a divergence here.
        //
        // Delivered frames are counted as (what reached this consumer) + (what
        // the ring refused, per the anchor's cumulative `lost_frames`) rather
        // than read straight off `anchor.sample_index + anchor.frames`. The two
        // agree, but only the first is free of the drain race: the consumer
        // drains the anchor ring and the sample ring in two separate steps, so
        // a callback landing between them makes the last anchor lead or lag the
        // block by one callback — ±10 ms of phantom divergence per record, which
        // at 16 kHz is 160 samples, ten times SPLICE_MIN_SAMPLES. A gap detector
        // that invents a gap every few minutes is worse than none, because the
        // first thing anyone does with a noisy detector is stop believing it.
        *native_frames += (interleaved.len() / ch) as u64;
        if let Some(anchor) = anchors.last() {
            // A DELTA, not the cumulative number: see `anchor_lost`.
            *lost_frames += anchor.lost_frames.saturating_sub(*anchor_lost);
            *anchor_lost = anchor.lost_frames;
            *last_host_ns = anchor.host_ns;
        }
        *captured = *captured_base
            + (*native_frames + *lost_frames) * TARGET_RATE as u64 / (*native_rate).max(1) as u64;
        // The cadence rides `captured`, not `spilled`: a queue that has stopped
        // accepting freezes `spilled`, and a cadence keyed on it would stop
        // writing the very records that make the freeze visible.
        //
        // The FIRST record is written on the first block rather than a second
        // in. It carries no divergence worth reading — it exists to be an
        // ORIGIN: `plan_silence_splices`'s wall-clock rule measures each
        // interval against its predecessor's `host_ns`, and the first record in
        // the file has none. Without this, the opening second of every meeting
        // is exactly the window a device stall could hide in.
        let cadence = TARGET_RATE as u64 * INDEX_INTERVAL.as_secs().max(1);
        if *index_written == 0 || (*captured).max(*spilled) >= *last_index_at + cadence {
            *last_index_at = (*captured).max(*spilled);
            let record = IndexRecord {
                host_ns: *last_host_ns,
                // NOT `.max(*spilled)`. That clamp forced captured >= spilled,
                // which reads as harmless rounding protection and is in fact
                // the one line that could hide a real divergence.
                captured_samples: *captured,
                spilled_samples: *spilled,
            };
            if let Some(journal) = journal.as_ref() {
                journal.index(record);
            }
            *index_written += 1;
        }
    }

    /// The input format changed under a recording meeting (YV92/OS-9 meets
    /// YV91's second consumer).
    ///
    /// A resample ratio must never survive a format change. The dictation path
    /// gets that for free — its `StreamDsp` is rebuilt by
    /// `record::open_stream_into` — but a meeting owns a SECOND resampler that
    /// nothing in YV92 knew about, so without this an AirPods swap writes the
    /// remaining two hours to the spill at the wrong rate: audio that plays
    /// back at half or double speed and, worse, times that no longer line up
    /// with the transcript.
    ///
    /// Three things happen, in this order, and the order is the point:
    ///
    /// 1. the OLD ratio's outstanding tail is flushed, because finishing it
    ///    after the retune would interpolate across the seam;
    /// 2. the epoch's frames are converted at the OLD rate and banked, so no
    ///    audio recorded before the swap is re-timed by it;
    /// 3. the resampler, the high-pass and the anchor bookkeeping are rebuilt.
    ///
    /// Step 3 includes `anchor_lost`, and that half applies to EVERY reopen,
    /// not only a rate change: the reopen builds a new ring whose cumulative
    /// loss counter restarts at zero.
    pub fn retune(&self, native_rate: u32, channels: u16) {
        let native_rate = native_rate.max(1);
        let mut guard = self.inner.lock();
        let MeetingCaptureInner {
            channels: ch,
            out,
            high_pass,
            resampler,
            journal,
            spilled,
            captured,
            captured_base,
            native_rate: rate,
            native_frames,
            lost_frames,
            anchor_lost,
            ..
        } = &mut *guard;

        out.clear();
        resampler.finish(out);
        let accepted = match journal.as_ref() {
            Some(journal) => journal.append(0, out),
            None => out.len(),
        };
        *spilled += accepted as u64;
        self.samples_16k
            .fetch_add(out.len() as u64, Ordering::Relaxed);
        out.clear();

        *captured_base +=
            (*native_frames + *lost_frames) * TARGET_RATE as u64 / (*rate).max(1) as u64;
        *captured = *captured_base;
        *native_frames = 0;
        *lost_frames = 0;
        *anchor_lost = 0;
        *rate = native_rate;
        *ch = channels.max(1);
        *high_pass = Biquad::high_pass(native_rate, HIGH_PASS_HZ);
        *resampler = StreamResampler::new(native_rate, TARGET_RATE);
    }

    /// Flush the resampler tail, write a final index record and hand the
    /// journal back to be finalized.
    pub fn close(&self) -> Option<MeetingJournal> {
        let mut guard = self.inner.lock();
        let MeetingCaptureInner {
            out,
            resampler,
            journal,
            spilled,
            captured,
            last_host_ns,
            index_written,
            ..
        } = &mut *guard;
        out.clear();
        resampler.finish(out);
        let accepted = match journal.as_ref() {
            Some(journal) => journal.append(0, out),
            None => out.len(),
        };
        *spilled += accepted as u64;
        self.samples_16k
            .fetch_add(out.len() as u64, Ordering::Relaxed);
        out.clear();
        // The last record is the one a recovery reads to size the FINAL splice,
        // so it carries the two raw numbers, unclamped, for the same reason the
        // periodic one does.
        let record = IndexRecord {
            host_ns: *last_host_ns,
            captured_samples: *captured,
            spilled_samples: *spilled,
        };
        if let Some(journal) = journal.as_ref() {
            journal.index(record);
        }
        *index_written += 1;
        journal.take()
    }
}

/// The one global slot a recording meeting occupies, as a state machine rather
/// than an `Option`.
///
/// It used to be `RwLock<Option<Arc<MeetingCapture>>>`, assigned in
/// `MeetingSession::start` and cleared in `finish`, and that shape had no way
/// to say no. A second `start` — a double-clicked tray item, a hotkey while
/// the Meetings window's own button is mid-flight — overwrote the slot: the
/// first session's watchdog, journal and power assertion all stayed alive while
/// the capture consumer quietly stopped feeding it, and because nothing had
/// asked its watchdog to stop, its `stopped_by` was `None` and it finalized
/// `state = Complete`. A meeting that lost everything after minute three
/// published itself as the state that means "nothing was lost".
///
/// `Claiming` is the state that closes the race between the check and the
/// registration, without holding the lock across the journal's disk work.
enum ActiveMeeting {
    Idle,
    /// A `start` is past the check and has not registered its capture yet.
    Claiming,
    Recording(Arc<MeetingCapture>),
}

fn active_slot() -> &'static RwLock<ActiveMeeting> {
    static ACTIVE: OnceLock<RwLock<ActiveMeeting>> = OnceLock::new();
    ACTIVE.get_or_init(|| RwLock::new(ActiveMeeting::Idle))
}

/// Reserve the slot, or report who already has it. RAII: dropping the claim
/// without installing a capture releases it, so every early return in `start`
/// (and a panic) leaves the slot free.
struct MeetingClaim {
    installed: bool,
}

impl MeetingClaim {
    /// `None` when a meeting is already recording (or already starting).
    fn acquire() -> Option<Self> {
        let mut slot = active_slot().write();
        match *slot {
            ActiveMeeting::Idle => {
                *slot = ActiveMeeting::Claiming;
                Some(Self { installed: false })
            }
            _ => None,
        }
    }

    fn install(&mut self, capture: Arc<MeetingCapture>) {
        *active_slot().write() = ActiveMeeting::Recording(capture);
        self.installed = true;
    }
}

impl Drop for MeetingClaim {
    fn drop(&mut self) {
        if self.installed {
            // The session owns the slot from here; it is released by
            // `MeetingSession::finish`, which is the only thing that knows the
            // capture has been closed.
            return;
        }
        let mut slot = active_slot().write();
        if matches!(*slot, ActiveMeeting::Claiming) {
            *slot = ActiveMeeting::Idle;
        }
    }
}

/// Release the slot, but ONLY if it still holds `capture`.
///
/// The identity check is the other half of the double-start fix: a session that
/// is no longer the registered one must not clear the slot out from under the
/// session that is. Returns whether this capture was the one registered — a
/// `false` means this meeting stopped receiving audio at some earlier,
/// unknowable point, which is exactly what [`MeetingState::Partial`] is for.
fn deregister_capture(capture: &Arc<MeetingCapture>) -> bool {
    let mut slot = active_slot().write();
    match &*slot {
        ActiveMeeting::Recording(active) if Arc::ptr_eq(active, capture) => {
            *slot = ActiveMeeting::Idle;
            true
        }
        _ => false,
    }
}

/// A process-wide turnstile for whole meeting SESSIONS.
///
/// Public, and only ever taken by tests. It has to live here because six
/// integration binaries need to share ONE lock: now that
/// [`MeetingSession::start`] refuses a second concurrent session — which is the
/// entire point — two `#[test]` threads that each start a meeting are no longer
/// independent of each other. Before, they were: the second start silently took
/// the slot, which is exactly the defect.
///
/// In production it is never contended. There is one user, one tray button and
/// one capture consumer.
pub fn session_turnstile() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// The meeting currently recording, if any.
pub fn active_capture() -> Option<Arc<MeetingCapture>> {
    match &*active_slot().read() {
        ActiveMeeting::Recording(capture) => Some(Arc::clone(capture)),
        _ => None,
    }
}

/// Is a meeting recording right now? The dictation path reads this to decide
/// its [`TakeContext`] — see [`take_context`].
pub fn meeting_capture_active() -> bool {
    matches!(*active_slot().read(), ActiveMeeting::Recording(_))
}

/// YV92/OS-9: tell the recording meeting (if there is one) that the input
/// format changed under it. A no-op when nothing is recording, which is the
/// common case — the dictation path calls this on every reopen.
///
/// See [`MeetingCapture::retune`] for why a meeting needs telling at all.
pub fn retune_active_capture(native_rate: u32, channels: u16) {
    if let Some(capture) = active_capture() {
        log::info!(
            "meeting capture retuned to {native_rate}Hz/{channels}ch after an input format change"
        );
        capture.retune(native_rate, channels);
    }
}

// ── The session ─────────────────────────────────────────────────────────────

/// The probes the session reads. Injectable so the watchdog's behaviour under
/// a full disk or a dying device is testable in milliseconds instead of hours.
pub trait CaptureEnv: Send + Sync + 'static {
    fn free_bytes(&self) -> u64;
    fn battery(&self) -> BatterySnapshot;
    fn thermal(&self) -> ThermalState;
    /// OS-9: YV92 wires `LiveStream::has_failed` in here. The watchdog that
    /// polls it — instead of only reading the flag at Arm/Disarm — is this one.
    fn device_failed(&self) -> bool {
        false
    }
}

/// The real machine.
pub struct SystemEnv {
    pub path: PathBuf,
}

impl CaptureEnv for SystemEnv {
    fn free_bytes(&self) -> u64 {
        free_disk_bytes(&self.path).unwrap_or(u64::MAX)
    }

    fn battery(&self) -> BatterySnapshot {
        probe_battery()
    }

    fn thermal(&self) -> ThermalState {
        probe_thermal()
    }
}

/// Free bytes on the volume holding `path`.
pub fn free_disk_bytes(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: `c` is a valid NUL-terminated path; `stat` is fully written
        // by the call or the call fails, which we check.
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::statvfs(c.as_ptr(), &mut stat) };
        if rc != 0 {
            return None;
        }
        Some(stat.f_bavail as u64 * stat.f_frsize)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// `pmset -g batt`, parsed. Spawning a process is fine here: this runs once at
/// start and once a minute, never on the audio path.
fn probe_battery() -> BatterySnapshot {
    #[cfg(target_os = "macos")]
    {
        match std::process::Command::new("/usr/bin/pmset")
            .args(["-g", "batt"])
            .output()
        {
            Ok(out) => parse_pmset_batt(&String::from_utf8_lossy(&out.stdout)),
            Err(_) => BatterySnapshot::default(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        BatterySnapshot::default()
    }
}

/// `NSProcessInfo.thermalState` (finding #27: log thermal transitions).
pub fn probe_thermal() -> ThermalState {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::NSProcessInfo;
        // Matched on the raw NSInteger rather than the associated constants:
        // the constant set is what Apple has shipped SO FAR, and an unknown
        // future state must degrade to `Unknown`, never to a wrong answer.
        match NSProcessInfo::processInfo().thermalState().0 {
            0 => ThermalState::Nominal,
            1 => ThermalState::Fair,
            2 => ThermalState::Serious,
            3 => ThermalState::Critical,
            _ => ThermalState::Unknown,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        ThermalState::Unknown
    }
}

/// The audio source a meeting records from, as a dependency the session OWNS
/// for its whole duration.
///
/// This exists because the alternative — a meeting that reads whatever blocks
/// the dictation path happens to fan out — is not a recording, it is a
/// coincidence. `record.rs`'s worker closes its cpal stream after 60 seconds
/// without a take (the mic indicator must not stay lit), and a meeting is
/// precisely the case with no take: nothing armed it, nothing disarms it, so a
/// meeting held only by that stream ends after one minute while the session
/// keeps counting elapsed time and finalizes a 60-second wav as `complete`.
/// Holding the stream is therefore part of starting a meeting, not a side
/// effect of a dictation having happened.
pub trait CaptureStream: Send + Sync + 'static {
    /// Open the input stream (if needed) and keep it open until `release`.
    fn hold(&self) -> Result<(), String>;
    fn release(&self);
}

/// The real microphone, held through `record.rs`'s persistent capture worker.
pub struct MicStream;

impl CaptureStream for MicStream {
    fn hold(&self) -> Result<(), String> {
        crate::record::hold_stream_for_meeting()
    }

    fn release(&self) {
        crate::record::release_stream_for_meeting();
    }
}

/// A source the CALLER drives — the session holds nothing and records whatever
/// is handed to [`fan_out_block`].
///
/// Two real users: the tests that feed synthetic blocks (so a lifecycle test
/// needs no microphone), and 22-B's CoreAudio tap, which owns its own IOProc
/// and has no cpal stream to hold. It is NOT the default, because "the caller
/// will surely feed me" is exactly the assumption that made a meeting last 60
/// seconds.
pub struct ExternalStream;

impl CaptureStream for ExternalStream {
    fn hold(&self) -> Result<(), String> {
        Ok(())
    }

    fn release(&self) {}
}

/// How a session was configured. Split out so a test can run the whole
/// lifecycle — preflight, watchdog, stop, finalize — in milliseconds.
pub struct SessionConfig {
    pub dir: PathBuf,
    pub tracks: usize,
    pub native_rate: u32,
    pub channels: u16,
    pub duration_estimate: Option<Duration>,
    pub watchdog_interval: Duration,
    pub env: Arc<dyn CaptureEnv>,
    /// The stream this meeting holds open for its whole duration.
    pub stream: Arc<dyn CaptureStream>,
}

impl SessionConfig {
    pub fn new(dir: impl Into<PathBuf>, native_rate: u32, channels: u16) -> Self {
        let dir = dir.into();
        Self {
            env: Arc::new(SystemEnv { path: dir.clone() }),
            stream: Arc::new(MicStream),
            dir,
            tracks: 1,
            native_rate,
            channels,
            duration_estimate: None,
            watchdog_interval: WATCHDOG_INTERVAL,
        }
    }
}

/// A recording meeting: the journal, the power assertion, the watchdog.
pub struct MeetingSession {
    capture: Arc<MeetingCapture>,
    stop: Arc<AtomicBool>,
    stopped_by: Arc<Mutex<Option<StopReason>>>,
    watchdog: Option<thread::JoinHandle<()>>,
    /// Released on drop — see [`crate::power`].
    power: Option<PowerAssertion>,
    /// The input stream this meeting holds open, released on every path out.
    stream: Option<Arc<dyn CaptureStream>>,
    plan: PreflightPlan,
}

impl MeetingSession {
    /// Preflight, take the power assertion, open the journal, register the
    /// capture and start the watchdog. Every failure path leaves the machine
    /// exactly as it was found.
    pub fn start(config: SessionConfig) -> Result<Self, PreflightError> {
        // FIRST, before anything that touches the machine: is a meeting already
        // recording? There is one capture consumer and one slot, so a second
        // session cannot be a second meeting — it can only take the first one's
        // audio away while every indicator on the first still says "recording".
        // The claim is held (RAII) across the whole start, so a `?` below or a
        // panic releases it.
        let mut claim = MeetingClaim::acquire().ok_or(PreflightError::AlreadyRecording)?;
        let env = Arc::clone(&config.env);
        let plan = preflight(&PreflightInputs {
            free_bytes: env.free_bytes(),
            duration_estimate: config.duration_estimate,
            battery: env.battery(),
            thermal: env.thermal(),
        })?;
        for warning in &plan.warnings {
            log::warn!("meeting preflight: {warning}");
        }
        // The audio source comes FIRST, before the journal, the marker, the
        // power assertion or the watchdog exist: a meeting that cannot hold a
        // stream must leave the machine exactly as it found it, and the
        // cheapest way to guarantee that is to have created nothing yet.
        let stream = Arc::clone(&config.stream);
        if let Err(detail) = stream.hold() {
            log::warn!("meeting refused: {detail}");
            return Err(PreflightError::NoInput { detail });
        }
        let _ = std::fs::create_dir_all(&config.dir);
        let journal = MeetingJournal::start(&config.dir, config.tracks);
        let capture = Arc::new(MeetingCapture::new(
            config.native_rate,
            config.channels,
            journal,
        ));
        claim.install(Arc::clone(&capture));

        // Finding #9: a meeting NEVER touches the Mac's output. Stated here as
        // an assertion rather than as an omission, so the rule is visible at
        // the one place a reader looks for it.
        debug_assert!(!auto_mute_allowed(TakeContext::Meeting));

        let power = PowerAssertion::prevent_idle_sleep("a meeting");
        let stop = Arc::new(AtomicBool::new(false));
        let stopped_by = Arc::new(Mutex::new(None));
        let watchdog = {
            let stop = Arc::clone(&stop);
            let stopped_by = Arc::clone(&stopped_by);
            let capture = Arc::clone(&capture);
            let interval = config.watchdog_interval;
            thread::Builder::new()
                .name("wv-meeting-watchdog".into())
                .spawn(move || {
                    let mut cap_warned = false;
                    while !stop.load(Ordering::Relaxed) {
                        // Sleep in slices, never for the whole interval: a user
                        // who stops a meeting must not wait out a 60-second
                        // timer for the journal to finalize.
                        if !sleep_unless_stopped(interval, &stop) {
                            break;
                        }
                        let inputs = WatchdogInputs {
                            elapsed: capture.elapsed(),
                            free_bytes: env.free_bytes(),
                            device_failed: env.device_failed(),
                            since_last_block: capture.since_last_block(),
                            thermal: env.thermal(),
                            cap_warned,
                        };
                        match watchdog_tick(&inputs) {
                            WatchdogAction::Continue => {}
                            WatchdogAction::WarnApproachingCap => {
                                cap_warned = true;
                                log::warn!(
                                    "meeting approaching the {} cap",
                                    human_duration(MEETING_HARD_CAP)
                                );
                            }
                            WatchdogAction::Stop(reason) => {
                                log::warn!("meeting {reason}");
                                *stopped_by.lock() = Some(reason);
                                stop.store(true, Ordering::Relaxed);
                                break;
                            }
                        }
                    }
                })
                .ok()
        };
        Ok(Self {
            capture,
            stop,
            stopped_by,
            watchdog,
            power,
            stream: Some(stream),
            plan,
        })
    }

    pub fn plan(&self) -> &PreflightPlan {
        &self.plan
    }

    pub fn capture(&self) -> &Arc<MeetingCapture> {
        &self.capture
    }

    /// Has the watchdog decided to stop this meeting? The UI polls this; the
    /// finalize still goes through [`MeetingSession::stop`].
    pub fn watchdog_stop(&self) -> Option<StopReason> {
        *self.stopped_by.lock()
    }

    /// Wait (bounded) for the watchdog to reach a verdict — used by tests and
    /// by the stop path so a watchdog stop is not raced by a user stop.
    pub fn wait_for_watchdog_stop(&self, timeout: Duration) -> Option<StopReason> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(reason) = self.watchdog_stop() {
                return Some(reason);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Stop cleanly and finalize. A watchdog stop lands as `partial`; a user
    /// stop lands as `complete`. Either way the audio captured so far is on
    /// disk as a playable wav before this returns.
    pub fn stop(mut self) -> Option<FinalizedMeeting> {
        self.finish()
    }

    fn finish(&mut self) -> Option<FinalizedMeeting> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
        // Only THIS meeting's registration is cleared, and whether it was
        // still the registered one decides the state. If some other session
        // holds the slot, this recording stopped receiving audio at a point
        // nobody can name — `Complete` would be a lie about exactly the thing
        // `Complete` is read for.
        let was_registered = deregister_capture(&self.capture);
        let state = if self.stopped_by.lock().is_some() || !was_registered {
            MeetingState::Partial
        } else {
            MeetingState::Complete
        };
        if !was_registered {
            log::warn!(
                "meeting finalized as partial: it was no longer the registered capture — \
                 audio stopped reaching it at an unknown point"
            );
        }
        let journal = self.capture.close();
        // Released only after the capture is deregistered and closed, so the
        // consumer is never left draining a stream this meeting has already
        // let go of. The stream itself lives on until the ordinary idle close,
        // which is what keeps a dictation right after a meeting warm.
        if let Some(stream) = self.stream.take() {
            stream.release();
        }
        // Releasing the power assertion is the LAST thing, and it happens on
        // every path out of here (including a panic) because it is a Drop.
        self.power.take();
        journal.and_then(|j| j.finalize(state))
    }
}

impl Drop for MeetingSession {
    fn drop(&mut self) {
        if self.watchdog.is_some() {
            // `stop` was never called (an error path, or a panic) — finalize
            // anyway so the audio is never the thing that gets lost.
            let _ = self.finish();
        }
        // A hold that outlives its session pins the microphone (and the mic
        // indicator) for the rest of the process.
        if let Some(stream) = self.stream.take() {
            stream.release();
        }
        // Identity-checked, for the same reason `finish` is: a dropped session
        // that is no longer the registered one must not deregister the meeting
        // that IS recording.
        deregister_capture(&self.capture);
    }
}

/// Sleep up to `total`, waking every 50 ms to notice a stop. Returns `false`
/// when the stop flag went up (i.e. don't tick).
fn sleep_unless_stopped(total: Duration, stop: &AtomicBool) -> bool {
    const SLICE: Duration = Duration::from_millis(50);
    let deadline = Instant::now() + total;
    while Instant::now() < deadline {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        thread::sleep(left.min(SLICE));
    }
    !stop.load(Ordering::Relaxed)
}

/// f32 → i16, saturating. Local copy of `record.rs`'s helper so the journal has
/// no reason to reach into the dictation path.
fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The active-meeting slot is process-global (the capture consumer has to
    /// reach it without a handle threaded through every layer), so the tests
    /// that start a real session take a turn rather than racing each other.
    fn session_turn() -> std::sync::MutexGuard<'static, ()> {
        // The same turnstile the integration suites take — one lock, or the
        // unit tests and the acceptance suites would serialize against two.
        session_turnstile()
    }

    // ── preflight ──────────────────────────────────────────────────────────

    #[test]
    fn a_manually_started_meeting_budgets_against_the_three_hour_cap() {
        // Finding #39: the plan's formula needs a duration estimate that a
        // manually-started meeting does not have.
        let plan = preflight(&PreflightInputs {
            free_bytes: 50 * 1024 * 1024 * 1024,
            duration_estimate: None,
            battery: BatterySnapshot::default(),
            thermal: ThermalState::Nominal,
        })
        .expect("50 GB is plenty");
        assert_eq!(plan.budgeted, MEETING_HARD_CAP);
        assert_eq!(plan.required_bytes, required_free_bytes(MEETING_HARD_CAP));
    }

    #[test]
    fn a_low_disk_refusal_carries_the_exact_free_space_number() {
        // Matrix row #4: "refuse with a clear number". A refusal the user
        // cannot act on is the failure this row exists to prevent.
        let free = 1_234_567_890u64;
        let err = preflight(&PreflightInputs {
            free_bytes: free,
            duration_estimate: None,
            battery: BatterySnapshot::default(),
            thermal: ThermalState::Nominal,
        })
        .expect_err("under the 2 GB headroom alone");
        let text = err.to_string();
        assert!(
            text.contains(&free.to_string()),
            "exact bytes missing: {text}"
        );
        assert!(text.contains("1.15 GB"), "human number missing: {text}");
        assert!(
            text.contains(&required_free_bytes(MEETING_HARD_CAP).to_string()),
            "required bytes missing: {text}"
        );
    }

    #[test]
    fn a_long_meeting_is_refused_on_a_nearly_flat_battery_but_a_short_one_warns() {
        let flat = BatterySnapshot {
            on_ac: false,
            percent: Some(11),
        };
        let long = preflight(&PreflightInputs {
            free_bytes: 50 * 1024 * 1024 * 1024,
            duration_estimate: None, // budgets to 3h → "long"
            battery: flat,
            thermal: ThermalState::Nominal,
        });
        assert!(matches!(long, Err(PreflightError::LowBattery { .. })));

        let short = preflight(&PreflightInputs {
            free_bytes: 50 * 1024 * 1024 * 1024,
            duration_estimate: Some(Duration::from_secs(20 * 60)),
            battery: flat,
            thermal: ThermalState::Nominal,
        })
        .expect("a 20-minute recording is allowed on 11%");
        assert_eq!(short.warnings.len(), 1, "but the user is told");
    }

    #[test]
    fn a_flat_battery_on_ac_power_is_not_a_problem() {
        let plan = preflight(&PreflightInputs {
            free_bytes: 50 * 1024 * 1024 * 1024,
            duration_estimate: None,
            battery: BatterySnapshot {
                on_ac: true,
                percent: Some(4),
            },
            thermal: ThermalState::Nominal,
        })
        .expect("plugged in");
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn pmset_output_parses_to_a_snapshot() {
        let battery = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=12345)\t87%; discharging; 4:32 remaining present: true";
        let snap = parse_pmset_batt(battery);
        assert!(!snap.on_ac);
        assert_eq!(snap.percent, Some(87));

        let ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=12345)\t100%; charged; 0:00 remaining present: true";
        let snap = parse_pmset_batt(ac);
        assert!(snap.on_ac);
        assert_eq!(snap.percent, Some(100));

        // A desktop reports no battery at all.
        assert_eq!(
            parse_pmset_batt("Now drawing from 'AC Power'").percent,
            None
        );
    }

    // ── watchdog ───────────────────────────────────────────────────────────

    fn healthy() -> WatchdogInputs {
        WatchdogInputs {
            elapsed: Duration::from_secs(60),
            free_bytes: 50 * 1024 * 1024 * 1024,
            device_failed: false,
            since_last_block: Duration::from_millis(20),
            thermal: ThermalState::Nominal,
            cap_warned: false,
        }
    }

    #[test]
    fn the_watchdog_leaves_a_healthy_meeting_alone() {
        assert_eq!(watchdog_tick(&healthy()), WatchdogAction::Continue);
    }

    #[test]
    fn the_watchdog_stops_below_the_one_gigabyte_floor_with_the_number() {
        let free = DISK_FLOOR_BYTES - 1;
        let action = watchdog_tick(&WatchdogInputs {
            free_bytes: free,
            ..healthy()
        });
        assert_eq!(
            action,
            WatchdogAction::Stop(StopReason::LowDisk { free_bytes: free })
        );
        assert!(action_text(action).contains(&free.to_string()));
    }

    fn action_text(action: WatchdogAction) -> String {
        match action {
            WatchdogAction::Stop(reason) => reason.to_string(),
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn the_watchdog_warns_once_then_stops_at_the_cap() {
        let warn = watchdog_tick(&WatchdogInputs {
            elapsed: MEETING_CAP_WARN_AT,
            ..healthy()
        });
        assert_eq!(warn, WatchdogAction::WarnApproachingCap);
        // Already warned → back to Continue, not a warning every 60s.
        let again = watchdog_tick(&WatchdogInputs {
            elapsed: MEETING_CAP_WARN_AT + Duration::from_secs(60),
            cap_warned: true,
            ..healthy()
        });
        assert_eq!(again, WatchdogAction::Continue);
        let stop = watchdog_tick(&WatchdogInputs {
            elapsed: MEETING_HARD_CAP,
            cap_warned: true,
            ..healthy()
        });
        assert_eq!(stop, WatchdogAction::Stop(StopReason::HardCap));
    }

    #[test]
    fn a_dead_device_outranks_the_cap() {
        // OS-9: during a 3h meeting there is no Arm, so this is the ONLY place
        // a dead capture device gets noticed before the very end.
        let action = watchdog_tick(&WatchdogInputs {
            elapsed: MEETING_HARD_CAP,
            device_failed: true,
            ..healthy()
        });
        assert_eq!(action, WatchdogAction::Stop(StopReason::DeviceFailed));
    }

    #[test]
    fn a_stalled_capture_outranks_everything_but_a_dead_device() {
        // OS-1: the failure the power assertion exists to PREVENT, seen from
        // the watchdog. Nothing else can see it — the device raises no error,
        // it simply stops calling back, so `device_failed` stays false and
        // every counter freezes in agreement.
        let action = watchdog_tick(&WatchdogInputs {
            since_last_block: CAPTURE_STALL_LIMIT,
            ..healthy()
        });
        assert_eq!(
            action,
            WatchdogAction::Stop(StopReason::CaptureStalled {
                seconds: CAPTURE_STALL_LIMIT.as_secs()
            })
        );
        let WatchdogAction::Stop(reason) = action else {
            unreachable!()
        };
        assert!(
            reason.to_string().contains("delivered nothing for 90s"),
            "the reason names the failure and how long it lasted: {reason}"
        );
        // One late tick is not a stall.
        assert_eq!(
            watchdog_tick(&WatchdogInputs {
                since_last_block: WATCHDOG_INTERVAL,
                ..healthy()
            }),
            WatchdogAction::Continue,
            "a single missed tick must not stop a healthy meeting"
        );
    }

    // ── auto-mute policy ───────────────────────────────────────────────────

    #[test]
    fn only_a_plain_dictation_may_mute_the_mac() {
        assert!(auto_mute_allowed(TakeContext::Dictation));
        assert!(!auto_mute_allowed(TakeContext::Meeting));
        assert!(!auto_mute_allowed(TakeContext::DictationDuringMeeting));
    }

    // ── gap detection ──────────────────────────────────────────────────────

    #[test]
    fn a_healthy_recording_plans_no_splices() {
        let records = [
            IndexRecord {
                host_ns: 1,
                captured_samples: 16_000,
                spilled_samples: 16_000,
            },
            IndexRecord {
                host_ns: 2,
                captured_samples: 32_000,
                spilled_samples: 32_000,
            },
        ];
        assert!(plan_silence_splices(&records).is_empty());
    }

    #[test]
    fn a_dropped_chunk_becomes_a_splice_instead_of_a_silent_offset() {
        // Finding #12: without this, every timestamp after the drop shifts and
        // nothing anywhere says so.
        let records = [
            IndexRecord {
                host_ns: 1,
                captured_samples: 16_000,
                spilled_samples: 16_000,
            },
            IndexRecord {
                host_ns: 2,
                captured_samples: 32_000,
                spilled_samples: 28_000,
            },
            IndexRecord {
                host_ns: 3,
                captured_samples: 48_000,
                spilled_samples: 44_000,
            },
        ];
        let splices = plan_silence_splices(&records);
        assert_eq!(
            splices,
            vec![Splice {
                at_sample: 16_000,
                silence_samples: 4_000
            }],
            "one gap, at the second where it happened, of exactly the size lost"
        );
    }

    /// One second of host time per record, and the sample counts to match.
    fn healthy_second(index: u64) -> IndexRecord {
        IndexRecord {
            host_ns: index * 1_000_000_000,
            captured_samples: index * TARGET_RATE as u64,
            spilled_samples: index * TARGET_RATE as u64,
        }
    }

    #[test]
    fn a_device_that_stops_delivering_is_spliced_even_though_the_counters_agree() {
        // THE case the counter rule cannot see. When the HAL stalls, no
        // callback fires: `captured` and `spilled` both freeze, agree
        // perfectly, and a detector reading only those two calls a recording
        // with a five-second hole in it healthy — then every timestamp after
        // the hole is wrong and nothing anywhere says so.
        //
        // Only `host_ns` keeps running, which is what it is for.
        let mut records = vec![healthy_second(1), healthy_second(2), healthy_second(3)];
        // …then the device delivers nothing for five seconds of wall clock,
        // and the next record lands a second of audio later…
        records.push(IndexRecord {
            host_ns: 9_000_000_000,
            captured_samples: 4 * TARGET_RATE as u64,
            spilled_samples: 4 * TARGET_RATE as u64,
        });
        // …and the recording carries on normally.
        records.push(IndexRecord {
            host_ns: 10_000_000_000,
            captured_samples: 5 * TARGET_RATE as u64,
            spilled_samples: 5 * TARGET_RATE as u64,
        });

        let splices = plan_silence_splices(&records);
        assert_eq!(
            splices,
            vec![Splice {
                // At the last sample that really existed before the stall.
                at_sample: 3 * TARGET_RATE as u64,
                // Six seconds of host time produced one second of audio, so
                // five seconds of it never existed.
                silence_samples: 5 * TARGET_RATE as u64,
            }],
            "a five-second stall must become five seconds of explicit silence"
        );
    }

    #[test]
    fn ordinary_callback_jitter_is_not_a_stall() {
        // The wall clock and the audio clock are different clocks: a record is
        // written when a block crosses the cadence, its `host_ns` was stamped
        // when the ADC captured the frames, and those differ by a callback
        // period plus whatever the scheduler did. A detector that splices for
        // that fires on every healthy recording and gets ignored.
        let records = [
            healthy_second(1),
            IndexRecord {
                // 80 ms late — four callbacks' worth of jitter.
                host_ns: 2_080_000_000,
                ..healthy_second(2)
            },
            IndexRecord {
                host_ns: 3_040_000_000,
                ..healthy_second(3)
            },
        ];
        assert!(plan_silence_splices(&records).is_empty());
    }

    #[test]
    fn a_rebased_host_clock_never_invents_a_gap() {
        // A stream rebuilt mid-meeting (YV92's format change) restarts its
        // `host_ns` epoch at zero. That interval runs backwards, which is a
        // changed clock, not a negative gap — and the interval after it is
        // measured in the NEW epoch, correctly.
        let records = [
            healthy_second(10),
            IndexRecord {
                host_ns: 0,
                captured_samples: 11 * TARGET_RATE as u64,
                spilled_samples: 11 * TARGET_RATE as u64,
            },
            IndexRecord {
                host_ns: 1_000_000_000,
                captured_samples: 12 * TARGET_RATE as u64,
                spilled_samples: 12 * TARGET_RATE as u64,
            },
        ];
        assert!(plan_silence_splices(&records).is_empty());
    }

    #[test]
    fn a_stall_and_a_drop_in_one_interval_splice_once() {
        // The wall-clock shortfall already contains the dropped chunk, so the
        // interval takes the larger of the two rules rather than their sum —
        // otherwise the same missing audio is spliced twice and the track ends
        // up longer than the meeting was.
        let records = [
            healthy_second(1),
            IndexRecord {
                host_ns: 4_000_000_000,                      // three seconds of wall clock…
                captured_samples: 2 * TARGET_RATE as u64,    // …one second delivered…
                spilled_samples: TARGET_RATE as u64 + 8_000, // …half of it spilled.
            },
        ];
        let splices = plan_silence_splices(&records);
        assert_eq!(splices.len(), 1);
        assert_eq!(
            splices[0].silence_samples,
            3 * TARGET_RATE as u64 - 8_000,
            "the interval is held open to its wall-clock length, once"
        );
    }

    #[test]
    fn an_impossible_host_gap_is_capped_rather_than_believed() {
        // A `host_ns` that jumped by a day is a clock that changed meaning, not
        // a day-long stall. Splicing it literally would write a multi-gigabyte
        // wav — a worse outcome than the gap it documents.
        let records = [
            healthy_second(1),
            IndexRecord {
                host_ns: 86_400 * 1_000_000_000,
                captured_samples: 2 * TARGET_RATE as u64,
                spilled_samples: 2 * TARGET_RATE as u64,
            },
        ];
        let splices = plan_silence_splices(&records);
        assert_eq!(splices.len(), 1);
        assert_eq!(splices[0].silence_samples, MAX_STALL_SPLICE_SAMPLES);
    }

    #[test]
    fn splicing_puts_the_silence_where_the_gap_was() {
        let samples: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let out = apply_splices(
            &samples,
            &[Splice {
                at_sample: 4,
                silence_samples: 3,
            }],
        );
        assert_eq!(out.len(), 13);
        assert_eq!(&out[..4], &samples[..4]);
        assert_eq!(&out[4..7], &[0.0, 0.0, 0.0]);
        assert_eq!(&out[7..], &samples[4..]);
    }

    #[test]
    fn the_streaming_finalize_agrees_with_the_pure_splice_spec() {
        // `apply_splices` is the readable specification; `spill_to_wav` is the
        // constant-memory implementation a three-hour spill actually goes
        // through. This is the test that keeps the two honest — otherwise the
        // spec is tested and the shipped path is not.
        let dir = tmpdir("splice-equiv");
        let spill = dir.join("t0.spill.pcm");
        let raw: Vec<i16> = (0..1_000).map(|i| (i * 7) as i16).collect();
        let bytes: Vec<u8> = raw.iter().flat_map(|s| s.to_le_bytes()).collect();
        std::fs::write(&spill, &bytes).expect("spill");

        let splices = [
            Splice {
                at_sample: 100,
                silence_samples: 40,
            },
            Splice {
                at_sample: 700,
                silence_samples: 5,
            },
        ];
        let wav = dir.join("t0.wav");
        let written = spill_to_wav(&spill, &wav, TARGET_RATE, &splices).expect("wav");
        assert_eq!(written, 1_000 + 45);

        let floats: Vec<f32> = raw.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
        let expected = apply_splices(&floats, &splices);
        let mut reader = hound::WavReader::open(&wav).expect("read back");
        let got: Vec<i16> = reader.samples::<i16>().filter_map(|s| s.ok()).collect();
        assert_eq!(got.len(), expected.len());
        for (i, (&g, &e)) in got.iter().zip(expected.iter()).enumerate() {
            let e16 = to_i16(e);
            assert!(
                (g as i32 - e16 as i32).abs() <= 1,
                "sample {i}: streamed {g} vs spec {e16}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── the journal ────────────────────────────────────────────────────────

    fn tmpdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-yv91-{tag}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_meeting_journal_round_trips_one_track_to_a_wav() {
        let dir = tmpdir("roundtrip");
        let journal = MeetingJournal::start(&dir, 1).expect("journal");
        let tone: Vec<f32> = (0..TARGET_RATE as usize)
            .map(|i| (i as f32 * 0.01).sin() * 0.5)
            .collect();
        assert_eq!(journal.append(0, &tone), tone.len());
        journal.index(IndexRecord {
            host_ns: 1_000,
            captured_samples: tone.len() as u64,
            spilled_samples: tone.len() as u64,
        });
        let finalized = journal.finalize(MeetingState::Complete).expect("finalized");
        assert_eq!(finalized.state, MeetingState::Complete);
        assert_eq!(finalized.tracks.len(), 1);
        assert!(finalized.tracks[0].exists());
        assert!(
            (finalized.seconds - 1.0).abs() < 0.01,
            "{}",
            finalized.seconds
        );
        assert_eq!(finalized.spliced_silence_samples, 0);
        // Nothing in-progress survives a clean stop.
        assert!(!marker_exists(&dir), "a clean stop leaves no crash marker");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn marker_exists(dir: &Path) -> bool {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .any(|e| is_meeting_marker(&e.file_name().to_string_lossy()))
            })
            .unwrap_or(false)
    }

    #[test]
    fn an_abandoned_meeting_is_recovered_as_partial() {
        let dir = tmpdir("orphan");
        let journal = MeetingJournal::start(&dir, 1).expect("journal");
        let tone: Vec<f32> = (0..TARGET_RATE as usize / 2).map(|_| 0.25).collect();
        assert_eq!(journal.append(0, &tone), tone.len());
        // Give the writer a moment to land the bytes, then "crash".
        thread::sleep(Duration::from_millis(400));
        journal.abandon();
        assert!(marker_exists(&dir), "a crash leaves the marker behind");

        let recovered = recover_orphaned_meetings(&dir);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].state, MeetingState::Partial);
        assert!(recovered[0].tracks[0].exists());
        assert!(!marker_exists(&dir), "recovery clears the marker");
        // Second scan finds nothing — recovery is not repeatable.
        assert!(recover_orphaned_meetings(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_tracks_round_trip_so_22b_needs_no_new_journal() {
        let dir = tmpdir("two-track");
        let journal = MeetingJournal::start(&dir, 2).expect("journal");
        let a: Vec<f32> = vec![0.5; TARGET_RATE as usize / 4];
        let b: Vec<f32> = vec![-0.5; TARGET_RATE as usize / 4];
        assert_eq!(journal.append(0, &a), a.len());
        assert_eq!(journal.append(1, &b), b.len());
        let finalized = journal.finalize(MeetingState::Complete).expect("finalized");
        assert_eq!(finalized.tracks.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── the RT callback + fan-out ──────────────────────────────────────────

    #[test]
    fn the_callback_records_one_anchor_per_invocation_with_the_host_time() {
        let capture = RtCapture::new(48_000, 2);
        let block = [0.1f32; 512]; // 256 stereo frames
        rt_capture_callback(&capture, &block, |s| s, 111);
        rt_capture_callback(&capture, &block, |s| s, 222);
        let mut anchors = Vec::new();
        capture.anchors.drain_into(&mut anchors);
        assert_eq!(anchors.len(), 2);
        assert_eq!(anchors[0].host_ns, 111);
        assert_eq!(anchors[0].sample_index, 0);
        assert_eq!(anchors[0].frames, 256);
        assert_eq!(anchors[1].host_ns, 222);
        assert_eq!(
            anchors[1].sample_index, 256,
            "the anchor's index is cumulative — this is the resume anchor too"
        );
        assert_eq!(capture.frames_delivered(), 512);
        assert_eq!(
            capture.frames_accepted(),
            512,
            "nothing was lost, so the two clocks agree"
        );
        assert!(anchors.iter().all(|a| a.lost_frames == 0));
    }

    #[test]
    fn a_full_ring_is_counted_not_dropped_silently() {
        let capture = RtCapture::new(48_000, 1);
        let block = vec![0.2f32; RING_FRAMES + 1_000];
        rt_capture_callback(&capture, &block, |s| s, 1);
        assert_eq!(capture.overruns(), 1_000);
        assert!(!capture.callback_panicked());
    }

    /// The reset that used to be destructive (`*self = Self::new(..)` in
    /// `StreamDsp::reset`) is finding #2a's bug: a dictation hotkey mid-meeting
    /// wiped the meeting. In the fan-out shape it cannot, because the meeting
    /// is not in `StreamDsp` at all — this proves the meeting's sample count is
    /// untouched by a whole dictation take starting and ending.
    #[test]
    fn a_dictation_take_mid_meeting_costs_the_meeting_nothing() {
        let _turn = session_turn();
        let dir = tmpdir("fanout");
        let session = MeetingSession::start(SessionConfig {
            // Synthetic blocks, so no microphone to hold (YV91).
            stream: Arc::new(ExternalStream),
            watchdog_interval: Duration::from_secs(3600),
            ..SessionConfig::new(&dir, 16_000, 1)
        })
        .expect("temp dir has room");

        let block: Vec<f32> = vec![0.3; 1_600];
        let anchors = |i: u64| {
            vec![CaptureAnchor {
                host_ns: i * 100_000_000,
                sample_index: i * 1_600,
                frames: 1_600,
                sample_rate: 16_000,
                lost_frames: 0,
            }]
        };
        // 5 blocks with nobody dictating…
        for i in 0..5 {
            fan_out_block(&block, &anchors(i), None, None);
        }
        let before = session.capture().samples();
        // …5 with a dictation take armed…
        let mut take: Vec<f32> = Vec::new();
        for i in 5..10 {
            fan_out_block(&block, &anchors(i), Some(&mut take), None);
        }
        // …and 5 after it ends.
        for i in 10..15 {
            fan_out_block(&block, &anchors(i), None, None);
        }
        let after = session.capture().samples();

        assert_eq!(
            take.len(),
            5 * 1_600,
            "the dictation take got its own audio"
        );
        assert!(before > 0);
        assert_eq!(
            after - before,
            10 * 1_600,
            "the meeting kept every sample through the dictation and after it"
        );
        let finalized = session.stop().expect("finalized");
        assert_eq!(finalized.state, MeetingState::Complete);
        assert_eq!(finalized.spliced_silence_samples, 0, "no gap anywhere");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_meeting_registers_and_deregisters_the_active_capture() {
        let _turn = session_turn();
        let dir = tmpdir("active");
        assert!(!meeting_capture_active());
        assert_eq!(take_context(), TakeContext::Dictation);
        let session = MeetingSession::start(SessionConfig {
            // Synthetic blocks, so no microphone to hold (YV91).
            stream: Arc::new(ExternalStream),
            watchdog_interval: Duration::from_secs(3600),
            ..SessionConfig::new(&dir, 16_000, 1)
        })
        .expect("started");
        assert!(meeting_capture_active());
        assert_eq!(
            take_context(),
            TakeContext::DictationDuringMeeting,
            "a hotkey now is a mid-meeting dictation, and must not mute"
        );
        session.stop();
        assert!(!meeting_capture_active());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── The stream a meeting owns (YV91) ───────────────────────────────────

    /// A stand-in for the microphone that records what the session asked of it.
    #[derive(Default)]
    struct FakeStream {
        holds: AtomicU64,
        releases: AtomicU64,
        refuse: Option<String>,
    }

    impl CaptureStream for FakeStream {
        fn hold(&self) -> Result<(), String> {
            self.holds.fetch_add(1, Ordering::SeqCst);
            match &self.refuse {
                Some(e) => Err(e.clone()),
                None => Ok(()),
            }
        }

        fn release(&self) {
            self.releases.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_meeting_session_holds_its_own_audio_stream_for_its_whole_length() {
        // The defect this is written against: `start` opened a journal, took
        // the power assertion and started the watchdog, but owned no stream —
        // frames only arrived if a DICTATION happened to have one open, and the
        // capture worker closes that after 60 idle seconds. A meeting with no
        // concurrent dictation therefore captured at most a minute while the
        // session went on counting elapsed time for three hours.
        let _turn = session_turn();
        let dir = tmpdir("hold");
        let stream = Arc::new(FakeStream::default());
        let session = MeetingSession::start(SessionConfig {
            stream: Arc::clone(&stream) as Arc<dyn CaptureStream>,
            watchdog_interval: Duration::from_secs(3600),
            ..SessionConfig::new(&dir, 16_000, 1)
        })
        .expect("started");
        assert_eq!(
            stream.holds.load(Ordering::SeqCst),
            1,
            "starting a meeting takes the stream, it does not hope for one"
        );
        assert_eq!(
            stream.releases.load(Ordering::SeqCst),
            0,
            "and keeps it for the whole meeting"
        );
        session.stop();
        assert_eq!(
            stream.releases.load(Ordering::SeqCst),
            1,
            "the hold ends with the meeting — otherwise the mic indicator never goes out"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── One meeting at a time (the double-start defect) ────────────────────

    /// The BLOCKING finding this section exists for.
    ///
    /// `start` used to assign the global slot unconditionally. A second start —
    /// a double-clicked tray item, a hotkey racing the Meetings window's own
    /// button — therefore *replaced* the first meeting's registration. The
    /// first session kept its journal, its watchdog, its power assertion and
    /// its UI, and stopped receiving a single frame of audio. Nothing had asked
    /// its watchdog to stop, so `stopped_by` was `None` and it finalized
    /// `state = Complete`: a recording that lost everything after minute three,
    /// published as the one state that promises nothing was lost.
    #[test]
    fn a_second_meeting_is_refused_not_silently_swapped_in() {
        let _turn = session_turn();
        let dir = tmpdir("double-start-first");
        let second_dir = tmpdir("double-start-second");
        let first = MeetingSession::start(SessionConfig {
            stream: Arc::new(ExternalStream),
            watchdog_interval: Duration::from_secs(3600),
            ..SessionConfig::new(&dir, 16_000, 1)
        })
        .expect("the first meeting starts");
        let registered = active_capture().expect("the first meeting is registered");

        let stream = Arc::new(FakeStream::default());
        let second = MeetingSession::start(SessionConfig {
            stream: Arc::clone(&stream) as Arc<dyn CaptureStream>,
            watchdog_interval: Duration::from_secs(3600),
            ..SessionConfig::new(&second_dir, 16_000, 1)
        });
        assert!(
            matches!(second, Err(PreflightError::AlreadyRecording)),
            "a second meeting must be refused, not swapped in"
        );
        assert_eq!(
            stream.holds.load(Ordering::SeqCst),
            0,
            "and refused BEFORE it touches the machine — no second hold on the mic"
        );
        assert!(
            std::fs::read_dir(&second_dir)
                .map(|mut d| d.next().is_none())
                .unwrap_or(true),
            "…and no journal, spill or marker on disk for a meeting that never was"
        );
        assert!(
            Arc::ptr_eq(&active_capture().expect("still registered"), &registered),
            "the FIRST meeting still owns the slot"
        );

        // The audio still reaches the meeting that is recording.
        let block = vec![0.25f32; 1_600];
        fan_out_block(&block, &[], None, None);
        assert!(first.capture().samples() > 0);

        let finalized = first.stop().expect("finalized");
        assert_eq!(
            finalized.state,
            MeetingState::Complete,
            "the meeting that kept its audio is the one that gets to say Complete"
        );
        assert!(!meeting_capture_active(), "and the slot is free again");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&second_dir);
    }

    /// The second half of the same finding: `finish` used to clear the slot
    /// unconditionally, so stopping EITHER session deregistered whichever
    /// meeting was recording.
    ///
    /// The claim in `start` now makes an orphan unreachable through the public
    /// API, so this drives the deregistration rule directly — a session whose
    /// registration is gone must (a) leave the slot alone and (b) finalize
    /// `Partial`, because the point at which its audio stopped arriving is not
    /// knowable.
    #[test]
    fn a_session_that_lost_the_slot_finalizes_partial_and_leaves_it_alone() {
        let _turn = session_turn();
        let dir = tmpdir("orphan-partial");
        let session = MeetingSession::start(SessionConfig {
            stream: Arc::new(ExternalStream),
            watchdog_interval: Duration::from_secs(3600),
            ..SessionConfig::new(&dir, 16_000, 1)
        })
        .expect("started");
        fan_out_block(&vec![0.25f32; 1_600], &[], None, None);

        // Force the state the old code could reach on its own.
        let usurper = Arc::new(MeetingCapture::new(16_000, 1, None));
        *active_slot().write() = ActiveMeeting::Recording(Arc::clone(&usurper));

        let finalized = session.stop().expect("finalized anyway");
        assert_eq!(
            finalized.state,
            MeetingState::Partial,
            "a meeting that stopped receiving audio at an unknown point is not Complete"
        );
        assert!(
            active_capture().is_some_and(|c| Arc::ptr_eq(&c, &usurper)),
            "and it does not deregister the meeting that IS recording"
        );
        *active_slot().write() = ActiveMeeting::Idle;
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refused start must not leave the slot half-claimed — otherwise the
    /// first double-click poisons every LATER meeting instead of only its own.
    #[test]
    fn a_refused_start_releases_its_claim() {
        let _turn = session_turn();
        let dir = tmpdir("claim-release");
        let stream = Arc::new(FakeStream {
            refuse: Some("microphone permission denied".to_string()),
            ..Default::default()
        });
        let failed = MeetingSession::start(SessionConfig {
            stream: Arc::clone(&stream) as Arc<dyn CaptureStream>,
            watchdog_interval: Duration::from_secs(3600),
            ..SessionConfig::new(&dir, 16_000, 1)
        });
        assert!(failed.is_err());
        // The claim is RAII, so the next start must succeed.
        let session = MeetingSession::start(SessionConfig {
            stream: Arc::new(ExternalStream),
            watchdog_interval: Duration::from_secs(3600),
            ..SessionConfig::new(&dir, 16_000, 1)
        })
        .expect("a failed start does not poison the slot");
        session.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── A format change under a recording meeting (YV92 × YV91) ────────────

    /// The rebase seam between YV92 (the input-format state machine) and YV91
    /// (the meeting's own consumer).
    ///
    /// YV92's rule is that a resample ratio must never survive a format change.
    /// It rebuilt the DICTATION path's ratio, which was the only one that
    /// existed when it was written. YV91 added a second resampler — the
    /// meeting's — downstream of the same device, and nothing told it. An
    /// AirPods swap at minute four therefore wrote the remaining two hours to
    /// the spill at the wrong rate.
    #[test]
    fn a_format_change_mid_meeting_rebuilds_the_meetings_ratio() {
        let capture = MeetingCapture::new(48_000, 1, None);
        // One second at 48 kHz.
        capture.accept(&vec![0.2f32; 48_000], &[]);
        let after_first = capture.samples();
        assert!(
            after_first.abs_diff(TARGET_RATE as u64) <= 2,
            "one second in is one second out: {after_first}"
        );

        // AirPods engage: the device is 24 kHz now.
        capture.retune(24_000, 1);
        capture.accept(&vec![0.2f32; 24_000], &[]);
        let after_second = capture.samples();
        assert!(
            after_second.abs_diff(2 * TARGET_RATE as u64) <= 4,
            "the second second is also one second of audio, not half of one: {after_second}"
        );
    }

    /// The other half of the same call: the meeting's gap detector counts
    /// DELIVERED frames, and a reopen hands it a brand-new ring whose
    /// cumulative counters restart at zero. Read straight, that reset reads as
    /// the loss un-happening (and, once a rate epoch closes, as an hour of
    /// phantom divergence). It must not move `captured` backwards.
    #[test]
    fn a_reopen_under_a_meeting_never_moves_the_captured_clock_backwards() {
        let capture = MeetingCapture::new(48_000, 1, None);
        let anchor = |sample_index: u64, lost: u64| CaptureAnchor {
            host_ns: sample_index * 1_000,
            sample_index,
            frames: 4_800,
            sample_rate: 48_000,
            lost_frames: lost,
        };
        capture.accept(&vec![0.2f32; 4_800], &[anchor(0, 0)]);
        capture.accept(&vec![0.2f32; 4_800], &[anchor(4_800, 960)]);
        let before = capture.captured_samples();

        // The stream is rebuilt: new ring, counters from zero.
        capture.retune(48_000, 1);
        capture.accept(&vec![0.2f32; 4_800], &[anchor(0, 0)]);
        let after = capture.captured_samples();
        assert!(
            after >= before,
            "captured must never go backwards across a reopen ({before} → {after})"
        );
        assert!(
            after.abs_diff(before + TARGET_RATE as u64 / 10) <= 4,
            "and must advance by exactly the audio the new ring delivered: {before} → {after}"
        );
    }

    #[test]
    fn a_meeting_whose_stream_refuses_never_starts() {
        // A session that starts without audio records silence while every
        // indicator says it is recording. Fail at the start, with the reason.
        let _turn = session_turn();
        let dir = tmpdir("no-input");
        let stream = Arc::new(FakeStream {
            refuse: Some("microphone permission denied".to_string()),
            ..Default::default()
        });
        let started = MeetingSession::start(SessionConfig {
            stream: Arc::clone(&stream) as Arc<dyn CaptureStream>,
            watchdog_interval: Duration::from_secs(3600),
            ..SessionConfig::new(&dir, 16_000, 1)
        });
        let Err(err) = started else {
            panic!("a meeting with no stream must not start");
        };
        assert!(
            err.to_string().contains("microphone permission denied"),
            "the refusal carries the reason: {err}"
        );
        assert!(
            !meeting_capture_active(),
            "and leaves nothing registered behind it"
        );
        assert!(
            std::fs::read_dir(&dir)
                .map(|mut d| d.next().is_none())
                .unwrap_or(true),
            "nor a marker, a spill or an index on disk"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
