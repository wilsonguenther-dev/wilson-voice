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
//! What is deliberately NOT here (Y6-B "what not to do"): no Formatting
//! submenu — an empty submenu is worse than no submenu. The Languages submenu
//! (WF-11) arrived with Y10's multi-language set and is never empty: it always
//! carries "Language Settings…".
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
    /// WF-07 — record one of today's calendar meetings, by its row index in
    /// the "Today's meetings" submenu ([`upcoming_row`]).
    RecordUpcoming(usize),
    /// WF-07 — "Hide meeting titles" (persisted by `meeting_detect`).
    ToggleHideMeetingTitles,
    /// WF-09 — pick an input device, by its row index in the "Microphone"
    /// submenu ([`microphone_row`]); row 0 is "System Default".
    PickMicrophone(usize),
    /// WF-11 — pick Auto-detect or one language of the set, by its row index
    /// in the "Languages" submenu ([`language_row`]). Written through the ONE
    /// language writer the bar's chip and Settings use.
    PickLanguage(usize),
    /// WF-11 — "Language Settings…" at the bottom of the Languages submenu:
    /// Settings, on the Dictation sub-tab where "Languages I speak" lives.
    OpenLanguageSettings,
    /// WF-14 — "Update ready — Restart to update". Restarts into the release
    /// `install_update` already put on disk, but only through the quiet gate
    /// ([`restart_click`]): while a take, a polish or a meeting runs the click
    /// arms a deferred restart instead, and a second click cancels it.
    RestartToUpdate,
    /// WF-04 — "Dock at Bottom / on Left / on Right" in the pill's right-click
    /// menu. Writes the same `AppSettings::pill_position` the Settings picker
    /// and a drag write ([`PillDock::as_settings`]).
    DockPill(PillDock),
    /// WF-04 — "Open Scratchpad" in the pill's right-click menu.
    OpenScratchpad,
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
            TrayCommand::RecordUpcoming(_) => "record_upcoming",
            TrayCommand::ToggleHideMeetingTitles => "toggle_hide_meeting_titles",
            TrayCommand::PickMicrophone(_) => "pick_microphone",
            TrayCommand::PickLanguage(_) => "pick_language",
            TrayCommand::OpenLanguageSettings => "open_language_settings",
            TrayCommand::RestartToUpdate => "restart_to_update",
            TrayCommand::DockPill(_) => "dock_pill",
            TrayCommand::OpenScratchpad => "open_scratchpad",
            TrayCommand::Quit => "quit",
        }
    }
}

/// Every [`TrayCommand::name`]. The test walks this to prove no command is
/// unreachable from the menu.
pub const COMMAND_NAMES: [&str; 25] = [
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
    "record_upcoming",
    "toggle_hide_meeting_titles",
    "pick_microphone",
    "pick_language",
    "open_language_settings",
    "restart_to_update",
    "dock_pill",
    "open_scratchpad",
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
    /// WF-07 — the "Today's meetings" submenu; its children are built from
    /// [`upcoming_menu`] and dispatch [`TrayCommand::RecordUpcoming`].
    UpcomingSubmenu,
    /// WF-09 — the "Microphone" submenu; its children are built from
    /// [`microphone_menu`] and dispatch [`TrayCommand::PickMicrophone`].
    MicrophoneSubmenu,
    /// WF-11 — the "Languages" submenu; its children are built from
    /// [`language_menu`] and dispatch [`TrayCommand::PickLanguage`], with
    /// [`ID_LANGUAGE_SETTINGS`] (→ [`TrayCommand::OpenLanguageSettings`]) last.
    LanguageSubmenu,
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
/// yap-1106 — the disabled "Recording · <title>" line shown above the Stop
/// item while a meeting records.
pub const ID_MEETING_HEADER: &str = "meeting_header";
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
/// WF-07 — the disabled "Next: <title> · 2:00 PM" line (calendar connected).
pub const ID_NEXT_MEETING: &str = "upcoming_next";
/// WF-07 — the "Today's meetings" submenu.
pub const ID_UPCOMING: &str = "upcoming_today";
/// WF-07 — the "Hide meeting titles" toggle.
pub const ID_HIDE_MEETING_TITLES: &str = "upcoming_hide_titles";
/// WF-07 — prefix of each "Record …" child of [`ID_UPCOMING`]; the suffix is
/// the row index.
pub const UPCOMING_RECORD_PREFIX: &str = "upcoming_record_";
/// WF-09 — the "Microphone" submenu (Dictation group, under Hands-free).
pub const ID_MICROPHONE: &str = "microphone";
/// WF-09 — prefix of each child of [`ID_MICROPHONE`]; the suffix is the row
/// index (0 = "System Default").
pub const MIC_PICK_PREFIX: &str = "mic_pick_";
/// WF-11 — the "Languages" submenu (Dictation group, under Microphone).
pub const ID_LANGUAGES: &str = "languages";
/// WF-11 — prefix of each pick child of [`ID_LANGUAGES`]; the suffix is the
/// row index.
pub const LANG_PICK_PREFIX: &str = "lang_pick_";
/// WF-11 — the "Language Settings…" child, always the last row of
/// [`ID_LANGUAGES`].
pub const ID_LANGUAGE_SETTINGS: &str = "language_settings";
/// The label of [`ID_LANGUAGE_SETTINGS`].
pub const LANGUAGE_SETTINGS_LABEL: &str = "Language Settings\u{2026}";
/// The label of the Auto-detect row.
pub const LANGUAGE_AUTO_LABEL: &str = "Auto-detect";
/// The Settings sub-tab "Language Settings…" lands on (the `SettingsTab` that
/// renders `LanguageSettings`, see `desktop/src/views/settings/Dictation.tsx`).
pub const LANGUAGE_SETTINGS_TAB: &str = "dictation";
/// WF-14 — the "Update ready" line, inserted under the status header only
/// while a downloaded release is waiting for a restart.
pub const ID_UPDATE_READY: &str = "update_ready";
/// WF-14 — the line's label while Yap is quiet and a click restarts at once.
pub const UPDATE_READY_LABEL: &str = "Update ready \u{2014} Restart to update";

/// The `navigate` payload that lands on the transcript history. Yap's history
/// lives on the Home view (`desktop/src/views/Home.tsx`); there is no separate
/// "history" route, and the test checks this value is a member of the
/// frontend's `Nav` union so the two cannot drift.
pub const HISTORY_NAV: &str = "home";

pub const TRAY_ITEMS: [TrayItem; 25] = [
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
        id: ID_MEETING_HEADER,
        label: "Recording",
        kind: ItemKind::StateLabel,
    },
    TrayItem {
        id: ID_MEETING_TOGGLE,
        label: MEETING_START_LABEL,
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
    TrayItem {
        id: ID_NEXT_MEETING,
        label: "Next meeting",
        kind: ItemKind::StateLabel,
    },
    TrayItem {
        id: ID_UPCOMING,
        label: "Today’s Meetings",
        kind: ItemKind::UpcomingSubmenu,
    },
    TrayItem {
        id: ID_HIDE_MEETING_TITLES,
        label: "Hide Meeting Titles",
        kind: ItemKind::Action(TrayCommand::ToggleHideMeetingTitles),
    },
    TrayItem {
        id: ID_MICROPHONE,
        label: "Microphone",
        kind: ItemKind::MicrophoneSubmenu,
    },
    TrayItem {
        id: ID_LANGUAGES,
        label: "Languages",
        kind: ItemKind::LanguageSubmenu,
    },
    TrayItem {
        id: ID_UPDATE_READY,
        label: UPDATE_READY_LABEL,
        kind: ItemKind::Action(TrayCommand::RestartToUpdate),
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
///
/// yap-1106-menubar-redesign — grouped by what the user came to do:
///   1. the status header (license line; while a meeting records the
///      "Recording · <title>" header and "Stop meeting · 12:00" sit above it),
///   2. Meeting — Start (or Stop) first, then the kind picker; with the
///      calendar connected (WF-07) the next meeting, "Today's Meetings ▸" and
///      "Hide Meeting Titles" are inserted directly under the picker,
///   3. Dictation — the toggle, hands-free, then the last transcript,
///   4. Open — Yap, history, settings, shortcuts,
///   5. Pill — hide / show,
///   6. Feedback and Quit.
pub const MENU_LAYOUT: [Slot; 22] = [
    // 1. Status header (Y2-E's license line; the upgrade line is inserted
    // under it).
    Slot::Item(ID_LICENSE_HEADER),
    Slot::Separator,
    // 2. Meeting: Start or Stop first.
    Slot::Item(ID_MEETING_TOGGLE),
    Slot::Item(ID_MEETING_KIND),
    Slot::Separator,
    // 3. Dictation, then the last transcript.
    Slot::Item(ID_TOGGLE),
    Slot::Item(ID_HANDS_FREE),
    Slot::Item(ID_PASTE_LAST),
    Slot::Item(ID_COPY_LAST),
    Slot::Item(ID_PASTE_RAW),
    Slot::Separator,
    // 4. Open: where things live.
    Slot::Item(ID_SHOW),
    Slot::Item(ID_HISTORY),
    Slot::Item(ID_SETTINGS),
    Slot::Item(ID_SHORTCUTS),
    Slot::Separator,
    // 5. The pill.
    Slot::Item(ID_HIDE_PILL),
    Slot::Item(ID_SHOW_PILL),
    Slot::Separator,
    // 6. Feedback, then Quit last.
    Slot::Item(ID_FEEDBACK),
    Slot::Separator,
    Slot::Item(ID_QUIT),
];

/// Items that are built with the menu but placed by `sync_tray` /
/// `announce_meeting_notes` only while they apply: the upgrade line (Y2-E,
/// under the header while a purchase is on offer) and the notes-ready line
/// (yap24-NT7, above "Record a meeting" once notes land) and the meeting
/// header (yap-1106, at the top of the menu while a meeting records), and the
/// three calendar rows (WF-07, under the kind picker while "Use my calendar"
/// is on and access is granted), and the "Microphone" submenu (WF-09, under
/// Hands-free once the device watcher has enumerated the inputs — it is
/// rebuilt whenever a device is plugged in or the pick changes), and the
/// "Languages" submenu (WF-11, under Microphone — or Hands-free before the
/// device list lands — rebuilt on every language write from any surface), and
/// the "Update ready" line (WF-14, under the header — below the upgrade line
/// when that shows — only while a downloaded release waits for a restart).
pub const INSERTED_ITEMS: [&str; 9] = [
    ID_UPGRADE,
    ID_NOTES_READY,
    ID_MEETING_HEADER,
    ID_NEXT_MEETING,
    ID_UPCOMING,
    ID_HIDE_MEETING_TITLES,
    ID_MICROPHONE,
    ID_LANGUAGES,
    ID_UPDATE_READY,
];

/// The row for `id`, if the menu bar or the pill's right-click menu (WF-04,
/// [`PILL_ONLY_ITEMS`]) has one. ONE lookup for both menus, so a click on
/// either resolves through the same [`command_for`].
pub fn item(id: &str) -> Option<&'static TrayItem> {
    TRAY_ITEMS
        .iter()
        .find(|i| i.id == id)
        .or_else(|| PILL_ONLY_ITEMS.iter().find(|i| i.id == id))
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
            ItemKind::StateLabel
            | ItemKind::KindSubmenu
            | ItemKind::UpcomingSubmenu
            | ItemKind::MicrophoneSubmenu
            | ItemKind::LanguageSubmenu => None,
        };
    }
    if id == ID_LANGUAGE_SETTINGS {
        return Some(TrayCommand::OpenLanguageSettings);
    }
    if let Some(index) = language_index_for_menu_id(id) {
        return Some(TrayCommand::PickLanguage(index));
    }
    if let Some(index) = upcoming_index_for_menu_id(id) {
        return Some(TrayCommand::RecordUpcoming(index));
    }
    if let Some(index) = microphone_index_for_menu_id(id) {
        return Some(TrayCommand::PickMicrophone(index));
    }
    meeting_control::kind_for_menu_id(id).map(TrayCommand::RecordMeetingAs)
}

/// The row index a "Record …" child id carries (`upcoming_record_3` → 3).
pub fn upcoming_index_for_menu_id(id: &str) -> Option<usize> {
    let digits = id.strip_prefix(UPCOMING_RECORD_PREFIX)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The row index a "Microphone" child id carries (`mic_pick_2` → 2).
pub fn microphone_index_for_menu_id(id: &str) -> Option<usize> {
    let digits = id.strip_prefix(MIC_PICK_PREFIX)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The row index a "Languages" child id carries (`lang_pick_1` → 1).
pub fn language_index_for_menu_id(id: &str) -> Option<usize> {
    let digits = id.strip_prefix(LANG_PICK_PREFIX)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Is `id` one of the WF-07 calendar rows `lib.rs` inserts and removes?
pub fn is_upcoming_row(id: &str) -> bool {
    id == ID_NEXT_MEETING || id == ID_UPCOMING || id == ID_HIDE_MEETING_TITLES
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

// ─── WF-04 — the pill's right-click menu ────────────────────────────────────
//
// Right-click (or Ctrl-click) on the pill pops a native menu: Hide for 1 hour,
// Dock bottom / left / right, Microphone ▸, Languages ▸, Open Scratchpad, Open
// Yap. It is NOT a second table: every row is an id looked up through
// [`item`], so the rows the pill shares with the menu bar (hide, Microphone,
// Languages, Open Yap) carry the menu bar's label and dispatch through the
// same [`command_for`] — the labels cannot drift. Only the rows the menu bar
// has no use for (the three docks, Open Scratchpad) live in
// [`PILL_ONLY_ITEMS`], in the same `TrayItem` shape.

/// The three docks the pill can sit on — the same three `float_pill`
/// `PillPosition` knows and the Settings picker offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillDock {
    Bottom,
    Left,
    Right,
}

impl PillDock {
    pub const ALL: [PillDock; 3] = [PillDock::Bottom, PillDock::Left, PillDock::Right];

    /// The exact `AppSettings::pill_position` string
    /// (`float_pill::PillPosition::as_settings`).
    pub fn as_settings(self) -> &'static str {
        match self {
            PillDock::Bottom => "bottom",
            PillDock::Left => "left",
            PillDock::Right => "right",
        }
    }

    /// Parse a stored `pill_position`; anything unknown is the default,
    /// bottom (`float_pill::PillPosition::from_settings`).
    pub fn from_settings(s: &str) -> Self {
        match s {
            "left" => PillDock::Left,
            "right" => PillDock::Right,
            _ => PillDock::Bottom,
        }
    }

    /// The pill-menu row that docks here.
    pub fn menu_id(self) -> &'static str {
        match self {
            PillDock::Bottom => ID_PILL_DOCK_BOTTOM,
            PillDock::Left => ID_PILL_DOCK_LEFT,
            PillDock::Right => ID_PILL_DOCK_RIGHT,
        }
    }
}

pub const ID_PILL_DOCK_BOTTOM: &str = "pill_dock_bottom";
pub const ID_PILL_DOCK_LEFT: &str = "pill_dock_left";
pub const ID_PILL_DOCK_RIGHT: &str = "pill_dock_right";
pub const ID_OPEN_SCRATCHPAD: &str = "open_scratchpad";

/// The rows only the pill's right-click menu shows. Looked up by [`item`] and
/// dispatched by [`command_for`] exactly like [`TRAY_ITEMS`].
pub const PILL_ONLY_ITEMS: [TrayItem; 4] = [
    TrayItem {
        id: ID_PILL_DOCK_BOTTOM,
        label: "Dock at Bottom",
        kind: ItemKind::Action(TrayCommand::DockPill(PillDock::Bottom)),
    },
    TrayItem {
        id: ID_PILL_DOCK_LEFT,
        label: "Dock on Left",
        kind: ItemKind::Action(TrayCommand::DockPill(PillDock::Left)),
    },
    TrayItem {
        id: ID_PILL_DOCK_RIGHT,
        label: "Dock on Right",
        kind: ItemKind::Action(TrayCommand::DockPill(PillDock::Right)),
    },
    TrayItem {
        id: ID_OPEN_SCRATCHPAD,
        label: "Open Scratchpad",
        kind: ItemKind::Action(TrayCommand::OpenScratchpad),
    },
];

/// The pill's right-click menu, top to bottom. `lib.rs::pill_context_menu`
/// builds the native menu from [`pill_context_menu`], which walks THIS.
pub const PILL_MENU_LAYOUT: [Slot; 11] = [
    Slot::Item(ID_HIDE_PILL),
    Slot::Separator,
    Slot::Item(ID_PILL_DOCK_BOTTOM),
    Slot::Item(ID_PILL_DOCK_LEFT),
    Slot::Item(ID_PILL_DOCK_RIGHT),
    Slot::Separator,
    Slot::Item(ID_MICROPHONE),
    Slot::Item(ID_LANGUAGES),
    Slot::Separator,
    Slot::Item(ID_OPEN_SCRATCHPAD),
    Slot::Item(ID_SHOW),
];

/// One row of the pill's right-click menu as `lib.rs` draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillMenuRow {
    /// A plain item: the id and label are the registry's own.
    Action {
        id: &'static str,
        label: &'static str,
        enabled: bool,
    },
    /// A dock row, checked on the dock the pill has now.
    Dock {
        id: &'static str,
        label: &'static str,
        checked: bool,
    },
    /// The Microphone submenu, drawn from [`MIC_SHOWN`] (the menu bar's rows).
    Microphone {
        label: &'static str,
    },
    /// The Languages submenu, drawn from [`LANG_SHOWN`] (the menu bar's rows).
    Languages {
        label: &'static str,
    },
    Separator,
}

/// What the pill menu is decided from — the same live state `sync_tray`
/// reads, gathered by `lib.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PillMenuInputs {
    /// `AppSettings::show_floating_pill`.
    pub show_floating_pill: bool,
    /// A [`PillHide`] is running.
    pub hidden: bool,
    /// What Yap is doing (`lib.rs::update_activity`).
    pub activity: Activity,
    /// The dock the pill has now (`AppSettings::pill_position`).
    pub dock: PillDock,
}

/// A take (the microphone open, a take being transcribed or polished) or a
/// meeting is running — the pill must stay where the user can see it.
pub fn take_or_meeting_running(a: Activity) -> bool {
    a.dictating || a.take_in_flight || a.polishing || a.meeting
}

/// "Hide Pill for 1 Hour" on the PILL: the menu bar's rule
/// ([`pill_menu_state`]) and, on top, greyed while a take or a meeting runs —
/// a live microphone is never hidden from the surface the user is looking at.
pub fn pill_hide_enabled(inputs: PillMenuInputs) -> bool {
    pill_menu_state(inputs.show_floating_pill, inputs.hidden).hide_enabled
        && !take_or_meeting_running(inputs.activity)
}

/// THE decision for the pill's right-click menu: [`PILL_MENU_LAYOUT`] with
/// each id resolved through [`item`] — labels are never written twice.
pub fn pill_context_menu(inputs: PillMenuInputs) -> Vec<PillMenuRow> {
    PILL_MENU_LAYOUT
        .iter()
        .map(|slot| match *slot {
            Slot::Separator => PillMenuRow::Separator,
            Slot::Item(id) => {
                let row = item(id).unwrap_or_else(|| panic!("tray_menu: pill menu names {id:?}"));
                match row.kind {
                    ItemKind::MicrophoneSubmenu => PillMenuRow::Microphone { label: row.label },
                    ItemKind::LanguageSubmenu => PillMenuRow::Languages { label: row.label },
                    ItemKind::Action(TrayCommand::DockPill(dock)) => PillMenuRow::Dock {
                        id: row.id,
                        label: row.label,
                        checked: dock == inputs.dock,
                    },
                    ItemKind::Action(TrayCommand::HidePillForAnHour) => PillMenuRow::Action {
                        id: row.id,
                        label: row.label,
                        enabled: pill_hide_enabled(inputs),
                    },
                    ItemKind::Action(_) => PillMenuRow::Action {
                        id: row.id,
                        label: row.label,
                        enabled: true,
                    },
                    ItemKind::StateLabel | ItemKind::KindSubmenu | ItemKind::UpcomingSubmenu => {
                        PillMenuRow::Action {
                            id: row.id,
                            label: row.label,
                            enabled: false,
                        }
                    }
                }
            }
        })
        .collect()
}

// ─── The meeting item (yap-1106) ─────────────────────────────────────────────

/// The tray item's idle label. A verb and a noun, so the item reads the same way
/// to somebody who has never heard of this feature — the acceptance line for
/// this item is a user who has not read a changelog.
pub const MEETING_START_LABEL: &str = "Record a meeting";
/// The label while a meeting records, followed by " · <clock>".
pub const MEETING_STOP_LABEL: &str = "Stop meeting";
/// The label while a stop is in flight. Disabled: a second press during a stop
/// must never become a start (yap-1106 (e)).
pub const MEETING_STOPPING_LABEL: &str = "Stopping…";

/// Everything the menu bar shows for the meeting, decided from ONE status.
/// `lib.rs` applies it; nothing here touches a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingMenu {
    /// The toggle item's text ("Record a meeting" / "Stop meeting · 12:00" /
    /// "Stopping…" / "Record a meeting (unavailable)").
    pub toggle_text: String,
    pub toggle_enabled: bool,
    /// The "Record a meeting as…" submenu: only while idle AND available.
    pub kind_enabled: bool,
    /// The disabled "Recording · <title>" header. `Some` exactly while the
    /// toggle item sits at the top of the menu as "Stop meeting".
    pub header: Option<String>,
}

/// The clock the Stop item carries, to the MINUTE: "12:00", "01:02:00". The
/// item is refreshed when this string changes, so the menu is mutated at most
/// once a minute (OS-12 still holds: no per-second menu mutation).
pub fn tray_meeting_clock(elapsed_seconds: u64) -> String {
    let minute = (elapsed_seconds / 60) * 60;
    let full = crate::meetings::format_offset(minute as f64);
    full.strip_prefix("00:").map(str::to_string).unwrap_or(full)
}

/// (text, enabled) for the tray's meeting item — the pure function the
/// acceptance names. `stop_in_flight` is lib.rs's "a stop was requested and
/// has not returned yet" flag (yap-1106 (e)).
pub fn tray_meeting_label(
    status: &meeting_control::MeetingStatus,
    stop_in_flight: bool,
) -> (String, bool) {
    let m = tray_meeting_menu(status, stop_in_flight);
    (m.toggle_text, m.toggle_enabled)
}

/// The whole meeting block of the menu for `status`.
pub fn tray_meeting_menu(
    status: &meeting_control::MeetingStatus,
    stop_in_flight: bool,
) -> MeetingMenu {
    let header = |status: &meeting_control::MeetingStatus| {
        let title = status.title.as_deref().unwrap_or("").trim();
        Some(if title.is_empty() {
            "Recording".to_string()
        } else {
            format!("Recording · {title}")
        })
    };
    if stop_in_flight {
        return MeetingMenu {
            toggle_text: MEETING_STOPPING_LABEL.to_string(),
            toggle_enabled: false,
            kind_enabled: false,
            header: if status.recording {
                header(status)
            } else {
                None
            },
        };
    }
    if status.recording {
        return MeetingMenu {
            toggle_text: format!(
                "{MEETING_STOP_LABEL} · {}",
                tray_meeting_clock(status.elapsed_seconds)
            ),
            toggle_enabled: true,
            kind_enabled: false,
            header: header(status),
        };
    }
    if !status.capture_available {
        return MeetingMenu {
            toggle_text: format!("{MEETING_START_LABEL} (unavailable)"),
            toggle_enabled: false,
            kind_enabled: false,
            header: None,
        };
    }
    MeetingMenu {
        toggle_text: MEETING_START_LABEL.to_string(),
        toggle_enabled: true,
        kind_enabled: true,
        header: None,
    }
}

/// What the menu bar LAST SUCCESSFULLY showed for the meeting. The old latch
/// was a process-wide `AtomicBool` flipped BEFORE the main-thread hop ran, so a
/// hop that failed (or found no tray yet) left "Record a meeting" on screen for
/// the rest of the session with nothing to retry it. This one records a state
/// only after the setter returned `Ok`, so every later status re-tries.
#[derive(Debug, Default)]
pub struct MeetingMenuLatch {
    shown: Mutex<Option<MeetingMenu>>,
}

impl MeetingMenuLatch {
    pub const fn new() -> Self {
        MeetingMenuLatch {
            shown: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<MeetingMenu>> {
        self.shown.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// What is on screen, if a set has ever succeeded.
    pub fn shown(&self) -> Option<MeetingMenu> {
        self.lock().clone()
    }

    /// Does showing `want` need a menu mutation? False when it is already up.
    pub fn needs(&self, want: &MeetingMenu) -> bool {
        self.lock().as_ref() != Some(want)
    }

    /// Forget what was shown — the tray was (re)built, so nothing is.
    pub fn reset(&self) {
        *self.lock() = None;
    }

    /// Show `want` through `set`, and record it ONLY if `set` succeeded.
    /// `Ok(false)` = already shown, `set` not called; `Ok(true)` = set and
    /// recorded; `Err` = `set` failed, the latch is unchanged and the next
    /// status will try again.
    pub fn apply<F>(&self, want: MeetingMenu, set: F) -> Result<bool, String>
    where
        F: FnOnce(&MeetingMenu, Option<&MeetingMenu>) -> Result<(), String>,
    {
        let mut shown = self.lock();
        if shown.as_ref() == Some(&want) {
            return Ok(false);
        }
        set(&want, shown.as_ref())?;
        *shown = Some(want);
        Ok(true)
    }
}

// ─── Upcoming meetings (WF-07) ───────────────────────────────────────────────

/// The disabled line when the calendar is connected but today is clear.
pub const NO_MORE_MEETINGS_LABEL: &str = "No more meetings today";

/// One "Record …" child of "Today's Meetings".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpcomingRow {
    /// `upcoming_record_<index>` — resolved by [`command_for`].
    pub id: String,
    pub text: String,
    /// The occurrence key Record snoozes (`meeting_detect::CalendarEvent::offer_key`).
    pub key: String,
    /// The real event title, used to name the recording (never shown when the
    /// user hid titles).
    pub title: Option<String>,
    pub has_link: bool,
}

/// The calendar rows of the menu, decided from ONE snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpcomingMenu {
    /// "Next: Standup · 2:00 PM" / "Next meeting 2:00 PM" / "No more meetings today".
    pub next_text: String,
    /// Empty = no "Today's Meetings" submenu.
    pub rows: Vec<UpcomingRow>,
    /// The "Hide Meeting Titles" check.
    pub hide_titles: bool,
}

/// "2:00 PM" for `start_unix` at `utc_offset_secs`.
pub fn meeting_clock(start_unix: i64, utc_offset_secs: i32) -> String {
    let offset = chrono::FixedOffset::east_opt(utc_offset_secs)
        .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).expect("UTC is a valid offset"));
    match chrono::DateTime::from_timestamp(start_unix, 0) {
        Some(t) => t.with_timezone(&offset).format("%-I:%M %p").to_string(),
        None => String::new(),
    }
}

/// THE decision for the calendar rows. Titles are shown only when the user has
/// not hidden them AND the event has one; a hidden title reads "Next meeting
/// 2:00 PM", exactly as WF-07 asks.
pub fn upcoming_menu(snap: &crate::meeting_detect::UpcomingSnapshot) -> UpcomingMenu {
    let shown_title = |m: &crate::meeting_detect::UpcomingMeeting| {
        if snap.hide_titles {
            None
        } else {
            m.title.clone()
        }
    };
    let next_text = match snap.meetings.first() {
        None => NO_MORE_MEETINGS_LABEL.to_string(),
        Some(m) => {
            let clock = meeting_clock(m.start_unix, snap.utc_offset_secs);
            match shown_title(m) {
                Some(t) => format!("Next: {t} \u{b7} {clock}"),
                None => format!("Next meeting {clock}"),
            }
        }
    };
    let rows = snap
        .meetings
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let clock = meeting_clock(m.start_unix, snap.utc_offset_secs);
            UpcomingRow {
                id: format!("{UPCOMING_RECORD_PREFIX}{i}"),
                text: match shown_title(m) {
                    Some(t) => format!("Record \u{201c}{t}\u{201d} \u{b7} {clock}"),
                    None => format!("Record Meeting at {clock}"),
                },
                key: m.key.clone(),
                title: m.title.clone(),
                has_link: m.has_link,
            }
        })
        .collect();
    UpcomingMenu {
        next_text,
        rows,
        hide_titles: snap.hide_titles,
    }
}

/// What the calendar rows LAST SUCCESSFULLY showed — the click on
/// `upcoming_record_<i>` resolves against THIS, never against a newer snapshot
/// the menu has not drawn yet, so Record always records the row the user saw.
#[derive(Debug, Default)]
pub struct UpcomingLatch {
    shown: Mutex<Option<UpcomingMenu>>,
}

impl UpcomingLatch {
    pub const fn new() -> Self {
        UpcomingLatch {
            shown: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<UpcomingMenu>> {
        self.shown.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Record what the menu now shows (`None` = no calendar rows).
    pub fn set(&self, shown: Option<UpcomingMenu>) {
        *self.lock() = shown;
    }

    pub fn shown(&self) -> Option<UpcomingMenu> {
        self.lock().clone()
    }

    /// The row at `index` of what is on screen.
    pub fn row(&self, index: usize) -> Option<UpcomingRow> {
        self.lock()
            .as_ref()
            .and_then(|m| m.rows.get(index).cloned())
    }
}

/// The process's one latch (the menu is a process singleton too).
pub static UPCOMING_SHOWN: UpcomingLatch = UpcomingLatch::new();

/// [`UPCOMING_SHOWN`]`.row(index)`.
pub fn upcoming_row(index: usize) -> Option<UpcomingRow> {
    UPCOMING_SHOWN.row(index)
}

// ─── WF-09 — the Microphone submenu ──────────────────────────────────────────

/// One child of "Microphone".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicRow {
    /// `mic_pick_<index>` — resolved by [`command_for`].
    pub id: String,
    pub text: String,
    /// The check: the device the next take will open.
    pub checked: bool,
    /// `false` for the "not connected" line of a pinned, unplugged device.
    pub enabled: bool,
    /// What clicking saves: `None` = System Default, else the stable id.
    pub choice: Option<String>,
    /// The device name saved next to the id (shown while it is unplugged).
    pub name: Option<String>,
}

/// The children of "Microphone", decided from ONE device snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicMenu {
    pub rows: Vec<MicRow>,
}

/// THE decision for the Microphone submenu. "System Default" is always the
/// first row (with the device it currently means, when there is one); then
/// every input plugged in now. The check sits on the device the next take
/// opens — so when the pinned mic is unplugged, the check moves to System
/// Default and the pin stays visible as a disabled "— Not Connected" row,
/// rather than vanishing from the menu.
pub fn microphone_menu(list: &crate::mic_select::MicList) -> MicMenu {
    let mut rows = Vec::with_capacity(list.devices.len() + 2);
    let default_text = match list.system_default_name().filter(|n| !n.is_empty()) {
        Some(name) => format!("{} ({name})", crate::mic_select::SYSTEM_DEFAULT_LABEL),
        None => crate::mic_select::SYSTEM_DEFAULT_LABEL.to_string(),
    };
    rows.push(MicRow {
        id: format!("{MIC_PICK_PREFIX}0"),
        text: default_text,
        checked: list.chosen_id.is_none() || list.chosen_missing,
        enabled: true,
        choice: None,
        name: None,
    });
    for d in &list.devices {
        let text = if d.name.is_empty() {
            "Unnamed Input".to_string()
        } else {
            d.name.clone()
        };
        rows.push(MicRow {
            id: format!("{MIC_PICK_PREFIX}{}", rows.len()),
            text,
            checked: !list.chosen_missing && list.chosen_id.as_deref() == Some(d.id.as_str()),
            enabled: true,
            choice: Some(d.id.clone()),
            name: Some(d.name.clone()),
        });
    }
    if list.chosen_missing {
        if let Some(id) = list.chosen_id.as_ref() {
            let name = list
                .chosen_name
                .clone()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "Saved microphone".to_string());
            rows.push(MicRow {
                id: format!("{MIC_PICK_PREFIX}{}", rows.len()),
                text: format!("{name} \u{2014} Not Connected"),
                checked: false,
                enabled: false,
                choice: Some(id.clone()),
                name: Some(name),
            });
        }
    }
    MicMenu { rows }
}

/// What the Microphone submenu LAST SUCCESSFULLY showed — a click on
/// `mic_pick_<i>` resolves against THIS, never against a newer device list the
/// menu has not drawn yet, so a pick always saves the row the user saw.
#[derive(Debug, Default)]
pub struct MicLatch {
    shown: Mutex<Option<MicMenu>>,
}

impl MicLatch {
    pub const fn new() -> Self {
        MicLatch {
            shown: Mutex::new(None),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<MicMenu>> {
        self.shown.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn set(&self, shown: Option<MicMenu>) {
        *self.lock() = shown;
    }

    /// The whole submenu last drawn — the pill's right-click menu (WF-04)
    /// draws its Microphone submenu from THIS, so a pick there resolves
    /// against exactly the rows it showed.
    pub fn snapshot(&self) -> Option<MicMenu> {
        self.lock().clone()
    }

    pub fn row(&self, index: usize) -> Option<MicRow> {
        self.lock()
            .as_ref()
            .and_then(|m| m.rows.get(index).cloned())
    }
}

/// The process's one Microphone latch.
pub static MIC_SHOWN: MicLatch = MicLatch::new();

/// [`MIC_SHOWN`]`.row(index)`.
pub fn microphone_row(index: usize) -> Option<MicRow> {
    MIC_SHOWN.row(index)
}

// ─── WF-11 — the Languages submenu ───────────────────────────────────────────

/// What the bar's chip calls "auto" (`languagePicker.ts` `AUTO`).
pub const LANGUAGE_PICK_AUTO: &str = "auto";

/// One pick child of "Languages".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageRow {
    /// `lang_pick_<index>` — resolved by [`command_for`].
    pub id: String,
    pub text: String,
    /// `"auto"` or a language code — the same value as the chip's
    /// `currentPick` (`languagePicker.ts`).
    pub pick: String,
    /// The check: what the next take uses (the chip's label).
    pub checked: bool,
    /// What clicking writes through the one language writer.
    pub prefs: crate::languages::LanguagePrefs,
}

/// The pick children of "Languages", decided from ONE picker view. The
/// "Language Settings…" row ([`ID_LANGUAGE_SETTINGS`]) is always appended
/// after them by `lib.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageMenu {
    pub rows: Vec<LanguageRow>,
}

impl LanguageMenu {
    /// The pick that carries the check (`"auto"` or a code), if any row does.
    pub fn checked_pick(&self) -> Option<&str> {
        self.rows
            .iter()
            .find(|r| r.checked)
            .map(|r| r.pick.as_str())
    }
}

/// The pick the next take uses — mirror of `languagePicker.ts` `currentPick`.
pub fn current_language_pick(o: &crate::languages::LanguageOptions) -> String {
    if o.mode == crate::languages::DecodeMode::AutoWithinSet {
        LANGUAGE_PICK_AUTO.to_string()
    } else {
        o.prefs.primary.clone()
    }
}

/// The prefs a pick writes — mirror of `languagePicker.ts` `pickPrefs`, the
/// same function the chip's click uses, so the tray and the chip can never
/// write two different things for the same choice.
pub fn prefs_for_pick(
    prefs: &crate::languages::LanguagePrefs,
    pick: &str,
) -> crate::languages::LanguagePrefs {
    if pick == LANGUAGE_PICK_AUTO {
        crate::languages::LanguagePrefs {
            auto: true,
            ..prefs.clone()
        }
    } else {
        crate::languages::LanguagePrefs {
            primary: pick.to_string(),
            set: prefs.set.clone(),
            auto: false,
        }
    }
}

/// THE decision for the Languages submenu — the same list, in the same order,
/// as the chip's click cycle (`languagePicker.ts` `pickCycle`): Auto-detect
/// first (only when the set has a choice in it and the model is not known to
/// be unable to detect), then each language of the set in the picker's own
/// order. The check sits on what the chip shows.
pub fn language_menu(o: &crate::languages::LanguageOptions) -> LanguageMenu {
    let rank = |code: &str| {
        o.offered
            .iter()
            .position(|c| c.code == code)
            .unwrap_or(o.offered.len())
    };
    let mut langs: Vec<String> = o.prefs.set.clone();
    langs.sort_by_key(|c| rank(c.as_str()));
    let mut picks: Vec<String> = Vec::with_capacity(langs.len() + 1);
    if o.prefs.set.len() > 1 && o.supports_detect != Some(false) {
        picks.push(LANGUAGE_PICK_AUTO.to_string());
    }
    picks.extend(langs);
    let current = current_language_pick(o);
    let rows = picks
        .into_iter()
        .enumerate()
        .map(|(i, pick)| {
            let text = if pick == LANGUAGE_PICK_AUTO {
                LANGUAGE_AUTO_LABEL.to_string()
            } else {
                o.offered
                    .iter()
                    .find(|c| c.code == pick)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| crate::languages::language_name(&pick))
            };
            LanguageRow {
                id: format!("{LANG_PICK_PREFIX}{i}"),
                text,
                checked: pick == current,
                prefs: prefs_for_pick(&o.prefs, &pick),
                pick,
            }
        })
        .collect();
    LanguageMenu { rows }
}

/// What the Languages submenu LAST SUCCESSFULLY showed, plus the last probed
/// model support — a click on `lang_pick_<i>` resolves against the rows on
/// screen, and a tray redraw never loads the speech model just to draw a
/// check (it reuses what the last `language_options` probe found).
#[derive(Debug, Default)]
pub struct LanguageLatch {
    shown: Mutex<Option<LanguageMenu>>,
    support: Mutex<Option<crate::languages::LanguageSupport>>,
}

impl LanguageLatch {
    pub const fn new() -> Self {
        LanguageLatch {
            shown: Mutex::new(None),
            support: Mutex::new(None),
        }
    }

    pub fn set(&self, shown: Option<LanguageMenu>) {
        *self.shown.lock().unwrap_or_else(|p| p.into_inner()) = shown;
    }

    /// The whole submenu last drawn — the pill's right-click menu (WF-04)
    /// draws its Languages submenu from THIS (see [`MicLatch::snapshot`]).
    pub fn snapshot(&self) -> Option<LanguageMenu> {
        self.shown.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn row(&self, index: usize) -> Option<LanguageRow> {
        self.shown
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .and_then(|m| m.rows.get(index).cloned())
    }

    /// Record the probe `language_options` just ran (`None` = no model).
    pub fn remember_support(&self, support: Option<crate::languages::LanguageSupport>) {
        *self.support.lock().unwrap_or_else(|p| p.into_inner()) = support;
    }

    pub fn support(&self) -> Option<crate::languages::LanguageSupport> {
        self.support
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}

/// The process's one Languages latch.
pub static LANG_SHOWN: LanguageLatch = LanguageLatch::new();

/// [`LANG_SHOWN`]`.row(index)`.
pub fn language_row(index: usize) -> Option<LanguageRow> {
    LANG_SHOWN.row(index)
}

#[cfg(test)]
mod language_menu_tests {
    use super::*;
    use crate::languages::{DecodeMode, LanguageChoice, LanguageOptions, LanguagePrefs};

    fn offered() -> Vec<LanguageChoice> {
        [("en", "English"), ("es", "Spanish"), ("fr", "French")]
            .iter()
            .map(|(c, n)| LanguageChoice {
                code: (*c).into(),
                name: (*n).into(),
            })
            .collect()
    }

    fn opts(primary: &str, set: &[&str], auto: bool, mode: DecodeMode) -> LanguageOptions {
        LanguageOptions {
            prefs: LanguagePrefs {
                primary: primary.into(),
                set: set.iter().map(|s| (*s).to_string()).collect(),
                auto,
            },
            offered: offered(),
            supports_detect: Some(true),
            mode,
            notice: None,
            max_set: 4,
        }
    }

    #[test]
    fn auto_then_each_language_with_the_check_on_auto() {
        let m = language_menu(&opts("es", &["es", "en"], true, DecodeMode::AutoWithinSet));
        let texts: Vec<&str> = m.rows.iter().map(|r| r.text.as_str()).collect();
        // Picker order (English before Spanish), whichever is primary.
        assert_eq!(texts, ["Auto-detect", "English", "Spanish"]);
        assert_eq!(m.checked_pick(), Some("auto"));
        assert_eq!(m.rows.iter().filter(|r| r.checked).count(), 1);
        assert_eq!(
            command_for(&m.rows[2].id),
            Some(TrayCommand::PickLanguage(2))
        );
    }

    #[test]
    fn the_active_language_carries_the_check() {
        let m = language_menu(&opts("es", &["es", "en"], false, DecodeMode::Single));
        assert_eq!(m.checked_pick(), Some("es"));
        assert!(m.rows[2].checked && !m.rows[0].checked && !m.rows[1].checked);
    }

    #[test]
    fn a_pick_writes_what_the_chip_would() {
        let o = opts("en", &["en", "es"], true, DecodeMode::AutoWithinSet);
        let m = language_menu(&o);
        assert_eq!(
            m.rows[2].prefs,
            LanguagePrefs {
                primary: "es".into(),
                set: vec!["en".into(), "es".into()],
                auto: false
            }
        );
        let single = opts("es", &["en", "es"], false, DecodeMode::Single);
        let back = language_menu(&single);
        assert_eq!(back.rows[0].pick, "auto");
        assert!(back.rows[0].prefs.auto);
        assert_eq!(back.rows[0].prefs.primary, "es");
    }

    #[test]
    fn no_auto_row_when_the_model_cannot_detect_or_the_set_has_one_language() {
        let mut o = opts("en", &["en", "es"], true, DecodeMode::FallbackNoDetect);
        o.supports_detect = Some(false);
        let m = language_menu(&o);
        assert!(m.rows.iter().all(|r| r.pick != "auto"));
        assert_eq!(m.checked_pick(), Some("en"));
        let one = language_menu(&opts("en", &["en"], true, DecodeMode::Single));
        assert_eq!(one.rows.len(), 1);
        assert_eq!(one.rows[0].text, "English");
        assert!(one.rows[0].checked);
    }

    #[test]
    fn language_ids_round_trip_and_settings_row_dispatches() {
        assert_eq!(language_index_for_menu_id("lang_pick_0"), Some(0));
        assert_eq!(language_index_for_menu_id("lang_pick_3"), Some(3));
        assert_eq!(language_index_for_menu_id("lang_pick_"), None);
        assert_eq!(language_index_for_menu_id("lang_pick_x"), None);
        assert_eq!(command_for(ID_LANGUAGES), None);
        assert_eq!(
            command_for(ID_LANGUAGE_SETTINGS),
            Some(TrayCommand::OpenLanguageSettings)
        );
        assert_eq!(
            command_for("lang_pick_1"),
            Some(TrayCommand::PickLanguage(1))
        );
    }

    #[test]
    fn the_latch_resolves_against_what_was_shown() {
        let latch = LanguageLatch::new();
        assert_eq!(latch.row(0), None);
        let m = language_menu(&opts("en", &["en", "es"], true, DecodeMode::AutoWithinSet));
        latch.set(Some(m.clone()));
        assert_eq!(latch.row(1), Some(m.rows[1].clone()));
        latch.set(None);
        assert_eq!(latch.row(1), None);
    }
}

#[cfg(test)]
mod microphone_menu_tests {
    use super::*;
    use crate::mic_select::{MicDevice, MicList};

    fn dev(id: &str, name: &str, default: bool) -> MicDevice {
        MicDevice {
            id: id.into(),
            name: name.into(),
            is_system_default: default,
            is_virtual: false,
        }
    }

    fn two_mics() -> Vec<MicDevice> {
        vec![
            dev("coreaudio:Built-in", "MacBook Pro Microphone", true),
            dev("coreaudio:MV7", "Shure MV7", false),
        ]
    }

    #[test]
    fn system_default_is_first_and_checked_when_nothing_is_picked() {
        let m = microphone_menu(&MicList {
            devices: two_mics(),
            ..MicList::default()
        });
        assert_eq!(m.rows.len(), 3);
        assert_eq!(m.rows[0].text, "System Default (MacBook Pro Microphone)");
        assert!(m.rows[0].checked);
        assert_eq!(m.rows[0].choice, None);
        assert!(m.rows[1..].iter().all(|r| !r.checked));
        assert_eq!(
            command_for(&m.rows[2].id),
            Some(TrayCommand::PickMicrophone(2))
        );
    }

    #[test]
    fn the_picked_device_carries_the_check() {
        let m = microphone_menu(&MicList {
            devices: two_mics(),
            chosen_id: Some("coreaudio:MV7".into()),
            chosen_name: Some("Shure MV7".into()),
            ..MicList::default()
        });
        assert!(!m.rows[0].checked);
        assert!(m.rows[2].checked);
        assert_eq!(m.rows[2].choice.as_deref(), Some("coreaudio:MV7"));
    }

    #[test]
    fn an_unplugged_pick_moves_the_check_to_default_and_stays_visible() {
        let m = microphone_menu(&MicList {
            devices: vec![dev("coreaudio:Built-in", "MacBook Pro Microphone", true)],
            chosen_id: Some("coreaudio:MV7".into()),
            chosen_name: Some("Shure MV7".into()),
            chosen_missing: true,
            ..MicList::default()
        });
        assert!(m.rows[0].checked, "the take falls back to the default");
        let last = m.rows.last().expect("a not-connected row");
        assert_eq!(last.text, "Shure MV7 \u{2014} Not Connected");
        assert!(!last.enabled && !last.checked);
    }

    #[test]
    fn microphone_ids_round_trip() {
        assert_eq!(microphone_index_for_menu_id("mic_pick_0"), Some(0));
        assert_eq!(microphone_index_for_menu_id("mic_pick_12"), Some(12));
        assert_eq!(microphone_index_for_menu_id("mic_pick_"), None);
        assert_eq!(microphone_index_for_menu_id("mic_pick_x"), None);
        assert_eq!(command_for(ID_MICROPHONE), None);
    }
}

#[cfg(test)]
mod upcoming_menu_tests {
    use super::*;
    use crate::meeting_detect::{UpcomingMeeting, UpcomingSnapshot};

    fn snap(hide_titles: bool, meetings: Vec<(&str, Option<&str>, i64)>) -> UpcomingSnapshot {
        UpcomingSnapshot {
            meetings: meetings
                .into_iter()
                .map(|(key, title, start)| UpcomingMeeting {
                    key: key.to_string(),
                    title: title.map(str::to_string),
                    start_unix: start,
                    has_link: false,
                })
                .collect(),
            hide_titles,
            utc_offset_secs: 0,
        }
    }

    // 14:00:00Z on a whole day boundary + 14 h.
    const TWO_PM: i64 = 1_800_014_400 - (1_800_014_400 % 86_400) + 14 * 3600;

    #[test]
    fn upcoming_menu_shows_next_meeting_title_and_time() {
        let m = upcoming_menu(&snap(
            false,
            vec![
                ("cal:a:1", Some("Standup"), TWO_PM),
                ("cal:b:2", None, TWO_PM + 5400),
            ],
        ));
        assert_eq!(m.next_text, "Next: Standup \u{b7} 2:00 PM");
        assert_eq!(m.rows.len(), 2);
        assert_eq!(m.rows[0].id, "upcoming_record_0");
        assert_eq!(
            m.rows[0].text,
            "Record \u{201c}Standup\u{201d} \u{b7} 2:00 PM"
        );
        assert_eq!(m.rows[1].text, "Record Meeting at 3:30 PM");
        assert!(!m.hide_titles);
        assert_eq!(meeting_clock(TWO_PM, -6 * 3600), "8:00 AM");
    }

    #[test]
    fn upcoming_menu_hide_titles_shows_only_the_time() {
        let m = upcoming_menu(&snap(true, vec![("cal:a:1", Some("Board call"), TWO_PM)]));
        assert_eq!(m.next_text, "Next meeting 2:00 PM");
        assert_eq!(m.rows[0].text, "Record Meeting at 2:00 PM");
        assert!(!m.next_text.contains("Board"));
        assert!(!m.rows[0].text.contains("Board"));
        // The recording is still named after the event.
        assert_eq!(m.rows[0].title.as_deref(), Some("Board call"));
        assert!(m.hide_titles);
    }

    #[test]
    fn upcoming_menu_empty_day_says_so() {
        let m = upcoming_menu(&snap(false, vec![]));
        assert_eq!(m.next_text, NO_MORE_MEETINGS_LABEL);
        assert!(m.rows.is_empty(), "no empty submenu");
    }

    #[test]
    fn upcoming_record_ids_dispatch_to_the_row_on_screen() {
        assert_eq!(
            command_for("upcoming_record_0"),
            Some(TrayCommand::RecordUpcoming(0))
        );
        assert_eq!(
            command_for("upcoming_record_7"),
            Some(TrayCommand::RecordUpcoming(7))
        );
        assert_eq!(command_for("upcoming_record_"), None);
        assert_eq!(command_for("upcoming_record_x1"), None);
        assert_eq!(command_for("upcoming_record_-1"), None);
        assert_eq!(command_for(ID_UPCOMING), None);
        assert_eq!(command_for(ID_NEXT_MEETING), None);
        assert_eq!(
            command_for(ID_HIDE_MEETING_TITLES),
            Some(TrayCommand::ToggleHideMeetingTitles)
        );
        for id in [ID_NEXT_MEETING, ID_UPCOMING, ID_HIDE_MEETING_TITLES] {
            assert!(is_upcoming_row(id));
            assert!(
                INSERTED_ITEMS.contains(&id),
                "{id} is inserted, not laid out"
            );
        }
        assert!(!is_upcoming_row(ID_MEETING_TOGGLE));
        let latch = UpcomingLatch::new();
        assert_eq!(latch.row(0), None);
        let m = upcoming_menu(&snap(false, vec![("cal:a:1", Some("Standup"), TWO_PM)]));
        latch.set(Some(m.clone()));
        assert_eq!(latch.row(0).map(|r| r.key), Some("cal:a:1".to_string()));
        assert_eq!(latch.row(1), None, "a stale index records nothing");
        latch.set(None);
        assert_eq!(latch.shown(), None);
    }

    /// yap-1106-menubar-redesign — Meeting is the first group under the status
    /// header, Start/Stop first; then Dictation, Open, Pill, Quit.
    #[test]
    fn menubar_layout_groups_in_order() {
        let groups: Vec<Vec<&str>> = MENU_LAYOUT
            .split(|s| *s == Slot::Separator)
            .map(|g| {
                g.iter()
                    .filter_map(|s| match s {
                        Slot::Item(id) => Some(*id),
                        Slot::Separator => None,
                    })
                    .collect()
            })
            .collect();
        assert_eq!(groups[0], [ID_LICENSE_HEADER]);
        assert_eq!(groups[1], [ID_MEETING_TOGGLE, ID_MEETING_KIND]);
        assert_eq!(groups[2][0], ID_TOGGLE);
        assert!(groups[2].contains(&ID_PASTE_LAST) && groups[2].contains(&ID_COPY_LAST));
        assert_eq!(groups[3][..3], [ID_SHOW, ID_HISTORY, ID_SETTINGS]);
        assert_eq!(groups[4], [ID_HIDE_PILL, ID_SHOW_PILL]);
        assert_eq!(groups.last().map(|g| *g.last().unwrap()), Some(ID_QUIT));
    }
}

#[cfg(test)]
mod meeting_menu_tests {
    use super::*;
    use crate::meeting_control::MeetingStatus;
    use std::cell::RefCell;

    fn status(
        recording: bool,
        available: bool,
        elapsed: u64,
        title: Option<&str>,
    ) -> MeetingStatus {
        MeetingStatus {
            recording,
            id: recording.then(|| "m1".to_string()),
            title: title.map(str::to_string),
            elapsed_seconds: elapsed,
            elapsed_label: crate::meetings::format_offset(elapsed as f64),
            capture_available: available,
            unavailable_reason: (!available).then(|| "no capture engine".to_string()),
            system_audio: None,
            call_audio: None,
        }
    }

    #[test]
    fn tray_meeting_label_idle() {
        let (text, enabled) = tray_meeting_label(&status(false, true, 0, None), false);
        assert_eq!(text, "Record a meeting");
        assert!(enabled);
        let m = tray_meeting_menu(&status(false, true, 0, None), false);
        assert!(m.kind_enabled);
        assert_eq!(m.header, None);
    }

    #[test]
    fn tray_meeting_label_recording() {
        let s = status(true, true, 12 * 60 + 4, Some("Standup"));
        let (text, enabled) = tray_meeting_label(&s, false);
        assert_eq!(text, "Stop meeting · 12:00");
        assert!(enabled);
        let m = tray_meeting_menu(&s, false);
        assert!(!m.kind_enabled, "no kind to pick for a meeting that runs");
        assert_eq!(m.header.as_deref(), Some("Recording · Standup"));
        // Over an hour keeps the hours; under a minute shows 00:00.
        assert_eq!(tray_meeting_clock(3_725), "01:02:00");
        assert_eq!(tray_meeting_clock(59), "00:00");
        // Within one minute the menu text is STABLE (OS-12: no per-second mutation).
        let a = tray_meeting_menu(&status(true, true, 120, None), false);
        let b = tray_meeting_menu(&status(true, true, 179, None), false);
        assert_eq!(a, b);
        assert_eq!(a.header.as_deref(), Some("Recording"));
    }

    #[test]
    fn tray_meeting_label_unavailable() {
        let (text, enabled) = tray_meeting_label(&status(false, false, 0, None), false);
        assert_eq!(text, "Record a meeting (unavailable)");
        assert!(!enabled);
        assert!(!tray_meeting_menu(&status(false, false, 0, None), false).kind_enabled);
    }

    /// The launch race the item is about: a status reaches the sink BEFORE the
    /// tray exists (a recovered meeting, an early emit). The setter fails, the
    /// latch records nothing, and the sync that runs once the tray is built
    /// lands the idle label from `controller.status()`.
    #[test]
    fn tray_meeting_label_recovered_at_launch() {
        let latch = MeetingMenuLatch::new();
        let early = tray_meeting_menu(&status(false, true, 0, None), false);
        let r = latch.apply(early.clone(), |_, _| Err("tray not built yet".into()));
        assert!(r.is_err());
        assert_eq!(latch.shown(), None, "a failed set must not be recorded");
        assert!(latch.needs(&early), "the same state must be retried later");
        // The tray is built: the retry succeeds and is recorded.
        let applied = RefCell::new(Vec::new());
        let r = latch.apply(early.clone(), |want, prev| {
            assert_eq!(prev, None);
            applied.borrow_mut().push(want.toggle_text.clone());
            Ok(())
        });
        assert_eq!(r, Ok(true));
        assert_eq!(applied.borrow().as_slice(), ["Record a meeting"]);
        assert_eq!(latch.shown(), Some(early.clone()));
        assert!(!latch.needs(&early));
        // Already shown: the setter is not called again.
        let r = latch.apply(early, |_, _| panic!("must not set an identical state"));
        assert_eq!(r, Ok(false));
    }

    /// A stop in flight shows "Stopping…" disabled, whatever the status says,
    /// and keeps the header until the stop lands.
    #[test]
    fn tray_meeting_label_stopping() {
        let s = status(true, true, 30, Some("Standup"));
        let (text, enabled) = tray_meeting_label(&s, true);
        assert_eq!(text, "Stopping…");
        assert!(!enabled);
        let m = tray_meeting_menu(&s, true);
        assert_eq!(m.header.as_deref(), Some("Recording · Standup"));
        assert!(!m.kind_enabled);
    }

    /// The latch never records a label it failed to set — through an edge.
    #[test]
    fn tray_meeting_label_latch_never_records_a_failed_set() {
        let latch = MeetingMenuLatch::new();
        let idle = tray_meeting_menu(&status(false, true, 0, None), false);
        assert_eq!(latch.apply(idle.clone(), |_, _| Ok(())), Ok(true));
        let rec = tray_meeting_menu(&status(true, true, 0, Some("T")), false);
        // The hop to the main thread failed: still idle on screen.
        let r = latch.apply(rec.clone(), |_, _| Err("run_on_main_thread failed".into()));
        assert_eq!(r, Err("run_on_main_thread failed".to_string()));
        assert_eq!(latch.shown(), Some(idle.clone()));
        assert!(latch.needs(&rec));
        // The next tick retries and succeeds, seeing the real previous state.
        let r = latch.apply(rec.clone(), |want, prev| {
            assert_eq!(prev, Some(&idle));
            assert_eq!(want.toggle_text, "Stop meeting · 00:00");
            Ok(())
        });
        assert_eq!(r, Ok(true));
        assert_eq!(latch.shown(), Some(rec));
        // A rebuild forgets everything, so the next status re-syncs.
        latch.reset();
        assert_eq!(latch.shown(), None);
    }

    #[test]
    fn tray_meeting_label_header_row_is_inserted_not_laid_out() {
        assert!(INSERTED_ITEMS.contains(&ID_MEETING_HEADER));
        assert_eq!(command_for(ID_MEETING_HEADER), None);
        assert_eq!(label(ID_MEETING_TOGGLE), MEETING_START_LABEL);
    }
}

// ─── WF-14: "Update ready" and the quiet gate ───────────────────────────────

/// What Yap is doing right now, as far as a restart is concerned. Filled in by
/// `lib.rs::update_activity` from the live state on every `sync_tray`; pure
/// here so the gate is testable without an app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Activity {
    /// The microphone is open for a dictation (push-to-talk or hands-free).
    pub dictating: bool,
    /// A take is still being transcribed / pasted after the key came up.
    pub take_in_flight: bool,
    /// The polish step is rewriting a take ([`PolishGuard`]).
    pub polishing: bool,
    /// Meeting notes are queued or being written.
    pub notes: bool,
    /// A meeting is recording.
    pub meeting: bool,
}

/// Why a restart has to wait. One reason is shown, the one that will take the
/// longest to clear: a meeting, then polishing (take polish or notes), then a
/// dictation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Meeting,
    Polish,
    Take,
}

impl Busy {
    /// Stable name for the log line.
    pub fn as_str(self) -> &'static str {
        match self {
            Busy::Meeting => "meeting",
            Busy::Polish => "polish",
            Busy::Take => "take",
        }
    }
}

/// The reason a restart must wait, or `None` when Yap is quiet.
pub fn restart_blocker(a: Activity) -> Option<Busy> {
    if a.meeting {
        Some(Busy::Meeting)
    } else if a.polishing || a.notes {
        Some(Busy::Polish)
    } else if a.dictating || a.take_in_flight {
        Some(Busy::Take)
    } else {
        None
    }
}

/// THE restart gate: `true` only when nothing the user is doing would be cut
/// off by a relaunch.
pub fn restart_gate(a: Activity) -> bool {
    restart_blocker(a).is_none()
}

/// The label of [`ID_UPDATE_READY`]. Quiet: "Update ready — Restart to
/// update". Busy and not yet clicked: what a click will do ("Restart after
/// this meeting"). Busy and clicked: the promise ("Will restart after this
/// meeting"), with the way out — a second click cancels.
pub fn update_ready_label(blocker: Option<Busy>, armed: bool) -> &'static str {
    match (blocker, armed) {
        (None, _) => UPDATE_READY_LABEL,
        (Some(Busy::Meeting), false) => "Update ready \u{2014} Restart after this meeting",
        (Some(Busy::Polish), false) => "Update ready \u{2014} Restart after polishing",
        (Some(Busy::Take), false) => "Update ready \u{2014} Restart after this dictation",
        (Some(Busy::Meeting), true) => "Will restart after this meeting \u{00b7} Cancel",
        (Some(Busy::Polish), true) => "Will restart after polishing \u{00b7} Cancel",
        (Some(Busy::Take), true) => "Will restart after this dictation \u{00b7} Cancel",
    }
}

/// What one click on [`ID_UPDATE_READY`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartAction {
    /// No release is waiting (a stale click): nothing.
    Nothing,
    /// Quiet — restart into the new release now.
    RestartNow,
    /// Busy — remember the request; restart the moment it is quiet.
    Arm,
    /// Busy and already armed — the user changed their mind.
    Disarm,
}

/// The click decision. A restart is NEVER the answer while the gate is shut.
pub fn restart_click(ready: bool, armed: bool, a: Activity) -> RestartAction {
    if !ready {
        RestartAction::Nothing
    } else if restart_gate(a) {
        RestartAction::RestartNow
    } else if armed {
        RestartAction::Disarm
    } else {
        RestartAction::Arm
    }
}

/// The deferred half: on every state transition (`sync_tray`), restart now?
/// Only for a release that is waiting, a user who asked, and a quiet Yap.
pub fn restart_when_quiet(ready: bool, armed: bool, a: Activity) -> bool {
    ready && armed && restart_gate(a)
}

/// Is the line in the menu? Only while a release NEWER than the running build
/// is on disk. After the restart the new process runs that version, so the
/// line is gone (and the in-memory [`UpdateReady`] starts empty anyway). An
/// unparseable version is shown — a release we cannot compare is still one the
/// user installed.
pub fn update_item_visible(ready_version: Option<&str>, running_version: &str) -> bool {
    let Some(ready) = ready_version else {
        return false;
    };
    let parse = |v: &str| semver::Version::parse(v.trim().trim_start_matches('v')).ok();
    match (parse(ready), parse(running_version)) {
        (Some(r), Some(cur)) => r > cur,
        _ => ready.trim().trim_start_matches('v') != running_version.trim().trim_start_matches('v'),
    }
}

/// The downloaded-and-waiting release, held in THIS process's memory only.
/// It survives nothing: a restart (the whole point) or a plain quit starts the
/// next process with no line, because the release is already the running build.
#[derive(Debug, Default)]
pub struct UpdateReady {
    inner: Mutex<UpdateReadyInner>,
}

#[derive(Debug, Default)]
struct UpdateReadyInner {
    version: Option<String>,
    armed: bool,
    shown: bool,
}

impl UpdateReady {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(UpdateReadyInner {
                version: None,
                armed: false,
                shown: false,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, UpdateReadyInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `install_update` finished: `version` is on disk and applies on relaunch.
    /// A newer install replaces an older one; an armed request is kept.
    pub fn mark_ready(&self, version: &str) {
        self.lock().version = Some(version.to_string());
    }

    /// The waiting release, if any.
    pub fn version(&self) -> Option<String> {
        self.lock().version.clone()
    }

    pub fn is_armed(&self) -> bool {
        self.lock().armed
    }

    pub fn set_armed(&self, armed: bool) {
        self.lock().armed = armed;
    }

    /// Record whether the line is in the menu now; returns the previous value
    /// so the caller inserts / removes only on an edge.
    pub fn swap_shown(&self, shown: bool) -> bool {
        std::mem::replace(&mut self.lock().shown, shown)
    }

    /// The tray was rebuilt: nothing is in the new menu yet.
    pub fn reset_shown(&self) {
        self.lock().shown = false;
    }
}

/// The one process-wide [`UpdateReady`].
pub static UPDATE_READY: UpdateReady = UpdateReady::new();

/// How many polish passes are running right now (take polish). Read by the
/// quiet gate; counted by [`PolishGuard`].
static POLISH_IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Held for the duration of one polish pass. Drop (including on a panic
/// unwind or an early `?`) releases it, so the count can never stick high.
#[must_use]
pub struct PolishGuard(());

impl PolishGuard {
    pub fn enter() -> Self {
        POLISH_IN_FLIGHT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        PolishGuard(())
    }
}

impl Drop for PolishGuard {
    fn drop(&mut self) {
        POLISH_IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Is a polish pass running right now?
pub fn polish_in_flight() -> bool {
    POLISH_IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst) > 0
}

#[cfg(test)]
mod update_ready_tests {
    use super::*;

    fn quiet() -> Activity {
        Activity::default()
    }

    /// Every busy state, one at a time.
    fn each_busy_state() -> [(&'static str, Activity, Busy); 5] {
        [
            (
                "dictating",
                Activity {
                    dictating: true,
                    ..quiet()
                },
                Busy::Take,
            ),
            (
                "take_in_flight",
                Activity {
                    take_in_flight: true,
                    ..quiet()
                },
                Busy::Take,
            ),
            (
                "polishing",
                Activity {
                    polishing: true,
                    ..quiet()
                },
                Busy::Polish,
            ),
            (
                "notes",
                Activity {
                    notes: true,
                    ..quiet()
                },
                Busy::Polish,
            ),
            (
                "meeting",
                Activity {
                    meeting: true,
                    ..quiet()
                },
                Busy::Meeting,
            ),
        ]
    }

    #[test]
    fn restart_gate_is_false_in_each_busy_state() {
        assert!(restart_gate(quiet()), "a quiet Yap may restart");
        for (name, a, why) in each_busy_state() {
            assert!(!restart_gate(a), "restart gate open while {name}");
            assert_eq!(restart_blocker(a), Some(why), "{name}");
            // A click while busy never restarts: it arms, or disarms.
            assert_eq!(restart_click(true, false, a), RestartAction::Arm, "{name}");
            assert_eq!(
                restart_click(true, true, a),
                RestartAction::Disarm,
                "{name}"
            );
            assert!(!restart_when_quiet(true, true, a), "{name}");
        }
    }

    #[test]
    fn a_meeting_outranks_every_other_busy_state() {
        let all = Activity {
            dictating: true,
            take_in_flight: true,
            polishing: true,
            notes: true,
            meeting: true,
        };
        assert_eq!(restart_blocker(all), Some(Busy::Meeting));
        assert_eq!(
            update_ready_label(restart_blocker(all), true),
            "Will restart after this meeting \u{00b7} Cancel"
        );
        let polish_and_take = Activity {
            polishing: true,
            take_in_flight: true,
            ..quiet()
        };
        assert_eq!(restart_blocker(polish_and_take), Some(Busy::Polish));
    }

    #[test]
    fn quiet_click_restarts_now_and_an_armed_request_fires_on_quiet() {
        assert_eq!(
            restart_click(true, false, quiet()),
            RestartAction::RestartNow
        );
        assert_eq!(restart_click(false, false, quiet()), RestartAction::Nothing);
        assert!(restart_when_quiet(true, true, quiet()));
        assert!(!restart_when_quiet(true, false, quiet()), "nobody asked");
        assert!(!restart_when_quiet(false, true, quiet()), "nothing waiting");
    }

    #[test]
    fn labels_say_what_happens() {
        assert_eq!(
            update_ready_label(None, false),
            "Update ready \u{2014} Restart to update"
        );
        assert_eq!(update_ready_label(None, true), UPDATE_READY_LABEL);
        assert_eq!(
            update_ready_label(Some(Busy::Meeting), true),
            "Will restart after this meeting \u{00b7} Cancel"
        );
        assert_eq!(
            update_ready_label(Some(Busy::Meeting), false),
            "Update ready \u{2014} Restart after this meeting"
        );
        assert_eq!(label(ID_UPDATE_READY), UPDATE_READY_LABEL);
        assert_eq!(
            command_for(ID_UPDATE_READY),
            Some(TrayCommand::RestartToUpdate)
        );
        assert!(INSERTED_ITEMS.contains(&ID_UPDATE_READY));
        assert!(!MENU_LAYOUT.contains(&Slot::Item(ID_UPDATE_READY)));
    }

    #[test]
    fn update_item_disappears_after_a_successful_restart() {
        // Before: 1.4.0 is on disk, 1.3.2 is running — the line shows.
        let before = UpdateReady::new();
        before.mark_ready("1.4.0");
        assert!(update_item_visible(before.version().as_deref(), "1.3.2"));
        // After the restart: a NEW process (fresh in-memory state) running the
        // installed build — no line, whichever way it is asked.
        let after = UpdateReady::new();
        assert_eq!(after.version(), None);
        assert!(!after.is_armed());
        assert!(!update_item_visible(after.version().as_deref(), "1.4.0"));
        assert!(!update_item_visible(Some("1.4.0"), "1.4.0"));
        assert!(!update_item_visible(Some("v1.4.0"), "1.4.0"));
        assert!(
            !update_item_visible(Some("1.3.0"), "1.4.0"),
            "never offer a downgrade"
        );
    }

    #[test]
    fn update_ready_shown_latch_reports_edges() {
        let u = UpdateReady::new();
        assert!(!u.swap_shown(true));
        assert!(u.swap_shown(true));
        u.reset_shown();
        assert!(!u.swap_shown(false));
        u.set_armed(true);
        assert!(u.is_armed());
    }

    #[test]
    fn polish_guard_counts_and_releases() {
        let g = PolishGuard::enter();
        assert!(polish_in_flight());
        assert!(!restart_gate(Activity {
            polishing: polish_in_flight(),
            ..quiet()
        }));
        drop(g);
        // The count went back down by exactly the one this test added.
        let after = POLISH_IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst);
        let g2 = PolishGuard::enter();
        assert!(POLISH_IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst) > after);
        drop(g2);
    }
}
