//! yap-p0-hotkeys-no-permission — a take starts and stops with ZERO TCC grants.
//!
//! Before this module the ONLY chord that could start a take was the fn / fn⌃
//! CGEvent tap in `ptt_macos`, and a CGEvent tap needs Input Monitoring (and,
//! for the selection/paste side, Accessibility). Every Carbon chord Yap held
//! while idle (⌃⌘V, ⌃⌘Z, ⌃⌘M) did something OTHER than dictate, and the one
//! dictation chord (⌘⇧V) was off by default. Meanwhile `hotkey_registered` was
//! written `true` the instant the tap THREAD was spawned — before
//! `CGEventTapCreate` had even run — so a Mac with every grant denied showed a
//! green "hotkey registered" row and no way to dictate.
//!
//! The shape now:
//!
//!   * [`crate::shortcuts::DICTATE_TOGGLE`] (default ⌃⌥⌘Space) is a Carbon
//!     `RegisterEventHotKey` chord registered unconditionally at launch. Carbon
//!     hot keys need no Accessibility, no Input Monitoring, and are delivered
//!     under Secure Input, so this is the PRIMARY path.
//!   * the fn tap stays for the chords Carbon cannot express, and is ALWAYS
//!     attempted — at launch and on every permission-watch edge — never gated on
//!     the permission probe ([`attempt_tap`]).
//!   * ONE arbiter ([`HotkeyArbiter`]) decides start / stop / ignore for every
//!     edge from either source, so the two can never double-start.
//!   * status is truthful ([`HotkeysStatus`]): Carbon carries the Ok/Err of the
//!     register call, the tap is `Delivering` only after a delivered edge.
//!
//! Everything that touches the OS goes through the three traits below
//! ([`CarbonRegistrar`], [`PermissionProbe`], [`TapEventSource`]) so
//! `tests/hotkey_no_tcc.rs` drives the real decision code with every grant
//! denied and no OS hook installed.

use std::fmt::Display;
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::Instant;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

use crate::shortcuts;

/// Which keyboard surface an edge came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum HotkeySource {
    /// A Carbon `RegisterEventHotKey` chord (the dictate toggle, or the legacy
    /// ⌘⇧V). Needs no TCC grant.
    Carbon,
    /// The fn / fn⌃ CGEvent tap (`ptt_macos`). Needs Input Monitoring.
    FnTap,
}

/// Key-down or key-up of the chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum HotkeyEdge {
    Press,
    Release,
}

/// What the dispatcher must do with one edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TakeAction {
    Start,
    Stop,
    Ignore,
}

/// A Carbon press held at least this long, then released, is hold-to-talk: the
/// release stops the take. Shorter than this, the press was a toggle tap and the
/// take keeps running until the next press. Equal to `ptt_macos::HOLD_ARM_MS`
/// (asserted in tests/hotkey_no_tcc.rs) so fn and the chord feel the same.
pub const CARBON_HOLD_MS: u64 = 280;

/// The ONE decision for every hotkey edge from either source. PURE.
#[derive(Debug, Clone, Default)]
pub struct HotkeyArbiter {
    owner: Option<HotkeySource>,
    carbon_press_at: Option<u64>,
}

impl HotkeyArbiter {
    pub const fn new() -> Self {
        Self {
            owner: None,
            carbon_press_at: None,
        }
    }

    /// The source that started the live take, if a hotkey started it.
    pub fn owner(&self) -> Option<HotkeySource> {
        self.owner
    }

    /// Decide one edge. `take_live` is the app's own recording flag, so a take
    /// stopped by the pill or the tray is seen here as over.
    pub fn edge(
        &mut self,
        edge: HotkeyEdge,
        source: HotkeySource,
        take_live: bool,
        now_ms: u64,
    ) -> TakeAction {
        if !take_live {
            self.owner = None;
        }
        match (source, edge) {
            (HotkeySource::Carbon, HotkeyEdge::Press) => {
                if take_live {
                    // Toggle off — whoever started it (fn, chord, pill, tray).
                    self.owner = None;
                    self.carbon_press_at = None;
                    TakeAction::Stop
                } else {
                    self.owner = Some(HotkeySource::Carbon);
                    self.carbon_press_at = Some(now_ms);
                    TakeAction::Start
                }
            }
            (HotkeySource::Carbon, HotkeyEdge::Release) => {
                let Some(at) = self.carbon_press_at.take() else {
                    return TakeAction::Ignore;
                };
                if take_live
                    && self.owner == Some(HotkeySource::Carbon)
                    && now_ms.saturating_sub(at) >= CARBON_HOLD_MS
                {
                    self.owner = None;
                    TakeAction::Stop
                } else {
                    TakeAction::Ignore
                }
            }
            (HotkeySource::FnTap, HotkeyEdge::Press) => {
                if take_live {
                    // Never a second start: a take is already live (started by
                    // the chord, or by fn itself).
                    TakeAction::Ignore
                } else {
                    self.owner = Some(HotkeySource::FnTap);
                    TakeAction::Start
                }
            }
            (HotkeySource::FnTap, HotkeyEdge::Release) => {
                if take_live {
                    self.owner = None;
                    self.carbon_press_at = None;
                    TakeAction::Stop
                } else {
                    TakeAction::Ignore
                }
            }
        }
    }
}

// ── truthful status ─────────────────────────────────────────────────────────

/// The dictate-toggle Carbon registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "camelCase")]
pub enum CarbonStatus {
    /// No register call has run (pre-launch, or a `--smoke` launch, which
    /// registers nothing system-wide by design).
    NotAttempted,
    Registered,
    Failed(String),
}

impl CarbonStatus {
    /// Straight from the register call's result — never inferred.
    pub fn from_result<E: Display>(r: &Result<(), E>) -> Self {
        match r {
            Ok(()) => Self::Registered,
            Err(e) => Self::Failed(e.to_string()),
        }
    }
}

/// The fn / fn⌃ event tap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "camelCase")]
pub enum FnTapStatus {
    /// Not tried yet (pre-launch, or `--smoke`). Never used for "tried and failed".
    NotAttempted,
    /// A real key / flags edge reached the callback since the tap was installed.
    Delivering,
    /// `CGEventTapCreate` succeeded but no edge has been delivered yet.
    ArmedNoDelivery,
    /// Attempted, nothing delivered: the install itself failed (reason).
    Blocked(String),
    /// Another app holds Secure Input; names the holder.
    SecureInput(String),
}

/// What `AppStatus.hotkeys` carries to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeysStatus {
    pub carbon: CarbonStatus,
    pub fn_tap: FnTapStatus,
}

impl HotkeysStatus {
    pub const fn new() -> Self {
        Self {
            carbon: CarbonStatus::NotAttempted,
            fn_tap: FnTapStatus::NotAttempted,
        }
    }
}

impl Default for HotkeysStatus {
    fn default() -> Self {
        Self::new()
    }
}

/// Where arming writes its results. The app's sink writes the process-wide
/// status ([`set_carbon_status`] / [`set_fn_tap_status`]); tests use a plain
/// [`HotkeysStatus`].
pub trait StatusSink {
    fn carbon(&mut self, s: CarbonStatus);
    fn fn_tap(&mut self, s: FnTapStatus);
    fn current_fn_tap(&self) -> FnTapStatus;
}

impl StatusSink for HotkeysStatus {
    fn carbon(&mut self, s: CarbonStatus) {
        self.carbon = s;
    }
    fn fn_tap(&mut self, s: FnTapStatus) {
        self.fn_tap = s;
    }
    fn current_fn_tap(&self) -> FnTapStatus {
        self.fn_tap.clone()
    }
}

static STATUS: Mutex<HotkeysStatus> = Mutex::new(HotkeysStatus::new());
type ChangeHook = Box<dyn Fn() + Send + Sync>;
static ON_CHANGE: OnceLock<ChangeHook> = OnceLock::new();

/// Install the one hook that re-emits `status` when either half changes. The
/// hook must be cheap: `set_fn_tap_status` is called from the tap callback.
pub fn on_status_change(f: impl Fn() + Send + Sync + 'static) {
    let _ = ON_CHANGE.set(Box::new(f));
}

fn notify() {
    if let Some(f) = ON_CHANGE.get() {
        f();
    }
}

/// Record the result of a DICTATE_TOGGLE register call.
pub fn set_carbon_status(s: CarbonStatus) {
    let changed = {
        let mut g = STATUS.lock();
        let changed = g.carbon != s;
        g.carbon = s.clone();
        changed
    };
    if changed {
        log::info!("hotkeys: carbon dictate toggle → {s:?}");
        notify();
    }
}

/// Record the fn tap's observed state (install result, recreate, first edge).
pub fn set_fn_tap_status(s: FnTapStatus) {
    let changed = {
        let mut g = STATUS.lock();
        let changed = g.fn_tap != s;
        g.fn_tap = s.clone();
        changed
    };
    if changed {
        log::info!("hotkeys: fn tap → {s:?}");
        notify();
    }
}

/// Snapshot of the process-wide status.
pub fn status() -> HotkeysStatus {
    STATUS.lock().clone()
}

// ── the OS seam ─────────────────────────────────────────────────────────────

/// Registers one Carbon hot key (the real one is the global-shortcut plugin,
/// which is `RegisterEventHotKey` on macOS).
pub trait CarbonRegistrar {
    fn register(&mut self, chord: Shortcut) -> Result<(), String>;
}

/// The TCC probe. INFORMATIONAL for arming: it never decides whether the tap is
/// attempted, only what the UI says.
pub trait PermissionProbe {
    fn accessibility(&self) -> bool;
    fn input_monitoring(&self) -> bool;
    /// Either grant is enough for a listen-only tap to plausibly deliver.
    fn tap_plausible(&self) -> bool {
        self.accessibility() || self.input_monitoring()
    }
}

/// How an install attempt went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapAttempt {
    /// The tap is installed right now (fakes; synchronous installers).
    Installed,
    /// Handed to the tap thread, which reports through [`set_fn_tap_status`]
    /// itself once `CGEventTapCreate` has answered (the real `ptt_macos`).
    Dispatched,
}

/// Installs the fn event tap.
pub trait TapEventSource {
    fn install(&mut self) -> Result<TapAttempt, String>;
}

/// Launch: register the dictate toggle UNCONDITIONALLY, then attempt the tap.
pub fn arm_at_launch(
    chord: Shortcut,
    registrar: &mut dyn CarbonRegistrar,
    probe: &dyn PermissionProbe,
    tap: &mut dyn TapEventSource,
    sink: &mut dyn StatusSink,
) {
    let r = registrar.register(chord);
    sink.carbon(CarbonStatus::from_result(&r));
    attempt_tap(probe, tap, sink);
}

/// Attempt (or re-attempt) the fn tap. Called at launch and on EVERY
/// permission-watch edge. Never gated on the probe — yap.log 2026-10-17
/// 07:40:34Z–07:40:39Z shows fn starting a take while AXIsProcessTrusted said
/// "not trusted". Skipped only when a tap is already installed. Returns whether
/// an install was attempted.
pub fn attempt_tap(
    probe: &dyn PermissionProbe,
    tap: &mut dyn TapEventSource,
    sink: &mut dyn StatusSink,
) -> bool {
    if matches!(
        sink.current_fn_tap(),
        FnTapStatus::Delivering | FnTapStatus::ArmedNoDelivery
    ) {
        return false;
    }
    log::info!(
        "fn tap: attempting install (probe, informational only: accessibility={} input_monitoring={})",
        probe.accessibility(),
        probe.input_monitoring()
    );
    match tap.install() {
        Ok(TapAttempt::Installed) => sink.fn_tap(FnTapStatus::ArmedNoDelivery),
        Ok(TapAttempt::Dispatched) => {}
        Err(reason) => sink.fn_tap(FnTapStatus::Blocked(reason)),
    }
    true
}

/// A delivered key / flags edge: the only evidence for `Delivering`.
pub fn note_delivered_edge(sink: &mut dyn StatusSink) {
    if sink.current_fn_tap() != FnTapStatus::Delivering {
        sink.fn_tap(FnTapStatus::Delivering);
    }
}

/// The fn status the UI shows: Secure Input outranks the stored value (the tap
/// is installed but macOS hides every key event from it).
pub fn effective_fn_tap(
    stored: &FnTapStatus,
    secure_input_blocked: bool,
    holder: Option<&str>,
) -> FnTapStatus {
    if secure_input_blocked {
        let who = holder
            .map(|h| h.split(" (pid").next().unwrap_or(h).to_string())
            .unwrap_or_else(|| "Another app".to_string());
        return FnTapStatus::SecureInput(who);
    }
    stored.clone()
}

/// The degraded-mode line for the pill and home screen, or `None` when fn
/// works. `probe_granted` only softens the pre-first-keystroke state: an
/// installed tap with grants present has simply not seen a key yet.
pub fn degraded_line(fn_tap: &FnTapStatus, probe_granted: bool, toggle_label: &str) -> Option<String> {
    let reason = match fn_tap {
        FnTapStatus::Delivering => return None,
        FnTapStatus::ArmedNoDelivery if probe_granted => return None,
        FnTapStatus::ArmedNoDelivery => {
            "macOS is not delivering key events to Yap (Input Monitoring is off)".to_string()
        }
        FnTapStatus::NotAttempted => "the fn listener has not started".to_string(),
        FnTapStatus::Blocked(r) => r.clone(),
        FnTapStatus::SecureInput(holder) => format!("{holder} has Secure Keyboard Entry on"),
    };
    Some(format!(
        "fn is off — {reason}. Use {toggle_label} (works without permissions)."
    ))
}

// ── the rebindable chord ────────────────────────────────────────────────────

/// A user-chosen dictate chord: a `Code` plus modifiers, nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChordSetting {
    /// W3C `KeyboardEvent.code`, e.g. `Space`, `KeyD`, `Digit1`.
    pub code: String,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub cmd: bool,
    #[serde(default)]
    pub shift: bool,
}

const MODIFIER_CODES: &[Code] = &[
    Code::ControlLeft,
    Code::ControlRight,
    Code::AltLeft,
    Code::AltRight,
    Code::MetaLeft,
    Code::MetaRight,
    Code::ShiftLeft,
    Code::ShiftRight,
    Code::Fn,
    Code::CapsLock,
];

/// The macOS glyph label for a chord: ⌃⌥⇧⌘ then the key.
pub fn chord_label(ctrl: bool, alt: bool, shift: bool, cmd: bool, code: Code) -> String {
    let mut s = String::new();
    if ctrl {
        s.push('⌃');
    }
    if alt {
        s.push('⌥');
    }
    if shift {
        s.push('⇧');
    }
    if cmd {
        s.push('⌘');
    }
    let key = format!("{code:?}");
    let key = key
        .strip_prefix("Key")
        .or_else(|| key.strip_prefix("Digit"))
        .unwrap_or(&key)
        .to_string();
    s.push_str(&key);
    s
}

/// Validate a rebind and turn it into the chord + its label. Refuses a chord
/// with no ⌃/⌥/⌘ (it would swallow a bare key system-wide) and a modifier-only
/// one (Carbon cannot register it).
pub fn chord_from_setting(c: &ChordSetting) -> Result<(Shortcut, String), String> {
    let code = Code::from_str(&c.code).map_err(|_| format!("unknown key code {:?}", c.code))?;
    if MODIFIER_CODES.contains(&code) || matches!(code, Code::Unidentified) {
        return Err("the chord needs a non-modifier key".into());
    }
    if !(c.ctrl || c.alt || c.cmd) {
        return Err("the chord needs ⌃, ⌥ or ⌘".into());
    }
    let mut m = Modifiers::empty();
    if c.ctrl {
        m |= Modifiers::CONTROL;
    }
    if c.alt {
        m |= Modifiers::ALT;
    }
    if c.shift {
        m |= Modifiers::SHIFT;
    }
    if c.cmd {
        m |= Modifiers::SUPER;
    }
    Ok((
        Shortcut::new(Some(m), code),
        chord_label(c.ctrl, c.alt, c.shift, c.cmd, code),
    ))
}

static ACTIVE_DICTATE: Mutex<Option<(Shortcut, String)>> = Mutex::new(None);

/// The dictate chord currently registered, and its label. Defaults to
/// [`shortcuts::DICTATE_TOGGLE`].
pub fn active_dictate() -> (Shortcut, String) {
    ACTIVE_DICTATE.lock().clone().unwrap_or_else(|| {
        (
            shortcuts::DICTATE_TOGGLE.shortcut(),
            shortcuts::DICTATE_TOGGLE.label.to_string(),
        )
    })
}

pub fn set_active_dictate(chord: Shortcut, label: String) {
    *ACTIVE_DICTATE.lock() = Some((chord, label));
}

static CLOCK: OnceLock<Instant> = OnceLock::new();

/// Monotonic ms since first use — the arbiter's clock.
pub fn now_ms() -> u64 {
    CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebind_accepts_code_plus_modifiers_only() {
        let ok = ChordSetting { code: "KeyD".into(), ctrl: true, alt: true, cmd: true, shift: false };
        let (sc, label) = chord_from_setting(&ok).unwrap();
        assert_eq!(label, "⌃⌥⌘D");
        assert_eq!(sc, Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER), Code::KeyD));
        let bare = ChordSetting { code: "KeyD".into(), ..Default::default() };
        assert!(chord_from_setting(&bare).is_err());
        let modifier_only = ChordSetting { code: "ShiftLeft".into(), cmd: true, ..Default::default() };
        assert!(chord_from_setting(&modifier_only).is_err());
        let default = ChordSetting { code: "Space".into(), ctrl: true, alt: true, cmd: true, shift: false };
        assert_eq!(chord_from_setting(&default).unwrap().0, shortcuts::DICTATE_TOGGLE.shortcut());
        assert_eq!(chord_from_setting(&default).unwrap().1, shortcuts::DICTATE_TOGGLE.label);
    }
}
