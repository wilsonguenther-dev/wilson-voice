//! YV99 · the phase gate itself — "every row has a passing named test or a
//! documented manual repro".
//!
//! That sentence is YV99's acceptance criterion, and left as prose it is a
//! checkbox in a PR description that nobody re-reads six weeks later when a row
//! quietly loses its test. So the matrix lives in code
//! (`meeting_matrix::ROWS`), and this file walks it against the filesystem:
//!
//!   * a row claiming a test must name a file that exists and contains tests;
//!   * a row claiming a manual repro must name a document that exists and
//!     actually describes that row;
//!   * a row whose mechanism is still in an unmerged PR must say so, name the
//!     PR, and — the tripwire — that PR's test file must NOT be present yet.
//!     The day #108 or #110 merges, that tripwire fires and the fix is one line
//!     per row: `Coverage::LandsWith` becomes `Coverage::Test`. That is the same
//!     device `tests/meeting_event_contract.rs` uses for YV95's emitter, and for
//!     the same reason: a comment saying "depends on #108" is a thing a merge
//!     queue reads zero times.
//!   * a row whose *decision* is tested here but whose *call site* exists
//!     nowhere must say `PolicyOnly` — and gets the symmetric tripwire, run in
//!     the opposite direction: the named call site must still be ABSENT from
//!     `src/`. Without this half, a tested-but-unwired policy reads exactly like
//!     a shipped feature, which is the failure mode that produced this variant.
//!   * every row that names a test names a `matrix_*` one, so the phase's
//!     acceptance sweep can actually reach it. That rule is what stops "all
//!     eight rows green in one run" from being unsatisfiable by construction,
//!     and it is why rows `3a`/`3b` publish a rename as a merge condition on
//!     #110 rather than pointing at the names that PR uses today.
//!
//! It also asserts `docs/yap22a-error-matrix.md` still carries the table this
//! code renders, so the document and the code cannot drift apart, and that the
//! acceptance sweep printed in that document names every row's test.
//!
//! ## Which tripwires run in CI, and why the rest are `#[ignore]`d
//!
//! A tripwire fires on a change to the tree — and the tree changes underneath
//! this branch when somebody ELSE's pull request merges. #108 declares
//! `MEETING_HARD_CAP` and adds `matrix_row4_disk_preflight.rs`; #110 adds the
//! two transcription tests. Left as ordinary tests, those tripwires would turn
//! #108's and #110's builds red on assertions those PRs do not own, in a file
//! they do not touch, and the only fix would be an edit to `src/meeting_matrix.rs`
//! — i.e. this item's work, blocking someone else's merge.
//!
//! So the rule is: **a tripwire that a known open PR will trip is a merge
//! checklist, not a CI gate.** Those live in `#[ignore]`d tests whose panic
//! message is the flip instruction, run deliberately with
//! `cargo test --test matrix_coverage -- --ignored` as the first step after
//! #108 or #110 lands. A tripwire nothing open owns — rows `5b` and 16, whose
//! call sites are in no branch at all — stays a real CI gate, because the only
//! way it can go red is somebody wiring the row, which is exactly the event it
//! is there to catch.

use std::fs;
use std::path::{Path, PathBuf};

use wilson_voice_lib::meeting_matrix::{render_markdown, Coverage, REQUIRED_TEST_PREFIX, ROWS};

fn tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `src/**/*.rs` except `meeting_matrix.rs` itself — the policy module is
/// allowed to *name* the call site it is waiting for (it documents exactly what
/// it does not do); every other file naming it is the wiring landing.
fn shipping_sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read src dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs")
                && path.file_name().is_some_and(|f| f != "meeting_matrix.rs")
            {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&src_dir(), &mut out);
    assert!(out.len() > 10, "src/ scan found almost nothing: {out:?}");
    out
}

/// True if `needle` appears on a line that is not wholly a comment.
///
/// The distinction matters: PR #108's `power.rs` carries the sentence
/// "*are matrix row #16's `NSWorkspaceWillSleepNotification` path*" in its
/// module docs and registers nothing. A comment mentioning the call site is
/// evidence the row is NOT wired; only code is evidence that it is.
fn mentions_as_code(body: &str, needle: &str) -> bool {
    body.lines().any(|line| {
        let t = line.trim_start();
        !(t.starts_with("//") || t.starts_with("*") || t.starts_with("/*")) && t.contains(needle)
    })
}

/// Repository root — `desktop/src-tauri/` → `../../`.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

#[test]
fn every_matrix_row_is_covered_by_something_that_exists() {
    for row in ROWS {
        match row.coverage {
            Coverage::Test(file) => {
                let path = tests_dir().join(file);
                let body = fs::read_to_string(&path).unwrap_or_else(|e| {
                    panic!(
                        "matrix row {} claims `{file}`, which does not read: {e}",
                        row.id
                    )
                });
                assert!(
                    body.contains("#[test]"),
                    "matrix row {} claims `{file}`, which contains no tests",
                    row.id
                );
            }
            Coverage::Manual(doc) => {
                let path = repo_root().join(doc);
                let body = fs::read_to_string(&path).unwrap_or_else(|e| {
                    panic!(
                        "matrix row {} claims `{doc}`, which does not read: {e}",
                        row.id
                    )
                });
                assert!(
                    body.contains(&format!("row {}", row.id))
                        || body.contains(&format!("Row {}", row.id)),
                    "matrix row {} claims `{doc}`, which never mentions the row",
                    row.id
                );
            }
            Coverage::LandsWith { pr, .. } => {
                assert!(
                    pr.starts_with('#'),
                    "matrix row {} must name the PR that carries it, got `{pr}`",
                    row.id
                );
                // The "that PR has not merged yet" half is the merge checklist
                // below, deliberately `#[ignore]`d — see the module docs.
            }
            Coverage::PolicyOnly {
                test, wiring_pr, ..
            } => {
                // The pure half must genuinely exist and be tested here — a
                // PolicyOnly row still owes the reader something runnable.
                let path = tests_dir().join(test);
                let body = fs::read_to_string(&path).unwrap_or_else(|e| {
                    panic!(
                        "matrix row {} claims `{test}`, which does not read: {e}",
                        row.id
                    )
                });
                assert!(
                    body.contains("#[test]"),
                    "matrix row {} claims `{test}`, which contains no tests",
                    row.id
                );
                if let Some(pr) = wiring_pr {
                    assert!(
                        pr.starts_with('#'),
                        "matrix row {} names `{pr}` as the PR bringing its wiring; \
                         use None if no PR actually owns it",
                        row.id
                    );
                }
            }
        }
    }
}

/// The `PolicyOnly` tripwire, run in the direction `LandsWith`'s cannot: the
/// named call site must still be ABSENT from the shipping tree.
///
/// Scoped to the rows **no open PR owns** — `5b` (`quality_note`) and 16
/// (`NSWorkspaceWillSleepNotification`). Neither symbol appears in #108 or #110
/// (checked against `gh pr diff` when this was written), so nothing on the
/// queue can turn this red; the only way it fires is somebody writing the call
/// site, which is precisely the event that must force the row to be promoted.
/// Row 17's call site IS on the queue, so it is a checklist item below instead.
///
/// A policy with no caller is a policy that is not in effect. Rows `5b` and 16
/// are published as *not wired*, and the only thing keeping that honest as the
/// tree changes is this test. The fix when it fires is not to delete the
/// assertion: it is to promote the row to `Coverage::Test` and rewrite its test
/// to drive the code that now ships.
#[test]
fn unowned_policy_rows_are_still_unwired_and_go_red_the_day_they_are_not() {
    for row in ROWS {
        let Coverage::PolicyOnly {
            absent_call_site,
            wiring_pr: None,
            test,
        } = row.coverage
        else {
            continue;
        };

        for path in shipping_sources() {
            let body = fs::read_to_string(&path).unwrap_or_default();
            assert!(
                !mentions_as_code(&body, absent_call_site),
                "TRIPWIRE — matrix row {}: `{absent_call_site}` is now code in {}, so somebody \
                 wired it. This row is no longer `PolicyOnly`: promote it to \
                 Coverage::Test(\"{test}\") in src/meeting_matrix.rs and rewrite `{test}` to \
                 drive that call site — assert the shipping surface performs the behaviour, \
                 not that the pure function returns the right value. This test exists \
                 so a policy cannot keep being published as covered after its call site \
                 appeared — and so it cannot keep being published as covered before it.",
                row.id,
                path.display(),
            );
        }
    }
}

// ── The merge checklist: tripwires an open PR will trip ─────────────────────
//
// Everything below is `#[ignore]`d on purpose, and the reason is in the module
// docs: these fire on a change that belongs to ANOTHER pull request, so as
// ordinary tests they would redden that PR's build over this file. Run them the
// moment #108 or #110 lands:
//
//     cargo test --test matrix_coverage -- --ignored
//
// Each panic message is the flip instruction. Nothing else in the suite tells
// you a row went stale, so this command is a step in the merge, not a nicety.

/// `LandsWith` rows: the day the owning PR's test file appears, the row is a
/// `Test` row and must say so.
///
/// Watches BOTH names for a row still owing a rename, so merging #110 under its
/// current file names trips this exactly as loudly as merging it renamed.
#[test]
#[ignore = "merge checklist — run with --ignored after #108 or #110 lands"]
fn merge_checklist_landswith_rows_flip_the_day_their_pr_merges() {
    for row in ROWS {
        let Coverage::LandsWith { pr, test, .. } = row.coverage else {
            continue;
        };
        for name in row.coverage.landing_file_names() {
            assert!(
                !tests_dir().join(name).exists(),
                "FLIP ME — matrix row {}: `{name}` is now present, so {pr} has merged. \
                 Change this row's Coverage::LandsWith to Coverage::Test(\"{test}\") in \
                 src/meeting_matrix.rs, rename the file to `{test}` if it is not already \
                 (the acceptance sweep globs `matrix_*` and cannot see any other name), \
                 regenerate the table in docs/yap22a-error-matrix.md, and re-run the sweep.",
                row.id
            );
        }
    }
}

/// `PolicyOnly` rows whose call site is carried by an open PR — row 17's
/// `MEETING_HARD_CAP`, which arrives with #108.
#[test]
#[ignore = "merge checklist — run with --ignored after #108 lands"]
fn merge_checklist_policy_rows_owned_by_an_open_pr_flip_when_it_merges() {
    for row in ROWS {
        let Coverage::PolicyOnly {
            absent_call_site,
            wiring_pr: Some(pr),
            test,
        } = row.coverage
        else {
            continue;
        };

        for path in shipping_sources() {
            let body = fs::read_to_string(&path).unwrap_or_default();
            assert!(
                !mentions_as_code(&body, absent_call_site),
                "FLIP ME — matrix row {}: `{absent_call_site}` is now code in {}, so the wiring \
                 from {pr} has landed. Promote the row to Coverage::Test(\"{test}\") in \
                 src/meeting_matrix.rs and rewrite `{test}` to drive the shipping call site — \
                 for row 17 that means importing `meeting::MEETING_HARD_CAP` / \
                 `meeting::MEETING_CAP_WARN_AT` and driving `meeting::watchdog_tick`, NOT \
                 re-declaring the thresholds here.",
                row.id,
                path.display(),
            );
        }
    }
}

/// The narrower, blunter form of the same rule for row 17's constants, stated
/// where a reader of the cap row will look for it: 22-A has exactly one cap.
///
/// This is the finding that produced it — YV99 shipped `MEETING_HARD_CAP` and
/// `MEETING_WARN_AT` of its own while #108 shipped `MEETING_HARD_CAP` and
/// `MEETING_CAP_WARN_AT`, with no compile-time link between the pair. Change
/// the one that runs, and the matrix row published as the required behaviour
/// stays green and wrong.
#[test]
fn the_three_hour_cap_is_declared_in_exactly_one_place_and_it_is_not_this_module() {
    let matrix =
        fs::read_to_string(src_dir().join("meeting_matrix.rs")).expect("meeting_matrix.rs");
    for forbidden in [
        "pub const MEETING_HARD_CAP",
        "pub const MEETING_WARN_AT",
        "pub const MEETING_CAP_WARN_AT",
    ] {
        assert!(
            !matrix.contains(forbidden),
            "src/meeting_matrix.rs declares `{forbidden}`. The cap that actually runs is \
             `meeting::MEETING_HARD_CAP` / `meeting::MEETING_CAP_WARN_AT` (PR #108), enforced by \
             `meeting::watchdog_tick`. A second copy here is a number that can drift away from \
             the shipping one with nothing to catch it."
        );
    }

    let declarations: Vec<String> = shipping_sources()
        .into_iter()
        .filter(|p| {
            fs::read_to_string(p)
                .map(|b| b.contains("const MEETING_HARD_CAP"))
                .unwrap_or(false)
        })
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        declarations.len() <= 1,
        "the 3 h cap is declared in more than one module: {declarations:?}"
    );
}

/// The rows 22-A owns, by id. If a future edit drops one, the phase's own
/// definition of done changed and that should be a deliberate act.
#[test]
fn the_matrix_still_covers_the_eight_failures_22a_owns() {
    let ids: Vec<&str> = ROWS.iter().map(|r| r.id).collect();
    assert_eq!(ids, vec!["4", "5", "5b", "6", "15", "16", "17", "3a", "3b"]);
}

/// **The acceptance criterion, made satisfiable.** AC2 is "`cargo test --test
/// matrix_*` (all eight) green in one run", and it was unsatisfiable by
/// construction: rows `3a`/`3b` pointed at `meeting_transcription_resume.rs`
/// and `meeting_chunk_timeout_isolation.rs`, which no `matrix_*` glob will ever
/// match, so the sweep would have reached six of eight rows forever with
/// nothing in the tree forcing the names to converge.
///
/// This is that forcing function. Every row that names a test names a
/// `matrix_`-prefixed one; for the two rows whose file is called something else
/// today, the required name is what the row publishes and the rename is a merge
/// condition on #110 (rendered into the table, and watched from both sides by
/// the merge checklist above).
#[test]
fn every_row_names_a_test_the_acceptance_sweep_can_reach() {
    let offenders: Vec<String> = ROWS
        .iter()
        .filter_map(|row| {
            let test = row.coverage.test_file()?;
            (!test.starts_with(REQUIRED_TEST_PREFIX)).then(|| format!("row {} → {test}", row.id))
        })
        .collect();

    assert!(
        offenders.is_empty(),
        "these rows name a test the acceptance sweep `cargo test --test '{REQUIRED_TEST_PREFIX}*'` \
         can never reach, so the phase's own criterion could never go green for them: {offenders:?}. \
         Either rename the test, or the row is outside the acceptance command and the command is \
         the thing that has to change."
    );
}

/// Fenced code blocks of a Markdown document, in order.
fn code_blocks(doc: &str) -> Vec<String> {
    doc.split("```")
        .skip(1)
        .step_by(2)
        .map(|b| b.to_string())
        .collect()
}

/// The `--test <name>` targets named by a shell command.
fn named_test_binaries(block: &str) -> Vec<String> {
    block
        .split("--test ")
        .skip(1)
        .filter_map(|tail| tail.split_whitespace().next())
        .map(|t| {
            t.trim_matches(|c| c == '\'' || c == '"' || c == '\\')
                .to_string()
        })
        .collect()
}

/// The document publishes two sweeps: the one that is runnable on this branch
/// and the full one AC2 asks for. Neither may lie.
///
///   * the runnable one may only name binaries that exist here — a command in a
///     doc that errors with "no test target named …" teaches people to stop
///     running the commands in docs;
///   * the full one must name every row's test, which is what makes "all eight
///     rows in one run" a thing that will be true rather than a glob that
///     silently covers six.
#[test]
fn the_published_sweeps_are_runnable_and_complete() {
    let doc = fs::read_to_string(repo_root().join("docs/yap22a-error-matrix.md"))
        .expect("read docs/yap22a-error-matrix.md");
    // Anchored on the section, not on the document: the merge-checklist command
    // higher up is also a fenced block, and indexing from the top of the file
    // would silently check the wrong two.
    let section = doc
        .split_once("## Running the whole sweep")
        .expect("docs/yap22a-error-matrix.md must publish a `Running the whole sweep` section")
        .1;
    let blocks = code_blocks(section);
    assert!(
        blocks.len() >= 2,
        "the matrix document must publish both the sweep that runs today and the full AC2 sweep"
    );

    let today = named_test_binaries(&blocks[0]);
    assert!(!today.is_empty(), "the first sweep names no test binaries");
    for binary in &today {
        assert!(
            tests_dir().join(format!("{binary}.rs")).exists(),
            "the sweep published as runnable today names `{binary}`, which does not exist here"
        );
    }

    let full = named_test_binaries(&blocks[1]);
    for row in ROWS {
        let Some(test) = row.coverage.test_file() else {
            continue;
        };
        let stem = test.trim_end_matches(".rs");
        assert!(
            full.iter().any(|b| b == stem),
            "matrix row {} is covered by `{stem}`, which the full AC2 sweep in \
             docs/yap22a-error-matrix.md does not name — so that command does not actually \
             cover all of the matrix. Rows named: {full:?}",
            row.id
        );
    }
}

#[test]
fn the_committed_matrix_document_matches_the_code() {
    let doc_path = repo_root().join("docs/yap22a-error-matrix.md");
    let doc = fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", doc_path.display()));
    let table = render_markdown();

    assert!(
        doc.contains(&table),
        "docs/yap22a-error-matrix.md has drifted from meeting_matrix::ROWS. \
         Replace its table with:\n\n{table}"
    );
}

/// The phase-closing demo script has to exist and has to be runnable by a human
/// who was not in this session: offline, with the exact steps and the exact pass
/// condition.
#[test]
fn the_phase_demo_script_is_committed_and_says_wifi_off() {
    let path = repo_root().join("docs/yap22a-phase-demo.md");
    let body = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    for needle in ["Wi-Fi off", "10-minute", "Row 15"] {
        assert!(
            body.contains(needle),
            "the phase demo script must still cover `{needle}`"
        );
    }
}
