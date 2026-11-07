//! WF-05 — the "Polished — Reveal" toast is raised ONLY when the polish stage
//! really rewrote the take (never for punctuation or capitals alone), and only
//! when "Use original" has a raw take to paste back. Pure: the take pipeline's
//! trace feeds `polish_reveal_payload`, exactly as the live take in `lib.rs`
//! does, and "Use original" is `dictation::use_original_for_take`.

use wilson_voice_lib::dictation::{
    run_cleanup_traced, use_original_for_take, CleanupLevel, DictationMode, Style,
    UseOriginalRefusal,
};
use wilson_voice_lib::polish_reveal_payload;

fn take(raw: &str, rewrite: &'static str) -> (String, usize, String) {
    let (text, trace) = run_cleanup_traced(
        raw,
        CleanupLevel::High,
        DictationMode::Notes,
        Style::default(),
        &[],
        |t| t.to_string(),
        move |_| Ok(rewrite.to_string()),
    );
    (text, trace.polish_changed_words, trace.stages_that_ran())
}

#[test]
fn a_real_rewrite_raises_the_toast_with_the_raw_take() {
    let raw = "the report is done";
    let (text, changed, stages) = take(raw, "The quarterly report is finished.");
    let payload = polish_reveal_payload("take-1", changed, &text, Some(raw), Some(&stages))
        .expect("a rewrite raises the toast");
    assert_eq!(payload["id"], "take-1");
    assert_eq!(payload["raw"], raw);
    assert_eq!(payload["text"], text.as_str());
    assert_eq!(payload["stages"], stages.as_str());
    assert!(payload["changedWords"].as_u64().unwrap() >= 2);
}

#[test]
fn punctuation_or_capitals_only_never_raise_the_toast() {
    for (raw, rewrite) in [
        ("the report is done", "The report is done."),
        ("hello world how are you", "Hello, world! How are you?"),
        ("dont stop now", "Don't stop now."),
    ] {
        let (text, changed, stages) = take(raw, rewrite);
        assert_eq!(changed, 0, "{raw:?} -> {rewrite:?}");
        assert!(polish_reveal_payload("t", changed, &text, Some(raw), Some(&stages)).is_none());
    }
}

#[test]
fn no_toast_without_a_raw_take_to_paste_back() {
    assert!(polish_reveal_payload("t", 5, "The report is finished.", None, None).is_none());
    assert!(polish_reveal_payload("t", 5, "same", Some("same"), None).is_none());
}

#[test]
fn use_original_replaces_the_polished_text_with_the_raw_take() {
    let raw = "the report is done";
    let (text, _, _) = take(raw, "The quarterly report is finished.");
    assert_eq!(
        use_original_for_take("take-1", "take-1", &text, Some(raw)),
        Ok(raw)
    );
    assert_eq!(
        use_original_for_take("take-1", "take-2", &text, Some(raw)),
        Err(UseOriginalRefusal::NotNewest)
    );
}
