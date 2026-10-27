//! PERM-F — deeper Accessibility context: the selection, the text after the
//! caret and (Chrome / Safari / Firefox) the page URL, all read in-process.
//!
//! The AX calls themselves need a live, granted, focused text field, so they
//! are injected: `focus::context_from_reads(trusted, read)` is the exact
//! function `focus::read_ax_context` runs (the source pin in the first test
//! proves the real grant check is what feeds it), and every assertion here is
//! about what that function lets through to the pipeline.
//!
//! What this file pins:
//!   * no grant → the reader is never called and the pipeline is byte for byte
//!     the pre-PERM-F path;
//!   * every text field is cut to its named budget, keeping the characters
//!     adjacent to the caret;
//!   * the URL never survives the read: it is not a field of the context, it
//!     is not in the context's Debug output, and the modules that persist,
//!     log or bundle anything never touch it;
//!   * the URL resolves Gmail to Email and a Google Doc to Document.

use wilson_voice_lib::dictation::{
    join_after, join_for_mode, mode_for_browser_url, resolve_mode, resolve_mode_with_context,
    DictationMode,
};
use wilson_voice_lib::focus::{
    context_from_reads, is_url_browser, AxContext, AxRaw, AFTER_CARET_CHAR_LIMIT,
    CONTEXT_CHAR_LIMIT, SELECTION_CHAR_LIMIT,
};

const FOCUS_RS: &str = include_str!("../src/focus.rs");
const DICTATION_RS: &str = include_str!("../src/dictation.rs");
const LIB_RS: &str = include_str!("../src/lib.rs");
const DB_RS: &str = include_str!("../src/db.rs");
const SUPPORT_RS: &str = include_str!("../src/support.rs");
const CRASH_RS: &str = include_str!("../src/crash.rs");

fn u16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// A granted read of a focused field holding `value`, caret at `location`
/// with `length` units selected.
fn granted(value: &str, location: usize, length: usize, url: Option<&str>) -> AxContext {
    let selected: String = value
        .encode_utf16()
        .skip(location)
        .take(length)
        .collect::<Vec<u16>>()
        .pipe(|u| String::from_utf16_lossy(&u));
    let value = value.to_string();
    let url = url.map(str::to_string);
    context_from_reads(true, move || AxRaw {
        secure: false,
        value: Some(value),
        range: Some((location, length)),
        selected: Some(selected),
        url,
    })
}

trait Pipe: Sized {
    fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}
impl<T> Pipe for T {}

/// The post-ASR pipeline stages the AX context feeds, in the order `lib.rs`
/// runs them: mode resolution, the before-caret join, the after-caret join.
fn pipeline(setting: &str, app: &str, text: &str, ctx: &AxContext) -> (DictationMode, String) {
    let mode =
        resolve_mode_with_context(setting, app, ctx.before_caret.as_deref(), ctx.browser_mode);
    let joined = join_for_mode(text, ctx.before_caret.as_deref(), mode);
    let joined = join_after(
        &joined,
        ctx.after_caret.as_deref(),
        ctx.selection.as_deref(),
        mode,
    );
    (mode, joined)
}

#[test]
fn without_the_grant_the_pipeline_is_byte_identical() {
    // 1. No grant: the AX reader is NEVER called (it would panic) and the
    //    context is the all-None default — what `text_before_cursor` returned
    //    on the same check before PERM-F.
    let ctx = context_from_reads(false, || panic!("an AX read ran without the grant"));
    assert_eq!(ctx, AxContext::default());
    assert!(ctx.before_caret.is_none());
    assert!(ctx.selection.is_none());
    assert!(ctx.after_caret.is_none());
    assert!(ctx.browser_mode.is_none());

    // 2. The real entry point gates on the real grant check, so (1) is the
    //    production path and not a test-only twin.
    assert!(
        FOCUS_RS.contains("context_from_reads(crate::permissions::is_accessibility_trusted(), ||"),
        "read_ax_context must gate the AX read on is_accessibility_trusted()"
    );

    // 3. Fed that context, every stage the context reaches is byte-identical
    //    to the pre-PERM-F pipeline: the mode is `resolve_mode` (what the old
    //    resolver returned for `None` context), the before-join is the old
    //    `join_for_mode(_, None, _)`, and the new after-join is the identity.
    let settings = [
        "auto", "plain", "email", "code", "notes", "list", "document",
    ];
    let apps = [
        "Google Chrome",
        "Safari",
        "Firefox",
        "Mail",
        "Slack",
        "Visual Studio Code",
        "Notion",
        "Some Unknown App",
        "",
    ];
    let takes = [
        "Ship it.",
        "hello world",
        "Meet on Wednesday.",
        "let x = 5.",
        "1. eggs\n2. milk",
        "  padded take.  ",
        "Wait...",
        "",
    ];
    for setting in settings {
        for app in apps {
            let (mode, _) = pipeline(setting, app, "x", &ctx);
            assert_eq!(mode, resolve_mode(setting, app), "{setting}/{app}");
            for take in takes {
                let today = join_for_mode(take, None, mode);
                let (_, now) = pipeline(setting, app, take, &ctx);
                assert_eq!(
                    now.as_bytes(),
                    today.as_bytes(),
                    "{setting}/{app}/{take:?}: no-grant output drifted"
                );
                assert_eq!(join_after(take, None, None, mode), take);
            }
        }
    }
}

#[test]
fn an_oversized_ax_field_is_truncated_to_the_budget() {
    // A whole document on both sides of a huge select-all-ish selection.
    let head = "x".repeat(400_000) + "the wire went out on ";
    let sel = "S".repeat(250_000);
    let tail = " and then it came back".to_string() + &"y".repeat(600_000);
    let value = format!("{head}{sel}{tail}");
    let ctx = granted(&value, u16_len(&head), u16_len(&sel), None);

    let before = ctx.before_caret.expect("before-caret read");
    assert_eq!(before.chars().count(), CONTEXT_CHAR_LIMIT);
    assert!(
        before.ends_with("the wire went out on "),
        "kept the text adjacent to the caret"
    );

    let selection = ctx.selection.expect("selection read");
    assert_eq!(selection.chars().count(), SELECTION_CHAR_LIMIT);

    let after = ctx.after_caret.expect("after-caret read");
    assert_eq!(after.chars().count(), AFTER_CARET_CHAR_LIMIT);
    assert!(
        after.starts_with(" and then it came back"),
        "after-caret starts where the selection ends: {after:?}"
    );

    // Budgets are characters, cut on character boundaries (emoji = 2 UTF-16 units).
    let emoji = "🚀".repeat(AFTER_CARET_CHAR_LIMIT * 3);
    let ctx = granted(&format!("go {emoji}"), 3, 0, None);
    let after = ctx.after_caret.unwrap();
    assert_eq!(after.chars().count(), AFTER_CARET_CHAR_LIMIT);
    assert!(after.chars().all(|c| c == '🚀'), "no split surrogate");
    assert_eq!(ctx.before_caret.as_deref(), Some("go "));
    assert_eq!(
        ctx.selection, None,
        "an empty AXSelectedText is no selection"
    );

    // The budgets are the named ones the docs promise.
    assert_eq!(CONTEXT_CHAR_LIMIT, 500);
    assert_eq!(AFTER_CARET_CHAR_LIMIT, 200);
    assert_eq!(SELECTION_CHAR_LIMIT, 500);
}

#[test]
fn the_url_never_reaches_the_db_a_log_or_a_bundle() {
    let url = "https://mail.google.com/mail/u/0/#inbox/SECRET-THREAD-4471";
    let ctx = granted("Hi Sam,\n", 8, 0, Some(url));
    // The URL did its one job…
    assert_eq!(ctx.browser_mode, Some(DictationMode::Email));

    // …and is gone. The context has no field that could hold it: this
    // exhaustive destructure stops compiling the day someone adds one.
    let AxContext {
        before_caret: _,
        selection: _,
        after_caret: _,
        browser_mode: _,
    } = ctx.clone();

    // Not even a `{:?}` of the context prints it (or any of the text).
    let debug = format!("{ctx:?}");
    for leak in ["mail.google", "SECRET", "https", "Hi Sam"] {
        assert!(!debug.contains(leak), "Debug leaked {leak:?}: {debug}");
    }

    // The raw URL reads live in focus.rs and nowhere else: the modules that
    // write the history DB, the support bundle and the crash report never name
    // them. The patterns are proven able to match by finding them in focus.rs.
    let raw_url_markers = [
        "AXURL",
        "AXDocument",
        "AxRaw",
        "focused_page_url",
        "read_raw(",
    ];
    for marker in raw_url_markers {
        assert!(
            FOCUS_RS.contains(marker),
            "pattern {marker:?} must match its source"
        );
        for (name, src) in [
            ("db.rs", DB_RS),
            ("support.rs", SUPPORT_RS),
            ("crash.rs", CRASH_RS),
            ("lib.rs", LIB_RS),
        ] {
            assert!(
                !src.contains(marker),
                "{name} touches the raw URL read ({marker})"
            );
        }
    }
    // The bundle/crash/DB modules do not consume the context at all.
    for (name, src) in [
        ("db.rs", DB_RS),
        ("support.rs", SUPPORT_RS),
        ("crash.rs", CRASH_RS),
    ] {
        for marker in [
            "read_ax_context",
            "AxContext",
            "browser_mode",
            "after_caret",
        ] {
            assert!(
                !src.contains(marker),
                "{name} reaches the AX context ({marker})"
            );
        }
    }
    // The two modules that hold the raw strings never log, print or emit.
    for (name, src) in [("focus.rs", FOCUS_RS), ("dictation.rs", DICTATION_RS)] {
        for banned in [
            "log::",
            "println!",
            "eprintln!",
            "dbg!",
            ".emit(",
            "Command::new",
        ] {
            assert!(!src.contains(banned), "{name} must not contain {banned}");
        }
    }
    // And no line of lib.rs that touches the context logs, emits or stores it.
    for (i, line) in LIB_RS.lines().enumerate() {
        if line.contains("cursor_context") && !line.trim_start().starts_with("//") {
            for banned in [
                "log::",
                "println!",
                ".emit(",
                "insert_transcript",
                "notify(",
            ] {
                assert!(!line.contains(banned), "lib.rs:{}: {line}", i + 1);
            }
        }
    }
    // No `axHTML`, no screenshot: the competitor's uploads are not read at all.
    assert_eq!(FOCUS_RS.matches("axHTML").count(), 0);
    assert_eq!(FOCUS_RS.matches("AXHTML").count(), 0);
}

#[test]
fn the_browser_url_resolves_gmail_to_email_and_docs_to_document() {
    let gmail = granted(
        "",
        0,
        0,
        Some("https://mail.google.com/mail/u/0/#inbox?compose=new"),
    );
    assert_eq!(gmail.browser_mode, Some(DictationMode::Email));
    assert_eq!(
        resolve_mode_with_context(
            "auto",
            "Google Chrome",
            gmail.before_caret.as_deref(),
            gmail.browser_mode
        ),
        DictationMode::Email
    );

    let doc = granted(
        "",
        0,
        0,
        Some("https://docs.google.com/document/d/1AbC/edit#heading=h.x"),
    );
    assert_eq!(doc.browser_mode, Some(DictationMode::Document));
    assert_eq!(
        resolve_mode_with_context("auto", "Safari", None, doc.browser_mode),
        DictationMode::Document
    );

    // Other surfaces the table names.
    assert_eq!(
        mode_for_browser_url("https://app.slack.com/client/T1/C2"),
        Some(DictationMode::Chat)
    );
    assert_eq!(
        mode_for_browser_url("https://www.notion.so/team/Page-123"),
        Some(DictationMode::Document)
    );
    assert_eq!(
        mode_for_browser_url("https://outlook.office.com/mail/"),
        Some(DictationMode::Email)
    );

    // A Sheet is not prose; an unknown page, a lookalike host and a non-web
    // scheme resolve to nothing, which leaves the browser on today's path.
    for url in [
        "https://docs.google.com/spreadsheets/d/1/edit",
        "https://github.com/org/repo/issues/1",
        "https://mail.google.com.attacker.example/",
        "https://notmail.google.com/",
        "file:///Users/me/mail.google.com.html",
        "mail.google.com",
        "",
    ] {
        assert_eq!(mode_for_browser_url(url), None, "{url}");
    }
    assert_eq!(
        resolve_mode_with_context("auto", "Firefox", None, None),
        resolve_mode("auto", "Firefox")
    );

    // A user-picked mode still wins, and a page mode is ignored for an app
    // that is not a browser.
    assert_eq!(
        resolve_mode_with_context("plain", "Google Chrome", None, Some(DictationMode::Email)),
        DictationMode::Plain
    );
    assert_eq!(
        resolve_mode_with_context("auto", "Some Unknown App", None, Some(DictationMode::Email)),
        resolve_mode("auto", "Some Unknown App")
    );

    // Only the three big browsers ever have their URL read.
    for id in [
        "com.google.Chrome",
        "com.apple.Safari",
        "org.mozilla.firefox",
    ] {
        assert!(is_url_browser(id), "{id}");
    }
    for id in [
        "com.microsoft.VSCode",
        "com.tinyspeck.slackmacgap",
        "com.apple.mail",
        "",
    ] {
        assert!(!is_url_browser(id), "{id}");
    }
}

#[test]
fn a_secure_field_yields_no_text_and_the_dictation_never_depends_on_the_read() {
    let ctx = context_from_reads(true, || AxRaw {
        secure: true,
        value: Some("hunter2".into()),
        range: Some((7, 0)),
        selected: Some("hunter2".into()),
        url: None,
    });
    assert_eq!(ctx, AxContext::default());

    // A granted read that found nothing (no focused element, AX error) is the
    // same no-op as no grant at all.
    let ctx = context_from_reads(true, AxRaw::default);
    assert_eq!(ctx, AxContext::default());
    let (_, out) = pipeline("auto", "Google Chrome", "Ship it.", &ctx);
    assert_eq!(
        out,
        join_for_mode("Ship it.", None, resolve_mode("auto", "Google Chrome"))
    );
}

#[test]
fn the_text_after_the_caret_joins_without_fusing_or_a_stray_period() {
    use DictationMode::*;
    // Caret glued to a word: one space so the take does not fuse with it.
    assert_eq!(
        join_after("Ship it.", Some("Thanks"), None, Plain),
        "Ship it. "
    );
    // The sentence continues after the caret: no stray period.
    assert_eq!(
        join_after("on Wednesday.", Some(" at noon"), None, Plain),
        "on Wednesday"
    );
    assert_eq!(
        join_after("on Wednesday.", Some(", then lunch"), None, Plain),
        "on Wednesday"
    );
    // Replacing a phrase that had no period, with text after it on the line.
    assert_eq!(
        join_after("wednesday.", Some(" Bob said"), Some("Tuesday"), Plain),
        "wednesday"
    );
    // Replacing a whole sentence keeps the period; a new line after keeps it.
    assert_eq!(
        join_after("Done.", Some(" Next"), Some("Old sentence."), Plain),
        "Done."
    );
    assert_eq!(
        join_after("Done.", Some("\nNext"), Some("Old"), Plain),
        "Done."
    );
    // An ellipsis is never trimmed; a lone "." is never emptied.
    assert_eq!(join_after("Wait...", Some(" and"), None, Plain), "Wait...");
    assert_eq!(join_after(".", Some(" and"), None, Plain), ".");
    // Code gets spacing only: `5.` before `foo` is not the end of a sentence.
    assert_eq!(
        join_after("let x = 5.", Some(" foo"), None, Code),
        "let x = 5."
    );
    assert_eq!(join_after("bar", Some("baz"), None, Code), "bar ");
    // Multi-line output (a list) never has its period touched.
    assert_eq!(
        join_after("1. a\n2. b.", Some(" and"), None, List),
        "1. a\n2. b."
    );
    // An empty after-caret (caret at the end) changes nothing.
    assert_eq!(join_after("Ship it.", Some(""), None, Plain), "Ship it.");
}
