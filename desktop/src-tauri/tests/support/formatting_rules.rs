//! The canonical id table for the formatting stage — ONE table, read by every
//! test that has an opinion about rule coverage.
//!
//! Y0-C moved this out of `formatting_fixtures.rs`, where it was a private
//! literal. `shipped_defaults.rs` asserts that every rule in `RULE_IDS` has a
//! fixture row at the level the product actually SHIPS, and an assertion like
//! that is worthless if it reads its own copy of the id list: the day a rule is
//! added to the corpus' table and not to the tripwire's, the tripwire goes
//! quietly green on a rule nobody covers. Two readers, one table.
//!
//! Included with `#[path]` (the convention already used by `support/meeting.rs`
//! and `support/callsite.rs`) rather than promoted into the library, because it
//! is a statement about the TEST CORPUS, not about shipping behaviour.

#![allow(dead_code)]

/// The rules the formatting stage owns — `R1`…`R14` of
/// `docs/research/wispr-formatting-deep-dive.md` §1.
pub const RULE_IDS: &[&str] = &[
    "R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10", "R11", "R12", "R13", "R14",
];

/// The measured failures of §1.1. Not rules — observed defects the corpus has
/// to keep a case for — which is why they are a separate list.
pub const MEASURED_FAILURE_IDS: &[&str] = &["F1", "F2", "F3", "F4", "F5", "F6"];

/// Every id the corpus has to cover: rules plus measured failures.
pub fn required_coverage() -> Vec<&'static str> {
    RULE_IDS
        .iter()
        .chain(MEASURED_FAILURE_IDS.iter())
        .copied()
        .collect()
}
