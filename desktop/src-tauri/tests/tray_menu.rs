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

use wilson_voice_lib::float_pill::PillPosition;
use wilson_voice_lib::lifecycle::{quit_plan, QuitStep};
use wilson_voice_lib::meeting_control::KIND_PICKER;
use wilson_voice_lib::tray_menu::{
    self, command_for, idle_pill_visible, pill_context_menu, pill_hide_enabled, pill_menu_state,
    Activity, ItemKind, PillDock, PillHide, PillMenuInputs, PillMenuRow, Slot, TrayCommand,
    COMMAND_NAMES, HISTORY_NAV, INSERTED_ITEMS, MENU_LAYOUT, PILL_HIDE_FOR, PILL_MENU_LAYOUT,
    PILL_ONLY_ITEMS, TRAY_ITEMS,
};
use wilson_voice_lib::AppSettings;

const LIB_SRC: &str = include_str!("../src/lib.rs");
const PASTE_TX_SRC: &str = include_str!("../src/paste_tx.rs");
const TRAY_MENU_SRC: &str = include_str!("../src/tray_menu.rs");
const NAV_SRC: &str = include_str!("../../src/appTypes.ts");
const FLOAT_MAIN_SRC: &str = include_str!("../../src/float-main.tsx");
const DRAG_SRC: &str = include_str!("../../src/pill/drag.ts");

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
            ItemKind::StateLabel
            | ItemKind::KindSubmenu
            | ItemKind::UpcomingSubmenu
            | ItemKind::MicrophoneSubmenu
            | ItemKind::LanguageSubmenu => assert_eq!(
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
            .unwrap_or_else(|| match name {
                // WF-07 — the "Today's Meetings" children.
                "record_upcoming" => "RecordUpcoming".to_string(),
                // WF-09 — the "Microphone" children.
                "pick_microphone" => "PickMicrophone".to_string(),
                // WF-11 — the "Languages" children.
                "pick_language" => "PickLanguage".to_string(),
                "open_language_settings" => "OpenLanguageSettings".to_string(),
                // WF-04 — the pill's right-click menu rows.
                "dock_pill" => "DockPill".to_string(),
                "open_scratchpad" => "OpenScratchpad".to_string(),
                _ => "RecordMeetingAs".to_string(),
            });
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
            ItemKind::UpcomingSubmenu => name == "record_upcoming",
            ItemKind::MicrophoneSubmenu => name == "pick_microphone",
            ItemKind::LanguageSubmenu => {
                name == "pick_language" || name == "open_language_settings"
            }
            ItemKind::StateLabel => false,
        }) || PILL_ONLY_ITEMS.iter().any(|i| match i.kind {
            // WF-04 — the docks and Open Scratchpad live in the pill's menu.
            ItemKind::Action(c) => c.name() == name,
            _ => false,
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
    // yap-1106-menubar-redesign — Meeting is the first group under the header,
    // Start/Stop first; Dictation follows it.
    assert_eq!(MENU_LAYOUT[1], Slot::Separator);
    assert_eq!(MENU_LAYOUT[2], Slot::Item(tray_menu::ID_MEETING_TOGGLE));
    assert_eq!(MENU_LAYOUT[3], Slot::Item(tray_menu::ID_MEETING_KIND));
    assert_eq!(MENU_LAYOUT[5], Slot::Item(tray_menu::ID_TOGGLE));
    // WF-07 — the calendar rows are anchored on the kind picker and dispatch
    // through the registry; lib.rs places them in ONE function.
    let lib = code_only(LIB_SRC);
    let place = fn_body(&lib, "place_upcoming_rows");
    assert!(place.contains("tray_menu::is_upcoming_row("));
    assert!(place.contains("tray_menu::ID_MEETING_KIND"));
    assert!(lib.contains("meeting_detect::set_upcoming_sink("));
    assert_eq!(
        command_for("upcoming_record_2"),
        Some(TrayCommand::RecordUpcoming(2))
    );
    // WF-09 — the Microphone submenu is anchored on Hands-free, placed by ONE
    // function, filled by the device-list watcher, and its children dispatch
    // through the registry.
    let place = fn_body(&lib, "place_microphone_submenu");
    assert!(place.contains("tray_menu::ID_MICROPHONE"));
    assert!(place.contains("tray_menu::ID_HANDS_FREE"));
    assert!(lib.contains("mic_select::start_watch("));
    assert!(INSERTED_ITEMS.contains(&tray_menu::ID_MICROPHONE));
    assert_eq!(
        command_for("mic_pick_1"),
        Some(TrayCommand::PickMicrophone(1))
    );
}

/// Y6-B "what not to do": no Formatting submenu. The one language row is
/// WF-11's Languages submenu, which is never empty.
#[test]
fn no_language_or_formatting_submenu() {
    for item in TRAY_ITEMS.iter() {
        let l = item.label.to_lowercase();
        assert!(
            !l.contains("formatting"),
            "{:?} ({}) is out of scope for Y6-B",
            item.id,
            item.label
        );
        if l.contains("language") {
            assert_eq!(item.id, tray_menu::ID_LANGUAGES, "one language row only");
            assert_eq!(item.kind, ItemKind::LanguageSubmenu);
        }
    }
}

/// WF-11 — the Languages submenu: anchored under Microphone (or Hands-free),
/// placed by ONE function that always ends on "Language Settings…", picks
/// written through the ONE language writer (the same one the bar's chip and
/// Settings call), and every write — from any surface — redraws the tray and
/// tells every window at once.
#[test]
fn languages_submenu_shares_the_one_language_writer() {
    let lib = code_only(LIB_SRC);
    assert!(INSERTED_ITEMS.contains(&tray_menu::ID_LANGUAGES));
    let place = fn_body(&lib, "place_language_submenu");
    assert!(place.contains("tray_menu::ID_LANGUAGES"));
    assert!(place.contains("tray_menu::ID_MICROPHONE"));
    assert!(place.contains("tray_menu::ID_HANDS_FREE"));
    // WF-04 — the submenu itself is built by ONE builder the pill menu shares.
    assert!(place.contains("language_submenu("));
    assert!(fn_body(&lib, "language_submenu").contains("tray_menu::ID_LANGUAGE_SETTINGS"));
    let writer = fn_body(&lib, "write_dictation_languages");
    assert!(writer.contains("languages::LANGUAGES_CHANGED_EVENT"));
    assert!(writer.contains("sync_language_menu("));
    let command = fn_body(&lib, "set_dictation_languages");
    assert!(command.contains("write_dictation_languages("));
    let tray_pick = fn_body(&lib, "pick_language_from_tray");
    assert!(tray_pick.contains("tray_menu::language_row("));
    assert!(tray_pick.contains("write_dictation_languages("));
    assert_eq!(
        command_for("lang_pick_2"),
        Some(TrayCommand::PickLanguage(2))
    );
    assert_eq!(
        command_for(tray_menu::ID_LANGUAGE_SETTINGS),
        Some(TrayCommand::OpenLanguageSettings)
    );
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
    // WF-16's screen-share switch is a real preference ("hide Yap from
    // capture during meetings"), not a persisted pill hide: the one named
    // exemption. Any other hide/hidden/until key still fails.
    const NOT_A_PILL_HIDE: &[&str] = &["hidefromcaptureinmeetings"];
    for key in json.as_object().expect("settings object").keys() {
        let k = key.to_lowercase();
        if NOT_A_PILL_HIDE.contains(&k.as_str()) {
            continue;
        }
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

// ─── WF-14: "Update ready" and the quiet gate ───────────────────────────────

/// WF-14 — an installed release puts "Update ready" in the menu, and both
/// update restarts in `lib.rs` sit behind the quiet gate: the click
/// (`restart_click` → `RestartNow`) and the deferred one
/// (`restart_when_quiet`, run on every `sync_tray`). The gate reads every busy
/// state — a dictation, a take in flight, a polish pass, notes being written
/// and a meeting recording.
#[test]
fn update_restart_goes_through_the_quiet_gate() {
    let lib = code_only(LIB_SRC);
    let install = fn_body(&lib, "install_update");
    assert!(install.contains("tray_menu::UPDATE_READY.mark_ready(&version)"));
    assert!(install.contains("sync_update_ready("));
    assert!(fn_body(&lib, "sync_tray").contains("sync_update_ready(app, state)"));

    let sync = fn_body(&lib, "sync_update_ready");
    assert!(sync.contains("tray_menu::update_item_visible("));
    assert!(sync.contains("tray_menu::restart_when_quiet("));
    assert!(sync.contains("tray_menu::update_ready_label("));
    assert!(sync.contains("tray_menu::ID_UPDATE_READY"));
    assert!(sync.contains("app.request_restart()"));

    let dispatch = fn_body(&lib, "run_tray_command");
    let click = arm(dispatch, "TrayCommand::RestartToUpdate =>", "TrayCommand::");
    assert!(click.contains("tray_menu::restart_click("));
    assert!(click.contains("tray_menu::RestartAction::Arm"));
    assert!(click.contains("tray_menu::RestartAction::Disarm"));
    assert!(click.contains("app.request_restart()"));

    assert_eq!(
        lib.matches("request_restart()").count(),
        2,
        "an update restart outside the click and the deferred path bypasses the quiet gate"
    );

    let activity = fn_body(&lib, "update_activity");
    for input in [
        "state.recording",
        "state.busy",
        "tray_menu::polish_in_flight()",
        "is_active()",
        "is_recording()",
    ] {
        assert!(activity.contains(input), "the restart gate ignores {input}");
    }
    assert!(lib.contains("tray_menu::PolishGuard::enter()"));
}

/// WF-14 — the line is an inserted row (never laid out), resolves to its
/// command, and a fresh process (the one a successful restart starts, already
/// running the installed version) never shows it.
#[test]
fn update_ready_line_is_inserted_and_gone_after_restart() {
    assert!(INSERTED_ITEMS.contains(&tray_menu::ID_UPDATE_READY));
    assert_eq!(
        command_for(tray_menu::ID_UPDATE_READY),
        Some(TrayCommand::RestartToUpdate)
    );
    assert!(!tray_menu::update_item_visible(None, "1.4.0"));
    assert!(!tray_menu::update_item_visible(Some("1.4.0"), "1.4.0"));
    assert!(tray_menu::update_item_visible(Some("1.4.0"), "1.3.9"));
    let busy = tray_menu::Activity {
        meeting: true,
        ..tray_menu::Activity::default()
    };
    assert!(!tray_menu::restart_gate(busy));
    assert_eq!(
        tray_menu::update_ready_label(tray_menu::restart_blocker(busy), true),
        "Will restart after this meeting \u{00b7} Cancel"
    );
}

// ─── WF-04 — the pill's right-click menu ─────────────────────────────────────

fn quiet_pill() -> PillMenuInputs {
    PillMenuInputs {
        show_floating_pill: true,
        hidden: false,
        activity: Activity::default(),
        dock: PillDock::Bottom,
    }
}

/// The id a pill row resolves to (submenus by their registry id).
fn pill_row_id(r: &PillMenuRow) -> Option<&'static str> {
    match r {
        PillMenuRow::Action { id, .. } | PillMenuRow::Dock { id, .. } => Some(*id),
        PillMenuRow::Microphone { .. } => Some(tray_menu::ID_MICROPHONE),
        PillMenuRow::Languages { .. } => Some(tray_menu::ID_LANGUAGES),
        PillMenuRow::Separator => None,
    }
}

fn pill_row_label(r: &PillMenuRow) -> Option<&'static str> {
    match r {
        PillMenuRow::Action { label, .. }
        | PillMenuRow::Dock { label, .. }
        | PillMenuRow::Microphone { label }
        | PillMenuRow::Languages { label } => Some(*label),
        PillMenuRow::Separator => None,
    }
}

fn hide_row_enabled(inputs: PillMenuInputs) -> bool {
    pill_context_menu(inputs)
        .iter()
        .find_map(|r| match r {
            PillMenuRow::Action { id, enabled, .. } if *id == tray_menu::ID_HIDE_PILL => {
                Some(*enabled)
            }
            _ => None,
        })
        .expect("the pill menu has a Hide row")
}

/// WF-04 ACCEPTANCE (single source): the pill's menu is built from the SAME
/// item table as the menu bar — every row is an id resolved through
/// `tray_menu::item`, the rows the two menus share carry the menu bar's label
/// byte for byte, every click dispatches through `command_for`, and lib.rs
/// draws it from the decision with the menu bar's own submenu builders and no
/// label literals of its own.
#[test]
fn pill_menu_is_built_from_the_tray_item_table() {
    let rows = pill_context_menu(quiet_pill());
    let ids: Vec<&str> = rows.iter().filter_map(pill_row_id).collect();
    assert_eq!(
        ids,
        vec![
            tray_menu::ID_HIDE_PILL,
            tray_menu::ID_PILL_DOCK_BOTTOM,
            tray_menu::ID_PILL_DOCK_LEFT,
            tray_menu::ID_PILL_DOCK_RIGHT,
            tray_menu::ID_MICROPHONE,
            tray_menu::ID_LANGUAGES,
            tray_menu::ID_OPEN_SCRATCHPAD,
            tray_menu::ID_SHOW,
        ],
        "Hide, the three docks, Microphone, Languages, Open Scratchpad, Open Yap"
    );
    let mut shared = 0;
    for r in rows.iter() {
        let (Some(id), Some(label)) = (pill_row_id(r), pill_row_label(r)) else {
            continue;
        };
        assert_eq!(
            tray_menu::label(id),
            label,
            "{id:?} label is not the registry's"
        );
        if let Some(t) = TRAY_ITEMS.iter().find(|t| t.id == id) {
            assert_eq!(t.label, label, "{id:?} drifted from the menu bar");
            shared += 1;
        }
    }
    assert_eq!(
        shared, 4,
        "hide, Microphone, Languages and Open Yap are the menu bar's rows"
    );
    // Every clickable row dispatches through the one registry.
    assert_eq!(
        command_for(tray_menu::ID_HIDE_PILL),
        Some(TrayCommand::HidePillForAnHour)
    );
    assert_eq!(
        command_for(tray_menu::ID_OPEN_SCRATCHPAD),
        Some(TrayCommand::OpenScratchpad)
    );
    assert_eq!(command_for(tray_menu::ID_SHOW), Some(TrayCommand::OpenYap));
    for item in PILL_ONLY_ITEMS.iter() {
        assert!(
            TRAY_ITEMS.iter().all(|t| t.id != item.id),
            "{:?} collides with a menu bar id",
            item.id
        );
        assert!(!INSERTED_ITEMS.contains(&item.id));
        assert!(!item.label.is_empty());
        match item.kind {
            ItemKind::Action(c) => assert_eq!(command_for(item.id), Some(c)),
            other => panic!(
                "{:?} is a pill-only {other:?}; only actions belong here",
                item.id
            ),
        }
        assert!(
            PILL_MENU_LAYOUT.contains(&Slot::Item(item.id)),
            "{:?} is never shown",
            item.id
        );
    }
    // Separator hygiene, as for the menu bar.
    assert!(matches!(PILL_MENU_LAYOUT.first(), Some(Slot::Item(_))));
    assert!(matches!(PILL_MENU_LAYOUT.last(), Some(Slot::Item(_))));
    for w in PILL_MENU_LAYOUT.windows(2) {
        assert!(!(w[0] == Slot::Separator && w[1] == Slot::Separator));
    }
    // lib.rs: drawn from the decision, with the menu bar's submenu builders.
    let lib = code_only(LIB_SRC);
    assert!(lib.contains("pill_context_menu,"), "command not registered");
    let cmd = fn_body(&lib, "pill_context_menu");
    assert!(cmd.contains("tray_menu::pill_context_menu("));
    assert!(cmd.contains("popup_menu(&menu)"));
    let build = fn_body(&lib, "build_pill_menu");
    assert!(build.contains("microphone_submenu(app, &plan)"));
    assert!(build.contains("language_submenu(app, &plan)"));
    assert!(build.contains("tray_menu::MIC_SHOWN.snapshot()"));
    assert!(build.contains("tray_menu::LANG_SHOWN.snapshot()"));
    for item in TRAY_ITEMS.iter().chain(PILL_ONLY_ITEMS.iter()) {
        assert!(
            !build.contains(&format!("\"{}\"", item.label)),
            "build_pill_menu spells the label {:?} itself",
            item.label
        );
    }
    assert!(fn_body(&lib, "place_microphone_submenu").contains("microphone_submenu("));
    assert!(fn_body(&lib, "place_language_submenu").contains("language_submenu("));
}

/// WF-04 ACCEPTANCE: Hide is greyed while a take or a meeting is running —
/// and only Hide; the rest of the menu stays usable.
#[test]
fn pill_hide_is_greyed_while_a_take_or_meeting_runs() {
    assert!(hide_row_enabled(quiet_pill()));
    let busy = [
        Activity {
            dictating: true,
            ..Activity::default()
        },
        Activity {
            take_in_flight: true,
            ..Activity::default()
        },
        Activity {
            polishing: true,
            ..Activity::default()
        },
        Activity {
            meeting: true,
            ..Activity::default()
        },
    ];
    for activity in busy {
        let inputs = PillMenuInputs {
            activity,
            ..quiet_pill()
        };
        assert!(
            !hide_row_enabled(inputs),
            "Hide must be greyed for {activity:?}"
        );
        assert!(!pill_hide_enabled(inputs));
        for r in pill_context_menu(inputs) {
            if let PillMenuRow::Action { id, enabled, .. } = r {
                if id != tray_menu::ID_HIDE_PILL {
                    assert!(enabled, "{id:?} must not grey with Hide");
                }
            }
        }
    }
    // Notes being written after a meeting is not a take: Hide stays.
    assert!(hide_row_enabled(PillMenuInputs {
        activity: Activity {
            notes: true,
            ..Activity::default()
        },
        ..quiet_pill()
    }));
    // The menu bar's own rule still holds underneath.
    assert!(!hide_row_enabled(PillMenuInputs {
        show_floating_pill: false,
        ..quiet_pill()
    }));
    assert!(!hide_row_enabled(PillMenuInputs {
        hidden: true,
        ..quiet_pill()
    }));
    assert_eq!(
        pill_menu_state(true, false).hide_enabled,
        pill_hide_enabled(quiet_pill())
    );
    // lib.rs feeds the decision the live activity.
    let lib = code_only(LIB_SRC);
    assert!(fn_body(&lib, "pill_context_menu").contains("update_activity("));
}

/// WF-04: the three dock rows check the dock the pill has, dispatch to that
/// dock, and the click is the same durable change as a drag.
#[test]
fn pill_dock_rows_check_the_current_dock_and_persist_like_a_drag() {
    for dock in PillDock::ALL {
        let rows = pill_context_menu(PillMenuInputs {
            dock,
            ..quiet_pill()
        });
        let checked: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                PillMenuRow::Dock {
                    id, checked: true, ..
                } => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(checked, vec![dock.menu_id()]);
        assert_eq!(
            command_for(dock.menu_id()),
            Some(TrayCommand::DockPill(dock))
        );
        assert_eq!(PillDock::from_settings(dock.as_settings()), dock);
        assert_eq!(
            PillPosition::from_settings(dock.as_settings()).as_settings(),
            dock.as_settings(),
            "the pill menu writes the same pill_position strings as a drag"
        );
    }
    assert_eq!(PillDock::from_settings("somewhere"), PillDock::Bottom);
    let lib = code_only(LIB_SRC);
    let dock_fn = fn_body(&lib, "dock_pill_from_menu");
    assert!(dock_fn.contains("persist_settings(&next)"));
    assert!(dock_fn.contains("app.emit(\"settings\", &next)"));
    assert!(dock_fn.contains("float_pill::set_position("));
    assert!(dock_fn.contains("float_pill::reposition(app)"));
    let dispatch = fn_body(&lib, "run_tray_command");
    assert!(dispatch.contains("dock_pill_from_menu(app, state, dock)"));
    assert!(dispatch.contains("scratchpad::open(app)"));
}

/// WF-04: right-click — and Ctrl-click, which WebKit reports as the same
/// `contextmenu` event — opens the menu, and a Ctrl-click never reaches the
/// pill's press handler (it would start a take).
#[test]
fn right_or_ctrl_click_opens_the_pill_menu_and_never_starts_a_take() {
    assert!(FLOAT_MAIN_SRC.contains("window.addEventListener(\"contextmenu\""));
    assert!(FLOAT_MAIN_SRC.contains("invoke(\"pill_context_menu\")"));
    let down = DRAG_SRC
        .find("onPointerDown: (e) =>")
        .expect("drag.ts press handler");
    let press = DRAG_SRC[down..]
        .split("beginPress(")
        .next()
        .expect("press handler body");
    assert!(press.contains("e.button !== 0"));
    assert!(
        press.contains("if (e.ctrlKey) return;"),
        "a Ctrl-click must not start a press"
    );
}

/// `src` with `//` comment lines removed but every other line kept verbatim.
/// Unlike [`code_only`] this keeps lines that start with `*` — the deref
/// assignments `*state.recording.lock() = false;` the stop-edge test orders —
/// and it does not re-indent, so [`fn_body`] still ends at the next top-level
/// `fn`.
fn code_keep_derefs(src: &str) -> String {
    src.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// fix-b7-1 (WF-14) — the dictation stop edge counts as busy. `lib.rs` sets
/// `recording` false, runs the context reads (seconds, worst case), and only
/// then sets `busy` true; the take's `TakeGuard` must cover that window, so it
/// is entered BEFORE the `recording = false` line, moved into the worker, and
/// released only after the worker sets `busy` false (and before the final
/// `emit_status`, so an armed restart fires on that transition). The order of
/// `recording` / `busy` themselves is unchanged.
#[test]
fn take_guard_spans_the_stop_edge_in_stop_and_transcribe() {
    // Not `code_only`: it drops `*`-leading lines, i.e. the very
    // `*state.recording.lock() = false;` / `*state.busy.lock() = true;` lines
    // this test has to find.
    let lib = code_keep_derefs(LIB_SRC);
    let stop = fn_body(&lib, "stop_and_transcribe");
    let at = |needle: &str| {
        stop.find(needle)
            .unwrap_or_else(|| panic!("stop_and_transcribe must still contain `{needle}`"))
    };
    let enter = at("let stop_edge_guard = tray_menu::TakeGuard::enter();");
    let recording_false = at("*state.recording.lock() = false;");
    let busy_true = at("*state.busy.lock() = true;");
    assert!(
        enter < recording_false,
        "the TakeGuard must be held before recording goes false"
    );
    assert!(
        recording_false < busy_true,
        "recording/busy order changed — other code depends on it"
    );
    assert_eq!(
        stop.matches("tray_menu::TakeGuard::enter()").count(),
        1,
        "one take, one guard"
    );

    let busy_guard = at("let mut busy_guard = WorkerBusyGuard {");
    let moved = at("let take_guard = stop_edge_guard;");
    assert!(
        busy_guard < moved,
        "the guard moves into the worker after busy_guard, so it drops first on unwind"
    );
    let busy_false = at("*state2.busy.lock() = false;");
    let released = at("drop(take_guard);");
    assert!(
        busy_false < released,
        "the TakeGuard is released only once busy is false"
    );
    assert!(
        stop[released..].contains("emit_status(&app2, &state2);"),
        "the end-of-take emit_status comes after the release, so a deferred restart fires there"
    );

    let activity = fn_body(&lib, "update_activity");
    assert!(
        activity.contains("tray_menu::take_in_flight("),
        "update_activity must read the TakeGuard into take_in_flight"
    );
}

/// fix-b7-1 — with a take's guard held and `recording` / `busy` both false
/// (the stop edge), the gate is shut and an armed restart does not fire.
#[test]
fn a_held_take_guard_keeps_a_deferred_restart_waiting() {
    let guard = tray_menu::TakeGuard::enter();
    let edge = tray_menu::Activity {
        dictating: false,
        take_in_flight: tray_menu::take_in_flight(false, false),
        ..tray_menu::Activity::default()
    };
    assert!(!tray_menu::restart_gate(edge));
    assert!(!tray_menu::restart_when_quiet(true, true, edge));
    assert_eq!(
        tray_menu::update_ready_label(tray_menu::restart_blocker(edge), true),
        "Will restart after this dictation \u{00b7} Cancel"
    );
    drop(guard);
}
