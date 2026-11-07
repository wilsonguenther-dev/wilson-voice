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
///
/// It is a POLL because macOS publishes no notification for Secure Input: there
/// is no `NSDistributedNotification`, no KVO key, nothing to observe, so
/// `IsSecureEventInputEnabled()` is the only reading there is. 2s × the two
/// samples [`SUSTAIN_SAMPLES`] wants is a ~4s worst case before the user is
/// told why fn stopped working — the longest interval that still answers the
/// question inside the pause where they are pressing the key and nothing
/// happens (YV81).
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
    /// yap-1107-secure-input-own-field-quiet — Secure Input is held by YAP
    /// ITSELF (WebKit turns it on while the masked cloud key field has focus).
    /// `blocked` stays false, so the red banner, the tray and the pill stay
    /// quiet; the main window shows only the one neutral
    /// [`OWN_FIELD_MESSAGE`] line, gone on the release edge (field blurred).
    pub own_field: bool,
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

/// The one quiet line the main window shows while Yap's OWN key field holds
/// Secure Input. Neutral, never red, no detail paragraph: the user caused it
/// by focusing the field, and blurring the field ends it.
pub const OWN_FIELD_MESSAGE: &str = "Shortcut paused while the key field is focused";

/// Who holds Secure Input, decided once on the engaged edge by
/// [`classify_holder`] from the IORegistry pid + its process name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    /// Yap's own process (its masked key field has focus). Quiet line only.
    Own,
    /// `loginwindow` — the lock screen (yap24-PILL3). Logged, never shown.
    LockScreen,
    /// Another app, as the culprit string `"Name (pid N)"`. Loud banner.
    Other(String),
    /// macOS did not expose a usable holder. Loud banner, unnamed.
    Unknown,
}

/// Pure classifier for the engaged edge. `own_pid` is
/// `std::process::id()`; a pid match wins over any name, so a renamed Yap
/// bundle (Yap Test, wilson-voice) is still recognised as itself.
pub fn classify_holder(pid: Option<i32>, name: Option<&str>, own_pid: i32) -> Holder {
    if pid == Some(own_pid) {
        return Holder::Own;
    }
    match (pid, name.map(str::trim).filter(|n| !n.is_empty())) {
        (_, Some(n)) if !reportable_holder(Some(n)) => Holder::LockScreen,
        (Some(p), Some(n)) => Holder::Other(format!("{} (pid {p})", holder_app(n))),
        (None, Some(n)) => Holder::Other(n.to_string()),
        (_, None) => Holder::Unknown,
    }
}

impl Holder {
    /// Is this hold shown LOUD (red banner, tray, pill, hotkey table)?
    pub fn reported(&self) -> bool {
        matches!(self, Self::Other(_) | Self::Unknown)
    }

    /// The `owner=` value of the one engaged log line.
    pub fn log_owner(&self) -> &'static str {
        match self {
            Self::Own => "self",
            Self::LockScreen => IGNORED_HOLDER,
            Self::Other(_) => "other",
            Self::Unknown => "unknown",
        }
    }

    /// The snapshot the UI renders for an ENGAGED hold by this holder.
    pub fn engaged_status(&self) -> SecureInputStatus {
        match self {
            Self::Own => SecureInputStatus {
                blocked: false,
                culprit: None,
                own_field: true,
            },
            Self::LockScreen => SecureInputStatus::default(),
            Self::Other(c) => SecureInputStatus {
                blocked: true,
                culprit: Some(c.clone()),
                own_field: false,
            },
            Self::Unknown => SecureInputStatus {
                blocked: true,
                culprit: None,
                own_field: false,
            },
        }
    }
}

/// This process's pid in the `kCGSSessionSecureInputPID` type.
pub fn own_pid() -> i32 {
    i32::try_from(std::process::id()).unwrap_or(-1)
}

/// yap24-PILL3 — the holder macOS reports at the LOCK SCREEN. Wilson's logs
/// show `loginwindow` as the most frequent Secure Input owner by far, and it
/// only holds it while the screen is locked — when the pill is not visible
/// and there is no fn press to rescue. Reporting it painted the pill and the
/// tray with a warning nobody could act on and wrote most of the 100+ WARN
/// lines, so a `loginwindow` hold is logged at INFO and never reported.
pub const IGNORED_HOLDER: &str = "loginwindow";

/// The app name inside a [`SecureInputStatus::culprit`] string
/// (`"Google Chrome (pid 431)"` -> `"Google Chrome"`). The pill names the APP,
/// never a pid: "fn is blocked by Google Chrome's secure field".
pub fn holder_app(culprit: &str) -> &str {
    match culprit.rfind(" (pid ") {
        Some(i) if culprit.ends_with(')') => &culprit[..i],
        _ => culprit,
    }
}

/// yap24-PILL3 dwell rule — is an engaged hold worth SHOWING? Every holder is
/// except [`IGNORED_HOLDER`]. An unknown holder (`None`) is reported: macOS
/// often hides the pid, and the fn tap is just as blind either way.
pub fn reportable_holder(culprit: Option<&str>) -> bool {
    culprit.is_none_or(|c| holder_app(c) != IGNORED_HOLDER)
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
    /// never on every poll. Returns the raw (pid, binary name) pair; the pid is
    /// kept so [`super::classify_holder`] can recognise Yap's own process.
    pub fn holder() -> (Option<i32>, Option<String>) {
        let Some(out) = Command::new("ioreg").args(["-l", "-w", "0"]).output().ok() else {
            return (None, None);
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let pid: Option<i32> = text
            .lines()
            .find_map(|l| l.split("\"kCGSSessionSecureInputPID\"=").nth(1))
            .and_then(|rest| {
                rest.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .ok()
            });
        let Some(pid) = pid else {
            return (None, None);
        };
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
            });
        (Some(pid), name)
    }
}

#[cfg(not(target_os = "macos"))]
mod sys {
    pub fn is_enabled() -> bool {
        false
    }
    pub fn holder() -> (Option<i32>, Option<String>) {
        (None, None)
    }
}

/// Live read of the flag (macOS only; always `false` elsewhere).
pub fn is_enabled_now() -> bool {
    sys::is_enabled()
}

static RUNNING: AtomicBool = AtomicBool::new(false);

/// Is the watchdog's [`POLL_INTERVAL`] poll running? One line of the YV81
/// energy telemetry, so a build that starts a second one is visible in the log.
pub fn active_polls() -> usize {
    usize::from(RUNNING.load(Ordering::SeqCst))
}

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
                        let (pid, name) = sys::holder();
                        let holder = classify_holder(pid, name.as_deref(), own_pid());
                        match &holder {
                            // yap-1107-secure-input-own-field-quiet: Yap's own
                            // masked key field. Once per hold (the watchdog
                            // stays Blocked until a released sample).
                            Holder::Own => log::info!(
                                "secure_input engaged owner={} reported=quiet",
                                holder.log_owner()
                            ),
                            // yap24-PILL3: the lock screen. Logged once per hold,
                            // never shown; the watchdog stays Blocked, so this
                            // does not re-fire until a released sample clears it.
                            Holder::LockScreen => log::info!(
                                "secure_input engaged owner={IGNORED_HOLDER} reported=false \
                                 reason=lock_screen"
                            ),
                            Holder::Other(c) => log::warn!(
                                "Secure Input ENABLED by {c} — the fn PTT event tap is blind; \
                                 fn / fn⌃ cannot start a take until it is released"
                            ),
                            Holder::Unknown => log::warn!(
                                "Secure Input ENABLED (holder not exposed by macOS) — the fn \
                                 PTT event tap is blind; fn / fn⌃ cannot start a take until \
                                 it is released"
                            ),
                        }
                        let mut status = holder.engaged_status();
                        // The machine stays the single source of truth for the
                        // loud flag: only a reported holder can raise it.
                        status.blocked = holder.reported() && watchdog.blocked();
                        status
                    }
                    Transition::Cleared => {
                        log::info!("Secure Input released — fn PTT event tap sees keys again");
                        SecureInputStatus {
                            blocked: watchdog.blocked(),
                            culprit: None,
                            own_field: false,
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
    use super::{
        classify_holder, holder_app, own_pid, reportable_holder, Holder, SecureInputStatus,
        Transition, Watchdog, BLOCKED_MESSAGE, OWN_FIELD_MESSAGE, POLL_INTERVAL, SUSTAIN_SAMPLES,
    };

    /// yap-1107-secure-input-own-field-quiet — Yap's own key field (pid ==
    /// this process) is Own: never the loud banner, only the quiet flag.
    #[test]
    fn own_pid_hold_is_quiet_and_never_blocked() {
        let me = own_pid();
        let h = classify_holder(Some(me), Some("wilson-voice"), me);
        assert_eq!(h, Holder::Own);
        assert!(!h.reported());
        assert_eq!(h.log_owner(), "self");
        let st = h.engaged_status();
        assert!(!st.blocked, "own field must not raise the red banner");
        assert!(st.own_field, "own field sets the quiet flag");
        assert_eq!(st.culprit, None, "never names wilson-voice as a culprit");
        // A pid match wins over any name (Yap Test, renamed bundles).
        assert_eq!(classify_holder(Some(77), Some("Yap Test"), 77), Holder::Own);
        assert_eq!(classify_holder(Some(77), None, 77), Holder::Own);
        assert_eq!(
            OWN_FIELD_MESSAGE,
            "Shortcut paused while the key field is focused"
        );
    }

    /// Another app's hold stays LOUD exactly as before, and names that app.
    #[test]
    fn another_pid_hold_is_blocked_and_names_the_app() {
        let h = classify_holder(Some(431), Some("Terminal"), 99);
        assert_eq!(h, Holder::Other("Terminal (pid 431)".into()));
        assert!(h.reported());
        let st = h.engaged_status();
        assert!(st.blocked);
        assert!(!st.own_field);
        assert_eq!(st.culprit.as_deref(), Some("Terminal (pid 431)"));
        assert!(st
            .detail()
            .starts_with("Terminal (pid 431) has Secure Input on"));
    }

    /// loginwindow (lock screen) is never reported; no holder at all is.
    #[test]
    fn lock_screen_is_quiet_and_unknown_is_reported() {
        let ls = classify_holder(Some(152), Some("loginwindow"), 99);
        assert_eq!(ls, Holder::LockScreen);
        assert!(!ls.reported());
        let st = ls.engaged_status();
        assert!(!st.blocked && !st.own_field && st.culprit.is_none());

        for unknown in [
            classify_holder(None, None, 99),
            classify_holder(Some(5), None, 99),
            classify_holder(Some(5), Some("  "), 99),
        ] {
            assert_eq!(unknown, Holder::Unknown);
            assert!(unknown.reported());
            let st = unknown.engaged_status();
            assert!(st.blocked && !st.own_field && st.culprit.is_none());
            assert!(st.detail().starts_with("Another app has Secure Input on"));
        }
    }

    /// yap24-PILL3 — the lock screen's hold is never shown; every other
    /// holder, and an unknown one, is.
    #[test]
    fn loginwindow_is_never_reported_and_every_other_holder_is() {
        assert!(!reportable_holder(Some("loginwindow (pid 152)")));
        assert!(!reportable_holder(Some("loginwindow")));
        assert!(reportable_holder(Some("Google Chrome (pid 431)")));
        assert!(reportable_holder(Some("Terminal (pid 9)")));
        assert!(reportable_holder(None));
    }

    #[test]
    fn holder_app_drops_the_pid_and_keeps_names_with_parentheses() {
        assert_eq!(holder_app("Google Chrome (pid 431)"), "Google Chrome");
        assert_eq!(holder_app("1Password 7"), "1Password 7");
        assert_eq!(holder_app("Foo (Beta) (pid 2)"), "Foo (Beta)");
    }

    /// yap24-PILL3 dwell rule: a Secure Input blip shorter than one poll
    /// (a Chrome password field focused for 500 ms) is at most ONE positive
    /// sample, and one sample never engages — so the pill cannot flicker.
    /// Engaging needs SUSTAIN_SAMPLES consecutive samples, i.e. a hold of at
    /// least one full POLL_INTERVAL (>= 1 s).
    #[test]
    fn a_half_second_blip_never_engages_and_a_sustained_hold_needs_a_full_poll() {
        assert!(POLL_INTERVAL.as_millis() * u128::from(SUSTAIN_SAMPLES - 1) >= 1_000);
        let mut w = Watchdog::new();
        assert_eq!(w.observe(true), None, "500 ms blip: one positive sample");
        assert_eq!(w.observe(false), None);
        assert!(!w.blocked());
        assert_eq!(w.observe(true), None);
        assert_eq!(w.observe(true), Some(Transition::Engaged));
        assert_eq!(
            w.observe(false),
            Some(Transition::Cleared),
            "clears on the first released sample"
        );
    }

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
            own_field: false,
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
            own_field: false,
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
