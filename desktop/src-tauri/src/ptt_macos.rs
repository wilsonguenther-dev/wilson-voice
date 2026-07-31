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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const KCG_EVENT_FLAG_MASK_SECONDARY_FN: u64 = 0x0080_0000;
const KCG_EVENT_FLAG_MASK_CONTROL: u64 = 0x0004_0000;

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
        pub static kCFRunLoopCommonModes: CFStringRef;
    }

    pub const KEYBOARD_EVENT_KEYCODE: u32 = 9;
}

pub fn start(binding: PttBinding, callback: Callback) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        if let Some(s) = GLOBAL_STATE.lock().as_ref() {
            *s.binding.lock() = binding;
        }
        return;
    }

    let state = Arc::new(TapState {
        binding: Mutex::new(binding),
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
        log::error!("failed to spawn fn PTT thread: {e}; PTT hotkey disabled (tray / ⌘⇧V still work)");
        RUNNING.store(false, Ordering::SeqCst);
    }
}

pub fn set_binding(binding: PttBinding) {
    if let Some(s) = GLOBAL_STATE.lock().as_ref() {
        *s.binding.lock() = binding;
        log::info!("PTT binding → {:?}", binding);
    }
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

    if event_type == KCG_EVENT_FLAGS_CHANGED {
        // Any flags-changed event may be an fn or Control edge — re-evaluate the
        // combo unconditionally; handle_combo_edge no-ops when nothing changed.
        handle_combo_edge(state, fn_down, control);
    } else if event_type == KCG_EVENT_KEY_DOWN {
        if state.combo_down.load(Ordering::SeqCst)
            && !state.hands_free.load(Ordering::SeqCst)
            && !is_modifier_keycode(keycode)
        {
            if !state.interrupted.swap(true, Ordering::SeqCst) {
                log::info!("PTT interrupted by keycode {keycode}");
                // cancel hold arm
                state.hold_armed.store(false, Ordering::SeqCst);
                state.combo_down.store(false, Ordering::SeqCst);
                (state.callback)(PttEvent::Interrupted);
            }
        }
    }

    event
}

fn is_modifier_keycode(code: i64) -> bool {
    matches!(code, 54 | 55 | 56 | 57 | 58 | 59 | 60 | 61 | 62 | 63)
}

fn combo_wanted(binding: PttBinding, fn_down: bool, control: bool) -> bool {
    match binding {
        PttBinding::Fn => fn_down,
        PttBinding::FnControl => fn_down && control,
        PttBinding::FnOrFnControl => fn_down,
    }
}

fn handle_combo_edge(state: &TapState, fn_down: bool, control: bool) {
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
        on_combo_down(state);
    } else {
        on_combo_up(state);
    }
}

fn on_combo_down(state: &TapState) {
    state.combo_down.store(true, Ordering::SeqCst);
    state.interrupted.store(false, Ordering::SeqCst);

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
