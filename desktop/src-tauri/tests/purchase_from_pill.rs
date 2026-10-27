//! Y2-D — the pill can ask for the purchase SHEET, and it can never open a URL.
//!
//! Yap has exactly one Payment Link, one price label and one function allowed to
//! hand a string to `open(1)`. This test stands guard over all three, because
//! every one of them is the kind of thing that grows a second copy during a
//! refactor and none of the growth is visible to `cargo build`, `clippy` or the
//! frontend typechecker.
//!
//! WHY A SOURCE-READING TEST AND NOT A BEHAVIOURAL ONE: the property under test
//! is "this function does NOT do a thing". A behavioural test only observes what
//! a function did on the one path it was driven down; it cannot observe the
//! absence of an opener call on a path nobody exercised. `tests/license_gate.rs`
//! settled this pattern already, so this file follows it.
//!
//! IF YOU ARE HERE BECAUSE THIS FAILED: that is the point. Either keep the URL
//! behind `open_purchase_page`, or come and change this list on purpose.

const LIB_RS: &str = include_str!("../src/lib.rs");
const LICENSE_RS: &str = include_str!("../src/license.rs");
const PILL_LICENSE_TS: &str = include_str!("../../src/pill/license.ts");
const CLASSIC_PILL_TSX: &str = include_str!("../../src/pill/PillShell.tsx");
const YAPPY_PILL_TSX: &str = include_str!("../../src/pill/YappyPill.tsx");
const APP_SHELL_TS: &str = include_str!("../../src/appShell.ts");

/// Line comments stripped, so this file tests CODE and not prose. String
/// literals are left alone on purpose: a URL in a literal IS the thing being
/// forbidden. (Only `//` that is not inside `://` counts as a comment, so a
/// literal URL is never mistaken for one and silently cut away.)
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| {
            let mut cut = l.len();
            let mut from = 0;
            while let Some(i) = l[from..].find("//") {
                let at = from + i;
                if at > 0 && l.as_bytes()[at - 1] == b':' {
                    from = at + 2;
                    continue;
                }
                cut = at;
                break;
            }
            &l[..cut]
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The source text of one `fn`, from its signature to its matching closing
/// brace. Brace-counted so a body that grows a nested block is still read in
/// full — a truncated body would let this file pass by reading less than it
/// claims to read.
fn fn_body(src: &str, name: &str) -> String {
    let sig = format!("fn {name}(");
    assert_eq!(
        src.matches(&sig).count(),
        1,
        "expected exactly one `fn {name}` in the source this test reads"
    );
    let start = src.find(&sig).unwrap();
    let open = start + src[start..].find('{').expect("fn has no body");
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return src[open..=open + i].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("`fn {name}` body is unbalanced");
}

/// The forbidden shapes, each paired with the fear it is there to catch.
const FORBIDDEN: &[(&str, &str)] = &[
    (
        "PAYMENT_LINK_URL",
        "the Stripe Payment Link constant itself",
    ),
    (
        "open_purchase_page",
        "delegating to the opener, which is the same thing one hop away",
    ),
    ("Command::new", "spawning any process at all"),
    ("std::process", "spawning any process at all, spelled long"),
    ("http://", "a literal URL"),
    ("https://", "a literal URL"),
    ("buy.stripe.com", "the checkout host"),
    ("opener", "the tauri-plugin-opener surface"),
    ("shell", "the tauri-plugin-shell surface"),
];

/// THE security property of this item.
///
/// `reveal_purchase_prompt` is reachable from the FLOAT webview — the pill, the
/// least-trusted surface in the app: an always-on capsule over every other
/// application. If that command could name a URL, "open an arbitrary thing on
/// this Mac" would become a primitive one `invoke` away. So it names none: it
/// presents a window and emits an event, and the URL stays behind
/// `open_purchase_page`, which takes no argument and reads a compile-time
/// constant.
#[test]
fn reveal_purchase_prompt_never_opens_a_url() {
    let body = code_only(&fn_body(LIB_RS, "reveal_purchase_prompt"));
    for (needle, why) in FORBIDDEN {
        assert!(
            !body.contains(needle),
            "`reveal_purchase_prompt` contains `{needle}` ({why}). The pill must \
             not be able to reach a URL; keep that behind `open_purchase_page`.\n\
             body was:\n{body}"
        );
    }
    // It does the one thing it exists for: ask the main window for the sheet.
    assert!(
        body.contains(r#"emit("show_purchase""#),
        "`reveal_purchase_prompt` no longer emits `show_purchase`:\n{body}"
    );

    // POSITIVE CONTROL — the patterns above can actually MATCH the shape they
    // fear. `open_purchase_page` legitimately does this; if the patterns did
    // not fire on it, they would prove nothing about the function under test.
    let opener = code_only(&fn_body(LIB_RS, "open_purchase_page"));
    let fired: Vec<&str> = FORBIDDEN
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| opener.contains(n))
        .collect();
    assert!(
        fired.contains(&"PAYMENT_LINK_URL") && fired.contains(&"Command::new"),
        "positive control failed: `open_purchase_page` no longer hands \
         PAYMENT_LINK_URL to a process (fired: {fired:?})"
    );

    // REACHABLE: a command missing from generate_handler! fails at runtime on
    // a perfectly green build.
    let h = LIB_RS
        .find("tauri::generate_handler![")
        .expect("no generate_handler! in lib.rs");
    let handler = &LIB_RS[h..h + LIB_RS[h..].find("])").expect("unterminated handler")];
    assert!(
        handler.contains("reveal_purchase_prompt"),
        "`reveal_purchase_prompt` is not in generate_handler!"
    );

    // …and the frontend actually listens for what it emits.
    assert!(
        APP_SHELL_TS.contains(r#"listen("show_purchase""#),
        "appShell.ts no longer listens for `show_purchase`, so the sheet never rises"
    );
}

/// A licensed install has no upgrade affordance AT ALL — not a disabled one,
/// not a transparent one. A dead region inside the small target a person aims
/// at to start dictating is a defect found only by missing; and `problem`
/// (Y2-F) is a PAYING customer who must never be routed to a checkout. Both are
/// decided by the policy module and asserted by reading it.
#[test]
fn pill_upgrade_is_inert_when_licensed() {
    let silent_start = PILL_LICENSE_TS
        .find("PILL_LICENSE_SILENT: PillLicense = {")
        .expect("PILL_LICENSE_SILENT is gone from pill/license.ts");
    let silent = &PILL_LICENSE_TS[silent_start
        ..silent_start
            + PILL_LICENSE_TS[silent_start..]
                .find("};")
                .expect("unterminated PILL_LICENSE_SILENT")];
    assert!(
        silent.contains("show: false"),
        "silent answer shows:\n{silent}"
    );
    assert!(
        silent.contains(r#"action: "none""#),
        "silent answer offers a route:\n{silent}"
    );
    assert!(
        PILL_LICENSE_TS.contains(r#"if (status.state === "licensed") return PILL_LICENSE_SILENT;"#),
        "the `licensed` branch no longer returns the silent answer unconditionally"
    );

    // The route function refuses before it looks at anything else.
    let route = code_only(&{
        let s = PILL_LICENSE_TS
            .find("export function pillLicenseRoute(")
            .expect("pillLicenseRoute is gone");
        PILL_LICENSE_TS[s..s + PILL_LICENSE_TS[s..].find("\n}").unwrap()].to_string()
    });
    let first = route
        .lines()
        .find(|l| l.trim_start().starts_with("if"))
        .unwrap_or("");
    assert!(
        first.contains("if (!pill?.show) return null;"),
        "pillLicenseRoute's first test is no longer `show`:\n{route}"
    );
    assert!(
        route.contains(r#"pill.action === "purchase") return "reveal_purchase""#),
        "only the `purchase` action may reach the purchase sheet:\n{route}"
    );

    // The capsule renders the mark only when the policy SHOWS (a licensed
    // install never does), and the mark is a BUTTON only behind a non-null
    // route — yap24-PILL1: with checkout closed the countdown is an inert note,
    // never a press. Both faces send a press to the sheet only on
    // `reveal_purchase`.
    assert!(
        CLASSIC_PILL_TSX.contains("license?.show && !blocked && !gated"),
        "PillShell no longer guards the upgrade mark on the policy's `show`"
    );
    assert!(
        CLASSIC_PILL_TSX.contains("return licenseRoute !== null ? ("),
        "PillShell no longer guards the upgrade BUTTON on the policy's route"
    );
    for (name, src) in [
        ("PillShell", CLASSIC_PILL_TSX),
        ("YappyPill", YAPPY_PILL_TSX),
    ] {
        assert!(
            src.contains(r#"licenseRoute === "reveal_purchase""#)
                && src.contains("invoke(REVEAL_PURCHASE_COMMAND)"),
            "{name} no longer routes only `reveal_purchase` to the sheet"
        );
        for needle in [
            "buy.stripe.com",
            "https://",
            "$29",
            "$19",
            "open_purchase_page",
        ] {
            assert!(
                !code_only(src).contains(needle),
                "{name} contains `{needle}` — the pill names no URL, no price and \
                 never the opener; that belongs to PurchasePrompt.tsx"
            );
        }
    }
    assert!(
        PILL_LICENSE_TS.contains(r#"REVEAL_PURCHASE_COMMAND = "reveal_purchase_prompt""#),
        "the pill's purchase route no longer names `reveal_purchase_prompt`"
    );
}

/// Exactly ONE Payment Link exists, and it is in Rust.
#[test]
fn there_is_exactly_one_payment_link() {
    assert_eq!(
        LICENSE_RS.matches("PAYMENT_LINK_URL: &str =").count(),
        1,
        "PAYMENT_LINK_URL is declared more than once in license.rs"
    );
    for src in [PILL_LICENSE_TS, CLASSIC_PILL_TSX, YAPPY_PILL_TSX] {
        assert!(
            !code_only(src).contains("stripe.com"),
            "a Stripe URL leaked into the pill layer"
        );
    }
}
