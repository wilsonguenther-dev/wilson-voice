//! WF-05 — the "Polished — Reveal" toast is raised ONLY when the polish stage
//! really rewrote the take (never for punctuation or capitals alone), and only
//! when "Use original" has a raw take to paste back. Pure: the take pipeline's
//! trace feeds `polish_reveal_payload`, exactly as the live take in `lib.rs`
//! does, and "Use original" is `dictation::use_original_for_take`.

use wilson_voice_lib::dictation::{
    replace_range_for_take, run_cleanup_traced, use_original_for_take, CleanupLevel, DictationMode,
    ReplaceRefusal, Style, UseOriginalRefusal,
};
use wilson_voice_lib::paste::{
    self, AxInsert, BridgeReceipt, CopyCard, FieldSelection, InsertBackend, InsertPath,
    REPLACE_NOT_FOUND_MESSAGE,
};
use wilson_voice_lib::{polish_reveal_payload, use_original_reply};

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

// ── fix-l3b7-1 — Use original REPLACES the polished take in its field ───────

const POLISHED: &str = "The quarterly report is finished.";
const RAW: &str = "the report is done";

fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

#[test]
fn replace_range_matches_the_take_right_before_the_caret() {
    let value = format!("Hi team. {POLISHED}");
    let caret = len16(&value);
    assert_eq!(
        replace_range_for_take(Some(&value), Some((caret, 0)), POLISHED),
        Ok((len16("Hi team. "), len16(POLISHED)))
    );
    // A stored take with stray outer whitespace still matches what landed.
    assert_eq!(
        replace_range_for_take(
            Some(&value),
            Some((caret, 0)),
            " The quarterly report is finished. "
        ),
        Ok((len16("Hi team. "), len16(POLISHED)))
    );
}

#[test]
fn replace_range_counts_utf16_units_for_emoji_and_cjk() {
    // CJK: one unit per character; 🎉 is a surrogate pair (two units).
    let polished = "会议改到周五 🎉";
    let value = format!("前文 {polished}");
    let caret = len16(&value);
    let got = replace_range_for_take(Some(&value), Some((caret, 0)), polished);
    assert_eq!(
        got,
        Ok((3, 9)),
        "前文 = 3 units, 会议改到周五 🎉 = 6 + 1 + 2"
    );
    // Emoji BEFORE the take shift the location, and a caret mid-field works.
    let value = "🎉🎉 Ship it today. tail";
    let caret = len16("🎉🎉 Ship it today.");
    assert_eq!(
        replace_range_for_take(Some(value), Some((caret, 0)), "Ship it today."),
        Ok((5, 14))
    );
}

#[test]
fn replace_range_refuses_when_the_user_typed_after_the_take() {
    let value = format!("{POLISHED} and one more thing");
    assert_eq!(
        replace_range_for_take(Some(&value), Some((len16(&value), 0)), POLISHED),
        Err(ReplaceRefusal::NotBeforeCaret)
    );
}

#[test]
fn replace_range_refuses_a_non_empty_selection() {
    let value = format!("Hi. {POLISHED}");
    assert_eq!(
        replace_range_for_take(Some(&value), Some((4, len16(POLISHED))), POLISHED),
        Err(ReplaceRefusal::Selection)
    );
}

#[test]
fn replace_range_refuses_a_mismatch_or_an_unreadable_field() {
    let value = "Something else entirely.";
    assert_eq!(
        replace_range_for_take(Some(value), Some((len16(value), 0)), POLISHED),
        Err(ReplaceRefusal::NotBeforeCaret)
    );
    assert_eq!(
        replace_range_for_take(None, Some((3, 0)), POLISHED),
        Err(ReplaceRefusal::Unreadable)
    );
    assert_eq!(
        replace_range_for_take(Some(value), None, POLISHED),
        Err(ReplaceRefusal::Unreadable)
    );
    // Caret past the end of the value that was read: the field changed.
    assert_eq!(
        replace_range_for_take(Some("short"), Some((500, 0)), "short"),
        Err(ReplaceRefusal::NotBeforeCaret)
    );
    assert_eq!(
        replace_range_for_take(Some(value), Some((len16(value), 0)), "  "),
        Err(ReplaceRefusal::EmptyTake)
    );
}

/// The focused field: what it reads as, and what a selection set does.
struct Field {
    value: Option<String>,
    range: Option<(usize, usize)>,
    select_ok: bool,
    /// A select that returns Ok but never actually moves the selection.
    select_sticks: bool,
    selects: Vec<(usize, usize)>,
}

impl Field {
    fn caret_after(value: &str) -> Self {
        Field {
            value: Some(value.to_string()),
            range: Some((len16(value), 0)),
            select_ok: true,
            select_sticks: true,
            selects: vec![],
        }
    }
}

impl FieldSelection for Field {
    fn read_field(&mut self) -> (Option<String>, Option<(usize, usize)>) {
        (self.value.clone(), self.range)
    }
    fn select(&mut self, location: usize, length: usize) -> Result<(), String> {
        self.selects.push((location, length));
        if !self.select_ok {
            return Err("refused".into());
        }
        if self.select_sticks {
            self.range = Some((location, length));
        }
        Ok(())
    }
}

/// Counting insert backend: `posts` = every ⌘V the live backend would post.
#[derive(Default)]
struct Backend {
    copies: Vec<String>,
    inserted: Vec<String>,
    ax_calls: usize,
    posts: usize,
    cards: usize,
}

impl InsertBackend for Backend {
    fn copy(&mut self, text: &str) -> Result<(), String> {
        self.copies.push(text.to_string());
        Ok(())
    }
    fn frontmost_app(&mut self) -> Option<String> {
        Some("com.apple.TextEdit".into())
    }
    fn ax_insert(&mut self, text: &str) -> AxInsert {
        self.ax_calls += 1;
        self.inserted.push(text.to_string());
        AxInsert::Inserted
    }
    fn paste_bridge(&mut self, text: &str) -> Result<BridgeReceipt, String> {
        self.posts += 1;
        self.inserted.push(text.to_string());
        Ok(BridgeReceipt::Confirmed { after_ms: 9 })
    }
    fn post_delete(&mut self) -> Result<(), String> {
        self.posts += 1;
        Ok(())
    }
    fn show_copy_card(&mut self, _card: CopyCard) {
        self.cards += 1;
    }
}

#[test]
fn use_original_selects_the_polished_take_then_types_the_raw_take_over_it() {
    let value = format!("Hi team. {POLISHED}");
    let mut field = Field::caret_after(&value);
    let mut b = Backend::default();
    let r = paste::replace_take_with(&mut field, &mut b, InsertPath::PasteBridge, POLISHED, RAW);
    assert_eq!(r.replaced, Ok(()));
    assert_eq!(field.selects, vec![(len16("Hi team. "), len16(POLISHED))]);
    assert_eq!(
        b.inserted,
        vec![RAW.to_string()],
        "one insert, over the selection"
    );
    assert_eq!(b.posts, 1);
    assert!(r.outcome.pasted);
    let line = paste::replace_log_line("dictation polish_reveal_use_original", &r, 12);
    assert!(line.contains("replaced=true"), "{line}");
    assert!(use_original_reply(&r).is_ok());
}

#[test]
fn use_original_on_a_refused_range_copies_only_and_never_pastes() {
    // The user typed after the take: the polished words are not before the caret.
    let value = format!("{POLISHED} and then I kept typing");
    let mut field = Field::caret_after(&value);
    let mut b = Backend::default();
    let r = paste::replace_take_with(&mut field, &mut b, InsertPath::PasteBridge, POLISHED, RAW);
    assert_eq!(r.replaced, Err(ReplaceRefusal::NotBeforeCaret));
    assert!(field.selects.is_empty(), "no selection is ever set");
    assert_eq!(b.posts, 0, "no ⌘V");
    assert_eq!(b.ax_calls, 0, "no AX insert");
    assert!(b.inserted.is_empty());
    assert_eq!(b.cards, 0);
    assert_eq!(
        b.copies,
        vec![RAW.to_string()],
        "the raw take is on the clipboard"
    );
    assert!(!r.outcome.pasted && r.outcome.copied);
    assert_eq!(r.outcome.message, REPLACE_NOT_FOUND_MESSAGE);
    assert_eq!(
        use_original_reply(&r),
        Err(REPLACE_NOT_FOUND_MESSAGE.to_string()),
        "the pill gets the plain clipboard sentence"
    );
    let line = paste::replace_log_line("dictation polish_reveal_use_original", &r, 3);
    assert!(
        line.contains(
            "dictation polish_reveal_use_original replaced=false reason=not_before_caret"
        ),
        "{line}"
    );
    for text in [RAW, POLISHED, "kept typing"] {
        assert!(
            !line.contains(text),
            "the log line never carries text: {line}"
        );
    }
}

#[test]
fn a_selection_that_is_refused_or_does_not_stick_is_copy_only() {
    let value = format!("Hi. {POLISHED}");
    for (select_ok, select_sticks) in [(false, true), (true, false)] {
        let mut field = Field::caret_after(&value);
        field.select_ok = select_ok;
        field.select_sticks = select_sticks;
        let mut b = Backend::default();
        let r = paste::replace_take_with(&mut field, &mut b, InsertPath::Ax, POLISHED, RAW);
        assert_eq!(r.replaced, Err(ReplaceRefusal::SelectFailed));
        assert_eq!((b.posts, b.ax_calls), (0, 0));
        assert_eq!(b.copies, vec![RAW.to_string()]);
        assert_eq!(r.outcome.message, REPLACE_NOT_FOUND_MESSAGE);
    }
}

#[test]
fn an_unreadable_field_is_copy_only() {
    let mut field = Field::caret_after("");
    field.value = None;
    field.range = None;
    let mut b = Backend::default();
    let r = paste::replace_take_with(&mut field, &mut b, InsertPath::PasteBridge, POLISHED, RAW);
    assert_eq!(r.replaced, Err(ReplaceRefusal::Unreadable));
    assert_eq!(b.posts, 0);
    assert!(paste::replace_log_line("dictation undo_ai_edit", &r, 1)
        .contains("replaced=false reason=ax_unreadable"));
}

/// The body of `fn <name>(` in lib.rs, up to the closing brace at column 0.
fn lib_fn_body(src: &str, name: &str) -> String {
    let start = src
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("{name} not found in lib.rs"));
    let rest = &src[start..];
    let end = rest.find("\n}\n").expect("fn body ends");
    rest[..end].to_string()
}

#[test]
fn undo_ai_edit_and_use_original_share_the_replace_helper() {
    let src = include_str!("../src/lib.rs");
    for name in ["paste_raw_last_transcript", "use_original_take"] {
        let body = lib_fn_body(src, name);
        assert!(
            body.contains("paste::replace_take("),
            "{name} must replace in place"
        );
        assert!(
            !body.contains("copy_and_maybe_paste("),
            "{name} must never paste blindly after the polished take"
        );
    }
}
