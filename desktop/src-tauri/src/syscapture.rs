//! YV104 — the zero-buffer ghost watchdog for the system-audio process tap
//! (plan finding OS-4).
//!
//! ## The defect this module exists for
//!
//! Apple Developer Forums thread **825780**: `AudioHardwareCreateProcessTap`
//! "delivers all-zero buffers while system audio is audible". The failure is
//! invisible to every check the plan originally proposed — the IOProc keeps
//! firing at the expected cadence, `mHostTime`/`mSampleTime`/`mDataByteSize`
//! all look normal, the `AudioBufferList` pointers are valid, **and every
//! sample in every buffer is exactly `0.0f`** while other apps are still
//! audibly producing output through the same device. Reported magnitude on a
//! fanless M2 Air: a 51-minute session lost 60 s, 53 s and 141 s in its first
//! segment and **16 min 3 s** in its second. Correlated triggers are a
//! 44.1 ↔ 48 kHz renegotiation on the output device and a Bluetooth state
//! change (AirPods sleep/wake) *where the UID does not change* — so a
//! UID-comparison device listener (YV103's `InputFormatWatch` output half)
//! sees nothing at all, by construction.
//!
//! That breaks two things at once, and both are why this module is a separate
//! item rather than a branch in an existing one:
//!
//! 1. §2.1's denial inference — "digital silence for N seconds while a target
//!    is producing audio ⇒ permission looks denied" — describes **this bug's
//!    symptom exactly**, and this bug is far more likely than a mid-session TCC
//!    revocation. A watchdog wired that way badges a healthy meeting
//!    permission-revoked (misapplying matrix row #2) and stops trying.
//! 2. A 16-minute hole in the middle of a lecture is not a badge. It is a lost
//!    meeting.
//!
//! ## The fix, as four rules this module implements
//!
//! **(a) Rebuild first, always.** The first response to sustained tap silence is
//! the **full 7-step rebuild** — [`full_rebuild_sequence`] — never a permission
//! verdict, no matter how long the silence has run and no matter what the tap
//! has or has not delivered so far. The forum thread is explicit that a partial
//! recovery does not work: *"restarting the IOProc alone or recreating only the
//! aggregate device is not reliable — both the Process Tap and Aggregate Device
//! must be destroyed and recreated."* [`ghost_tick`] can only ever answer
//! [`TapWatchdogAction::RebuildFull`] the first time it sees silence;
//! `tests/syscapture_ghost_watchdog_rebuild_first.rs` is what keeps that true.
//!
//! **(b) Budget it and log it.** [`MAX_TAP_REBUILDS_PER_MEETING`] rebuilds per
//! meeting, and every one is recorded in [`TapRebuildLog`] — attempt number,
//! monotonic stamp, host time, how long the tap had been silent, why it fired,
//! and what the caller reported back. That log is what YV106's migration
//! persists onto the meeting row (see [`TAP_REBUILDS_COLUMN`]), so a session
//! that needed three rebuilds is diagnosable afterwards instead of silently
//! degraded.
//!
//! **(c) The denial discriminator OS-4 says does not otherwise exist:** *did
//! this tap ever deliver a non-zero sample, ever?* A TCC denial is silent from
//! sample zero and stays silent forever; the ghost is silent only **after**
//! minutes of good audio. That single bit ([`TapLiveness::ever_nonzero`])
//! separates them with no private API and no guessing, and it is a fold over
//! data YV100's ring already ships — [`crate::rtring::CaptureAnchor`] needs no
//! new field. The bit never changes what the watchdog does *first* (that is
//! rule (a)); it changes only the verdict carried out the far end, once the
//! budget is spent, which is what YV102's denied-state UI reads.
//!
//! **(d) Silence is overloaded, so enumerate the other causes.** OS-4's own
//! list, as [`SilenceCause`]: the tapped process rendering to a different
//! output device than the aggregate's main sub-device (routine with AirPods —
//! which is why YV103 lands first), everyone on the call muted, the §2.1
//! `exclusive`-flag inversion (mutating it post-init silently flips
//! include ↔ exclude), and a nil dispatch queue at
//! `AudioDeviceCreateIOProcIDWithBlock`. These are carried on the action and
//! into the log so the line a human reads afterwards says which of five things
//! it might have been, rather than asserting one of them.
//!
//! Once the budget is exhausted and the tap is still all-zero, the meeting
//! degrades to **matrix row #2's** stated behaviour — a `track_lost` marker,
//! Track A (the mic) keeps recording, a banner in the pill, and the meeting is
//! **never stopped**. [`TapWatchdogAction`] has no stop variant at all, which is
//! that rule expressed in the type rather than in a comment.
//!
//! **Two limits of the degrade, stated rather than discovered later.** The
//! budget is per *meeting*, exactly as OS-4 specifies, so a long session that
//! survives three separate holes and then meets a fourth degrades on the fourth
//! even though every rebuild before it worked — the transcript in
//! `docs/pr-screenshots/YV104/` shows that happening against the forum thread's
//! own reported timings, because it is the specified behaviour and not an
//! accident. And the degrade is final: the watchdog never re-badges. What it
//! does *not* do is turn anything off — it stops nothing, mutes nothing and
//! destroys nothing, so a tap that comes back to life after the degrade keeps
//! writing into the journal exactly as before. The verdict describes what
//! happened, not what is allowed to happen next.
//!
//! ## What is here and what is YV100's
//!
//! This module is **pure**: state in, action out, no CoreAudio, no FFI, no
//! allocation on any audio thread. The actual `AudioHardwareCreateProcessTap`
//! call sequence, the IOProc block and the aggregate-device dictionary are
//! YV100's, and YV100 is not merged at this commit. What this item lands for it
//! is [`full_rebuild_sequence`] — the seven steps in the one order every exit
//! path uses, declared **once**, in this file, so YV100 binds its FFI to this
//! list instead of writing a second copy that can drift from it. YV103's
//! `FormatChangeAction::RebuildAggregate` already names this same sequence as
//! the thing it is a decision *about*.
//!
//! (Apple's forum thread is cited by number rather than by link on purpose:
//! this module is on the meeting path and `tests/matrix_phase_offline.rs` fails
//! the build if a meeting-path module contains a URL — the standing proof
//! behind the phase's "records with Wi-Fi off" claim.)

use std::time::Duration;

use crate::rtring::CaptureAnchor;

// ── Budgets and thresholds, each declared exactly once ──────────────────────

/// OS-4's budget: three full rebuilds per meeting, then degrade honestly.
///
/// Three and not "until it works": each rebuild destroys and recreates a tap
/// *and* an aggregate device, which takes on the order of a second and emits
/// its own device-list notifications (the loop YV103's guard exists to break).
/// A tap that has been rebuilt three times and is still all-zero is not going
/// to be fixed by a fourth — it is a meeting that needs a banner and a mic
/// track, not more teardowns.
pub const MAX_TAP_REBUILDS_PER_MEETING: u32 = 3;

/// How long the tap must deliver nothing but zeros before the first rebuild.
///
/// The number is a compromise between OS-4's two failure shapes and it is
/// deliberately NOT the mic path's [`crate::meeting::CAPTURE_STALL_LIMIT`]
/// (90 s): that limit guards a *stop* decision, where being wrong costs the
/// user their meeting, so it is generous. This one guards a *rebuild*, where
/// being wrong costs about a second of tap audio and one log line, so it can
/// afford to be brisk — and OS-4's holes run to sixteen minutes, so a
/// conservative threshold here is measured in lost minutes.
///
/// It must still be long enough to survive an ordinary quiet moment: nobody
/// talking for half a minute on a call whose participants are all muted is
/// genuinely all-zero output, and rebuilding the tap under it would be a
/// self-inflicted gap. 30 s is longer than any natural conversational pause and
/// far shorter than the shortest hole the forum thread reports (53 s).
pub const TAP_SILENCE_REBUILD_AFTER: Duration = Duration::from_secs(30);

/// How long a freshly rebuilt tap is given to produce its first sample before
/// the silence clock is believed again.
///
/// Without this the watchdog would spend its whole budget in three consecutive
/// ticks: the silence duration on the tick *after* a rebuild is still whatever
/// it was on the tick before, because a tap that was just recreated has not had
/// time to deliver anything yet. The grace period is what makes the budget
/// three *attempts spread over the meeting* rather than three attempts inside
/// one minute.
pub const TAP_REBUILD_GRACE: Duration = Duration::from_secs(15);

/// How long a rebuild may claim to be in flight before the watchdog stops
/// waiting for it.
///
/// A rebuild is driven by a caller that must report back
/// ([`GhostWatchdog::finish_rebuild`]). If that report never arrives — the
/// caller panicked between steps, a CoreAudio call blocked, YV103's
/// `finish_aggregate_work` was missed — then without this timeout the watchdog
/// waits forever and the tap is dead for the rest of the meeting with no
/// banner and no log line. That is the same "deaf for the rest of the meeting"
/// failure YV103's guard release exists to prevent, arriving by another route,
/// so the expiry lives here where the budget already does. An expired rebuild
/// counts against the budget: it was an attempt, it just did not report.
pub const TAP_REBUILD_IN_FLIGHT_TIMEOUT: Duration = Duration::from_secs(20);

/// The name of the `meetings` column YV106's migration 3 adds, carrying
/// [`TapRebuildLog::to_json`].
///
/// Declared here rather than in `meetings.rs` for the reason `meeting_matrix`'s
/// docs record about thresholds: the producer of a value and the name of the
/// place it is stored drift apart when they are written down twice. YV104
/// writes the structure; YV106 adds the column that holds it.
pub const TAP_REBUILDS_COLUMN: &str = "tap_rebuilds";

// ── The 7-step rebuild sequence: declared once, here ────────────────────────

/// One step of the tap's teardown/rebuild, named for the CoreAudio call that
/// performs it.
///
/// The variants carry no data on purpose: this is the *order*, which is the
/// part that is easy to get wrong and cheap to test, and it is the part YV100's
/// `syscapture_teardown_order` test asserts against. YV100 binds each variant
/// to its `extern "C-unwind"` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TapStep {
    /// `AudioDeviceStop` — stop the IOProc before anything it touches is freed.
    AudioDeviceStop,
    /// `AudioDeviceDestroyIOProcID` — release the proc id itself.
    AudioDeviceDestroyIOProcID,
    /// `AudioHardwareDestroyAggregateDevice` — the private aggregate goes next,
    /// because it holds the tap in its tap list.
    AudioHardwareDestroyAggregateDevice,
    /// `AudioHardwareDestroyProcessTap` — and only then the tap. Destroying the
    /// tap while an aggregate still references it is the ordering bug this list
    /// exists to make impossible.
    AudioHardwareDestroyProcessTap,
    /// `AudioHardwareCreateProcessTap` — a brand new tap. OS-4 is explicit that
    /// reusing the old one does not recover.
    AudioHardwareCreateProcessTap,
    /// `AudioHardwareCreateAggregateDevice` — a brand new aggregate around it,
    /// pointed at whatever the default output device is *now*.
    AudioHardwareCreateAggregateDevice,
    /// `AudioDeviceCreateIOProcIDWithBlock` immediately followed by
    /// `AudioDeviceStart` — one step because a created-but-unstarted IOProc is
    /// exactly the silent-tap state this whole module exists to detect.
    CreateAndStartIOProc,
}

impl TapStep {
    /// Stable string for the log line and the rebuild log.
    pub fn as_str(self) -> &'static str {
        match self {
            TapStep::AudioDeviceStop => "AudioDeviceStop",
            TapStep::AudioDeviceDestroyIOProcID => "AudioDeviceDestroyIOProcID",
            TapStep::AudioHardwareDestroyAggregateDevice => "AudioHardwareDestroyAggregateDevice",
            TapStep::AudioHardwareDestroyProcessTap => "AudioHardwareDestroyProcessTap",
            TapStep::AudioHardwareCreateProcessTap => "AudioHardwareCreateProcessTap",
            TapStep::AudioHardwareCreateAggregateDevice => "AudioHardwareCreateAggregateDevice",
            TapStep::CreateAndStartIOProc => "AudioDeviceCreateIOProcIDWithBlock+AudioDeviceStart",
        }
    }

    /// Is this step part of the teardown half? The four destroy steps must all
    /// precede all three create steps, and this is how that is asserted without
    /// hard-coding indices.
    pub fn is_teardown(self) -> bool {
        matches!(
            self,
            TapStep::AudioDeviceStop
                | TapStep::AudioDeviceDestroyIOProcID
                | TapStep::AudioHardwareDestroyAggregateDevice
                | TapStep::AudioHardwareDestroyProcessTap
        )
    }
}

/// The full 7-step rebuild, in the one order every caller uses.
///
/// `AudioDeviceStop` → `AudioDeviceDestroyIOProcID` →
/// `AudioHardwareDestroyAggregateDevice` → `AudioHardwareDestroyProcessTap` →
/// `AudioHardwareCreateProcessTap` → `AudioHardwareCreateAggregateDevice` →
/// create + start a new IOProc.
///
/// This is the single declaration of that order in the codebase. YV100's tap
/// teardown, YV103's `FormatChangeAction::RebuildAggregate` handler and this
/// module's ghost watchdog all run *this* list — the backlog's rule for this
/// item is "it does not duplicate that sequence, it calls it", and a shared
/// constant is the only version of that rule a compiler can enforce.
pub const fn full_rebuild_sequence() -> [TapStep; 7] {
    [
        TapStep::AudioDeviceStop,
        TapStep::AudioDeviceDestroyIOProcID,
        TapStep::AudioHardwareDestroyAggregateDevice,
        TapStep::AudioHardwareDestroyProcessTap,
        TapStep::AudioHardwareCreateProcessTap,
        TapStep::AudioHardwareCreateAggregateDevice,
        TapStep::CreateAndStartIOProc,
    ]
}

// ── Liveness: the fold over what the tap actually delivered ─────────────────

/// What the tap has delivered, as of one watchdog tick.
///
/// `Copy` because it is carried inside `meeting::WatchdogInputs`, which is
/// `Copy` so a tick can be assembled and passed without allocating on the
/// watchdog thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapLiveness {
    /// **The discriminator.** Has this tap EVER delivered a sample that was not
    /// exactly zero, at any point in this meeting? A TCC denial is false here
    /// forever; OS-4's ghost is true here and then goes quiet.
    pub ever_nonzero: bool,
    /// Wall time since the last non-zero sample. When `ever_nonzero` is false
    /// this is time since the tap was started, which is the right reading: a
    /// tap that has never produced anything has been silent for its whole life.
    pub since_nonzero: Duration,
    /// Wall time since the IOProc last delivered a block of ANY kind, zero or
    /// not. This separates OS-4's ghost (callbacks firing, samples all zero)
    /// from a plain dead IOProc (no callbacks at all) — they look identical if
    /// you only measure silence, and they are not the same failure.
    pub since_last_block: Duration,
    /// Per-channel frames the tap has delivered in total. Zero means the IOProc
    /// has not fired once yet, which is a *starting* tap, not a silent one.
    pub frames_delivered: u64,
    /// Cumulative frames the RT ring refused, folded from
    /// [`CaptureAnchor::lost_frames`]. Not a silence signal — it is the
    /// opposite, a tap that is delivering faster than the consumer drains — and
    /// it is recorded on the rebuild so a log line can say which of the two
    /// was happening.
    pub lost_frames: u64,
}

impl TapLiveness {
    /// A tap that has been started but has not been heard from at all.
    pub fn started() -> Self {
        Self {
            ever_nonzero: false,
            since_nonzero: Duration::ZERO,
            since_last_block: Duration::ZERO,
            frames_delivered: 0,
            lost_frames: 0,
        }
    }

    /// Is the tap silent *in the sense that matters* — long enough, and with
    /// the IOProc still alive or recently alive?
    pub fn is_ghost_silent(&self) -> bool {
        self.since_nonzero >= TAP_SILENCE_REBUILD_AFTER
    }

    /// The verdict this liveness supports once every rebuild has been spent.
    ///
    /// This is rule (c), and it is the ONLY place the two are told apart. Note
    /// what it is not: it is not consulted before the budget is spent, so it
    /// can never turn a first silence into a permission badge.
    pub fn verdict(&self) -> TapVerdict {
        if self.ever_nonzero {
            TapVerdict::GhostTapUnrecovered
        } else {
            TapVerdict::PermissionLikelyDenied
        }
    }
}

/// Fold a drained block of tap samples and its anchors into a running liveness.
///
/// Consumer-side on purpose. The IOProc block itself does nothing but push into
/// the ring and return (YV100, OS-7): scanning a buffer for a non-zero sample is
/// cheap, but "cheap" is not the rule on a real-time thread, and the same scan
/// on the worker costs nothing anybody can measure.
///
/// `elapsed` is the meeting's monotonic clock at the moment this block was
/// drained; the liveness carries durations rather than timestamps so the
/// watchdog stays a pure function of two numbers.
pub fn fold_block(
    live: &mut TapLiveness,
    block: &[f32],
    anchors: &[CaptureAnchor],
    elapsed: Duration,
    last_nonzero_at: &mut Duration,
) {
    if !block.is_empty() || !anchors.is_empty() {
        live.since_last_block = Duration::ZERO;
    }
    for anchor in anchors {
        live.frames_delivered = live.frames_delivered.saturating_add(anchor.frames as u64);
        // Cumulative on the anchor, so the maximum is the total — an anchor the
        // anchor ring itself dropped loses nothing.
        live.lost_frames = live.lost_frames.max(anchor.lost_frames);
    }
    // OS-4's signature is "every sample in every buffer is exactly 0.0f", so
    // the test is exact equality with zero and not an amplitude threshold. A
    // real room at -60 dBFS is not silent to this check, and it should not be:
    // a tap delivering room tone is a tap that works.
    if block.iter().any(|s| *s != 0.0) {
        live.ever_nonzero = true;
        *last_nonzero_at = elapsed;
    }
    live.since_nonzero = elapsed.saturating_sub(*last_nonzero_at);
}

/// Advance the liveness clocks on a tick where nothing was drained.
pub fn fold_idle(
    live: &mut TapLiveness,
    elapsed: Duration,
    last_nonzero_at: Duration,
    tick: Duration,
) {
    live.since_last_block = live.since_last_block.saturating_add(tick);
    live.since_nonzero = elapsed.saturating_sub(last_nonzero_at);
}

// ── The other silence causes (rule d) ───────────────────────────────────────

/// One of OS-4's *other* explanations for an all-zero tap, so the watchdog's
/// log line is an enumeration rather than an accusation.
///
/// Represented as a bitset ([`SilenceCauses`]) so the whole set stays `Copy` and
/// can ride inside the meeting watchdog's `Copy` action type without a `Vec`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SilenceCause {
    /// The tapped process is rendering to a different output device than the
    /// aggregate's main sub-device. Routine with AirPods, and the reason YV103
    /// (the output-device rebuild) lands before this item.
    OutputRoutedElsewhere,
    /// Nobody on the call is producing audio. Not a fault at all — the most
    /// common explanation for thirty seconds of zeros, and the one a credulous
    /// heuristic mistakes for a dead tap.
    EveryoneMuted,
    /// §2.1's own gotcha: mutating `CATapDescription`'s `exclusive` flag after
    /// init silently flips include ↔ exclude, so a tap built to exclude Yap
    /// ends up capturing *only* Yap — which, with Yap silent, is all zeros.
    ExclusiveFlagInverted,
    /// `AudioDeviceCreateIOProcIDWithBlock` was handed a nil dispatch queue, so
    /// the block is installed and never invoked.
    NilDispatchQueue,
    /// The IOProc is not firing at all. Distinct from every cause above, all of
    /// which produce a healthy cadence of zero-filled buffers.
    IoProcNotFiring,
}

impl SilenceCause {
    pub fn as_str(self) -> &'static str {
        match self {
            SilenceCause::OutputRoutedElsewhere => "output_routed_elsewhere",
            SilenceCause::EveryoneMuted => "everyone_muted",
            SilenceCause::ExclusiveFlagInverted => "exclusive_flag_inverted",
            SilenceCause::NilDispatchQueue => "nil_dispatch_queue",
            SilenceCause::IoProcNotFiring => "ioproc_not_firing",
        }
    }

    const ALL: [SilenceCause; 5] = [
        SilenceCause::OutputRoutedElsewhere,
        SilenceCause::EveryoneMuted,
        SilenceCause::ExclusiveFlagInverted,
        SilenceCause::NilDispatchQueue,
        SilenceCause::IoProcNotFiring,
    ];

    const fn bit(self) -> u8 {
        match self {
            SilenceCause::OutputRoutedElsewhere => 1 << 0,
            SilenceCause::EveryoneMuted => 1 << 1,
            SilenceCause::ExclusiveFlagInverted => 1 << 2,
            SilenceCause::NilDispatchQueue => 1 << 3,
            SilenceCause::IoProcNotFiring => 1 << 4,
        }
    }
}

/// A set of [`SilenceCause`]s, as a `Copy` bitset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct SilenceCauses(u8);

impl SilenceCauses {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn insert(&mut self, cause: SilenceCause) {
        self.0 |= cause.bit();
    }

    pub fn contains(&self, cause: SilenceCause) -> bool {
        self.0 & cause.bit() != 0
    }

    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    pub fn len(&self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn iter(&self) -> impl Iterator<Item = SilenceCause> + '_ {
        SilenceCause::ALL.into_iter().filter(|c| self.contains(*c))
    }

    /// `output_routed_elsewhere|everyone_muted`, or `none` for the empty set.
    pub fn as_log_string(&self) -> String {
        if self.is_empty() {
            return "none".to_string();
        }
        self.iter()
            .map(SilenceCause::as_str)
            .collect::<Vec<_>>()
            .join("|")
    }
}

/// What the rest of the machine can tell the watchdog about *why* the tap might
/// legitimately be quiet. Every field is an observation, never a conclusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapEnvironment {
    /// Does the default output device still match the aggregate's main
    /// sub-device? YV103's `InputFormatWatch::output_device_uid` comparison
    /// answers this. `false` means the audio is going somewhere the tap is not.
    pub aggregate_matches_output_device: bool,
    /// Is anything on the machine producing output right now
    /// (`kAudioProcessPropertyIsRunningOutput` folded over the process list)?
    /// `None` when it could not be read, which is the honest default — an
    /// unreadable probe must not become evidence in either direction.
    pub system_output_active: Option<bool>,
    /// Was `CATapDescription`'s `exclusive` flag mutated after init? YV100 sets
    /// this once at tap creation; it exists because §2.1's gotcha is silent by
    /// nature and would otherwise be indistinguishable from everything else here.
    pub exclusive_flag_mutated_post_init: bool,
    /// Did `AudioDeviceCreateIOProcIDWithBlock` get a real dispatch queue?
    pub dispatch_queue_installed: bool,
}

impl Default for TapEnvironment {
    /// The benign reading: aggregate pointed at the right device, a real queue
    /// installed, no post-init mutation, and no opinion about whether anything
    /// is playing.
    fn default() -> Self {
        Self {
            aggregate_matches_output_device: true,
            system_output_active: None,
            exclusive_flag_mutated_post_init: false,
            dispatch_queue_installed: true,
        }
    }
}

/// Enumerate the plausible innocent explanations for this silence — rule (d).
///
/// This never decides anything. It is attached to the rebuild attempt and to
/// the degrade so the sentence a human reads afterwards is "the tap went quiet
/// and here are the four things that could mean", not "permission denied".
pub fn plausible_silence_causes(live: &TapLiveness, env: &TapEnvironment) -> SilenceCauses {
    let mut causes = SilenceCauses::empty();
    if !env.aggregate_matches_output_device {
        causes.insert(SilenceCause::OutputRoutedElsewhere);
    }
    // Only claimable when the probe actually answered. `None` — the common case
    // on a machine where the process list could not be read — adds nothing,
    // which is the point of it being an `Option`.
    if env.system_output_active == Some(false) {
        causes.insert(SilenceCause::EveryoneMuted);
    }
    if env.exclusive_flag_mutated_post_init {
        causes.insert(SilenceCause::ExclusiveFlagInverted);
    }
    if !env.dispatch_queue_installed {
        causes.insert(SilenceCause::NilDispatchQueue);
    }
    if live.since_last_block >= TAP_SILENCE_REBUILD_AFTER {
        causes.insert(SilenceCause::IoProcNotFiring);
    }
    causes
}

// ── The watchdog's answer ───────────────────────────────────────────────────

/// Why a rebuild fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapSilenceKind {
    /// The IOProc is firing on cadence and every sample is zero — OS-4's ghost
    /// proper.
    AllZeroBuffers,
    /// The IOProc has stopped firing altogether. Same remedy (the tap and the
    /// aggregate are both suspect), different diagnosis, so it is logged
    /// differently.
    IoProcSilent,
    /// A previous rebuild was issued and never reported back inside
    /// [`TAP_REBUILD_IN_FLIGHT_TIMEOUT`].
    PreviousRebuildTimedOut,
}

impl TapSilenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TapSilenceKind::AllZeroBuffers => "all_zero_buffers",
            TapSilenceKind::IoProcSilent => "ioproc_silent",
            TapSilenceKind::PreviousRebuildTimedOut => "previous_rebuild_timed_out",
        }
    }
}

/// The verdict carried on the degrade, once every rebuild has been spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapVerdict {
    /// The tap never delivered a non-zero sample, from sample zero to the end
    /// of the budget. That is what a TCC denial looks like, and nothing else
    /// this module can see looks like it.
    PermissionLikelyDenied,
    /// The tap delivered real audio and then went to zeros and stayed there
    /// through three full rebuilds. OS-4's bug, unrecovered. **Not** a
    /// permission problem, and the UI must not say it is.
    GhostTapUnrecovered,
}

impl TapVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            TapVerdict::PermissionLikelyDenied => "permission_likely_denied",
            TapVerdict::GhostTapUnrecovered => "ghost_tap_unrecovered",
        }
    }

    /// The sentence the pill's banner shows. Honest in both directions: the
    /// ghost's line never mentions permission, and the denial's line says
    /// "looks like" because that is the strength of the evidence.
    pub fn banner(self) -> &'static str {
        match self {
            TapVerdict::PermissionLikelyDenied => {
                "System audio was never captured — it looks like permission is off. \
                 Your microphone track is still recording."
            }
            TapVerdict::GhostTapUnrecovered => {
                "System audio stopped coming through and could not be restarted. \
                 Your microphone track is still recording."
            }
        }
    }
}

/// What the ghost watchdog wants done about the tap.
///
/// **There is no stop variant, and there never will be.** Matrix row #2's rule
/// is that a lost system-audio track degrades the meeting and never ends it —
/// Track A is still recording the person holding the Mac. Encoding that in the
/// type means no future caller can wire a tap failure to a stop by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapWatchdogAction {
    /// Nothing to do: the tap is delivering, or it is inside a grace window, or
    /// a rebuild is legitimately still in flight.
    Continue,
    /// Run [`full_rebuild_sequence`]. `attempt` is 1-based and never exceeds
    /// [`MAX_TAP_REBUILDS_PER_MEETING`].
    RebuildFull {
        attempt: u32,
        kind: TapSilenceKind,
        causes: SilenceCauses,
    },
    /// The budget is spent and the tap is still silent: write matrix row #2's
    /// `track_lost` marker, banner the pill, keep recording Track A.
    DegradeTrackLost {
        verdict: TapVerdict,
        after_rebuilds: u32,
        causes: SilenceCauses,
    },
}

impl TapWatchdogAction {
    pub fn is_rebuild(&self) -> bool {
        matches!(self, TapWatchdogAction::RebuildFull { .. })
    }

    pub fn is_degrade(&self) -> bool {
        matches!(self, TapWatchdogAction::DegradeTrackLost { .. })
    }

    /// The verdict, if this action carries one. `None` for every action except
    /// the degrade — which is the type-level statement that a rebuild is never
    /// a permission verdict.
    pub fn verdict(&self) -> Option<TapVerdict> {
        match self {
            TapWatchdogAction::DegradeTrackLost { verdict, .. } => Some(*verdict),
            _ => None,
        }
    }
}

// ── The pure tick ───────────────────────────────────────────────────────────

/// The watchdog's own state, as a `Copy` snapshot, so [`ghost_tick`] can stay a
/// pure function and the mutation stays at the caller (the same split
/// `meeting::watchdog_tick` already uses).
///
/// `Default` is derived rather than written out: the zero value of every field
/// IS the start-of-meeting state (no rebuilds, not degraded, nothing in flight,
/// no grace window), and a hand-written impl saying the same thing is one more
/// place for the two to disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GhostState {
    /// Rebuilds already issued this meeting.
    pub rebuilds_issued: u32,
    /// Has the meeting already degraded to `track_lost`? Once true the
    /// watchdog is done — it never re-badges and never rebuilds again.
    pub degraded: bool,
    /// Elapsed time at which the currently in-flight rebuild was issued, if one
    /// is in flight.
    pub rebuild_issued_at: Option<Duration>,
    /// Elapsed time until which a freshly rebuilt tap is given the benefit of
    /// the doubt.
    pub grace_until: Option<Duration>,
}

/// One tick's view of the tap, mirroring `meeting::WatchdogInputs` in shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapWatchdogInputs {
    /// The meeting's monotonic elapsed time at this tick.
    pub elapsed: Duration,
    pub liveness: TapLiveness,
    pub env: TapEnvironment,
    pub state: GhostState,
}

/// **The rule, as one function.**
///
/// Read it in order, because the order IS the policy:
///
/// 1. A meeting that already degraded stays degraded. No re-badging.
/// 2. A rebuild in flight is waited for — until it times out, at which point it
///    is treated as a spent attempt rather than as a reason to wait forever.
/// 3. A tap inside its post-rebuild grace window is left alone.
/// 4. A tap that is delivering is left alone.
/// 5. Sustained silence with budget left ⇒ **[`TapWatchdogAction::RebuildFull`],
///    unconditionally** — this is rule (a), and note that
///    [`TapLiveness::verdict`] is not called anywhere above this line, which is
///    what makes "never a permission verdict on the first silence" structural
///    rather than aspirational.
/// 6. Sustained silence with the budget spent ⇒ degrade, carrying the verdict
///    the discriminator supports.
pub fn ghost_tick(inputs: &TapWatchdogInputs) -> TapWatchdogAction {
    let TapWatchdogInputs {
        elapsed,
        liveness,
        env,
        state,
    } = inputs;

    if state.degraded {
        return TapWatchdogAction::Continue;
    }

    // A rebuild we asked for is still running. Wait for it — but not forever.
    if let Some(issued_at) = state.rebuild_issued_at {
        if elapsed.saturating_sub(issued_at) < TAP_REBUILD_IN_FLIGHT_TIMEOUT {
            return TapWatchdogAction::Continue;
        }
        let causes = plausible_silence_causes(liveness, env);
        return if state.rebuilds_issued < MAX_TAP_REBUILDS_PER_MEETING {
            TapWatchdogAction::RebuildFull {
                attempt: state.rebuilds_issued + 1,
                kind: TapSilenceKind::PreviousRebuildTimedOut,
                causes,
            }
        } else {
            TapWatchdogAction::DegradeTrackLost {
                verdict: liveness.verdict(),
                after_rebuilds: state.rebuilds_issued,
                causes,
            }
        };
    }

    // A tap that was just rebuilt has not had time to prove itself.
    if let Some(until) = state.grace_until {
        if *elapsed < until {
            return TapWatchdogAction::Continue;
        }
    }

    if !liveness.is_ghost_silent() {
        return TapWatchdogAction::Continue;
    }

    let causes = plausible_silence_causes(liveness, env);
    if state.rebuilds_issued < MAX_TAP_REBUILDS_PER_MEETING {
        let kind = if liveness.since_last_block >= TAP_SILENCE_REBUILD_AFTER {
            TapSilenceKind::IoProcSilent
        } else {
            TapSilenceKind::AllZeroBuffers
        };
        return TapWatchdogAction::RebuildFull {
            attempt: state.rebuilds_issued + 1,
            kind,
            causes,
        };
    }

    TapWatchdogAction::DegradeTrackLost {
        verdict: liveness.verdict(),
        after_rebuilds: state.rebuilds_issued,
        causes,
    }
}

// ── The rebuild log (rule b) ────────────────────────────────────────────────

/// How a rebuild attempt ended, as reported by whoever ran the seven steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapRebuildOutcome {
    /// The seven steps ran and the new IOProc started.
    Succeeded,
    /// A step returned a non-zero `OSStatus`. The step that failed is recorded
    /// alongside, because "the rebuild failed" is not a diagnosis and
    /// "`AudioHardwareCreateProcessTap` returned -4" is.
    Failed { at: TapStep, status: i32 },
    /// The caller never reported back inside [`TAP_REBUILD_IN_FLIGHT_TIMEOUT`].
    TimedOut,
}

impl TapRebuildOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            TapRebuildOutcome::Succeeded => "succeeded",
            TapRebuildOutcome::Failed { .. } => "failed",
            TapRebuildOutcome::TimedOut => "timed_out",
        }
    }
}

/// One rebuild attempt, as it is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapRebuildAttempt {
    /// 1-based, so a log with three entries reads 1, 2, 3.
    pub attempt: u32,
    /// Meeting-relative milliseconds at which the rebuild was issued. Relative
    /// rather than wall-clock so it lines up with the journal's own timeline
    /// and survives a clock change.
    pub at_ms: u64,
    /// `mach_absolute_time`-derived host nanoseconds, when the caller had one —
    /// the same anchor `CaptureAnchor::host_ns` carries, so a rebuild can be
    /// located in the recorded audio rather than only in the log.
    pub host_ns: u64,
    /// How long the tap had been silent when the rebuild fired.
    pub silent_ms: u64,
    pub kind: TapSilenceKind,
    pub causes: SilenceCauses,
    /// `None` while the attempt is in flight.
    pub outcome: Option<TapRebuildOutcome>,
}

impl TapRebuildAttempt {
    fn to_json(self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "attempt": self.attempt,
            "at_ms": self.at_ms,
            "host_ns": self.host_ns,
            "silent_ms": self.silent_ms,
            "kind": self.kind.as_str(),
            "causes": self.causes.iter().map(SilenceCause::as_str).collect::<Vec<_>>(),
            "outcome": self.outcome.map(TapRebuildOutcome::as_str),
        });
        if let Some(TapRebuildOutcome::Failed { at, status }) = self.outcome {
            value["failed_at"] = serde_json::Value::from(at.as_str());
            value["os_status"] = serde_json::Value::from(status);
        }
        value
    }
}

/// Every rebuild this meeting needed, and how it ended.
///
/// **This is the structure YV106's migration persists** into the `meetings`
/// row's [`TAP_REBUILDS_COLUMN`], in exactly the shape [`TapRebuildLog::to_json`]
/// produces. It is built here, in the item that knows why each attempt fired,
/// rather than in the item that adds the column — the column is a place to put
/// it, not a reason to shape it differently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TapRebuildLog {
    attempts: Vec<TapRebuildAttempt>,
    verdict: Option<TapVerdict>,
}

impl TapRebuildLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many rebuilds this meeting needed.
    pub fn count(&self) -> u32 {
        self.attempts.len() as u32
    }

    pub fn attempts(&self) -> &[TapRebuildAttempt] {
        &self.attempts
    }

    /// The degrade verdict, if the meeting reached one.
    pub fn verdict(&self) -> Option<TapVerdict> {
        self.verdict
    }

    /// Is the budget spent?
    pub fn budget_exhausted(&self) -> bool {
        self.count() >= MAX_TAP_REBUILDS_PER_MEETING
    }

    /// The JSON blob written to the meeting row. Versioned, like every other
    /// blob this app stores, so a later shape change is readable rather than
    /// ambiguous.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "version": 1,
            "budget": MAX_TAP_REBUILDS_PER_MEETING,
            "count": self.count(),
            "verdict": self.verdict.map(TapVerdict::as_str),
            "attempts": self.attempts.iter().map(|a| a.to_json()).collect::<Vec<_>>(),
        })
    }

    /// Nothing happened: no rebuilds, no verdict. Stored as `None` rather than
    /// as an empty blob, so the common case costs the row nothing.
    pub fn is_empty(&self) -> bool {
        self.attempts.is_empty() && self.verdict.is_none()
    }
}

// ── The stateful driver ─────────────────────────────────────────────────────

/// Owns the ghost watchdog's state and its log for the life of one meeting.
///
/// The split is deliberate and matches `meeting::watchdog_tick`: [`ghost_tick`]
/// **decides** and is pure; this type **applies** and is not. A test can drive
/// forty ticks through the pure function with no clock and no hardware, and the
/// session gets one object to hold.
#[derive(Debug, Clone, Default)]
pub struct GhostWatchdog {
    state: GhostState,
    log: TapRebuildLog,
}

impl GhostWatchdog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> GhostState {
        self.state
    }

    pub fn log(&self) -> &TapRebuildLog {
        &self.log
    }

    pub fn is_degraded(&self) -> bool {
        self.state.degraded
    }

    /// Decide and apply in one call — the shape the watchdog thread uses.
    ///
    /// `host_ns` is the tap's most recent capture anchor host time, or 0 when
    /// there is none (a tap that never delivered has no anchor, which is
    /// exactly the denial case).
    pub fn tick(
        &mut self,
        elapsed: Duration,
        liveness: TapLiveness,
        env: TapEnvironment,
        host_ns: u64,
    ) -> TapWatchdogAction {
        let action = ghost_tick(&TapWatchdogInputs {
            elapsed,
            liveness,
            env,
            state: self.state,
        });
        self.apply(action, elapsed, liveness, host_ns);
        action
    }

    /// Fold an action back into the state. Public because a caller that gets
    /// its action from the meeting watchdog's own tick (rather than from
    /// [`GhostWatchdog::tick`]) still has to record it.
    pub fn apply(
        &mut self,
        action: TapWatchdogAction,
        elapsed: Duration,
        liveness: TapLiveness,
        host_ns: u64,
    ) {
        match action {
            TapWatchdogAction::Continue => {}
            TapWatchdogAction::RebuildFull {
                attempt,
                kind,
                causes,
            } => {
                // An in-flight attempt that is being superseded by a timeout
                // gets its outcome written before the new one opens, so the log
                // never carries two attempts with no ending.
                if self.state.rebuild_issued_at.is_some() {
                    self.close_open_attempt(TapRebuildOutcome::TimedOut);
                }
                self.state.rebuilds_issued = attempt;
                self.state.rebuild_issued_at = Some(elapsed);
                self.state.grace_until = None;
                self.log.attempts.push(TapRebuildAttempt {
                    attempt,
                    at_ms: elapsed.as_millis() as u64,
                    host_ns,
                    silent_ms: liveness.since_nonzero.as_millis() as u64,
                    kind,
                    causes,
                    outcome: None,
                });
            }
            TapWatchdogAction::DegradeTrackLost { verdict, .. } => {
                if self.state.rebuild_issued_at.is_some() {
                    self.close_open_attempt(TapRebuildOutcome::TimedOut);
                }
                self.state.degraded = true;
                self.state.rebuild_issued_at = None;
                self.log.verdict = Some(verdict);
            }
        }
    }

    /// The caller ran the seven steps and is reporting back. **Must** be called
    /// on the failure path too: an attempt that is never closed is what
    /// [`TAP_REBUILD_IN_FLIGHT_TIMEOUT`] exists to survive, not what it is for.
    pub fn finish_rebuild(&mut self, outcome: TapRebuildOutcome, elapsed: Duration) {
        if self.state.rebuild_issued_at.is_none() {
            return;
        }
        self.close_open_attempt(outcome);
        self.state.rebuild_issued_at = None;
        // Give the new tap a fair hearing before judging it again — including
        // after a FAILED rebuild, because a failed rebuild that is retried
        // instantly spends the whole budget inside one watchdog interval.
        self.state.grace_until = Some(elapsed + TAP_REBUILD_GRACE);
    }

    fn close_open_attempt(&mut self, outcome: TapRebuildOutcome) {
        if let Some(open) = self
            .attempts_mut()
            .iter_mut()
            .rev()
            .find(|a| a.outcome.is_none())
        {
            open.outcome = Some(outcome);
        }
    }

    fn attempts_mut(&mut self) -> &mut Vec<TapRebuildAttempt> {
        &mut self.log.attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy_tap() -> TapLiveness {
        TapLiveness {
            ever_nonzero: true,
            since_nonzero: Duration::from_secs(1),
            since_last_block: Duration::from_millis(20),
            frames_delivered: 480_000,
            lost_frames: 0,
        }
    }

    fn ghost_tap() -> TapLiveness {
        TapLiveness {
            since_nonzero: Duration::from_secs(45),
            ..healthy_tap()
        }
    }

    #[test]
    fn the_rebuild_sequence_tears_down_before_it_creates() {
        let steps = full_rebuild_sequence();
        assert_eq!(steps.len(), 7);
        let first_create = steps
            .iter()
            .position(|s| !s.is_teardown())
            .expect("there are create steps");
        assert!(steps[..first_create].iter().all(|s| s.is_teardown()));
        assert!(steps[first_create..].iter().all(|s| !s.is_teardown()));
        assert_eq!(steps[0], TapStep::AudioDeviceStop);
        assert_eq!(steps[6], TapStep::CreateAndStartIOProc);
    }

    #[test]
    fn a_delivering_tap_is_left_alone() {
        let action = ghost_tick(&TapWatchdogInputs {
            elapsed: Duration::from_secs(600),
            liveness: healthy_tap(),
            env: TapEnvironment::default(),
            state: GhostState::default(),
        });
        assert_eq!(action, TapWatchdogAction::Continue);
    }

    #[test]
    fn the_first_answer_to_silence_is_always_a_rebuild() {
        let action = ghost_tick(&TapWatchdogInputs {
            elapsed: Duration::from_secs(600),
            liveness: ghost_tap(),
            env: TapEnvironment::default(),
            state: GhostState::default(),
        });
        assert!(action.is_rebuild(), "got {action:?}");
        assert_eq!(action.verdict(), None);
    }

    #[test]
    fn a_muted_call_is_named_as_a_cause_rather_than_asserted_against() {
        let env = TapEnvironment {
            system_output_active: Some(false),
            ..TapEnvironment::default()
        };
        let causes = plausible_silence_causes(&ghost_tap(), &env);
        assert!(causes.contains(SilenceCause::EveryoneMuted));
        assert_eq!(causes.len(), 1);
        assert_eq!(causes.as_log_string(), "everyone_muted");
    }

    #[test]
    fn an_unreadable_output_probe_is_not_evidence() {
        let causes = plausible_silence_causes(&ghost_tap(), &TapEnvironment::default());
        assert!(causes.is_empty(), "got {causes:?}");
        assert_eq!(causes.as_log_string(), "none");
    }

    #[test]
    fn an_unreported_rebuild_times_out_instead_of_hanging_the_watchdog() {
        let mut dog = GhostWatchdog::new();
        let first = dog.tick(
            Duration::from_secs(100),
            ghost_tap(),
            TapEnvironment::default(),
            42,
        );
        assert!(first.is_rebuild());
        // Nobody ever calls finish_rebuild.
        let waiting = dog.tick(
            Duration::from_secs(110),
            ghost_tap(),
            TapEnvironment::default(),
            42,
        );
        assert_eq!(waiting, TapWatchdogAction::Continue);
        let expired = dog.tick(
            Duration::from_secs(100) + TAP_REBUILD_IN_FLIGHT_TIMEOUT,
            ghost_tap(),
            TapEnvironment::default(),
            42,
        );
        assert!(expired.is_rebuild(), "got {expired:?}");
        assert_eq!(dog.log().count(), 2);
        assert_eq!(
            dog.log().attempts()[0].outcome,
            Some(TapRebuildOutcome::TimedOut)
        );
    }

    #[test]
    fn folding_a_block_of_pure_zeros_never_sets_the_discriminator() {
        let mut live = TapLiveness::started();
        let mut last_nonzero = Duration::ZERO;
        let anchors = [CaptureAnchor {
            host_ns: 1,
            sample_index: 0,
            frames: 512,
            sample_rate: 48_000,
            lost_frames: 0,
        }];
        fold_block(
            &mut live,
            &[0.0; 512],
            &anchors,
            Duration::from_secs(40),
            &mut last_nonzero,
        );
        assert!(!live.ever_nonzero);
        assert_eq!(live.frames_delivered, 512);
        assert_eq!(live.since_nonzero, Duration::from_secs(40));
        assert_eq!(live.verdict(), TapVerdict::PermissionLikelyDenied);
    }

    #[test]
    fn one_nonzero_sample_is_enough_to_rule_permission_out_forever() {
        let mut live = TapLiveness::started();
        let mut last_nonzero = Duration::ZERO;
        let mut block = [0.0f32; 512];
        block[300] = -0.000_03;
        fold_block(
            &mut live,
            &block,
            &[],
            Duration::from_secs(5),
            &mut last_nonzero,
        );
        assert!(live.ever_nonzero);
        assert_eq!(live.verdict(), TapVerdict::GhostTapUnrecovered);
        // …and it stays ruled out through an hour of zeros afterwards.
        fold_block(
            &mut live,
            &[0.0; 512],
            &[],
            Duration::from_secs(3600),
            &mut last_nonzero,
        );
        assert!(live.ever_nonzero);
        assert_eq!(live.since_nonzero, Duration::from_secs(3595));
    }
}
