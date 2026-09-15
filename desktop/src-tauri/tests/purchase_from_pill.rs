//! Y2-D — the pill can ask for the purchase SHEET, and it can never open a URL.
//!
//! Yap has exactly one Payment Link, one price label and one function allowed to
//! hand a string to `open(1)`. This test stands guard over all three, because
//! every one of them is the kind of thing that grows a second copy during a
//! refactor and none of the growth is visible to `cargo build`, `clippy` or the
//! frontend typechecker.
//!
//! WHY A SOURCE-READING TEST AND NOT A BEHAVIOURAL ONE
//! --------------------------------------------------
//! The property under test is "this function does NOT do a thing". A behavioural
//! test can only observe what a function DID on the one path it was driven down;
//! it cannot observe the absence of an opener call on a path nobody exercised.
//! `tests/license_gate.rs` already settled this pattern for the license gate and
//! the reasoning is identical here, so this file follows it rather than invent a
//! second style.
//!
//! IF YOU ARE HERE BECAUSE THIS FAILED: that is the point. Either keep the URL
//! behind `open_purchase_page`, or come and change this list on purpose.

const LIB_RS: &str = include_str!("../src/lib.rs");
const LICENSE_RS: &str = include_str!("../src/license.rs");
const PILL_LICENSE_TS: &str = include_str!("../../src/pill/license.ts");
const CLASSIC_PILL_TSX: &str = include_str!("../../src/pill/ClassicPill.tsx");

/// Line comments stripped, so this file tests CODE and not prose.
///
/// Without this the word "shell" in a sentence explaining that the pill must not
/// reach the shell would fail the test that enforces it — which is a test that
/// punishes documentation, and the first thing a hurried reader would delete the
/// documentation to satisfy. String literals are left alone on purpose: a URL in
/// a literal IS the thing being forbidden.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The source text of one `fn`, from its signature to its matching closing
/// brace. Brace-counted rather than line-counted so a body that grows, or gains
/// a nested block, is still read in full — a truncated body would let this whole
/// file pass by reading less than it claims to read.
fn fn_body(src: &str, name: &str) -> String {
    let sig = format!("fn {name}(");
    let start = src
        .find(&sig)
        .unwrap_or_else(|| panic!("`fn {name}` is not in the source this test reads"));
    let open = start
        + src[start..]
            .find('{')
            .unwrap_or_else(|| panic!("`fn {name}` has no body"));
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

/// THE security property of this item.
///
/// `reveal_purchase_prompt` is reachable from the FLOAT webview — the pill — and
/// the float window is the least-trusted surface in the app: it is an always-on
/// capsule sitting over every other application, and anything it can invoke is
/// effectively ambient. If that command could name a URL, "open an arbitrary
/// thing on this Mac" would become a primitive one `invoke` away.
///
/// So it names none. It unminimizes a window, focuses it and emits an event. The
/// URL stays behind `open_purchase_page`, which takes no argument and reads a
/// compile-time constant — the discipline `license.rs:121` and `status.ts:56-62`
/// both document in prose and nothing enforced until now.
#[test]
fn reveal_purchase_prompt_never_opens_a_url() {
    let raw = fn_body(LIB_RS, "reveal_purchase_prompt");
    let body = code_only(&raw);

    // Each pattern is paired with the shape it is there to catch, so a future
    // reader can tell whether the pattern still matches the fear.
    let forbidden: &[(&str, &str)] = &[
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
    for (needle, why) in forbidden {
        assert!(
            !body.contains(needle),
            "`reveal_purchase_prompt` contains `{needle}` ({why}). The pill must \
             not be able to reach a URL; keep that behind `open_purchase_page`.\n\
             body was:\n{body}"
        );
    }

    // Prove the patterns above can actually MATCH the shape they fear, rather
    // than being a grep that would pass against any text. `open_purchase_page`
    // is the positive control: it is the function that legitimately does this.
    let opener = code_only(&fn_body(LIB_RS, "open_purchase_page"));
    assert!(
        opener.contains("PAYMENT_LINK_URL") && opener.contains("Command::new"),
        "the positive control failed: `open_purchase_page` no longer hands \
         PAYMENT_LINK_URL to a process, so this test's patterns prove nothing \
         about `reveal_purchase_prompt`"
    );

    // …and that it is still the ONLY function in the binary that does so. A
    // second one would make the single-choke-point claim above a lie.
    let openers = LIB_RS
        .match_indices("PAYMENT_LINK_URL")
        .filter(|(i, _)| {
            // Ignore mentions inside doc comments: a `///` line explaining the
            // discipline is not a use of the constant.
            let line_start = LIB_RS[..*i].rfind('\n').map(|n| n + 1).unwrap_or(0);
            !LIB_RS[line_start..*i].trim_start().starts_with("//")
        })
        .count();
    assert!(
        openers > 0,
        "no non-comment use of PAYMENT_LINK_URL in lib.rs at all — the checkout \
         is unreachable, which is a worse bug than the one this test guards"
    );

    // The command must also be REACHABLE, or this whole item is dead code that
    // compiles. `invoke("reveal_purchase_prompt")` fails at runtime on a
    // perfectly green build when the handler list is missing an entry.
    let handler_start = LIB_RS
        .find("tauri::generate_handler![")
        .expect("no generate_handler! invocation in lib.rs");
    let handler = &LIB_RS[handler_start
        ..handler_start
            + LIB_RS[handler_start..]
                .find("])")
                .expect("unterminated generate_handler!")];
    assert!(
        handler.contains("reveal_purchase_prompt"),
        "`reveal_purchase_prompt` is not in generate_handler!, so the pill's \
         invoke() would fail at runtime while everything compiled"
    );
}

/// A licensed install has no upgrade affordance AT ALL — not a disabled one, not
/// a transparent one. This matters twice over:
///
///  1. The pill is a small target that a person aims at to start dictating. A
///     dead region inside it that swallows the press is a defect you only find
///     by missing, and only sometimes.
///  2. `problem` (Y2-F) is a PAYING customer whose key stopped granting. Routing
///     that tone to a checkout asks somebody to buy what they already bought.
///
/// Both are decided by the policy module, never by a component, so both can be
/// asserted by reading rather than by driving a UI that has no test harness.
#[test]
fn pill_upgrade_is_inert_when_licensed() {
    // The silent answer — what a licensed status returns — offers nothing.
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
        "PILL_LICENSE_SILENT no longer sets `show: false`, so a licensed pill \
         would draw something:\n{silent}"
    );
    assert!(
        silent.contains(r#"action: "none""#),
        "PILL_LICENSE_SILENT no longer sets `action: \"none\"`, so a licensed \
         pill would offer a route somewhere:\n{silent}"
    );

    // `licensed` returns exactly that object, before any trial arithmetic runs.
    assert!(
        PILL_LICENSE_TS.contains(r#"if (status.state === "licensed") return PILL_LICENSE_SILENT;"#),
        "the `licensed` branch of pillLicense no longer returns the silent \
         answer unconditionally"
    );

    // The capsule renders the mark only behind `license?.show`, and the
    // purchase route only behind `action === \"purchase\"`.
    assert!(
        CLASSIC_PILL_TSX.contains("license?.show && license.action !== \"none\""),
        "ClassicPill no longer guards the upgrade mark on `show` + a real \
         action, so a licensed pill could grow a dead click region"
    );
    assert!(
        CLASSIC_PILL_TSX.contains("if (!license?.show) return;"),
        "ClassicPill's license-surface handler no longer refuses to act when \
         the policy said `show: false`"
    );
    assert!(
        CLASSIC_PILL_TSX.contains(r#"license.action === "purchase""#),
        "ClassicPill no longer gates the purchase route on the `purchase` \
         action, so Y2-F's `problem` tone could reach a checkout"
    );

    // And the pill never learns a URL or a price: it names a COMMAND.
    assert!(
        PILL_LICENSE_TS.contains(r#"REVEAL_PURCHASE_COMMAND = "reveal_purchase_prompt""#),
        "the pill's purchase route no longer names `reveal_purchase_prompt`"
    );
    for needle in ["buy.stripe.com", "https://", "$29", "$19"] {
        assert!(
            !CLASSIC_PILL_TSX.contains(needle),
            "ClassicPill.tsx contains `{needle}` — the pill shows no price and \
             names no URL; that copy belongs to PurchasePrompt.tsx"
        );
    }
}

/// Exactly ONE Payment Link exists, and it is in Rust. A second copy — in a
/// config, a .env, a TS constant or a second Rust const — is a mispriced or
/// dead checkout that nothing would catch until a customer hit it.
#[test]
fn there_is_exactly_one_payment_link() {
    assert_eq!(
        LICENSE_RS.matches("PAYMENT_LINK_URL: &str =").count(),
        1,
        "PAYMENT_LINK_URL is declared more than once in license.rs"
    );
    assert!(
        !PILL_LICENSE_TS.contains("stripe") && !CLASSIC_PILL_TSX.contains("stripe"),
        "a Stripe URL leaked into the pill layer"
    );
}
