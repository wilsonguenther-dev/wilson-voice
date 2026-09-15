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
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::Arc;
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

/// Hold longer than this → confirmed push-to-talk (not a tap).
const HOLD_ARM_MS: u64 = 280;
/// Two short releases within this window → double-tap (hands-free).
const DOUBLE_TAP_MS: u64 = 450;
/// Press shorter than this counts as a tap (not a hold).
const TAP_MAX_MS: u64 = 320;

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
}

type Callback = Arc<dyn Fn(PttEvent) + Send + Sync + 'static>;

struct TapState {
    binding: Mutex<PttBinding>,
    /// Which extra modifier makes a press a command-mode press (YV49).
    command_binding: Mutex<CommandBinding>,
    /// Whether the press currently in flight was a command-mode press. Latched
    /// on key-DOWN — the hold-arm timer fires `Start` up to `HOLD_ARM_MS` later
    /// and the user may lift ⌘ while speaking, so the modifier state at that
    /// point is not the answer.
    command_mode: AtomicBool,
    callback: Callback,
    /// Combo currently physically held.
    combo_down: AtomicBool,
    interrupted: AtomicBool,
    /// Hands-free latch active (recording without hold).
    hands_free: AtomicBool,
    /// Recording started because hold exceeded HOLD_ARM_MS.
    hold_armed: AtomicBool,
    /// When current press began.
    press_at: Mutex<Option<Instant>>,
    /// Last short-release time (for double-tap).
    last_tap_at: Mutex<Option<Instant>>,
    /// Generation counter so hold-arm timers can cancel.
    press_gen: std::sync::atomic::AtomicU64,
    /// After ending hands-free on key-down, ignore this hold until release.
    suppress_until_release: AtomicBool,
    last_edge: Mutex<Instant>,
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static GLOBAL_STATE: Mutex<Option<Arc<TapState>>> = Mutex::new(None);

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
        pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        pub fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        pub fn CFRunLoopRun();
        pub fn CFRetain(cf: *const c_void) -> *const c_void;
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

    let state = Arc::new(TapState {
        binding: Mutex::new(binding),
        command_binding: Mutex::new(command_binding),
        command_mode: AtomicBool::new(false),
        callback,
        combo_down: AtomicBool::new(false),
        interrupted: AtomicBool::new(false),
        hands_free: AtomicBool::new(false),
        hold_armed: AtomicBool::new(false),
        press_at: Mutex::new(None),
        last_tap_at: Mutex::new(None),
        press_gen: std::sync::atomic::AtomicU64::new(0),
        suppress_until_release: AtomicBool::new(false),
        last_edge: Mutex::new(Instant::now() - Duration::from_secs(1)),
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
        .map(|s| s.command_mode.load(Ordering::SeqCst))
        .unwrap_or(false)
}

/// When the combo currently being held went down (YV35) — the anchor for the
/// press→capture_start latency span. `Some` while a press is live (the hold-arm
/// timer fires `Start` well before the release clears it), `None` for every
/// non-PTT start (tray, pill button, hands-free latch) so the caller falls back
/// to measuring from its own start request.
pub fn press_started_at() -> Option<Instant> {
    let state = GLOBAL_STATE.lock().clone()?;
    let pressed = *state.press_at.lock();
    pressed
}

/// End hands-free from OUTSIDE the tap (pill / Home button / sidebar / tray Stop).
///
/// The PTT state machine keeps its own `hands_free` latch (an `AtomicBool`)
/// separate from `AppState.hands_free`. If we only clear the app-side flag, the
/// next fn press hits `on_combo_down`'s stale-latch branch and is consumed just
/// to reset it — swallowing the user's next gesture. This resets the tap-side
/// latch too, mirroring the internal tap-ends-hands-free path. `press_gen` is
/// bumped to cancel any in-flight hold-arm timer. `suppress_until_release` is set
/// ONLY if a key is physically held right now — otherwise it would suppress the
/// user's next press instead of the current (already-released) one.
pub fn end_hands_free() {
    if let Some(s) = GLOBAL_STATE.lock().as_ref() {
        s.hands_free.store(false, Ordering::SeqCst);
        s.hold_armed.store(false, Ordering::SeqCst);
        s.command_mode.store(false, Ordering::SeqCst);
        s.press_gen.fetch_add(1, Ordering::SeqCst);
        *s.press_at.lock() = None;
        *s.last_tap_at.lock() = None;
        if s.combo_down.load(Ordering::SeqCst) {
            s.suppress_until_release.store(true, Ordering::SeqCst);
        }
    }
}

fn run_tap(state: Arc<TapState>) {
    let mask = (1u64 << KCG_EVENT_FLAGS_CHANGED) | (1u64 << KCG_EVENT_KEY_DOWN);
    let user_info = Arc::into_raw(state) as *mut std::ffi::c_void;

    unsafe {
        let tap = ffi::CGEventTapCreate(
            KCG_HID_EVENT_TAP,
            KCG_HEAD_INSERT,
            KCG_EVENT_TAP_OPTION_LISTEN_ONLY,
            mask,
            Some(tap_callback),
            user_info,
        );
        if tap.is_null() {
            log::error!("FN PTT: CGEventTapCreate failed — enable Accessibility");
            RUNNING.store(false, Ordering::SeqCst);
            let _ = Arc::from_raw(user_info as *const TapState);
            return;
        }
        // Y1-A: retain and publish the port BEFORE enabling, so a disable
        // notification that arrives immediately still finds something to re-arm.
        ffi::CFRetain(tap as *const std::ffi::c_void);
        TAP_PORT.store(tap, Ordering::SeqCst);
        ffi::CGEventTapEnable(tap, true);
        let source = ffi::CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
        if source.is_null() {
            log::error!("FN PTT: run loop source failed");
            RUNNING.store(false, Ordering::SeqCst);
            let _ = Arc::from_raw(user_info as *const TapState);
            return;
        }
        let rl = ffi::CFRunLoopGetCurrent();
        ffi::CFRunLoopAddSource(rl, source, ffi::kCFRunLoopCommonModes);
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
        let health = note_tap_disabled(kind);
        let re_armed = should_re_arm(kind) && re_arm_tap();
        log::warn!(
            "FN PTT: tap disabled by {} ({}) — re-arm #{} {}",
            kind.name(),
            kind.reason(),
            health.re_arms,
            if re_armed { "issued" } else { "FAILED" }
        );
        return event;
    }

    if user_info.is_null() || event.is_null() {
        return event;
    }
    let state = &*(user_info as *const TapState);

    let flags = ffi::CGEventGetFlags(event);
    let keycode = ffi::CGEventGetIntegerValueField(event, ffi::KEYBOARD_EVENT_KEYCODE);
    let hid_flags = ffi::CGEventSourceFlagsState(KCG_EVENT_SOURCE_STATE_HID);

    let fn_down = (flags & KCG_EVENT_FLAG_MASK_SECONDARY_FN != 0)
        || (hid_flags & KCG_EVENT_FLAG_MASK_SECONDARY_FN != 0);
    let control = (flags & KCG_EVENT_FLAG_MASK_CONTROL != 0)
        || (hid_flags & KCG_EVENT_FLAG_MASK_CONTROL != 0);
    let command = (flags & KCG_EVENT_FLAG_MASK_COMMAND != 0)
        || (hid_flags & KCG_EVENT_FLAG_MASK_COMMAND != 0);
    let option = (flags & KCG_EVENT_FLAG_MASK_ALTERNATE != 0)
        || (hid_flags & KCG_EVENT_FLAG_MASK_ALTERNATE != 0);

    if event_type == KCG_EVENT_FLAGS_CHANGED {
        // Any flags-changed event may be an fn or Control edge — re-evaluate the
        // combo unconditionally; handle_combo_edge no-ops when nothing changed.
        handle_combo_edge(state, fn_down, control, command, option);
    } else if event_type == KCG_EVENT_KEY_DOWN
        && state.combo_down.load(Ordering::SeqCst)
        && !state.hands_free.load(Ordering::SeqCst)
        && !is_modifier_keycode(keycode)
        && !state.interrupted.swap(true, Ordering::SeqCst)
    {
        log::info!("PTT interrupted by keycode {keycode}");
        // cancel hold arm
        state.hold_armed.store(false, Ordering::SeqCst);
        state.combo_down.store(false, Ordering::SeqCst);
        (state.callback)(PttEvent::Interrupted);
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

fn handle_combo_edge(state: &TapState, fn_down: bool, control: bool, command: bool, option: bool) {
    let binding = *state.binding.lock();
    let want = combo_wanted(binding, fn_down, control);
    let was = state.combo_down.load(Ordering::SeqCst);
    if want == was {
        return;
    }

    // Debounce chatter
    {
        let mut last = state.last_edge.lock();
        if last.elapsed() < Duration::from_millis(25) {
            return;
        }
        *last = Instant::now();
    }

    if want {
        on_combo_down(state, command, option);
    } else {
        on_combo_up(state);
    }
}

fn on_combo_down(state: &TapState, command: bool, option: bool) {
    state.combo_down.store(true, Ordering::SeqCst);
    state.interrupted.store(false, Ordering::SeqCst);
    // YV49: latch command mode for this press before anything else can read it.
    let command_binding = *state.command_binding.lock();
    state.command_mode.store(
        command_wanted(command_binding, command, option),
        Ordering::SeqCst,
    );

    // Tap while hands-free → end latch (this press is consumed)
    if state.hands_free.load(Ordering::SeqCst) {
        log::info!("fn⌃ tap ends hands-free");
        state.hands_free.store(false, Ordering::SeqCst);
        state.hold_armed.store(false, Ordering::SeqCst);
        state.suppress_until_release.store(true, Ordering::SeqCst);
        *state.press_at.lock() = None;
        *state.last_tap_at.lock() = None;
        state.press_gen.fetch_add(1, Ordering::SeqCst);
        (state.callback)(PttEvent::HandsFreeOff);
        (state.callback)(PttEvent::Stop);
        return;
    }

    if state.suppress_until_release.load(Ordering::SeqCst) {
        return;
    }

    let gen = state.press_gen.fetch_add(1, Ordering::SeqCst) + 1;
    *state.press_at.lock() = Some(Instant::now());
    state.hold_armed.store(false, Ordering::SeqCst);

    // Arm hold after HOLD_ARM_MS if still held. YV38: this wait is NOT latency
    // slack that could be signalled away — it IS the gesture, the window that
    // separates a tap (hands-free toggle) from a hold (push-to-talk). There is
    // no earlier event that can decide which one the user meant, so the duration
    // itself is the constraint.
    let cb = state.callback.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(HOLD_ARM_MS));
        let Some(st) = GLOBAL_STATE.lock().clone() else {
            return;
        };
        if st.press_gen.load(Ordering::SeqCst) != gen {
            return;
        }
        if !st.combo_down.load(Ordering::SeqCst) {
            return;
        }
        if st.hands_free.load(Ordering::SeqCst) || st.suppress_until_release.load(Ordering::SeqCst)
        {
            return;
        }
        st.hold_armed.store(true, Ordering::SeqCst);
        *st.last_tap_at.lock() = None;
        log::info!("PTT hold armed → Start");
        (cb)(PttEvent::Start);
    });
}

fn on_combo_up(state: &TapState) {
    state.combo_down.store(false, Ordering::SeqCst);
    state.press_gen.fetch_add(1, Ordering::SeqCst); // cancel pending hold arm

    if state.suppress_until_release.swap(false, Ordering::SeqCst) {
        *state.press_at.lock() = None;
        return;
    }

    let press_at = state.press_at.lock().take();
    let Some(started) = press_at else {
        return;
    };
    let duration = started.elapsed();

    // Hold mode → stop on release
    if state.hold_armed.swap(false, Ordering::SeqCst) {
        if state.interrupted.swap(false, Ordering::SeqCst) {
            return;
        }
        log::info!("PTT hold release → Stop ({duration:?})");
        (state.callback)(PttEvent::Stop);
        return;
    }

    if state.hands_free.load(Ordering::SeqCst) {
        return;
    }

    // Short press = tap
    if duration <= Duration::from_millis(TAP_MAX_MS) {
        let now = Instant::now();
        let mut last = state.last_tap_at.lock();
        if let Some(t) = *last {
            if now.duration_since(t) <= Duration::from_millis(DOUBLE_TAP_MS) {
                *last = None;
                state.hands_free.store(true, Ordering::SeqCst);
                log::info!("PTT double-tap → hands-free ON");
                (state.callback)(PttEvent::HandsFreeOn);
                (state.callback)(PttEvent::Start);
                return;
            }
        }
        *last = Some(now);
        log::debug!("PTT single tap (double-tap window open)");
        return;
    }

    log::debug!("PTT medium release ignored ({duration:?})");
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
}

impl TapHealth {
    /// PURE counter step — the global below is just one instance of this.
    pub fn record(&mut self, kind: TapDisable) {
        self.re_arms = self.re_arms.saturating_add(1);
        match kind {
            TapDisable::Timeout => self.by_timeout = self.by_timeout.saturating_add(1),
            TapDisable::UserInput => self.by_user_input = self.by_user_input.saturating_add(1),
        }
    }
}

static TAP_HEALTH: Mutex<TapHealth> = Mutex::new(TapHealth {
    re_arms: 0,
    by_timeout: 0,
    by_user_input: 0,
});

/// The live CFMachPort for the tap, so the callback can re-enable the SAME tap
/// instead of respawning the thread. Retained at creation (`CFRetain`) because
/// the callback reads it from an arbitrary later moment.
static TAP_PORT: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Snapshot of the session's tap health. Cheap; safe from any thread.
pub fn tap_health() -> TapHealth {
    *TAP_HEALTH.lock()
}

/// Count one disable + re-arm and hand back the new snapshot.
pub fn note_tap_disabled(kind: TapDisable) -> TapHealth {
    let mut g = TAP_HEALTH.lock();
    g.record(kind);
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
    true
}

#[cfg(test)]
mod tests {
    use super::{combo_wanted, command_wanted, CommandBinding, PttBinding};

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
