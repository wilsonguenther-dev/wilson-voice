//! macOS push-to-talk via FN / Globe and FN+Control.
//!
//! Carbon `RegisterEventHotKey` (tauri-plugin-global-shortcut) cannot bind bare FN.
//! Industry pattern (open-wispr / OpenWhispr / VoiceInk ideas, clean-room):
//!   CGEvent tap → FlagsChanged → keycode 63 + SecondaryFn flag edge detect.
//!
//! Accessibility is required for the tap. User should set
//! System Settings → Keyboard → “Press 🌐 key to” → **Do Nothing**.

#![cfg(target_os = "macos")]

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// kVK_Function / Globe
const KVK_FUNCTION: i64 = 63;
const KCG_EVENT_FLAG_MASK_SECONDARY_FN: u64 = 0x0080_0000;
const KCG_EVENT_FLAG_MASK_CONTROL: u64 = 0x0004_0000;
const KCG_EVENT_FLAG_MASK_COMMAND: u64 = 0x0010_0000;
const KCG_EVENT_FLAG_MASK_SHIFT: u64 = 0x0002_0000;
const KCG_EVENT_FLAG_MASK_ALTERNATE: u64 = 0x0008_0000;

const KCG_EVENT_FLAGS_CHANGED: u32 = 12;
const KCG_EVENT_KEY_DOWN: u32 = 10;
const KCG_EVENT_KEY_UP: u32 = 11;
const KCG_HID_EVENT_TAP: u32 = 0; // kCGHIDEventTap
const KCG_HEAD_INSERT: u32 = 0;
const KCG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;
const KCG_EVENT_SOURCE_STATE_HID: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttBinding {
    /// Hold bare FN / Globe (Wispr-style). Default.
    Fn,
    /// Hold FN+Control (less conflict with system Globe actions).
    FnControl,
    /// Either bare FN **or** FN+Control counts as hold.
    FnOrFnControl,
}

impl PttBinding {
    pub fn from_settings(s: &str) -> Self {
        match s {
            "fn_control" | "fn+control" | "fnctrl" => Self::FnControl,
            "fn_or_fn_control" | "both" => Self::FnOrFnControl,
            _ => Self::Fn,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fn => "fn hold",
            Self::FnControl => "fn⌃ hold",
            Self::FnOrFnControl => "fn / fn⌃ hold",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PttEvent {
    Down,
    Up,
    Interrupted,
}

type Callback = Arc<dyn Fn(PttEvent) + Send + Sync + 'static>;

struct TapState {
    binding: Mutex<PttBinding>,
    callback: Callback,
    is_down: AtomicBool,
    interrupted: AtomicBool,
    last_edge: Mutex<Instant>,
}

static RUNNING: AtomicBool = AtomicBool::new(false);

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

    /// kCGKeyboardEventKeycode
    pub const KEYBOARD_EVENT_KEYCODE: u32 = 9;
}

/// Start FN PTT listener on a background CFRunLoop thread. Idempotent.
pub fn start(binding: PttBinding, callback: Callback) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        // Already running — just update binding
        if let Some(s) = GLOBAL_STATE.lock().as_ref() {
            *s.binding.lock() = binding;
        }
        return;
    }

    let state = Arc::new(TapState {
        binding: Mutex::new(binding),
        callback,
        is_down: AtomicBool::new(false),
        interrupted: AtomicBool::new(false),
        last_edge: Mutex::new(Instant::now() - Duration::from_secs(1)),
    });
    *GLOBAL_STATE.lock() = Some(state.clone());

    thread::Builder::new()
        .name("wv-fn-ptt".into())
        .spawn(move || run_tap(state))
        .expect("spawn fn ptt thread");
}

/// Update binding live without restarting the tap.
pub fn set_binding(binding: PttBinding) {
    if let Some(s) = GLOBAL_STATE.lock().as_ref() {
        *s.binding.lock() = binding;
        log::info!("PTT binding → {:?}", binding);
    }
}

static GLOBAL_STATE: Mutex<Option<Arc<TapState>>> = Mutex::new(None);

fn run_tap(state: Arc<TapState>) {
    // flagsChanged | keyDown | keyUp
    let mask = (1u64 << KCG_EVENT_FLAGS_CHANGED)
        | (1u64 << KCG_EVENT_KEY_DOWN)
        | (1u64 << KCG_EVENT_KEY_UP);

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
            log::error!(
                "FN PTT: CGEventTapCreate failed — enable Accessibility for Wilson Voice"
            );
            RUNNING.store(false, Ordering::SeqCst);
            // reclaim arc
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
        log::info!("FN PTT event tap running (keycode 63 / SecondaryFn)");
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

    // HID system flags for FN cross-check (ignore false release pulses)
    let hid_flags = ffi::CGEventSourceFlagsState(KCG_EVENT_SOURCE_STATE_HID);
    let fn_from_event = flags & KCG_EVENT_FLAG_MASK_SECONDARY_FN != 0;
    let fn_from_hid = hid_flags & KCG_EVENT_FLAG_MASK_SECONDARY_FN != 0;
    let control = flags & KCG_EVENT_FLAG_MASK_CONTROL != 0
        || hid_flags & KCG_EVENT_FLAG_MASK_CONTROL != 0;

    if event_type == KCG_EVENT_FLAGS_CHANGED {
        // Only update FN state from actual FN keycode events
        if keycode == KVK_FUNCTION {
            let fn_down = fn_from_event || fn_from_hid;
            handle_fn_edge(state, fn_down, control);
        } else {
            // Control released while FN mode required control — re-evaluate
            let fn_down = fn_from_event || fn_from_hid;
            handle_fn_edge(state, fn_down, control);
        }
    } else if event_type == KCG_EVENT_KEY_DOWN {
        // Non-modifier key while holding → interrupt (Fn+Arrow etc.)
        if state.is_down.load(Ordering::SeqCst) && !is_modifier_keycode(keycode) {
            if !state.interrupted.swap(true, Ordering::SeqCst) {
                log::info!("PTT interrupted by keycode {keycode}");
                (state.callback)(PttEvent::Interrupted);
            }
        }
    }

    event
}

fn is_modifier_keycode(code: i64) -> bool {
    matches!(code, 54 | 55 | 56 | 57 | 58 | 59 | 60 | 61 | 62 | 63)
}

fn handle_fn_edge(state: &TapState, fn_down: bool, control: bool) {
    let binding = *state.binding.lock();
    let want = match binding {
        PttBinding::Fn => fn_down,
        PttBinding::FnControl => fn_down && control,
        PttBinding::FnOrFnControl => fn_down, // bare FN enough; Control optional
    };

    let was = state.is_down.load(Ordering::SeqCst);
    if want == was {
        return;
    }

    // Debounce double-edges (~30ms)
    {
        let mut last = state.last_edge.lock();
        if last.elapsed() < Duration::from_millis(30) {
            return;
        }
        *last = Instant::now();
    }

    if want {
        state.is_down.store(true, Ordering::SeqCst);
        state.interrupted.store(false, Ordering::SeqCst);
        log::info!("PTT down ({binding:?})");
        (state.callback)(PttEvent::Down);
    } else {
        state.is_down.store(false, Ordering::SeqCst);
        let interrupted = state.interrupted.swap(false, Ordering::SeqCst);
        if interrupted {
            log::info!("PTT up after interrupt — discard");
            // No Up — already interrupted (caller cancelled)
        } else {
            log::info!("PTT up ({binding:?})");
            (state.callback)(PttEvent::Up);
        }
    }
}

// silence unused constant warnings if not referenced elsewhere
#[allow(dead_code)]
const _MASKS: [u64; 4] = [
    KCG_EVENT_FLAG_MASK_COMMAND,
    KCG_EVENT_FLAG_MASK_SHIFT,
    KCG_EVENT_FLAG_MASK_ALTERNATE,
    KCG_EVENT_FLAG_MASK_CONTROL,
];
