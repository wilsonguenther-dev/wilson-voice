//! YV99 · error-matrix row 15 — "a second Yap instance is launched".
//!
//! The plan calls this row "already solved by `tauri-plugin-single-instance`",
//! and it is — but "already solved" is a claim about a line in a builder chain,
//! and builder chains get reordered. Two live Yap processes during a meeting is
//! not a cosmetic bug: both take the CGEvent tap, both open the SQLite WAL, and
//! both would open a capture journal in the same recovery directory.
//!
//! The row's real requirement is therefore an ORDERING one, which is why this
//! test is not "the dependency is in Cargo.toml". The guard has to be the first
//! plugin in the chain — Handy's order, and the comment above the call already
//! says so — because anything registered before it takes a lock the duplicate
//! process would also grab, before the duplicate has been told to exit.
//!
//! **This file is a guard, not the row's coverage claim.** Every assertion below
//! reads `lib.rs` and `Cargo.toml` as TEXT: no process is launched, no plugin is
//! initialised, nothing that ships is driven. That is the right instrument for
//! "the guard must stay first in the builder chain" and the wrong one for "a
//! second launch during a meeting is survivable" — which is two live processes
//! and a running capture, and which nothing in-process can perform. The backlog
//! specifies this row's coverage as a manual repro for exactly that reason, so
//! `ROWS` publishes row 15 as `Coverage::Manual("docs/yap22a-phase-demo.md")`
//! (step 5 of the script) and these three greps sit underneath it. An earlier
//! revision published the row as `Coverage::Test` — the table's strongest cell,
//! the one a reader takes as "driven end to end" — on the strength of a source
//! grep.

use std::fs;
use std::path::PathBuf;

use wilson_voice_lib::meeting_matrix::{Coverage, ROWS};

fn src_tauri(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn single_instance_is_the_first_plugin_registered() {
    let lib = src_tauri("src/lib.rs");

    let chain = lib
        .split_once("tauri::Builder::default()")
        .expect("the app must still be built from tauri::Builder::default()")
        .1;

    let first_plugin = chain
        .find(".plugin(")
        .expect("the builder chain must register at least one plugin");
    let head = &chain[first_plugin..first_plugin + 80.min(chain.len() - first_plugin)];

    assert!(
        head.contains("tauri_plugin_single_instance::init"),
        "row 15: the single-instance guard must stay the FIRST plugin — anything \
         before it takes a lock the duplicate process would grab too. Found: {head}"
    );
}

#[test]
fn the_duplicate_focuses_the_running_app_instead_of_recording() {
    let lib = src_tauri("src/lib.rs");
    let handler = lib
        .split_once("tauri_plugin_single_instance::init(")
        .expect("single-instance handler")
        .1;
    let body = &handler[..handler.find("}))").expect("handler body ends")];

    assert!(
        body.contains("focus_main_window"),
        "row 15: the second launch must be handed to the running app: {body}"
    );
    // Whatever else the handler grows, it must never start capture: that is the
    // corruption this row exists to prevent.
    for forbidden in ["start_recording", "toggle_meeting", "CaptureJournal"] {
        assert!(
            !body.contains(forbidden),
            "row 15: the duplicate-instance handler must not touch capture ({forbidden}): {body}"
        );
    }
}

/// And the published row does not overstate what the three greps above are: it
/// points at the two-process repro in the demo script, not at this file.
#[test]
fn the_published_row_points_at_the_manual_repro_not_at_this_file() {
    let row = ROWS.iter().find(|r| r.id == "15").expect("row 15");
    assert_eq!(
        row.coverage,
        Coverage::Manual("docs/yap22a-phase-demo.md"),
        "row 15's failure is two live processes. This file greps source text; it cannot \
         start one. If it is ever promoted back to Coverage::Test, the test behind it has \
         to actually launch a second instance during a recording."
    );
}

#[test]
fn the_plugin_is_a_pinned_dependency() {
    let cargo = src_tauri("Cargo.toml");
    assert!(
        cargo.contains("tauri-plugin-single-instance = \""),
        "row 15: the guard must be a pinned dependency, not an optional feature"
    );
}
