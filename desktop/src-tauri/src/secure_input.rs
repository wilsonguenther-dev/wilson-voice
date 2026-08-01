//! YV43 — macOS Secure Input watchdog.
//!
//! When ANY process enables Secure Keyboard Entry (a password field taking
//! focus, Terminal's "Secure Keyboard Entry" menu item, a wedged
//! `loginwindow`), the WindowServer stops delivering key events to CGEventTaps.
//! Yap's whole push-to-talk gesture lives in one such tap (`ptt_macos`), so
//! fn / fn⌃ stops working **silently**: no error, no callback, no log — the app
//! keeps saying "Ready — hold fn⌃" while the key it is asking for can never
//! reach it. That is the single most confusing failure this app has.
//!
//! This module makes it loud instead:
//!   * poll `IsSecureEventInputEnabled()` (Carbon) on a slow timer
//!     ([`POLL_INTERVAL`]) — a flag read, never a hook,
//!   * run the samples through [`Watchdog`], a two-sample state machine so a
//!     password field that grabs focus for an instant does not flash a banner,
//!   * on the engaged edge: WARN with the holding pid/name when macOS exposes
//!     one, and surface [`BLOCKED_MESSAGE`] in the tray tooltip + main window,
//!   * on the cleared edge: drop the warning and go back to normal.
//!
//! **No Carbon fallback is possible for Yap's binding.** `RegisterEventHotKey`
//! (what `tauri-plugin-global-shortcut` uses, and what Handy shadow-registers
//! its keyed shortcuts through) is immune to Secure Input, but it cannot
//! express the fn/globe modifier at all — every Yap PTT binding
//! (`fn`, `fn⌃`, `fn / fn⌃`) contains it, so there is nothing to shadow. The
//! honest status indicator IS the deliverable; the detail line points the user
//! at the paths that DO keep working (the menu-bar Start Dictation item, and
//! ⌘⇧V when they have enabled it), because those are Carbon-backed.
//!
//! Culprit attribution is best-effort by design: Apple documents no supported
//! API for "who turned this on", so the IORegistry session property is read the
//! way every other tool does it, and the status is still correct when it comes
//! back empty.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How often the watchdog samples the flag. Deliberately slow — this is a
/// background health check, not a hot path, and each sample is one Carbon call.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Consecutive positive samples before the watchdog calls it blocked. Two
/// samples at [`POLL_INTERVAL`] means Secure Input has been held for at least
/// one full interval, which filters the momentary activation you get from
/// tabbing through a password field without hiding a genuinely stuck holder.
pub const SUSTAIN_SAMPLES: u32 = 2;

/// The exact user-facing line. Kept here (not in the UI) so the tray tooltip,
/// the status pill and the banner cannot drift apart.
pub const BLOCKED_MESSAGE: &str =
    "Dictation paused — another app is blocking keyboard monitoring (Secure Input)";

/// What the watchdog decided at one sample. `None` from [`Watchdog::observe`]
/// means "nothing changed" — the caller only does work on an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Secure Input has been held long enough to be reported: the fn tap is
    /// blind right now.
    Engaged,
    /// Secure Input was released; the fn tap sees keys again.
    Cleared,
}

/// Where the watchdog currently sits. `Pending` is the one-sample grace state
/// that keeps a transient password-field focus off the user's screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Clear,
    Pending,
    Blocked,
}

/// The poll → user-visible-state machine, kept free of any FFI or app handle so
/// the transitions are testable by feeding it samples (see the tests below).
#[derive(Debug, Default)]
pub struct Watchdog {
    phase: Phase,
    /// Consecutive `true` samples, reset by any `false`.
    positive: u32,
}

impl Watchdog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed ONE sample of `IsSecureEventInputEnabled()`. Returns `Some` only on
    /// the edges the UI cares about, so a caller can emit/log unconditionally on
    /// the result without re-checking whether anything actually changed.
    pub fn observe(&mut self, enabled: bool) -> Option<Transition> {
        if !enabled {
            self.positive = 0;
            let was_blocked = self.phase == Phase::Blocked;
            self.phase = Phase::Clear;
            return was_blocked.then_some(Transition::Cleared);
        }

        // Saturating: a holder that never lets go must not wrap the counter back
        // through the sustain threshold and re-fire `Engaged` days later.
        self.positive = self.positive.saturating_add(1);
        if self.phase == Phase::Blocked {
            return None;
        }
        if self.positive >= SUSTAIN_SAMPLES {
            self.phase = Phase::Blocked;
            return Some(Transition::Engaged);
        }
        self.phase = Phase::Pending;
        None
    }

    /// Is the fn tap currently reported blind?
    pub fn blocked(&self) -> bool {
        self.phase == Phase::Blocked
    }
}

/// The snapshot the UI renders. Flattened into `AppStatus` so the pill, the
/// banner and the tray all read one source.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecureInputStatus {
    /// Secure Input is sustained: fn / fn⌃ cannot reach Yap right now.
    pub blocked: bool,
    /// Best-effort holder, e.g. `"Terminal (pid 431)"`. `None` whenever macOS
    /// does not expose one — which is common, and never invalidates `blocked`.
    pub culprit: Option<String>,
}

impl SecureInputStatus {
    /// The secondary line under [`BLOCKED_MESSAGE`]: who (if known) plus the
    /// paths that still work, since fn has no Carbon equivalent to fall back to.
    pub fn detail(&self) -> String {
        let who = match &self.culprit {
            Some(c) => format!("{c} has Secure Input on"),
            None => "Another app has Secure Input on".to_string(),
        };
        format!(
            "{who} — macOS is hiding key events from Yap, so fn / fn⌃ cannot start a take. \
             Close that app's password field (or turn off Secure Keyboard Entry) to restore it. \
             Until then use the menu-bar Start Dictation item, which is unaffected."
        )
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use std::process::Command;

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        // HIToolbox; Carbon `Boolean` is an unsigned char.
        fn IsSecureEventInputEnabled() -> u8;
    }

    pub fn is_enabled() -> bool {
        unsafe { IsSecureEventInputEnabled() != 0 }
    }

    /// Best-effort "who holds it", via the IORegistry session property every
    /// other tool reads. Apple ships no supported API, and the pid is often
    /// absent (the holder already quit) or the responsible parent rather than
    /// the literal window — so this is only ever decoration on a status that is
    /// already true without it. Shells out, so it runs on the engaged EDGE only,
    /// never on every poll.
    pub fn culprit() -> Option<String> {
        let out = Command::new("ioreg")
            .args(["-l", "-w", "0"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let pid: i32 = text
            .lines()
            .find_map(|l| l.split("\"kCGSSessionSecureInputPID\"=").nth(1))?
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .ok()?;
        // `ps -o comm=` prints the full executable path; show the binary name.
        let name = Command::new("ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .output()
            .ok()
            .and_then(|o| {
                let raw = String::from_utf8_lossy(&o.stdout);
                let trimmed = raw.trim();
                (!trimmed.is_empty())
                    .then(|| trimmed.rsplit('/').next().unwrap_or(trimmed).to_string())
            })?;
        Some(format!("{name} (pid {pid})"))
    }
}

#[cfg(not(target_os = "macos"))]
mod sys {
    pub fn is_enabled() -> bool {
        false
    }
    pub fn culprit() -> Option<String> {
        None
    }
}

/// Live read of the flag (macOS only; always `false` elsewhere).
pub fn is_enabled_now() -> bool {
    sys::is_enabled()
}

static RUNNING: AtomicBool = AtomicBool::new(false);

/// Start the polling watchdog. `on_change` fires ONLY on an edge, with the new
/// snapshot — the caller stores it and re-emits its own status. Idempotent: a
/// second call is a no-op, mirroring `ptt_macos::start`.
pub fn start<F>(on_change: F)
where
    F: Fn(SecureInputStatus) + Send + 'static,
{
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Err(e) = std::thread::Builder::new()
        .name("wv-secure-input".into())
        .spawn(move || {
            let mut watchdog = Watchdog::new();
            log::info!(
                "secure-input watchdog started (poll {}s)",
                POLL_INTERVAL.as_secs()
            );
            loop {
                std::thread::sleep(POLL_INTERVAL);
                let Some(transition) = watchdog.observe(is_enabled_now()) else {
                    continue;
                };
                // The edge decides what to LOG and whether to pay for a culprit
                // lookup; the machine itself stays the single source of truth
                // for the flag the UI renders.
                let status = match transition {
                    Transition::Engaged => {
                        let culprit = sys::culprit();
                        match &culprit {
                            Some(c) => log::warn!(
                                "Secure Input ENABLED by {c} — the fn PTT event tap is blind; \
                                 fn / fn⌃ cannot start a take until it is released"
                            ),
                            None => log::warn!(
                                "Secure Input ENABLED (holder not exposed by macOS) — the fn PTT \
                                 event tap is blind; fn / fn⌃ cannot start a take until it is \
                                 released"
                            ),
                        }
                        SecureInputStatus {
                            blocked: watchdog.blocked(),
                            culprit,
                        }
                    }
                    Transition::Cleared => {
                        log::info!("Secure Input released — fn PTT event tap sees keys again");
                        SecureInputStatus {
                            blocked: watchdog.blocked(),
                            culprit: None,
                        }
                    }
                };
                on_change(status);
            }
        })
    {
        // Non-fatal: without the watchdog the app behaves exactly as it did
        // before YV43 (silent when Secure Input engages) — that is a lost
        // warning, not a reason to take the process down.
        log::error!("failed to spawn secure-input watchdog: {e}; Secure Input will go unreported");
        RUNNING.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::{SecureInputStatus, Transition, Watchdog, BLOCKED_MESSAGE, SUSTAIN_SAMPLES};

    /// The whole point of the sustain window: a password field that takes focus
    /// between two polls must never put a banner on screen.
    #[test]
    fn a_single_positive_sample_is_not_reported() {
        let mut w = Watchdog::new();
        assert_eq!(w.observe(true), None);
        assert!(!w.blocked());
        assert_eq!(
            w.observe(false),
            None,
            "no edge — nothing was ever reported"
        );
        assert!(!w.blocked());
    }

    /// Sustained Secure Input engages exactly once, stays engaged while held,
    /// and clears exactly once on release.
    #[test]
    fn sustained_secure_input_engages_once_and_clears_once() {
        let mut w = Watchdog::new();
        for _ in 1..SUSTAIN_SAMPLES {
            assert_eq!(w.observe(true), None);
        }
        assert_eq!(w.observe(true), Some(Transition::Engaged));
        assert!(w.blocked());

        // Still held: no repeat edges, so the WARN + emit fire once, not forever.
        for _ in 0..10 {
            assert_eq!(w.observe(true), None);
            assert!(w.blocked());
        }

        assert_eq!(w.observe(false), Some(Transition::Cleared));
        assert!(!w.blocked());
        assert_eq!(w.observe(false), None);
    }

    /// A gap resets the run, so flapping never accumulates its way to blocked.
    #[test]
    fn positive_samples_must_be_consecutive() {
        let mut w = Watchdog::new();
        for _ in 0..5 {
            assert_eq!(w.observe(true), None);
            assert_eq!(w.observe(false), None);
        }
        assert!(!w.blocked());
        // Now hold it for real.
        for _ in 1..SUSTAIN_SAMPLES {
            assert_eq!(w.observe(true), None);
        }
        assert_eq!(w.observe(true), Some(Transition::Engaged));
    }

    /// Re-arming: after a clear, the next sustained episode reports again.
    #[test]
    fn clearing_rearms_the_watchdog() {
        let mut w = Watchdog::new();
        for _ in 1..SUSTAIN_SAMPLES {
            w.observe(true);
        }
        assert_eq!(w.observe(true), Some(Transition::Engaged));
        assert_eq!(w.observe(false), Some(Transition::Cleared));
        for _ in 1..SUSTAIN_SAMPLES {
            assert_eq!(w.observe(true), None);
        }
        assert_eq!(w.observe(true), Some(Transition::Engaged));
        assert!(w.blocked());
    }

    /// The detail line is honest with and without a holder, and never claims a
    /// fn fallback exists (Carbon cannot express the fn modifier).
    #[test]
    fn detail_names_the_holder_when_known_and_never_promises_an_fn_fallback() {
        let known = SecureInputStatus {
            blocked: true,
            culprit: Some("Terminal (pid 431)".into()),
        };
        let detail = known.detail();
        assert!(
            detail.starts_with("Terminal (pid 431) has Secure Input on"),
            "{detail}"
        );
        assert!(detail.contains("menu-bar Start Dictation"), "{detail}");

        let unknown = SecureInputStatus {
            blocked: true,
            culprit: None,
        };
        assert!(
            unknown
                .detail()
                .starts_with("Another app has Secure Input on"),
            "{}",
            unknown.detail()
        );

        // The headline the user sees names the cause rather than blaming Yap.
        assert!(BLOCKED_MESSAGE.contains("Secure Input"));
    }
}
