//! YV99 · the phase gate itself — "every row has a passing named test or a
//! documented manual repro".
//!
//! That sentence is YV99's acceptance criterion, and left as prose it is a
//! checkbox in a PR description that nobody re-reads six weeks later when a row
//! quietly loses its test. So the matrix lives in code
//! (`meeting_matrix::ROWS`), and this file walks it against the filesystem:
//!
//!   * a row claiming a test must name a file that exists and contains tests;
//!   * **and that test must drive the code the row is about** — the row names a
//!     shipping symbol and the module it lives in, both of which are checked
//!     here. This is the half that was missing, and the finding that produced
//!     it is worth stating plainly: row 5 was published as a green
//!     `cargo test` cell whose test read `record.rs`, the *dictation* journal,
//!     while the row is about the meeting journal a three-hour recording lives
//!     or dies by. Two bounded queues, two drop counters, same shape — the test
//!     passed and proved nothing about the row. A `Test` cell is the strongest
//!     claim in the table, so it is the one that needs a tripwire pointing at
//!     the code, not just at a file name;
//!   * a row claiming a manual repro must name a document that exists and
//!     actually describes that row;
//!   * a row whose *decision* is tested but whose *call site* exists nowhere
//!     must say `PolicyOnly` — and gets the symmetric tripwire, run in the
//!     opposite direction: the named call site must still be ABSENT from
//!     `src/`. Without this half, a tested-but-unwired policy reads exactly like
//!     a shipped feature, which is the failure mode that produced the variant.
//!   * every row that names a test names a `matrix_*` one, so the phase's
//!     acceptance sweep can actually reach it. That rule is what stops "all
//!     eight rows green in one run" from being unsatisfiable by construction.
//!
//! It also asserts `docs/yap22a-error-matrix.md` still carries the table this
//! code renders, so the document and the code cannot drift apart, and that the
//! acceptance sweep printed in that document names every row's test.
//!
//! ## Every tripwire in this file is a CI gate
//!
//! An earlier revision kept some of them behind `#[ignore]` as a "merge
//! checklist", because a tripwire that fires on somebody ELSE's merge would
//! redden that PR's build over assertions it does not own. That reasoning was
//! sound while YV91 and YV93 were open. They have merged; this branch consumed
//! the checklist (rows 4, 6, 17, `3a`, `3b` flipped to `Test`, and `3a`/`3b`'s
//! test files were renamed here to the names the sweep can reach), and nothing
//! this matrix depends on is outstanding. So there is no `--ignored` step left:
//! every assertion below runs on every commit, which is the only state in which
//! "CI checks the matrix" is true without a footnote.

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

/// The line with its comment tail and every string literal removed.
///
/// Both exclusions are load-bearing, and both were learned from a false result.
/// `power.rs` carries the sentence "*are matrix row #16's
/// `NSWorkspaceWillSleepNotification` path*" in its module docs and registers
/// nothing: a comment naming a call site is evidence the row is NOT wired. And
/// this very test file, plus every row's test, contains the row's subject inside
/// a string literal — `subject: "MeetingJournal"` in the published table, or a
/// panic message quoting it — so a "does the test mention its subject?" check
/// that counted literals would be satisfied by a test that only asserts what the
/// table says about itself. Which is a tautology, and the whole family of defect
/// this file exists to catch.
fn code_only(line: &str) -> String {
    let t = line.trim_start();
    if t.starts_with("//") || t.starts_with("*") || t.starts_with("/*") {
        return String::new();
    }
    let mut out = String::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '/' if chars.peek() == Some(&'/') => break,
            _ => out.push(c),
        }
    }
    out
}

/// True if `needle` appears as executable code — not in a comment, not inside a
/// string literal. See [`code_only`] for why both exclusions matter.
fn mentions_as_code(body: &str, needle: &str) -> bool {
    body.lines().any(|line| code_only(line).contains(needle))
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
            Coverage::Test { test, .. } => {
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

/// **The `Test` tripwire.** A row published as covered end-to-end must name the
/// shipping symbol it is about, that symbol must be real code in the module the
/// row names, and the row's test must actually mention it as code.
///
/// This is the finding that produced it, stated so it cannot come back: row 5
/// ("journal write falls behind") shipped as `Coverage::Test`, and its test
/// asserted things about `record.rs` — the journal a five-second dictation
/// uses. The row is about a three-hour meeting, where the journal is not a
/// crash-recovery copy but the recording itself. Both modules have a bounded
/// `sync_channel`, a `try_send` and a `dropped` counter, so every assertion in
/// that test passed while the meeting journal went unexercised by the row that
/// claimed to cover it.
///
/// Nothing in the old gate could catch that: it checked the test file existed
/// and contained `#[test]`. A cell that a reader interprets as "the app does
/// this and we proved it" has to be anchored to the code that does it.
#[test]
fn test_rows_drive_the_shipping_code_they_name() {
    let mut checked = 0;
    for row in ROWS {
        let Some((subject, module)) = row.coverage.shipping_subject() else {
            continue;
        };
        let test = row
            .coverage
            .test_file()
            .expect("a Test row always names a test");

        // 1. The subject is real, shipping code — in the module the row names,
        //    not merely somewhere in the tree.
        let module_path = src_dir().join(module);
        let module_body = fs::read_to_string(&module_path).unwrap_or_else(|e| {
            panic!(
                "matrix row {} says its subject lives in `src/{module}`, which does not read: {e}",
                row.id
            )
        });
        assert!(
            mentions_as_code(&module_body, subject),
            "matrix row {}: `{subject}` is not code in `src/{module}`. A `Test` cell is a claim \
             that the SHIPPING app does this — if the symbol moved, follow it; if it is gone, \
             the row is no longer covered and must not keep saying it is.",
            row.id
        );

        // 2. …and the row's test actually touches it. Not in a doc comment
        //    about what the file would like to check — as code.
        let test_body = fs::read_to_string(tests_dir().join(test)).unwrap_or_else(|e| {
            panic!(
                "matrix row {} claims `{test}`, which does not read: {e}",
                row.id
            )
        });
        assert!(
            mentions_as_code(&test_body, subject),
            "TRIPWIRE — matrix row {}: `{test}` never touches `{subject}`, the shipping code this \
             row is about. This is how row 5 was published as covered by a test that exercised \
             the dictation journal in `record.rs` instead of the meeting journal: same shape, \
             green, and evidence for nothing. Either drive `{subject}` from that test, or change \
             the row to name what it really covers.",
            row.id
        );
        checked += 1;
    }

    assert!(
        checked >= 6,
        "only {checked} rows were checked — a tripwire over an empty set is worse than no \
         tripwire, so if the table stopped publishing `Test` rows that is a deliberate act \
         this number has to record"
    );
}

/// The `PolicyOnly` tripwire, run in the direction the `Test` one cannot: the
/// named call site must still be ABSENT from the shipping tree.
///
/// Rows `5b` (`quality_note`), 16 (`NSWorkspaceWillSleepNotification`) and
/// `17b` (`continuation_title`). None of those symbols is called by anything on
/// `main`, so the only way this goes red is somebody writing the call site,
/// which is precisely the event that must force the row to be promoted.
///
/// A policy with no caller is a policy that is not in effect. Those three rows
/// are published as *not wired*, and the only thing keeping that honest as the
/// tree changes is this test. The fix when it fires is not to delete the
/// assertion: it is to promote the row to `Coverage::Test`, name the shipping
/// subject, and rewrite its test to drive the code that now ships.
#[test]
fn unowned_policy_rows_are_still_unwired_and_go_red_the_day_they_are_not() {
    let mut checked = 0;
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
                 Coverage::Test in src/meeting_matrix.rs — naming `{absent_call_site}`'s module \
                 as the subject — and rewrite `{test}` to drive that call site: assert the \
                 shipping surface performs the behaviour, not that the pure function returns \
                 the right value. This test exists so a policy cannot keep being published as \
                 covered after its call site appeared — and so it cannot keep being published \
                 as covered before it.",
                row.id,
                path.display(),
            );
        }
        checked += 1;
    }
    assert_eq!(
        checked, 3,
        "rows 5b, 16 and 17b are the unwired policies; if that set changed, the change is a \
         claim about what the app now does and belongs in the table's tests too"
    );
}

/// The narrower, blunter form of the no-duplicate-threshold rule, stated where a
/// reader of the cap row will look for it: 22-A has exactly one cap.
///
/// This is the finding that produced it — YV99 shipped `MEETING_HARD_CAP` and
/// `MEETING_WARN_AT` of its own while YV91 shipped `MEETING_HARD_CAP` and
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
             `meeting::MEETING_HARD_CAP` / `meeting::MEETING_CAP_WARN_AT`, enforced by \
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
    assert_eq!(
        declarations.len(),
        1,
        "the 3 h cap must be declared in exactly one module, found: {declarations:?}"
    );
}

/// The rows 22-A owns, by id. If a future edit drops one, the phase's own
/// definition of done changed and that should be a deliberate act.
#[test]
fn the_matrix_still_covers_the_eight_failures_22a_owns() {
    let ids: Vec<&str> = ROWS.iter().map(|r| r.id).collect();
    assert_eq!(
        ids,
        vec!["4", "5", "5b", "6", "15", "16", "17", "17b", "3a", "3b"]
    );
}

/// **The acceptance criterion, made satisfiable.** AC2 is "`cargo test --test
/// matrix_*` (all eight) green in one run", and it was unsatisfiable by
/// construction: rows `3a`/`3b` pointed at `meeting_transcription_resume.rs`
/// and `meeting_chunk_timeout_isolation.rs`, which no `matrix_*` glob will ever
/// match, so the sweep would have reached six of eight rows forever with
/// nothing in the tree forcing the names to converge. Those two files are
/// renamed on this branch; this is the rule that keeps them that way.
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
        "these rows name a test the acceptance sweep cannot reach, so the phase's own criterion \
         could never go green for them: {offenders:?}. Either rename the test, or the row is \
         outside the acceptance command and the command is the thing that has to change."
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

/// The document publishes the acceptance sweep as an exact command, and it may
/// not lie in either direction:
///
///   * every binary it names must exist here — a command in a doc that errors
///     with "no test target named …" teaches people to stop running the
///     commands in docs;
///   * it must name every row's test, which is what makes "all eight rows in one
///     run" a thing that will be true rather than a glob that silently covers
///     six.
#[test]
fn the_published_sweep_is_runnable_and_complete() {
    let doc = fs::read_to_string(repo_root().join("docs/yap22a-error-matrix.md"))
        .expect("read docs/yap22a-error-matrix.md");
    // Anchored on the section, not on the document: other fenced blocks appear
    // higher up, and indexing from the top of the file would check the wrong one.
    let section = doc
        .split_once("## Running the whole sweep")
        .expect("docs/yap22a-error-matrix.md must publish a `Running the whole sweep` section")
        .1;
    let blocks = code_blocks(section);
    assert!(
        !blocks.is_empty(),
        "the matrix document must publish the acceptance sweep as a runnable command"
    );

    let sweep = named_test_binaries(&blocks[0]);
    assert!(
        !sweep.is_empty(),
        "the published sweep names no test binaries"
    );
    for binary in &sweep {
        assert!(
            tests_dir().join(format!("{binary}.rs")).exists(),
            "the published sweep names `{binary}`, which does not exist here"
        );
    }

    for row in ROWS {
        let Some(test) = row.coverage.test_file() else {
            continue;
        };
        let stem = test.trim_end_matches(".rs");
        assert!(
            sweep.iter().any(|b| b == stem),
            "matrix row {} is covered by `{stem}`, which the sweep in \
             docs/yap22a-error-matrix.md does not name — so that command does not actually \
             cover all of the matrix. Rows named: {sweep:?}",
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
