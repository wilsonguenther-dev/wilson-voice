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
    /// Y8-A — the Carbon hands-free toggle (⌃⌘H by default).
    HandsFreeToggle,
    /// Y8-A — copy the newest transcript, no paste (⌃⌘C by default).
    CopyLast,
    /// Y8-A — bring Yap forward on the Scratchpad (⌃⌘J by default).
    OpenScratchpad,
}

impl BindingAction {
    pub const ALL: [BindingAction; 8] = [
        BindingAction::DictateHold,
        BindingAction::HandsFree,
        BindingAction::DictateToggle,
        BindingAction::Cancel,
        BindingAction::RecordMeeting,
        BindingAction::HandsFreeToggle,
        BindingAction::CopyLast,
        BindingAction::OpenScratchpad,
    ];

    /// The rebindable chords Yap holds system-wide from launch to quit (every
    /// rebindable Carbon action but the dictate toggle, which has its own
    /// path, and Cancel, which is live only during a take). `save_settings`
    /// swaps each of these the same way.
    pub const ALWAYS_GLOBAL_REBINDABLE: [BindingAction; 4] = [
        BindingAction::RecordMeeting,
        BindingAction::HandsFreeToggle,
        BindingAction::CopyLast,
        BindingAction::OpenScratchpad,
    ];

    /// The table binding that is this action's built-in default, for the
    /// Carbon actions that have one.
    pub fn default_binding(self) -> Option<&'static shortcuts::Binding> {
        match self {
            Self::DictateToggle => Some(&shortcuts::DICTATE_TOGGLE),
            Self::Cancel => Some(&shortcuts::CANCEL),
            Self::RecordMeeting => Some(&shortcuts::MEETING_TOGGLE),
            Self::HandsFreeToggle => Some(&shortcuts::HANDS_FREE),
            Self::CopyLast => Some(&shortcuts::COPY_LAST),
            Self::OpenScratchpad => Some(&shortcuts::OPEN_SCRATCHPAD),
            Self::DictateHold | Self::HandsFree => None,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::DictateHold => "Dictate (hold)",
            Self::HandsFree => "Hands-free",
            Self::DictateToggle => "Dictate (toggle)",
            Self::Cancel => "Cancel take",
            Self::RecordMeeting => "Record meeting",
            Self::HandsFreeToggle => "Hands-free (toggle)",
            Self::CopyLast => "Copy last transcript",
            Self::OpenScratchpad => "Open Scratchpad",
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
    /// The live check says Input Monitoring is off and the TCC database could
    /// not be read (no Full Disk Access), so "off" and "on but bound to an old
    /// signature" cannot be told apart — the chip carries both fixes.
    NeedsInputMonitoringUnverified,
    /// The live check says Input Monitoring is off and the TCC database query
    /// FAILED (not "no row"): the error is shown, never read as "off".
    GrantCheckFailed(String),
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
            Self::NeedsInputMonitoringUnverified => {
                "Needs Input Monitoring — if it is already on, remove and re-add it".into()
            }
            Self::GrantCheckFailed(e) => format!(
                "Grant check failed ({e}) — turn on Input Monitoring, or remove and re-add it"
            ),
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
    /// Why the STORED chord for this action is not the one Yap holds (launch
    /// re-validation, [`stored_chord_refusal`]); `None` when it is.
    pub note: Option<String>,
}

/// What the system TCC database says about this bundle's ListenEvent (Input
/// Monitoring) row. Only used to tell "off" from "on but stale".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TccRow {
    /// An allowed row (`auth_value` 2).
    Allowed,
    /// A denied row, or no row at all.
    NotAllowed,
    /// The database cannot be opened by this process (it needs Full Disk
    /// Access), or there is no bundle id to look up.
    Unreadable,
    /// The database opened but the query failed — an error, never "no row".
    QueryError(String),
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
    /// The TCC database's ListenEvent row for this bundle id.
    pub listen_event_row: TccRow,
    pub hold_label: String,
    pub toggle_label: String,
    pub cancel_label: String,
    pub meeting_label: String,
    /// Y8-A — the always-global chords beyond Record meeting: each action's
    /// live label and its register result.
    pub extra_chords: Vec<(BindingAction, String, CarbonStatus)>,
    /// Stored chords launch re-validation refused, with the reason
    /// ([`stored_chord_refusal`]); shown on that action's row.
    pub refusals: Vec<(BindingAction, String)>,
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
/// "none" splits on the TCC row: no row / denied → NeedsInputMonitoring; the
/// database unreadable (no Full Disk Access) → NeedsInputMonitoringUnverified
/// (carries the remove + re-add hint, since a stale grant looks the same from
/// here); the query failed → GrantCheckFailed(error).
///
/// Every non-usable row falls back to the dictate chord in [`binding_table`].
/// Stale-grant rule: a ListenEvent row the TCC database says is allowed while
/// the live check disagrees, or a granted check while the tap still cannot
/// install / re-arm, is a grant bound to another signature — the only fix is
/// remove + re-add.
pub fn fn_binding_status(i: &TableInputs) -> BindingStatus {
    let stale_row = i.listen_event_row == TccRow::Allowed && !i.input_monitoring;
    match &i.fn_tap {
        FnTapStatus::Delivering => BindingStatus::Works,
        FnTapStatus::SecureInput(who) => BindingStatus::SecureInput(who.clone()),
        FnTapStatus::NotAttempted => BindingStatus::NotStarted,
        _ if stale_row => BindingStatus::StaleGrant,
        FnTapStatus::ArmedNoDelivery if i.input_monitoring => BindingStatus::Ready,
        FnTapStatus::Blocked(r) if i.input_monitoring && r.starts_with(PAUSED_PREFIX) => {
            BindingStatus::Paused
        }
        FnTapStatus::Blocked(_) if i.input_monitoring => BindingStatus::StaleGrant,
        // The live check says off: the TCC row tells "off" from "can't tell".
        _ => match &i.listen_event_row {
            TccRow::QueryError(e) => BindingStatus::GrantCheckFailed(e.clone()),
            TccRow::Unreadable => BindingStatus::NeedsInputMonitoringUnverified,
            TccRow::Allowed | TccRow::NotAllowed => BindingStatus::NeedsInputMonitoring,
        },
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
                BindingAction::HandsFreeToggle
                | BindingAction::CopyLast
                | BindingAction::OpenScratchpad => {
                    let (label, s) = i
                        .extra_chords
                        .iter()
                        .find(|(a, ..)| *a == action)
                        .map(|(_, l, c)| (l.clone(), carbon_status(c)))
                        .unwrap_or_else(|| {
                            (
                                action
                                    .default_binding()
                                    .map(|b| b.label.to_string())
                                    .unwrap_or_default(),
                                BindingStatus::NotStarted,
                            )
                        });
                    let live = s.usable().then(|| label.clone());
                    (label, s, (live, false))
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
                note: i
                    .refusals
                    .iter()
                    .find(|(a, _)| *a == action)
                    .map(|(_, why)| why.clone()),
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
        let note = r
            .note
            .as_deref()
            .map(|n| format!(" — {n}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "{:<18} {:<20} {:<36} {}{}\n",
            r.title, r.key, r.chip, live, note
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

/// macOS / system chords a rebind may never take (fix-249 BLOCKING 2): Yap
/// registers its chords system-wide, so one stray press in the recorder would
/// otherwise kill ⌘C / ⌘Q in every app or steal the lock-screen chord.
/// `(ctrl, alt, shift, cmd, code, what it does)`. The frontend reads the same
/// list from `desktop/src/reservedChords.json` (imported by
/// `hotkeyRecorder.ts`); test `reserved_chords_parity` fails when this table
/// and that file drift, and both sides run the shared case table
/// `desktop/src/reservedChordCases.json`.
pub const RESERVED_CHORDS: &[(bool, bool, bool, bool, &str, &str)] = &[
    (false, false, false, true, "KeyQ", "Quit"),
    (false, false, false, true, "KeyW", "Close Window"),
    (false, false, false, true, "KeyC", "Copy"),
    (false, false, false, true, "KeyV", "Paste"),
    (false, false, false, true, "KeyX", "Cut"),
    (false, false, false, true, "KeyZ", "Undo"),
    (false, false, false, true, "KeyA", "Select All"),
    (false, false, false, true, "KeyS", "Save"),
    (false, false, false, true, "KeyH", "Hide"),
    (false, false, false, true, "KeyM", "Minimize"),
    (false, false, false, true, "Tab", "the app switcher"),
    (false, false, false, true, "Space", "Spotlight"),
    (false, false, false, true, "Backquote", "next window"),
    (false, false, true, true, "KeyQ", "Log Out"),
    (false, false, true, true, "Digit3", "screenshot"),
    (false, false, true, true, "Digit4", "screenshot selection"),
    (false, false, true, true, "Digit5", "the screenshot toolbar"),
    (
        true,
        false,
        true,
        true,
        "Digit3",
        "screenshot to the clipboard",
    ),
    (
        true,
        false,
        true,
        true,
        "Digit4",
        "screenshot selection to the clipboard",
    ),
    (true, false, false, true, "KeyQ", "Lock Screen"),
    (true, false, false, true, "KeyF", "full screen"),
    (true, false, false, true, "Space", "the emoji picker"),
    (true, false, false, true, "KeyD", "Look Up"),
    (false, true, false, true, "Escape", "Force Quit"),
    (false, true, false, true, "KeyD", "show / hide the Dock"),
    (false, true, false, true, "Space", "Finder search"),
    (false, true, false, true, "KeyH", "Hide Others"),
    (false, true, false, true, "KeyM", "Minimize All"),
    (false, true, false, true, "KeyW", "Close All"),
    (true, false, false, false, "Space", "input-source switching"),
    (true, true, false, false, "Space", "input-source switching"),
    (true, false, false, false, "ArrowUp", "Mission Control"),
    (true, false, false, false, "ArrowDown", "App Exposé"),
    (true, false, false, false, "ArrowLeft", "the previous Space"),
    (true, false, false, false, "ArrowRight", "the next Space"),
];

fn bare_cancel_key_ok(code: &str) -> bool {
    code == "Escape"
        || code
            .strip_prefix('F')
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=20).contains(&n))
}

/// Why `c` may not be rebound to, or `None` when it may. Rules: never a
/// [`RESERVED_CHORDS`] entry; never ⌘ (or ⌘⇧) as the only modifier — that is
/// every app's menu shortcut space; never no modifier or only ⇧ — except the
/// Cancel key (`allow_bare`), which is registered only while a take runs and
/// may be bare Esc or F1–F20.
pub fn reserved_chord_reason(c: &ChordSetting, allow_bare: bool) -> Option<String> {
    let label = Code::from_str(&c.code)
        .map(|code| chord_label(c.ctrl, c.alt, c.shift, c.cmd, code))
        .unwrap_or_else(|_| c.code.clone());
    if let Some((.., what)) = RESERVED_CHORDS.iter().find(|(ctrl, alt, shift, cmd, code, _)| {
        (*ctrl, *alt, *shift, *cmd) == (c.ctrl, c.alt, c.shift, c.cmd) && *code == c.code
    }) {
        return Some(format!("{label} is macOS's {what} — pick another chord"));
    }
    if !(c.ctrl || c.alt || c.cmd) {
        if allow_bare && !c.shift && bare_cancel_key_ok(&c.code) {
            return None;
        }
        return Some(if allow_bare {
            format!("{label} would stop typing that key in every app during a take — use Esc, an F-key, or add ⌃, ⌥ or ⌘")
        } else {
            format!("{label} needs ⌃, ⌥ or ⌘ — Yap holds it in every app")
        });
    }
    if c.cmd && !c.ctrl && !c.alt {
        return Some(format!(
            "{label} is an app menu shortcut in every app — add ⌃ or ⌥"
        ));
    }
    None
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
        BindingAction::RecordMeeting
        | BindingAction::Cancel
        | BindingAction::HandsFreeToggle
        | BindingAction::CopyLast
        | BindingAction::OpenScratchpad => action.default_binding().map(|b| b.id),
        _ => None,
    };
    let Some(own_id) = own_id else {
        return Err("the hold keys and the dictate toggle are not rebound here".into());
    };
    // A macOS chord is named first — "⌃⇧⌘3 is macOS's screenshot" says more
    // than "4 keys".
    if let Some(why) = reserved_chord_reason(c, action == BindingAction::Cancel) {
        return Err(why);
    }
    // Y8-A — then Wispr's rules (at most 3 keys, a modifier, no left/right
    // duplicate); the Cancel key alone may be bare.
    shortcuts::validate::refuse(
        &shortcuts::validate::keys_of_chord(c.ctrl, c.alt, c.shift, c.cmd, code),
        action == BindingAction::Cancel,
    )
    .map_err(|v| v.to_string())?;
    // fix-249 advisory (d): only the FIXED Yap chords block here. The three
    // rebindable ones (dictate toggle, Record meeting, Cancel) are checked
    // against each other's NEW values by [`check_distinct`], so ⌃⌘M is free
    // for Cancel once Record meeting moved off it.
    const REBINDABLE: [&str; 6] = [
        shortcuts::DICTATE_TOGGLE.id,
        shortcuts::MEETING_TOGGLE.id,
        shortcuts::CANCEL.id,
        shortcuts::HANDS_FREE.id,
        shortcuts::COPY_LAST.id,
        shortcuts::OPEN_SCRATCHPAD.id,
    ];
    let _ = own_id;
    let chord = (c.ctrl, c.cmd, c.shift, c.alt, code);
    if let Some(b) = shortcuts::ALL
        .iter()
        .find(|b| !REBINDABLE.contains(&b.id) && b.chord() == chord)
    {
        return Err(format!(
            "{} is already a Yap shortcut ({}) — pick another chord",
            b.label, b.id
        ));
    }
    Ok((
        shortcut_of(c, code),
        chord_label(c.ctrl, c.alt, c.shift, c.cmd, code),
    ))
}

/// fix-249 advisory (d): no two actions on one chord. `chords` are the
/// rebindable actions' NEW chords (dictate toggle, Record meeting, Cancel);
/// a clash is refused with both names, before anything is saved or registered.
pub fn check_distinct(chords: &[(BindingAction, Shortcut, String)]) -> Result<(), String> {
    for (i, (a, sa, label)) in chords.iter().enumerate() {
        if let Some((b, ..)) = chords[i + 1..].iter().find(|(_, sb, _)| sb == sa) {
            return Err(format!(
                "{label} would be both {} and {} — pick a different chord for one of them",
                a.title(),
                b.title()
            ));
        }
    }
    Ok(())
}

/// fix-249 advisory (e): the tray item's accelerator for the Record meeting
/// chord a setting resolves to (muda tokens: `Ctrl+Alt+Shift+Cmd+KEYR`), so
/// the menu shows the LIVE chord, not the literal ⌃⌘M. PURE.
pub fn meeting_accelerator(setting: Option<&ChordSetting>) -> String {
    match setting {
        Some(c) if chord_for_action(BindingAction::RecordMeeting, c).is_ok() => {
            let mut parts: Vec<&str> = Vec::new();
            if c.ctrl {
                parts.push("Ctrl");
            }
            if c.alt {
                parts.push("Alt");
            }
            if c.shift {
                parts.push("Shift");
            }
            if c.cmd {
                parts.push("Cmd");
            }
            parts.push(&c.code);
            parts.join("+")
        }
        _ => shortcuts::MEETING_TOGGLE.accelerator.to_string(),
    }
}

/// The meeting-start notification body, naming the live chord.
pub fn meeting_started_body(live_label: &str) -> String {
    format!("Stop with {live_label} or the menu bar.")
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

/// fix-249 advisory (c): the cancel chord that is ACTUALLY registered right
/// now. A rebind during a take changes `active_cancel` but not this, so the
/// release always drops the key the take armed — Esc can never stay
/// registered system-wide while idle.
static REGISTERED_CANCEL: Mutex<Option<Shortcut>> = Mutex::new(None);

/// The chord a (re-)register should use: the one already armed this take, else
/// the active one. PURE.
pub fn cancel_for_register(slot: &Option<Shortcut>, active: Shortcut) -> Shortcut {
    slot.unwrap_or(active)
}

/// The chord a release must drop (and forget). PURE.
pub fn cancel_for_release(slot: &mut Option<Shortcut>, active: Shortcut) -> Shortcut {
    slot.take().unwrap_or(active)
}

pub fn cancel_to_register() -> Shortcut {
    cancel_for_register(&REGISTERED_CANCEL.lock(), active_cancel().0)
}

pub fn note_cancel_registered(sc: Shortcut) {
    *REGISTERED_CANCEL.lock() = Some(sc);
}

pub fn cancel_to_release() -> Shortcut {
    cancel_for_release(&mut REGISTERED_CANCEL.lock(), active_cancel().0)
}

pub fn meeting_status() -> CarbonStatus {
    MEETING_STATUS.lock().clone()
}

pub fn set_meeting_status(s: CarbonStatus) {
    *MEETING_STATUS.lock() = s;
}

/// Y8-A — the live chord + register status of the always-global actions
/// beyond Record meeting (hands-free toggle, copy last, open Scratchpad).
type ActiveChord = (BindingAction, (Shortcut, String), CarbonStatus);
static ACTIVE_EXTRA: Mutex<Vec<ActiveChord>> = Mutex::new(Vec::new());

/// The chord `action` holds right now (its default until launch resolves it).
/// Record meeting and Cancel read their own slots.
pub fn active_chord(action: BindingAction) -> (Shortcut, String) {
    match action {
        BindingAction::RecordMeeting => active_meeting(),
        BindingAction::Cancel => active_cancel(),
        _ => ACTIVE_EXTRA
            .lock()
            .iter()
            .find(|(a, ..)| *a == action)
            .map(|(_, v, _)| v.clone())
            .unwrap_or_else(|| resolve(action, None)),
    }
}

pub fn set_active_chord(action: BindingAction, v: (Shortcut, String)) {
    match action {
        BindingAction::RecordMeeting => set_active_meeting(v),
        BindingAction::Cancel => set_active_cancel(v),
        _ => {
            let mut slots = ACTIVE_EXTRA.lock();
            match slots.iter_mut().find(|(a, ..)| *a == action) {
                Some(slot) => slot.1 = v,
                None => slots.push((action, v, CarbonStatus::NotAttempted)),
            }
        }
    }
}

pub fn chord_status(action: BindingAction) -> CarbonStatus {
    match action {
        BindingAction::RecordMeeting => meeting_status(),
        _ => ACTIVE_EXTRA
            .lock()
            .iter()
            .find(|(a, ..)| *a == action)
            .map(|(.., s)| s.clone())
            .unwrap_or(CarbonStatus::NotAttempted),
    }
}

pub fn set_chord_status(action: BindingAction, s: CarbonStatus) {
    match action {
        BindingAction::RecordMeeting => set_meeting_status(s),
        _ => {
            let fallback = resolve(action, None);
            let mut slots = ACTIVE_EXTRA.lock();
            match slots.iter_mut().find(|(a, ..)| *a == action) {
                Some(slot) => slot.2 = s,
                None => slots.push((action, fallback, s)),
            }
        }
    }
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
    let b = action
        .default_binding()
        .unwrap_or(&shortcuts::MEETING_TOGGLE);
    (b.shortcut(), b.label.to_string())
}

/// fix-249 advisory 2 — why the STORED chord for `action` is not the one Yap
/// holds, or `None` when it is (or nothing is stored). Stored chords are
/// re-validated at every launch, not only at save: a chord saved before a rule
/// existed (a newly reserved macOS chord) or edited by hand in settings.json
/// falls back to the default, and this sentence is what its row shows.
pub fn stored_chord_refusal(
    action: BindingAction,
    setting: Option<&ChordSetting>,
) -> Option<String> {
    let c = setting?;
    let (why, default) = match action {
        BindingAction::DictateToggle => (
            crate::hotkeys::chord_from_setting(c)
                .err()
                .or_else(|| reserved_chord_reason(c, false))?,
            shortcuts::DICTATE_TOGGLE.label,
        ),
        BindingAction::RecordMeeting => (
            chord_for_action(action, c).err()?,
            shortcuts::MEETING_TOGGLE.label,
        ),
        BindingAction::Cancel => (chord_for_action(action, c).err()?, shortcuts::CANCEL.label),
        BindingAction::HandsFreeToggle
        | BindingAction::CopyLast
        | BindingAction::OpenScratchpad => (
            chord_for_action(action, c).err()?,
            action.default_binding()?.label,
        ),
        BindingAction::DictateHold | BindingAction::HandsFree => return None,
    };
    Some(format!(
        "Your saved chord is not used: {why}. Yap is using {default} until you record another."
    ))
}

/// The dictate chord Yap registers at launch and on every rebind: the stored
/// chord when it passes EVERY rule (including [`RESERVED_CHORDS`]), else
/// `shortcuts::DICTATE_TOGGLE`.
pub fn resolve_dictate(setting: Option<&ChordSetting>) -> (Shortcut, String) {
    if let Some(c) = setting {
        match stored_chord_refusal(BindingAction::DictateToggle, Some(c)) {
            Some(why) => log::warn!("dictate toggle: {why}"),
            None => match crate::hotkeys::chord_from_setting(c) {
                Ok(v) => return v,
                Err(e) => log::warn!("dictate toggle rebind ignored ({e}) — using the default"),
            },
        }
    }
    (
        shortcuts::DICTATE_TOGGLE.shortcut(),
        shortcuts::DICTATE_TOGGLE.label.to_string(),
    )
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
/// `bundle_id`. Read-only; never writes, never prompts. Used ONLY to tell "off"
/// from "on but bound to an old signature" — the live check stays
/// `IOHIDCheckAccess`. A failed query is [`TccRow::QueryError`], never "no row".
pub fn tcc_listen_event_row(bundle_id: &str) -> TccRow {
    tcc_listen_event_row_at(
        std::path::Path::new("/Library/Application Support/com.apple.TCC/TCC.db"),
        bundle_id,
    )
}

/// [`tcc_listen_event_row`] against any database file (tests use a fixture).
pub fn tcc_listen_event_row_at(db: &std::path::Path, bundle_id: &str) -> TccRow {
    if bundle_id.is_empty() {
        return TccRow::Unreadable;
    }
    let conn = match rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(e) => return classify_tcc_error(&e),
    };
    match conn.query_row(
        "select auth_value from access where service = 'kTCCServiceListenEvent' and client = ?1",
        [bundle_id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(2) => TccRow::Allowed,
        Ok(_) => TccRow::NotAllowed,
        Err(e) => classify_tcc_error(&e),
    }
}

/// No row → [`TccRow::NotAllowed`]; cannot open / not authorised (no Full
/// Disk Access) → [`TccRow::Unreadable`]; anything else → the error itself.
fn classify_tcc_error(e: &rusqlite::Error) -> TccRow {
    use rusqlite::ErrorCode as C;
    match e {
        rusqlite::Error::QueryReturnedNoRows => TccRow::NotAllowed,
        rusqlite::Error::SqliteFailure(f, _)
            if matches!(
                f.code,
                C::CannotOpen | C::PermissionDenied | C::AuthorizationForStatementDenied
            ) =>
        {
            TccRow::Unreadable
        }
        other => TccRow::QueryError(other.to_string()),
    }
}

/// The sentence the Input Monitoring row adds when the live check says off but
/// the TCC row cannot settle "off" vs "stale": unreadable (no Full Disk
/// Access) → the remove + re-add hint; a failed query → the error AND the hint.
pub fn input_monitoring_unverified_hint(
    granted: bool,
    row: &TccRow,
    product: &str,
) -> Option<String> {
    if granted {
        return None;
    }
    let fix = format!(
        "If {product} is already listed in Input Monitoring, remove it (select it, click −), add it again and reopen {product} — a grant from an older build looks the same as no grant from here."
    );
    match row {
        TccRow::Unreadable => Some(fix),
        TccRow::QueryError(e) => Some(format!(
            "{product} could not read the permission database ({e}). {fix}"
        )),
        TccRow::Allowed | TccRow::NotAllowed => None,
    }
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
            listen_event_row: TccRow::Unreadable,
            hold_label: "fn⌃".into(),
            toggle_label: "⌃⌥⌘Space".into(),
            cancel_label: "esc".into(),
            meeting_label: "⌃⌘M".into(),
            extra_chords: Vec::new(),
            refusals: vec![],
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
        use TccRow as R;
        let qerr = || R::QueryError("database disk image is malformed".into());
        let failed = || S::GrantCheckFailed("database disk image is malformed".into());
        // (tap, input_monitoring, listen_event_row, expected, fn⌃ live?)
        let rows: Vec<(FnTapStatus, bool, TccRow, BindingStatus, bool)> = vec![
            (T::Delivering, true, R::Allowed, S::Works, true),
            (T::Delivering, false, R::Unreadable, S::Works, true),
            (T::Delivering, false, qerr(), S::Works, true),
            (
                T::SecureInput("1Password".into()),
                true,
                R::Unreadable,
                S::SecureInput("1Password".into()),
                false,
            ),
            (T::NotAttempted, true, R::Unreadable, S::NotStarted, false),
            // granted, tap installed, no edge yet → Ready, fn⌃ live
            (T::ArmedNoDelivery, true, R::Allowed, S::Ready, true),
            (T::ArmedNoDelivery, true, R::Unreadable, S::Ready, true),
            (T::ArmedNoDelivery, true, qerr(), S::Ready, true),
            // no Input Monitoring row → Needs Input Monitoring, chord live
            (
                T::ArmedNoDelivery,
                false,
                R::NotAllowed,
                S::NeedsInputMonitoring,
                false,
            ),
            (
                install_failed.clone(),
                false,
                R::NotAllowed,
                S::NeedsInputMonitoring,
                false,
            ),
            (
                paused.clone(),
                false,
                R::NotAllowed,
                S::NeedsInputMonitoring,
                false,
            ),
            // advisory 4: no Full Disk Access → can't tell off from stale → the hint
            (
                T::ArmedNoDelivery,
                false,
                R::Unreadable,
                S::NeedsInputMonitoringUnverified,
                false,
            ),
            (
                install_failed.clone(),
                false,
                R::Unreadable,
                S::NeedsInputMonitoringUnverified,
                false,
            ),
            (
                paused.clone(),
                false,
                R::Unreadable,
                S::NeedsInputMonitoringUnverified,
                false,
            ),
            // advisory 5: a failed TCC query is an error state, never "no row"
            (T::ArmedNoDelivery, false, qerr(), failed(), false),
            (install_failed.clone(), false, qerr(), failed(), false),
            (paused.clone(), false, qerr(), failed(), false),
            // TCC row allowed, live check says no → stale, chord live
            (T::ArmedNoDelivery, false, R::Allowed, S::StaleGrant, false),
            (
                install_failed.clone(),
                false,
                R::Allowed,
                S::StaleGrant,
                false,
            ),
            (paused.clone(), false, R::Allowed, S::StaleGrant, false),
            // granted but the tap cannot install / the bounded re-arm gave up → stale
            (
                install_failed.clone(),
                true,
                R::Unreadable,
                S::StaleGrant,
                false,
            ),
            (install_failed, true, R::Allowed, S::StaleGrant, false),
            (gave_up, true, R::Unreadable, S::StaleGrant, false),
            // granted, macOS switched it off and the re-arm is running → Paused
            (paused.clone(), true, R::Unreadable, S::Paused, false),
            (paused, true, qerr(), S::Paused, false),
        ];
        for (tap, im, row, want, fn_live) in rows {
            for accessibility in [true, false] {
                let mut i = inputs(tap.clone(), CarbonStatus::Registered);
                i.accessibility = accessibility;
                i.input_monitoring = im;
                i.listen_event_row = row.clone();
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
        yap_test.listen_event_row = TccRow::NotAllowed;
        let rows = binding_table(&yap_test);
        assert_eq!(hold(&rows).status, BindingStatus::NeedsInputMonitoring);
        assert_eq!(hold(&rows).live_key.as_deref(), Some("⌃⌥⌘Space"));
        assert!(hold(&rows).fallback);
        assert_eq!(live_dictate_key(&rows).as_deref(), Some("⌃⌥⌘Space"));

        let mut real_yap = yap_test.clone();
        real_yap.listen_event_row = TccRow::Allowed;
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

    /// fix-249 BLOCKING 2 — every reserved / system chord is refused for both
    /// rebindable actions, with the reason; ordinary chords still pass.
    #[test]
    fn reserved_and_system_chords_are_refused() {
        let ch = |code: &str, ctrl: bool, alt: bool, shift: bool, cmd: bool| ChordSetting {
            code: code.into(),
            ctrl,
            alt,
            shift,
            cmd,
        };
        for &(ctrl, alt, shift, cmd, code, what) in RESERVED_CHORDS {
            let c = ch(code, ctrl, alt, shift, cmd);
            for action in [BindingAction::RecordMeeting, BindingAction::Cancel] {
                let e = chord_for_action(action, &c).unwrap_err();
                assert!(e.contains(what), "{action:?} {c:?}: {e}");
            }
        }
        // The named minimum list, spelled out.
        for c in [
            ch("KeyQ", false, false, false, true),
            ch("KeyW", false, false, false, true),
            ch("KeyC", false, false, false, true),
            ch("KeyV", false, false, false, true),
            ch("KeyX", false, false, false, true),
            ch("KeyZ", false, false, false, true),
            ch("KeyA", false, false, false, true),
            ch("KeyS", false, false, false, true),
            ch("Tab", false, false, false, true),
            ch("Space", false, false, false, true),
            ch("KeyH", false, false, false, true),
            ch("KeyM", false, false, false, true),
            ch("KeyQ", true, false, false, true),
            ch("Escape", false, true, false, true),
            ch("Backquote", false, false, false, true),
            // any ⌘-only chord, no modifier, only ⇧
            ch("KeyN", false, false, false, true),
            ch("KeyN", false, false, true, true),
            ch("KeyR", false, false, false, false),
            ch("KeyR", false, false, true, false),
            ch("Space", false, false, false, false),
        ] {
            assert!(
                chord_for_action(BindingAction::RecordMeeting, &c).is_err(),
                "meeting {c:?}"
            );
            assert!(chord_for_action(BindingAction::Cancel, &c).is_err(), "cancel {c:?}");
        }
        // Cancel: bare Esc / F-keys only; ⇧Esc and bare letters refused.
        assert!(chord_for_action(BindingAction::Cancel, &ch("Escape", false, false, false, false)).is_ok());
        assert!(chord_for_action(BindingAction::Cancel, &ch("F13", false, false, false, false)).is_ok());
        assert!(chord_for_action(BindingAction::Cancel, &ch("Escape", false, false, true, false)).is_err());
        assert!(chord_for_action(BindingAction::RecordMeeting, &ch("F13", false, false, false, false)).is_err());
        // Ordinary chords pass.
        // Y8-A: ⌃⌥⌘R is four keys — refused by the at-most-3 rule; ⌃⌥R passes.
        assert!(chord_for_action(BindingAction::RecordMeeting, &ch("KeyR", true, true, false, true))
            .unwrap_err()
            .contains("at most 3"));
        assert!(chord_for_action(BindingAction::RecordMeeting, &ch("KeyR", true, true, false, false)).is_ok());
        assert!(chord_for_action(BindingAction::RecordMeeting, &ch("KeyR", true, false, false, true)).is_ok());
        assert!(chord_for_action(BindingAction::Cancel, &ch("Period", true, false, false, true)).is_ok());
        assert_eq!(
            reserved_chord_reason(&ch("KeyQ", false, false, false, true), false).unwrap(),
            "⌘Q is macOS's Quit — pick another chord"
        );
    }

    /// fix-249 advisory (d): two actions on one chord are refused; a default
    /// chord another rebind vacated is free.
    #[test]
    fn two_actions_on_one_chord_are_refused() {
        let r = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SUPER), Code::KeyR);
        let m = shortcuts::MEETING_TOGGLE.shortcut();
        let d = shortcuts::DICTATE_TOGGLE.shortcut();
        let e = check_distinct(&[
            (BindingAction::DictateToggle, d, "⌃⌥⌘Space".into()),
            (BindingAction::RecordMeeting, r, "⌃⌘R".into()),
            (BindingAction::Cancel, r, "⌃⌘R".into()),
        ])
        .unwrap_err();
        assert!(e.contains("Record meeting") && e.contains("Cancel take"), "{e}");
        assert!(check_distinct(&[
            (BindingAction::DictateToggle, d, "⌃⌥⌘Space".into()),
            (BindingAction::RecordMeeting, d, "⌃⌥⌘Space".into()),
        ])
        .is_err());
        // Meeting moved to ⌃⌘R → ⌃⌘M is free for Cancel.
        let cm = ChordSetting {
            code: "KeyM".into(),
            ctrl: true,
            cmd: true,
            ..Default::default()
        };
        let (cancel, _) = chord_for_action(BindingAction::Cancel, &cm).unwrap();
        assert_eq!(cancel, m);
        assert!(check_distinct(&[
            (BindingAction::DictateToggle, d, "⌃⌥⌘Space".into()),
            (BindingAction::RecordMeeting, r, "⌃⌘R".into()),
            (BindingAction::Cancel, cancel, "⌃⌘M".into()),
        ])
        .is_ok());
    }

    /// fix-249 advisory (c): a cancel rebind that lands mid-take (the take
    /// armed Esc, then the setting moved to F6) still releases Esc, and a
    /// re-latch inside the take re-uses Esc instead of arming F6 too.
    #[test]
    fn cancel_rebind_mid_take_never_leaves_esc_registered_idle() {
        let esc = shortcuts::CANCEL.shortcut();
        let f6 = Shortcut::new(None, Code::F6);
        let mut registry: Vec<Shortcut> = Vec::new();
        let mut slot: Option<Shortcut> = None;
        let mut active = esc;
        // take starts: arm
        let sc = cancel_for_register(&slot, active);
        registry.push(sc);
        slot = Some(sc);
        // Settings rebinds the cancel key while the take runs
        active = f6;
        // hands-free re-latch inside the same take
        let sc = cancel_for_register(&slot, active);
        assert_eq!(sc, esc, "the re-latch keeps the key this take armed");
        if !registry.contains(&sc) {
            registry.push(sc);
        }
        // take ends: release
        let sc = cancel_for_release(&mut slot, active);
        registry.retain(|x| *x != sc);
        assert!(registry.is_empty(), "idle with {registry:?} still registered");
        assert_eq!(slot, None);
        // the next take arms the new key
        assert_eq!(cancel_for_register(&slot, active), f6);
        // a second release with nothing armed targets the active key (no-op)
        assert_eq!(cancel_for_release(&mut slot, active), f6);
    }

    /// fix-249 advisory (e): tray + notification follow the rebind.
    #[test]
    fn tray_and_notification_name_the_live_meeting_chord() {
        assert_eq!(meeting_accelerator(None), shortcuts::MEETING_TOGGLE.accelerator);
        // Y8-A: three keys — a four-key ⌃⌥⌘R is now refused (at most 3).
        let r = ChordSetting {
            code: "KeyR".into(),
            ctrl: true,
            alt: true,
            ..Default::default()
        };
        assert_eq!(meeting_accelerator(Some(&r)), "Ctrl+Alt+KeyR");
        // A refused setting falls back to the default, exactly like `resolve`.
        let q = ChordSetting {
            code: "KeyQ".into(),
            cmd: true,
            ..Default::default()
        };
        assert_eq!(meeting_accelerator(Some(&q)), shortcuts::MEETING_TOGGLE.accelerator);
        assert_eq!(meeting_started_body("⌃⌥⌘R"), "Stop with ⌃⌥⌘R or the menu bar.");
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

    fn ch(code: &str, ctrl: bool, alt: bool, shift: bool, cmd: bool) -> ChordSetting {
        ChordSetting {
            code: code.into(),
            ctrl,
            alt,
            shift,
            cmd,
        }
    }

    /// yap-hotkeys-advisories-r1 #6 — the Rust reserved list and the frontend's
    /// `desktop/src/reservedChords.json` are the same list, in the same order,
    /// and both sides answer the shared case table identically (the vitest
    /// twin runs the same file). Drift on either side fails here.
    #[test]
    fn reserved_chords_parity() {
        #[derive(Deserialize)]
        struct Row {
            ctrl: bool,
            alt: bool,
            shift: bool,
            cmd: bool,
            code: String,
            what: String,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Case {
            chord: ChordSetting,
            allow_bare: bool,
            reason: Option<String>,
        }
        let json: Vec<Row> = serde_json::from_str(include_str!("../../src/reservedChords.json"))
            .expect("desktop/src/reservedChords.json parses");
        let ts: Vec<(bool, bool, bool, bool, String, String)> = json
            .into_iter()
            .map(|r| (r.ctrl, r.alt, r.shift, r.cmd, r.code, r.what))
            .collect();
        let rust: Vec<(bool, bool, bool, bool, String, String)> = RESERVED_CHORDS
            .iter()
            .map(|&(a, b, c, d, e, f)| (a, b, c, d, e.to_string(), f.to_string()))
            .collect();
        assert_eq!(
            rust, ts,
            "RESERVED_CHORDS and desktop/src/reservedChords.json drifted"
        );
        for (.., code, _) in RESERVED_CHORDS {
            assert!(Code::from_str(code).is_ok(), "{code} is not a key code");
        }
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../src/reservedChordCases.json"))
                .expect("desktop/src/reservedChordCases.json parses");
        assert!(cases.len() >= 20);
        for c in &cases {
            assert_eq!(
                reserved_chord_reason(&c.chord, c.allow_bare),
                c.reason,
                "{:?} allow_bare={}",
                c.chord,
                c.allow_bare
            );
        }
        // The frontend reads the JSON — it keeps no literal copy of the list.
        let ts_src = include_str!("../../src/hotkeyRecorder.ts");
        assert!(
            ts_src.contains("./reservedChords.json"),
            "hotkeyRecorder.ts must import reservedChords.json"
        );
        assert!(
            !ts_src.contains("\"Lock Screen\""),
            "hotkeyRecorder.ts carries a literal copy of the list"
        );
    }

    /// advisory 1 — the six chords the #249 review named are refused.
    #[test]
    fn advisory_chords_are_reserved() {
        for (c, what) in [
            (
                ch("Digit3", true, false, true, true),
                "screenshot to the clipboard",
            ),
            (
                ch("Digit4", true, false, true, true),
                "screenshot selection to the clipboard",
            ),
            (ch("KeyH", false, true, false, true), "Hide Others"),
            (ch("KeyM", false, true, false, true), "Minimize All"),
            (ch("KeyW", false, true, false, true), "Close All"),
            (ch("KeyD", true, false, false, true), "Look Up"),
        ] {
            for bare in [false, true] {
                let why = reserved_chord_reason(&c, bare).unwrap_or_default();
                assert!(why.contains(what), "{c:?}: {why}");
            }
            assert!(stored_chord_refusal(BindingAction::DictateToggle, Some(&c)).is_some());
        }
    }

    /// advisory 2 — a reserved chord stored before the rule existed is
    /// re-validated at launch: Yap registers the default, and the row says why.
    #[test]
    fn stored_reserved_dictate_chord_falls_back_at_launch() {
        let spotlight = ch("Space", false, false, false, true);
        let (chord, label) = resolve_dictate(Some(&spotlight));
        assert_eq!(chord, shortcuts::DICTATE_TOGGLE.shortcut());
        assert_eq!(label, shortcuts::DICTATE_TOGGLE.label);
        let why = stored_chord_refusal(BindingAction::DictateToggle, Some(&spotlight)).unwrap();
        assert_eq!(
            why,
            "Your saved chord is not used: ⌘Space is macOS's Spotlight — pick another chord. Yap is using ⌃⌥⌘Space until you record another."
        );
        let look_up = ch("KeyD", true, false, false, true);
        assert_eq!(
            resolve_dictate(Some(&look_up)).1,
            shortcuts::DICTATE_TOGGLE.label
        );
        // A valid stored chord is used and carries no note.
        let ok = ch("KeyR", true, true, false, true);
        assert_eq!(resolve_dictate(Some(&ok)).1, "⌃⌥⌘R");
        assert_eq!(
            stored_chord_refusal(BindingAction::DictateToggle, Some(&ok)),
            None
        );
        assert_eq!(
            stored_chord_refusal(BindingAction::DictateToggle, None),
            None
        );
        // Meeting / Cancel get the same sentence from their own rules.
        let m = stored_chord_refusal(
            BindingAction::RecordMeeting,
            Some(&ch("KeyW", false, true, false, true)),
        )
        .unwrap();
        assert!(
            m.contains("Close All") && m.ends_with("Yap is using ⌃⌘M until you record another."),
            "{m}"
        );
        // The row and the launch dump carry the note.
        let mut i = inputs(FnTapStatus::Delivering, CarbonStatus::Registered);
        i.refusals = vec![(BindingAction::DictateToggle, why.clone())];
        let rows = binding_table(&i);
        let toggle = rows
            .iter()
            .find(|r| r.action == BindingAction::DictateToggle)
            .unwrap();
        assert_eq!(toggle.note.as_deref(), Some(why.as_str()));
        assert!(rows.iter().filter(|r| r.note.is_some()).count() == 1);
        assert!(debug_dump(&rows).contains("Spotlight"));
    }

    /// advisories 4 + 5 — the chips and the Permissions sentence.
    #[test]
    fn unverified_and_failed_grant_checks_say_so() {
        assert!(BindingStatus::NeedsInputMonitoringUnverified
            .chip()
            .contains("remove and re-add"));
        let f = BindingStatus::GrantCheckFailed("disk I/O error".into());
        assert!(f.chip().contains("disk I/O error") && f.chip().contains("remove and re-add"));
        assert!(!f.usable() && !BindingStatus::NeedsInputMonitoringUnverified.usable());
        let h = input_monitoring_unverified_hint(false, &TccRow::Unreadable, "Yap Test").unwrap();
        assert!(h.contains("remove it") && h.contains("Yap Test"), "{h}");
        let e = input_monitoring_unverified_hint(false, &TccRow::QueryError("x".into()), "Yap")
            .unwrap();
        assert!(
            e.contains("could not read the permission database (x)") && e.contains("remove it"),
            "{e}"
        );
        assert_eq!(
            input_monitoring_unverified_hint(true, &TccRow::Unreadable, "Yap"),
            None
        );
        assert_eq!(
            input_monitoring_unverified_hint(false, &TccRow::NotAllowed, "Yap"),
            None
        );
        assert_eq!(
            input_monitoring_unverified_hint(false, &TccRow::Allowed, "Yap"),
            None
        );
    }

    /// advisory 5 — the TCC query, against fixture databases: a failed query is
    /// an error, never "no row"; a missing database is "unreadable".
    #[test]
    fn tcc_query_errors_are_not_no_row() {
        let dir = std::env::temp_dir().join(format!("yap-tcc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mk = |name: &str, sql: &str| {
            let p = dir.join(name);
            let _ = std::fs::remove_file(&p);
            rusqlite::Connection::open(&p)
                .unwrap()
                .execute_batch(sql)
                .unwrap();
            p
        };
        let schema = "create table access (service text, client text, auth_value);";
        let good = mk(
            "good.db",
            &format!(
                "{schema} insert into access values ('kTCCServiceListenEvent','com.a',2), \
                 ('kTCCServiceListenEvent','com.b',0), ('kTCCServiceListenEvent','com.c','oops');"
            ),
        );
        assert_eq!(tcc_listen_event_row_at(&good, "com.a"), TccRow::Allowed);
        assert_eq!(tcc_listen_event_row_at(&good, "com.b"), TccRow::NotAllowed);
        assert_eq!(
            tcc_listen_event_row_at(&good, "com.absent"),
            TccRow::NotAllowed
        );
        // A quote in the bundle id is a bound parameter, not SQL.
        assert_eq!(
            tcc_listen_event_row_at(&good, "x' or '1'='1"),
            TccRow::NotAllowed
        );
        assert!(matches!(
            tcc_listen_event_row_at(&good, "com.c"),
            TccRow::QueryError(_)
        ));
        let no_table = mk("notable.db", "create table other (x);");
        match tcc_listen_event_row_at(&no_table, "com.a") {
            TccRow::QueryError(e) => assert!(e.contains("no such table"), "{e}"),
            other => panic!("want QueryError, got {other:?}"),
        }
        assert_eq!(
            tcc_listen_event_row_at(&dir.join("missing.db"), "com.a"),
            TccRow::Unreadable
        );
        assert_eq!(tcc_listen_event_row_at(&good, ""), TccRow::Unreadable);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
