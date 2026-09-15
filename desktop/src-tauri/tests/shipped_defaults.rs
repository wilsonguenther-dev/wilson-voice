//! Y0-C — the defaults the product SHIPS are asserted, so a feature that is off
//! on a fresh install can never be called "tested" again.
//!
//! ## Why this file exists
//!
//! The audit's worst finding was not a broken function. Every function worked.
//! The defect was that the whole formatting corpus
//! (`tests/fixtures/formatting/*.jsonl`) pins its own `level` on every row —
//! 36 rows at `"medium"`, 10 at `"high"` — while `AppSettings::default()` ships
//! `cleanup_level: "light"` and `CleanupLevel::runs_format()` is true only from
//! `Medium` up. So the corpus is green and, on a fresh install, the formatting
//! stage never runs at all. That is the mechanism behind "formatting does not
//! work whatsoever": a test suite exercising a configuration the product never
//! ships.
//!
//! Two kinds of assertion live here, and they are deliberately different:
//!
//! * [`shipped_defaults_table`] — a TABLE of the default values user-visible
//!   behaviour depends on, each constructed from `AppSettings::default()` and
//!   each carrying, in a comment, what the user SEES if it changes. Changing a
//!   default is allowed; changing one silently is not.
//! * [`defaults_reach_every_cleanup_stage`] and
//!   [`every_rule_has_a_fixture_row_at_the_shipped_level`] — the two tripwires.
//!   Both are RED on today's main, both are `#[ignore]`d, and both name the
//!   item that is allowed to remove the ignore.
//!
//! ## What this file deliberately does NOT do
//!
//! It does not change a single default. Y4-A owns that change. It does not
//! read a literal back out of `lib.rs` with a grep either — every assertion
//! constructs `AppSettings::default()`, so a changed default breaks the test
//! rather than the test tracking the change.

use wilson_voice_lib::dictation::{self, CleanupLevel, Style};
use wilson_voice_lib::AppSettings;

/// The rule-id table, shared with `formatting_fixtures.rs`. One list, two
/// readers — see the module's own header for why a second copy would be a
/// tripwire that goes green on its own.
#[path = "support/formatting_rules.rs"]
mod formatting_rules;

// ---------------------------------------------------------------------------
// 1. The table
// ---------------------------------------------------------------------------

/// Every default a user can FEEL, asserted against `AppSettings::default()`.
///
/// Each row's comment is the user-visible consequence of changing it. If you
/// are here because this test went red: that is the test working. Decide
/// whether the new default is what the product means to ship, update the row
/// AND its consequence comment, and say so in the PR.
#[test]
fn shipped_defaults_table() {
    let d = AppSettings::default();

    // --- Text output ------------------------------------------------------
    // "light" means `CleanupLevel::runs_format()` is FALSE on a fresh install:
    // no list detection, no spoken punctuation ("period" stays the word
    // "period"), no email shape, no tone-dialled trailing period. This is the
    // single value behind "formatting does not work whatsoever". Y4-A owns
    // raising it; this row exists so the change can never be silent.
    assert_eq!(d.cleanup_level, "light", "cleanup_level default");
    // Empty = no local polish model installed, so the LLM polish stage is a
    // no-op even at `High`. The user gets rules output only.
    assert_eq!(d.polish_model, "", "polish_model default (empty = OFF)");
    // False would mean the transcript lands in history and NOWHERE ELSE — the
    // user dictates and nothing appears in the app they were typing into.
    assert!(d.auto_paste, "auto_paste default");
    // "inline" = snippets expand in the transcript itself. A different scope
    // changes where a user's saved snippet is allowed to fire.
    assert_eq!(d.snippet_scope, "inline", "snippet_scope default");
    // "off" = no signature is appended. Any other value silently adds text the
    // user did not dictate to the end of a take.
    assert_eq!(d.signature_mode, "off", "signature_mode default");

    // --- Input / hotkey ---------------------------------------------------
    // The push-to-talk binding. Change it and the hotkey the onboarding teaches
    // ("hold fn⌃") is not the hotkey the app listens for.
    assert_eq!(d.ptt_binding, "fn_control", "ptt_binding default");
    // "auto" = the mode is detected from the frontmost app. A fixed value here
    // would apply e.g. Code mode's verbatim passthrough everywhere.
    assert_eq!(d.dictation_mode, "auto", "dictation_mode default");

    // --- What the user sees ----------------------------------------------
    // False = no floating pill at all: dictation runs with no visible state,
    // so a user cannot tell recording from idle from processing.
    assert!(d.show_floating_pill, "show_floating_pill default");
    // The pill's look on first launch. A different value ships a different
    // first impression than every screenshot and the onboarding art.
    assert_eq!(d.pill_style, "classic", "pill_style default");

    // --- Audio ------------------------------------------------------------
    // False = raw mic into the ASR, so fan and keyboard noise reach the model
    // and word error rate rises on exactly the machines that are loudest.
    assert!(d.denoise, "denoise default");
    // False = system audio keeps playing into the room (and, with the tap, into
    // the take) while the user dictates.
    assert!(d.mute_while_dictating, "mute_while_dictating default");

    // --- Lifecycle --------------------------------------------------------
    // False = the model loads on the FIRST take instead of at launch, so the
    // user's first dictation eats the multi-second load.
    assert!(!d.preload_model, "preload_model default");
    // False = the app never tells the user a new build exists.
    assert!(d.check_updates, "check_updates default");
}

// ---------------------------------------------------------------------------
// 2. The tripwire with teeth: do the shipped defaults reach the pipeline?
// ---------------------------------------------------------------------------

/// The cleanup stages a fresh install reaches with NOTHING extra downloaded.
///
/// Stage 4 (local-LLM polish) is deliberately absent: it is gated on
/// `polish_model`, which is empty by default and by design — no model is
/// shipped in the bundle — so demanding it here would be demanding a download.
/// Its default is asserted in the table above instead, with its consequence
/// spelled out.
const NO_DOWNLOAD_STAGES: &[&str] = &[
    "1 dictionary/vocabulary replacement",
    "2 backtrack (fillers + spoken self-correction)",
    "3 rules formatting (lists, spoken marks, email shape, trailing period)",
];

/// Build the cleanup pipeline from `AppSettings::default()` — NOT from a
/// hand-written `CleanupLevel` — and assert every no-download stage is REACHED.
///
/// This is the assertion the formatting corpus cannot make, because every
/// fixture row pins its own level. Here the level comes from the shipped
/// settings struct and from nowhere else.
///
/// Each stage is detected by an effect only that stage can produce:
///   * stage 1 — the `apply_dictionary` closure is called (recorded).
///   * stage 2 — a leading filler is removed. `clean_backtrack` is the only
///     stage that does that at `Light`.
///   * stage 3 — `apply_spoken_marks` turns the spoken word "period" into "."
///     and it runs ONLY inside `runs_format()`, which is the gate at issue.
///
/// RED ON MAIN, ON PURPOSE: `cleanup_level: "light"` fails stage 3.
/// UNBLOCKED BY Y4-A — Y4-A raises the shipped default and removes the
/// `#[ignore]` in the same diff. Do not remove the ignore without that change;
/// do not change the default here.
#[test]
#[ignore = "RED on main by design — UNBLOCKED BY Y4-A (raises the shipped cleanup_level)"]
fn defaults_reach_every_cleanup_stage() {
    use std::cell::Cell;

    let d = AppSettings::default();
    let level = CleanupLevel::from_setting(&d.cleanup_level);
    // "auto" resolves against the frontmost app; Obsidian is the corpus'
    // canonical notes surface, i.e. a mode where `should_format` is true. A
    // mode where formatting is inert (Code/Plain) would make stage 3 unreachable
    // for a reason that is not the default under test.
    let mode = dictation::resolve_mode(&d.dictation_mode, "Obsidian");
    assert!(
        dictation::should_format(mode),
        "probe precondition: resolve_mode({:?}, \"Obsidian\") = {mode:?}, which never formats",
        d.dictation_mode
    );

    let dictionary_called = Cell::new(false);
    let probe = "um the plan is ready period";
    let out = dictation::run_cleanup(
        probe,
        level,
        mode,
        Style::Default,
        |t| {
            dictionary_called.set(true);
            t.to_string()
        },
        |_| None,
    );

    let mut unreached: Vec<&str> = Vec::new();
    if !dictionary_called.get() {
        unreached.push(NO_DOWNLOAD_STAGES[0]);
    }
    if out.to_lowercase().starts_with("um ") {
        unreached.push(NO_DOWNLOAD_STAGES[1]);
    }
    if out.contains("period") {
        unreached.push(NO_DOWNLOAD_STAGES[2]);
    }

    assert!(
        unreached.is_empty(),
        "AppSettings::default() ships cleanup_level {:?} (= {level:?}), which never reaches:\n  {}\n\
         probe {probe:?} came out as {out:?}.\n\
         The formatting corpus is green because every fixture row pins its own level; \
         a fresh install runs none of the above. Y4-A owns raising the default.",
        d.cleanup_level,
        unreached.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// 3. The corpus tripwire: is the SHIPPED level actually covered?
// ---------------------------------------------------------------------------

/// Every rule the formatting stage owns must have at least one fixture row at
/// the level `AppSettings::default().cleanup_level` names.
///
/// PANEL 2026-09-12 rejected the weaker form of this ("at least ONE row
/// anywhere carries the shipped level"): measured on main, four rows already
/// carry `"light"` — one of them literally named
/// `b17-level-light-runs-no-formatting` — and it stays green after Y4-A flips
/// the default to anything the corpus already uses. It can never go red, so it
/// asserts nothing.
///
/// This form has teeth. It fails by NAMING the rule ids with no row at the
/// shipped level, it is red on main, it stays red after a partial Y4-A, and it
/// goes green only when the corpus covers the level the product ships.
///
/// The rule id set comes from `support/formatting_rules.rs`, the same table
/// `formatting_fixtures.rs` reads — never from a literal in this file.
///
/// RED ON MAIN, ON PURPOSE.
/// UNBLOCKED BY Y4-A — remove this `#[ignore]` in the diff that makes the corpus
/// cover the shipped level.
#[test]
#[ignore = "RED on main by design — UNBLOCKED BY Y4-A (corpus must cover the shipped level)"]
fn every_rule_has_a_fixture_row_at_the_shipped_level() {
    let shipped = AppSettings::default().cleanup_level;
    let rows = fixture_rows();
    assert!(!rows.is_empty(), "no fixture rows loaded");

    let mut missing: Vec<String> = Vec::new();
    for rule in formatting_rules::RULE_IDS {
        let covered = rows
            .iter()
            .any(|(level, rules, _)| level == &shipped && rules.iter().any(|r| r == rule));
        if !covered {
            missing.push((*rule).to_string());
        }
    }

    assert!(
        missing.is_empty(),
        "AppSettings::default().cleanup_level is {shipped:?}, but these rules have NO fixture row \
         at that level: {}.\n\
         {} of {} rows are at the shipped level. The corpus therefore proves nothing about the \
         configuration the product actually installs.",
        missing.join(", "),
        rows.iter().filter(|(l, _, _)| l == &shipped).count(),
        rows.len()
    );
}

/// `(level, rules, id)` for every row of every `*.jsonl` in the formatting
/// fixture directory. Reads the DIRECTORY, like `formatting_fixtures.rs` does,
/// so a new fixture file counts the moment it is added.
fn fixture_rows() -> Vec<(String, Vec<String>, String)> {
    let dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/formatting");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| e.expect("readable dir entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    files.sort();

    let mut rows = Vec::new();
    for file in files {
        let body = std::fs::read_to_string(&file).expect("fixture file is readable");
        for (n, line) in body.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("{}:{} is not valid JSON: {e}", file.display(), n + 1));
            let level = v["level"]
                .as_str()
                .unwrap_or_else(|| panic!("{}:{} has no level", file.display(), n + 1))
                .to_string();
            let rules = v["rules"]
                .as_array()
                .unwrap_or_else(|| panic!("{}:{} has no rules", file.display(), n + 1))
                .iter()
                .map(|r| r.as_str().expect("rule id is a string").to_string())
                .collect();
            let id = v["id"].as_str().unwrap_or("<no id>").to_string();
            rows.push((level, rules, id));
        }
    }
    rows
}
