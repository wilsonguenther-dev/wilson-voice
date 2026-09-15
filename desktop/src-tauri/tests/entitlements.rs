//! SEC-A — the entitlements and the signing identity, as a test instead of a
//! habit.
//!
//! Two invariants live here, and both of them look like "the permission broke
//! again" when they regress:
//!
//! 1. `Entitlements.plist` must keep `com.apple.security.app-sandbox` present
//!    and **false**. The VALUE is the assertion, not the key: a plist that
//!    flipped it to `<true/>` still greps clean for "app-sandbox" while the
//!    sandbox silently kills the CGEvent tap, the synthesized Cmd-V and
//!    `AXIsProcessTrusted`. The two dylib-injection entitlements the plist's
//!    own comment forbids must stay ABSENT — this process is non-sandboxed,
//!    Accessibility-trusted and synthesizes keystrokes, so
//!    `DYLD_INSERT_LIBRARIES` into it is a TCC-inheritance escalation.
//!
//! 2. `tauri.conf.json` must not commit an ad-hoc signing identity. Ad-hoc
//!    ("-") has no stable designated requirement: the cdhash changes on every
//!    build, so macOS treats each rebuild as a different program and drops the
//!    Microphone / Accessibility / Input-Monitoring grants. The identity is
//!    resolved at sign time instead (APPLE_SIGNING_IDENTITY, or
//!    `scripts/sign-local.sh`), so nothing here hardcodes a certificate.
//!
//! This is a plain `cargo test` target on purpose: it runs in CI, on every
//! developer machine, with no keychain and no `codesign` available.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn src_tauri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Strip XML comments, then read every `<key>NAME</key><true/>|<false/>` pair
/// in document order. Deliberately tiny: the plist is a flat dict of booleans,
/// and pulling a plist crate into the app's dependency graph to read four keys
/// would be a worse trade than twenty lines here.
fn parse_boolean_entitlements(xml: &str) -> BTreeMap<String, bool> {
    let mut stripped = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find("<!--") {
        stripped.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    stripped.push_str(rest);

    let mut out = BTreeMap::new();
    let mut cursor = stripped.as_str();
    while let Some(k0) = cursor.find("<key>") {
        let after = &cursor[k0 + 5..];
        let Some(k1) = after.find("</key>") else {
            break;
        };
        let name = after[..k1].trim().to_string();
        let tail = after[k1 + 6..].trim_start();
        let value = if tail.starts_with("<true") {
            Some(true)
        } else if tail.starts_with("<false") {
            Some(false)
        } else {
            None
        };
        if let Some(v) = value {
            out.insert(name, v);
        }
        cursor = &after[k1 + 6..];
    }
    out
}

fn entitlements() -> BTreeMap<String, bool> {
    let path = src_tauri().join("Entitlements.plist");
    let xml = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let parsed = parse_boolean_entitlements(&xml);
    assert!(
        !parsed.is_empty(),
        "parsed no boolean entitlements out of {} — the parser and the plist have diverged",
        path.display()
    );
    parsed
}

#[test]
fn app_sandbox_is_present_and_false() {
    let ents = entitlements();
    let value = ents.get("com.apple.security.app-sandbox").copied().unwrap_or_else(|| {
        panic!("com.apple.security.app-sandbox is missing from Entitlements.plist; it must be present and false")
    });
    assert!(
        !value,
        "com.apple.security.app-sandbox is TRUE. The sandbox silently kills the CGEvent tap, the \
         synthesized Cmd-V and AXIsProcessTrusted — the failure then looks like a permissions bug \
         even when every grant is in place. Yap is a utility app: it is never sandboxed."
    );
}

#[test]
fn microphone_entitlement_is_granted() {
    let ents = entitlements();
    assert_eq!(
        ents.get("com.apple.security.device.audio-input").copied(),
        Some(true),
        "com.apple.security.device.audio-input must be true — without it the hardened runtime \
         denies the capture device and every take records silence."
    );
}

#[test]
fn dylib_injection_entitlements_are_absent() {
    let ents = entitlements();
    for forbidden in [
        "com.apple.security.cs.disable-library-validation",
        "com.apple.security.cs.allow-dyld-environment-variables",
    ] {
        assert!(
            !ents.contains_key(forbidden),
            "{forbidden} is granted. This process is non-sandboxed, Accessibility-trusted and \
             synthesizes keystrokes; DYLD_INSERT_LIBRARIES into it is a TCC-inheritance \
             escalation. The plist's own comment forbids both — keep them out."
        );
    }
}

#[test]
fn tauri_config_does_not_commit_an_adhoc_signing_identity() {
    let path = src_tauri().join("tauri.conf.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let cfg: serde_json::Value =
        serde_json::from_str(&raw).expect("tauri.conf.json is not valid JSON");

    let identity = &cfg["bundle"]["macOS"]["signingIdentity"];
    assert!(
        identity.is_null(),
        "bundle.macOS.signingIdentity is {identity} — it must be null so the identity comes from \
         APPLE_SIGNING_IDENTITY (or scripts/sign-local.sh) at sign time. Committing \"-\" signs \
         ad-hoc: no stable designated requirement, so macOS drops Microphone / Accessibility / \
         Input-Monitoring on every rebuild. Committing a certificate name hardcodes a machine."
    );
}

#[test]
fn parser_reads_the_value_not_just_the_key() {
    // The bug this whole test file guards against is a grep that passes on a
    // plist whose value flipped. Prove the parser can see the difference, and
    // that a commented-out entitlement does not count as granted.
    let xml = r#"<plist version="1.0"><dict>
        <key>a.sandbox</key><true/>
        <key>b.sandbox</key>
        <false/>
        <!-- <key>c.forbidden</key><true/> -->
    </dict></plist>"#;
    let parsed = parse_boolean_entitlements(xml);
    assert_eq!(parsed.get("a.sandbox").copied(), Some(true));
    assert_eq!(parsed.get("b.sandbox").copied(), Some(false));
    assert!(
        !parsed.contains_key("c.forbidden"),
        "commented-out keys must not parse as granted"
    );
}
