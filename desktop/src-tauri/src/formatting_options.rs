//! Y4-H — the formatting screen's copy, **derived from the pipeline's own
//! predicates** instead of retyped beside them.
//!
//! The controls this screen drives are named for their implementation:
//! `cleanup_level` (`none|light|medium|high`), `polish_model`,
//! `polish_deadline_ms`, `polish_styles`, `signature_mode`. A user cannot tell
//! from those names what "high" does, that "high" needs a downloaded model, or
//! that a deadline exists at all.
//!
//! The fix is not more hand-written copy next to the picker — that is exactly
//! what drifted: before this module the Settings screen still told users that
//! High "isn't wired up yet", months after `polish::polish_llm` shipped. Every
//! line below is built from [`CleanupLevel::runs_dictionary`],
//! [`CleanupLevel::runs_backtrack`], [`CleanupLevel::runs_format`] and
//! [`CleanupLevel::runs_llm`] — the same four predicates `run_cleanup` branches
//! on — so a level that stops running a stage loses that clause in the UI on
//! the same commit, and a level that gains one gains the clause.
//!
//! `polish_deadline_ms` stays a bounded number on disk (100..5000, the range
//! `polish::clamp_polish_deadline_ms` enforces) and becomes three NAMED choices
//! on screen. A millisecond box is not a choice a person can make.

use serde::Serialize;

use crate::dictation::{
    CleanupLevel, STAGE_BACKTRACK, STAGE_DICTIONARY, STAGE_POLISH, STAGE_RULES,
};
use crate::polish;

/// One row of the "How much should Yap clean up?" picker.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupLevelOption {
    /// The stored `cleanup_level` value. The KEY never changes — only the label.
    pub id: &'static str,
    /// What a person reads.
    pub label: &'static str,
    /// One line, derived from the `runs_*` predicates below.
    pub description: String,
    /// The stage tags this level runs, in pipeline order — the same closed set
    /// `FormattingTrace` records, so the picker and the per-take trace agree.
    pub stages: Vec<&'static str>,
    /// True only where `runs_llm()` is true: this level asks for the local
    /// model, so without a downloaded polish model it silently degrades to the
    /// level below. The screen says so instead of leaving the user to find out.
    pub needs_polish_model: bool,
}

/// One row of the "How long may the model think?" picker — a named choice, not
/// a millisecond field.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PolishSpeedOption {
    pub id: &'static str,
    pub label: &'static str,
    pub description: String,
    /// The value written to `polish_deadline_ms`; always inside the bounded
    /// range `polish::clamp_polish_deadline_ms` enforces.
    pub deadline_ms: u64,
}

/// Everything the formatting screen needs to render itself without retyping a
/// single fact about the pipeline.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FormattingOptions {
    pub cleanup_levels: Vec<CleanupLevelOption>,
    pub polish_speeds: Vec<PolishSpeedOption>,
}

/// The levels, in the order the picker shows them.
const LEVELS: [(&str, &str, CleanupLevel); 4] = [
    ("none", "Leave it alone", CleanupLevel::None),
    ("light", "Tidy it up", CleanupLevel::Light),
    ("medium", "Tidy and format", CleanupLevel::Medium),
    ("high", "Rewrite it properly", CleanupLevel::High),
];

/// The one-line description for a level, assembled from what it actually runs.
///
/// `None` is its own sentence because "runs nothing" is the promise, not an
/// empty list: a raw passthrough is the reason that level exists.
pub fn describe(level: CleanupLevel) -> String {
    if !level.runs_dictionary() && !level.runs_backtrack() && !level.runs_format() {
        return "Exactly as spoken, word for word — nothing is changed.".into();
    }
    let mut clauses: Vec<&'static str> = Vec::new();
    if level.runs_dictionary() {
        clauses.push("your dictionary spellings");
    }
    if level.runs_backtrack() {
        clauses.push("no “um”s and no half-sentences you talked over");
    }
    if level.runs_format() {
        clauses.push("spoken lists and email shapes become real ones");
    }
    if level.runs_llm() {
        clauses.push("the local model rewrites the wording");
    }
    format!("{}.", join_clauses(&clauses))
}

/// "a", "a and b", "a, b and c" — a sentence, not a comma-joined debug list.
fn join_clauses(clauses: &[&'static str]) -> String {
    match clauses {
        [] => String::new(),
        [only] => (*only).to_string(),
        [head @ .., last] => format!("{} and {last}", head.join(", ")),
    }
}

/// The stage tags a level runs, in pipeline order.
pub fn stages_for(level: CleanupLevel) -> Vec<&'static str> {
    let mut stages = Vec::new();
    if level.runs_dictionary() {
        stages.push(STAGE_DICTIONARY);
    }
    if level.runs_backtrack() {
        stages.push(STAGE_BACKTRACK);
    }
    if level.runs_format() {
        stages.push(STAGE_RULES);
    }
    if level.runs_llm() {
        stages.push(STAGE_POLISH);
    }
    stages
}

pub fn cleanup_level_options() -> Vec<CleanupLevelOption> {
    LEVELS
        .iter()
        .map(|(id, label, level)| CleanupLevelOption {
            id,
            label,
            description: describe(*level),
            stages: stages_for(*level),
            needs_polish_model: level.runs_llm(),
        })
        .collect()
}

pub fn polish_speed_options() -> Vec<PolishSpeedOption> {
    polish::POLISH_SPEEDS
        .iter()
        .map(|(id, label, blurb, ms)| PolishSpeedOption {
            id,
            label,
            description: (*blurb).to_string(),
            deadline_ms: polish::clamp_polish_deadline_ms(*ms),
        })
        .collect()
}

pub fn formatting_options() -> FormattingOptions {
    FormattingOptions {
        cleanup_levels: cleanup_level_options(),
        polish_speeds: polish_speed_options(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point: the copy is a function of the predicates. If `High`
    /// stopped running the model, this line would change on the same commit.
    #[test]
    fn only_the_level_that_runs_the_model_says_so() {
        for opt in cleanup_level_options() {
            let level = CleanupLevel::from_setting(opt.id);
            assert_eq!(
                opt.needs_polish_model,
                level.runs_llm(),
                "{} advertises the model differently from runs_llm()",
                opt.id
            );
            assert_eq!(
                opt.description.contains("local model"),
                level.runs_llm(),
                "{} mentions the local model but does not run it (or vice versa)",
                opt.id
            );
        }
    }

    #[test]
    fn none_promises_a_raw_passthrough_and_the_rest_do_not() {
        let opts = cleanup_level_options();
        assert!(opts[0].stages.is_empty(), "none runs no stages");
        assert!(opts[0].description.contains("word for word"));
        for opt in &opts[1..] {
            assert!(!opt.stages.is_empty(), "{} runs at least one stage", opt.id);
        }
    }

    /// Every id round-trips through the parser that the pipeline uses, so a
    /// button can never write a value `CleanupLevel::from_setting` silently
    /// falls back on.
    #[test]
    fn every_option_id_is_a_value_the_pipeline_understands() {
        for (id, _, level) in LEVELS {
            assert_eq!(CleanupLevel::from_setting(id), level, "id {id}");
        }
    }

    /// A millisecond field is not a choice; three named ones are — and every
    /// one of them must survive the bounds check.
    #[test]
    fn every_speed_is_inside_the_bounded_range() {
        let speeds = polish_speed_options();
        assert_eq!(speeds.len(), 3);
        for s in &speeds {
            assert_eq!(
                s.deadline_ms,
                polish::clamp_polish_deadline_ms(s.deadline_ms),
                "{} is outside the bounded deadline range",
                s.id
            );
        }
        let mut ordered: Vec<u64> = speeds.iter().map(|s| s.deadline_ms).collect();
        let sorted = {
            let mut c = ordered.clone();
            c.sort_unstable();
            c
        };
        ordered.dedup();
        assert_eq!(
            ordered.len(),
            3,
            "the three speeds are three different values"
        );
        assert_eq!(
            speeds.iter().map(|s| s.deadline_ms).collect::<Vec<_>>(),
            sorted,
            "speeds are listed fastest first"
        );
    }

    #[test]
    fn the_default_deadline_is_one_of_the_named_speeds() {
        assert!(polish_speed_options()
            .iter()
            .any(|s| s.deadline_ms == polish::DEFAULT_POLISH_DEADLINE_MS));
    }
}
