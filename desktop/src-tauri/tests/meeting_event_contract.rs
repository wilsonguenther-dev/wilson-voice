//! YV96 — the notice's trigger, held to the same standard as the notice itself.
//!
//! `consent_sheet_shown_once.rs` proves the once-ness. It proves nothing about
//! whether the sheet is ever asked to open, and that question has an answer the
//! compiler cannot give: the trigger crosses a process boundary on a bare
//! string. Rust emits `"meeting"`; the webview listens for `"meeting"`. Rename
//! either end — or land the listener while the emitter is still sitting in an
//! unmerged branch — and the app builds clean, every other test in this suite
//! stays green, and the one-time legal notice never renders. A one-time notice
//! that shows zero times is the whole item, gone, silently.
//!
//! So this file is the seam test. It reads the TypeScript back off disk (there
//! is no other way to check a string against a value in another language) and
//! it keeps an explicit, dated tripwire on the emitter, which lives in YV95.

use std::fs;
use std::path::{Path, PathBuf};

use wilson_voice_lib::meetings::MEETING_EVENT;

/// YV95 (PR #112) owns the emitter: `meeting_status_sink` broadcasts
/// `MeetingStatus` under this name once a second. It is a SEPARATE pull request
/// and is not merged yet, so on this branch the listener has nothing to hear —
/// which is correct (there is no meeting recording to notice) but is exactly one
/// merge away from being a live bug.
///
/// **When YV95 lands, this test goes red and the fix is two lines:** point its
/// emit at `meetings::MEETING_EVENT` instead of the `"meeting"` literal, and set
/// this constant to `None`. From then on the emitter is permanently required,
/// and a future refactor cannot delete it — or rename it — quietly.
///
/// This is deliberately not a comment: a comment saying "depends on #112" is a
/// thing a merge queue reads zero times. A constant that fails a test is a thing
/// it cannot merge past.
const EMITTER_LANDS_WITH: Option<&str> = Some("YV95 / PR #112 — meeting_status_sink");

fn src_tauri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn frontend(rel: &str) -> String {
    let path = src_tauri()
        .parent()
        .expect("desktop/")
        .join("src")
        .join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Every `.rs` file under `src/`, comment lines stripped, so a doc comment that
/// merely *talks about* emitting the event never counts as an emitter.
fn backend_code() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(dir).expect("read src/").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let code = fs::read_to_string(&path)
                    .expect("read rust source")
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("//"))
                    .collect::<Vec<_>>()
                    .join("\n");
                out.push((path.display().to_string(), code));
            }
        }
    }
    let mut out = Vec::new();
    walk(&src_tauri().join("src"), &mut out);
    out
}

/// Lines that actually emit something under this event name.
fn emit_sites() -> Vec<String> {
    let mut sites = Vec::new();
    for (file, code) in backend_code() {
        for (n, line) in code.lines().enumerate() {
            let emits = line.contains("emit(")
                || line.contains("emit_to(")
                || line.contains("emit_filter(");
            let names_event = line.contains("MEETING_EVENT") || line.contains("\"meeting\"");
            if emits && names_event {
                sites.push(format!("{file}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    sites
}

/// The seam: one word, three places, all of which must agree.
#[test]
fn the_frontend_listens_to_the_name_the_backend_emits() {
    // 1. Rust's value. Changing it here is fine; changing it here ALONE is not.
    assert_eq!(MEETING_EVENT, "meeting");

    // 2. The TypeScript mirror, read out of the file rather than trusted.
    let consent = frontend("meetings/consent.ts");
    let declared = consent
        .lines()
        .find_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("export const MEETING_EVENT = ")?;
            rest.trim_end_matches(';')
                .trim()
                .strip_prefix('"')?
                .strip_suffix('"')
                .map(str::to_string)
        })
        .expect("consent.ts must declare `export const MEETING_EVENT = \"…\"` on one line");
    assert_eq!(
        declared, MEETING_EVENT,
        "the webview listens for `{declared}` and Rust emits `{MEETING_EVENT}`: \
         the one-time capture notice can never open"
    );

    // 3. The subscription itself uses the constant. A listener that re-types the
    // string is a listener a rename walks straight past.
    let watch = frontend("meetings/consentWatch.ts");
    assert!(
        watch.contains("listen<RecordingSignal>(MEETING_EVENT"),
        "consentWatch.ts must subscribe via MEETING_EVENT, not a literal"
    );
    assert!(
        !watch.contains("\"meeting\""),
        "consentWatch.ts re-types the event name instead of using the constant"
    );
    // And App.tsx delegates rather than opening a second, untested subscription.
    let app = frontend("App.tsx");
    assert!(
        !app.contains("listen<{ recording: boolean }>(\"meeting\""),
        "App.tsx must go through watchMeetingConsent so the wiring stays tested"
    );
    assert!(
        app.contains("watchMeetingConsent"),
        "App.tsx no longer wires the notice to anything"
    );
}

/// Emitters name the constant. This is what makes a rename a compile error on
/// the Rust side instead of a silent behaviour change.
#[test]
fn every_meeting_emit_uses_the_shared_constant() {
    let literals: Vec<_> = emit_sites()
        .into_iter()
        .filter(|s| s.contains("\"meeting\""))
        .collect();
    assert!(
        literals.is_empty(),
        "emit the event as `meetings::MEETING_EVENT`, not a bare string — a \
         literal here is the exact drift `the_frontend_listens_to_the_name_the_\
         backend_emits` cannot catch:\n  {}",
        literals.join("\n  ")
    );
}

/// The tripwire. See [`EMITTER_LANDS_WITH`].
#[test]
fn the_listener_has_an_emitter_or_a_named_blocker() {
    let sites = emit_sites();
    match EMITTER_LANDS_WITH {
        Some(blocker) => assert!(
            sites.is_empty(),
            "`{blocker}` has landed — the emitter now exists:\n  {}\n\
             Set EMITTER_LANDS_WITH to None (and emit MEETING_EVENT, not a literal) \
             so this test starts REQUIRING the emitter from here on.",
            sites.join("\n  ")
        ),
        None => assert!(
            !sites.is_empty(),
            "nothing emits `{MEETING_EVENT}` any more, so the one-time capture \
             notice can never open. If the emitter moved, point this test at it; \
             do not delete the test."
        ),
    }
}
