//! Y6-B — the menu bar as a usable surface, decided in ONE table.
//!
//! Before this module the tray was a literal `match event.id.as_ref()` with a
//! `_ => {}` arm: an item whose id was misspelled, or whose arm was forgotten,
//! compiled, rendered and silently did nothing. Now every item the menu shows
//! is a row of [`TRAY_ITEMS`], every row names a [`TrayCommand`], and `lib.rs`
//! dispatches through [`command_for`] into an EXHAUSTIVE match on that enum —
//! a new command without a handler is a compile error, and a new row without a
//! place in [`MENU_LAYOUT`] (or [`INSERTED_ITEMS`]) fails `tests/tray_menu.rs`.
//!
//! What is deliberately NOT here (Y6-B "what not to do"): no Languages and no
//! Formatting submenus. Multi-language is Y10, and an empty submenu is worse
//! than no submenu.
//!
//! `sync_tray` in `lib.rs` stays the only place the tray is mutated after it is
//! built; this module holds no tray handles, only decisions.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::meeting_control;
use crate::meetings::MeetingKind;

// ─── The registry ────────────────────────────────────────────────────────────

/// Every action the menu bar can take. `lib.rs::run_tray_command` matches on
/// this WITHOUT a wildcard arm, so a variant nobody handles does not compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    /// Start / Stop Dictation (label follows `recording`).
    ToggleDictation,
    /// Re-paste the newest transcript into the frontmost app (⌃⌘V).
    PasteLastTranscript,
    /// Put the newest transcript on the clipboard, sequenced behind any paste
    /// transaction still holding the pasteboard (YV39).
    CopyLastTranscript,
    /// YV51 — re-paste the RAW take over the polished one (⌃⌘Z).
    PasteRawLastTranscript,
    /// YV95 — start/stop a meeting with the kind question skipped.
    ToggleMeeting,
    /// YV125 — start a meeting with the kind the user picked.
    RecordMeetingAs(MeetingKind),
    /// yap24-NT7 — open the finished meeting's notes.
    OpenMeetingNotes,
    /// Hands-free latch.
    ToggleHandsFree,
    /// Present the main window.
    OpenYap,
    /// Present the main window on the transcript history ([`HISTORY_NAV`]).
    OpenHistory,
    /// Present the main window on Settings.
    OpenSettings,
    /// Settings, on the Shortcut sub-tab.
    OpenShortcuts,
    /// Hide the idle pill for [`PILL_HIDE_FOR`] (see [`PillHide`]).
    HidePillForAnHour,
    /// End a [`PillHide`] early.
    ShowPillNow,
    /// Y2-E — Settings → License (only in the menu while it offers a purchase).
    Upgrade,
    /// Open the new-issue page.
    SendFeedback,
    /// `app.exit(0)` — reaches `RunEvent::Exit` and its one bounded teardown
    /// (`lifecycle::quit_plan`), which drains the ASR engine, waits for the
    /// in-flight take and finalizes a live meeting. Never `process::exit`.
    Quit,
}

impl TrayCommand {
    /// A stable name per variant (the payload of [`TrayCommand::RecordMeetingAs`]
    /// is not part of it). Exhaustive on purpose: adding a variant forces a name.
    pub fn name(self) -> &'static str {
        match self {
            TrayCommand::ToggleDictation => "toggle_dictation",
            TrayCommand::PasteLastTranscript => "paste_last_transcript",
            TrayCommand::CopyLastTranscript => "copy_last_transcript",
            TrayCommand::PasteRawLastTranscript => "paste_raw_last_transcript",
            TrayCommand::ToggleMeeting => "toggle_meeting",
            TrayCommand::RecordMeetingAs(_) => "record_meeting_as",
            TrayCommand::OpenMeetingNotes => "open_meeting_notes",
            TrayCommand::ToggleHandsFree => "toggle_hands_free",
            TrayCommand::OpenYap => "open_yap",
            TrayCommand::OpenHistory => "open_history",
            TrayCommand::OpenSettings => "open_settings",
            TrayCommand::OpenShortcuts => "open_shortcuts",
            TrayCommand::HidePillForAnHour => "hide_pill_for_an_hour",
            TrayCommand::ShowPillNow => "show_pill_now",
            TrayCommand::Upgrade => "upgrade",
            TrayCommand::SendFeedback => "send_feedback",
            TrayCommand::Quit => "quit",
        }
    }
}

/// Every [`TrayCommand::name`]. The test walks this to prove no command is
/// unreachable from the menu.
pub const COMMAND_NAMES: [&str; 17] = [
    "toggle_dictation",
    "paste_last_transcript",
    "copy_last_transcript",
    "paste_raw_last_transcript",
    "toggle_meeting",
    "record_meeting_as",
    "open_meeting_notes",
    "toggle_hands_free",
    "open_yap",
    "open_history",
    "open_settings",
    "open_shortcuts",
    "hide_pill_for_an_hour",
    "show_pill_now",
    "upgrade",
    "send_feedback",
    "quit",
];

/// What a row of the menu is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// A clickable item that runs exactly one command.
    Action(TrayCommand),
    /// A disabled line that only reports state (the license header).
    StateLabel,
    /// A submenu whose children are `meeting_control::KIND_PICKER`.
    KindSubmenu,
}

/// One row of the menu bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayItem {
    pub id: &'static str,
    /// The label the item is BUILT with. A few are rewritten by `sync_tray`
    /// (Start↔Stop Dictation, the license header) — the id never changes.
    pub label: &'static str,
    pub kind: ItemKind,
}

pub const ID_LICENSE_HEADER: &str = "license_header";
pub const ID_UPGRADE: &str = "upgrade";
pub const ID_TOGGLE: &str = "toggle";
pub const ID_PASTE_LAST: &str = "paste_last";
pub const ID_COPY_LAST: &str = "copy_last";
pub const ID_PASTE_RAW: &str = "paste_raw";
pub const ID_NOTES_READY: &str = "meeting_notes_open";
pub const ID_MEETING_TOGGLE: &str = "meeting_toggle";
pub const ID_MEETING_KIND: &str = "meeting_kind";
pub const ID_HANDS_FREE: &str = "hands_free";
pub const ID_SHOW: &str = "show";
pub const ID_HISTORY: &str = "history";
pub const ID_SETTINGS: &str = "settings";
pub const ID_SHORTCUTS: &str = "shortcuts";
pub const ID_HIDE_PILL: &str = "hide_pill_hour";
pub const ID_SHOW_PILL: &str = "show_pill_now";
pub const ID_FEEDBACK: &str = "feedback";
pub const ID_QUIT: &str = "quit";

/// The `navigate` payload that lands on the transcript history. Yap's history
/// lives on the Home view (`desktop/src/views/Home.tsx`); there is no separate
/// "history" route, and the test checks this value is a member of the
/// frontend's `Nav` union so the two cannot drift.
pub const HISTORY_NAV: &str = "home";

pub const TRAY_ITEMS: [TrayItem; 18] = [
    TrayItem {
        id: ID_LICENSE_HEADER,
        label: "Yap",
        kind: ItemKind::StateLabel,
    },
    TrayItem {
        id: ID_UPGRADE,
        label: crate::license::UPGRADE_TRAY_LABEL,
        kind: ItemKind::Action(TrayCommand::Upgrade),
    },
    TrayItem {
        id: ID_TOGGLE,
        label: "Start Dictation",
        kind: ItemKind::Action(TrayCommand::ToggleDictation),
    },
    TrayItem {
        id: ID_PASTE_LAST,
        label: "Paste Last Transcript",
        kind: ItemKind::Action(TrayCommand::PasteLastTranscript),
    },
    TrayItem {
        id: ID_COPY_LAST,
        label: "Copy Last Transcript",
        kind: ItemKind::Action(TrayCommand::CopyLastTranscript),
    },
    TrayItem {
        id: ID_PASTE_RAW,
        label: "Undo AI Edit (Paste Raw)",
        kind: ItemKind::Action(TrayCommand::PasteRawLastTranscript),
    },
    TrayItem {
        id: ID_NOTES_READY,
        label: "Meeting notes ready — Open",
        kind: ItemKind::Action(TrayCommand::OpenMeetingNotes),
    },
    TrayItem {
        id: ID_MEETING_TOGGLE,
        label: "Record a meeting",
        kind: ItemKind::Action(TrayCommand::ToggleMeeting),
    },
    TrayItem {
        id: ID_MEETING_KIND,
        label: "Record a meeting as…",
        kind: ItemKind::KindSubmenu,
    },
    TrayItem {
        id: ID_HANDS_FREE,
        label: "Hands-free",
        kind: ItemKind::Action(TrayCommand::ToggleHandsFree),
    },
    TrayItem {
        id: ID_SHOW,
        label: "Open Yap",
        kind: ItemKind::Action(TrayCommand::OpenYap),
    },
    TrayItem {
        id: ID_HISTORY,
        label: "Transcript History…",
        kind: ItemKind::Action(TrayCommand::OpenHistory),
    },
    TrayItem {
        id: ID_SETTINGS,
        label: "Settings…",
        kind: ItemKind::Action(TrayCommand::OpenSettings),
    },
    TrayItem {
        id: ID_SHORTCUTS,
        label: "Keyboard Shortcuts…",
        kind: ItemKind::Action(TrayCommand::OpenShortcuts),
    },
    TrayItem {
        id: ID_HIDE_PILL,
        label: "Hide Pill for 1 Hour",
        kind: ItemKind::Action(TrayCommand::HidePillForAnHour),
    },
    TrayItem {
        id: ID_SHOW_PILL,
        label: "Show Pill Now",
        kind: ItemKind::Action(TrayCommand::ShowPillNow),
    },
    TrayItem {
        id: ID_FEEDBACK,
        label: "Send Feedback…",
        kind: ItemKind::Action(TrayCommand::SendFeedback),
    },
    TrayItem {
        id: ID_QUIT,
        label: "Quit Yap",
        kind: ItemKind::Action(TrayCommand::Quit),
    },
];

/// One slot of the menu as it is BUILT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Item(&'static str),
    Separator,
}

/// The menu, top to bottom, as `lib.rs` builds it. `lib.rs` iterates THIS
/// list, so the order lives in one place.
pub const MENU_LAYOUT: [Slot; 23] = [
    // State header (Y2-E's license line; the upgrade line is inserted under it).
    Slot::Item(ID_LICENSE_HEADER),
    Slot::Separator,
    // The dictation control, then the last transcript.
    Slot::Item(ID_TOGGLE),
    Slot::Item(ID_PASTE_LAST),
    Slot::Item(ID_COPY_LAST),
    Slot::Item(ID_PASTE_RAW),
    Slot::Separator,
    Slot::Item(ID_MEETING_TOGGLE),
    Slot::Item(ID_MEETING_KIND),
    Slot::Separator,
    Slot::Item(ID_HANDS_FREE),
    Slot::Separator,
    // Where things live.
    Slot::Item(ID_SHOW),
    Slot::Item(ID_HISTORY),
    Slot::Item(ID_SETTINGS),
    Slot::Item(ID_SHORTCUTS),
    Slot::Separator,
    // The pill.
    Slot::Item(ID_HIDE_PILL),
    Slot::Item(ID_SHOW_PILL),
    Slot::Separator,
    Slot::Item(ID_FEEDBACK),
    Slot::Separator,
    Slot::Item(ID_QUIT),
];

/// Items that are built with the menu but placed by `sync_tray` /
/// `announce_meeting_notes` only while they apply: the upgrade line (Y2-E,
/// under the header while a purchase is on offer) and the notes-ready line
/// (yap24-NT7, above "Record a meeting" once notes land).
pub const INSERTED_ITEMS: [&str; 2] = [ID_UPGRADE, ID_NOTES_READY];

/// The row for `id`, if the menu has one.
pub fn item(id: &str) -> Option<&'static TrayItem> {
    TRAY_ITEMS.iter().find(|i| i.id == id)
}

/// The label an item is built with. Panics on an id that is not in the table —
/// a programming error caught the first time the tray is built.
pub fn label(id: &str) -> &'static str {
    item(id)
        .map(|i| i.label)
        .unwrap_or_else(|| panic!("tray_menu: no item with id {id:?}"))
}

/// THE dispatch decision: which command a clicked menu id runs. `None` is an id
/// the menu never built (or a non-action row) — logged by the caller, never
/// silently ignored.
pub fn command_for(id: &str) -> Option<TrayCommand> {
    if let Some(i) = item(id) {
        return match i.kind {
            ItemKind::Action(c) => Some(c),
            ItemKind::StateLabel | ItemKind::KindSubmenu => None,
        };
    }
    meeting_control::kind_for_menu_id(id).map(TrayCommand::RecordMeetingAs)
}

// ─── Hide the pill for one hour ──────────────────────────────────────────────

/// How long "Hide Pill for 1 Hour" hides it.
pub const PILL_HIDE_FOR: Duration = Duration::from_secs(60 * 60);

/// A TEMPORARY hide of the idle pill — a different thing from the persistent
/// `AppSettings::show_floating_pill` boolean, and deliberately not stored next
/// to it.
///
/// The deadline is a monotonic [`Instant`] held only in this process's memory.
/// It survives NOTHING: it is not written to settings.json, not to the database
/// and not to any marker file, so a restart (or a crash) during the hour shows
/// the pill again. That is the intended, forgiving behaviour — a user who forgot
/// they hid the pill gets it back the next time Yap launches rather than
/// wondering where it went.
///
/// The hide governs the IDLE pill only. A dictation or a meeting still shows
/// the pill while it records: a live microphone is never invisible.
#[derive(Debug, Default)]
pub struct PillHide {
    inner: Mutex<HideState>,
}

#[derive(Debug, Default)]
struct HideState {
    until: Option<Instant>,
    generation: u64,
}

/// Returned by [`PillHide::hide_for_an_hour`]; the expiry timer hands it back
/// to [`PillHide::expire`] so a superseded hide (re-armed or ended early) can
/// never re-show the pill on the old schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HideTicket {
    pub generation: u64,
    pub until: Instant,
}

impl PillHide {
    pub const fn new() -> Self {
        PillHide {
            inner: Mutex::new(HideState {
                until: None,
                generation: 0,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HideState> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Hide from `now` until `now + PILL_HIDE_FOR`. Re-arming restarts the hour.
    pub fn hide_for_an_hour(&self, now: Instant) -> HideTicket {
        let mut s = self.lock();
        s.generation += 1;
        let until = now + PILL_HIDE_FOR;
        s.until = Some(until);
        HideTicket {
            generation: s.generation,
            until,
        }
    }

    /// End the hide now. Returns whether one was active.
    pub fn show_now(&self) -> bool {
        let mut s = self.lock();
        s.generation += 1;
        s.until.take().is_some()
    }

    /// Is the idle pill hidden at `now`?
    pub fn is_hidden(&self, now: Instant) -> bool {
        self.lock().until.is_some_and(|u| now < u)
    }

    /// The expiry timer's half: clears the hide iff `ticket` is still the
    /// current one and its deadline has passed. `true` means the caller should
    /// put the pill back.
    pub fn expire(&self, ticket: HideTicket, now: Instant) -> bool {
        let mut s = self.lock();
        if s.generation != ticket.generation || now < ticket.until || s.until.is_none() {
            return false;
        }
        s.until = None;
        true
    }
}

/// Should the IDLE pill be on screen? The persistent setting AND not inside a
/// temporary hide. Every "after a take / after a meeting / on settings change"
/// decision in `lib.rs` asks this instead of reading the setting directly.
pub fn idle_pill_visible(show_floating_pill: bool, hidden: bool) -> bool {
    show_floating_pill && !hidden
}

/// Enabled state of the two pill items, refreshed by `sync_tray`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PillMenuState {
    /// "Hide Pill for 1 Hour": only when there is an always-on pill to hide.
    pub hide_enabled: bool,
    /// "Show Pill Now": only while a hide is running.
    pub show_now_enabled: bool,
}

pub fn pill_menu_state(show_floating_pill: bool, hidden: bool) -> PillMenuState {
    PillMenuState {
        hide_enabled: show_floating_pill && !hidden,
        show_now_enabled: hidden,
    }
}
