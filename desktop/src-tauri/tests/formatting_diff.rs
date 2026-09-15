//! Y4-G acceptance — **a take records WHICH stages ran, and why the LLM stage
//! produced nothing.**
//!
//! This is the silent-skip signal. Both polish failure modes are invisible by
//! construction — a missed deadline returns `Err(Deadline)` and the parent keeps
//! the rules text, a rejected rewrite is discarded by `validate_polish` — and
//! for any take over ~150 words those are the NORMAL outcomes. So a user who
//! installs a 1.12 GB model and sets Auto-Cleanup to High gets rules-only output
//! on every real dictation with no indication the model was ever consulted.
//! That is precisely how the defect this item exists for stayed invisible for a
//! release cycle.
//!
//! These tests drive `dictation::run_cleanup_traced` with polish closures that
//! stand in for each failure mode, because the real failure modes need a 1.12 GB
//! model and a real deadline miss to reproduce and the thing under test is the
//! RECORDING, not the model.

use wilson_voice_lib::dictation::{
    run_cleanup_traced, CleanupLevel, DictationMode, FormattingTrace, Style, STAGE_BACKTRACK,
    STAGE_DICTIONARY, STAGE_POLISH, STAGE_RULES,
};

const TAKE: &str = "um so the first thing is we ship the recovery item uh today";

fn trace_at(
    level: CleanupLevel,
    polish: fn(&str) -> Result<String, &'static str>,
) -> FormattingTrace {
    run_cleanup_traced(
        TAKE,
        level,
        DictationMode::Notes,
        Style::default(),
        &[],
        |t| t.to_string(),
        polish,
    )
    .1
}

#[test]
fn a_skipped_llm_stage_records_its_reason() {
    // Every skip reason the shipped stage can produce, in the only place a user
    // can ever see it.
    for reason in [
        "no_model",
        "no_sidecar",
        "deadline",
        "client_panic",
        "v2_truncated",
        "v2_runaway",
    ] {
        let trace = run_cleanup_traced(
            TAKE,
            CleanupLevel::High,
            DictationMode::Notes,
            Style::default(),
            &[],
            |t| t.to_string(),
            |_| Err(reason),
        )
        .1;
        assert_eq!(
            trace.llm_skip_reason,
            Some(reason),
            "a take whose LLM stage produced nothing must say WHY"
        );
        assert!(
            !trace.stages.contains(&STAGE_POLISH),
            "a skipped stage must not be listed as having run ({reason})"
        );
        // …and the rules stages that DID run are still attributable, so the
        // user can tell a rules change from a model change.
        assert_eq!(
            trace.stages,
            vec![STAGE_DICTIONARY, STAGE_BACKTRACK, STAGE_RULES],
            "the rules stages ran and must be named ({reason})"
        );
    }
}

#[test]
fn an_accepted_rewrite_lists_polish_and_no_reason() {
    let trace = trace_at(CleanupLevel::High, |t| Ok(format!("{t} (polished)")));
    assert_eq!(trace.llm_skip_reason, None);
    assert!(trace.stages.contains(&STAGE_POLISH));
    assert_eq!(
        trace.stages_that_ran(),
        "dictionary,backtrack,rules,polish",
        "the column value is the stage list in pipeline order"
    );
}

#[test]
fn an_empty_rewrite_is_a_skip_not_an_acceptance() {
    // `run_cleanup` already refuses to let a stage empty a transcript. The
    // recording has to agree with it, or the panel would claim the model wrote
    // the text the rules wrote.
    let trace = trace_at(CleanupLevel::High, |_| Ok("   \n ".to_string()));
    assert_eq!(trace.llm_skip_reason, Some("v1_empty"));
    assert!(!trace.stages.contains(&STAGE_POLISH));
}

#[test]
fn lower_levels_name_only_the_stages_they_run_and_never_a_skip_reason() {
    let none = trace_at(CleanupLevel::None, |_| Err("deadline"));
    assert!(none.stages.is_empty(), "raw passthrough runs no stage");
    assert_eq!(none.llm_skip_reason, None);
    assert_eq!(none.stages_that_ran(), "");

    let light = trace_at(CleanupLevel::Light, |_| Err("deadline"));
    assert_eq!(light.stages, vec![STAGE_DICTIONARY, STAGE_BACKTRACK]);
    assert_eq!(
        light.llm_skip_reason, None,
        "a user who never enabled the model must never be told it skipped"
    );

    let medium = trace_at(CleanupLevel::Medium, |_| Err("deadline"));
    assert_eq!(
        medium.stages,
        vec![STAGE_DICTIONARY, STAGE_BACKTRACK, STAGE_RULES]
    );
    assert_eq!(medium.llm_skip_reason, None);
}

/// The counters half of the signal: one per reason, in the local log, never
/// transmitted.
#[test]
fn skip_reasons_are_counted_per_reason() {
    use wilson_voice_lib::polish;
    polish::reset_polish_skip_log_for_tests();
    polish::note_polish_skip("deadline");
    polish::note_polish_skip("deadline");
    polish::note_polish_skip("no_model");

    let counts = polish::polish_skip_counts();
    assert_eq!(
        counts
            .iter()
            .find(|(r, _)| *r == "deadline")
            .map(|(_, n)| *n),
        Some(2)
    );
    assert_eq!(
        counts
            .iter()
            .find(|(r, _)| *r == "no_model")
            .map(|(_, n)| *n),
        Some(1)
    );

    // The latch is a one-shot: a take consumes it, the next take starts clean.
    assert_eq!(polish::take_polish_skip_reason(), Some("no_model"));
    assert_eq!(polish::take_polish_skip_reason(), None);
    polish::log_polish_skip_counts();
    polish::reset_polish_skip_log_for_tests();
}
