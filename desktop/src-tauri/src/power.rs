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
//! are matrix row #16's `NSWorkspaceWillSleepNotification` path: this assertion
//! PAIRS WITH that handling, it does not replace it.
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
