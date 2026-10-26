//! macOS push-to-talk via FN+Control (default) and optional bare FN.
//!
//! Hybrid gesture (VoiceInk-style, clean-room):
//!   • Hold fn⌃  → push-to-talk (start on hold, stop on release)
//!   • Double-tap fn⌃ → hands-free latch (keeps recording)
//!   • Single tap fn⌃ while latched → end hands-free
//!
//! CGEvent tap cannot bind bare FN via Carbon. Accessibility required.
//! System Settings → Keyboard → “Press 🌐 key to” → Do Nothing.

#![cfg(target_os = "macos")]

use parking_lot::Mutex;

use crate::hotkeys::{set_fn_tap_status, FnTapStatus};

/// The human reason a failed `CGEventTapCreate` carries into the degraded line.
pub const TAP_BLOCKED_REASON: &str =
    "macOS refused the key listener (Input Monitoring is off for Yap)";
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const KCG_EVENT_FLAG_MASK_SECONDARY_FN: u64 = 0x0080_0000;
const KCG_EVENT_FLAG_MASK_CONTROL: u64 = 0x0004_0000;
const KCG_EVENT_FLAG_MASK_COMMAND: u64 = 0x0010_0000;
const KCG_EVENT_FLAG_MASK_ALTERNATE: u64 = 0x0008_0000;

const KCG_EVENT_FLAGS_CHANGED: u32 = 12;
const KCG_EVENT_KEY_DOWN: u32 = 10;
const KCG_HID_EVENT_TAP: u32 = 0;
const KCG_HEAD_INSERT: u32 = 0;
const KCG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;
const KCG_EVENT_SOURCE_STATE_HID: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttBinding {
    Fn,
    FnControl,
    FnOrFnControl,
}

impl PttBinding {
    pub fn from_settings(s: &str) -> Self {
        match s {
            "fn" | "globe" => Self::Fn,
            "fn_or_fn_control" | "both" => Self::FnOrFnControl,
            _ => Self::FnControl, // default: fn⌃
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fn => "fn hold",
            Self::FnControl => "fn⌃",
            Self::FnOrFnControl => "fn / fn⌃",
        }
    }
}

/// The EXTRA modifier that turns a normal push-to-talk press into a command-mode
/// press (YV49) — one that edits the current selection instead of typing.
///
/// This rides ON TOP of whichever [`PttBinding`] is configured: the default
/// `Command` variant means fn⌃⌘ when the PTT binding is fn⌃ and fn⌘ when it is
/// bare fn. The plain binding keeps working untouched — a press without the
/// extra modifier is still ordinary dictation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandBinding {
    /// Default: hold the PTT combo with ⌘.
    Command,
    /// Alternate: hold the PTT combo with ⌥ (for users who already bind ⌘ + fn).
    Option,
    /// Command mode off — every press dictates.
    Off,
}

impl CommandBinding {
    pub fn from_settings(s: &str) -> Self {
        match s {
            "option" | "alt" => Self::Option,
            "off" | "none" => Self::Off,
            _ => Self::Command, // default: + ⌘
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Command => "+⌘",
            Self::Option => "+⌥",
            Self::Off => "off",
        }
    }
}

// ── yap-qa-hotkey-walk-binding DO 3: fn ALONE says why it did nothing ─────────
//
// The default binding is fn⌃ and the UI says "Hold fn⌃". A user who holds fn by
// itself got nothing and no reason. Now a bare fn press — down and up, or held
// past `FN_HINT_HOLD_MS` — under a binding that needs ⌃ emits ONE
// `PttEvent::FnWithoutModifierHint`, throttled to once per `FN_HINT_THROTTLE_MS`,
// and the pill shows `FN_WITHOUT_MODIFIER_HINT`.

/// The pill line for fn pressed without the binding's ⌃. Mirrors
/// `FN_HINT_LINE` in desktop/src/pill/live.ts (live.test.ts holds them equal and
/// inside the side-dock strip's one line).
pub const FN_WITHOUT_MODIFIER_HINT: &str = "Hold fn⌃ — fn alone does nothing";
/// A bare fn held longer than this (ms) earns the hint before it is released.
pub const FN_HINT_HOLD_MS: u64 = 400;
/// At most one hint per this many ms.
pub const FN_HINT_THROTTLE_MS: u64 = 30_000;

/// PURE: is a "fn alone does nothing" hint due right now?
///
/// * `binding` — only a binding that NEEDS ⌃ (`FnControl`) can be missing it;
///   `Fn` and `FnOrFnControl` start a take on bare fn, so they never hint.
/// * `fn_down` — fn is still held (`true`) or this is its release (`false`).
/// * `ctrl_down` — ⌃ was down at any point in this fn press.
/// * `held_ms` — how long fn has been (or was) down.
/// * `last_hint_ms` / `now_ms` — the monotonic clock of the last hint (None:
///   never) and now, for the 30 s throttle.
pub fn fn_hint_due(
    binding: PttBinding,
    fn_down: bool,
    ctrl_down: bool,
    held_ms: u64,
    last_hint_ms: Option<u64>,
    now_ms: u64,
) -> bool {
    if binding != PttBinding::FnControl || ctrl_down {
        return false;
    }
    if fn_down && held_ms <= FN_HINT_HOLD_MS {
        return false;
    }
    match last_hint_ms {
        Some(last) => now_ms.saturating_sub(last) >= FN_HINT_THROTTLE_MS,
        None => true,
    }
}

/// One fn press, as the hint sees it. Pure: no FFI, time in ms.
#[derive(Debug, Default)]
struct FnHintTracker {
    /// When the current fn press went down, if one is in progress.
    press_at: Option<u64>,
    /// ⌃ was seen at some point during this press.
    ctrl_seen: bool,
    /// Another key or modifier joined this press (fn+arrow, fn+F-key, fn⌘…):
    /// that is a different shortcut, not a missed push-to-talk.
    dirty: bool,
    /// Bumped per press so a stale hold check never speaks for a later press.
    gen: u64,
    last_hint: Option<u64>,
}

impl FnHintTracker {
    /// A flags edge. Returns `(hint_now, arm_hold_check_gen)`.
    fn on_flags(
        &mut self,
        binding: PttBinding,
        fn_down: bool,
        ctrl: bool,
        other_mod: bool,
        now: u64,
    ) -> (bool, Option<u64>) {
        match (self.press_at, fn_down) {
            (None, true) => {
                self.press_at = Some(now);
                self.ctrl_seen = ctrl;
                self.dirty = other_mod;
                self.gen += 1;
                let arm = binding == PttBinding::FnControl && !ctrl && !other_mod;
                (false, arm.then_some(self.gen))
            }
            (Some(_), true) => {
                self.ctrl_seen |= ctrl;
                self.dirty |= other_mod;
                (false, None)
            }
            (Some(at), false) => {
                self.press_at = None;
                let due = !self.dirty
                    && fn_hint_due(
                        binding,
                        false,
                        self.ctrl_seen || ctrl,
                        now.saturating_sub(at),
                        self.last_hint,
                        now,
                    );
                if due {
                    self.last_hint = Some(now);
                }
                (due, None)
            }
            (None, false) => (false, None),
        }
    }

    /// A non-modifier key went down: whatever this fn press is, it is not PTT.
    fn on_key_down(&mut self) {
        if self.press_at.is_some() {
            self.dirty = true;
        }
    }

    /// The `FN_HINT_HOLD_MS` check for press `gen`, fn still held.
    fn on_hold_check(&mut self, gen: u64, binding: PttBinding, ctrl: bool, now: u64) -> bool {
        let Some(at) = self.press_at else {
            return false;
        };
        if gen != self.gen || self.dirty {
            return false;
        }
        self.ctrl_seen |= ctrl;
        let due = fn_hint_due(
            binding,
            true,
            self.ctrl_seen,
            now.saturating_sub(at),
            self.last_hint,
            now,
        );
        if due {
            self.last_hint = Some(now);
        }
        due
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttEvent {
    /// Start recording (hold armed or hands-free latch on).
    Start,
    /// Stop recording (hold release or hands-free end).
    Stop,
    /// Discard in-flight take.
    Interrupted,
    /// Hands-free latched on (for UI message).
    HandsFreeOn,
    /// Hands-free latched off.
    HandsFreeOff,
    /// yap-hotkey-stop: the take is still running and the pill must say so —
    /// a hold with no key-up for `STILL_RECORDING_AFTER_SECS`, or any take
    /// during which macOS switched the tap off.
    StillRecording,
    /// yap-qa-hotkey-walk-binding: fn went down (and up, or past
    /// `FN_HINT_HOLD_MS`) without the binding's ⌃ — the pill shows
    /// `FN_WITHOUT_MODIFIER_HINT`. Throttled to one per `FN_HINT_THROTTLE_MS`.
    FnWithoutModifierHint,
}

type Callback = Arc<dyn Fn(PttEvent) + Send + Sync + 'static>;

// ── yap-hotkey-stop: ONE edge state machine, TWO edge sources ────────────────
//
// Wilson 2026-10-16: fifteen fn presses and the take would not stop. START and
// STOP used to share exactly one mechanism — the listen-only HID CGEventTap —
// and three things could leave a live take with no way to end it from the key:
//   1. macOS disabled the tap (7× by user input in one session) and the
//      fire-and-forget re-arm never checked it had worked;
//   2. an up-edge was missed (debounced, or delivered while the tap was off),
//      `combo_down` stayed true, and `want == was` swallowed every later press;
//   3. nothing ever said "this take is still running".
//
// So the state machine below is PURE — plain fields, time as milliseconds on a
// monotonic clock, no FFI, no threads — and it is fed by two sources:
//   • the tap (`EdgeSource::Tap`), as before, and
//   • while a take is live, a 30 ms poll of the HID system flags state
//     (`EdgeSource::Poll`), which is process-independent: it reads the keyboard,
//     not whichever app is frontmost.
// Both call the SAME `edge` function. An edge is applied only when it is a
// TRANSITION of `combo_down`, and every applied edge stamps its time and source
// (`last_edge`). The second report of the same edge — the poll seeing the
// release the tap already delivered, or the tap's late event after the poll got
// there first — is therefore not a transition and produces nothing, so a take
// is never stopped twice.

/// Hold longer than this → confirmed push-to-talk (not a tap).
pub const HOLD_ARM_MS: u64 = 280;
/// Two short releases within this window → double-tap (hands-free).
pub const DOUBLE_TAP_MS: u64 = 450;
/// Press shorter than this counts as a tap (not a hold).
pub const TAP_MAX_MS: u64 = 320;
/// Chatter filter: an edge this soon after the previous one is ignored (the
/// poll reconciles it on its next tick if it was real).
pub const DEBOUNCE_MS: u64 = 25;
/// The second stop mechanism's period, while a take is live.
pub const POLL_INTERVAL_MS: u64 = 30;
/// A hold take with no key-up for this long, or any take during which the tap
/// was disabled, puts "Still recording" on the pill.
pub const STILL_RECORDING_AFTER_SECS: u64 = 90;
/// The pill line for that state. Mirrored by `STILL_RECORDING_LINE` in
/// `src/pill/live.ts` (tests/hotkey_stop.rs holds the two together).
pub const STILL_RECORDING_MESSAGE: &str = "Still recording — tap fn to stop";
/// Where the tap sits: kCGHIDEventTap, below every app, so the stop path never
/// depends on Yap (or any particular app) being frontmost.
pub const TAP_LOCATION: u32 = KCG_HID_EVENT_TAP;
/// What the poll reads: kCGEventSourceStateHIDSystemState — the keyboard's own
/// state, not any process's view of it.
pub const POLL_SOURCE_STATE: i32 = KCG_EVENT_SOURCE_STATE_HID;

/// Which mechanism reported an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeSource {
    /// The CGEventTap callback.
    Tap,
    /// The live-take poll of `CGEventSourceFlagsState(HID)`, or the hold-arm
    /// timer's reconcile read.
    Poll,
}

/// One applied edge: direction, when, and who reported it — the dedup key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppliedEdge {
    pub down: bool,
    pub at_ms: u64,
    pub source: EdgeSource,
}

/// What one input to the machine asks the outside world to do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Step {
    /// Events for the app callback, in order.
    pub events: Vec<PttEvent>,
    /// Arm a hold timer for this press generation (`HOLD_ARM_MS` from now).
    pub arm_hold: Option<u64>,
}

impl Step {
    fn none() -> Self {
        Self::default()
    }
    fn of(events: &[PttEvent]) -> Self {
        Self {
            events: events.to_vec(),
            arm_hold: None,
        }
    }
}

/// The push-to-talk edge state machine, as a pure value.
#[derive(Debug, Clone, Default)]
pub struct PttMachine {
    /// Combo currently held, as far as the machine knows.
    combo_down: bool,
    /// The press in flight was cut short by an ordinary key-down.
    interrupted: bool,
    /// Hands-free latch active (recording without hold).
    hands_free: bool,
    /// Recording started because hold exceeded HOLD_ARM_MS.
    hold_armed: bool,
    /// YV49: the press in flight carried the command-mode modifier.
    command_mode: bool,
    /// When the current press began (ms).
    press_at: Option<u64>,
    /// Last short-release time (for double-tap).
    last_tap_at: Option<u64>,
    /// Generation counter so hold-arm timers can cancel.
    press_gen: u64,
    /// After a press was consumed (it ended hands-free, or was interrupted),
    /// ignore it until it is released.
    suppress_until_release: bool,
    /// The last applied edge — the dedup record.
    last_edge: Option<AppliedEdge>,
    /// When the live take started (ms); `None` with no take.
    take_started_at: Option<u64>,
    /// "Still recording" has been raised for this take.
    still_prompted: bool,
    /// The tap was disabled at some point during this take.
    tap_disabled_in_take: bool,
    /// The HID flags read has, at least once, reported fn DOWN — proof that it
    /// can see fn at all on this machine's grants. Until then the poll is
    /// ignored: a read that is blind (all zero) would otherwise look like an
    /// instant key-up and stop every hold take the moment it started.
    hid_proven: bool,
}

impl PttMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// A take is live from Start until Stop / Interrupted / an external stop.
    pub fn take_live(&self) -> bool {
        self.hold_armed || self.hands_free
    }

    pub fn combo_down(&self) -> bool {
        self.combo_down
    }

    pub fn hands_free(&self) -> bool {
        self.hands_free
    }

    pub fn command_mode(&self) -> bool {
        self.command_mode
    }

    pub fn press_at_ms(&self) -> Option<u64> {
        self.press_at
    }

    pub fn last_edge(&self) -> Option<AppliedEdge> {
        self.last_edge
    }

    pub fn hid_proven(&self) -> bool {
        self.hid_proven
    }

    /// The pill should be saying "Still recording".
    pub fn still_recording(&self) -> bool {
        self.still_prompted && self.take_live()
    }

    /// Should the second mechanism be running? While a take is live — and,
    /// once the HID read is proven, until the key that ended it is released, so
    /// a stuck `combo_down` is reconciled before the next press needs it.
    pub fn poll_wanted(&self) -> bool {
        self.take_live() || (self.combo_down && self.hid_proven)
    }

    fn end_take(&mut self) {
        self.take_started_at = None;
        self.still_prompted = false;
        self.tap_disabled_in_take = false;
    }

    /// THE edge function. Both sources call it; only a transition does anything.
    pub fn edge(
        &mut self,
        want: bool,
        command_mode: bool,
        now_ms: u64,
        source: EdgeSource,
    ) -> Step {
        if want == self.combo_down {
            // Not a transition: the other source already applied this edge (or
            // nothing changed). This is the dedup — a take is never stopped twice.
            return Step::none();
        }
        if let Some(last) = self.last_edge {
            if now_ms.saturating_sub(last.at_ms) < DEBOUNCE_MS {
                return Step::none();
            }
        }
        self.last_edge = Some(AppliedEdge {
            down: want,
            at_ms: now_ms,
            source,
        });
        if want {
            self.on_down(command_mode, now_ms)
        } else {
            self.on_up(now_ms)
        }
    }

    /// One poll sample. `raw_fn_down` is the fn bit itself, used only to prove
    /// the read can see fn; `want` is the binding's combo for that sample.
    pub fn poll(&mut self, want: bool, raw_fn_down: bool, command_mode: bool, now_ms: u64) -> Step {
        if raw_fn_down {
            self.hid_proven = true;
        }
        if !self.hid_proven {
            return Step::none();
        }
        self.edge(want, command_mode, now_ms, EdgeSource::Poll)
    }

    fn on_down(&mut self, command_mode: bool, now_ms: u64) -> Step {
        self.combo_down = true;
        self.interrupted = false;
        self.command_mode = command_mode;

        // A tap while hands-free ends the latch; this press is consumed. (A
        // hold take showing "Still recording" ends on its release: if the
        // machine missed the first release it still holds the key down, so the
        // user's fn tap is a no-op down followed by the up that stops it.)
        if self.hands_free {
            self.hands_free = false;
            self.hold_armed = false;
            self.suppress_until_release = true;
            self.press_at = None;
            self.last_tap_at = None;
            self.press_gen += 1;
            self.end_take();
            return Step::of(&[PttEvent::HandsFreeOff, PttEvent::Stop]);
        }

        if self.suppress_until_release {
            return Step::none();
        }

        self.press_gen += 1;
        self.press_at = Some(now_ms);
        self.hold_armed = false;
        // YV38: this wait is NOT latency slack — it IS the gesture, the window
        // that separates a tap (hands-free toggle) from a hold (push-to-talk).
        Step {
            events: Vec::new(),
            arm_hold: Some(self.press_gen),
        }
    }

    fn on_up(&mut self, now_ms: u64) -> Step {
        self.combo_down = false;
        self.press_gen += 1; // cancel pending hold arm

        if self.suppress_until_release {
            self.suppress_until_release = false;
            self.press_at = None;
            return Step::none();
        }

        let Some(started) = self.press_at.take() else {
            return Step::none();
        };
        let duration = now_ms.saturating_sub(started);

        // Hold mode → stop on release.
        if self.hold_armed {
            self.hold_armed = false;
            self.end_take();
            return Step::of(&[PttEvent::Stop]);
        }

        if self.hands_free {
            return Step::none();
        }

        // Short press = tap.
        if duration <= TAP_MAX_MS {
            if let Some(t) = self.last_tap_at {
                if now_ms.saturating_sub(t) <= DOUBLE_TAP_MS {
                    self.last_tap_at = None;
                    self.hands_free = true;
                    self.take_started_at = Some(now_ms);
                    self.still_prompted = false;
                    self.tap_disabled_in_take = false;
                    return Step::of(&[PttEvent::HandsFreeOn, PttEvent::Start]);
                }
            }
            self.last_tap_at = Some(now_ms);
        }
        Step::none()
    }

    /// The hold-arm timer for press generation `gen` fired.
    pub fn hold_timer(&mut self, gen: u64, now_ms: u64) -> Step {
        if gen != self.press_gen
            || !self.combo_down
            || self.hands_free
            || self.suppress_until_release
        {
            return Step::none();
        }
        self.hold_armed = true;
        self.last_tap_at = None;
        self.take_started_at = Some(now_ms);
        self.still_prompted = false;
        self.tap_disabled_in_take = false;
        Step::of(&[PttEvent::Start])
    }

    /// An ordinary (non-modifier) key went down. A press that has not latched
    /// hands-free is cut short; the held key is then ignored until released, so
    /// neither source can turn the still-held fn into a fresh press.
    pub fn key_down(&mut self, is_modifier: bool) -> Step {
        if !self.combo_down || self.hands_free || is_modifier || self.interrupted {
            return Step::none();
        }
        self.interrupted = true;
        self.hold_armed = false;
        self.suppress_until_release = true;
        self.press_at = None;
        self.press_gen += 1;
        self.end_take();
        Step::of(&[PttEvent::Interrupted])
    }

    /// The live-take clock, called every poll tick.
    pub fn tick(&mut self, now_ms: u64) -> Step {
        if self.still_prompted || !self.hold_armed || !self.combo_down {
            return Step::none();
        }
        let Some(start) = self.take_started_at else {
            return Step::none();
        };
        if now_ms.saturating_sub(start) >= STILL_RECORDING_AFTER_SECS * 1000 {
            self.still_prompted = true;
            return Step::of(&[PttEvent::StillRecording]);
        }
        Step::none()
    }

    /// macOS switched the tap off. Any take live right now says so on the pill.
    pub fn tap_disabled(&mut self) -> Step {
        if !self.take_live() {
            return Step::none();
        }
        self.tap_disabled_in_take = true;
        if self.still_prompted {
            return Step::none();
        }
        self.still_prompted = true;
        Step::of(&[PttEvent::StillRecording])
    }

    /// The take was ended from OUTSIDE the key (pill, Home, tray). See
    /// [`end_hands_free`].
    pub fn external_stop(&mut self) {
        self.hands_free = false;
        self.hold_armed = false;
        self.command_mode = false;
        self.press_gen += 1;
        self.press_at = None;
        self.last_tap_at = None;
        self.end_take();
        if self.combo_down {
            self.suppress_until_release = true;
        }
    }
}

struct TapState {
    binding: Mutex<PttBinding>,
    /// Which extra modifier makes a press a command-mode press (YV49).
    command_binding: Mutex<CommandBinding>,
    machine: Mutex<PttMachine>,
    /// The bare-fn hint's own view of the current fn press.
    fn_hint: Mutex<FnHintTracker>,
    callback: Callback,
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static GLOBAL_STATE: Mutex<Option<Arc<TapState>>> = Mutex::new(None);
/// The live-take poll thread is running.
static POLLING: AtomicBool = AtomicBool::new(false);
/// The user_info pointer the tap was created with, kept so a recreated tap
/// shares the same state.
static TAP_USER_INFO: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

static CLOCK_ORIGIN: OnceLock<Instant> = OnceLock::new();

fn origin() -> Instant {
    *CLOCK_ORIGIN.get_or_init(Instant::now)
}

/// Milliseconds on the machine's monotonic clock.
fn now_ms() -> u64 {
    origin().elapsed().as_millis() as u64
}

mod ffi {
    use std::ffi::c_void;

    pub type CGEventRef = *mut c_void;
    pub type CGEventTapProxy = *mut c_void;
    pub type CFMachPortRef = *mut c_void;
    pub type CFRunLoopSourceRef = *mut c_void;
    pub type CFRunLoopRef = *mut c_void;
    pub type CFStringRef = *const c_void;

    pub type CGEventTapCallBack = Option<
        unsafe extern "C" fn(
            proxy: CGEventTapProxy,
            event_type: u32,
            event: CGEventRef,
            user_info: *mut c_void,
        ) -> CGEventRef,
    >;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        pub fn CGEventTapIsEnabled(tap: CFMachPortRef) -> bool;
        pub fn CGEventGetFlags(event: CGEventRef) -> u64;
        pub fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
        pub fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub fn CFMachPortCreateRunLoopSource(
            allocator: *mut c_void,
            port: CFMachPortRef,
            order: i64,
        ) -> CFRunLoopSourceRef;
        pub fn CFMachPortInvalidate(port: CFMachPortRef);
        pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        pub fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        pub fn CFRunLoopRun();
        pub fn CFRetain(cf: *const c_void) -> *const c_void;
        pub fn CFRelease(cf: *mut c_void);
        pub static kCFRunLoopCommonModes: CFStringRef;
    }

    pub const KEYBOARD_EVENT_KEYCODE: u32 = 9;
}

pub fn start(binding: PttBinding, command_binding: CommandBinding, callback: Callback) {
    // Y0-D: a `--smoke` launch installs NO system-wide keyboard tap. This is the
    // single chokepoint for the PTT hotkey, so refusing here is refusing for
    // every caller — two lanes plus Wilson's own install can no longer all claim
    // fn⌃ at once.
    if !crate::smoke::global_hotkeys_allowed() {
        log::warn!("{}", crate::smoke::HOTKEY_REFUSAL);
        return;
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        if let Some(s) = GLOBAL_STATE.lock().as_ref() {
            *s.binding.lock() = binding;
            *s.command_binding.lock() = command_binding;
        }
        return;
    }
    let _ = origin();

    let state = Arc::new(TapState {
        binding: Mutex::new(binding),
        command_binding: Mutex::new(command_binding),
        machine: Mutex::new(PttMachine::new()),
        fn_hint: Mutex::new(FnHintTracker::default()),
        callback,
    });
    *GLOBAL_STATE.lock() = Some(state.clone());

    if let Err(e) = thread::Builder::new()
        .name("wv-fn-ptt".into())
        .spawn(move || run_tap(state))
    {
        // Non-fatal: without the fn PTT tap the app still runs via the tray
        // Start/Stop toggle and the secondary ⌘⇧V shortcut — don't take the
        // whole process down just because the OS refused this one thread.
        log::error!(
            "failed to spawn fn PTT thread: {e}; PTT hotkey disabled (tray / ⌘⇧V still work)"
        );
        RUNNING.store(false, Ordering::SeqCst);
    }
}

pub fn set_binding(binding: PttBinding) {
    if let Some(s) = GLOBAL_STATE.lock().as_ref() {
        *s.binding.lock() = binding;
        log::info!("PTT binding → {:?}", binding);
    }
}

/// Change the command-mode modifier live (Settings save), like `set_binding`.
pub fn set_command_binding(binding: CommandBinding) {
    if let Some(s) = GLOBAL_STATE.lock().as_ref() {
        *s.command_binding.lock() = binding;
        log::info!("command-mode binding → {:?}", binding);
    }
}

/// Was the press that produced the current take a COMMAND-MODE press (YV49)?
///
/// Read by the `Start` handler: `true` means "edit the selection", `false` means
/// ordinary dictation. Latched at key-down, so it stays true for the whole take
/// even after the user lifts the extra modifier. Always `false` when no tap is
/// running (tray / pill / hands-free starts).
pub fn command_mode_active() -> bool {
    GLOBAL_STATE
        .lock()
        .as_ref()
        .map(|s| s.machine.lock().command_mode())
        .unwrap_or(false)
}

/// When the combo currently being held went down (YV35) — the anchor for the
/// press→capture_start latency span. `Some` while a press is live (the hold-arm
/// timer fires `Start` well before the release clears it), `None` for every
/// non-PTT start (tray, pill button, hands-free latch) so the caller falls back
/// to measuring from its own start request.
pub fn press_started_at() -> Option<Instant> {
    let state = GLOBAL_STATE.lock().clone()?;
    let at = state.machine.lock().press_at_ms()?;
    Some(origin() + Duration::from_millis(at))
}

/// Is the live take in its "Still recording" state (read by `build_status`)?
pub fn still_recording_prompted() -> bool {
    GLOBAL_STATE
        .lock()
        .as_ref()
        .map(|s| s.machine.lock().still_recording())
        .unwrap_or(false)
}

static END_HANDS_FREE_CALLS: AtomicU64 = AtomicU64::new(0);

/// How many times [`end_hands_free`] has run this process (tests read it).
pub fn end_hands_free_calls() -> u64 {
    END_HANDS_FREE_CALLS.load(Ordering::SeqCst)
}

/// End hands-free from OUTSIDE the tap (pill / Home button / sidebar / tray Stop).
///
/// The PTT state machine keeps its own `hands_free` latch separate from
/// `AppState.hands_free`. If we only clear the app-side flag, the next fn press
/// is consumed just to reset the stale latch — swallowing the user's next
/// gesture. This resets the machine's take state too, mirroring the internal
/// tap-ends-hands-free path; a key physically held right now is suppressed
/// until it is released (and only then — otherwise it would suppress the
/// user's NEXT press instead).
pub fn end_hands_free() {
    END_HANDS_FREE_CALLS.fetch_add(1, Ordering::SeqCst);
    if let Some(s) = GLOBAL_STATE.lock().as_ref() {
        s.machine.lock().external_stop();
    }
}

/// Every way a take can be stopped. All of them go through
/// [`stop_take_with`], which is the one stop function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOrigin {
    /// fn key-up (hold) or fn tap (hands-free / still recording).
    Hotkey,
    /// The pill's stop control (and the Home / sidebar button — the same
    /// `manual_toggle` command).
    OnScreen,
    /// The menu bar's Stop / Hands-free items.
    MenuBar,
}

impl StopOrigin {
    pub fn name(self) -> &'static str {
        match self {
            Self::Hotkey => "the hotkey",
            Self::OnScreen => "the pill / on-screen stop",
            Self::MenuBar => "the menu bar",
        }
    }
    fn index(self) -> usize {
        match self {
            Self::Hotkey => 0,
            Self::OnScreen => 1,
            Self::MenuBar => 2,
        }
    }
}

static STOPS_BY_ORIGIN: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// How many takes this process stopped via `origin`.
pub fn stops_by(origin: StopOrigin) -> u64 {
    STOPS_BY_ORIGIN[origin.index()].load(Ordering::SeqCst)
}

/// THE stop function. Clears the machine's latch with [`end_hands_free`] FIRST
/// (so the next fn press is never swallowed resetting it), counts the origin,
/// then runs the app's stop (`stop_and_transcribe`).
pub fn stop_take_with(origin: StopOrigin, stop: impl FnOnce()) {
    end_hands_free();
    STOPS_BY_ORIGIN[origin.index()].fetch_add(1, Ordering::SeqCst);
    log::info!("take stop via {}", origin.name());
    stop();
}

/// Run one machine step's effects. Called with NO machine lock held.
fn dispatch(state: &Arc<TapState>, step: Step) {
    if let Some(gen) = step.arm_hold {
        spawn_hold_timer(gen);
    }
    for ev in &step.events {
        match ev {
            PttEvent::Start => log::info!("PTT → Start"),
            PttEvent::Stop => log::info!("PTT → Stop"),
            PttEvent::StillRecording => log::warn!("PTT → still recording (prompt on pill)"),
            other => log::info!("PTT → {other:?}"),
        }
        (state.callback)(*ev);
    }
    if state.machine.lock().poll_wanted() {
        ensure_poll();
    }
}

/// Decode a flags word into (fn, control, command, option).
fn decode_flags(flags: u64) -> (bool, bool, bool, bool) {
    (
        flags & KCG_EVENT_FLAG_MASK_SECONDARY_FN != 0,
        flags & KCG_EVENT_FLAG_MASK_CONTROL != 0,
        flags & KCG_EVENT_FLAG_MASK_COMMAND != 0,
        flags & KCG_EVENT_FLAG_MASK_ALTERNATE != 0,
    )
}

static HID_READ_LOGGED: AtomicBool = AtomicBool::new(false);

/// One HID-state sample fed to the machine as a `Poll` edge.
fn poll_once(state: &Arc<TapState>) -> Step {
    let hid = unsafe { ffi::CGEventSourceFlagsState(POLL_SOURCE_STATE) };
    let (fn_down, control, command, option) = decode_flags(hid);
    let want = combo_wanted(*state.binding.lock(), fn_down, control);
    let cmd = command_wanted(*state.command_binding.lock(), command, option);
    let now = now_ms();
    let mut m = state.machine.lock();
    let was_proven = m.hid_proven();
    let mut step = m.poll(want, fn_down, cmd, now);
    if !was_proven && m.hid_proven() && !HID_READ_LOGGED.swap(true, Ordering::SeqCst) {
        // The measurement the PR asks for, on whatever grants this build has.
        log::info!("FN PTT: HID flags read sees fn (second stop mechanism live)");
    }
    let tick = m.tick(now);
    step.events.extend(tick.events);
    step
}

fn spawn_hold_timer(gen: u64) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(HOLD_ARM_MS));
        let Some(st) = GLOBAL_STATE.lock().clone() else {
            return;
        };
        // Reconcile against the keyboard first: a release the tap lost must
        // not become a take nobody is holding.
        let reconcile = poll_once(&st);
        dispatch(&st, reconcile);
        let step = st.machine.lock().hold_timer(gen, now_ms());
        dispatch(&st, step);
    });
}

/// The bare-fn hint, once: log it and hand it to the app (the pill shows it).
fn emit_fn_hint(state: &Arc<TapState>) {
    log::info!("PTT → fn without ⌃ — hint: {FN_WITHOUT_MODIFIER_HINT}");
    (state.callback)(PttEvent::FnWithoutModifierHint);
}

/// `FN_HINT_HOLD_MS` after a bare fn went down: still held, still bare → hint.
fn spawn_fn_hint_check(gen: u64) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(FN_HINT_HOLD_MS + 1));
        let Some(st) = GLOBAL_STATE.lock().clone() else {
            return;
        };
        let (_, hctl, _, _) =
            decode_flags(unsafe { ffi::CGEventSourceFlagsState(POLL_SOURCE_STATE) });
        let binding = *st.binding.lock();
        let due = st
            .fn_hint
            .lock()
            .on_hold_check(gen, binding, hctl, now_ms());
        if due {
            emit_fn_hint(&st);
        }
    });
}

/// Start the live-take poll if it is not already running.
fn ensure_poll() {
    if POLLING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = thread::Builder::new()
        .name("wv-fn-poll".into())
        .spawn(|| loop {
            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
            let Some(st) = GLOBAL_STATE.lock().clone() else {
                POLLING.store(false, Ordering::SeqCst);
                return;
            };
            let step = poll_once(&st);
            dispatch(&st, step);
            if !st.machine.lock().poll_wanted() {
                POLLING.store(false, Ordering::SeqCst);
                // A take that went live between the check and the store must
                // not be left without its poll.
                if st.machine.lock().poll_wanted() && !POLLING.swap(true, Ordering::SeqCst) {
                    continue;
                }
                return;
            }
        });
    if let Err(e) = spawned {
        log::error!("FN PTT: could not start the live-take poll: {e}");
        POLLING.store(false, Ordering::SeqCst);
    }
}

/// Create the tap on the CURRENT thread's run loop and publish its port.
unsafe fn install_tap(
    user_info: *mut std::ffi::c_void,
) -> Result<ffi::CFMachPortRef, &'static str> {
    let mask = (1u64 << KCG_EVENT_FLAGS_CHANGED) | (1u64 << KCG_EVENT_KEY_DOWN);
    let tap = ffi::CGEventTapCreate(
        TAP_LOCATION,
        KCG_HEAD_INSERT,
        KCG_EVENT_TAP_OPTION_LISTEN_ONLY,
        mask,
        Some(tap_callback),
        user_info,
    );
    if tap.is_null() {
        return Err("CGEventTapCreate failed — enable Accessibility");
    }
    // Y1-A: retain and publish the port BEFORE enabling, so a disable
    // notification that arrives immediately still finds something to re-arm.
    ffi::CFRetain(tap as *const std::ffi::c_void);
    TAP_PORT.store(tap, Ordering::SeqCst);
    ffi::CGEventTapEnable(tap, true);
    let source = ffi::CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
    if source.is_null() {
        return Err("run loop source failed");
    }
    ffi::CFRunLoopAddSource(
        ffi::CFRunLoopGetCurrent(),
        source,
        ffi::kCFRunLoopCommonModes,
    );
    Ok(tap)
}

fn run_tap(state: Arc<TapState>) {
    let user_info = Arc::into_raw(state) as *mut std::ffi::c_void;
    TAP_USER_INFO.store(user_info, Ordering::SeqCst);

    unsafe {
        if let Err(e) = install_tap(user_info) {
            log::error!("FN PTT: {e}");
            // yap-p0-hotkeys-no-permission: attempted, nothing delivered. The
            // next permission-watch edge retries (RUNNING is cleared below).
            set_fn_tap_status(FnTapStatus::Blocked(crate::permissions::named(
                TAP_BLOCKED_REASON,
            )));
            RUNNING.store(false, Ordering::SeqCst);
            TAP_USER_INFO.store(std::ptr::null_mut(), Ordering::SeqCst);
            let _ = Arc::from_raw(user_info as *const TapState);
            return;
        }
        FIRST_EDGE_REPORTED.store(false, Ordering::SeqCst);
        set_fn_tap_status(FnTapStatus::ArmedNoDelivery);
        log::info!("FN PTT hybrid tap running (fn⌃ hold / double-tap hands-free)");
        ffi::CFRunLoopRun();
    }
}

unsafe extern "C" fn tap_callback(
    _proxy: ffi::CGEventTapProxy,
    event_type: u32,
    event: ffi::CGEventRef,
    user_info: *mut std::ffi::c_void,
) -> ffi::CGEventRef {
    // Y1-A FIRST, above the null guards: macOS delivers the two disable
    // pseudo-types regardless of our event mask, and the `event` it passes with
    // them is not one of ours — a null-event early return would drop exactly the
    // notification we must act on.
    if let Some(kind) = classify_tap_disable(event_type) {
        on_tap_disabled(kind, user_info);
        return event;
    }

    if user_info.is_null() || event.is_null() {
        return event;
    }
    Arc::increment_strong_count(user_info as *const TapState);
    let state = Arc::from_raw(user_info as *const TapState);

    // yap-perm-truth: a real key / flags edge is the ONLY delivery evidence
    // `permissions::input_monitoring_state` accepts.
    if event_type == KCG_EVENT_FLAGS_CHANGED || event_type == KCG_EVENT_KEY_DOWN {
        note_tap_edge();
    }

    let flags = ffi::CGEventGetFlags(event);
    let keycode = ffi::CGEventGetIntegerValueField(event, ffi::KEYBOARD_EVENT_KEYCODE);
    let hid_flags = ffi::CGEventSourceFlagsState(POLL_SOURCE_STATE);
    let (efn, ectl, ecmd, eopt) = decode_flags(flags);
    let (hfn, hctl, hcmd, hopt) = decode_flags(hid_flags);
    let fn_down = efn || hfn;
    let control = ectl || hctl;
    let command = ecmd || hcmd;
    let option = eopt || hopt;

    if event_type == KCG_EVENT_FLAGS_CHANGED {
        // Any flags-changed event may be an fn or Control edge — re-evaluate the
        // combo unconditionally; the machine no-ops when nothing changed.
        let want = combo_wanted(*state.binding.lock(), fn_down, control);
        let cmd = command_wanted(*state.command_binding.lock(), command, option);
        let step = state
            .machine
            .lock()
            .edge(want, cmd, now_ms(), EdgeSource::Tap);
        dispatch(&state, step);
        let binding = *state.binding.lock();
        let (hint, arm) =
            state
                .fn_hint
                .lock()
                .on_flags(binding, fn_down, control, command || option, now_ms());
        if let Some(gen) = arm {
            spawn_fn_hint_check(gen);
        }
        if hint {
            emit_fn_hint(&state);
        }
    } else if event_type == KCG_EVENT_KEY_DOWN {
        if !is_modifier_keycode(keycode) {
            state.fn_hint.lock().on_key_down();
        }
        let step = state.machine.lock().key_down(is_modifier_keycode(keycode));
        if !step.events.is_empty() {
            log::info!("PTT interrupted by keycode {keycode}");
        }
        dispatch(&state, step);
    }

    event
}

fn is_modifier_keycode(code: i64) -> bool {
    matches!(code, 54..=63)
}

fn combo_wanted(binding: PttBinding, fn_down: bool, control: bool) -> bool {
    match binding {
        PttBinding::Fn => fn_down,
        PttBinding::FnControl => fn_down && control,
        PttBinding::FnOrFnControl => fn_down,
    }
}

/// Does the extra modifier held right now mean COMMAND MODE (YV49)?
///
/// Pure so the binding policy is testable without a CGEvent tap. `Off` disables
/// command mode entirely, which is why it can never be true for `Off`.
fn command_wanted(binding: CommandBinding, command: bool, option: bool) -> bool {
    match binding {
        CommandBinding::Command => command,
        CommandBinding::Option => option,
        CommandBinding::Off => false,
    }
}

// ── Y1-A: a tap macOS switched OFF is re-armed, not mourned ──────────────────
//
// macOS kills an event tap whose callback is slow, and it tells the callback by
// invoking it with a *pseudo event type* — `kCGEventTapDisabledByTimeout`
// (0xFFFFFFFE) or `kCGEventTapDisabledByUserInput` (0xFFFFFFFF). Those two are
// delivered REGARDLESS of the event mask this tap asked for, and once either
// arrives the tap stays dead until `CGEventTapEnable` is called again. Before
// this section existed the tree called `CGEventTapEnable` exactly once, at
// startup, so a single slow callback, one long decode or one sleep/wake cycle
// left the push-to-talk hotkey silently dead for the rest of the process — a
// failure indistinguishable, from the user's chair, from a revoked
// Accessibility grant.

/// The tap timed out: our callback took longer than the window server's budget.
const KCG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFFFFFE;
/// A user-input flood took the tap out. NOT normal, and still needs re-arming.
const KCG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFFFFFF;

/// Which of the two ways macOS switched the tap off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapDisable {
    /// `kCGEventTapDisabledByTimeout` — our callback was too slow.
    Timeout,
    /// `kCGEventTapDisabledByUserInput` — a user-input flood.
    UserInput,
}

impl TapDisable {
    /// The Apple constant name, for the log line.
    pub fn name(self) -> &'static str {
        match self {
            Self::Timeout => "kCGEventTapDisabledByTimeout",
            Self::UserInput => "kCGEventTapDisabledByUserInput",
        }
    }

    /// One clause of cause, for the support bundle.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Timeout => "the tap callback exceeded the window server timeout",
            Self::UserInput => "a user-input flood took the tap out",
        }
    }
}

/// PURE classification of the pseudo event type macOS hands the callback.
///
/// Pure on purpose: it is the whole decision the callback makes, and this way
/// it is unit-testable with no window server, no tap and no permission grant
/// (`tests/tap_health.rs`). `None` means "an ordinary event, keep handling it".
pub fn classify_tap_disable(event_type: u32) -> Option<TapDisable> {
    match event_type {
        KCG_EVENT_TAP_DISABLED_BY_TIMEOUT => Some(TapDisable::Timeout),
        KCG_EVENT_TAP_DISABLED_BY_USER_INPUT => Some(TapDisable::UserInput),
        _ => None,
    }
}

/// How often this process has had to switch its own tap back on, split by cause.
///
/// Counted, not merely logged: "the hotkey died once an hour" and "the hotkey
/// died once since launch" are different bugs and a log line the user never
/// reads cannot tell them apart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct TapHealth {
    /// Total re-arm attempts this session.
    pub re_arms: u32,
    pub by_timeout: u32,
    pub by_user_input: u32,
    /// yap-perm-truth — key / flags edges `tap_callback` received since the
    /// last re-arm (or since the tap was created). THE delivery evidence: a
    /// re-arm count says nothing about whether events arrive.
    pub edges_since_re_arm: u64,
}

impl TapHealth {
    /// PURE counter step — the global below is just one instance of this.
    /// A re-arm resets the delivery evidence: what the old tap delivered says
    /// nothing about the re-armed one.
    pub fn record(&mut self, kind: TapDisable) {
        self.re_arms = self.re_arms.saturating_add(1);
        self.edges_since_re_arm = 0;
        match kind {
            TapDisable::Timeout => self.by_timeout = self.by_timeout.saturating_add(1),
            TapDisable::UserInput => self.by_user_input = self.by_user_input.saturating_add(1),
        }
    }

    /// PURE — one key or flags edge reached the callback.
    pub fn note_edge(&mut self) {
        self.edges_since_re_arm = self.edges_since_re_arm.saturating_add(1);
    }

    /// Is the tap observably delivering? Only an edge since the last re-arm says so.
    pub fn delivering(&self) -> bool {
        self.edges_since_re_arm > 0
    }
}

/// The live edge counter, bumped from `tap_callback` without taking a lock
/// (the callback runs on every key event and must stay well inside the window
/// server's timeout). Reset to zero on every re-arm by [`note_tap_disabled`].
static EDGES_SINCE_RE_ARM: AtomicU64 = AtomicU64::new(0);

/// Count one key or flags edge. Called from `tap_callback` only.
pub fn note_tap_edge() {
    EDGES_SINCE_RE_ARM.fetch_add(1, Ordering::Relaxed);
    // yap-p0-hotkeys-no-permission: "Delivering" needs a delivered edge. One
    // atomic swap per event; the status write runs once per install.
    if !FIRST_EDGE_REPORTED.swap(true, Ordering::Relaxed) {
        set_fn_tap_status(FnTapStatus::Delivering);
    }
}

/// The first delivered edge since this tap was installed has been reported.
static FIRST_EDGE_REPORTED: AtomicBool = AtomicBool::new(false);

static TAP_HEALTH: Mutex<TapHealth> = Mutex::new(TapHealth {
    re_arms: 0,
    by_timeout: 0,
    by_user_input: 0,
    edges_since_re_arm: 0,
});

/// The live CFMachPort for the tap, so the callback can re-enable the SAME tap
/// instead of respawning the thread. Retained at creation (`CFRetain`) because
/// the callback reads it from an arbitrary later moment.
static TAP_PORT: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Snapshot of the session's tap health. Cheap; safe from any thread.
pub fn tap_health() -> TapHealth {
    let mut h = *TAP_HEALTH.lock();
    h.edges_since_re_arm = EDGES_SINCE_RE_ARM.load(Ordering::Relaxed);
    h
}

/// Count one disable + re-arm and hand back the new snapshot.
pub fn note_tap_disabled(kind: TapDisable) -> TapHealth {
    let mut g = TAP_HEALTH.lock();
    g.record(kind);
    EDGES_SINCE_RE_ARM.store(0, Ordering::Relaxed);
    *g
}

/// What the pill (PERM-C's surface) and the support bundle (PERM-E's report)
/// should SAY about tap health — `None` when there is nothing to say.
///
/// Silent the first time: one re-arm is macOS being macOS, we fixed it in
/// microseconds and the user saw nothing. Visible from the second one in a
/// session, because a tap that keeps dying is a real, reportable fault.
pub fn tap_health_message(health: TapHealth) -> Option<String> {
    if health.re_arms < 2 {
        return None;
    }
    Some(format!(
        "the hotkey stopped listening — re-armed ({}× this session)",
        health.re_arms
    ))
}

/// Is this disable type one we re-arm? BOTH of them are.
///
/// Spelled out as a function, and read by the callback, so "ByUserInput is not
/// normal — it means a user-input flood took the tap out, and it still needs
/// re-arming" is a rule with a test on it rather than a comment someone can
/// quietly regress.
pub fn should_re_arm(kind: TapDisable) -> bool {
    match kind {
        TapDisable::Timeout => true,
        TapDisable::UserInput => true,
    }
}

/// Switch the existing tap back on. NOT a respawn: same CFMachPort, same run
/// loop source, same thread.
///
/// Returns whether the `CGEventTapEnable` call was actually issued — false only
/// when no tap port has been published yet (no tap installed, e.g. in a test
/// binary or a `--smoke` launch), which is reported, never silently swallowed.
pub fn re_arm_tap() -> bool {
    let tap = TAP_PORT.load(Ordering::SeqCst);
    if tap.is_null() {
        log::error!("FN PTT: tap disabled before the port was recorded — cannot re-arm");
        return false;
    }
    unsafe { ffi::CGEventTapEnable(tap, true) };
    spawn_re_arm_verifier(tap);
    true
}

// ── yap-hotkey-stop DO 1: a re-arm is VERIFIED, and a tap that keeps dying is
//    rebuilt ───────────────────────────────────────────────────────────────────

/// How long after `CGEventTapEnable` the tap must read enabled.
pub const RE_ARM_VERIFY_MS: u64 = 100;
/// This many disables …
pub const RECREATE_AFTER_DISABLES: usize = 3;
/// … inside this window → invalidate the port and create a fresh tap.
pub const RECREATE_WINDOW_MS: u64 = 60_000;

/// Sliding window of recent disables. PURE (tests/hotkey_stop.rs).
#[derive(Debug, Clone, Default)]
pub struct DisableWindow {
    at: Vec<u64>,
}

impl DisableWindow {
    pub const fn new() -> Self {
        Self { at: Vec::new() }
    }

    /// Record a disable at `now_ms`; `true` means "recreate the tap now" (the
    /// window is then cleared, so the fresh tap starts with a clean slate).
    pub fn record(&mut self, now_ms: u64) -> bool {
        self.at
            .retain(|t| now_ms.saturating_sub(*t) < RECREATE_WINDOW_MS);
        self.at.push(now_ms);
        if self.at.len() >= RECREATE_AFTER_DISABLES {
            self.at.clear();
            return true;
        }
        false
    }
}

static DISABLES: Mutex<DisableWindow> = Mutex::new(DisableWindow::new());
// The edge counter is yap-perm-truth's single `EDGES_SINCE_RE_ARM` above (fed by
// `note_tap_edge` for FLAGS_CHANGED + KEY_DOWN, reset by `note_tap_disabled`).
// There is deliberately no second counter here.
static RE_ARMS_VERIFIED: AtomicU64 = AtomicU64::new(0);
static RE_ARMS_FAILED: AtomicU64 = AtomicU64::new(0);
static TAPS_RECREATED: AtomicU64 = AtomicU64::new(0);
/// The port a recreate invalidated. Released on the NEXT recreate, never inside
/// its own callout.
static RETIRED_PORT: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Re-arm bookkeeping for the support bundle / logs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct ReArmStats {
    pub verified: u64,
    pub failed: u64,
    pub recreated: u64,
    pub edges_since_re_arm: u64,
}

pub fn re_arm_stats() -> ReArmStats {
    ReArmStats {
        verified: RE_ARMS_VERIFIED.load(Ordering::SeqCst),
        failed: RE_ARMS_FAILED.load(Ordering::SeqCst),
        recreated: TAPS_RECREATED.load(Ordering::SeqCst),
        edges_since_re_arm: tap_health().edges_since_re_arm,
    }
}

/// PURE verdict for a verification read: `Some(ms)` = the tap read enabled
/// `ms` after `CGEventTapEnable`; `None` = it never did inside the budget.
pub fn re_arm_verdict(enabled_after_ms: Option<u64>) -> &'static str {
    match enabled_after_ms {
        Some(ms) if ms <= RE_ARM_VERIFY_MS => "re-arm verified",
        _ => "re-arm FAILED",
    }
}

/// Read `CGEventTapIsEnabled` for up to `RE_ARM_VERIFY_MS`, off the tap thread
/// (sleeping on the run loop would itself trip the timeout), and log the
/// verdict.
fn spawn_re_arm_verifier(tap: ffi::CFMachPortRef) {
    let port = tap as usize;
    let _ = thread::Builder::new()
        .name("wv-tap-verify".into())
        .spawn(move || {
            let tap = port as ffi::CFMachPortRef;
            let t0 = Instant::now();
            let mut enabled_after = None;
            loop {
                if unsafe { ffi::CGEventTapIsEnabled(tap) } {
                    enabled_after = Some(t0.elapsed().as_millis() as u64);
                    break;
                }
                if t0.elapsed() >= Duration::from_millis(RE_ARM_VERIFY_MS) {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            let verdict = re_arm_verdict(enabled_after);
            if enabled_after.is_some() {
                RE_ARMS_VERIFIED.fetch_add(1, Ordering::SeqCst);
                log::info!(
                    "FN PTT: {verdict} — CGEventTapIsEnabled true after {} ms",
                    enabled_after.unwrap_or(0)
                );
            } else {
                RE_ARMS_FAILED.fetch_add(1, Ordering::SeqCst);
                log::error!(
                    "FN PTT: {verdict} — CGEventTapIsEnabled still false {RE_ARM_VERIFY_MS} ms after CGEventTapEnable"
                );
                // focus-2026-10-19: a failed re-arm used to END here, leaving
                // the stored status `Delivering` — nothing retried, every
                // surface said fn works, and fn was dead until the next
                // disable (which a disabled tap does not get). Say so, so the
                // hotkey table falls back to the dictate chord, and keep
                // re-enabling until macOS lets the tap back on.
                set_fn_tap_status(FnTapStatus::Blocked(format!(
                    "{} — re-arming",
                    crate::hotkey_table::PAUSED_PREFIX
                )));
                keep_re_arming(tap);
            }
        });
}

/// Retry delays for a tap macOS will not re-enable yet (Secure Input up, a
/// user-input flood still running). Short at first, then a steady 2 s.
pub fn re_arm_retry_delay_ms(attempt: u32) -> u64 {
    match attempt {
        0..=9 => 250,
        10..=29 => 1_000,
        _ => 2_000,
    }
}

/// fix-249 advisory (a): the retry is BOUNDED. 10 × 250 ms + 20 × 1 s + 300 ×
/// 2 s ≈ 10.4 min, then the thread stops and leaves the terminal
/// [`RE_ARM_GAVE_UP_PREFIX`] status (the hotkey table reads it as a stale grant
/// when Input Monitoring is on, as Needs Input Monitoring when it is off; the
/// dictate chord is the live key either way), and `attempt_tap` / the next
/// disable owns recovery.
pub const RE_ARM_MAX_ATTEMPTS: u32 = 330;

/// The terminal reason stored once [`RE_ARM_MAX_ATTEMPTS`] retries failed.
pub const RE_ARM_GAVE_UP_PREFIX: &str = "macOS kept the fn listener off";

pub fn re_arm_gave_up_reason(attempts: u32) -> String {
    format!("{RE_ARM_GAVE_UP_PREFIX} after {attempts} re-arm tries")
}

/// The port a re-arm retry thread currently owns (null = none). Single flight
/// PER TAP: a second verifier for the same port does not start a second loop;
/// a recreated port takes ownership and the old loop exits on its next tick.
static RE_ARM_RETRY_OWNER: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Claim the retry loop for `tap`. `false` = one is already running for it.
pub fn claim_re_arm(owner: &AtomicPtr<std::ffi::c_void>, tap: *mut std::ffi::c_void) -> bool {
    owner.swap(tap, Ordering::SeqCst) != tap
}

/// Release the claim, only if `tap` still owns it.
pub fn release_re_arm(owner: &AtomicPtr<std::ffi::c_void>, tap: *mut std::ffi::c_void) {
    let _ = owner.compare_exchange(tap, std::ptr::null_mut(), Ordering::SeqCst, Ordering::SeqCst);
}

/// Keep calling `CGEventTapEnable` on `tap` until it reads enabled, a recreate
/// replaced the port (then the new tap's own verifier owns it), or
/// [`RE_ARM_MAX_ATTEMPTS`] tries failed (terminal status, thread ends).
fn keep_re_arming(tap: ffi::CFMachPortRef) {
    if !claim_re_arm(&RE_ARM_RETRY_OWNER, tap) {
        log::debug!("FN PTT: re-arm retry already running for this tap");
        return;
    }
    let mut attempt: u32 = 0;
    loop {
        thread::sleep(Duration::from_millis(re_arm_retry_delay_ms(attempt)));
        attempt = attempt.saturating_add(1);
        if TAP_PORT.load(Ordering::SeqCst) != tap
            || RE_ARM_RETRY_OWNER.load(Ordering::SeqCst) != tap
        {
            log::info!("FN PTT: re-arm retry stopped — the tap was replaced");
            release_re_arm(&RE_ARM_RETRY_OWNER, tap);
            return;
        }
        unsafe { ffi::CGEventTapEnable(tap, true) };
        if unsafe { ffi::CGEventTapIsEnabled(tap) } {
            RE_ARMS_VERIFIED.fetch_add(1, Ordering::SeqCst);
            FIRST_EDGE_REPORTED.store(false, Ordering::SeqCst);
            set_fn_tap_status(FnTapStatus::ArmedNoDelivery);
            log::info!("FN PTT: re-arm retry #{attempt} took — fn listener back on");
            release_re_arm(&RE_ARM_RETRY_OWNER, tap);
            return;
        }
        if attempt >= RE_ARM_MAX_ATTEMPTS {
            log::error!(
                "FN PTT: re-arm gave up after {attempt} tries — the dictate chord stays the live key"
            );
            set_fn_tap_status(FnTapStatus::Blocked(re_arm_gave_up_reason(attempt)));
            release_re_arm(&RE_ARM_RETRY_OWNER, tap);
            return;
        }
        if attempt.is_multiple_of(30) {
            log::warn!("FN PTT: tap still off after {attempt} re-arm retries (Secure Input or a stale grant?)");
        }
    }
}

/// Invalidate the current port and build a fresh tap on THIS thread's run loop
/// (the callback runs on the tap thread, so that is the tap's own run loop).
unsafe fn recreate_tap(user_info: *mut std::ffi::c_void) -> bool {
    let old = TAP_PORT.swap(std::ptr::null_mut(), Ordering::SeqCst);
    if !old.is_null() {
        ffi::CFMachPortInvalidate(old);
    }
    let previous = RETIRED_PORT.swap(old, Ordering::SeqCst);
    if !previous.is_null() {
        ffi::CFRelease(previous);
    }
    match install_tap(user_info) {
        Ok(tap) => {
            TAPS_RECREATED.fetch_add(1, Ordering::SeqCst);
            FIRST_EDGE_REPORTED.store(false, Ordering::SeqCst);
            set_fn_tap_status(FnTapStatus::ArmedNoDelivery);
            log::warn!(
                "FN PTT: tap recreated on the same run loop ({RECREATE_AFTER_DISABLES} disables in {} s) — old port invalidated",
                RECREATE_WINDOW_MS / 1000
            );
            spawn_re_arm_verifier(tap);
            true
        }
        Err(e) => {
            log::error!("FN PTT: tap recreate FAILED: {e}");
            set_fn_tap_status(FnTapStatus::Blocked(crate::permissions::named(
                TAP_BLOCKED_REASON,
            )));
            false
        }
    }
}

/// The disable pseudo-event: count it, tell a live take, then re-arm — or,
/// on the third disable inside a minute, rebuild the tap.
unsafe fn on_tap_disabled(kind: TapDisable, user_info: *mut std::ffi::c_void) {
    // Read the delivery evidence BEFORE `note_tap_disabled` resets it, or the
    // log line would always report 0 edges.
    let edges = tap_health().edges_since_re_arm;
    let health = note_tap_disabled(kind);
    if !user_info.is_null() {
        Arc::increment_strong_count(user_info as *const TapState);
        let state = Arc::from_raw(user_info as *const TapState);
        let step = state.machine.lock().tap_disabled();
        dispatch(&state, step);
    }
    let recreate = DISABLES.lock().record(now_ms());
    let ui = if user_info.is_null() {
        TAP_USER_INFO.load(Ordering::SeqCst)
    } else {
        user_info
    };
    let issued = if recreate && !ui.is_null() {
        recreate_tap(ui)
    } else {
        should_re_arm(kind) && re_arm_tap()
    };
    log::warn!(
        "FN PTT: tap disabled by {} ({}) — re-arm #{} {} ({} edges delivered since the last re-arm)",
        kind.name(),
        kind.reason(),
        health.re_arms,
        match (recreate, issued) {
            (true, true) => "→ tap recreated",
            (_, true) => "issued, verifying",
            _ => "FAILED",
        },
        edges
    );
}

#[cfg(test)]
mod fn_hint_tests {
    use super::{
        fn_hint_due, FnHintTracker, PttBinding, FN_HINT_HOLD_MS, FN_HINT_THROTTLE_MS,
        FN_WITHOUT_MODIFIER_HINT,
    };

    /// `fn_hint_due` under the default fn⌃ binding.
    fn due(fn_down: bool, ctrl: bool, held: u64, last: Option<u64>, now: u64) -> bool {
        fn_hint_due(PttBinding::FnControl, fn_down, ctrl, held, last, now)
    }

    #[test]
    fn fn_hint_due_bare_fn_tap_under_fn_control_hints() {
        assert!(due(false, false, 120, None, 10_000));
    }

    #[test]
    fn fn_hint_due_with_ctrl_never_hints() {
        assert!(!due(false, true, 120, None, 10_000));
        assert!(!due(true, true, 900, None, 10_000));
    }

    #[test]
    fn fn_hint_due_bindings_that_fire_on_bare_fn_never_hint() {
        for b in [PttBinding::Fn, PttBinding::FnOrFnControl] {
            assert!(!fn_hint_due(b, false, false, 120, None, 10_000));
            assert!(!fn_hint_due(b, true, false, 900, None, 10_000));
        }
    }

    #[test]
    fn fn_hint_due_while_held_only_past_400_ms() {
        assert!(!due(true, false, 200, None, 10_000));
        assert!(!due(true, false, FN_HINT_HOLD_MS, None, 10_000));
        assert!(due(true, false, FN_HINT_HOLD_MS + 1, None, 10_000));
    }

    #[test]
    fn fn_hint_due_throttled_to_once_per_30_s() {
        let last = Some(10_000);
        assert!(!due(false, false, 120, last, 10_000 + 5_000));
        let edge = 10_000 + FN_HINT_THROTTLE_MS;
        assert!(!due(false, false, 120, last, edge - 1));
        assert!(due(false, false, 120, last, edge));
    }

    #[test]
    fn fn_hint_due_tracker_tap_hints_once_then_throttles() {
        let mut t = FnHintTracker::default();
        let b = PttBinding::FnControl;
        let (h, arm) = t.on_flags(b, true, false, false, 1_000);
        assert!(!h);
        assert_eq!(arm, Some(1));
        assert_eq!(t.on_flags(b, false, false, false, 1_090), (true, None));
        // A second bare tap 5 s later is throttled.
        t.on_flags(b, true, false, false, 6_000);
        assert_eq!(t.on_flags(b, false, false, false, 6_080), (false, None));
        // 30 s after the first hint it speaks again.
        t.on_flags(b, true, false, false, 32_000);
        assert_eq!(t.on_flags(b, false, false, false, 32_080), (true, None));
    }

    #[test]
    fn fn_hint_due_tracker_fn_then_ctrl_is_the_real_chord_no_hint() {
        let mut t = FnHintTracker::default();
        let b = PttBinding::FnControl;
        t.on_flags(b, true, false, false, 1_000);
        t.on_flags(b, true, true, false, 1_050); // ⌃ joins: the user is doing fn⌃
        assert!(!t.on_hold_check(1, b, true, 1_401));
        assert_eq!(t.on_flags(b, false, false, false, 2_000), (false, None));
    }

    #[test]
    fn fn_hint_due_tracker_fn_arrow_is_another_shortcut_no_hint() {
        let mut t = FnHintTracker::default();
        let b = PttBinding::FnControl;
        t.on_flags(b, true, false, false, 1_000);
        t.on_key_down(); // fn+← (Home)
        assert!(!t.on_hold_check(1, b, false, 1_401));
        assert_eq!(t.on_flags(b, false, false, false, 1_600), (false, None));
    }

    #[test]
    fn fn_hint_due_tracker_long_hold_hints_once_not_again_at_release() {
        let mut t = FnHintTracker::default();
        let b = PttBinding::FnControl;
        let (_, arm) = t.on_flags(b, true, false, false, 1_000);
        assert!(t.on_hold_check(arm.unwrap(), b, false, 1_401));
        // Release of the same press: throttled, no second hint.
        assert_eq!(t.on_flags(b, false, false, false, 2_500), (false, None));
        // A stale check for an earlier press never fires.
        t.on_flags(b, true, false, false, 40_000);
        assert!(!t.on_hold_check(arm.unwrap(), b, false, 40_401));
    }

    #[test]
    fn fn_hint_due_tracker_bare_fn_binding_never_arms() {
        let mut t = FnHintTracker::default();
        let b = PttBinding::Fn;
        assert_eq!(t.on_flags(b, true, false, false, 1_000), (false, None));
        assert_eq!(t.on_flags(b, false, false, false, 1_100), (false, None));
    }

    #[test]
    fn fn_hint_due_message_names_the_chord() {
        assert!(FN_WITHOUT_MODIFIER_HINT.starts_with("Hold fn⌃"));
        assert!(FN_WITHOUT_MODIFIER_HINT.chars().count() <= 34);
    }
}

#[cfg(test)]
mod tests {
    use super::{combo_wanted, command_wanted, CommandBinding, PttBinding};

    /// fix-249 advisory (a): one retry loop per tap, bounded, terminal != Paused.
    #[test]
    fn re_arm_retry_is_single_flight_per_tap_and_bounded() {
        use super::{
            claim_re_arm, re_arm_gave_up_reason, re_arm_retry_delay_ms, release_re_arm,
            RE_ARM_MAX_ATTEMPTS,
        };
        use std::sync::atomic::{AtomicPtr, Ordering};
        let owner = AtomicPtr::new(std::ptr::null_mut());
        let a = 0x10usize as *mut std::ffi::c_void;
        let b = 0x20usize as *mut std::ffi::c_void;
        assert!(claim_re_arm(&owner, a), "first claim wins");
        assert!(!claim_re_arm(&owner, a), "a second loop for the same tap is refused");
        assert!(claim_re_arm(&owner, b), "a recreated tap takes over");
        release_re_arm(&owner, a); // the old loop exiting must not free b's claim
        assert_eq!(owner.load(Ordering::SeqCst), b);
        release_re_arm(&owner, b);
        assert!(owner.load(Ordering::SeqCst).is_null());
        let total_ms: u64 = (0..RE_ARM_MAX_ATTEMPTS).map(re_arm_retry_delay_ms).sum();
        assert!(total_ms <= 11 * 60 * 1_000, "bounded: {total_ms} ms");
        assert!(
            !re_arm_gave_up_reason(RE_ARM_MAX_ATTEMPTS)
                .starts_with(crate::hotkey_table::PAUSED_PREFIX),
            "the terminal state is not Paused"
        );
    }

    /// YV49: the command-mode modifier must ride ON TOP of the existing PTT
    /// bindings — adding ⌘ can never stop a normal press from starting.
    #[test]
    fn command_modifier_does_not_break_the_ptt_bindings() {
        for binding in [
            PttBinding::Fn,
            PttBinding::FnControl,
            PttBinding::FnOrFnControl,
        ] {
            let plain = combo_wanted(binding, true, true);
            // The command modifier is not part of `combo_wanted` at all, so the
            // same fn/Control state still starts the take.
            assert_eq!(combo_wanted(binding, true, true), plain);
        }
        assert!(combo_wanted(PttBinding::FnControl, true, true));
        assert!(!combo_wanted(PttBinding::FnControl, true, false));
    }

    #[test]
    fn command_binding_selects_the_extra_modifier() {
        // Default: ⌘ arms command mode, ⌥ does not.
        assert!(command_wanted(CommandBinding::Command, true, false));
        assert!(!command_wanted(CommandBinding::Command, false, true));
        // Settings-chosen alternate binding.
        assert!(command_wanted(CommandBinding::Option, false, true));
        assert!(!command_wanted(CommandBinding::Option, true, false));
        // Off means every press dictates, whatever is held.
        assert!(!command_wanted(CommandBinding::Off, true, true));
        // No extra modifier → ordinary dictation.
        assert!(!command_wanted(CommandBinding::Command, false, false));
    }

    #[test]
    fn command_binding_from_settings() {
        assert_eq!(
            CommandBinding::from_settings("command"),
            CommandBinding::Command
        );
        assert_eq!(
            CommandBinding::from_settings("option"),
            CommandBinding::Option
        );
        assert_eq!(CommandBinding::from_settings("off"), CommandBinding::Off);
        // Unknown / missing values fall back to the default.
        assert_eq!(
            CommandBinding::from_settings("nonsense"),
            CommandBinding::Command
        );
    }
}
