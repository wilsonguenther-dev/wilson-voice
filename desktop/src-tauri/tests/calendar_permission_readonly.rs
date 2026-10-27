//! yap-notetaker-onboard — Connect calendar is READ-ONLY and OPT-IN.
//!
//! A grep gate over `src/`, because the property it protects is an ABSENCE:
//! no code path in the app may write to the user's calendars. EventKit's write
//! surface is a small, fixed set of selectors (save/remove an event, save/remove
//! a calendar, commit a batch, ask for write-only access); if any of them is
//! spelled anywhere under `src/`, this test fails.
//!
//! Scope: every `.rs` file under `desktop/src-tauri/src`, recursively. This
//! file lives in `tests/` precisely so its own pattern strings are out of scope.
//! A positive control proves the scanner flags the shapes we fear — a
//! `msg_send!` selector and a method-call spelling — rather than passing
//! because it can match nothing.

use std::fs;
use std::path::{Path, PathBuf};

/// Spellings of an EventKit write, in both Objective-C selector form and a
/// Rust-binding method-call form.
const WRITE_SHAPES: &[&str] = &[
    "saveEvent",
    "removeEvent",
    "save(event",
    "remove(event",
    "save_event",
    "remove_event",
    "saveCalendar",
    "removeCalendar",
    "saveReminder",
    "removeReminder",
    "commit:",
    "requestWriteOnlyAccessToEvents",
];

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn hits(text: &str) -> Vec<&'static str> {
    WRITE_SHAPES
        .iter()
        .copied()
        .filter(|p| text.contains(p))
        .collect()
}

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn scanner_flags_the_shapes_we_fear() {
    assert_eq!(
        hits("let _: () = msg_send![store, saveEvent: ev, span: 0, error: e];"),
        ["saveEvent"]
    );
    assert_eq!(hits("store.remove(event, span)"), ["remove(event"]);
    assert_eq!(hits("msg_send![store, commit: err]"), ["commit:"]);
    assert!(hits("msg_send![store, calendarsForEntityType: 0]").is_empty());
}

#[test]
fn no_calendar_write_anywhere_in_src() {
    let mut files = Vec::new();
    rs_files(&src_dir(), &mut files);
    assert!(files.len() > 20, "scanned too few files: {}", files.len());
    let mut found = Vec::new();
    for f in &files {
        let text = fs::read_to_string(f).expect("read source");
        for h in hits(&text) {
            found.push(format!("{}: {h}", f.display()));
        }
    }
    assert!(
        found.is_empty(),
        "calendar write call(s) in src/: {found:#?}"
    );
}

#[test]
fn calendar_access_is_requested_from_exactly_one_command() {
    let mut files = Vec::new();
    rs_files(&src_dir(), &mut files);
    let mut callers = Vec::new();
    for f in &files {
        let text = fs::read_to_string(f).expect("read source");
        let n = text.matches("calendar::request_access").count();
        if n > 0 {
            callers.push((f.file_name().unwrap().to_string_lossy().to_string(), n));
        }
    }
    assert_eq!(
        callers,
        vec![("lib.rs".to_string(), 1)],
        "Calendars access must be requested only by the connect_calendar command"
    );
    let lib = fs::read_to_string(src_dir().join("lib.rs")).unwrap();
    let at = lib.find("calendar::request_access").unwrap();
    let cmd = lib[..at]
        .rfind("async fn connect_calendar(")
        .expect("inside connect_calendar");
    assert!(
        lib[cmd..at].matches("\nfn ").count() == 0
            && lib[cmd..at].matches("async fn ").count() == 1
    );
    // The calendar module asks for FULL (read) access and never write-only.
    let cal = fs::read_to_string(src_dir().join("calendar.rs")).unwrap();
    assert!(cal.contains("requestFullAccessToEventsWithCompletion"));
}

#[test]
fn info_plist_carries_the_calendar_usage_string() {
    let plist =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Info.plist")).unwrap();
    assert!(plist.contains("NSCalendarsFullAccessUsageDescription"));
}
