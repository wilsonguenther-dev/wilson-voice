//! DB-D — the Scratchpad as a real second window: notes, versions, search,
//! dictate-into-note, and a non-activating open.
//!
//! What is proven here vs. by hand (headless limits):
//! * Proven by test: the notes/versions/FTS storage through the same
//!   `Database` the app opens; the dictation routing through the same
//!   `paste::take_target` + `paste::insert_traced` the live take runs; the
//!   open sequence `scratchpad::open_steps` the live opener executes, and the
//!   window declaration in tauri.conf.json.
//! * NOT proven here: that AppKit really keeps the prior app active when a
//!   non-activating NSPanel is made key (that is AppKit's documented contract
//!   for `NSWindowStyleMaskNonactivatingPanel`, and a live-app check).

mod support;

use support::{open_db, temp_dir};
use wilson_voice_lib::paste::{
    self, AxInsert, BridgeReceipt, CopyCard, CopyCardReason, InsertBackend, InsertPath, PasteTarget,
};
use wilson_voice_lib::scratchpad::{self, OpenStep};

// ---------------------------------------------------------------- storage --

#[test]
fn notes_survive_restart() {
    let dir = temp_dir("dbd-restart");
    let db = open_db(&dir);
    let a = db
        .finalize_note(None, "Groceries".into(), "eggs\noat milk".into())
        .unwrap();
    let b = db
        .save_note_draft(None, "Draft".into(), "only autosaved".into())
        .unwrap();
    db.set_note_pinned(&b.id, true).unwrap();
    drop(db);

    let db = open_db(&dir);
    let notes = db.list_notes().unwrap();
    assert_eq!(notes.len(), 2, "both notes must survive a restart");
    assert_eq!(notes[0].id, b.id, "pinned first");
    assert!(notes[0].pinned);
    assert_eq!(
        notes[0].content, "only autosaved",
        "a draft autosave survives too"
    );
    let again = db.get_note(&a.id).unwrap().expect("note a");
    assert_eq!(again.title, "Groceries");
    assert_eq!(again.content, "eggs\noat milk");
    assert_eq!(again.preview, "eggs");
    assert_eq!(
        again.created, a.created,
        "a restart never rewrites `created`"
    );
    assert_eq!(db.list_note_versions(&a.id).unwrap().len(), 1);
    drop(db);

    // A third open: idempotent schema, nothing duplicated.
    let db = open_db(&dir);
    assert_eq!(db.list_notes().unwrap().len(), 2);
}

#[test]
fn a_version_row_is_written_on_every_finalize() {
    let dir = temp_dir("dbd-versions");
    let db = open_db(&dir);
    let n = db.finalize_note(None, "Plan".into(), "one".into()).unwrap();
    // Autosave while typing is NOT a finalize.
    db.save_note_draft(Some(n.id.clone()), "Plan".into(), "one tw".into())
        .unwrap();
    db.save_note_draft(Some(n.id.clone()), "Plan".into(), "one two".into())
        .unwrap();
    assert_eq!(
        db.list_note_versions(&n.id).unwrap().len(),
        1,
        "drafts write no version"
    );
    db.finalize_note(Some(n.id.clone()), "Plan".into(), "one two".into())
        .unwrap();
    db.finalize_note(Some(n.id.clone()), "Plan v2".into(), "one two three".into())
        .unwrap();
    let versions = db.list_note_versions(&n.id).unwrap();
    assert_eq!(versions.len(), 3, "one version row per finalize");
    assert_eq!(versions[0].content, "one two three", "newest first");
    assert_eq!(versions[0].title, "Plan v2");
    assert_eq!(versions[2].content, "one");
    // The main window's legacy Save is a finalize as well.
    db.save_scratch(Some(n.id.clone()), "Plan v2".into(), "four".into())
        .unwrap();
    assert_eq!(db.list_note_versions(&n.id).unwrap().len(), 4);
    assert_eq!(db.get_note(&n.id).unwrap().unwrap().content, "four");
}

#[test]
fn fts_finds_a_word_in_a_note_body() {
    let dir = temp_dir("dbd-fts");
    let db = open_db(&dir);
    let n = db
        .finalize_note(
            None,
            "Zoo trip".into(),
            "we saw a quokka near the gate".into(),
        )
        .unwrap();
    db.finalize_note(None, "Other".into(), "nothing to see".into())
        .unwrap();
    let hits = db.search_notes("quokka").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, n.id);
    assert_eq!(db.search_notes("quok").unwrap().len(), 1, "prefix match");
    assert_eq!(
        db.search_notes("zoo").unwrap().len(),
        1,
        "titles are searchable too"
    );
    // The index follows edits and deletes (the triggers, not a rebuild).
    db.save_note_draft(
        Some(n.id.clone()),
        "Zoo trip".into(),
        "we saw a wombat".into(),
    )
    .unwrap();
    assert!(db.search_notes("quokka").unwrap().is_empty());
    assert_eq!(db.search_notes("wombat").unwrap().len(), 1);
    db.delete_note(&n.id).unwrap();
    assert!(db.search_notes("wombat").unwrap().is_empty());
    // A query FTS5 rejects degrades to a substring search, never an error.
    assert!(db.search_notes("\"").is_ok());
    assert_eq!(
        db.search_notes("").unwrap().len(),
        1,
        "empty query = every note"
    );
}

#[test]
fn delete_cascades_to_versions() {
    let dir = temp_dir("dbd-cascade");
    let db = open_db(&dir);
    let n = db.finalize_note(None, "A".into(), "1".into()).unwrap();
    db.finalize_note(Some(n.id.clone()), "A".into(), "2".into())
        .unwrap();
    let keep = db.finalize_note(None, "B".into(), "kept".into()).unwrap();
    assert_eq!(db.list_note_versions(&n.id).unwrap().len(), 2);
    db.delete_note(&n.id).unwrap();
    assert!(db.get_note(&n.id).unwrap().is_none());
    assert!(
        db.list_note_versions(&n.id).unwrap().is_empty(),
        "versions go with the note"
    );
    assert_eq!(
        db.list_note_versions(&keep.id).unwrap().len(),
        1,
        "other notes untouched"
    );
    drop(db);

    // The schema-level cascade too, for any path that deletes a row directly.
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    conn.execute("DELETE FROM notes WHERE id = ?1", [&keep.id])
        .unwrap();
    let left: i64 = conn
        .query_row("SELECT COUNT(*) FROM note_versions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0, "ON DELETE CASCADE removes the orphaned versions");
}

#[test]
fn the_pre_db_d_scratchpad_table_is_folded_in_once() {
    let dir = temp_dir("dbd-fold");
    drop(open_db(&dir));
    {
        // The shape every pre-DB-D install has.
        let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE scratchpad (id TEXT PRIMARY KEY, title TEXT NOT NULL DEFAULT 'Note',
               body TEXT NOT NULL DEFAULT '', updated_at TEXT NOT NULL);
             INSERT INTO scratchpad VALUES ('old-1', 'Legacy', 'kept from v0.9', '2026-09-01T10:00:00+00:00');",
        )
        .unwrap();
    }
    let db = open_db(&dir);
    let n = db
        .get_note("old-1")
        .unwrap()
        .expect("legacy note folded in");
    assert_eq!(n.content, "kept from v0.9");
    assert_eq!(db.list_note_versions("old-1").unwrap().len(), 1);
    assert_eq!(db.search_notes("legacy").unwrap().len(), 1);
    db.delete_note("old-1").unwrap();
    drop(db);
    // Reopening must not resurrect the deleted note from the old table.
    let db = open_db(&dir);
    assert!(db.get_note("old-1").unwrap().is_none());
}

// -------------------------------------------------------------- dictation --

const PRIOR_APP: &str = "com.apple.Notes";
const TAKE: &str = "remember to call the plumber";

#[derive(Default)]
struct Fake {
    frontmost: Option<String>,
    copies: usize,
    posts: usize,
    ax_calls: usize,
    secure_calls: usize,
    cards: Vec<CopyCard>,
    delivered: Vec<(String, String)>,
    note_window_open: bool,
}

impl InsertBackend for Fake {
    fn copy(&mut self, _text: &str) -> Result<(), String> {
        self.copies += 1;
        Ok(())
    }
    fn frontmost_app(&mut self) -> Option<String> {
        self.frontmost.clone()
    }
    fn target_is_secure(&mut self) -> bool {
        self.secure_calls += 1;
        false
    }
    fn ax_insert(&mut self, _text: &str) -> AxInsert {
        self.ax_calls += 1;
        AxInsert::Inserted
    }
    fn paste_bridge(&mut self, _text: &str) -> Result<BridgeReceipt, String> {
        self.posts += 1;
        Ok(BridgeReceipt::Confirmed { after_ms: 3 })
    }
    fn post_delete(&mut self) -> Result<(), String> {
        self.posts += 1;
        Ok(())
    }
    fn show_copy_card(&mut self, card: CopyCard) {
        self.cards.push(card);
    }
    fn deliver_to_note(&mut self, note_id: &str, text: &str) -> Result<(), String> {
        if !self.note_window_open {
            return Err("the Scratchpad was closed".into());
        }
        self.delivered.push((note_id.into(), text.into()));
        Ok(())
    }
}

/// The scratchpad panel is key over Notes (non-activating, so Notes is still
/// FRONTMOST). The take must land in the note and nowhere near Notes.
#[test]
fn dictation_in_the_note_lands_in_the_note() {
    scratchpad::set_focused_note(true, Some("note-42".into()));
    let note = scratchpad::dictation_note();
    assert_eq!(note.as_deref(), Some("note-42"));
    for path in [
        InsertPath::Ax,
        InsertPath::PasteBridge,
        InsertPath::CopyCard,
    ] {
        let mut f = Fake {
            frontmost: Some(PRIOR_APP.into()),
            note_window_open: true,
            ..Fake::default()
        };
        let target = paste::take_target(note.as_deref(), Some(PRIOR_APP));
        assert_eq!(target, PasteTarget::Note("note-42"));
        let (out, trace) = paste::insert_traced(&mut f, path, TAKE, true, target);
        assert_eq!(
            f.delivered,
            vec![("note-42".to_string(), TAKE.to_string())],
            "{path:?}"
        );
        assert_eq!(f.posts, 0, "{path:?}: no ⌘V into the app behind the panel");
        assert_eq!(f.ax_calls, 0, "{path:?}: no AX insert into it either");
        assert_eq!(f.copies, 0, "{path:?}: the user's clipboard is left alone");
        assert_eq!(
            f.secure_calls, 0,
            "{path:?}: the app behind is not even probed"
        );
        assert!(out.pasted);
        assert_eq!(out.message, paste::NOTE_DELIVERED_MESSAGE);
        assert_eq!(trace.app.as_deref(), Some(scratchpad::TRACE_APP));
    }
    // The panel blurs (or hides): the note is no longer a dictation target.
    scratchpad::set_focused_note(false, Some("note-42".into()));
    assert_eq!(
        scratchpad::dictation_note(),
        None,
        "a blurred panel is never a target"
    );
    scratchpad::set_focused_note(true, None);
    assert_eq!(
        scratchpad::dictation_note(),
        None,
        "a panel with no note open is not one"
    );
}

/// The other half of the acceptance: with the panel NOT key, a dictation goes
/// to the previous app exactly as before — the guard is re-pointed, not swapped.
#[test]
fn dictation_elsewhere_still_lands_in_the_previous_app() {
    // (The global focus slot is exercised in ONE test only — the test binary
    // runs tests in parallel; this one starts from "no note focused".)
    let note: Option<String> = None;
    let mut f = Fake {
        frontmost: Some(PRIOR_APP.into()),
        note_window_open: true,
        ..Fake::default()
    };
    let target = paste::take_target(note.as_deref(), Some(PRIOR_APP));
    assert_eq!(target, PasteTarget::Dictated(Some(PRIOR_APP)));
    let (out, _) = paste::insert_traced(&mut f, InsertPath::PasteBridge, TAKE, true, target);
    assert!(f.delivered.is_empty(), "nothing goes to the note");
    assert_eq!(f.posts, 1, "one ⌘V into the app it was dictated into");
    assert!(out.pasted);
}

/// The panel closed while the take decoded: HELD on the pill, never typed into
/// the app that is in front now.
#[test]
fn a_note_take_whose_window_closed_is_held_not_pasted() {
    let mut f = Fake {
        frontmost: Some(PRIOR_APP.into()),
        note_window_open: false,
        ..Fake::default()
    };
    let target = paste::take_target(Some("note-9"), Some(PRIOR_APP));
    let (out, _) = paste::insert_traced(&mut f, InsertPath::PasteBridge, TAKE, true, target);
    assert_eq!(f.posts, 0);
    assert_eq!(f.ax_calls, 0);
    assert!(!out.pasted);
    assert_eq!(out.message, paste::NOTE_GONE_MESSAGE);
    assert_eq!(f.cards.len(), 1);
    assert_eq!(f.cards[0].reason, CopyCardReason::TargetMoved);
    assert_eq!(f.cards[0].text, TAKE);
}

// ------------------------------------------------------------------ focus --

#[test]
fn opening_the_scratchpad_does_not_steal_focus() {
    // 1. The sequence the live opener runs, first open and every later one.
    for already in [false, true] {
        let steps = scratchpad::open_steps(already);
        assert!(
            steps.iter().all(|s| !s.steals_focus()),
            "open_steps({already}) = {steps:?} activates Yap"
        );
        assert!(
            steps.contains(&OpenStep::MakeKey),
            "the panel must take keystrokes"
        );
        assert!(steps.contains(&OpenStep::OrderFrontRegardless));
        assert_eq!(
            steps.contains(&OpenStep::ConvertToNonactivatingPanel),
            !already,
            "converted to a non-activating panel exactly once"
        );
    }
    assert!(OpenStep::ActivateApp.steals_focus() && OpenStep::PresentMainWindow.steals_focus());

    // 2. The window is declared hidden and unfocused, so launch never shows it
    //    or takes focus with it.
    let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let win = conf["app"]["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["label"] == scratchpad::WINDOW_LABEL)
        .expect("a `scratchpad` window in tauri.conf.json");
    assert_eq!(win["url"], "scratchpad.html");
    assert_eq!(win["visible"], false);
    assert_eq!(win["focus"], false);

    // 3. The chord is wired to the panel, not to the main window.
    let lib = include_str!("../src/lib.rs");
    let arm = &lib[lib.find("J::OpenScratchpad =>").expect("the chord arm")..];
    let arm = &arm[..arm.find("\n        }").unwrap()];
    assert!(arm.contains("scratchpad::open(app)"), "{arm}");
    assert!(
        !arm.contains("present_main_window"),
        "the chord must not activate Yap: {arm}"
    );

    // 4. The opener itself never activates the app or focuses a normal window.
    let src = include_str!("../src/scratchpad.rs");
    let body = &src[src.find("fn open_on_main").unwrap()..];
    let body = &body[..body.find("\n}\n").unwrap()];
    for banned in [
        "set_focus",
        "activateIgnoringOtherApps",
        ".activate(",
        "NSApp",
        "show_and_make_key",
    ] {
        assert!(!body.contains(banned), "open_on_main calls `{banned}`");
    }
    assert!(body.contains("nonactivating_panel()"));
}
