//! YV105 — "is this symbol CALLED anywhere in the shipping tree?", shared by
//! the five tap-scoped matrix rows.
//!
//! Four of those rows are published as [`Coverage::PolicyOnly`] with an open PR
//! named as the owner of their wiring. `matrix_coverage.rs`'s absence tripwire
//! deliberately runs only over the rows with **no** owning PR — those are the
//! ones that can rot for months with nobody looking — so a row whose
//! `wiring_pr` is `Some` would otherwise carry an `absent_call_site` that
//! nothing ever re-checks. That is a claim in a comment, which is the shape of
//! thing this whole matrix exists to stop being.
//!
//! So each of YV105's rows checks its own: the day YV100's tap session or
//! YV102's pre-warm lands, the row's test goes red with an instruction to
//! promote the row, exactly as the unowned rows already do.
//!
//! **This is a second implementation of the rule `matrix_coverage.rs` applies
//! privately, and that is stated rather than hidden.** The two must agree on
//! both exclusions, and both exclusions were learned from a false result:
//!
//!   * **comments do not count** — `power.rs` names
//!     `NSWorkspaceWillSleepNotification` in its module docs and registers
//!     nothing, and `record.rs` names `watch_output` in a comment explaining
//!     that the mic path never calls it. A comment naming a call site is
//!     evidence the wiring is ABSENT.
//!   * **string literals do not count** — the published table carries every
//!     one of these symbols as a `&'static str`, and a scan that counted
//!     literals would report the table as its own call site.

#![allow(dead_code)] // each test binary uses a different subset

use std::fs;
use std::path::{Path, PathBuf};

pub fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The line with its comment tail and every string literal removed.
pub fn code_only(line: &str) -> String {
    let t = line.trim_start();
    if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
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
/// string literal.
pub fn mentions_as_code(body: &str, needle: &str) -> bool {
    body.lines().any(|line| code_only(line).contains(needle))
}

/// Every `src/**/*.rs` whose file name is not in `except`.
pub fn shipping_sources(except: &[&str]) -> Vec<PathBuf> {
    fn walk(dir: &Path, except: &[&str], out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read src dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, except, out);
            } else if path.extension().is_some_and(|e| e == "rs")
                && path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .is_some_and(|f| !except.contains(&f))
            {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&src_dir(), except, &mut out);
    assert!(
        out.len() > 10,
        "src/ scan found almost nothing, so an empty result below would mean \
         nothing: {out:?}"
    );
    out
}

/// The shipping files that mention `symbol` as code, ignoring the files named
/// in `defined_in` (a symbol's own definition is not a call site).
///
/// An empty result is the state a `PolicyOnly` row asserts.
pub fn call_sites(symbol: &str, defined_in: &[&str]) -> Vec<String> {
    shipping_sources(defined_in)
        .into_iter()
        .filter(|p| {
            fs::read_to_string(p)
                .map(|b| mentions_as_code(&b, symbol))
                .unwrap_or(false)
        })
        .map(|p| p.display().to_string())
        .collect()
}

/// The failure message every one of these assertions shares: what happened, and
/// what to do about it — which is never "delete the assertion".
pub fn promote_the_row(row: &str, symbol: &str, found_in: &[String]) -> String {
    format!(
        "TRIPWIRE — matrix row {row}: `{symbol}` is now code in {found_in:?}, so somebody wired \
         it. That row is no longer `PolicyOnly`: promote it to `Coverage::Test` in \
         src/meeting_matrix.rs, name the shipping subject and the module it lives in, and rewrite \
         this test to assert the shipping surface performs the behaviour rather than that a pure \
         function returns the right value."
    )
}
