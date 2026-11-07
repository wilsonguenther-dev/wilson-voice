//! WF-06 — the "Backtrack caught" toast is raised ONLY for a real spoken
//! self-correction, never for filler-only cleanup, and only when Undo has a
//! raw take to paste back. Pure: the take pipeline's trace feeds
//! `backtrack_caught_payload`, exactly as the live take in `lib.rs` does.

use wilson_voice_lib::backtrack_caught_payload;
use wilson_voice_lib::dictation::{
    clean_backtrack_report, run_cleanup_traced, BacktrackCorrection, CleanupLevel, DictationMode,
    Style,
};

fn take(raw: &str) -> (String, Option<BacktrackCorrection>) {
    let (text, trace) = run_cleanup_traced(
        raw,
        CleanupLevel::Light,
        DictationMode::Notes,
        Style::default(),
        &[],
        |t| t.to_string(),
        |_| Err("no_model"),
    );
    (text, trace.backtrack)
}

#[test]
fn toast_payload_names_the_correction_and_carries_the_raw_take() {
    let raw = "coffee at 2 actually 3";
    let (text, caught) = take(raw);
    let payload = backtrack_caught_payload("take-1", caught.as_ref(), &text, Some(raw))
        .expect("a real correction raises the toast");
    assert_eq!(payload["id"], "take-1");
    assert_eq!(payload["dropped"], "2");
    assert_eq!(payload["kept"], "3");
    assert_eq!(payload["raw"], raw);
}

#[test]
fn filler_only_cleanup_never_raises_the_toast() {
    for raw in [
        "um so the report is uh done",
        "we need milk, like, and eggs",
        "Basically, the migration is done",
    ] {
        let (text, caught) = take(raw);
        assert_ne!(text, raw, "fillers were cleaned from {raw:?}");
        assert!(
            caught.is_none(),
            "filler-only take {raw:?} caught a backtrack"
        );
        assert!(backtrack_caught_payload("t", caught.as_ref(), &text, Some(raw)).is_none());
    }
}

#[test]
fn i_actually_enjoyed_it_never_raises_the_toast() {
    let raw = "I actually enjoyed it";
    assert!(!clean_backtrack_report(raw).caught());
    let (text, caught) = take(raw);
    assert!(caught.is_none());
    assert!(backtrack_caught_payload("t", caught.as_ref(), &text, Some(raw)).is_none());
}

#[test]
fn no_toast_without_a_raw_take_to_undo_to() {
    let c = BacktrackCorrection {
        dropped: "Tuesday".into(),
        kept: "Friday".into(),
    };
    // No raw column (an old row), or raw identical to what was pasted.
    assert!(backtrack_caught_payload("t", Some(&c), "let's meet Friday", None).is_none());
    assert!(backtrack_caught_payload(
        "t",
        Some(&c),
        "let's meet Friday",
        Some("let's meet Friday")
    )
    .is_none());
    // A correction with an empty side is not something the toast can name.
    let blank = BacktrackCorrection {
        dropped: " ".into(),
        kept: "Friday".into(),
    };
    assert!(backtrack_caught_payload(
        "t",
        Some(&blank),
        "let's meet Friday",
        Some("let's meet Tuesday, wait no, Friday")
    )
    .is_none());
}
