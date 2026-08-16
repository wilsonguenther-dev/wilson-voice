//! YV130 — this item ships zero tuned constants, and here is the guard.
//!
//! The AI-seat rule for this epic is that every threshold is tuned against
//! YV120's harness with the tuning transcript in the PR, and that a number
//! copied from a vendor blog is a blocking finding. YV130's answer is that it
//! has no number to tune:
//!
//!   * deciding **which** segments are the same voice is YV129's `match_cluster`
//!     — `plan_retroactive_relabel` is handed a candidate set and never
//!     re-derives it, so the similarity threshold lives where it was measured;
//!   * `split_partition` makes only **relative** comparisons — "closer to seed A
//!     or to seed B" — which has no floor to move;
//!   * `k = 2` is arity, not tuning: split is the inverse of a pairwise merge.
//!
//! So the guard can be absolute rather than a judgement call: `speaker_corrections.rs`
//! contains no float literal at all, anywhere outside its documentation. That is
//! a property a reader can check in one grep and a reviewer cannot argue with,
//! and it is stronger than scanning `const` lines — a threshold wearing a bare
//! `f32` inside a function body is caught by exactly the same rule.
//!
//! Scanning what SHIPS, not what is tested: fixtures are full of floats by
//! nature, and a guard that had to allow them would allow the thing it exists to
//! forbid.

use std::path::PathBuf;

fn module_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/speaker_corrections.rs");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Strip line comments (`//`, `///`, `//!`) — the reasoning is allowed to talk
/// about numbers; the code is not allowed to hold one. Nothing else is stripped,
/// because nothing else in this file is exempt.
fn code_lines(src: &str) -> Vec<(usize, String)> {
    src.lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim().to_string()))
        .filter(|(_, l)| !l.starts_with("//"))
        .collect()
}

/// A float literal: a digit, a dot, a digit. `1.` and `.5` are not valid Rust
/// float literals in expression position without a suffix, and `f32::EPSILON`
/// is a named constant, not a tuned one — this catches the form a threshold
/// actually takes.
fn holds_float_literal(line: &str) -> bool {
    let bytes: Vec<char> = line.chars().collect();
    for i in 1..bytes.len().saturating_sub(1) {
        if bytes[i] == '.' && bytes[i - 1].is_ascii_digit() && bytes[i + 1].is_ascii_digit() {
            return true;
        }
    }
    false
}

#[test]
fn the_correction_module_holds_no_float_literal() {
    let src = module_source();
    let offenders: Vec<(usize, String)> = code_lines(&src)
        .into_iter()
        .filter(|(_, l)| holds_float_literal(l))
        .collect();
    assert!(
        offenders.is_empty(),
        "speaker_corrections.rs is supposed to hold no tuned constant, and a \
         float literal in it is what one looks like. Move the number to where it \
         was measured (YV120's harness) or express the comparison relatively:\n{}",
        offenders
            .iter()
            .map(|(n, l)| format!("  line {n}: {l}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The guard has to be able to SEE a threshold, or its silence means nothing.
/// This is the mutation the PR's evidence file records, run in-process: the same
/// detector, pointed at lines that do hold numbers.
#[test]
fn the_guard_catches_a_threshold_in_every_shape_it_could_arrive_in() {
    for shape in [
        "const CLUSTER_THRESHOLD: f32 = 0.70;",
        "    let threshold = 0.35;",
        "    if similarity > 0.7 { return true; }",
        "    CosineDistance::new(0.42)",
        "    let bias: f64 = 1.25;",
    ] {
        assert!(
            holds_float_literal(shape),
            "the guard must catch: {shape}"
        );
    }
    // …and does not fire on the things that are not thresholds.
    for innocent in [
        "    let width = members[0].1.len();",
        "    if widest <= CosineDistance::MIN {",
        "use crate::diarize_metrics::{cosine_similarity, CosineDistance};",
        "pub const EMBEDDING_COMPONENT_BYTES: usize = 4;",
    ] {
        assert!(
            !holds_float_literal(innocent),
            "the guard must not fire on: {innocent}"
        );
    }
}

/// The comment stripper must not be the reason the scan is quiet: a threshold on
/// a line that merely ENDS with a comment is still a threshold.
#[test]
fn a_trailing_comment_does_not_hide_a_threshold() {
    let src = "//! doc mentioning 0.70\n/// also 0.35\nlet t = 0.70; // measured somewhere\n";
    let lines = code_lines(src);
    assert_eq!(lines.len(), 1, "only the code line survives: {lines:?}");
    assert!(holds_float_literal(&lines[0].1));
}
