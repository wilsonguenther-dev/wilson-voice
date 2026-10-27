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

/// yap24-OS4 — the hardened-runtime diet. Both were measured unnecessary under a
/// runtime-ON signature (`scripts/qa/entitlement-diet.sh`, docs/RELEASE.md
/// "Entitlement audit"): the signed binary's in-process ggml-Metal decode passes
/// without them. Each weakens the hardened runtime for the whole process, so a
/// re-add needs a failing signed-runtime run, not a hunch.
#[test]
fn jit_entitlements_are_absent_from_the_app() {
    let ents = entitlements();
    for removed in [
        "com.apple.security.cs.allow-jit",
        "com.apple.security.cs.allow-unsigned-executable-memory",
    ] {
        assert!(
            !ents.contains_key(removed),
            "{removed} is back in Entitlements.plist. yap24-OS4 measured it unnecessary under the \
             hardened runtime (scripts/qa/entitlement-diet.sh); re-adding it needs a signed-runtime \
             run that fails without it, recorded in docs/RELEASE.md."
        );
    }
}

/// yap24-OS4 — the sidecar entitlement files stay empty. SCOPE: these files are
/// read only by `scripts/sign-local.sh`'s default sign mode, which signs each
/// sidecar on its own (no deep signing) so it carries no microphone grant and no
/// runtime-weakening key. `tauri build` (desktop:build, release.yml,
/// test-build.sh) ignores them: tauri-bundler signs every externalBin with the
/// app's Entitlements.plist, so the DMG sidecars carry the app's entitlements.
/// This test asserts the files, not the shipped DMG.
#[test]
fn sidecars_have_their_own_minimal_entitlements() {
    for sidecar in ["yap-polish", "yap-diarize"] {
        let path = src_tauri()
            .join("entitlements")
            .join(format!("{sidecar}.plist"));
        let xml = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        assert!(
            xml.contains("<dict>"),
            "{} is not a plist dict",
            path.display()
        );
        let granted: Vec<String> = parse_boolean_entitlements(&xml)
            .into_iter()
            .filter(|(_, v)| *v)
            .map(|(k, _)| k)
            .collect();
        assert!(
            granted.is_empty(),
            "{sidecar} grants {granted:?}; its entitlements must stay empty unless a signed-runtime \
             run proves a key necessary (docs/RELEASE.md, Entitlement audit)"
        );
    }
}

/// yap24-OS4 — the local signing path turns the hardened runtime ON and signs
/// inside-out. Without `--options runtime` every cs.* entitlement is inert and
/// an entitlement audit passes vacuously; `--deep` stamps the app's
/// entitlements onto the sidecars.
#[test]
fn sign_local_uses_the_hardened_runtime_and_never_deep_signs() {
    let path = src_tauri().join("../../scripts/sign-local.sh");
    let script = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let codesign_calls: Vec<&str> = script
        .lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("codesign --force"))
        .collect();
    assert!(
        codesign_calls.len() >= 2,
        "expected a sidecar codesign call and a bundle codesign call, found {codesign_calls:?}"
    );
    for call in &codesign_calls {
        assert!(
            call.contains("--options runtime") || call.contains("RUNTIME_FLAGS"),
            "codesign call without the hardened runtime: {call}"
        );
        assert!(!call.contains("--deep"), "deep signing is back: {call}");
    }
    assert!(script.contains("RUNTIME_FLAGS=(--options runtime --timestamp)"));
    assert!(script.contains("flags=0x10000(runtime)"));
}

/// yap24-OS4 (PR #262 review, BLOCKING 1) — `scripts/rebuild_app.sh` hand-builds
/// a bundle and hands it to `sign-local.sh`'s sign mode, which refuses a bundle
/// without both sidecars. The script must therefore copy both staged sidecars
/// into Contents/MacOS before signing, and must check they are staged BEFORE it
/// deletes the installed app (otherwise a failed sign leaves /Applications with
/// an app carrying only the linker's ad-hoc signature).
#[test]
fn rebuild_app_bundles_both_sidecars_before_signing() {
    let path = src_tauri().join("../../scripts/rebuild_app.sh");
    let script = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let pos = |needle: &str| {
        script
            .find(needle)
            .unwrap_or_else(|| panic!("rebuild_app.sh is missing `{needle}`"))
    };
    let sidecar_list = pos("SIDECARS=(yap-polish yap-diarize)");
    let staged_check = pos(r#"[[ -x "$SRC/binaries/$s-$TRIPLE" ]]"#);
    let destructive_rm = pos(r#"rm -rf "$HOME/Desktop/Wilson Voice.app""#);
    let copy_in = pos(r#"cp -f "$SRC/binaries/$s-$TRIPLE" "$APP_SRC/Contents/MacOS/$s""#);
    let sign = pos(r#""$SIGN" "$APP_DST""#);
    assert!(
        sidecar_list < staged_check && staged_check < destructive_rm,
        "the staged-sidecar check must run before the installed app is deleted"
    );
    assert!(
        copy_in < sign,
        "both sidecars must be copied into the bundle before sign-local.sh signs it"
    );
}
