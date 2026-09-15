//! Y4-F — app-aware formatting that is actually APPLIED.
//!
//! `mode_for_app` and six `DictationMode`s have existed since YV3, the shipped
//! default is `dictation_mode: "auto"`, and `focus.rs` resolves the frontmost
//! app. None of that proved the thing the user actually cares about: that the
//! mode CHANGES THE OUTPUT. "Detected, threaded through the pipeline, and then
//! ignored" compiles, type-checks, passes every existing test, and is exactly
//! the failure this file exists to catch.
//!
//! Two axes, both asserted against the SHIPPED pipeline (`run_cleanup` +
//! `join_for_mode`, the wiring `lib.rs` uses), never against a helper in
//! isolation:
//!
//!   1. **The mode changes the output.** One input utterance, every mode, and a
//!      pairwise matrix: outputs that the rules say must differ are asserted
//!      DIFFERENT, and the pairs the rules say are the same contract are
//!      asserted IDENTICAL. An accidental collapse (someone drops the mode
//!      parameter on a stage) turns a `assert_ne!` red; an accidental
//!      divergence turns an `assert_eq!` red.
//!   2. **`auto` picks the right mode.** A fixture table of REAL bundle
//!      identifiers, with an explicit row for an unknown one, asserting the
//!      unknown default is the CONSERVATIVE mode and — separately, because this
//!      is the failure with teeth — that it is not `Email`.
//!
//! Browser ambiguity (Gmail in Chrome is email, Google Docs in Chrome is a
//! document) is NOT resolved here: that needs the tab URL or `AXSelectedText`,
//! which is P1 #11 in the parity backlog. A browser resolves to the conservative
//! mode so the wrong guess is cheap, and that is asserted below.

use std::collections::BTreeMap;

use wilson_voice_lib::dictation::{
    self, CleanupLevel, DictationMode, PauseSpan, Style, DEFAULT_MODE, PARAGRAPH_PAUSE_SECONDS,
};

/// Every mode the app can be in. `List` is included even though it is never
/// inferred from an app (the user forces it), because the pipeline must still
/// give it a defined contract.
const ALL_MODES: &[DictationMode] = &[
    DictationMode::Email,
    DictationMode::Document,
    DictationMode::Notes,
    DictationMode::Code,
    DictationMode::Chat,
    DictationMode::Plain,
    DictationMode::List,
];

/// ONE utterance, run through every mode. It deliberately carries the three
/// things the modes disagree about: a greeting and a sign-off cue (R13, email
/// shape), a punctuation mark dictated by name (R1, inert in `Code`), and a
/// sentence that ends in a period (R3/R14, dropped in `Chat` at the casual
/// positions of the tone dial).
const UTTERANCE: &str =
    "hey team the migration landed last night comma and every check is green thanks so much period";

/// The shipped pipeline, exactly as `lib.rs` wires it: cleanup at the level the
/// app ships with, then the mode-aware caret join. The dictionary is the
/// identity (user vocabulary is per-install) and the polish stage is a no-op,
/// which is also the shipped state — `PolishConfig::default().model` is empty,
/// so no model is installed until the user installs one. Everything asserted
/// here is therefore the RULES stage, on a machine with no model.
fn run(mode: DictationMode, style: Style, pauses: &[PauseSpan]) -> String {
    let text = dictation::run_cleanup(
        UTTERANCE,
        CleanupLevel::High,
        mode,
        style,
        pauses,
        |t| t.to_string(),
        |_| None,
    );
    dictation::join_for_mode(&text, None, mode)
}

fn outputs(style: Style) -> BTreeMap<String, String> {
    ALL_MODES
        .iter()
        .map(|m| (format!("{m:?}"), run(*m, style, &[])))
        .collect()
}

fn out(style: Style, mode: DictationMode) -> String {
    run(mode, style, &[])
}

// ---------------------------------------------------------------------------
// 1. The mode changes the output
// ---------------------------------------------------------------------------

/// THE table test. One input, six modes (plus `List`), and the full pairwise
/// matrix declared explicitly: every pair is either a DIFFERENT contract or the
/// SAME one, and there is no third answer. This is the assertion that catches
/// "the mode is detected, threaded, and then ignored".
#[test]
fn every_mode_is_a_distinct_formatting_contract() {
    let o = outputs(Style::Default);

    // --- pairs that MUST differ, with the rule that makes them differ --------
    let must_differ: &[(DictationMode, DictationMode, &str)] = &[
        // R13: only Email gets the greeting/sign-off shape.
        (
            DictationMode::Email,
            DictationMode::Document,
            "R13 email shape",
        ),
        (
            DictationMode::Email,
            DictationMode::Notes,
            "R13 email shape",
        ),
        (
            DictationMode::Email,
            DictationMode::Chat,
            "R13 email shape + R3 trailing period",
        ),
        (
            DictationMode::Email,
            DictationMode::Plain,
            "R13 email shape",
        ),
        (
            DictationMode::Email,
            DictationMode::Code,
            "R13 email shape + R1 marks are inert in code",
        ),
        // R1/R2: spoken marks expand everywhere EXCEPT code.
        (
            DictationMode::Code,
            DictationMode::Plain,
            "R1 spoken marks are inert in code",
        ),
        (
            DictationMode::Code,
            DictationMode::Document,
            "R1 spoken marks are inert in code",
        ),
        (
            DictationMode::Code,
            DictationMode::Chat,
            "R1 spoken marks are inert in code",
        ),
        // R3/R14: the casual trailing-period rule reaches Chat and nothing else
        // at the shipped `Default` position of the tone dial.
        (
            DictationMode::Chat,
            DictationMode::Document,
            "R3 chat drops the trailing period",
        ),
        (
            DictationMode::Chat,
            DictationMode::Plain,
            "R3 chat drops the trailing period",
        ),
    ];
    for (a, b, why) in must_differ {
        let (ka, kb) = (format!("{a:?}"), format!("{b:?}"));
        assert_ne!(
            o[&ka], o[&kb],
            "{a:?} and {b:?} produced the SAME output, so {why} is not reaching the output.\n\
             {a:?}: {:?}\n{b:?}: {:?}",
            o[&ka], o[&kb]
        );
    }

    // --- pairs that MUST NOT differ, because they are the same contract -----
    // Document and Notes are both "long-form prose": paragraphs, lists, full
    // punctuation, keep the terminal period. If they ever diverge it is a bug,
    // not a feature, and this assertion is where it gets caught.
    assert_eq!(
        o["Document"], o["Notes"],
        "Document and Notes are the same formatting contract and must not diverge"
    );

    // Nothing may be empty: the "never lose text" backstop applies per mode.
    for (mode, text) in &o {
        assert!(!text.trim().is_empty(), "{mode} produced empty output");
    }
}

/// Named explicitly because it is the single most load-bearing difference: the
/// two surfaces the user is in all day must not format the same way.
#[test]
fn chat_mode_output_differs_from_email_mode_output() {
    let chat = out(Style::Default, DictationMode::Chat);
    let email = out(Style::Default, DictationMode::Email);
    assert_ne!(
        chat, email,
        "chat and email formatted identically — the mode is not reaching the output"
    );
    // And specifically, in the directions the rules name:
    assert!(
        !chat.contains('\n'),
        "chat must stay on one line (a newline SENDS the message): {chat:?}"
    );
    assert!(
        email.contains('\n'),
        "email must get the R13 greeting/sign-off shape: {email:?}"
    );
}

/// Email (R13): greeting line, sign-off line, blank line between, full
/// punctuation, terminal period kept at the shipped tone position.
#[test]
fn email_mode_gets_greeting_signoff_and_paragraphs() {
    let email = out(Style::Default, DictationMode::Email);
    let lines: Vec<&str> = email.lines().collect();
    assert!(
        lines.len() >= 3,
        "email shape needs a greeting line, a body and a sign-off: {email:?}"
    );
    assert!(
        lines[0].to_lowercase().starts_with("hey team"),
        "the greeting must be cut onto its own line: {email:?}"
    );
    assert!(
        lines
            .last()
            .expect("non-empty")
            .to_lowercase()
            .contains("thanks so much"),
        "the sign-off must be cut onto its own line: {email:?}"
    );
    assert!(
        email.contains("\n\n"),
        "email keeps a blank line between the greeting and the body: {email:?}"
    );
    // R1 ran: the marks dictated by name became glyphs.
    assert!(
        email.contains(','),
        "spoken 'comma' must expand in email: {email:?}"
    );
    assert!(
        !email.to_lowercase().contains(" comma "),
        "the literal word 'comma' must not survive in email: {email:?}"
    );
}

/// Chat / work chat: no paragraph breaks (a newline sends), no sign-off shape,
/// the trailing period goes at the casual positions of the tone dial, and emoji
/// are left exactly as dictated.
#[test]
fn chat_mode_never_breaks_lines_and_drops_the_trailing_period() {
    // Even handed a pause long enough to be a paragraph break, chat stays flat.
    let pauses = &[PauseSpan {
        seconds: PARAGRAPH_PAUSE_SECONDS * 3.0,
        at_voiced_fraction: 0.5,
    }];
    let chat = run(DictationMode::Chat, Style::Default, pauses);
    assert!(
        !chat.contains('\n'),
        "a newline SENDS a chat message — chat must never be paragraphed: {chat:?}"
    );
    assert!(
        !chat.trim_end().ends_with('.'),
        "R3: chat drops the trailing period: {chat:?}"
    );
    // No sign-off shape: the closing words stay in the flow of the sentence.
    assert!(
        chat.to_lowercase().contains("thanks so much"),
        "the words must all survive: {chat:?}"
    );

    // R14: the tone dial still governs. Formal keeps the period, in chat too.
    let formal = run(DictationMode::Chat, Style::Formal, &[]);
    assert!(
        formal.trim_end().ends_with('.'),
        "R14: the formal position of the dial keeps the period even in chat: {formal:?}"
    );

    // Emoji are the user's, not ours.
    let emoji = dictation::run_cleanup(
        "ship it 🚀 tonight",
        CleanupLevel::High,
        DictationMode::Chat,
        Style::Default,
        &[],
        |t| t.to_string(),
        |_| None,
    );
    assert!(emoji.contains('🚀'), "emoji must be left alone: {emoji:?}");
}

/// Code / terminal: no spoken-mark expansion of characters that ARE syntax, no
/// auto-capitalisation of the lead word, no trailing period. Wilson lives in
/// Claude Code and Cursor; a dictation that capitalises his identifiers is worse
/// than no dictation.
#[test]
fn code_mode_is_verbatim_no_marks_no_capitals_no_period() {
    let code = out(Style::Default, DictationMode::Code);
    assert_eq!(
        code, UTTERANCE,
        "code mode must be verbatim — no marks, no reflow, no casing: {code:?}"
    );

    // The spoken-mark names survive as WORDS: expanding them would have written
    // syntax into a source file the user did not ask for.
    let syntax = dictation::run_cleanup(
        "let path equal open paren dot dot slash src close paren semicolon",
        CleanupLevel::High,
        DictationMode::Code,
        Style::Default,
        &[],
        |t| t.to_string(),
        |_| None,
    );
    assert!(
        !syntax.contains('(') && !syntax.contains(';'),
        "code mode must not expand spoken marks into syntax: {syntax:?}"
    );

    // Auto-capitalisation: a caret sitting after a period in a source file is
    // NOT the end of a sentence. `join_for_mode` is what makes this hold; with
    // the context-only rule the lead word comes back capitalised.
    // Precondition, asserted so this test cannot go vacuously green: the
    // context-only rule really DOES capitalise here. That is the defect, and
    // `join_for_mode`'s code gate is the fix.
    assert!(
        dictation::join_with_context("foo_bar()", Some("let x = 5. ")).starts_with("Foo"),
        "precondition: the context-only rule capitalises after a period — if it stops          doing that, this test is no longer proving anything about the mode gate"
    );
    let joined = dictation::join_for_mode("foo_bar()", Some("let x = 5. "), DictationMode::Code);
    assert!(
        joined.trim_start().starts_with("foo_bar"),
        "code mode must never capitalise an identifier: {joined:?}"
    );
    // The prose rule is unchanged — this is a MODE gate, not a deletion.
    let prose = dictation::join_for_mode("foo bar", Some("It shipped. "), DictationMode::Notes);
    assert!(
        prose.trim_start().starts_with("Foo"),
        "the R5 casing rule must still apply in prose modes: {prose:?}"
    );

    // No trailing period is ever added or removed in code.
    let tail = dictation::run_cleanup(
        "cargo test --release",
        CleanupLevel::High,
        DictationMode::Code,
        Style::Default,
        &[],
        |t| t.to_string(),
        |_| None,
    );
    assert_eq!(tail, "cargo test --release");
}

/// Document / notes: paragraphs at the speaker's own pauses, lists when the
/// speaker dictates one, full punctuation.
#[test]
fn document_mode_gets_paragraphs_and_lists_and_full_punctuation() {
    let long = "The migration landed last night and every check is green. \
                We moved the whole history table over in one pass and nothing \
                needed a manual fix afterwards. The rollback plan was never used. \
                Today is about the read path and the indexes behind it, which is \
                where the remaining latency lives.";
    let pauses = &[PauseSpan {
        seconds: PARAGRAPH_PAUSE_SECONDS * 2.0,
        at_voiced_fraction: 0.5,
    }];
    let doc = dictation::run_cleanup(
        long,
        CleanupLevel::High,
        DictationMode::Document,
        Style::Default,
        pauses,
        |t| t.to_string(),
        |_| None,
    );
    assert!(
        doc.contains("\n\n"),
        "R12: a long take in a document becomes paragraphs: {doc:?}"
    );
    // The same take in chat stays one wall of text, on purpose.
    let chat = dictation::run_cleanup(
        long,
        CleanupLevel::High,
        DictationMode::Chat,
        Style::Default,
        pauses,
        |t| t.to_string(),
        |_| None,
    );
    assert!(
        !chat.contains('\n'),
        "chat is never paragraphed, however long the take: {chat:?}"
    );

    // Full punctuation: the terminal period is kept in a document.
    let doc_period = out(Style::Default, DictationMode::Document);
    assert!(
        doc_period.trim_end().ends_with('.') || doc_period.contains(". "),
        "a document keeps its punctuation: {doc_period:?}"
    );

    // Lists still render in a document mode.
    let list = dictation::run_cleanup(
        "here are the steps first open the file second run the tests third ship it",
        CleanupLevel::High,
        DictationMode::Document,
        Style::Default,
        &[],
        |t| t.to_string(),
        |_| None,
    );
    assert!(
        list.contains('\n'),
        "list intent must still render as a list in a document: {list:?}"
    );
}

// ---------------------------------------------------------------------------
// 2. `auto` resolution — real bundle identifiers
// ---------------------------------------------------------------------------

/// Real bundle identifiers, as `focus::frontmost_app_name` reports them, and the
/// mode `auto` must resolve each to.
const BUNDLE_TABLE: &[(&str, DictationMode)] = &[
    ("com.apple.mail", DictationMode::Email),
    ("com.microsoft.Outlook", DictationMode::Email),
    ("com.tinyspeck.slackmacgap", DictationMode::Chat),
    ("com.hnc.Discord", DictationMode::Chat),
    ("com.apple.MobileSMS", DictationMode::Chat),
    ("com.microsoft.VSCode", DictationMode::Code),
    ("com.apple.dt.Xcode", DictationMode::Code),
    ("com.apple.Terminal", DictationMode::Code),
    ("com.googlecode.iterm2", DictationMode::Code),
    ("com.todesktop.230313mzl4w4u92.Cursor", DictationMode::Code),
    ("com.apple.TextEdit", DictationMode::Document),
    ("notion.id", DictationMode::Document),
    ("md.obsidian", DictationMode::Notes),
    ("com.apple.Notes", DictationMode::Notes),
    // Browsers: ambiguous, therefore conservative. See BROWSER_KEYS.
    ("com.google.Chrome", DEFAULT_MODE),
    ("com.apple.Safari", DEFAULT_MODE),
    ("org.mozilla.firefox", DEFAULT_MODE),
    ("com.microsoft.edgemac", DEFAULT_MODE),
    ("company.thebrowser.Browser", DEFAULT_MODE),
    // Unknown: conservative.
    ("com.acme.SomeUnknownApp", DEFAULT_MODE),
    ("", DEFAULT_MODE),
];

#[test]
fn auto_resolves_real_bundle_identifiers_to_the_right_mode() {
    for (bundle, expected) in BUNDLE_TABLE {
        let resolved = dictation::resolve_mode("auto", bundle);
        assert_eq!(
            resolved, *expected,
            "auto resolved {bundle:?} to {resolved:?}, expected {expected:?}"
        );
    }
}

/// The row with teeth. An app we do not recognise must get the CONSERVATIVE
/// mode — and, named separately because it is the failure a user uninstalls
/// over, it must never get `Email`: a sign-off appearing in a random text field
/// is not a formatting miss, it is text the user has to go and delete.
#[test]
fn unknown_bundle_id_gets_the_conservative_mode() {
    let unknown = [
        "com.acme.SomeUnknownApp",
        "com.example.Whatever",
        "SomeRandomGame",
        "",
        "com.roblox.RobloxPlayer",
    ];
    for app in unknown {
        let mode = dictation::resolve_mode("auto", app);
        assert_eq!(
            mode, DEFAULT_MODE,
            "unknown app {app:?} must get the conservative default"
        );
        assert_ne!(
            mode,
            DictationMode::Email,
            "an unknown app must NEVER get email formatting ({app:?})"
        );
        assert_ne!(
            mode,
            DictationMode::Document,
            "an unknown app must not get paragraphing ({app:?})"
        );
    }

    // And the conservative default is conservative in OUTPUT, not just in name:
    // no sign-off shape, no paragraph break, and the trailing period survives.
    let default_out = out(Style::Default, DEFAULT_MODE);
    assert!(
        !default_out.contains('\n'),
        "the conservative default must not reshape the take: {default_out:?}"
    );
    assert!(
        default_out.trim_end().ends_with('.'),
        "the conservative default must not drop the user's period: {default_out:?}"
    );
}

/// A browser is ambiguous and this item does NOT read the URL. The guess must
/// therefore be the cheap one — and it must not be silently inherited from a
/// coincidental keyword in the bundle id.
#[test]
fn a_browser_is_ambiguous_so_it_gets_the_conservative_mode() {
    for browser in [
        "Google Chrome",
        "com.google.Chrome",
        "Safari",
        "Firefox",
        "Arc",
        "com.microsoft.edgemac",
    ] {
        let mode = dictation::resolve_mode("auto", browser);
        assert_eq!(
            mode, DEFAULT_MODE,
            "{browser:?} resolved to {mode:?}; browser ambiguity is unresolved (P1 #11), \
             so it must take the cheap guess"
        );
    }
    // The one browser signal we CAN read today without a new permission: an
    // email greeting already typed before the caret upgrades the surface.
    assert_eq!(
        dictation::resolve_mode_with_context("auto", "Google Chrome", Some("Hi Sarah,\n")),
        DictationMode::Email,
        "the caret context is the only browser disambiguation this item ships"
    );
}

/// A user-picked mode always beats app detection — `auto` is a default, not a
/// policy. Without this, "app-aware" would quietly mean "app-controlled".
#[test]
fn a_user_picked_mode_beats_the_app() {
    assert_eq!(
        dictation::resolve_mode("code", "com.apple.mail"),
        DictationMode::Code
    );
    assert_eq!(
        dictation::resolve_mode("email", "com.microsoft.VSCode"),
        DictationMode::Email
    );
    assert_eq!(
        dictation::resolve_mode("plain", "com.tinyspeck.slackmacgap"),
        DictationMode::Plain
    );
}
