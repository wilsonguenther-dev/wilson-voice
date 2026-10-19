//! focus-2026-10-19 — every hotkey action, its key, and whether it works RIGHT
//! NOW, in one table the Settings → Hotkeys rows, the home banner and the
//! support dump all read.
//!
//! Why this exists (yap.log, both channels, 2026-10-19): the fn / fn⌃ hold is a
//! listen-only CGEventTap, and macOS switched it off with
//! `kCGEventTapDisabledByUserInput` dozens of times a day (re-arm #1 … #19 in
//! one morning). When the re-arm's 100 ms verification read the tap still off,
//! `ptt_macos::spawn_re_arm_verifier` logged "re-arm FAILED" and stopped — the
//! stored fn status stayed `Delivering`, so nothing retried, the banner kept
//! saying fn works, and fn presses went nowhere until the next disable. The
//! Carbon dictate chord (⌃⌥⌘Space, no TCC needed, delivered under Secure Input)
//! was registered the whole time, but no surface said "use it now".
//!
//! The table is PURE ([`binding_table`]) so the fallback decision has unit
//! tests; the app feeds it the live statuses (`lib.rs::hotkey_bindings`).

use std::str::FromStr;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

use crate::hotkeys::{chord_label, CarbonStatus, ChordSetting, FnTapStatus};
use crate::shortcuts;

/// Every user-facing hotkey action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingAction {
    /// Hold fn / fn⌃ to talk (the CGEventTap).
    DictateHold,
    /// Double-tap the hold key for hands-free (same tap).
    HandsFree,
    /// The Carbon dictate chord (⌃⌥⌘Space by default): press toggles.
    DictateToggle,
    /// Cancel the take in flight (Escape by default, only while a take runs).
    Cancel,
    /// Start / stop a meeting recording (⌃⌘M by default).
    RecordMeeting,
}

impl BindingAction {
    pub const ALL: [BindingAction; 5] = [
        BindingAction::DictateHold,
        BindingAction::HandsFree,
        BindingAction::DictateToggle,
        BindingAction::Cancel,
        BindingAction::RecordMeeting,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::DictateHold => "Dictate (hold)",
            Self::HandsFree => "Hands-free",
            Self::DictateToggle => "Dictate (toggle)",
            Self::Cancel => "Cancel take",
            Self::RecordMeeting => "Record meeting",
        }
    }
}

/// Whether one binding works right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum BindingStatus {
    /// A key edge was delivered / the register call succeeded.
    Works,
    /// Installed with grants present; no key pressed yet.
    Ready,
    /// The fn tap gets no events: Input Monitoring is off for this bundle.
    NeedsInputMonitoring,
    /// A grant row exists but the tap still cannot install / re-arm: the grant
    /// is bound to an older signature. Remove and re-add.
    StaleGrant,
    /// Another app holds the chord (the register call's error).
    Conflict(String),
    /// Another app has Secure Keyboard Entry on (names it).
    SecureInput(String),
    /// macOS switched the fn listener off and the re-arm has not taken yet.
    Paused,
    /// Registered only while a take is live (Cancel); idle is correct.
    DuringTakeOnly,
    /// Not attempted yet (pre-launch / `--smoke`).
    NotStarted,
}

impl BindingStatus {
    /// Does pressing the key do something right now?
    pub fn usable(&self) -> bool {
        matches!(self, Self::Works | Self::Ready | Self::DuringTakeOnly)
    }

    pub fn chip(&self) -> String {
        match self {
            Self::Works => "Works".into(),
            Self::Ready => "Ready".into(),
            Self::NeedsInputMonitoring => "Needs Input Monitoring".into(),
            Self::StaleGrant => "Stale grant — remove and re-add".into(),
            Self::Conflict(e) => format!("Conflict: {e}"),
            Self::SecureInput(who) => format!("Blocked by {who} (Secure Input)"),
            Self::Paused => "Paused by macOS — re-arming".into(),
            Self::DuringTakeOnly => "Active during a take".into(),
            Self::NotStarted => "Not started".into(),
        }
    }
}

/// One row of the table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingRow {
    pub action: BindingAction,
    pub title: String,
    /// The configured key, glyph form.
    pub key: String,
    pub status: BindingStatus,
    pub chip: String,
    /// The key that performs this action RIGHT NOW: `key` when it works, the
    /// fallback chord when the engine fell back, `None` when nothing does.
    pub live_key: Option<String>,
    /// `true` when `live_key` is a fallback, not the configured key.
    pub fallback: bool,
}

/// Everything the table is computed from. Built from live state by the app;
/// built by hand in tests.
#[derive(Debug, Clone)]
pub struct TableInputs {
    pub fn_tap: FnTapStatus,
    pub dictate_carbon: CarbonStatus,
    pub meeting_carbon: CarbonStatus,
    pub accessibility: bool,
    /// `IOHIDCheckAccess(ListenEvent)` says granted for this binary.
    pub input_monitoring: bool,
    /// The TCC database holds an allowed ListenEvent row for this bundle id
    /// (`None` when it could not be read — no Full Disk Access).
    pub listen_event_row: Option<bool>,
    pub hold_label: String,
    pub toggle_label: String,
    pub cancel_label: String,
    pub meeting_label: String,
}

/// The fn tap's status in table terms. PURE.
///
/// The fn / fn⌃ hold is a listen-only CGEventTap: it needs **Input
/// Monitoring** and nothing else. Accessibility gates insertion (PostEvent /
/// AX), never the tap, so it plays no part here (fix-249 BLOCKING 1: an
/// `|| accessibility` arm read "Ready" on both of Wilson's failing Macs).
///
/// | tap                         | Input Monitoring            | status                  |
/// |-----------------------------|-----------------------------|-------------------------|
/// | delivering                  | any                         | Works                   |
/// | Secure Input                | any                         | SecureInput(app)        |
/// | not attempted               | any                         | NotStarted              |
/// | armed / blocked             | TCC row allowed, live = no  | StaleGrant              |
/// | armed, no edge yet          | granted                     | Ready                   |
/// | armed, no edge yet          | none                        | NeedsInputMonitoring    |
/// | blocked, re-arming          | granted                     | Paused                  |
/// | blocked (install / re-arm failed, or the bounded re-arm gave up) | granted | StaleGrant |
/// | blocked                     | none                        | NeedsInputMonitoring    |
///
/// Every non-usable row falls back to the dictate chord in [`binding_table`].
/// Stale-grant rule: a ListenEvent row the TCC database says is allowed while
/// the live check disagrees, or a granted check while the tap still cannot
/// install / re-arm, is a grant bound to another signature — the only fix is
/// remove + re-add.
pub fn fn_binding_status(i: &TableInputs) -> BindingStatus {
    let stale_row = i.listen_event_row == Some(true) && !i.input_monitoring;
    match &i.fn_tap {
        FnTapStatus::Delivering => BindingStatus::Works,
        FnTapStatus::SecureInput(who) => BindingStatus::SecureInput(who.clone()),
        FnTapStatus::NotAttempted => BindingStatus::NotStarted,
        _ if stale_row => BindingStatus::StaleGrant,
        FnTapStatus::ArmedNoDelivery if i.input_monitoring => BindingStatus::Ready,
        FnTapStatus::ArmedNoDelivery => BindingStatus::NeedsInputMonitoring,
        FnTapStatus::Blocked(_) if !i.input_monitoring => BindingStatus::NeedsInputMonitoring,
        FnTapStatus::Blocked(r) if r.starts_with(PAUSED_PREFIX) => BindingStatus::Paused,
        FnTapStatus::Blocked(_) => BindingStatus::StaleGrant,
    }
}

/// The reason string `ptt_macos` stores while a disabled tap is being re-armed.
pub const PAUSED_PREFIX: &str = "macOS switched the fn listener off";

fn carbon_status(c: &CarbonStatus) -> BindingStatus {
    match c {
        CarbonStatus::Registered => BindingStatus::Works,
        CarbonStatus::Failed(e) => BindingStatus::Conflict(e.clone()),
        CarbonStatus::NotAttempted => BindingStatus::NotStarted,
    }
}

/// THE fallback decision table. PURE.
///
/// | fn tap     | dictate chord | hold/hands-free live key |
/// |------------|---------------|--------------------------|
/// | usable     | any           | the hold key             |
/// | not usable | registered    | the dictate chord (fallback) |
/// | not usable | failed/none   | none — banner says so    |
pub fn binding_table(i: &TableInputs) -> Vec<BindingRow> {
    let fn_status = fn_binding_status(i);
    let toggle_status = carbon_status(&i.dictate_carbon);
    let hold_live = if fn_status.usable() {
        (Some(i.hold_label.clone()), false)
    } else if toggle_status.usable() {
        (Some(i.toggle_label.clone()), true)
    } else {
        (None, false)
    };
    BindingAction::ALL
        .iter()
        .map(|&action| {
            let (key, status, live) = match action {
                BindingAction::DictateHold => {
                    (i.hold_label.clone(), fn_status.clone(), hold_live.clone())
                }
                BindingAction::HandsFree => (
                    format!("double-tap {}", i.hold_label),
                    fn_status.clone(),
                    hold_live.clone(),
                ),
                BindingAction::DictateToggle => {
                    let live = toggle_status.usable().then(|| i.toggle_label.clone());
                    (i.toggle_label.clone(), toggle_status.clone(), (live, false))
                }
                BindingAction::Cancel => (
                    i.cancel_label.clone(),
                    BindingStatus::DuringTakeOnly,
                    (Some(i.cancel_label.clone()), false),
                ),
                BindingAction::RecordMeeting => {
                    let s = carbon_status(&i.meeting_carbon);
                    let live = s.usable().then(|| i.meeting_label.clone());
                    (i.meeting_label.clone(), s, (live, false))
                }
            };
            BindingRow {
                action,
                title: action.title().to_string(),
                key,
                chip: status.chip(),
                status,
                live_key: live.0,
                fallback: live.1,
            }
        })
        .collect()
}

/// The key that starts a take right now, for the pill / home line.
pub fn live_dictate_key(rows: &[BindingRow]) -> Option<String> {
    rows.iter()
        .find(|r| r.action == BindingAction::DictateHold)
        .and_then(|r| r.live_key.clone())
}

/// The debug dump: one aligned line per binding (logged at launch, printed by
/// the acceptance run, carried in the support bundle).
pub fn debug_dump(rows: &[BindingRow]) -> String {
    let mut out = format!(
        "{:<18} {:<20} {:<36} {}\n",
        "ACTION", "KEY", "STATUS", "LIVE KEY"
    );
    for r in rows {
        let live = match (&r.live_key, r.fallback) {
            (Some(k), true) => format!("{k} (fallback)"),
            (Some(k), false) => k.clone(),
            (None, _) => "— none —".to_string(),
        };
        out.push_str(&format!(
            "{:<18} {:<20} {:<36} {}\n",
            r.title, r.key, r.chip, live
        ));
    }
    out
}

// ── parse / format: a chord's glyph label round-trips to its setting ────────

/// Format a chord setting as its glyph label (⌃⌥⇧⌘ then the key).
pub fn format_chord(c: &ChordSetting) -> Result<String, String> {
    let code = Code::from_str(&c.code).map_err(|_| format!("unknown key code {:?}", c.code))?;
    Ok(chord_label(c.ctrl, c.alt, c.shift, c.cmd, code))
}

/// Parse a glyph label back into a chord setting. Accepts what [`format_chord`]
/// writes (`⌃⌥⌘Space`, `⌃⌘M`, `Escape`, `⌘1`).
pub fn parse_chord(label: &str) -> Result<ChordSetting, String> {
    let mut c = ChordSetting::default();
    let mut rest = label.trim();
    loop {
        let mut chars = rest.chars();
        match chars.next() {
            Some('⌃') => c.ctrl = true,
            Some('⌥') => c.alt = true,
            Some('⇧') => c.shift = true,
            Some('⌘') => c.cmd = true,
            _ => break,
        }
        rest = chars.as_str();
    }
    if rest.is_empty() {
        return Err("no key after the modifiers".into());
    }
    let candidates = [
        rest.to_string(),
        format!("Key{rest}"),
        format!("Digit{rest}"),
    ];
    let code = candidates
        .iter()
        .find(|s| Code::from_str(s).is_ok())
        .ok_or_else(|| format!("unknown key {rest:?}"))?;
    c.code = code.clone();
    Ok(c)
}

/// Validate a rebind for `action` and turn it into a chord + label. Meeting
/// needs ⌃/⌥/⌘ (it is held system-wide while idle); Cancel may be a bare key
/// because it is registered only while a take runs. Neither may collide with
/// another Yap chord.
pub fn chord_for_action(
    action: BindingAction,
    c: &ChordSetting,
) -> Result<(Shortcut, String), String> {
    let code = Code::from_str(&c.code).map_err(|_| format!("unknown key code {:?}", c.code))?;
    if matches!(
        code,
        Code::ControlLeft
            | Code::ControlRight
            | Code::AltLeft
            | Code::AltRight
            | Code::MetaLeft
            | Code::MetaRight
            | Code::ShiftLeft
            | Code::ShiftRight
            | Code::Fn
            | Code::CapsLock
            | Code::Unidentified
    ) {
        return Err("the chord needs a non-modifier key".into());
    }
    let own_id = match action {
        BindingAction::RecordMeeting => shortcuts::MEETING_TOGGLE.id,
        BindingAction::Cancel => shortcuts::CANCEL.id,
        _ => return Err("only Record meeting and Cancel take are rebound here".into()),
    };
    if action == BindingAction::RecordMeeting && !(c.ctrl || c.alt || c.cmd) {
        return Err("the chord needs ⌃, ⌥ or ⌘".into());
    }
    let chord = (c.ctrl, c.cmd, c.shift, c.alt, code);
    if let Some(b) = shortcuts::ALL
        .iter()
        .find(|b| b.id != own_id && b.chord() == chord)
    {
        return Err(format!(
            "{} is already a Yap shortcut ({}) — pick another chord",
            b.label, b.id
        ));
    }
    if crate::hotkeys::active_dictate().0 == shortcut_of(c, code) {
        return Err("that chord is your dictate chord — pick another".into());
    }
    Ok((
        shortcut_of(c, code),
        chord_label(c.ctrl, c.alt, c.shift, c.cmd, code),
    ))
}

fn shortcut_of(c: &ChordSetting, code: Code) -> Shortcut {
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
    Shortcut::new(if m.is_empty() { None } else { Some(m) }, code)
}

// ── the active meeting / cancel chords ──────────────────────────────────────

static ACTIVE_MEETING: Mutex<Option<(Shortcut, String)>> = Mutex::new(None);
static ACTIVE_CANCEL: Mutex<Option<(Shortcut, String)>> = Mutex::new(None);
static MEETING_STATUS: Mutex<CarbonStatus> = Mutex::new(CarbonStatus::NotAttempted);

pub fn active_meeting() -> (Shortcut, String) {
    ACTIVE_MEETING.lock().clone().unwrap_or_else(|| {
        (
            shortcuts::MEETING_TOGGLE.shortcut(),
            shortcuts::MEETING_TOGGLE.label.to_string(),
        )
    })
}

pub fn active_cancel() -> (Shortcut, String) {
    ACTIVE_CANCEL.lock().clone().unwrap_or_else(|| {
        (
            shortcuts::CANCEL.shortcut(),
            shortcuts::CANCEL.label.to_string(),
        )
    })
}

pub fn set_active_meeting(v: (Shortcut, String)) {
    *ACTIVE_MEETING.lock() = Some(v);
}

pub fn set_active_cancel(v: (Shortcut, String)) {
    *ACTIVE_CANCEL.lock() = Some(v);
}

pub fn meeting_status() -> CarbonStatus {
    MEETING_STATUS.lock().clone()
}

pub fn set_meeting_status(s: CarbonStatus) {
    *MEETING_STATUS.lock() = s;
}

/// The chord a setting resolves to for `action`, or the built-in default when
/// it is unset or invalid (logged).
pub fn resolve(action: BindingAction, setting: Option<&ChordSetting>) -> (Shortcut, String) {
    if let Some(c) = setting {
        match chord_for_action(action, c) {
            Ok(v) => return v,
            Err(e) => log::warn!(
                "{} rebind ignored ({e}) — using the default",
                action.title()
            ),
        }
    }
    match action {
        BindingAction::Cancel => (
            shortcuts::CANCEL.shortcut(),
            shortcuts::CANCEL.label.to_string(),
        ),
        _ => (
            shortcuts::MEETING_TOGGLE.shortcut(),
            shortcuts::MEETING_TOGGLE.label.to_string(),
        ),
    }
}

/// Swap a Carbon chord atomically: register `new` FIRST, drop `old` only after
/// that succeeded, so a refused chord leaves the old one live. Never call this
/// from inside the global-shortcut plugin's handler (yap-p0-chord-deadlock —
/// the plugin holds its table lock there); `save_settings` runs on a command
/// thread, which is where it is called from.
pub fn swap_chord(
    old: Shortcut,
    new: Shortcut,
    is_registered: &dyn Fn(Shortcut) -> bool,
    register: &mut dyn FnMut(Shortcut) -> Result<(), String>,
    unregister: &mut dyn FnMut(Shortcut) -> Result<(), String>,
) -> Result<(), String> {
    if old == new {
        return Ok(());
    }
    if !is_registered(new) {
        register(new)?;
    }
    if is_registered(old) {
        if let Err(e) = unregister(old) {
            log::warn!("old chord unregister failed: {e}");
        }
    }
    Ok(())
}

/// The system TCC database's ListenEvent (Input Monitoring) row for
/// `bundle_id`: `Some(true)` allowed, `Some(false)` denied/absent, `None` when
/// the database cannot be read (it needs Full Disk Access). Read-only; never
/// writes, never prompts. Used ONLY to tell "off" from "on but bound to an old
/// signature" — the live check stays `IOHIDCheckAccess`.
pub fn tcc_listen_event_row(bundle_id: &str) -> Option<bool> {
    const TCC_DB: &str = "/Library/Application Support/com.apple.TCC/TCC.db";
    if bundle_id.is_empty() {
        return None;
    }
    let conn = rusqlite::Connection::open_with_flags(
        TCC_DB,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let v: Option<i64> = conn
        .query_row(
            "select auth_value from access where service = 'kTCCServiceListenEvent' and client = ?1",
            [bundle_id],
            |r| r.get(0),
        )
        .ok();
    Some(v == Some(2))
}

/// The Permissions → Input Monitoring detail for THIS bundle (`product` is
/// "Yap" or "Yap Test" — each channel has its own grant). `stale` = a grant
/// row exists but the live check / tap disagrees.
pub fn input_monitoring_detail_for(granted: bool, stale: bool, product: &str) -> String {
    if stale {
        return format!(
            "{product} is listed in Input Monitoring, but the grant belongs to an older build and macOS ignores it. \
             Remove {product} from System Settings → Privacy & Security → Input Monitoring (select it, click −), \
             then add it again and reopen {product}."
        );
    }
    if granted {
        format!("Input Monitoring is on for {product} — the fn hold reaches it.")
    } else {
        format!(
            "Input Monitoring is off for {product}, so the fn hold may not reach it. Turn {product} on in \
             System Settings → Privacy & Security → Input Monitoring. Until then the dictate chord works."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn inputs(fn_tap: FnTapStatus, dictate: CarbonStatus) -> TableInputs {
        TableInputs {
            fn_tap,
            dictate_carbon: dictate,
            meeting_carbon: CarbonStatus::Registered,
            accessibility: true,
            input_monitoring: true,
            listen_event_row: None,
            hold_label: "fn⌃".into(),
            toggle_label: "⌃⌥⌘Space".into(),
            cancel_label: "esc".into(),
            meeting_label: "⌃⌘M".into(),
        }
    }

    fn hold(rows: &[BindingRow]) -> &BindingRow {
        rows.iter()
            .find(|r| r.action == BindingAction::DictateHold)
            .unwrap()
    }

    #[test]
    fn fn_working_keeps_the_hold_key_live() {
        let rows = binding_table(&inputs(FnTapStatus::Delivering, CarbonStatus::Registered));
        assert_eq!(hold(&rows).status, BindingStatus::Works);
        assert_eq!(hold(&rows).live_key.as_deref(), Some("fn⌃"));
        assert!(!hold(&rows).fallback);
    }

    #[test]
    fn dead_tap_falls_back_to_the_dictate_chord() {
        for fn_tap in [
            FnTapStatus::Blocked(format!("{PAUSED_PREFIX} — re-arming")),
            FnTapStatus::Blocked("CGEventTapCreate failed".into()),
            FnTapStatus::SecureInput("1Password".into()),
            FnTapStatus::NotAttempted,
        ] {
            let rows = binding_table(&inputs(fn_tap.clone(), CarbonStatus::Registered));
            let h = hold(&rows);
            assert!(!h.status.usable(), "{fn_tap:?}");
            assert_eq!(h.live_key.as_deref(), Some("⌃⌥⌘Space"), "{fn_tap:?}");
            assert!(h.fallback, "{fn_tap:?}");
            assert_eq!(live_dictate_key(&rows).as_deref(), Some("⌃⌥⌘Space"));
        }
    }

    #[test]
    fn dead_tap_and_dead_chord_means_no_live_key() {
        let rows = binding_table(&inputs(
            FnTapStatus::Blocked("x".into()),
            CarbonStatus::Failed("held by Raycast".into()),
        ));
        assert_eq!(hold(&rows).live_key, None);
        let t = rows
            .iter()
            .find(|r| r.action == BindingAction::DictateToggle)
            .unwrap();
        assert_eq!(t.status, BindingStatus::Conflict("held by Raycast".into()));
    }

    #[test]
    fn paused_tap_reads_paused_not_permission() {
        let s = fn_binding_status(&inputs(
            FnTapStatus::Blocked(format!("{PAUSED_PREFIX} — re-arming")),
            CarbonStatus::Registered,
        ));
        assert_eq!(s, BindingStatus::Paused);
    }

    /// fix-249 BLOCKING 1 — the whole fn decision table, one row per case, and
    /// the hold row's live key / fallback for each. Accessibility is varied on
    /// every row and must never change the answer.
    #[test]
    fn fn_decision_table_every_row() {
        let paused = FnTapStatus::Blocked(format!("{PAUSED_PREFIX} — re-arming"));
        let install_failed = FnTapStatus::Blocked("CGEventTapCreate failed".into());
        let gave_up = FnTapStatus::Blocked("macOS kept the fn listener off after 330 re-arm tries".into());
        use BindingStatus as S;
        use FnTapStatus as T;
        // (tap, input_monitoring, listen_event_row, expected, fn⌃ live?)
        let rows: Vec<(FnTapStatus, bool, Option<bool>, BindingStatus, bool)> = vec![
            (T::Delivering, true, Some(true), S::Works, true),
            (T::Delivering, false, None, S::Works, true),
            (T::SecureInput("1Password".into()), true, None, S::SecureInput("1Password".into()), false),
            (T::NotAttempted, true, None, S::NotStarted, false),
            // granted, tap installed, no edge yet → Ready, fn⌃ live
            (T::ArmedNoDelivery, true, Some(true), S::Ready, true),
            (T::ArmedNoDelivery, true, None, S::Ready, true),
            // no Input Monitoring row → Needs Input Monitoring, chord live
            (T::ArmedNoDelivery, false, Some(false), S::NeedsInputMonitoring, false),
            (T::ArmedNoDelivery, false, None, S::NeedsInputMonitoring, false),
            (install_failed.clone(), false, Some(false), S::NeedsInputMonitoring, false),
            (install_failed.clone(), false, None, S::NeedsInputMonitoring, false),
            (paused.clone(), false, None, S::NeedsInputMonitoring, false),
            // TCC row allowed, live check says no → stale, chord live
            (T::ArmedNoDelivery, false, Some(true), S::StaleGrant, false),
            (install_failed.clone(), false, Some(true), S::StaleGrant, false),
            (paused.clone(), false, Some(true), S::StaleGrant, false),
            // granted but the tap cannot install / the bounded re-arm gave up → stale
            (install_failed.clone(), true, None, S::StaleGrant, false),
            (install_failed, true, Some(true), S::StaleGrant, false),
            (gave_up, true, None, S::StaleGrant, false),
            // granted, macOS switched it off and the re-arm is running → Paused
            (paused, true, None, S::Paused, false),
        ];
        for (tap, im, row, want, fn_live) in rows {
            for accessibility in [true, false] {
                let mut i = inputs(tap.clone(), CarbonStatus::Registered);
                i.accessibility = accessibility;
                i.input_monitoring = im;
                i.listen_event_row = row;
                let ctx = format!("{tap:?} im={im} row={row:?} acc={accessibility}");
                assert_eq!(fn_binding_status(&i), want, "{ctx}");
                let table = binding_table(&i);
                let h = hold(&table);
                if fn_live {
                    assert_eq!(h.live_key.as_deref(), Some("fn⌃"), "{ctx}");
                    assert!(!h.fallback, "{ctx}");
                } else {
                    assert_eq!(h.live_key.as_deref(), Some("⌃⌥⌘Space"), "{ctx}");
                    assert!(h.fallback, "{ctx}");
                }
            }
        }
    }

    /// Wilson's two Macs, 2026-10-19: Yap Test (Accessibility on, no
    /// Input Monitoring row) and real Yap (Accessibility on, a stale
    /// Input Monitoring row). Neither may read Ready / fn⌃.
    #[test]
    fn wilsons_two_machines_name_the_chord() {
        let mut yap_test = inputs(FnTapStatus::ArmedNoDelivery, CarbonStatus::Registered);
        yap_test.accessibility = true;
        yap_test.input_monitoring = false;
        yap_test.listen_event_row = Some(false);
        let rows = binding_table(&yap_test);
        assert_eq!(hold(&rows).status, BindingStatus::NeedsInputMonitoring);
        assert_eq!(hold(&rows).live_key.as_deref(), Some("⌃⌥⌘Space"));
        assert!(hold(&rows).fallback);
        assert_eq!(live_dictate_key(&rows).as_deref(), Some("⌃⌥⌘Space"));

        let mut real_yap = yap_test.clone();
        real_yap.listen_event_row = Some(true);
        let rows = binding_table(&real_yap);
        assert_eq!(hold(&rows).status, BindingStatus::StaleGrant);
        assert_eq!(hold(&rows).live_key.as_deref(), Some("⌃⌥⌘Space"));
        assert!(hold(&rows).fallback);
        let hf = rows.iter().find(|r| r.action == BindingAction::HandsFree).unwrap();
        assert_eq!(hf.status, BindingStatus::StaleGrant);
        assert!(hf.fallback);
    }

    #[test]
    fn every_action_has_one_row_and_the_dump_names_them() {
        let rows = binding_table(&inputs(FnTapStatus::Delivering, CarbonStatus::Registered));
        assert_eq!(rows.len(), BindingAction::ALL.len());
        let dump = debug_dump(&rows);
        for a in BindingAction::ALL {
            assert!(dump.contains(a.title()), "{dump}");
        }
        println!("{dump}");
    }

    #[test]
    fn chord_format_parse_roundtrip() {
        for (code, ctrl, alt, shift, cmd) in [
            ("Space", true, true, false, true),
            ("KeyM", true, false, false, true),
            ("Digit1", false, false, false, true),
            ("Escape", false, false, false, false),
            ("KeyD", true, true, true, true),
            ("F5", false, true, false, false),
        ] {
            let c = ChordSetting {
                code: code.into(),
                ctrl,
                alt,
                shift,
                cmd,
            };
            let label = format_chord(&c).unwrap();
            assert_eq!(parse_chord(&label).unwrap(), c, "{label}");
        }
        assert_eq!(
            format_chord(&ChordSetting {
                code: "Space".into(),
                ctrl: true,
                alt: true,
                shift: false,
                cmd: true
            })
            .unwrap(),
            "⌃⌥⌘Space"
        );
        assert!(parse_chord("⌃⌘").is_err());
        assert!(parse_chord("⌃Nope").is_err());
    }

    #[test]
    fn input_monitoring_detail_names_the_bundle_and_the_stale_fix() {
        let d = input_monitoring_detail_for(true, true, "Yap Test");
        assert!(
            d.contains("Remove Yap Test from") && d.contains("add it again"),
            "{d}"
        );
        assert!(input_monitoring_detail_for(false, false, "Yap Test").contains("off for Yap Test"));
        assert!(input_monitoring_detail_for(true, false, "Yap").contains("on for Yap"));
    }

    #[test]
    fn action_validation() {
        let bare_m = ChordSetting {
            code: "KeyM".into(),
            ..Default::default()
        };
        assert!(chord_for_action(BindingAction::RecordMeeting, &bare_m).is_err());
        let ok = ChordSetting {
            code: "KeyR".into(),
            ctrl: true,
            cmd: true,
            ..Default::default()
        };
        assert_eq!(
            chord_for_action(BindingAction::RecordMeeting, &ok)
                .unwrap()
                .1,
            "⌃⌘R"
        );
        // Cancel may be bare — it is only registered during a take.
        let f = ChordSetting {
            code: "F6".into(),
            ..Default::default()
        };
        assert_eq!(chord_for_action(BindingAction::Cancel, &f).unwrap().1, "F6");
        // Collides with ⌃⌘V paste-last.
        let v = ChordSetting {
            code: "KeyV".into(),
            ctrl: true,
            cmd: true,
            ..Default::default()
        };
        assert!(chord_for_action(BindingAction::Cancel, &v).is_err());
        let fnk = ChordSetting {
            code: "Fn".into(),
            ctrl: true,
            ..Default::default()
        };
        assert!(chord_for_action(BindingAction::RecordMeeting, &fnk).is_err());
    }

    #[test]
    fn swap_registers_new_before_dropping_old_and_keeps_old_on_refusal() {
        let reg: RefCell<Vec<Shortcut>> = RefCell::new(vec![shortcuts::MEETING_TOGGLE.shortcut()]);
        let new = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SUPER), Code::KeyR);
        let order = RefCell::new(Vec::new());
        swap_chord(
            shortcuts::MEETING_TOGGLE.shortcut(),
            new,
            &|s| reg.borrow().contains(&s),
            &mut |s| {
                order.borrow_mut().push("register");
                reg.borrow_mut().push(s);
                Ok(())
            },
            &mut |s| {
                order.borrow_mut().push("unregister");
                reg.borrow_mut().retain(|x| *x != s);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*order.borrow(), vec!["register", "unregister"]);
        assert_eq!(*reg.borrow(), vec![new]);
        // Refused: old stays.
        let r = swap_chord(
            new,
            shortcuts::MEETING_TOGGLE.shortcut(),
            &|s| reg.borrow().contains(&s),
            &mut |_| Err("held by another app".into()),
            &mut |_| panic!("must not unregister after a refusal"),
        );
        assert!(r.is_err());
        assert_eq!(*reg.borrow(), vec![new]);
    }
}
