//! YV100 — the CoreAudio process tap (yap22-B), **global-exclude-self**, feeding
//! the 22-A real-time ring verbatim.
//!
//! This is Track B: the audio the *other* people on the call make, captured from
//! the Mac's own output path rather than from the microphone. It is a macOS 14.4+
//! process tap over an aggregate device, which is the same substrate AudioCap and
//! Recall.ai use, and it is deliberately the *only* thing in this file that is
//! new — the callback body, the ring, the anchor and the session plumbing all
//! already exist for the mic path and are reused, not re-derived.
//!
//! ## Three decisions this module is built around
//!
//! **1. Global-exclude-self only — never app-scoped (plan finding OS-5).** The
//! obvious design is `initMonoMixdownOfProcesses([zoom_pid])`: tap *only* Zoom.
//! The panel killed it for v1 and the reason is not taste. Browsers, Electron
//! apps and Zoom itself render audio through *helper* processes; Google Meet in
//! Chrome plays through Chrome's audio-service helper, and
//! `NSRunningApplication.processIdentifier` hands back the UI PID, which produces
//! no audio at all. An app-scoped tap built that way is legitimately, permanently
//! silent — and a permanently silent tap is **indistinguishable** from a TCC
//! denial and from OS-4's all-zero ghost-tap bug. Three failure modes collapsing
//! onto one symptom is the largest silent-failure surface in the whole design,
//! and collapsing them would also destroy YV104's discriminator ("did this tap
//! *ever* deliver a non-zero sample?"), which is the only honest way to tell a
//! denial from a wedge without a private API.
//!
//! So: [`CATapDescription::initMonoGlobalTapButExcludeProcesses`] with exactly
//! one exclusion — us. No PID resolution for a *target*, no
//! `kAudioHardwarePropertyProcessObjectList` enumeration, no re-enumeration
//! listener. The accepted cost, stated out loud: Spotify and notification chimes
//! land in the meeting track. That is the same trade §2.1 already accepted when
//! it rejected ScreenCaptureKit.
//!
//! The one PID translation that *is* honest is our own — and it is a hard
//! requirement, not a nicety. An **empty** exclusion list is not "exclude
//! nothing safely", it is a global tap that includes Yap's own output, i.e. the
//! meeting recording feeding itself. [`exclusion_list`] therefore refuses to
//! build a description at all when our own process object cannot be resolved.
//! This is the same class of silent inversion OS-1 warns about with the
//! `exclusive` flag: the failure looks like a working tap.
//!
//! **2. The IOProc body is the 22-A ring, verbatim (OS-7).** `rtring.rs` says so
//! in its own doc comment — *"built ONCE here for the mic path so 22-B reuses it
//! verbatim rather than growing a second copy"*. So the block does exactly what
//! the mic callback does: [`crate::meeting::rt_capture_callback`], nothing else.
//! No DSP, no allocation, no lock, no logging. The constraint is *stricter* here
//! than on the mic thread, because the aggregate's main sub-device is the user's
//! real output device — a missed IOProc deadline is a glitch in the call the user
//! is listening to, not merely a defect in the recording.
//!
//! The one thing the block does that is *not* a straight copy of the mic
//! callback is the timestamp conversion, and it exists precisely so the anchor
//! that comes out is the same: CoreAudio hands the block `mHostTime`, absolute
//! mach ticks since boot, while `CaptureAnchor::host_ns` is a nanosecond clock
//! **rebased to each stream's own first callback** (`record.rs` does the
//! rebase with `duration_since`; `meeting.rs`'s splice planner spells the
//! convention out). [`TapClock`] is that conversion, and
//! `tests/syscapture_ioproc_rt_safety.rs` drives it — not a stand-in — to check
//! both halves of the claim: first anchor `0`, and no allocation on the way
//! there. Without it the two tracks would be an uptime apart in YV106's merge,
//! and nothing single-track would notice.
//!
//! **3. The FFI is `extern "C-unwind"` (OS-6, correction #2).** A Rust panic
//! inside the block unwinds *into CoreAudio's real-time thread*: undefined
//! behaviour and a HAL-level process kill, not a `Result`. Everything the block
//! does — including the `AudioBufferList` decode that produces the slice — runs
//! inside [`tap_ioproc_guarded`], which catches, sets the atomic flag YV104's
//! watchdog reads, and returns normally.
//!
//! ## Structure: pure state machine, thin platform
//!
//! Same split `sysaudio.rs`'s `restore_plan` established in this codebase. The
//! *order of operations* — create tap, read UID and format, resolve the default
//! output, compose the aggregate dictionary, create the IOProc, start; and on
//! every exit path stop → destroy IOProc → destroy aggregate → destroy tap — is
//! a pure state machine over the [`TapPlatform`] trait, so `cargo test` proves it
//! with **zero audio hardware and no TCC grant**. The real CoreAudio calls live
//! in [`imp`] behind that trait and are the only part that needs a Mac with a
//! permission dialog.
//!
//! That teardown order is not stylistic either. Apple's own forum guidance for
//! the ghost-tap bug is that restarting the IOProc alone, or recreating only the
//! aggregate device, is *not* reliable — both the process tap and the aggregate
//! device must be destroyed and recreated. YV104's 7-step rebuild is literally
//! this teardown followed by this setup, which is why it is written once, here,
//! and asserted by `tests/syscapture_teardown_order.rs`.
//!
//! ## What this item does NOT do
//!
//! * **No user-facing OS gate yet.** The visible, honest affordance ("System
//!   audio capture requires macOS 14.4 or later"), the `ProcessInfo` version
//!   read, `NSAudioCaptureUsageDescription` and the signed-build TCC acceptance
//!   are **YV101**, and until they land nothing in the app calls
//!   [`imp::start_system_tap`].
//!
//!   What is **not** deferred is the *linkage* floor, because deferring it was
//!   wrong: `AudioHardwareCreateProcessTap`/`…DestroyProcessTap` are macOS 14.4
//!   symbols and `AudioHardwareCreate/DestroyAggregateDevice` are 13.0 symbols,
//!   and dyld binds imports at **load** time — before any Rust runs, and
//!   entirely independent of whether a call site exists. A hard import of any of
//!   the four is therefore not "a broken Notetaker on macOS 12/13", it is *Yap
//!   failing to launch at all* for every one of those users, dictation and all.
//!   So [`imp`] resolves those four through `dlsym` and the `CATapDescription`
//!   class through the ObjC runtime, refusing with
//!   [`TapError::ProcessTapApiUnavailable`] when they are absent.
//!
//!   That alone is not enough, and the reason is worth knowing: **cpal 0.18.1
//!   calls the same four symbols itself** (`src/host/coreaudio/macos/loopback.rs`),
//!   from an object the microphone path keeps alive, so `origin/main` — with no
//!   process tap in it at all — already hard-imported them. `build.rs` therefore
//!   also links CoreAudio with `-weak_framework`, which turns every CoreAudio
//!   import in the binary weak: dyld binds the missing ones to NULL on macOS
//!   12/13 and the app launches. Yap never asks cpal for a loopback device, and
//!   [`imp`] null-checks before it calls, so nothing dereferences a NULL
//!   binding. `scripts/assert-weak-linked-14_4-symbols.sh` (YV101, which landed this
//!   check while this item was in review) asserts the outcome on the real
//!   release binary in CI. `minimumSystemVersion` stays `"12.0"` (OS-11).
//! * **No second DSP/epoch pipeline.** Per-track DSP, journal track 1 and the
//!   two-track merge are **YV106**. This item stops at "the tap reliably delivers
//!   frames + anchors into the ring, with a clean teardown".
//! * **No TCC pre-warm UX** (YV102) and **no device-change guard** (YV103).
//!   The silence watchdog (YV104) is in this file, below, but nothing wires the
//!   two halves together yet — `CaptureEnv::tap_liveness` still returns `None`
//!   for every shipping env, exactly as YV104 and YV105 both say out loud.
//!
//! ---
//!
//! # Two items, one module: the tap (YV100) and its watchdog (YV104)
//!
//! `syscapture.rs` landed in two pieces and in the reverse of the order they
//! read. YV104's zero-buffer ghost watchdog merged to `main` first, while this
//! item — the tap it watches — was still in review; the doc comment that
//! follows was written in that world and its own words for the arrangement were
//! *"YV100 binds its FFI to this list instead of writing a second copy that can
//! drift from it."* That is now literally true rather than a plan:
//! [`teardown`] runs the first four entries of [`full_rebuild_sequence`], and
//! `syscapture_teardown_order`'s `the_teardown_order_is_the_first_four_steps_of_the_one_declared_rebuild`
//! fails if the two ever diverge. The tap half of the file grew its own
//! `TeardownStep` enum while the two branches were apart — a textually clean
//! rebase that would have shipped exactly the drift the comment warned about —
//! and that enum is gone: there is one [`TapStep`], one order, one declaration.
//!
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
//! **What that bit does NOT do, and the review round that made the distinction
//! load-bearing:** `ever_nonzero == false` separates a denial from OS-4's
//! ghost. It does **not** separate a denial from a tap that was granted and
//! simply had nothing to record — an in-person meeting, a call where the remote
//! side is quiet, a lecture before the lecturer starts. Reading it as if it did
//! is this module's own headline failure arriving from the opposite direction:
//! a healthy, permission-GRANTED tap badged "permission is off", after three
//! real CoreAudio teardown/recreate cycles it never needed. So a permission
//! verdict now requires *positive* evidence that something was playing while
//! the tap was quiet ([`TapEnvironment::system_output_active`] observed
//! `Some(true)` during the silence). With no such evidence the verdict is
//! [`TapVerdict::NoSystemAudioObserved`], which says what was actually seen.
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
//! One of those five is not merely named, it is **acted on**: when the machine
//! is observably producing no output at all (`system_output_active ==
//! Some(false)`) and the IOProc is still firing on cadence, the tap's zeros are
//! *explained* — there is nothing to capture — and [`is_unexplained_silence`]
//! returns false, so the silence clock never starts the budget. Enumerating a
//! cause and then rebuilding anyway would be decoration; this is the cause
//! being used. Note the second half of that condition: a dead IOProc is not
//! explained by a quiet room, so `IoProcSilent` still rebuilds even with
//! nothing playing.
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
//! YV100's, and they are now in this same file, above. What this item landed
//! for it is [`full_rebuild_sequence`] — the seven steps in the one order every exit
//! path uses, declared **once**, in this file, so YV100 binds its FFI to this
//! list instead of writing a second copy that can drift from it. YV103's
//! `FormatChangeAction::RebuildAggregate` already names this same sequence as
//! the thing it is a decision *about*.
//!
//! (Apple's forum thread is cited by number rather than by link on purpose:
//! this module is on the meeting path and `tests/matrix_phase_offline.rs` fails
//! the build if a meeting-path module contains a URL — the standing proof
//! behind the phase's "records with Wi-Fi off" claim.)

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::meeting::{ExternalStream, RtCapture, SessionConfig};
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
///
/// **Derived from [`crate::meeting::WATCHDOG_INTERVAL`], not written down as a
/// number**, and the review round that forced that is worth recording: at a
/// hard-coded 20 s the timeout was shorter than the 60 s tick that reads it, so
/// an in-flight rebuild was *always* already expired by the next tick and the
/// "wait for it" branch below was unreachable in the shipping configuration —
/// a wait that only existed in tests that ticked faster than the product does.
/// Two intervals is the smallest multiple that leaves at least one whole tick
/// of genuine waiting; a caller that has not reported back after two full
/// watchdog periods is not slow, it is gone.
pub const TAP_REBUILD_IN_FLIGHT_TIMEOUT: Duration =
    Duration::from_secs(crate::meeting::WATCHDOG_INTERVAL.as_secs() * 2);

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
    /// This is rule (c), and it is the ONLY place the three are told apart.
    /// Note what it is not: it is not consulted before the budget is spent, so
    /// it can never turn a first silence into a permission badge.
    ///
    /// `output_was_active_during_silence` is the second bit the verdict needs
    /// and the one the first round of this module was missing: whether
    /// `kAudioProcessPropertyIsRunningOutput` was ever observed **true** while
    /// this tap was quiet ([`GhostState::output_active_observed`]). Without it,
    /// `!ever_nonzero` alone reads "nothing was playing" and "permission is
    /// off" as the same sentence, and one of those two is an accusation.
    ///
    /// | `ever_nonzero` | something was playing | verdict |
    /// |---|---|---|
    /// | true  | — | [`TapVerdict::GhostTapUnrecovered`] |
    /// | false | yes | [`TapVerdict::PermissionLikelyDenied`] |
    /// | false | no / unknown | [`TapVerdict::NoSystemAudioObserved`] |
    pub fn verdict(&self, output_was_active_during_silence: bool) -> TapVerdict {
        match (self.ever_nonzero, output_was_active_during_silence) {
            (true, _) => TapVerdict::GhostTapUnrecovered,
            (false, true) => TapVerdict::PermissionLikelyDenied,
            (false, false) => TapVerdict::NoSystemAudioObserved,
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

/// Is this silence **explained** — is there a reading on the table that accounts
/// for the zeros without anything being wrong with the tap?
///
/// Exactly one of the five causes can do that, and it is the ordinary case
/// rather than an edge case: nothing on the machine is producing output
/// ([`SilenceCause::EveryoneMuted`]). An in-person meeting, a call where the
/// remote side is quiet, a lecture that has not started — thirty seconds of
/// digital silence there is not a symptom, it is the truth, and tearing down a
/// working tap and its aggregate device over it costs a real gap in Track B and
/// emits the device-change notifications YV103's guard exists to absorb.
///
/// The second half of the condition is the part that keeps this honest: a
/// **dead IOProc** is not explained by a quiet room. A process tap's IOProc
/// keeps firing on the device clock whether or not anything is playing into it,
/// so callbacks that have stopped altogether are a fault no matter how quiet
/// the machine is, and that silence stays actionable.
///
/// The other four causes deliberately do NOT suppress: a tap pointed at the
/// wrong output device, an inverted `exclusive` flag and a nil dispatch queue
/// are all things the rebuild might actually fix, so they are named on the
/// action and acted on anyway.
pub fn silence_is_explained(live: &TapLiveness, env: &TapEnvironment) -> bool {
    env.system_output_active == Some(false) && live.since_last_block < TAP_SILENCE_REBUILD_AFTER
}

/// Is the tap silent **in a way the watchdog is allowed to act on** — long
/// enough, and with no innocent explanation on the table?
///
/// This is the predicate the whole tick runs on. [`TapLiveness::is_ghost_silent`]
/// answers "has it been quiet for [`TAP_SILENCE_REBUILD_AFTER`]", which is a
/// clock reading; this answers "and is that worth a teardown", which is the
/// decision.
pub fn is_unexplained_silence(live: &TapLiveness, env: &TapEnvironment) -> bool {
    live.is_ghost_silent() && !silence_is_explained(live, env)
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
    /// The tap never delivered a non-zero sample **and something was observably
    /// playing while it stayed quiet**. That conjunction is what a TCC denial
    /// looks like, and nothing else this module can see looks like it.
    ///
    /// The second half is not decoration. Without it this variant also covers
    /// every granted tap that had nothing to record, which is the ordinary
    /// in-person meeting.
    PermissionLikelyDenied,
    /// The tap never delivered a non-zero sample and nothing was ever seen
    /// playing while it was quiet — either the probe read "nothing is producing
    /// output" or it could not be read at all.
    ///
    /// This says what was observed and stops there. It is **not** a permission
    /// accusation, and the banner must not imply one: the most likely reading
    /// by far is a meeting with no system audio in it.
    NoSystemAudioObserved,
    /// The tap delivered real audio and then went to zeros and stayed there
    /// through three full rebuilds. OS-4's bug, unrecovered. **Not** a
    /// permission problem, and the UI must not say it is.
    GhostTapUnrecovered,
}

impl TapVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            TapVerdict::PermissionLikelyDenied => "permission_likely_denied",
            TapVerdict::NoSystemAudioObserved => "no_system_audio_observed",
            TapVerdict::GhostTapUnrecovered => "ghost_tap_unrecovered",
        }
    }

    /// Does this verdict accuse the user's privacy settings of anything? The
    /// one place that question is answered, so the UI never has to guess and a
    /// test can assert the other two stay quiet about permission.
    pub fn blames_permission(self) -> bool {
        matches!(self, TapVerdict::PermissionLikelyDenied)
    }

    /// The sentence the pill's banner shows. Honest in all three directions:
    /// the ghost's line never mentions permission, the "nothing was playing"
    /// line never mentions permission either, and the denial's line says
    /// "looks like" because that is the strength of the evidence.
    pub fn banner(self) -> &'static str {
        match self {
            TapVerdict::PermissionLikelyDenied => {
                "System audio was never captured — it looks like permission is off. \
                 Your microphone track is still recording."
            }
            TapVerdict::NoSystemAudioObserved => {
                "No system audio was playing during this meeting, so there was nothing \
                 to capture. Your microphone track is still recording."
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
    /// **Nothing to tear down.** A rebuild is flagged as in flight, and the
    /// watchdog has decided to stop waiting for its caller to report back —
    /// because the tap is delivering audio again (`Succeeded`), or because the
    /// silence turned out to be explained and its fate is unknowable
    /// (`Unknown`). Close the open attempt in the log and carry on.
    ///
    /// This exists because the alternative is the failure the second review
    /// round found: the in-flight timeout fired on a tap that was *actively
    /// delivering*, rebuilt it twice more and then degraded it with "system
    /// audio stopped coming through" while it was recording fine. A recovered
    /// tap must never be torn down by a stale flag, and "close the flag" needed
    /// to be something the pure tick could say out loud.
    RebuildSettled { outcome: TapRebuildOutcome },
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
    /// Was the machine ever observed **producing output** while this tap was
    /// quiet — `system_output_active == Some(true)` on a tick where the tap was
    /// already past [`TAP_SILENCE_REBUILD_AFTER`]?
    ///
    /// This is the second bit [`TapLiveness::verdict`] needs, and it is watchdog
    /// state rather than liveness state because it is a fold over *observations
    /// across ticks*, not over samples: one probe reading is a sample, and the
    /// question is whether the conjunction "audio playing, tap silent" was ever
    /// true at all during this stretch of silence.
    ///
    /// It resets the moment the tap delivers again, because the evidence
    /// belongs to the stretch of silence it was collected in, not to the next
    /// one. A tap that never delivers anything never resets it, which is
    /// exactly the denial case.
    pub output_active_observed: bool,
}

/// Fold this tick's observation into the watchdog's silence bookkeeping.
///
/// Pure and **idempotent** — `observe(observe(s)) == observe(s)` for the same
/// tick — which is what lets both [`ghost_tick`] (deciding) and
/// [`GhostWatchdog::apply`] (recording) run it on the same inputs without the
/// two disagreeing about what was seen. The alternative, latching it only in
/// `apply`, would have the decision run one whole 60 s tick behind the
/// observation it depends on.
pub fn observe(mut state: GhostState, live: &TapLiveness, env: &TapEnvironment) -> GhostState {
    if !live.is_ghost_silent() {
        state.output_active_observed = false;
    } else if env.system_output_active == Some(true) {
        state.output_active_observed = true;
    }
    state
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
/// 2. **A tap that is delivering again closes any in-flight rebuild instead of
///    being torn down by it.** Liveness is read *before* the timeout, never
///    after: a stale in-flight flag on a working tap is not a reason to rebuild
///    anything, and reading the clock first is exactly how the second review
///    round's "rebuilt twice more, then degraded, while recording fine" bug
///    happened.
/// 3. A rebuild in flight is otherwise waited for — until it times out, at
///    which point it is treated as a spent attempt rather than as a reason to
///    wait forever.
/// 4. A tap inside its post-rebuild grace window is left alone.
/// 5. A tap that is delivering, or whose silence is explained
///    ([`is_unexplained_silence`]), is left alone.
/// 6. Sustained *unexplained* silence with budget left ⇒
///    **[`TapWatchdogAction::RebuildFull`], unconditionally** — this is rule
///    (a), and note that [`TapLiveness::verdict`] is not called anywhere above
///    this line, which is what makes "never a permission verdict on the first
///    silence" structural rather than aspirational.
/// 7. Sustained unexplained silence with the budget spent ⇒ degrade, carrying
///    the verdict the discriminator supports.
pub fn ghost_tick(inputs: &TapWatchdogInputs) -> TapWatchdogAction {
    let TapWatchdogInputs {
        elapsed,
        liveness,
        env,
        state,
    } = inputs;
    let state = observe(*state, liveness, env);

    if state.degraded {
        return TapWatchdogAction::Continue;
    }

    let actionable = is_unexplained_silence(liveness, env);

    // A rebuild we asked for is still running. Wait for it — but not forever,
    // and not at all if the thing it was meant to fix has fixed itself.
    if let Some(issued_at) = state.rebuild_issued_at {
        if !liveness.is_ghost_silent() {
            // Audio is arriving. Whatever the caller did or did not report, the
            // tap works: close the attempt and touch nothing.
            return TapWatchdogAction::RebuildSettled {
                outcome: TapRebuildOutcome::Succeeded,
            };
        }
        let expired = elapsed.saturating_sub(issued_at) >= TAP_REBUILD_IN_FLIGHT_TIMEOUT;
        if !actionable {
            // Still quiet, but the quiet is explained. Nothing to conclude and
            // nothing to spend — release the flag once it is stale so the next
            // real silence is not blocked behind it.
            return if expired {
                TapWatchdogAction::RebuildSettled {
                    outcome: TapRebuildOutcome::Unknown,
                }
            } else {
                TapWatchdogAction::Continue
            };
        }
        if !expired {
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
                verdict: liveness.verdict(state.output_active_observed),
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

    if !actionable {
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
        verdict: liveness.verdict(state.output_active_observed),
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
    /// The attempt was closed without ever learning how it ended: the caller
    /// never reported, and by the time the watchdog stopped waiting the tap's
    /// silence had an innocent explanation (nothing was playing), so neither
    /// "it worked" nor "it timed out" is a fact anyone here has.
    ///
    /// Distinct from [`TapRebuildOutcome::TimedOut`] on purpose — the log is
    /// read months later to answer "did rebuilding help", and an honest "we do
    /// not know" is a different answer from "it failed to report".
    Unknown,
}

impl TapRebuildOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            TapRebuildOutcome::Succeeded => "succeeded",
            TapRebuildOutcome::Failed { .. } => "failed",
            TapRebuildOutcome::TimedOut => "timed_out",
            TapRebuildOutcome::Unknown => "unknown",
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
        self.apply(action, elapsed, liveness, env, host_ns);
        action
    }

    /// Fold an action back into the state. Public because a caller that gets
    /// its action from the meeting watchdog's own tick (rather than from
    /// [`GhostWatchdog::tick`]) still has to record it.
    ///
    /// `env` is taken here and not only in [`GhostWatchdog::tick`] because the
    /// tick's own bookkeeping — [`observe`] — is a function of it, and a caller
    /// that decides through `meeting::watchdog_tick` must record exactly the
    /// observation the decision was made on.
    pub fn apply(
        &mut self,
        action: TapWatchdogAction,
        elapsed: Duration,
        liveness: TapLiveness,
        env: TapEnvironment,
        host_ns: u64,
    ) {
        self.state = observe(self.state, &liveness, &env);
        match action {
            TapWatchdogAction::Continue => {}
            TapWatchdogAction::RebuildSettled { outcome } => {
                if self.state.rebuild_issued_at.is_some() {
                    self.close_open_attempt(outcome);
                    self.state.rebuild_issued_at = None;
                    self.state.grace_until = Some(elapsed + TAP_REBUILD_GRACE);
                }
            }
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

// ── The aggregate-device composition dictionary (pure) ──────────────────────

/// The CoreAudio dictionary keys the aggregate device is composed from.
///
/// These are string keys, not four-char codes, and a typo in one of them is a
/// silent misconfiguration rather than a compile error: CoreAudio ignores keys
/// it does not recognise, so `"tapautostart"` misspelled produces an aggregate
/// device that is created successfully and never starts its tap. They are
/// spelled out here so the pure builder can be tested without linking
/// CoreAudio, and `coreaudio_aggregate_key_names` reads the *real* constants
/// back out of `objc2-core-audio` so the test can prove the two agree.
pub mod keys {
    /// `kAudioAggregateDeviceUIDKey`
    pub const AGGREGATE_UID: &str = "uid";
    /// `kAudioAggregateDeviceNameKey`
    pub const AGGREGATE_NAME: &str = "name";
    /// `kAudioAggregateDeviceIsPrivateKey` — a private aggregate is not shown to
    /// other apps and disappears with the process that made it, which is what
    /// keeps a crashed Yap from leaving a phantom device in Audio MIDI Setup.
    pub const IS_PRIVATE: &str = "private";
    /// `kAudioAggregateDeviceMainSubDeviceKey`
    pub const MAIN_SUB_DEVICE: &str = "master";
    /// `kAudioAggregateDeviceSubDeviceListKey`
    pub const SUB_DEVICE_LIST: &str = "subdevices";
    /// `kAudioAggregateDeviceTapListKey`
    pub const TAP_LIST: &str = "taps";
    /// `kAudioAggregateDeviceTapAutoStartKey`
    pub const TAP_AUTO_START: &str = "tapautostart";
    /// `kAudioSubDeviceUIDKey`
    pub const SUB_DEVICE_UID: &str = "uid";
    /// `kAudioSubTapUIDKey`
    pub const SUB_TAP_UID: &str = "uid";
}

/// The four inputs the composition dictionary is a pure function of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateSpec {
    /// A UID we invent for our own private aggregate device.
    pub aggregate_uid: String,
    /// Human-readable name. Private devices are not listed anywhere the user
    /// looks, but a crash report or a `coreaudiod` log will name it.
    pub aggregate_name: String,
    /// The UID of the CURRENT default output device — the aggregate's main sub
    /// device. Reading this at compose time (rather than caching it) is what
    /// YV103's device-change guard will re-run on an output switch.
    pub output_uid: String,
    /// The UID of the process tap created just before this call.
    pub tap_uid: String,
}

/// A CoreFoundation dictionary value, as far as this composition needs one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictValue {
    Str(String),
    Bool(bool),
    List(Vec<DictValue>),
    Dict(Vec<(String, DictValue)>),
}

/// The aggregate-device description CoreAudio expects, as an ordered key/value
/// list. Pure: no FFI, no hardware, no TCC.
///
/// `tapautostart: true` is what makes the tap begin delivering as soon as the
/// device does, rather than waiting for a separate start that this design never
/// issues. `private: true` keeps the device out of every other app's device
/// list. The tap rides in `taps` as a one-element list of `{uid: <tap uid>}`,
/// which is the shape the sub-tap keys describe — a bare string there is
/// accepted and silently ignored.
pub fn aggregate_description(spec: &AggregateSpec) -> Vec<(String, DictValue)> {
    vec![
        (
            keys::AGGREGATE_UID.to_string(),
            DictValue::Str(spec.aggregate_uid.clone()),
        ),
        (
            keys::AGGREGATE_NAME.to_string(),
            DictValue::Str(spec.aggregate_name.clone()),
        ),
        (keys::IS_PRIVATE.to_string(), DictValue::Bool(true)),
        (
            keys::MAIN_SUB_DEVICE.to_string(),
            DictValue::Str(spec.output_uid.clone()),
        ),
        (
            keys::SUB_DEVICE_LIST.to_string(),
            DictValue::List(vec![DictValue::Dict(vec![(
                keys::SUB_DEVICE_UID.to_string(),
                DictValue::Str(spec.output_uid.clone()),
            )])]),
        ),
        (
            keys::TAP_LIST.to_string(),
            DictValue::List(vec![DictValue::Dict(vec![(
                keys::SUB_TAP_UID.to_string(),
                DictValue::Str(spec.tap_uid.clone()),
            )])]),
        ),
        (keys::TAP_AUTO_START.to_string(), DictValue::Bool(true)),
    ]
}

/// The same key names, read back out of `objc2-core-audio`'s own constants.
///
/// This exists for exactly one assertion: that [`keys`] is not a set of
/// plausible-looking typos. Returned in the order [`aggregate_description`]
/// writes them.
#[cfg(target_os = "macos")]
pub fn coreaudio_aggregate_key_names() -> Vec<&'static str> {
    use objc2_core_audio::{
        kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceMainSubDeviceKey,
        kAudioAggregateDeviceNameKey, kAudioAggregateDeviceSubDeviceListKey,
        kAudioAggregateDeviceTapAutoStartKey, kAudioAggregateDeviceTapListKey,
        kAudioAggregateDeviceUIDKey, kAudioSubDeviceUIDKey, kAudioSubTapUIDKey,
    };
    [
        kAudioAggregateDeviceUIDKey,
        kAudioAggregateDeviceNameKey,
        kAudioAggregateDeviceIsPrivateKey,
        kAudioAggregateDeviceMainSubDeviceKey,
        kAudioAggregateDeviceSubDeviceListKey,
        kAudioAggregateDeviceTapListKey,
        kAudioAggregateDeviceTapAutoStartKey,
        kAudioSubDeviceUIDKey,
        kAudioSubTapUIDKey,
    ]
    .iter()
    .map(|k| k.to_str().expect("CoreAudio keys are ASCII"))
    .collect()
}

/// The same names [`keys`] declares, in the same order
/// [`coreaudio_aggregate_key_names`] returns them.
pub fn declared_aggregate_key_names() -> Vec<&'static str> {
    vec![
        keys::AGGREGATE_UID,
        keys::AGGREGATE_NAME,
        keys::IS_PRIVATE,
        keys::MAIN_SUB_DEVICE,
        keys::SUB_DEVICE_LIST,
        keys::TAP_LIST,
        keys::TAP_AUTO_START,
        keys::SUB_DEVICE_UID,
        keys::SUB_TAP_UID,
    ]
}

// ── Errors ─────────────────────────────────────────────────────────────────

/// Which CoreAudio call failed. Carried in the error so a log line names the
/// step rather than an `OSStatus` on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapStage {
    CreateTap,
    ReadTapUid,
    ReadTapFormat,
    /// Not a CoreAudio call — the one place the format the tap *actually*
    /// delivers is bound to the [`RtCapture`] that stamps every anchor from it.
    /// It is a stage rather than a bare `?` because it is a real exit path with
    /// a real teardown obligation: the tap exists by this point.
    BindCapture,
    ResolveDefaultOutput,
    CreateAggregate,
    CreateIoProc,
    Start,
}

impl TapStage {
    pub fn call(self) -> &'static str {
        match self {
            TapStage::CreateTap => "AudioHardwareCreateProcessTap",
            TapStage::ReadTapUid => "kAudioTapPropertyUID",
            TapStage::ReadTapFormat => "kAudioTapPropertyFormat",
            TapStage::BindCapture => "bind kAudioTapPropertyFormat to the capture ring",
            TapStage::ResolveDefaultOutput => "kAudioHardwarePropertyDefaultOutputDevice",
            TapStage::CreateAggregate => "AudioHardwareCreateAggregateDevice",
            TapStage::CreateIoProc => "AudioDeviceCreateIOProcIDWithBlock",
            TapStage::Start => "AudioDeviceStart",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TapError {
    /// Our own process object could not be resolved, so the exclusion list would
    /// be empty — which is not "exclude nothing", it is a global tap that records
    /// Yap recording. Refused. See the module docs.
    SelfProcessObjectUnavailable,
    /// A CoreAudio call returned a non-zero `OSStatus`.
    Os { stage: TapStage, status: i32 },
    /// Setup unwound. The FFI boundary is `extern "C-unwind"`, so this is caught
    /// rather than propagated; the resources that existed at that moment were
    /// torn down in the canonical order before this was returned.
    PanicDuringSetup { stage: TapStage },
    /// This OS has no process-tap API at all: `dlsym` found no
    /// `AudioHardwareCreateProcessTap` (or one of the other three
    /// availability-gated entry points, or the `CATapDescription` class) in the
    /// running process. That is macOS 12/13, and it is a *linkage* fact, not a
    /// user-facing verdict — the honest, visible "System audio capture requires
    /// macOS 14.4 or later" affordance is YV101's `MeetingUnavailable` gate.
    /// This variant exists so the symbols are resolved once, checked once, and
    /// never called through a null pointer.
    ProcessTapApiUnavailable { symbol: &'static str },
    /// The tap delivers one format and the [`RtCapture`] stamping its anchors
    /// was built for another. Refused before a single sample is stamped —
    /// see [`capture_matches_format`] for why this is fatal rather than
    /// cosmetic.
    CaptureFormatMismatch {
        tap_sample_rate: u32,
        tap_channels: u16,
        capture_sample_rate: u32,
        capture_channels: u16,
    },
}

impl std::fmt::Display for TapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TapError::SelfProcessObjectUnavailable => write!(
                f,
                "could not resolve Yap's own audio process object — refusing to \
                 build a tap that would record Yap itself"
            ),
            TapError::Os { stage, status } => write!(
                f,
                "{} failed with OSStatus {status} ({})",
                stage.call(),
                fourcc(*status)
            ),
            TapError::PanicDuringSetup { stage } => {
                write!(f, "panicked during {}", stage.call())
            }
            TapError::ProcessTapApiUnavailable { symbol } => write!(
                f,
                "this macOS build has no process-tap API ({symbol} is not present \
                 in the running process) — system audio capture needs macOS 14.4 \
                 or later"
            ),
            TapError::CaptureFormatMismatch {
                tap_sample_rate,
                tap_channels,
                capture_sample_rate,
                capture_channels,
            } => write!(
                f,
                "the tap delivers {tap_sample_rate} Hz / {tap_channels} ch but the \
                 capture ring stamping its anchors was built for \
                 {capture_sample_rate} Hz / {capture_channels} ch — refusing to \
                 stamp a sample axis that is wrong by construction"
            ),
        }
    }
}

/// CoreAudio statuses are usually four-char codes. Rendering both spellings is
/// the difference between a searchable log line and a bare negative number.
fn fourcc(status: i32) -> String {
    let bytes = (status as u32).to_be_bytes();
    if bytes.iter().all(|b| (0x20..=0x7e).contains(b)) {
        format!("'{}'", String::from_utf8_lossy(&bytes))
    } else {
        format!("0x{:08x}", status as u32)
    }
}

// ── The platform seam ──────────────────────────────────────────────────────

/// The tap's native format, as reported by `kAudioTapPropertyFormat`.
///
/// Read from the tap rather than assumed: a mono global tap is 1 channel, but
/// the rate follows the output device and an AirPods swap moves it (OS-9). The
/// [`crate::rtring::CaptureAnchor`] carries the rate per callback for exactly
/// this reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// An opaque IOProc token. The real platform keeps the `AudioDeviceIOProcID`,
/// the retained block and the dispatch queue in its own fields; the state
/// machine only needs to know that one exists.
pub type IoProcToken = u64;

/// Every CoreAudio call the setup/teardown sequence makes, behind a seam.
///
/// A fake implementation is what lets `tests/syscapture_teardown_order.rs` prove
/// the ordering contract on a CI box with no audio device and no TCC grant.
/// THE INVARIANT THE TAP'S SAMPLE AXIS RESTS ON.
///
/// `rt_capture_callback` divides the interleaved block by
/// [`RtCapture::channels`] to get a frame count, and stamps every
/// [`CaptureAnchor`] with [`RtCapture::sample_rate`]. The tap's IOProc trims its
/// buffer by [`TapFormat::channels`]. If those two disagree, **nothing fails** —
/// every call returns `noErr`, audio arrives, the ring fills, and each anchor
/// reports a frame count and a rate that are simply wrong.
///
/// Concretely, the case that was actually reproduced: a 44.1 kHz stereo tap
/// stamped through a ring built from the mic's 48 kHz mono gives
/// `anchor.frames = 882` where the truth is `441`, and `sample_rate = 48000`
/// where the truth is `44100` — the sample axis 2× off and the declared rate
/// 8.8 % off, silently. YV107's merge divides one of those by the other, so the
/// error does not stay in this module: it lands as Track 1 drifting against
/// Track 0 at a rate no measurement in this codebase would attribute to it.
///
/// It cannot be fixed by asking the caller to pass the right ring, because the
/// caller cannot know: `kAudioTapPropertyFormat` is only readable *after*
/// `AudioHardwareCreateProcessTap` succeeds. So the ring is constructed from the
/// format — [`capture_for_format`] — and this function is the standing check
/// that whatever ring reaches the IOProc really was.
pub fn capture_matches_format(capture: &RtCapture, format: TapFormat) -> Result<(), TapError> {
    if capture.sample_rate() == format.sample_rate && capture.channels() == format.channels {
        Ok(())
    } else {
        Err(TapError::CaptureFormatMismatch {
            tap_sample_rate: format.sample_rate,
            tap_channels: format.channels,
            capture_sample_rate: capture.sample_rate(),
            capture_channels: capture.channels(),
        })
    }
}

/// The ring the tap's anchors are stamped from, built from the format the tap
/// reported rather than from anything a caller believed.
///
/// `RtCapture::new` clamps `channels` to at least 1, which is why
/// [`capture_matches_format`] compares against the *constructed* ring rather
/// than against the arguments: a zero-channel format must not silently become a
/// one-channel ring that the IOProc then trims by zero.
pub fn capture_for_format(format: TapFormat) -> Arc<RtCapture> {
    Arc::new(RtCapture::new(format.sample_rate, format.channels))
}

pub trait TapPlatform {
    fn create_tap(&mut self, exclude_process_objects: &[u32]) -> Result<u32, i32>;
    fn tap_uid(&mut self, tap: u32) -> Result<String, i32>;
    fn tap_format(&mut self, tap: u32) -> Result<TapFormat, i32>;
    /// Bind the just-read [`TapFormat`] to the [`RtCapture`] whose
    /// `sample_rate`/`channels` stamp every [`CaptureAnchor`], and refuse if
    /// they disagree.
    ///
    /// This exists because the format is **undiscoverable before the tap is
    /// created**, and the anchors are **wrong from the first callback** if it is
    /// guessed. There is no caller ordering that gets it right by passing a
    /// ready-made ring in, which is why the ring is built *here*, from the
    /// truth, and handed back out afterwards. See [`capture_matches_format`].
    fn bind_capture(&mut self, format: TapFormat) -> Result<(), TapError>;
    fn default_output_uid(&mut self) -> Result<String, i32>;
    fn create_aggregate(&mut self, description: &[(String, DictValue)]) -> Result<u32, i32>;
    fn create_ioproc(&mut self, aggregate: u32, format: TapFormat) -> Result<IoProcToken, i32>;
    fn start(&mut self, aggregate: u32, ioproc: IoProcToken) -> Result<(), i32>;

    // Teardown never reports failure. There is nothing a caller could do with
    // it, and a teardown that stops early on the first non-zero status is how a
    // process tap outlives the app that made it.
    fn stop(&mut self, aggregate: u32, ioproc: IoProcToken);
    fn destroy_ioproc(&mut self, aggregate: u32, ioproc: IoProcToken);
    fn destroy_aggregate(&mut self, aggregate: u32);
    fn destroy_tap(&mut self, tap: u32);
}

/// The canonical teardown order — **derived**, never retyped.
///
/// The four teardown calls are the first four entries of
/// [`full_rebuild_sequence`], which this module declares exactly once (above,
/// with YV104's watchdog). This constant slices that list rather than spelling
/// the four names a second time, so there is no second copy for a later edit to
/// leave behind.
///
/// That is not a stylistic preference, it is the fix for a real near-miss.
/// While YV104 was on `main` and this tap was in review, the tap grew its own
/// four-variant `TeardownStep` enum with its own literal order. Nothing about
/// that conflicts *textually* — the rebase was clean — but the whole reason
/// `full_rebuild_sequence`'s doc comment says *"YV100 binds its FFI to this
/// list instead of writing a second copy that can drift from it"* is that a
/// second copy is precisely what a clean rebase produces. Reordering either
/// list now fails to compile or fails
/// `the_teardown_order_is_the_first_four_steps_of_the_one_declared_rebuild`.
///
/// Every exit path emits a SUBSEQUENCE of this — never a permutation, never a
/// step whose resource does not exist.
pub const TEARDOWN_ORDER: [TapStep; 4] = {
    let all = full_rebuild_sequence();
    [all[0], all[1], all[2], all[3]]
};

/// What exists right now. Every field is cleared by the step that destroys it,
/// so a second teardown is a no-op rather than a double-free.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TapResources {
    pub tap: Option<u32>,
    pub aggregate: Option<u32>,
    pub ioproc: Option<IoProcToken>,
    pub running: bool,
}

impl TapResources {
    pub fn is_empty(&self) -> bool {
        self.tap.is_none() && self.aggregate.is_none() && self.ioproc.is_none() && !self.running
    }
}

/// Tear down whatever exists, in [`TEARDOWN_ORDER`], and return the steps that
/// actually fired.
///
/// This is the first half of YV104's 7-step rebuild; that item calls this
/// function rather than writing the sequence a second time.
pub fn teardown<P: TapPlatform + ?Sized>(
    platform: &mut P,
    resources: &mut TapResources,
) -> Vec<TapStep> {
    let mut steps = Vec::with_capacity(4);
    if let (true, Some(aggregate), Some(ioproc)) =
        (resources.running, resources.aggregate, resources.ioproc)
    {
        platform.stop(aggregate, ioproc);
        steps.push(TapStep::AudioDeviceStop);
    }
    resources.running = false;
    if let (Some(aggregate), Some(ioproc)) = (resources.aggregate, resources.ioproc.take()) {
        platform.destroy_ioproc(aggregate, ioproc);
        steps.push(TapStep::AudioDeviceDestroyIOProcID);
    }
    if let Some(aggregate) = resources.aggregate.take() {
        platform.destroy_aggregate(aggregate);
        steps.push(TapStep::AudioHardwareDestroyAggregateDevice);
    }
    if let Some(tap) = resources.tap.take() {
        platform.destroy_tap(tap);
        steps.push(TapStep::AudioHardwareDestroyProcessTap);
    }
    steps
}

/// The exclusion list for the tap description — never empty.
///
/// `Ok([self])` or a refusal. There is no third case on purpose: an empty
/// `initMonoGlobalTapButExcludeProcesses` argument excludes nothing, so the tap
/// would include Yap's own output. That is a working-looking tap that records
/// the recording, and it is the exact shape of silent inversion the plan's
/// §2.1 `exclusive`-flag gotcha describes.
pub fn exclusion_list(self_process_object: Option<u32>) -> Result<[u32; 1], TapError> {
    match self_process_object {
        Some(object) if object != 0 => Ok([object]),
        _ => Err(TapError::SelfProcessObjectUnavailable),
    }
}

/// A tap that is created, wired to an aggregate device and running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTap {
    pub resources: TapResources,
    pub format: TapFormat,
    pub tap_uid: String,
    pub aggregate_uid: String,
}

/// THE SETUP SEQUENCE (pure state machine over [`TapPlatform`]).
///
/// `CATapDescription` → `AudioHardwareCreateProcessTap` → read
/// `kAudioTapPropertyUID` + `kAudioTapPropertyFormat` → resolve
/// `kAudioHardwarePropertyDefaultOutputDevice` →
/// `AudioHardwareCreateAggregateDevice` →
/// `AudioDeviceCreateIOProcIDWithBlock` → `AudioDeviceStart`.
///
/// **Every** exit path — a non-zero `OSStatus` at any step, or a panic mid-setup
/// — runs [`teardown`] over exactly what existed at that moment before
/// returning. A half-built tap is the one outcome this function will not
/// produce: a leaked process tap survives the app, and a leaked aggregate device
/// takes the user's output device with it.
pub fn open_tap<P: TapPlatform>(
    platform: &mut P,
    self_process_object: Option<u32>,
    aggregate_uid: &str,
    aggregate_name: &str,
) -> Result<OpenTap, TapError> {
    let exclude = exclusion_list(self_process_object)?;
    let resources = RefCell::new(TapResources::default());
    let stage = std::cell::Cell::new(TapStage::CreateTap);

    let attempt = catch_unwind(AssertUnwindSafe(|| -> Result<OpenTap, TapError> {
        let os = |stage: TapStage| move |status: i32| TapError::Os { stage, status };

        stage.set(TapStage::CreateTap);
        let tap = platform
            .create_tap(&exclude)
            .map_err(os(TapStage::CreateTap))?;
        resources.borrow_mut().tap = Some(tap);

        stage.set(TapStage::ReadTapUid);
        let tap_uid = platform.tap_uid(tap).map_err(os(TapStage::ReadTapUid))?;

        stage.set(TapStage::ReadTapFormat);
        let format = platform
            .tap_format(tap)
            .map_err(os(TapStage::ReadTapFormat))?;

        // The format is known and nothing has been stamped yet. This is the
        // only window in which the ring can be built from the truth, so it is
        // where it happens — and a failure here tears the tap down like any
        // other, rather than proceeding with a ring that disagrees.
        stage.set(TapStage::BindCapture);
        platform.bind_capture(format)?;

        stage.set(TapStage::ResolveDefaultOutput);
        let output_uid = platform
            .default_output_uid()
            .map_err(os(TapStage::ResolveDefaultOutput))?;

        stage.set(TapStage::CreateAggregate);
        let description = aggregate_description(&AggregateSpec {
            aggregate_uid: aggregate_uid.to_string(),
            aggregate_name: aggregate_name.to_string(),
            output_uid,
            tap_uid: tap_uid.clone(),
        });
        let aggregate = platform
            .create_aggregate(&description)
            .map_err(os(TapStage::CreateAggregate))?;
        resources.borrow_mut().aggregate = Some(aggregate);

        stage.set(TapStage::CreateIoProc);
        let ioproc = platform
            .create_ioproc(aggregate, format)
            .map_err(os(TapStage::CreateIoProc))?;
        resources.borrow_mut().ioproc = Some(ioproc);

        stage.set(TapStage::Start);
        platform
            .start(aggregate, ioproc)
            .map_err(os(TapStage::Start))?;
        resources.borrow_mut().running = true;

        Ok(OpenTap {
            resources: *resources.borrow(),
            format,
            tap_uid,
            aggregate_uid: aggregate_uid.to_string(),
        })
    }));

    match attempt {
        Ok(Ok(open)) => Ok(open),
        Ok(Err(err)) => {
            let mut live = resources.borrow_mut();
            teardown(platform, &mut live);
            Err(err)
        }
        Err(_) => {
            // A panic mid-setup is the path that leaks if it is not handled
            // here: `?` unwinding past the platform calls would drop a tap and
            // an aggregate device that only CoreAudio can free.
            let mut live = resources.borrow_mut();
            teardown(platform, &mut live);
            Err(TapError::PanicDuringSetup { stage: stage.get() })
        }
    }
}

// ── The IOProc body ────────────────────────────────────────────────────────

/// THE `extern "C-unwind"` GUARD (OS-6, correction #2).
///
/// The block registered with `AudioDeviceCreateIOProcIDWithBlock` runs on the
/// aggregate device's real-time IO thread, behind an `extern "C-unwind"`
/// boundary. A Rust panic that reaches that boundary is undefined behaviour and
/// a HAL-level process kill. So the *whole* block body — including the
/// `AudioBufferList` decode, which is where an off-by-one becomes a panic —
/// runs inside this guard, which catches, flags and returns.
///
/// Real-time safe on the happy path: `catch_unwind` costs nothing when nothing
/// unwinds, and the flag is a `Relaxed` atomic store only on the failure path.
/// `tests/syscapture_ioproc_rt_safety.rs` proves the zero-allocation claim with
/// a scoped allocator hook; `tests/syscapture_ioproc_catch_unwind.rs` proves the
/// catch.
///
/// The flag it sets is the same [`RtCapture::callback_panicked`] the mic path
/// already exposes — YV104's watchdog reads one flag, not two.
pub fn tap_ioproc_guarded<R>(capture: &RtCapture, body: impl FnOnce() -> R) -> Option<R> {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => Some(value),
        Err(_) => {
            capture.note_callback_panic();
            None
        }
    }
}

// ── The tap's host clock: rebased to ITS OWN first callback ────────────────

/// `mHostTime` is mach absolute time — *ticks since boot*, order 10^12 ns after
/// a few hours of uptime. [`CaptureAnchor::host_ns`](crate::rtring::CaptureAnchor)
/// is not that. The mic path stamps
/// `captured_at.duration_since(first_callback)` (`record.rs`, `build_capture_stream`),
/// i.e. a **stream-relative** nanosecond clock whose first anchor is `0`, and
/// `meeting.rs`'s splice planner documents the convention in those words:
/// *"record 0's `host_ns` is an arbitrary stream-relative instant, not zero"* —
/// arbitrary in WHICH instant it corresponds to, but starting at zero for the
/// stream that produced it.
///
/// So the tap converts to the same shape or it is in a different time domain
/// than the track it will be merged against. Single-track logic would not
/// notice: `plan_silence_splices` only ever takes *differences* within one
/// track, and a constant offset cancels. YV106's two-track merge is where it
/// would land, and it would land as Track 1 sitting hours-to-days away from
/// Track 0 — silently, with every per-track number still self-consistent.
///
/// Hence this: the first tick value the tap's IOProc ever sees becomes the
/// epoch, and every anchor is the delta from it in nanoseconds. Per *stream*,
/// not per process — a rebuilt tap (YV104's 7-step rebuild) gets a fresh
/// `TapClock` and rebases to zero again, exactly as a rebuilt cpal stream does
/// on the mic side.
///
/// Real-time safe: one relaxed load, at most one `compare_exchange` (only the
/// first callback ever takes the store path), one multiply and one divide. No
/// allocation, no lock, no syscall — [`TapClock::new`] pays the one syscall
/// (`mach_timebase_info`) up front, off the IO thread.
pub struct TapClock {
    /// The first `mHostTime` this stream delivered, in raw mach ticks, or
    /// [`Self::UNSET`].
    ///
    /// One atomic and not a `(bool, u64)` pair on purpose: a flag plus a value
    /// is two stores and therefore a race a second IOProc invocation can land
    /// inside. `compare_exchange` makes "claim the epoch" a single operation.
    first_ticks: AtomicU64,
}

impl TapClock {
    /// "No callback has arrived yet." `u64::MAX` and not `0`: `mHostTime` of
    /// exactly zero is a legal (if degenerate) reading at the instant of boot,
    /// while `u64::MAX` ticks is ~12 years of uptime at the Apple-silicon
    /// timebase, so the sentinel cannot collide with a real epoch that matters.
    const UNSET: u64 = u64::MAX;

    /// Builds a clock and **primes the mach timebase**.
    ///
    /// Called from `create_ioproc`, before the block is registered — the
    /// timebase read is a one-time `mach_timebase_info` syscall and the comment
    /// on [`mach_ticks_to_ns`] says the IOProc must not make it. Before this,
    /// the very first callback made it, on the real-time thread.
    pub fn new() -> Self {
        let _ = mach_ticks_to_ns(0);
        Self {
            first_ticks: AtomicU64::new(Self::UNSET),
        }
    }

    /// Raw `mHostTime` in, stream-relative nanoseconds out. The first call
    /// returns `0` by construction.
    pub fn stamp(&self, host_ticks: u64) -> u64 {
        let first = match self.first_ticks.compare_exchange(
            Self::UNSET,
            host_ticks,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => host_ticks,
            Err(already) => already,
        };
        // `saturating_sub`: a timestamp that runs backwards (a device the HAL
        // re-timed under us) clamps to the epoch instead of wrapping to ~10^19,
        // which is what `plan_silence_splices` already treats as "changed
        // clock, skip this interval" rather than a negative gap.
        mach_ticks_to_ns(host_ticks.saturating_sub(first))
    }
}

impl Default for TapClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Mach ticks → nanoseconds.
///
/// Lives at module level, not inside [`imp`], so the test that checks the tap's
/// anchors are in the mic path's domain can drive the real conversion instead of
/// a hand-written stand-in.
// `libc`'s `mach_timebase_info` carries a deprecation pointing at the `mach2`
// crate. Taking that advice would add a crate to a signed bundle to read two
// `u32`s that have not changed since 2006; the declaration is stable ABI and is
// called ONCE per process, so the deprecation is acknowledged and declined here
// rather than paid for in supply chain.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn mach_ticks_to_ns(ticks: u64) -> u64 {
    // `mach_timebase_info` is constant for the life of the process, so it is
    // read once and cached — the IOProc must not make this call, which is why
    // `TapClock::new` forces it off the real-time thread.
    static NUMER: AtomicU64 = AtomicU64::new(0);
    static DENOM: AtomicU64 = AtomicU64::new(0);
    let (mut numer, mut denom) = (NUMER.load(Ordering::Relaxed), DENOM.load(Ordering::Relaxed));
    if denom == 0 {
        let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: `info` is a valid out-parameter for the whole call.
        let rc = unsafe { libc::mach_timebase_info(&mut info) };
        (numer, denom) = if rc == 0 && info.denom != 0 {
            (info.numer as u64, info.denom as u64)
        } else {
            (1, 1)
        };
        NUMER.store(numer, Ordering::Relaxed);
        DENOM.store(denom, Ordering::Relaxed);
    }
    ticks.saturating_mul(numer) / denom
}

/// Off macOS there is no mach clock and no tap; the identity keeps the pure
/// state machine and its tests compiling on any host.
#[cfg(not(target_os = "macos"))]
fn mach_ticks_to_ns(ticks: u64) -> u64 {
    ticks
}

// ── Wiring into a meeting session ──────────────────────────────────────────

/// The `SessionConfig` a tap-backed (virtual-meeting) session runs under.
///
/// Two tracks — the mic keeps writing track 0 through its own `MicStream`, and
/// the tap is the second producer `MeetingJournal::start(dir, tracks)` was
/// generalized for — and [`ExternalStream`], because a CoreAudio tap owns its
/// own IOProc and has no cpal stream for the session to hold open. Both of those
/// seams already exist and already say in their own doc comments that 22-B is
/// what they were built for; this function is that claim being cashed.
///
/// The per-track DSP/epoch bookkeeping that turns two producers into two
/// finished tracks is **YV106**. This builds the config; it does not start
/// anything.
pub fn virtual_meeting_config(dir: impl Into<PathBuf>, format: TapFormat) -> SessionConfig {
    let mut config = SessionConfig::new(dir, format.sample_rate, format.channels);
    config.tracks = 2;
    config.stream = Arc::new(ExternalStream);
    config
}

// ── YV102 · the TCC pre-warm ───────────────────────────────────────────────
//
// OS-10's finding, in one line from AudioCap's own README: *"There's no public
// API to request audio recording permission or to check if the app has that
// permission."* The system alert is a SIDE EFFECT of creating and starting a
// process tap. There is no `requestAccess`, no `authorizationStatus`, and — the
// part that makes this an item rather than a footnote — **if the user dismisses
// or denies it, TCC does not ask again**.
//
// Left where the plan originally had it, that prompt lands at T-0 of the user's
// first real meeting: mid-Zoom-join, focus stolen (a system alert ignores the
// pill's `.nonactivatingPanel` politeness entirely), zero context for what is
// being asked, and a denial there is terminal for that install. So Yap provokes
// it on purpose, from an explicit Settings step, where the sentence explaining
// what is about to happen is already on screen.
//
// The pre-warm is the smallest thing that is still a real tap: YV100's exact
// setup sequence, a 200 ms dwell, every sample discarded, then YV100's exact
// teardown. Not a second implementation of either — [`open_tap`] and
// [`teardown`] are called, so a change to the ordering contract cannot drift
// away from the surface that exercises it most often.

/// How long the pre-warm holds the tap open.
///
/// Long enough that `AudioDeviceStart` has actually started and the IOProc has
/// had a chance to fire; short enough that the Settings step feels like a
/// button, not a wait. Nothing is kept: the ring the callback writes into is
/// read once for the discriminator below and dropped with the function.
pub const PREWARM_DWELL: Duration = Duration::from_millis(200);

/// What one pre-warm run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prewarm {
    /// The tap was created, wired, and started.
    pub opened: bool,
    /// Why it was not, when it was not.
    pub error: Option<TapError>,
    /// The teardown steps THIS function issued, in the order it issued them.
    ///
    /// Empty on the failure path, and that is not a gap: [`open_tap`] runs the
    /// same [`teardown`] over whatever existed at the moment it gave up, before
    /// it returns the error. The platform's own call log is what proves that
    /// half — see `tests/syscapture_prewarm_tap.rs`, which asserts against the
    /// fake's recorded calls rather than against this field.
    pub teardown_steps: Vec<TeardownStep>,
}

impl Prewarm {
    /// Did the full four-call teardown fire from this function?
    pub fn tore_down_completely(&self) -> bool {
        self.teardown_steps == TEARDOWN_ORDER
    }
}

/// Create a tap, start it, hold it for `dwell`, throw away everything it heard,
/// and tear it down. **This is the permission request** — there is no other.
///
/// `dwell` is injected rather than hardcoded so the state machine is testable
/// without sleeping: production passes a `sleep(PREWARM_DWELL)`, the test passes
/// a closure that records a sample or records nothing. It runs inside
/// `catch_unwind` for the same reason [`open_tap`] does: a panic between "the
/// tap is running" and "the tap is destroyed" leaves a private aggregate device
/// holding the user's output device until they log out.
pub fn prewarm_tap<P: TapPlatform>(
    platform: &mut P,
    self_process_object: Option<u32>,
    aggregate_uid: &str,
    aggregate_name: &str,
    dwell: impl FnOnce(),
) -> Prewarm {
    match open_tap(platform, self_process_object, aggregate_uid, aggregate_name) {
        Ok(open) => {
            let mut resources = open.resources;
            // Deliberately ignored: a panic in the dwell is not a reason to
            // leak a process tap, and there is nothing to report — the caller
            // already knows the tap opened.
            let _ = catch_unwind(AssertUnwindSafe(dwell));
            let teardown_steps = teardown(platform, &mut resources);
            debug_assert!(resources.is_empty(), "pre-warm left a resource behind");
            Prewarm {
                opened: true,
                error: None,
                teardown_steps,
            }
        }
        Err(error) => Prewarm {
            opened: false,
            error: Some(error),
            teardown_steps: Vec::new(),
        },
    }
}

// ── YV102 · the denial discriminator ───────────────────────────────────────
//
// A denied tap is SILENT. So is OS-4's ghost tap — a healthy tap that starts
// delivering all-zero buffers after minutes of good audio, for as long as 16
// minutes at a stretch ([Apple Developer Forums 825780]). So is a call where
// everybody is muted, and so is a tap whose aggregate is pointed at a different
// output device than the app is rendering to (routine with AirPods; YV103).
// §2.1's original heuristic — "silence for N seconds ⇒ permission looks denied"
// — cannot tell any of those apart, and badging a healthy meeting "permission
// revoked" is worse than saying nothing.
//
// The one bit that DOES separate them needs no private API and no new data:
//
//     did this tap EVER deliver a non-zero sample?
//
// A TCC denial is silent from sample zero, forever. Every other cause of
// silence has audio before it or after it. That single boolean is a fold over
// [`CaptureAnchor`]s and sample blocks YV100 already ships, and it is the only
// thing this item's denied-state UI is allowed to read.
//
// The private-TCC-framework path AudioCap documents is NOT shipped, not even
// behind a flag: a Developer-ID-signed, notarized, Accessibility-trusted,
// keystroke-synthesizing app calling private TCC symbols is the exact pattern
// that gets a notarization ticket pulled (OS-10), and this bit answers the same
// question legitimately.

/// How long a tap must have been running before "never delivered anything" is
/// allowed to mean anything at all.
///
/// Below this, the honest answer is [`SystemAudioPermission::Unknown`]: a tap
/// that has been open for 200 ms and heard nothing has told you that nothing
/// was playing, not that you were refused.
pub const DENIAL_GRACE: Duration = Duration::from_secs(3);

/// The running evidence the verdict is a pure function of.
///
/// Consumer-side: fed from the drained ring, never from the IOProc. Nothing
/// here runs on the real-time thread, so `observe` is free to be ordinary code.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TapDelivery {
    /// Callbacks observed. Zero is itself a signal — a denied tap on some
    /// systems never fires at all rather than firing silence.
    pub callbacks: u64,
    /// Per-channel frames the tap handed over.
    pub frames: u64,
    /// **The bit.** Set by the first sample that is not exactly zero, and never
    /// cleared. Once true, this session can never be reported as denied.
    pub ever_nonzero: bool,
}

impl TapDelivery {
    /// Fold one callback's worth of the tap into the evidence.
    pub fn observe(&mut self, anchor: &CaptureAnchor, samples: &[f32]) {
        self.callbacks += 1;
        self.frames += u64::from(anchor.frames);
        if !self.ever_nonzero {
            self.ever_nonzero = samples.iter().any(|s| *s != 0.0);
        }
    }

    /// Has this tap ever produced audio? The whole discriminator.
    pub fn ever_delivered(&self) -> bool {
        self.ever_nonzero
    }
}

/// What Yap is willing to SAY about system-audio permission.
///
/// Three values on purpose. Two would force silence to mean denial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemAudioPermission {
    /// Not enough has happened to say anything. The resting state, and the one
    /// a 200 ms pre-warm almost always lands in — the prompt has been shown,
    /// which is what the pre-warm was for.
    Unknown,
    /// At least one non-zero sample arrived. Permission is granted, and this
    /// verdict is **sticky for the session**: silence after this point is
    /// YV104's ghost-tap territory and must never be re-badged as a denial.
    Granted,
    /// The tap ran for at least [`DENIAL_GRACE`] and has never, once, delivered
    /// a non-zero sample. "Looks denied" — never "denied", because Yap cannot
    /// read TCC and will not claim it can.
    LooksDenied,
}

impl SystemAudioPermission {
    /// The sentence the UI shows. One place, so the copy cannot drift between
    /// the Settings step, the permissions list and the meeting banner.
    pub fn message(self) -> &'static str {
        match self {
            SystemAudioPermission::Unknown => {
                "Yap has not heard any system audio yet. macOS shows a purple \
                 dot in the menu bar whenever it is capturing."
            }
            SystemAudioPermission::Granted => {
                "System audio is being captured. macOS shows a purple dot in \
                 the menu bar the whole time."
            }
            SystemAudioPermission::LooksDenied => LOOKS_DENIED_MESSAGE,
        }
    }

    pub fn looks_denied(self) -> bool {
        matches!(self, SystemAudioPermission::LooksDenied)
    }
}

/// The denied-state sentence, and the only place it is written.
///
/// It says "has not granted", not "you denied": the user may have dismissed the
/// alert, or never seen it. It names the exact pane
/// ([`crate::permissions::SYSTEM_AUDIO_PANE`] opens it) because after a denial a
/// deep link is the ONLY recovery — TCC will not ask a second time.
pub const LOOKS_DENIED_MESSAGE: &str =
    "Yap has not received any system audio. macOS has not granted System Audio \
     Recording to Yap, and it will not ask again — allow it in System Settings, \
     then start a new meeting.";

/// The verdict: pure, total, and the only place the comparison is made.
///
/// Note the order of the arms. `ever_delivered` wins over everything, including
/// a long run of silence, because that is precisely the case §2.1's heuristic
/// got wrong.
pub fn permission_verdict(delivery: &TapDelivery, ran_for: Duration) -> SystemAudioPermission {
    if delivery.ever_delivered() {
        SystemAudioPermission::Granted
    } else if ran_for >= DENIAL_GRACE {
        SystemAudioPermission::LooksDenied
    } else {
        SystemAudioPermission::Unknown
    }
}

// ── The real CoreAudio implementation ──────────────────────────────────────

#[cfg(target_os = "macos")]
pub mod imp {
    //! The only part of this module that needs a Mac, a 14.4 kernel and a TCC
    //! grant. Everything above is provable without any of the three.
    //!
    //! ## Why the four 14.4-era entry points are resolved by `dlsym`
    //!
    //! `minimumSystemVersion` is `12.0` and stays there (OS-11, §2.1). If this
    //! module *imported* `AudioHardwareCreateProcessTap` and the three other
    //! availability-gated symbols the ordinary way, they would land in the
    //! shipped Mach-O as plain `(undefined) external`, and **dyld binds those at
    //! LOAD time, before any Rust code runs**. Not "the tap fails on macOS 13" —
    //! *Yap does not launch at all* on macOS 12/13, dictation included. Having
    //! no call site does not help: linkage is decided by what is linked, not by
    //! what is called. (`AudioHardwareCreate/DestroyAggregateDevice` are 13.0+,
    //! so the blast radius is macOS 12 as well, not just pre-14.4.)
    //!
    //! So the four functions are looked up once, at runtime, through
    //! `dlsym(RTLD_DEFAULT, …)`, and the `CATapDescription` class through
    //! `AnyClass::get` (objc2 already resolves ObjC classes by name — there is
    //! no `_OBJC_CLASS_$_CATapDescription` import in the binary either). If any
    //! of the five is missing, [`CoreAudioPlatform::new`] refuses with
    //! [`TapError::ProcessTapApiUnavailable`] and nothing is ever called through
    //! a null pointer. The remaining CoreAudio calls this module makes
    //! (`AudioObjectGetPropertyData`, `AudioDeviceCreateIOProcIDWithBlock`,
    //! `AudioDeviceStart/Stop/DestroyIOProcID`) are 10.x-era symbols that exist
    //! on every OS Yap supports, so they stay ordinary imports.
    //!
    //! **This module is not the only source of those imports**, which is why
    //! `build.rs` also weak-links the CoreAudio framework: cpal 0.18.1's
    //! `host/coreaudio/macos/loopback.rs` calls all four, from an object the
    //! microphone path keeps live, and `origin/main` already shipped them as
    //! hard imports before this file existed. Weak linking makes dyld bind the
    //! missing ones to NULL and launch the app; this module's `dlsym` probe is
    //! what turns "NULL" into a typed refusal instead of a crash.
    //!
    //! `scripts/assert-weak-linked-14_4-symbols.sh` (YV101's, which landed this
    //! check on `main` while this item was in review) runs in CI against the
    //! actual release binary and fails the build if any of the four ever comes
    //! back as a non-weak import. It carries two controls of its own so it
    //! cannot pass vacuously. It is the falsifiable half of this comment; the
    //! comment alone is worth nothing.
    //!
    //! **Still no call site.** Nothing in the app calls [`start_system_tap`]
    //! until YV101 lands the user-visible macOS 14.4 gate
    //! (`MeetingUnavailable::RequiresMacOS14_4`), the `NSAudioCaptureUsageDescription`
    //! string and the signed-build TCC acceptance. What changed here is only
    //! that the *absence* of a call site is no longer load-bearing.

    use std::ffi::{c_void, CStr};
    use std::ptr::NonNull;
    use std::sync::Arc;

    use block2::RcBlock;
    use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
    use objc2::rc::Retained;
    use objc2::runtime::AnyClass;
    use objc2::AllocAnyThread;
    use objc2_core_audio::{
        kAudioDevicePropertyDeviceUID, kAudioHardwarePropertyDefaultOutputDevice,
        kAudioHardwarePropertyTranslatePIDToProcessObject, kAudioObjectPropertyElementMain,
        kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioTapPropertyFormat,
        kAudioTapPropertyUID, AudioDeviceCreateIOProcIDWithBlock, AudioDeviceDestroyIOProcID,
        AudioDeviceStart, AudioDeviceStop, AudioObjectGetPropertyData, AudioObjectID,
        AudioObjectPropertyAddress, CATapDescription,
    };
    use objc2_core_audio_types::{AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp};
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSObject, NSString};

    use super::{
        keys, permission_verdict, teardown, DictValue, IoProcToken, OpenTap, SystemAudioPermission,
        TapClock, TapDelivery, TapError, TapFormat, TapPlatform, TapResources, PREWARM_DWELL,
    };
    use crate::meeting::{rt_capture_callback, RtCapture};
    use crate::rtring::CaptureAnchor;

    // ── The availability-gated symbols, resolved at runtime ────────────────

    /// `dlsym`'s "search every image already loaded into this process" handle.
    ///
    /// `libc` declares `dlsym` but not this constant on Apple platforms;
    /// `<dlfcn.h>` spells it `((void *) -2)` and has since 10.3. CoreAudio is
    /// unconditionally loaded here — the app links `AudioObjectGetPropertyData`
    /// and cpal links the framework too — so no `dlopen` is needed.
    const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

    /// `OSStatus AudioHardwareCreateProcessTap(CATapDescription *, AudioObjectID *)`
    /// — macOS 14.4+.
    type CreateProcessTapFn = unsafe extern "C-unwind" fn(*const c_void, *mut AudioObjectID) -> i32;
    /// `OSStatus AudioHardwareDestroyProcessTap(AudioObjectID)` — macOS 14.4+.
    type DestroyProcessTapFn = unsafe extern "C-unwind" fn(AudioObjectID) -> i32;
    /// `OSStatus AudioHardwareCreateAggregateDevice(CFDictionaryRef, AudioObjectID *)`
    /// — macOS 13.0+ (this one breaks macOS 12 on its own).
    type CreateAggregateFn = unsafe extern "C-unwind" fn(*const c_void, *mut AudioObjectID) -> i32;
    /// `OSStatus AudioHardwareDestroyAggregateDevice(AudioObjectID)` — macOS 13.0+.
    type DestroyAggregateFn = unsafe extern "C-unwind" fn(AudioObjectID) -> i32;

    /// The names, in the order [`TapSymbols::resolve`] looks them up. Also the
    /// list `scripts/assert-weak-linked-14_4-symbols.sh` asserts is absent (or
    /// weak) in the shipped binary — two copies of one list, in two languages,
    /// which is a drift waiting to happen. `tests/supply_chain.rs`'s
    /// `the_weak_link_ci_script_and_the_dlsym_list_are_the_same_four_symbols`
    /// reads the script and keeps them in step.
    pub const AVAILABILITY_GATED_SYMBOLS: [&str; 4] = [
        "AudioHardwareCreateProcessTap",
        "AudioHardwareDestroyProcessTap",
        "AudioHardwareCreateAggregateDevice",
        "AudioHardwareDestroyAggregateDevice",
    ];

    /// The `CATapDescription` class is 14.4-gated too. objc2 resolves ObjC
    /// classes by name at runtime already, so this is a check, not a fix — but
    /// it belongs in the same probe, because a resolved function set plus a
    /// missing class is still a crash.
    const TAP_DESCRIPTION_CLASS: &CStr = c"CATapDescription";

    /// The four entry points, resolved once and null-checked once.
    ///
    /// Holding them as fields is the point: after [`TapSymbols::resolve`]
    /// returns, every call site below goes through a pointer that has already
    /// been proven non-null, and there is no path that can call an absent
    /// symbol.
    #[derive(Clone, Copy)]
    struct TapSymbols {
        create_process_tap: CreateProcessTapFn,
        destroy_process_tap: DestroyProcessTapFn,
        create_aggregate: CreateAggregateFn,
        destroy_aggregate: DestroyAggregateFn,
    }

    /// `dlsym(RTLD_DEFAULT, name)`, or `None` when this OS does not have it.
    fn lookup(name: &CStr) -> Option<NonNull<c_void>> {
        // SAFETY: `name` is a NUL-terminated C string, and RTLD_DEFAULT is the
        // documented pseudo-handle for "the images already loaded".
        NonNull::new(unsafe { libc::dlsym(RTLD_DEFAULT, name.as_ptr()) })
    }

    impl TapSymbols {
        /// Resolve all five (four functions + the description class) or name the
        /// first one missing. All-or-nothing on purpose: a partial set is how a
        /// tap gets created on an OS that cannot destroy it.
        fn resolve() -> Result<Self, TapError> {
            fn get<F: Copy>(c_name: &CStr, name: &'static str) -> Result<F, TapError> {
                debug_assert_eq!(c_name.to_bytes(), name.as_bytes());
                assert_eq!(
                    std::mem::size_of::<F>(),
                    std::mem::size_of::<*mut c_void>(),
                    "F must be a plain (non-nullable) function pointer"
                );
                let symbol =
                    lookup(c_name).ok_or(TapError::ProcessTapApiUnavailable { symbol: name })?;
                // SAFETY: `F` is one of the fn-pointer aliases above, declared
                // with the exact C signature CoreAudio publishes for `name`;
                // `symbol` is non-null and the sizes were just asserted equal.
                Ok(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&symbol.as_ptr()) })
            }

            let symbols = Self {
                create_process_tap: get(
                    c"AudioHardwareCreateProcessTap",
                    AVAILABILITY_GATED_SYMBOLS[0],
                )?,
                destroy_process_tap: get(
                    c"AudioHardwareDestroyProcessTap",
                    AVAILABILITY_GATED_SYMBOLS[1],
                )?,
                create_aggregate: get(
                    c"AudioHardwareCreateAggregateDevice",
                    AVAILABILITY_GATED_SYMBOLS[2],
                )?,
                destroy_aggregate: get(
                    c"AudioHardwareDestroyAggregateDevice",
                    AVAILABILITY_GATED_SYMBOLS[3],
                )?,
            };
            if AnyClass::get(TAP_DESCRIPTION_CLASS).is_none() {
                return Err(TapError::ProcessTapApiUnavailable {
                    symbol: "CATapDescription",
                });
            }
            Ok(symbols)
        }
    }

    /// Is the process-tap API present in *this* process?
    ///
    /// The honest, user-visible gate — the disabled affordance and its plain
    /// sentence — is YV101's `os_version_gate.rs` and reads the OS version.
    /// This is the linkage probe underneath it: whether the four entry points
    /// are actually resolvable in *this* process, which is a different question
    /// from what the OS claims to be. Nothing calls it yet, for the same reason
    /// nothing calls [`start_system_tap`] yet.
    pub fn process_tap_api_available() -> bool {
        TapSymbols::resolve().is_ok()
    }

    fn address(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        }
    }

    /// Read a fixed-size property off an audio object.
    ///
    /// # Safety
    /// `T` must be the exact type CoreAudio writes for `selector`.
    unsafe fn get_property<T>(object: AudioObjectID, selector: u32) -> Result<T, i32> {
        let address = address(selector);
        let mut value = std::mem::MaybeUninit::<T>::zeroed();
        let mut size = std::mem::size_of::<T>() as u32;
        let status = AudioObjectGetPropertyData(
            object,
            NonNull::from(&address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(value.as_mut_ptr().cast::<c_void>()).expect("stack pointer is non-null"),
        );
        if status == 0 {
            Ok(value.assume_init())
        } else {
            Err(status)
        }
    }

    /// Read a `CFStringRef`-valued property and copy it into a Rust `String`.
    /// CoreFoundation gives us a +1 reference here; `Retained` takes ownership
    /// of it (these properties are documented as "the caller is responsible for
    /// releasing the returned CFString").
    fn get_string_property(object: AudioObjectID, selector: u32) -> Result<String, i32> {
        // SAFETY: both `kAudioTapPropertyUID` and `kAudioDevicePropertyDeviceUID`
        // are documented as CFStringRef-valued, and NSString is toll-free
        // bridged with CFString.
        let raw: *mut NSString = unsafe { get_property(object, selector)? };
        let owned = NonNull::new(raw)
            .map(|p| unsafe { Retained::from_raw(p.as_ptr()) })
            .flatten()
            .ok_or(-1i32)?;
        Ok(owned.to_string())
    }

    /// Our own audio process object, translated from our PID.
    ///
    /// This is the ONE PID translation this design makes, and it is the safe
    /// one: OS-5's finding is about translating somebody *else's* UI PID and
    /// getting a process that renders no audio. Ours is the process we want to
    /// exclude, and if it cannot be resolved [`super::exclusion_list`] refuses
    /// to build a description at all.
    pub fn self_process_object() -> Option<u32> {
        let address = address(kAudioHardwarePropertyTranslatePIDToProcessObject);
        let pid: i32 = std::process::id() as i32;
        let mut object: AudioObjectID = 0;
        let mut size = std::mem::size_of::<AudioObjectID>() as u32;
        // SAFETY: the qualifier is a `pid_t` as the selector documents, and the
        // out-parameter is a stack `AudioObjectID` of the size we pass.
        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject as AudioObjectID,
                NonNull::from(&address),
                std::mem::size_of::<i32>() as u32,
                (&pid as *const i32).cast::<c_void>(),
                NonNull::from(&mut size),
                NonNull::from(&mut object).cast::<c_void>(),
            )
        };
        (status == 0 && object != 0).then_some(object)
    }

    /// Build the CoreFoundation dictionary from the pure description.
    ///
    /// NSDictionary/NSArray/NSNumber/NSString are toll-free bridged with their
    /// CF counterparts, so the composed object is passed straight to
    /// `AudioHardwareCreateAggregateDevice` — which is why this file needs no
    /// hand-rolled CFDictionary construction.
    fn to_ns_object(value: &DictValue) -> Retained<NSObject> {
        match value {
            DictValue::Str(s) => Retained::into_super(NSString::from_str(s)),
            // NSNumber → NSValue → NSObject: two supers, because CoreAudio wants
            // a CFBoolean here and the toll-free bridge is through NSNumber.
            DictValue::Bool(b) => {
                Retained::into_super(Retained::into_super(NSNumber::new_bool(*b)))
            }
            DictValue::List(items) => {
                let objects: Vec<Retained<NSObject>> = items.iter().map(to_ns_object).collect();
                let refs: Vec<&NSObject> = objects.iter().map(|o| &**o).collect();
                Retained::into_super(NSArray::from_slice(&refs))
            }
            DictValue::Dict(entries) => Retained::into_super(to_ns_dictionary(entries)),
        }
    }

    fn to_ns_dictionary(
        entries: &[(String, DictValue)],
    ) -> Retained<NSDictionary<NSString, NSObject>> {
        let keys: Vec<Retained<NSString>> =
            entries.iter().map(|(k, _)| NSString::from_str(k)).collect();
        let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let values: Vec<Retained<NSObject>> =
            entries.iter().map(|(_, v)| to_ns_object(v)).collect();
        let value_refs: Vec<&NSObject> = values.iter().map(|v| &**v).collect();
        NSDictionary::from_slices(&key_refs, &value_refs)
    }

    /// The live IOProc's owned state. Dropping it releases the block and the
    /// queue; CoreAudio holds its own reference until `AudioDeviceDestroyIOProcID`,
    /// which is why teardown order matters here too.
    struct LiveIoProc {
        id: objc2_core_audio::AudioDeviceIOProcID,
        /// The aggregate device this IOProc belongs to.
        ///
        /// `AudioDeviceDestroyIOProcID` needs it, and the two paths that must
        /// destroy an IOProc without a caller handing one over — a rebuild that
        /// creates a second one, and a panic between `create_ioproc` returning
        /// and its token reaching `TapResources` — have no other way to know it.
        aggregate: objc2_core_audio::AudioObjectID,
        _block: RcBlock<
            dyn Fn(
                NonNull<AudioTimeStamp>,
                NonNull<AudioBufferList>,
                NonNull<AudioTimeStamp>,
                NonNull<AudioBufferList>,
                NonNull<AudioTimeStamp>,
            ),
        >,
        _queue: DispatchRetained<DispatchQueue>,
    }

    /// `kAudioHardwareIllegalOperationError` ('!hog'), returned when
    /// `create_ioproc` is reached without a bound capture ring. Unreachable
    /// through [`start_system_tap`]; it exists so the impossible path is an
    /// error with a name rather than an `unwrap`.
    const CAPTURE_NOT_BOUND: i32 = i32::from_be_bytes(*b"!hog");

    /// The real CoreAudio platform.
    pub struct CoreAudioPlatform {
        /// `None` until [`TapPlatform::bind_capture`] runs, which is after
        /// `kAudioTapPropertyFormat` has been read and before the IOProc that
        /// stamps through it exists. There is deliberately no way to construct
        /// this platform with a ring already in it: the format is not knowable
        /// that early, and a guessed ring is a wrong sample axis with no
        /// symptom (see [`capture_matches_format`]).
        capture: Option<Arc<RtCapture>>,
        live: Option<LiveIoProc>,
        symbols: TapSymbols,
    }

    impl CoreAudioPlatform {
        /// Fails — before anything is created — when this OS has no process-tap
        /// API. Constructing the platform IS the availability check, so there is
        /// no ordering in which a call goes out to an unresolved symbol.
        pub fn new() -> Result<Self, TapError> {
            Ok(Self {
                capture: None,
                live: None,
                symbols: TapSymbols::resolve()?,
            })
        }

        /// The ring this platform bound to the tap's own format, once
        /// `bind_capture` has run.
        pub fn capture(&self) -> Option<&Arc<RtCapture>> {
            self.capture.as_ref()
        }

        /// Destroy whatever IOProc this platform is currently holding, if any.
        ///
        /// Idempotent, and the single place `AudioDeviceDestroyIOProcID` is
        /// called from: the ordinary teardown, a rebuild that creates a second
        /// IOProc, and `Drop` all route through here, so there is one
        /// destroy-and-release pair rather than three.
        fn destroy_live_ioproc(&mut self) {
            if let Some(live) = self.live.take() {
                // SAFETY: `live.id` came from `AudioDeviceCreateIOProcIDWithBlock`
                // on `live.aggregate`, and `live` is consumed here so the block
                // and the queue are released only after CoreAudio has let go.
                unsafe { AudioDeviceDestroyIOProcID(live.aggregate, live.id) };
            }
        }
    }

    /// The backstop for the one window `TapResources` cannot cover.
    ///
    /// `open_tap` records the IOProc token into `TapResources` on the line
    /// *after* `create_ioproc` returns, and `teardown` destroys an IOProc only
    /// when that field is `Some`. A panic in between — inside this platform,
    /// after CoreAudio has already registered the proc — therefore tears down
    /// the aggregate and the tap and leaves the IOProc behind, holding its block
    /// and its dispatch queue, with no handle anywhere that could free it.
    ///
    /// The window is narrow and the consequence is not: an IOProc registered
    /// against a destroyed aggregate device is exactly the half-built state the
    /// whole teardown sequence exists to make impossible. So the platform owns
    /// its own IOProc for the platform's whole life, and drops it when it dies.
    /// On every ordinary path `self.live` is already `None` by then and this
    /// does nothing.
    impl Drop for CoreAudioPlatform {
        fn drop(&mut self) {
            self.destroy_live_ioproc();
        }
    }

    impl TapPlatform for CoreAudioPlatform {
        fn create_tap(&mut self, exclude_process_objects: &[u32]) -> Result<u32, i32> {
            debug_assert!(
                !exclude_process_objects.is_empty(),
                "an empty exclusion list is a global tap that records Yap itself"
            );
            let numbers: Vec<Retained<NSNumber>> = exclude_process_objects
                .iter()
                .map(|id| NSNumber::new_u32(*id))
                .collect();
            let refs: Vec<&NSNumber> = numbers.iter().map(|n| n.as_ref()).collect();
            let excluded = NSArray::from_slice(&refs);
            // SAFETY: the initializer takes an NSArray of NSNumbers holding
            // AudioObjectIDs, which is exactly what was built above.
            let description = unsafe {
                CATapDescription::initMonoGlobalTapButExcludeProcesses(
                    CATapDescription::alloc(),
                    &excluded,
                )
            };
            let mut tap: AudioObjectID = 0;
            // SAFETY: `description` is live for the call, `tap` is a valid
            // out-parameter, and `create_process_tap` is the dlsym-resolved
            // `AudioHardwareCreateProcessTap` — a `CATapDescription *` first
            // argument, which is what `Retained::as_ptr` hands over.
            let status = unsafe {
                (self.symbols.create_process_tap)(
                    Retained::as_ptr(&description).cast::<c_void>(),
                    &mut tap,
                )
            };
            if status == 0 && tap != 0 {
                Ok(tap)
            } else {
                Err(if status == 0 { -1 } else { status })
            }
        }

        fn tap_uid(&mut self, tap: u32) -> Result<String, i32> {
            get_string_property(tap, kAudioTapPropertyUID)
        }

        fn tap_format(&mut self, tap: u32) -> Result<TapFormat, i32> {
            // SAFETY: `kAudioTapPropertyFormat` is documented as an
            // AudioStreamBasicDescription.
            let asbd: AudioStreamBasicDescription =
                unsafe { get_property(tap, kAudioTapPropertyFormat)? };
            let channels = asbd.mChannelsPerFrame.max(1).min(u16::MAX as u32) as u16;
            let rate = asbd.mSampleRate;
            if !(rate.is_finite() && rate > 0.0) {
                return Err(-1);
            }
            Ok(TapFormat {
                sample_rate: rate.round() as u32,
                channels,
            })
        }

        fn bind_capture(&mut self, format: TapFormat) -> Result<(), TapError> {
            // Built from the format the tap just reported, never from a value a
            // caller carried in. Then checked anyway: `RtCapture::new` clamps
            // `channels`, so "constructed from it" and "agrees with it" are not
            // the same statement, and the one the IOProc depends on is the
            // second one.
            let capture = super::capture_for_format(format);
            super::capture_matches_format(&capture, format)?;
            self.capture = Some(capture);
            Ok(())
        }

        fn default_output_uid(&mut self) -> Result<String, i32> {
            // SAFETY: the selector is documented as an AudioObjectID.
            let device: AudioObjectID = unsafe {
                get_property(
                    kAudioObjectSystemObject as AudioObjectID,
                    kAudioHardwarePropertyDefaultOutputDevice,
                )?
            };
            if device == 0 {
                return Err(-1);
            }
            get_string_property(device, kAudioDevicePropertyDeviceUID)
        }

        fn create_aggregate(&mut self, description: &[(String, DictValue)]) -> Result<u32, i32> {
            debug_assert_eq!(
                description.len(),
                7,
                "the composition dictionary is {} keys",
                keys::TAP_AUTO_START
            );
            let dictionary = to_ns_dictionary(description);
            let mut device: AudioObjectID = 0;
            // SAFETY: NSDictionary is toll-free bridged with CFDictionary, the
            // dictionary outlives the call, and `device` is a valid
            // out-parameter.
            let status = unsafe {
                (self.symbols.create_aggregate)(
                    Retained::as_ptr(&dictionary).cast::<c_void>(),
                    &mut device,
                )
            };
            if status == 0 && device != 0 {
                Ok(device)
            } else {
                Err(if status == 0 { -1 } else { status })
            }
        }

        fn create_ioproc(&mut self, aggregate: u32, format: TapFormat) -> Result<IoProcToken, i32> {
            // A second IOProc must not silently replace a first. `open_tap`
            // calls this once, but YV104's 7-step rebuild — in this same file,
            // above — is create-after-teardown by construction and a future
            // caller that skips a step would otherwise leave the old IOProc
            // registered with `coreaudiod` and its block alive on the RT thread,
            // with nothing holding a handle to destroy it. Overwriting
            // `self.live` was that leak; destroying first is the fix.
            self.destroy_live_ioproc();
            // `open_tap` always runs `bind_capture` first, so this is `Some` on
            // every path that exists. It is still an error rather than an
            // `unwrap`: this block is what stamps the anchors, and the one thing
            // it must never do is stamp them through a ring nobody checked.
            let Some(capture) = self.capture.as_ref().map(Arc::clone) else {
                return Err(CAPTURE_NOT_BOUND);
            };
            debug_assert!(
                super::capture_matches_format(&capture, format).is_ok(),
                "the IOProc trims by the tap's channel count and the ring stamps \
                 by its own — bind_capture is what makes those the same number"
            );
            let channels = format.channels.max(1) as usize;
            // A dedicated serial queue, NOT `None`. Passing a nil queue makes
            // CoreAudio invoke the block directly, which is one of the silence
            // causes OS-4 enumerates — and a queue is retained by CoreAudio
            // until `AudioDeviceDestroyIOProcID`, so it is owned here for
            // exactly as long as the IOProc is.
            let queue = DispatchQueue::new("consulting.drivia.yap.tap", DispatchQueueAttr::SERIAL);
            // Built HERE, off the IO thread, so the first callback finds the
            // mach timebase already cached — and owned by the block, so it is
            // per-stream: a rebuilt tap rebases to zero the way a rebuilt cpal
            // stream does.
            let clock = TapClock::new();
            let block = RcBlock::new(
                move |in_now: NonNull<AudioTimeStamp>,
                      in_input_data: NonNull<AudioBufferList>,
                      _in_input_time: NonNull<AudioTimeStamp>,
                      _out_output_data: NonNull<AudioBufferList>,
                      _in_output_time: NonNull<AudioTimeStamp>| {
                    // EVERYTHING is inside the guard: this is an
                    // `extern "C-unwind"` boundary onto a real-time thread, and
                    // the buffer-list decode below is exactly where an
                    // off-by-one becomes a panic (OS-6, correction #2).
                    super::tap_ioproc_guarded(&capture, || {
                        // SAFETY: CoreAudio guarantees both pointers are valid
                        // for the duration of the block, and `mNumberBuffers`
                        // describes the trailing array of `AudioBuffer`.
                        unsafe {
                            // Rebased to THIS stream's first callback, which is
                            // the domain `CaptureAnchor::host_ns` is defined in
                            // (see `TapClock`). Absolute mach time here would
                            // put Track 1 an uptime away from Track 0 in
                            // YV106's merge.
                            let host_ns = clock.stamp(in_now.as_ref().mHostTime);
                            let list = in_input_data.as_ref();
                            if list.mNumberBuffers == 0 {
                                return;
                            }
                            // A mono global tap delivers one interleaved buffer;
                            // only the first is read, and a de-interleaved
                            // multi-buffer layout would be a format this item
                            // does not claim to handle.
                            let buffer = &*list.mBuffers.as_ptr();
                            let samples =
                                buffer.mDataByteSize as usize / std::mem::size_of::<f32>();
                            if buffer.mData.is_null() || samples == 0 {
                                return;
                            }
                            let frames = std::slice::from_raw_parts(
                                buffer.mData.cast::<f32>(),
                                samples - (samples % channels),
                            );
                            // Verbatim: the same body the mic path runs, from
                            // the same module (`rtring.rs`'s own doc comment
                            // asks for exactly this).
                            rt_capture_callback(&capture, frames, |s| s, host_ns);
                        }
                    });
                },
            );

            let mut id: objc2_core_audio::AudioDeviceIOProcID = None;
            // SAFETY: the block and the queue are kept alive in `self.live`
            // below for as long as CoreAudio holds the IOProc.
            let status = unsafe {
                AudioDeviceCreateIOProcIDWithBlock(
                    NonNull::from(&mut id),
                    aggregate,
                    Some(&queue),
                    RcBlock::as_ptr(&block),
                )
            };
            if status != 0 || id.is_none() {
                return Err(if status == 0 { -1 } else { status });
            }
            self.live = Some(LiveIoProc {
                id,
                aggregate,
                _block: block,
                _queue: queue,
            });
            Ok(1)
        }

        fn start(&mut self, aggregate: u32, _ioproc: IoProcToken) -> Result<(), i32> {
            let Some(live) = self.live.as_ref() else {
                return Err(-1);
            };
            // SAFETY: the IOProc id came from this device.
            let status = unsafe { AudioDeviceStart(aggregate, live.id) };
            if status == 0 {
                Ok(())
            } else {
                Err(status)
            }
        }

        fn stop(&mut self, aggregate: u32, _ioproc: IoProcToken) {
            if let Some(live) = self.live.as_ref() {
                // SAFETY: as above; a non-zero status is logged by the caller's
                // teardown, never a reason to skip the remaining steps.
                unsafe { AudioDeviceStop(aggregate, live.id) };
            }
        }

        fn destroy_ioproc(&mut self, _aggregate: u32, _ioproc: IoProcToken) {
            // The device id comes from `LiveIoProc` rather than from the
            // argument: they are always the same device, and taking it from the
            // handle is what lets `Drop` and the rebuild path do this too.
            self.destroy_live_ioproc();
        }

        fn destroy_aggregate(&mut self, aggregate: u32) {
            // SAFETY: `aggregate` came from AudioHardwareCreateAggregateDevice,
            // and the destroy symbol was resolved alongside the create one — an
            // aggregate device can never exist here without its destructor.
            unsafe { (self.symbols.destroy_aggregate)(aggregate) };
        }

        fn destroy_tap(&mut self, tap: u32) {
            // SAFETY: `tap` came from AudioHardwareCreateProcessTap; same
            // all-or-nothing resolution argument as above.
            unsafe { (self.symbols.destroy_process_tap)(tap) };
        }
    }

    /// A started system tap, torn down on drop.
    ///
    /// Drop is the backstop, not the plan: [`SystemTap::stop`] is the ordinary
    /// path. But a panic anywhere in the meeting stack must not leave a private
    /// aggregate device holding the user's output device, so the teardown runs
    /// from `Drop` as well — the same four calls, the same order.
    pub struct SystemTap {
        platform: CoreAudioPlatform,
        resources: TapResources,
        format: TapFormat,
        capture: Arc<RtCapture>,
    }

    impl SystemTap {
        pub fn format(&self) -> TapFormat {
            self.format
        }

        pub fn capture(&self) -> &Arc<RtCapture> {
            &self.capture
        }

        pub fn stop(mut self) {
            teardown(&mut self.platform, &mut self.resources);
        }
    }

    impl Drop for SystemTap {
        fn drop(&mut self) {
            if !self.resources.is_empty() {
                teardown(&mut self.platform, &mut self.resources);
            }
        }
    }

    /// Create, wire and start the tap — and **return** the capture ring it is
    /// stamping into.
    ///
    /// The ring is an output, not an input, and that is the correction this
    /// function exists in its current shape for. It used to take an
    /// `Arc<RtCapture>`, which reads naturally and is impossible to call
    /// correctly: `kAudioTapPropertyFormat` — the tap's real rate and channel
    /// count — cannot be read until `AudioHardwareCreateProcessTap` has already
    /// succeeded, so every caller had to guess the format before the tap that
    /// determines it existed. A wrong guess is not an error, it is a silently
    /// wrong sample axis on every anchor (see [`capture_matches_format`]). So
    /// the tap is opened, the format is read, the ring is built from it, and the
    /// caller gets both back together via [`SystemTap::capture`] and
    /// [`SystemTap::format`] — which is also exactly the pair
    /// [`virtual_meeting_config`] needs.
    ///
    /// **Caller contract:** gate this on macOS 14.4 first (YV101). Nothing calls
    /// it yet for that reason. The linkage floor is enforced here regardless —
    /// on an OS without the API this returns
    /// [`TapError::ProcessTapApiUnavailable`] rather than trusting a caller to
    /// have checked.
    pub fn start_system_tap() -> Result<SystemTap, TapError> {
        let mut platform = CoreAudioPlatform::new()?;
        let uid = format!("consulting.drivia.yap.tap.{}", uuid::Uuid::new_v4());
        let open: OpenTap = super::open_tap(
            &mut platform,
            self_process_object(),
            &uid,
            "Yap meeting capture",
        )?;
        // `open_tap` returning `Ok` means `bind_capture` ran, so this is `Some`.
        // Checked rather than unwrapped for the same reason `create_ioproc`
        // checks: an unbound ring here would be a tap delivering into nothing.
        let capture =
            platform
                .capture()
                .map(Arc::clone)
                .ok_or(TapError::CaptureFormatMismatch {
                    tap_sample_rate: open.format.sample_rate,
                    tap_channels: open.format.channels,
                    capture_sample_rate: 0,
                    capture_channels: 0,
                })?;
        super::capture_matches_format(&capture, open.format)?;
        Ok(SystemTap {
            platform,
            resources: open.resources,
            format: open.format,
            capture,
        })
    }

    /// YV102 — provoke the TCC alert, on purpose, from Settings.
    ///
    /// The whole function is 200 ms of real process tap whose audio is thrown
    /// away. It exists because there is no `requestAccess` for this permission:
    /// creating and starting a tap IS the request, so the only choice available
    /// is *when* the alert fires, and the answer is "while the user is reading a
    /// sentence about it", not "as they join a call".
    ///
    /// The caller must have passed [`crate::os_version_gate::system_audio_gate`]
    /// first. This checks anyway — [`CoreAudioPlatform::new`] refuses on an OS
    /// with no process-tap API — because a gate a caller can forget is not a
    /// gate.
    pub fn prewarm_system_audio_permission() -> PrewarmReport {
        // 48 kHz mono is a throwaway: the tap's real format is read from the
        // tap itself during setup, and nothing here keeps a frame.
        let capture = Arc::new(RtCapture::new(48_000, 1));
        let mut platform = match CoreAudioPlatform::new(Arc::clone(&capture)) {
            Ok(platform) => platform,
            Err(error) => {
                return PrewarmReport {
                    opened: false,
                    error: Some(error),
                    verdict: SystemAudioPermission::Unknown,
                }
            }
        };
        let uid = format!("consulting.drivia.yap.prewarm.{}", uuid::Uuid::new_v4());
        let run = super::prewarm_tap(
            &mut platform,
            self_process_object(),
            &uid,
            "Yap permission check",
            || std::thread::sleep(PREWARM_DWELL),
        );

        // Fold whatever the 200 ms produced through the SAME discriminator a
        // real meeting uses. It will nearly always say `Unknown`, and that is
        // the honest answer: 200 ms of silence means nothing was playing.
        let mut delivery = TapDelivery::default();
        let mut anchors: Vec<CaptureAnchor> = Vec::new();
        let mut samples: Vec<f32> = Vec::new();
        capture.anchors.drain_into(&mut anchors);
        capture.samples.drain_into(&mut samples);
        let channels = usize::from(capture.channels()).max(1);
        let mut offset = 0usize;
        for anchor in &anchors {
            let start = offset.min(samples.len());
            let end = (start + anchor.frames as usize * channels).min(samples.len());
            delivery.observe(anchor, &samples[start..end]);
            offset = end;
        }

        PrewarmReport {
            opened: run.opened,
            error: run.error,
            verdict: permission_verdict(&delivery, PREWARM_DWELL),
        }
    }

    /// What [`prewarm_system_audio_permission`] found out.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct PrewarmReport {
        /// The tap was created and started — which means the alert either fired
        /// or had already been answered. Both are "the step did its job".
        pub opened: bool,
        pub error: Option<TapError>,
        /// Almost always [`SystemAudioPermission::Unknown`]: 200 ms is far below
        /// [`DENIAL_GRACE`] on purpose, so this step can never manufacture a
        /// denial verdict out of a quiet Mac.
        pub verdict: SystemAudioPermission,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── YV100: the tap's own pure seams ─────────────────────────────────

    #[test]
    fn an_unresolvable_self_process_object_refuses_rather_than_taps_everything() {
        assert_eq!(
            exclusion_list(None),
            Err(TapError::SelfProcessObjectUnavailable)
        );
        // Object 0 is "no object" in CoreAudio, so it is the same refusal by a
        // second route rather than an exclusion of nothing.
        assert_eq!(
            exclusion_list(Some(0)),
            Err(TapError::SelfProcessObjectUnavailable)
        );
        assert_eq!(exclusion_list(Some(77)), Ok([77]));
    }

    #[test]
    fn the_declared_key_names_are_the_ones_coreaudio_uses() {
        #[cfg(target_os = "macos")]
        assert_eq!(
            declared_aggregate_key_names(),
            coreaudio_aggregate_key_names()
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(declared_aggregate_key_names().len(), 9);
    }

    #[test]
    fn a_status_renders_as_its_four_char_code_when_it_is_one() {
        // 'who?' — kAudioHardwareBadObjectError, the status a destroyed tap
        // returns, and the one a log reader will actually search for.
        assert_eq!(fourcc(i32::from_be_bytes(*b"who?")), "'who?'");
        assert_eq!(fourcc(-1), "0xffffffff");
    }

    // ── YV104: the ghost watchdog ───────────────────────────────────────

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
    fn nothing_playing_explains_the_zeros_so_the_watchdog_does_not_act_on_them() {
        let env = TapEnvironment {
            system_output_active: Some(false),
            ..TapEnvironment::default()
        };
        assert!(silence_is_explained(&ghost_tap(), &env));
        assert!(!is_unexplained_silence(&ghost_tap(), &env));
        let action = ghost_tick(&TapWatchdogInputs {
            elapsed: Duration::from_secs(600),
            liveness: ghost_tap(),
            env,
            state: GhostState::default(),
        });
        assert_eq!(action, TapWatchdogAction::Continue);
    }

    #[test]
    fn a_quiet_room_does_not_explain_an_ioproc_that_stopped_firing() {
        let dead = TapLiveness {
            since_last_block: Duration::from_secs(120),
            ..ghost_tap()
        };
        let env = TapEnvironment {
            system_output_active: Some(false),
            ..TapEnvironment::default()
        };
        assert!(!silence_is_explained(&dead, &env));
        let action = ghost_tick(&TapWatchdogInputs {
            elapsed: Duration::from_secs(600),
            liveness: dead,
            env,
            state: GhostState::default(),
        });
        assert!(action.is_rebuild(), "got {action:?}");
    }

    #[test]
    fn observing_the_same_tick_twice_says_the_same_thing() {
        let env = TapEnvironment {
            system_output_active: Some(true),
            ..TapEnvironment::default()
        };
        let once = observe(GhostState::default(), &ghost_tap(), &env);
        assert!(once.output_active_observed);
        assert_eq!(observe(once, &ghost_tap(), &env), once);
        // …and a tap that starts delivering again drops the evidence with the
        // silence it belonged to.
        let recovered = observe(once, &healthy_tap(), &env);
        assert!(!recovered.output_active_observed);
    }

    #[test]
    fn a_delivering_tap_closes_an_in_flight_rebuild_instead_of_being_rebuilt() {
        let state = GhostState {
            rebuilds_issued: 1,
            rebuild_issued_at: Some(Duration::from_secs(60)),
            ..GhostState::default()
        };
        // Two whole in-flight timeouts later, with audio arriving every tick.
        let action = ghost_tick(&TapWatchdogInputs {
            elapsed: Duration::from_secs(60) + TAP_REBUILD_IN_FLIGHT_TIMEOUT * 2,
            liveness: healthy_tap(),
            env: TapEnvironment::default(),
            state,
        });
        assert_eq!(
            action,
            TapWatchdogAction::RebuildSettled {
                outcome: TapRebuildOutcome::Succeeded
            }
        );
    }

    #[test]
    fn the_in_flight_wait_is_reachable_at_the_interval_the_product_ticks_at() {
        assert!(
            TAP_REBUILD_IN_FLIGHT_TIMEOUT >= crate::meeting::WATCHDOG_INTERVAL * 2,
            "a timeout shorter than two ticks makes the wait branch dead code"
        );
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
        // …and the bit alone is not enough to say "denied". It says denied only
        // when something was observed playing into the silence.
        assert_eq!(live.verdict(true), TapVerdict::PermissionLikelyDenied);
        assert_eq!(live.verdict(false), TapVerdict::NoSystemAudioObserved);
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
        // Ghost either way: one real sample rules permission out whatever the
        // output probe says.
        assert_eq!(live.verdict(true), TapVerdict::GhostTapUnrecovered);
        assert_eq!(live.verdict(false), TapVerdict::GhostTapUnrecovered);
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
