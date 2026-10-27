//! Y6-B — the menu bar as a usable surface: state, the last transcript,
//! hide-for-an-hour, quit.
//!
//! Headless: nothing here builds a tray or launches the app. The menu's
//! decisions live in `wilson_voice_lib::tray_menu` (pure), and the few places
//! where the claim is about how `lib.rs` WIRES those decisions are checked
//! against the source with comments stripped (see `code_only` — the same trap
//! `tests/tray_license.rs` documents: a doc comment that quotes the code must
//! not satisfy an assertion about the code).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use wilson_voice_lib::lifecycle::{quit_plan, QuitStep};
use wilson_voice_lib::meeting_control::KIND_PICKER;
use wilson_voice_lib::tray_menu::{
    self, command_for, idle_pill_visible, pill_menu_state, ItemKind, PillHide, Slot, TrayCommand,
    COMMAND_NAMES, HISTORY_NAV, INSERTED_ITEMS, MENU_LAYOUT, PILL_HIDE_FOR, TRAY_ITEMS,
};
use wilson_voice_lib::AppSettings;

const LIB_SRC: &str = include_str!("../src/lib.rs");
const PASTE_TX_SRC: &str = include_str!("../src/paste_tx.rs");
const TRAY_MENU_SRC: &str = include_str!("../src/tray_menu.rs");
const NAV_SRC: &str = include_str!("../../src/appTypes.ts");

fn code_only(src: &str) -> String {
    src.lines()
        .map(str::trim_start)
        .filter(|l| !(l.starts_with("//") || l.starts_with("/*") || l.starts_with('*')))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The body of the first `fn <name>(` in `src`, up to the next top-level `fn`
/// or item doc comment. Good enough for the flat functions in `lib.rs`.
fn fn_body<'a>(src: &'a str, name: &str) -> &'a str {
    let start = src
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("no fn {name} in source"));
    let rest = &src[start..];
    let end = rest[1..]
        .find("\nfn ")
        .or_else(|| rest[1..].find("\npub fn "))
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    &rest[..end]
}

/// The text of a match arm that starts with `pat` up to the next arm that
/// starts with `next_prefix`.
fn arm<'a>(src: &'a str, pat: &str, next_prefix: &str) -> &'a str {
    let start = src
        .find(pat)
        .unwrap_or_else(|| panic!("no arm {pat:?} in source"));
    let rest = &src[start + pat.len()..];
    let end = rest.find(next_prefix).unwrap_or(rest.len());
    &rest[..end]
}

// ─── The registry ────────────────────────────────────────────────────────────

/// Every clickable row resolves to exactly the command its row names, the
/// kind-picker children resolve to "record as <kind>", label rows resolve to
/// nothing, and `lib.rs` dispatches EVERY click through that registry into an
/// exhaustive match — the old `match event.id.as_ref() { … _ => {} }` that let a
/// misspelled id compile and do nothing is gone.
#[test]
fn every_menu_item_maps_to_a_registered_command() {
    let mut ids = HashSet::new();
    for item in TRAY_ITEMS.iter() {
        assert!(ids.insert(item.id), "duplicate tray id {:?}", item.id);
        match item.kind {
            ItemKind::Action(c) => assert_eq!(
                command_for(item.id),
                Some(c),
                "{:?} does not dispatch to the command its row names",
                item.id
            ),
            ItemKind::StateLabel | ItemKind::KindSubmenu => assert_eq!(
                command_for(item.id),
                None,
                "{:?} is not an action and must not dispatch",
                item.id
            ),
        }
        assert!(!item.label.is_empty(), "{:?} has no label", item.id);
    }
    for choice in KIND_PICKER.iter() {
        assert!(
            ids.insert(choice.menu_id),
            "kind id {:?} collides",
            choice.menu_id
        );
        assert_eq!(
            command_for(choice.menu_id),
            Some(TrayCommand::RecordMeetingAs(choice.kind))
        );
    }
    // An id the menu never built dispatches to nothing (and is logged).
    assert_eq!(command_for("definitely_not_a_tray_id"), None);

    // The new items exist as commands at all.
    assert_eq!(
        command_for(tray_menu::ID_COPY_LAST),
        Some(TrayCommand::CopyLastTranscript)
    );
    assert_eq!(
        command_for(tray_menu::ID_HIDE_PILL),
        Some(TrayCommand::HidePillForAnHour)
    );
    assert_eq!(
        command_for(tray_menu::ID_SHOW_PILL),
        Some(TrayCommand::ShowPillNow)
    );
    assert_eq!(
        command_for(tray_menu::ID_HISTORY),
        Some(TrayCommand::OpenHistory)
    );

    // lib.rs wiring: one dispatch path, and it is the registry.
    let lib = code_only(LIB_SRC);
    assert!(
        lib.contains("tray_menu::command_for(event.id.as_ref())"),
        "on_menu_event must resolve clicks through tray_menu::command_for"
    );
    assert!(
        !lib.contains("match event.id.as_ref()"),
        "a literal id match in the tray handler bypasses the registry"
    );
    let dispatch = fn_body(&lib, "run_tray_command");
    for name in COMMAND_NAMES {
        // Exhaustiveness is the compiler's job; this only proves the handler is
        // the function the registry feeds, by finding each variant in it.
        let variant = TRAY_ITEMS
            .iter()
            .filter_map(|i| match i.kind {
                ItemKind::Action(c) if c.name() == name => Some(format!("{c:?}")),
                _ => None,
            })
            .next()
            .unwrap_or_else(|| "RecordMeetingAs".to_string());
        assert!(
            dispatch.contains(&format!("TrayCommand::{variant}")),
            "run_tray_command has no arm for {variant}"
        );
    }
    assert!(
        !dispatch.contains("_ =>"),
        "run_tray_command must stay exhaustive — no wildcard arm"
    );
    // The paste item advertises the chord that is actually registered globally.
    assert!(lib.contains("Some(shortcuts::PASTE_LAST.accelerator)"));
}

/// Every row has a place: fixed rows appear exactly once in the layout `lib.rs`
/// builds from, the two conditional rows are only ever inserted, every layout
/// slot names a real row, every command is reachable from some row, and the
/// layout never starts, ends or doubles up on a separator.
#[test]
fn no_item_is_unreachable() {
    let layout_ids: Vec<&str> = MENU_LAYOUT
        .iter()
        .filter_map(|s| match s {
            Slot::Item(id) => Some(*id),
            Slot::Separator => None,
        })
        .collect();
    for id in &layout_ids {
        assert!(
            tray_menu::item(id).is_some(),
            "layout names unknown id {id:?}"
        );
    }
    for item in TRAY_ITEMS.iter() {
        let in_layout = layout_ids.iter().filter(|i| **i == item.id).count();
        let inserted = INSERTED_ITEMS.contains(&item.id);
        assert!(
            (in_layout == 1) ^ inserted,
            "{:?}: in layout {in_layout}x, inserted={inserted} — exactly one placement",
            item.id
        );
    }
    // Every command reachable from the menu.
    let mut names = HashSet::new();
    for name in COMMAND_NAMES {
        assert!(names.insert(name), "duplicate command name {name}");
        let reachable = TRAY_ITEMS.iter().any(|i| match i.kind {
            ItemKind::Action(c) => c.name() == name,
            ItemKind::KindSubmenu => name == "record_meeting_as" && !KIND_PICKER.is_empty(),
            ItemKind::StateLabel => false,
        });
        assert!(reachable, "command {name} has no menu item");
    }
    // The kind submenu that makes record_meeting_as reachable is in the layout.
    assert!(layout_ids.contains(&tray_menu::ID_MEETING_KIND));
    // Separator hygiene.
    assert!(matches!(MENU_LAYOUT.first(), Some(Slot::Item(_))));
    assert!(matches!(MENU_LAYOUT.last(), Some(Slot::Item(_))));
    for w in MENU_LAYOUT.windows(2) {
        assert!(
            !(w[0] == Slot::Separator && w[1] == Slot::Separator),
            "two separators in a row"
        );
    }
    // Y2-E's upgrade line goes in at index 1 — directly under the header.
    assert_eq!(MENU_LAYOUT[0], Slot::Item(tray_menu::ID_LICENSE_HEADER));
    // Quit is last.
    assert_eq!(MENU_LAYOUT.last(), Some(&Slot::Item(tray_menu::ID_QUIT)));
}

/// Y6-B "what not to do": no Languages / Formatting submenu (Y10).
#[test]
fn no_language_or_formatting_submenu() {
    for item in TRAY_ITEMS.iter() {
        let l = item.label.to_lowercase();
        assert!(
            !l.contains("language") && !l.contains("formatting"),
            "{:?} ({}) is out of scope for Y6-B",
            item.id,
            item.label
        );
    }
}

/// "Transcript History…" lands on a view the frontend actually has: the value
/// must be a member of the `Nav` union in `desktop/src/appTypes.ts`.
#[test]
fn history_lands_on_a_real_view() {
    let start = NAV_SRC.find("export type Nav =").expect("Nav union");
    let union = &NAV_SRC[start..start + NAV_SRC[start..].find(';').expect("end of Nav")];
    assert!(
        union.contains(&format!("\"{HISTORY_NAV}\"")),
        "HISTORY_NAV {HISTORY_NAV:?} is not a Nav: {union}"
    );
}

// ─── Hide the pill for one hour ──────────────────────────────────────────────

#[test]
fn hide_for_an_hour_expires() {
    assert_eq!(PILL_HIDE_FOR, Duration::from_secs(3600));
    let t0 = Instant::now();
    let hide = PillHide::new();
    assert!(!hide.is_hidden(t0));
    let ticket = hide.hide_for_an_hour(t0);
    assert!(hide.is_hidden(t0));
    assert!(hide.is_hidden(t0 + Duration::from_secs(3599)));
    assert!(!idle_pill_visible(
        true,
        hide.is_hidden(t0 + Duration::from_secs(1800))
    ));
    // The timer firing early does nothing.
    assert!(!hide.expire(ticket, t0 + Duration::from_secs(1800)));
    assert!(hide.is_hidden(t0 + Duration::from_secs(1800)));
    // On the hour it is over, by the clock alone and by the timer.
    assert!(!hide.is_hidden(t0 + PILL_HIDE_FOR));
    assert!(hide.expire(ticket, t0 + PILL_HIDE_FOR));
    assert!(idle_pill_visible(true, hide.is_hidden(t0 + PILL_HIDE_FOR)));
    // …exactly once.
    assert!(!hide.expire(ticket, t0 + PILL_HIDE_FOR));
}

/// A re-armed hide or "Show Pill Now" retires the old timer: it can never put
/// the pill back on the old schedule.
#[test]
fn a_superseded_hide_never_reshows_on_the_old_schedule() {
    let t0 = Instant::now();
    let hide = PillHide::new();
    let first = hide.hide_for_an_hour(t0);
    let second = hide.hide_for_an_hour(t0 + Duration::from_secs(600));
    assert!(!hide.expire(first, t0 + PILL_HIDE_FOR));
    assert!(
        hide.is_hidden(t0 + PILL_HIDE_FOR),
        "the re-armed hour still runs"
    );
    assert!(hide.show_now());
    assert!(!hide.is_hidden(t0 + Duration::from_secs(601)));
    assert!(!hide.expire(second, t0 + Duration::from_secs(600) + PILL_HIDE_FOR));
    assert!(!hide.show_now(), "nothing left to end");
}

/// The hide survives NOTHING. A relaunch builds a fresh `PillHide` (what
/// `AppState` holds) and the persistent setting carries no trace of the hide,
/// so the pill comes back.
#[test]
fn a_restart_during_the_hide_window_shows_the_pill() {
    let t0 = Instant::now();
    let before_restart = PillHide::new();
    before_restart.hide_for_an_hour(t0);
    assert!(before_restart.is_hidden(t0 + Duration::from_secs(60)));

    // "Restart": the process state is gone; the next AppState gets a new one.
    let after_restart = PillHide::new();
    assert!(!after_restart.is_hidden(t0 + Duration::from_secs(60)));
    assert!(idle_pill_visible(
        AppSettings::default().show_floating_pill,
        after_restart.is_hidden(t0 + Duration::from_secs(60))
    ));

    // Nothing about the hide reaches settings.json.
    let json = serde_json::to_value(AppSettings::default()).expect("settings serialize");
    for key in json.as_object().expect("settings object").keys() {
        let k = key.to_lowercase();
        assert!(
            !(k.contains("hide") || k.contains("hidden") || k.contains("until")),
            "settings key {key:?} looks like a persisted pill hide"
        );
    }
    // …nor anywhere else: the hide module has no persistence at all.
    let tm = code_only(TRAY_MENU_SRC);
    for forbidden in [
        "Serialize",
        "std::fs",
        "write_settings",
        "SystemTime",
        "data_dir",
    ] {
        assert!(!tm.contains(forbidden), "tray_menu.rs mentions {forbidden}");
    }
    // AppState starts every launch with a fresh, empty hide.
    let lib = code_only(LIB_SRC);
    assert!(lib.contains("pill_hide: tray_menu::PillHide::new(),"));
}

/// Every pill-visibility decision asks `idle_pill_on` (setting AND not hidden),
/// so the end of a take cannot undo a hide.
#[test]
fn pill_decisions_respect_the_hide() {
    let lib = code_only(LIB_SRC);
    let raw_reads = lib.matches("settings.lock().show_floating_pill").count();
    // The only raw reads left: idle_pill_on itself and the two menu-state reads.
    assert_eq!(
        raw_reads, 3,
        "a pill decision reads show_floating_pill directly instead of idle_pill_on"
    );
    assert!(!idle_pill_visible(false, false));
    assert!(!idle_pill_visible(true, true));
    assert!(idle_pill_visible(true, false));
    let s = pill_menu_state(true, false);
    assert!(s.hide_enabled && !s.show_now_enabled);
    let s = pill_menu_state(true, true);
    assert!(!s.hide_enabled && s.show_now_enabled);
    let s = pill_menu_state(false, false);
    assert!(
        !s.hide_enabled && !s.show_now_enabled,
        "no always-on pill, nothing to hide"
    );
}

// ─── Copy last transcript ────────────────────────────────────────────────────

/// YV39 discipline: the copy settles any open paste transaction first, on the
/// main thread, so a pending clipboard restore can never overwrite it.
#[test]
fn copy_last_goes_through_the_paste_sequencer() {
    let lib = code_only(LIB_SRC);
    let body = fn_body(&lib, "copy_last_transcript");
    assert!(body.contains("paste::copy_text_sequenced("));
    assert!(
        !body.contains("paste::copy_text("),
        "a bare clipboard write races a paste"
    );
    assert!(
        body.contains("spawn_blocking"),
        "the main-thread hop must not run on main"
    );
    let tx = code_only(PASTE_TX_SRC);
    let settle = fn_body(&tx, "settle_then_copy");
    let flush = settle.find("flush_pending(app)").expect("settles first");
    let write = settle.find("write_text(").expect("then writes");
    assert!(
        flush < write,
        "the copy must be written AFTER the pending paste settles"
    );
    assert!(fn_body(&tx, "copy_sequenced").contains("run_on_main_thread"));
}

// ─── Quit ────────────────────────────────────────────────────────────────────

/// The tray's Quit is `app.exit(0)` — the same `RunEvent::Exit` teardown as
/// Cmd-Q — and that teardown finalizes a live meeting, drains the ASR engine
/// (the YV93 `ABANDONED_FOR_EXIT` path) and waits for the take in flight,
/// rather than killing the process mid-take.
#[test]
fn quit_runs_the_exit_drain() {
    assert_eq!(command_for(tray_menu::ID_QUIT), Some(TrayCommand::Quit));
    let plan = quit_plan();
    for step in [
        QuitStep::MeetingStop,
        QuitStep::AsrDrain,
        QuitStep::AwaitTake,
        QuitStep::DbCheckpoint,
    ] {
        assert!(plan.contains(&step), "quit plan lost {step:?}");
    }
    let lib = code_only(LIB_SRC);
    let quit_arm = arm(
        fn_body(&lib, "run_tray_command"),
        "TrayCommand::Quit =>",
        "TrayCommand::",
    );
    assert!(
        quit_arm.contains("app.exit(0)"),
        "tray Quit must reach RunEvent::Exit"
    );
    assert!(
        !quit_arm.contains("process::exit"),
        "tray Quit must not skip the drain"
    );
    let exit_arm = arm(&lib, "tauri::RunEvent::Exit =>", "_ =>");
    assert!(
        exit_arm.contains("quit_teardown("),
        "RunEvent::Exit must run the teardown"
    );
    let teardown = fn_body(&lib, "quit_teardown");
    assert!(teardown.contains("lifecycle::quit_plan()"));
    assert!(teardown.contains("exit_step_asr_drain"));
    assert!(teardown.contains("exit_step_await_take"));
    assert!(fn_body(&lib, "exit_step_asr_drain").contains("drain_and_unload("));
}
