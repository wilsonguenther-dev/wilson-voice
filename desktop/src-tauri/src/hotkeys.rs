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

/// yap-1107-start-stop-any-surface — the intent one hotkey edge sends to the
/// ONE take machine (`take_control::TakeMachine`). The hotkey keeps no state
/// of its own: whether a release stops the take (hold-to-talk) is a property
/// of the TAKE (`take_control::Hold`), so a pill / menu / Dictate-button stop
/// during a hold also stops and the release that follows is a no-op.
pub fn intent_for(edge: HotkeyEdge, source: HotkeySource) -> crate::take_control::Intent {
    use crate::take_control::{HoldKey, Intent};
    match (source, edge) {
        // Idle: start a take this press rides on. Live: toggle it off —
        // whoever started it (fn, chord, pill, tray, Dictate button).
        (HotkeySource::Carbon, HotkeyEdge::Press) => Intent::HoldPress(HoldKey::Chord),
        // Held >= CARBON_HOLD_MS: hold-to-talk, the release stops. Shorter:
        // a toggle tap, the take keeps running.
        (HotkeySource::Carbon, HotkeyEdge::Release) => Intent::HoldRelease(HoldKey::Chord),
        // Never a second start while a take is live.
        (HotkeySource::FnTap, HotkeyEdge::Press) => Intent::HoldPress(HoldKey::Fn),
        // `ptt_macos` already decided this edge ends the take (`PttEvent::Stop`).
        (HotkeySource::FnTap, HotkeyEdge::Release) => Intent::Stop,
    }
}

/// The hotkey's view of the ONE take machine, for a caller that has only the
/// keyboard (tests/hotkey_no_tcc.rs drives both sources through it). It holds
/// no flag of its own: it is a `TakeMachine` fed by [`intent_for`], with the
/// recorder assumed to arm at once. The app does NOT use this wrapper — every
/// surface, the hotkey included, goes through `lib.rs::take_intent` on the
/// app's one machine.
#[derive(Debug, Clone, Default)]
pub struct HotkeyArbiter {
    machine: crate::take_control::TakeMachine,
}

impl HotkeyArbiter {
    pub const fn new() -> Self {
        Self {
            machine: crate::take_control::TakeMachine::new(),
        }
    }

    /// The hotkey source the live take is riding on as a hold, if any.
    pub fn owner(&self) -> Option<HotkeySource> {
        use crate::take_control::HoldKey;
        self.machine.hold().map(|h| match h.key {
            HoldKey::Chord => HotkeySource::Carbon,
            HoldKey::Fn => HotkeySource::FnTap,
        })
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
        use crate::take_control::{Decision, Surface};
        self.machine.observe(take_live);
        match self
            .machine
            .intent(Surface::Hotkey, intent_for(edge, source), now_ms)
        {
            Decision::Start => {
                self.machine.armed(true);
                TakeAction::Start
            }
            Decision::Stop | Decision::StopWhenArmed => {
                self.machine.stopped();
                TakeAction::Stop
            }
            Decision::Ignore(_) => TakeAction::Ignore,
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

// ── yap-1107-hotkey-exclusive-to-yap: do Yap's chord keys stay Yap's? ────────

/// Whether the key events of a chord the fn tap acts on are kept from the
/// front app (`ptt_macos::ensure_key_swallow_tap`). Carbon chords are always
/// Yap's alone — `RegisterEventHotKey` consumes them — so this only describes
/// the tap side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExclusiveStatus {
    /// Not tried yet (pre-launch, `--smoke`, or the fn tap never came up).
    NotAttempted,
    /// The active swallow tap is live.
    Exclusive,
    /// macOS refused the active tap (Accessibility is off for Yap): the hotkeys
    /// still work through the listen-only tap, and their keys also reach the
    /// app in front.
    ListenOnly,
}

/// The degraded line for [`ExclusiveStatus::ListenOnly`]: one sentence, one
/// action. "Yap" is renamed per build by `permissions::named`.
pub const LISTEN_ONLY_LINE: &str =
    "Your hotkeys work, but their keys also reach the app in front. Turn on Accessibility for Yap to keep them to Yap.";

static EXCLUSIVE: Mutex<ExclusiveStatus> = Mutex::new(ExclusiveStatus::NotAttempted);

/// Record the swallow tap's state; a change is logged and re-emits status.
pub fn set_exclusive_status(s: ExclusiveStatus) {
    let changed = {
        let mut g = EXCLUSIVE.lock();
        let changed = *g != s;
        *g = s;
        changed
    };
    if changed {
        log::info!("hotkeys exclusive_status status={s:?}");
        notify();
    }
}

pub fn exclusive_status() -> ExclusiveStatus {
    *EXCLUSIVE.lock()
}

/// The status-chip line for an exclusivity state, or `None` when nothing is
/// degraded. Pure, so the refused-Accessibility path is unit-tested.
pub fn exclusive_line(status: ExclusiveStatus) -> Option<String> {
    match status {
        ExclusiveStatus::ListenOnly => Some(crate::permissions::named(LISTEN_ONLY_LINE)),
        ExclusiveStatus::Exclusive | ExclusiveStatus::NotAttempted => None,
    }
}

/// [`exclusive_line`] for the live process state — what `app_status` falls
/// back to when [`degraded_line`] has nothing worse to say.
pub fn exclusive_degraded_line() -> Option<String> {
    exclusive_line(exclusive_status())
}

/// The glyph label of a Carbon chord, e.g. `⌃⌥⌘Space` — the only key text a
/// `hotkey swallowed` line may carry.
pub fn carbon_chord_label(shortcut: &Shortcut) -> String {
    let m = shortcut.mods;
    chord_label(
        m.contains(Modifiers::CONTROL),
        m.contains(Modifiers::ALT),
        m.contains(Modifiers::SHIFT),
        m.contains(Modifiers::SUPER),
        shortcut.key,
    )
}

/// A Carbon chord reached Yap: `RegisterEventHotKey` consumed it, so neither
/// this phase nor its twin reached the front app. One diagnostics line per
/// phase (`hotkey swallowed chord=<label> phase=down|up front=<bundle id>`).
///
/// Called from the plugin handler, which runs with the shortcut table locked
/// and must only route (yap-p0-chord-deadlock): the front-app lookup and the
/// log line run on a short-lived thread, never inline.
pub fn note_carbon_swallowed(shortcut: &Shortcut, pressed: bool) {
    let label = carbon_chord_label(shortcut);
    let phase = if pressed { "down" } else { "up" };
    let _ = std::thread::Builder::new()
        .name("wv-hotkey-swallow-log".into())
        .spawn(move || {
            let front = crate::focus::frontmost_bundle_id();
            crate::diagnostics::record_hotkey_swallowed(&label, phase, front.as_deref(), "carbon");
        });
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
    fn unregister(&mut self, chord: Shortcut) -> Result<(), String>;
    fn is_registered(&self, chord: Shortcut) -> bool;
}

/// Swap the dictate chord for a Settings rebind. The NEW chord is registered
/// FIRST and the old one is dropped only after that succeeded, so a chord
/// another app holds leaves the old chord live (and its status untouched) and
/// comes back as an `Err` for the Settings capture — never a moment, let alone
/// a saved setting, with no dictate chord at all.
pub fn rebind(
    registrar: &mut dyn CarbonRegistrar,
    old: Shortcut,
    old_label: &str,
    new: Shortcut,
    new_label: &str,
    sink: &mut dyn StatusSink,
) -> Result<(), String> {
    let old_live = registrar.is_registered(old);
    if new == old && old_live {
        sink.carbon(CarbonStatus::Registered);
        return Ok(());
    }
    if let Err(e) = registrar.register(new) {
        return Err(if old_live && old != new {
            format!("{new_label} could not be registered ({e}) — kept {old_label}")
        } else {
            format!("{new_label} could not be registered ({e})")
        });
    }
    if old_live && old != new {
        if let Err(e) = registrar.unregister(old) {
            log::warn!("dictate toggle: unregister of {old_label} failed after rebind: {e}");
        }
    }
    sink.carbon(CarbonStatus::Registered);
    Ok(())
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

/// The degraded-mode line for the main window (the header status chip's idle
/// message and the home banner), or `None` when both fn and the dictate chord
/// work. The floating pill does not render it. `probe_granted` only softens the
/// pre-first-keystroke state: an installed tap with grants present has simply
/// not seen a key yet.
///
/// The chord is advertised as the way out ONLY when its register call returned
/// Ok: a chord that failed (another app holds it) or was never attempted (pre
/// arm, `--smoke`) is named as not working, never offered as the fallback.
pub fn degraded_line(
    carbon: &CarbonStatus,
    fn_tap: &FnTapStatus,
    probe_granted: bool,
    toggle_label: &str,
) -> Option<String> {
    let fn_reason = match fn_tap {
        FnTapStatus::Delivering => None,
        FnTapStatus::ArmedNoDelivery if probe_granted => None,
        FnTapStatus::ArmedNoDelivery => Some(crate::permissions::named(
            "macOS is not delivering key events to Yap (Input Monitoring is off)",
        )),
        FnTapStatus::NotAttempted => Some("the fn listener has not started".to_string()),
        FnTapStatus::Blocked(r) => Some(r.clone()),
        FnTapStatus::SecureInput(holder) => Some(format!("{holder} has Secure Keyboard Entry on")),
    };
    match (fn_reason, carbon) {
        (None, CarbonStatus::Failed(e)) => Some(format!(
            "{toggle_label} did not register ({e}). fn still works — pick another chord in Settings → Shortcut."
        )),
        (None, _) => None,
        (Some(r), CarbonStatus::Registered) => Some(format!(
            "fn is off — {r}. Use {toggle_label} (works without permissions)."
        )),
        (Some(r), CarbonStatus::Failed(e)) => Some(format!(
            "fn is off — {r}, and {toggle_label} did not register ({e}). No hotkey can start a take — pick another chord in Settings → Shortcut."
        )),
        (Some(r), CarbonStatus::NotAttempted) => Some(format!(
            "fn is off — {r}, and {toggle_label} is not registered. No hotkey can start a take right now."
        )),
    }
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
/// with no ⌃/⌥/⌘ (it would swallow a bare key system-wide), a modifier-only
/// one (Carbon cannot register it), and any chord another Yap binding in
/// [`shortcuts::ALL`] owns — the dictate branch runs first in the plugin
/// handler, so such a rebind would silently hijack Paste Last, Undo AI edit,
/// Meeting or ⌘⇧V, and its register call at the next launch would make theirs
/// fail.
pub fn chord_from_setting(c: &ChordSetting) -> Result<(Shortcut, String), String> {
    let code = Code::from_str(&c.code).map_err(|_| format!("unknown key code {:?}", c.code))?;
    if MODIFIER_CODES.contains(&code) || matches!(code, Code::Unidentified) {
        return Err("the chord needs a non-modifier key".into());
    }
    if !(c.ctrl || c.alt || c.cmd) {
        return Err("the chord needs ⌃, ⌥ or ⌘".into());
    }
    if let Some(b) = shortcuts::ALL
        .iter()
        .find(|b| b.id != shortcuts::DICTATE_TOGGLE.id && b.chord() == (c.ctrl, c.cmd, c.shift, c.alt, code))
    {
        return Err(format!(
            "{} is already a Yap shortcut ({}) — pick another chord",
            b.label, b.id
        ));
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

/// Record the active dictate chord. A CHANGE re-emits `status` (through the
/// [`on_status_change`] hook) — the chord's label is part of the status every
/// surface reads, and a rebind's Carbon status is Registered → Registered,
/// which [`set_carbon_status`] rightly does not count as a change.
pub fn set_active_dictate(chord: Shortcut, label: String) {
    let next = Some((chord, label));
    let changed = {
        let mut g = ACTIVE_DICTATE.lock();
        let changed = *g != next;
        *g = next;
        changed
    };
    if changed {
        notify();
    }
}

/// The Settings rebind, end to end: [`rebind`] from the active chord to `new`,
/// then make `new` the active chord (which re-emits `status`). On `Err` the
/// old chord stays active and nothing is re-emitted.
pub fn rebind_and_activate(
    registrar: &mut dyn CarbonRegistrar,
    new: Shortcut,
    new_label: String,
    sink: &mut dyn StatusSink,
) -> Result<(), String> {
    let (old, old_label) = active_dictate();
    rebind(registrar, old, &old_label, new, &new_label, sink)?;
    set_active_dictate(new, new_label);
    Ok(())
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
    fn exclusive_refused_accessibility_falls_back_with_a_degraded_line() {
        // yap-1107-hotkey-exclusive-to-yap (5): a refused active tap leaves the
        // listen-only hotkeys working and says so in one sentence + one action.
        let line = super::exclusive_line(super::ExclusiveStatus::ListenOnly)
            .expect("listen-only must carry a degraded line");
        assert!(line.contains("hotkeys work"), "{line}");
        assert!(line.contains("Accessibility"), "{line}");
        assert_eq!(super::exclusive_line(super::ExclusiveStatus::Exclusive), None);
        assert_eq!(super::exclusive_line(super::ExclusiveStatus::NotAttempted), None);
    }

    #[test]
    fn carbon_chord_label_names_only_the_chord() {
        use crate::shortcuts;
        assert_eq!(
            super::carbon_chord_label(&shortcuts::DICTATE_TOGGLE.shortcut()),
            super::chord_label(true, true, false, true, shortcuts::DICTATE_TOGGLE.code)
        );
        for b in shortcuts::ALL {
            let label = super::carbon_chord_label(&b.shortcut());
            assert!(!label.is_empty(), "{}", b.id);
            assert!(!label.contains(' '), "{} label {label:?} must be one token", b.id);
        }
    }

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

    #[test]
    fn rebind_refuses_every_other_yap_chord() {
        let mut refused = 0;
        for b in shortcuts::ALL.iter().filter(|b| b.id != shortcuts::DICTATE_TOGGLE.id) {
            let c = ChordSetting {
                code: format!("{:?}", b.code),
                ctrl: b.ctrl,
                alt: b.alt,
                cmd: b.cmd,
                shift: b.shift,
            };
            let r = chord_from_setting(&c);
            if b.ctrl || b.alt || b.cmd {
                let e = r.expect_err(b.id);
                assert!(e.contains(b.id), "{e}");
                refused += 1;
            } else {
                assert!(r.is_err(), "{} has no ⌃/⌥/⌘ and must be refused anyway", b.id);
            }
        }
        // ⌃⌘V, ⌃⌘Z, ⌘⇧V, ⌃⌘M, and Y8-A's ⌃⌘H, ⌃⌘C, ⌃⌘J (Escape is refused
        // for having no ⌃/⌥/⌘).
        assert_eq!(refused, 7);
    }
}
