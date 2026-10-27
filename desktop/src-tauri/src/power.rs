//! YV91 — the power assertion that keeps a meeting recording while the Mac
//! sits idle (plan finding OS-1, matrix row #16).
//!
//! A student starts a recording and does not touch the keyboard for ninety
//! minutes. With default energy settings the Mac idle-sleeps, the audio HAL
//! stops, and the recording ends without anybody being told. The fix is one
//! IOKit assertion held for exactly as long as capture runs.
//!
//! **The assertion type is the whole design decision.** We take
//! `kIOPMAssertionTypePreventUserIdleSystemSleep` — "do not idle-sleep the
//! SYSTEM" — and deliberately NOT
//! `kIOPMAssertionTypePreventUserIdleDisplaySleep`. Keeping the display awake
//! for three hours is a battery bug wearing a feature's clothes: the panel
//! called this out explicitly, so [`ASSERTION_TYPE`] is asserted by a test that
//! fails if anybody ever swaps in the display variant.
//!
//! It also does not cover lid close or an explicit Sleep — nothing can. Those
//! are matrix row #16's sleep/wake path, which Y1-B registers further down this
//! file ([`SleepWakeObserver`], fanned out through [`SleepWakeHub`]): this
//! assertion PAIRS WITH that handling, it does not replace it. yap24-NT10 adds
//! the IOKit half of that path ([`on_power_message`]): `IORegisterForSystemPower`
//! is what lets Yap HOLD a sleep it cannot prevent until the meeting journal is
//! finalized, acknowledging with `IOAllowPowerChange` only afterwards.
//!
//! The assertion is released by `Drop`, so every exit path out of a meeting —
//! normal stop, watchdog stop, an error return, a panic unwinding through the
//! session — puts the Mac's idle timer back exactly the way it found it. That
//! is the same never-leave-the-system-changed rule `sysaudio::restore` keeps
//! for the output device.

/// The IOKit assertion type Yap takes while a meeting is recording. Prevents
/// **system** idle sleep only — the display still sleeps normally.
pub const ASSERTION_TYPE: &str = "PreventUserIdleSystemSleep";

/// The assertion type Yap must NEVER take. Present as a named constant so the
/// test that forbids it reads as a rule rather than as a string literal.
pub const FORBIDDEN_ASSERTION_TYPE: &str = "PreventUserIdleDisplaySleep";

/// `kIOPMAssertionLevelOn`.
const ASSERTION_LEVEL_ON: u32 = 255;

/// Human-readable reason, shown to the user in Activity Monitor's Energy tab
/// and in `pmset -g assertions`. Naming the app AND the reason is the polite
/// posture: a user who wonders why their Mac is awake gets an answer.
pub fn assertion_reason(what: &str) -> String {
    format!("Yap is recording {what}")
}

/// Is this assertion type one we are allowed to take? Pure, so the "never the
/// display variant" rule is testable without IOKit.
pub fn is_allowed_assertion_type(kind: &str) -> bool {
    kind == ASSERTION_TYPE
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{assertion_reason, ASSERTION_LEVEL_ON, ASSERTION_TYPE};
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};

    type IOPMAssertionID = u32;
    type IOReturn = i32;
    const K_IO_RETURN_SUCCESS: IOReturn = 0;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            assertion_level: u32,
            assertion_name: CFStringRef,
            assertion_id: *mut IOPMAssertionID,
        ) -> IOReturn;
        fn IOPMAssertionRelease(assertion_id: IOPMAssertionID) -> IOReturn;
    }

    /// A held power assertion. Released on drop — see the module docs for why
    /// that matters more than the take does.
    pub struct PowerAssertion {
        id: IOPMAssertionID,
    }

    impl PowerAssertion {
        /// Take the idle-sleep assertion for `what` (e.g. `"a meeting"`).
        /// `None` when IOKit refuses — capture still runs, it just isn't
        /// protected from idle sleep, which is strictly what happens today.
        pub fn prevent_idle_sleep(what: &str) -> Option<Self> {
            let kind = CFString::new(ASSERTION_TYPE);
            let name = CFString::new(&assertion_reason(what));
            let mut id: IOPMAssertionID = 0;
            // SAFETY: both CFStrings outlive the call; `id` is a valid out-param.
            let status = unsafe {
                IOPMAssertionCreateWithName(
                    kind.as_concrete_TypeRef(),
                    ASSERTION_LEVEL_ON,
                    name.as_concrete_TypeRef(),
                    &mut id,
                )
            };
            if status != K_IO_RETURN_SUCCESS {
                log::warn!("power assertion refused (IOReturn {status}) — idle sleep may end this recording");
                return None;
            }
            log::info!("power assertion held: {ASSERTION_TYPE} for {what}");
            Some(Self { id })
        }

        /// The live assertion id, for logging/tests. Never zero for a held
        /// assertion.
        pub fn id(&self) -> u32 {
            self.id
        }
    }

    impl Drop for PowerAssertion {
        fn drop(&mut self) {
            // SAFETY: `id` came from a successful create and is released once —
            // `Drop` runs at most once per value.
            let status = unsafe { IOPMAssertionRelease(self.id) };
            if status != K_IO_RETURN_SUCCESS {
                log::warn!("power assertion release returned {status}");
            } else {
                log::info!("power assertion released");
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    /// Non-macOS stand-in: there is no assertion to take, and capture behaves
    /// exactly as it does today.
    pub struct PowerAssertion;

    impl PowerAssertion {
        pub fn prevent_idle_sleep(_what: &str) -> Option<Self> {
            None
        }

        pub fn id(&self) -> u32 {
            0
        }
    }
}

pub use imp::PowerAssertion;

// ── Y1-B · the ONE sleep/wake publisher ─────────────────────────────────────
//
// Until this item, nothing in Yap registered for sleep or wake. PERM-E's spec
// and Y6-D's both said "power.rs already observes sleep/wake — reuse it", and
// it did not: everything above this line is IOPMAssertion create/release. The
// module docs named `NSWorkspaceWillSleepNotification` in a comment, which is
// why `meeting_matrix` row 16 was published as `PolicyOnly`.
//
// The shape is one publisher, three subscribers:
//
//   NSWorkspace.sharedWorkspace.notificationCenter
//       NSWorkspaceWillSleepNotification ─┐
//       NSWorkspaceDidWakeNotification ───┴─> SleepWakeObserver (2 block tokens)
//                                               └─> SleepWakeHub::publish(SleepEvent)
//                                                     ├─ "dictation"   — park a take in flight (Y6-D)
//                                                     ├─ "meetings"    — row 16's SleepPolicy (finalize / offer resume)
//                                                     └─ "permissions" — PERM-E's re-read on wake
//
// This is the only file in `src/` that registers either notification. A second
// observer anywhere else is a defect, and `tests/matrix_row16_sleep_wake.rs`
// fails if one appears.

pub use crate::meeting_matrix::SleepEvent;

/// The dictation path: a take in flight when the lid closes (Y6-D).
pub const CONSUMER_DICTATION: &str = "dictation";
/// Meeting capture: row 16's [`crate::meeting_matrix::SleepPolicy`].
pub const CONSUMER_MEETINGS: &str = "meetings";
/// PERM-E's watcher: a wake is when a TCC grant or an audio device most often
/// changed under the user.
pub const CONSUMER_PERMISSIONS: &str = "permissions";
/// Every consumer the app subscribes, in subscription order.
pub const SLEEP_WAKE_CONSUMERS: [&str; 3] =
    [CONSUMER_DICTATION, CONSUMER_MEETINGS, CONSUMER_PERMISSIONS];

/// Should the dictation path park the take in flight? Only on will-sleep, and
/// only while a take is actually recording. A wake changes nothing for
/// dictation: the take was already parked on the way down, and starting a new
/// one is the user's call.
pub fn dictation_should_park_take(event: SleepEvent, take_in_flight: bool) -> bool {
    event == SleepEvent::WillSleep && take_in_flight
}

// ── yap24-NT10 · the IOKit sleep HOLD ───────────────────────────────────────
//
// `NSWorkspaceWillSleepNotification` is a courtesy: macOS does not wait for its
// observers, so a meeting finalizing there races the machine going down. The
// root power domain DOES wait. A client registered with
// `IORegisterForSystemPower` receives `kIOMessageSystemWillSleep` before any
// hardware powers off, and the sleep does not proceed until every client has
// called `IOAllowPowerChange` — or 30 s pass (IOPMLib.h, read from the SDK
// header for this item). So the order is the whole design: publish
// `WillSleep` to the hub (the meeting consumer stops the capture, closes the
// journal, writes the row `partial`, enqueues it), THEN acknowledge.
//
// The NSWorkspace observer above is kept (PR #239's registration, the one hub):
// both deliver `WillSleep` for one sleep and every consumer is idempotent on
// the second — `SleepPolicy` ignores a duplicate, dictation has no take left
// to park, permissions does nothing on sleep. Wake stays NSWorkspace-only:
// IOKit's wake messages must NOT be acknowledged and add nothing.

/// `kIOMessageCanSystemSleep` — `iokit_common_msg(0x270)`. Idle sleep is being
/// considered; must be acknowledged (allow) or cancelled (veto).
pub const IO_MESSAGE_CAN_SYSTEM_SLEEP: u32 = 0xE000_0270;
/// `kIOMessageSystemWillSleep` — `iokit_common_msg(0x280)`. A non-abortable
/// sleep (lid close, Apple menu Sleep, an idle sleep nobody vetoed). MUST be
/// acknowledged with `IOAllowPowerChange`.
pub const IO_MESSAGE_SYSTEM_WILL_SLEEP: u32 = 0xE000_0280;
/// `kIOMessageSystemWillNotSleep` — `iokit_common_msg(0x290)`. Never acknowledged.
pub const IO_MESSAGE_SYSTEM_WILL_NOT_SLEEP: u32 = 0xE000_0290;
/// `kIOMessageSystemHasPoweredOn` — `iokit_common_msg(0x300)`. Never acknowledged.
pub const IO_MESSAGE_SYSTEM_HAS_POWERED_ON: u32 = 0xE000_0300;
/// `kIOMessageSystemWillPowerOn` — `iokit_common_msg(0x320)`. Never acknowledged.
pub const IO_MESSAGE_SYSTEM_WILL_POWER_ON: u32 = 0xE000_0320;

/// What [`on_power_message`] did about one root-domain message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerReply {
    /// `IOAllowPowerChange` was called (after the hub, for a will-sleep).
    Acknowledged,
    /// A message IOPMLib.h says must NOT be acknowledged; nothing was sent.
    NotAcknowledged,
}

/// The ONE handler of an IOKit root-domain power message. The C callback
/// registered by [`SleepWakeObserver`] calls exactly this, and
/// `tests/meeting_survives_sleep.rs` injects `kIOMessageSystemWillSleep`
/// through it with a recording `acknowledge`.
///
/// - will-sleep: every hub consumer runs to completion FIRST (the meeting is
///   finalized and its row written), THEN `acknowledge` releases the sleep.
/// - can-sleep (idle): acknowledged straight away. Yap does not veto idle sleep
///   here; keeping a meeting awake through idle is [`PowerAssertion`]'s job, and
///   if that assertion was refused the will-sleep that follows finalizes the
///   meeting like any other sleep.
/// - everything else: not acknowledged, per IOPMLib.h.
pub fn on_power_message(
    hub: &SleepWakeHub,
    message_type: u32,
    acknowledge: impl FnOnce(),
) -> PowerReply {
    match message_type {
        IO_MESSAGE_SYSTEM_WILL_SLEEP => {
            hub.publish(SleepEvent::WillSleep);
            acknowledge();
            PowerReply::Acknowledged
        }
        IO_MESSAGE_CAN_SYSTEM_SLEEP => {
            acknowledge();
            PowerReply::Acknowledged
        }
        _ => PowerReply::NotAcknowledged,
    }
}

#[cfg(target_os = "macos")]
mod hold {
    use super::{on_power_message, SleepWakeHub};
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    type IoConnect = u32;
    type IoObject = u32;
    type IoReturn = i32;
    type NotificationPort = *mut c_void;
    type RunLoopSource = *mut c_void;
    type RunLoop = *mut c_void;
    type CfString = *const c_void;
    type InterestCallback = extern "C" fn(*mut c_void, IoObject, u32, *mut c_void);

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IORegisterForSystemPower(
            refcon: *mut c_void,
            the_port_ref: *mut NotificationPort,
            callback: InterestCallback,
            notifier: *mut IoObject,
        ) -> IoConnect;
        fn IODeregisterForSystemPower(notifier: *mut IoObject) -> IoReturn;
        fn IOAllowPowerChange(kernel_port: IoConnect, notification_id: isize) -> IoReturn;
        fn IONotificationPortGetRunLoopSource(notify: NotificationPort) -> RunLoopSource;
        fn IONotificationPortDestroy(notify: NotificationPort);
        fn IOServiceClose(connect: IoConnect) -> IoReturn;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRunLoopGetMain() -> RunLoop;
        fn CFRunLoopAddSource(rl: RunLoop, source: RunLoopSource, mode: CfString);
        fn CFRunLoopRemoveSource(rl: RunLoop, source: RunLoopSource, mode: CfString);
        static kCFRunLoopCommonModes: CfString;
    }

    /// What the C callback needs: the hub to publish to and the root-domain
    /// connection to acknowledge on. Boxed so its address is stable for the
    /// `refcon`, freed only after the registration is gone.
    struct Ctx {
        hub: Arc<SleepWakeHub>,
        root_port: AtomicU32,
    }

    extern "C" fn power_callback(
        refcon: *mut c_void,
        _service: IoObject,
        message_type: u32,
        message_argument: *mut c_void,
    ) {
        if refcon.is_null() {
            return;
        }
        // SAFETY: `refcon` is the `Ctx` box `PowerHold::register` leaked; it is
        // freed in `Drop` only after the run-loop source is removed and the
        // registration deregistered, both on the thread that delivers this.
        let ctx = unsafe { &*(refcon as *const Ctx) };
        let port = ctx.root_port.load(Ordering::Acquire);
        // The notification id rides in `messageArgument` (IOPMLib.h).
        let id = message_argument as isize;
        // An unwinding panic across `extern "C"` aborts the process; the hub
        // already catches consumer panics, this catches the rest.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            on_power_message(&ctx.hub, message_type, || {
                // SAFETY: `port` is the live root-domain connection and `id`
                // the notification id IOKit just handed us.
                let r = unsafe { IOAllowPowerChange(port, id) };
                if r != 0 {
                    log::warn!("IOAllowPowerChange refused (IOReturn {r:#x})");
                } else {
                    log::info!("sleep acknowledged (message {message_type:#x})");
                }
            })
        }));
        if outcome.is_err() {
            log::error!("power message {message_type:#x} handler panicked");
        }
    }

    /// A live `IORegisterForSystemPower` registration whose notifications are
    /// delivered on the MAIN run loop — the thread the NSWorkspace observer and
    /// every hub consumer already run on.
    pub struct PowerHold {
        ctx: *mut Ctx,
        port: NotificationPort,
        notifier: IoObject,
        root_port: IoConnect,
        source: RunLoopSource,
    }

    // SAFETY: the raw handles are only touched in `register` and `Drop`; the
    // IOKit and CFRunLoop calls made with them are documented thread-safe, and
    // the callback state is behind `Arc` + atomics.
    unsafe impl Send for PowerHold {}

    impl PowerHold {
        /// Register with the root power domain. `None` (logged) when IOKit
        /// refuses — the NSWorkspace observer still finalizes, it just cannot
        /// hold the sleep while it does.
        pub fn register(hub: Arc<SleepWakeHub>) -> Option<Self> {
            let ctx = Box::into_raw(Box::new(Ctx {
                hub,
                root_port: AtomicU32::new(0),
            }));
            let mut port: NotificationPort = std::ptr::null_mut();
            let mut notifier: IoObject = 0;
            // SAFETY: valid out-params; `ctx` outlives the registration (Drop).
            let root_port = unsafe {
                IORegisterForSystemPower(ctx.cast(), &mut port, power_callback, &mut notifier)
            };
            if root_port == 0 || port.is_null() {
                log::warn!("IORegisterForSystemPower refused — sleep cannot be held for a meeting");
                // SAFETY: never handed to a live registration.
                drop(unsafe { Box::from_raw(ctx) });
                return None;
            }
            // SAFETY: `ctx` is live; set before the source can deliver anything.
            unsafe { (*ctx).root_port.store(root_port, Ordering::Release) };
            // SAFETY: `port` is the notification port IOKit just allocated.
            let source = unsafe { IONotificationPortGetRunLoopSource(port) };
            // SAFETY: CFRunLoop is thread-safe; the source is owned by `port`.
            unsafe { CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes) };
            log::info!("sleep hold registered (IORegisterForSystemPower)");
            Some(Self {
                ctx,
                port,
                notifier,
                root_port,
                source,
            })
        }
    }

    impl Drop for PowerHold {
        fn drop(&mut self) {
            // SAFETY: each handle came from a successful `register` and is torn
            // down exactly once, in the order IOPMLib.h prescribes (deregister,
            // then destroy the port, then close the connection). The context is
            // freed last, when nothing can call back into it.
            unsafe {
                CFRunLoopRemoveSource(CFRunLoopGetMain(), self.source, kCFRunLoopCommonModes);
                IODeregisterForSystemPower(&mut self.notifier);
                IONotificationPortDestroy(self.port);
                IOServiceClose(self.root_port);
                drop(Box::from_raw(self.ctx));
            }
            log::info!("sleep hold unregistered");
        }
    }
}

type Subscriber = Box<dyn Fn(SleepEvent) + Send + Sync + 'static>;

/// The fan-out. The observer publishes ONE event here and every subscriber sees
/// it, synchronously, on the posting (main) thread — which is the thread macOS
/// holds sleep for while will-sleep observers run, so a meeting that finalizes
/// here finalizes before the machine goes down.
#[derive(Default)]
pub struct SleepWakeHub {
    subscribers: std::sync::Mutex<Vec<(&'static str, Subscriber)>>,
}

impl SleepWakeHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a named consumer. One subscriber per name: a second `subscribe` with
    /// a name already present is ignored (and logged) rather than doubling
    /// every action that consumer takes. Must not be called from inside a
    /// subscriber.
    pub fn subscribe<F>(&self, name: &'static str, f: F)
    where
        F: Fn(SleepEvent) + Send + Sync + 'static,
    {
        let mut subs = self.subscribers.lock().unwrap_or_else(|p| p.into_inner());
        if subs.iter().any(|(n, _)| *n == name) {
            log::warn!("sleep/wake consumer {name:?} subscribed twice — second ignored");
            return;
        }
        subs.push((name, Box::new(f)));
    }

    /// The subscribed consumers, in subscription order.
    pub fn consumers(&self) -> Vec<&'static str> {
        let subs = self.subscribers.lock().unwrap_or_else(|p| p.into_inner());
        subs.iter().map(|(n, _)| *n).collect()
    }

    /// Deliver `event` to every consumer; returns how many ran to completion.
    ///
    /// A panicking consumer is caught and logged: this runs inside an
    /// Objective-C block, where an unwinding panic aborts the process, and one
    /// consumer's bug must not stop the other two from hearing the event.
    pub fn publish(&self, event: SleepEvent) -> usize {
        let subs = self.subscribers.lock().unwrap_or_else(|p| p.into_inner());
        let mut delivered = 0;
        for (name, f) in subs.iter() {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(event))) {
                Ok(()) => delivered += 1,
                Err(_) => log::error!("sleep/wake consumer {name:?} panicked on {event:?}"),
            }
        }
        log::info!(
            "sleep/wake: {event:?} delivered to {delivered}/{} consumers",
            subs.len()
        );
        delivered
    }
}

#[cfg(target_os = "macos")]
mod observer {
    use super::{SleepEvent, SleepWakeHub};
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
    use objc2_app_kit::{
        NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    use std::ptr::NonNull;
    use std::sync::Arc;

    /// The registered sleep/wake observer: two block tokens on the WORKSPACE
    /// notification centre (sleep/wake are never posted on the default centre).
    /// Removing both tokens is what [`SleepWakeObserver::unregister`] and `Drop`
    /// do — a block that outlives the hub it calls into is a crash on the next
    /// lid close.
    pub struct SleepWakeObserver {
        center: Retained<NSNotificationCenter>,
        tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
        /// yap24-NT10 — the IOKit registration that HOLDS a sleep until the hub
        /// has run. Same hub, same lifetime as the two tokens.
        hold: Option<super::hold::PowerHold>,
    }

    // SAFETY: the tokens are opaque observer objects that are only ever handed
    // back to `-[NSNotificationCenter removeObserver:]`, which Foundation
    // documents as safe from any thread; `NSNotificationCenter` itself is
    // `Send + Sync`. Nothing else is done with them.
    unsafe impl Send for SleepWakeObserver {}

    impl SleepWakeObserver {
        /// Register for `NSWorkspaceWillSleepNotification` and
        /// `NSWorkspaceDidWakeNotification`, each publishing its
        /// [`SleepEvent`] to `hub`. Delivered on the posting thread (macOS posts
        /// both on the main thread).
        pub fn register(hub: Arc<SleepWakeHub>) -> Self {
            let center = NSWorkspace::sharedWorkspace().notificationCenter();
            // SAFETY: framework-provided constant notification names.
            let names = unsafe {
                [
                    (NSWorkspaceWillSleepNotification, SleepEvent::WillSleep),
                    (NSWorkspaceDidWakeNotification, SleepEvent::DidWake),
                ]
            };
            let mut tokens = Vec::with_capacity(names.len());
            let hold = super::hold::PowerHold::register(Arc::clone(&hub));
            for (name, event) in names {
                let hub = Arc::clone(&hub);
                let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
                    hub.publish(event);
                });
                // SAFETY: a framework constant name, no sender filter, no queue
                // (delivered synchronously on the posting thread); the centre
                // copies the block, and the token is removed in `unregister`.
                let token = unsafe {
                    center.addObserverForName_object_queue_usingBlock(
                        Some(name),
                        None,
                        None,
                        &block,
                    )
                };
                tokens.push(token);
            }
            log::info!(
                "sleep/wake observer registered ({} notifications)",
                tokens.len()
            );
            Self {
                center,
                tokens,
                hold,
            }
        }

        /// How many notifications this observer still holds. 2 while live.
        pub fn registered(&self) -> usize {
            self.tokens.len()
        }

        /// Is the IOKit sleep hold (`IORegisterForSystemPower`) live?
        pub fn holds_sleep(&self) -> bool {
            self.hold.is_some()
        }

        /// Remove both registrations now.
        pub fn unregister(mut self) {
            self.remove_all();
        }

        fn remove_all(&mut self) {
            // The IOKit hold goes first: after this nothing can publish to the
            // hub from a power message.
            drop(self.hold.take());
            let n = self.tokens.len();
            for token in self.tokens.drain(..) {
                let obj: &AnyObject = token.as_ref();
                // SAFETY: `obj` is a token this centre returned and that has not
                // been removed yet (drained exactly once).
                unsafe { self.center.removeObserver(obj) };
            }
            if n > 0 {
                log::info!("sleep/wake observer unregistered ({n} notifications)");
            }
        }
    }

    impl Drop for SleepWakeObserver {
        fn drop(&mut self) {
            self.remove_all();
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod observer {
    use super::SleepWakeHub;
    use std::sync::Arc;

    /// Non-macOS stand-in: there is no workspace to observe.
    pub struct SleepWakeObserver;

    impl SleepWakeObserver {
        pub fn register(_hub: Arc<SleepWakeHub>) -> Self {
            Self
        }
        pub fn registered(&self) -> usize {
            0
        }
        pub fn holds_sleep(&self) -> bool {
            false
        }
        pub fn unregister(self) {}
    }
}

pub use observer::SleepWakeObserver;

/// The process's one observer, held from `setup` until `RunEvent::Exit`.
static OBSERVER: std::sync::Mutex<Option<SleepWakeObserver>> = std::sync::Mutex::new(None);

/// Register the process-wide sleep/wake observer feeding `hub`. Returns false
/// — and registers nothing — when one is already installed: there is exactly
/// one observer per process, never a second.
pub fn install_sleep_wake_observer(hub: std::sync::Arc<SleepWakeHub>) -> bool {
    let mut slot = OBSERVER.lock().unwrap_or_else(|p| p.into_inner());
    if slot.is_some() {
        log::warn!("sleep/wake observer already installed — not registering a second");
        return false;
    }
    *slot = Some(SleepWakeObserver::register(hub));
    true
}

/// Is the process-wide observer installed right now?
pub fn sleep_wake_observer_installed() -> bool {
    OBSERVER.lock().unwrap_or_else(|p| p.into_inner()).is_some()
}

/// Unregister the process-wide observer (called from `RunEvent::Exit`). Returns
/// true if one was removed. Idempotent.
pub fn unregister_sleep_wake_observer() -> bool {
    let taken = OBSERVER.lock().unwrap_or_else(|p| p.into_inner()).take();
    match taken {
        Some(observer) => {
            observer.unregister();
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The panel's line, as a test: the display-sleep variant is a battery bug,
    // not a feature. If anybody ever "improves" this by keeping the screen on,
    // this fails.
    #[test]
    fn meeting_capture_never_takes_the_display_sleep_assertion() {
        assert_eq!(ASSERTION_TYPE, "PreventUserIdleSystemSleep");
        assert!(is_allowed_assertion_type(ASSERTION_TYPE));
        assert!(!is_allowed_assertion_type(FORBIDDEN_ASSERTION_TYPE));
        assert!(
            !ASSERTION_TYPE.contains("Display"),
            "the display must still be allowed to sleep during a 3h meeting"
        );
    }

    #[test]
    fn the_reason_string_names_the_app_and_the_activity() {
        let reason = assertion_reason("a meeting");
        assert!(reason.starts_with("Yap "), "{reason}");
        assert!(reason.contains("a meeting"), "{reason}");
    }

    // Smoke: taking and releasing must not panic, and a taken assertion has a
    // real id. Not asserted to SUCCEED — a sandboxed CI runner is allowed to
    // refuse, and refusing is a warn-and-continue path by design.
    #[test]
    fn take_and_release_is_panic_free() {
        let held = PowerAssertion::prevent_idle_sleep("a meeting (test)");
        if let Some(a) = held.as_ref() {
            assert_ne!(a.id(), 0, "a held assertion has a real IOKit id");
        }
        drop(held);
    }
}
