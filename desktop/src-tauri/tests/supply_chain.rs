//! YV45 supply-chain + webview-hardening guards (YV46 adds the signing guard).
//!
//! Both things these tests protect are one-character regressions that nothing
//! else in the build would catch:
//!
//! * the webview CSP — `"csp": null` is valid config and ships a webview with
//!   NO content-security-policy at all, while that webview holds IPC capability
//!   for clipboard read/write, global-shortcut registration and the updater;
//! * git dependencies pinned to a branch instead of a rev — CI builds with a
//!   plain `cargo build` (no `--locked`), so a moved branch head enters a
//!   release build silently.
//!
//! Both files are read at COMPILE time, so the assertions are about the config
//! that actually ships in this binary.

const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const CARGO_TOML: &str = include_str!("../Cargo.toml");
/// The sidecar's manifest (YV60). It is a separate link unit — it vendors its
/// own `ggml` through `llama-cpp-sys-2` — but it ships inside the same signed
/// bundle, so its dependency pins are this app's supply chain too.
const POLISH_CARGO_TOML: &str = include_str!("../../yap-polish/Cargo.toml");

/// The CSP has to exist, and has to be the policy this app was verified
/// against. Every allowance below is something the frontend genuinely loads
/// (see the PR for the audit): bundled JS/CSS from `tauri://localhost`, the
/// Tauri IPC custom protocol, and nothing else — no remote origins, no `eval`.
#[test]
fn csp_is_a_real_policy_not_null() {
    let config: serde_json::Value =
        serde_json::from_str(TAURI_CONF).expect("tauri.conf.json is valid JSON");
    let csp = config["app"]["security"]["csp"]
        .as_str()
        .expect("app.security.csp must be a policy string, never null");

    for directive in [
        "default-src 'self'",
        "script-src 'self'",
        "style-src 'self'",
        "img-src 'self'",
        // The IPC bridge: Tauri v2 posts every `invoke` as a fetch to the
        // `ipc:` custom protocol. Drop this and the whole app is inert.
        "connect-src 'self' ipc: http://ipc.localhost",
        "object-src 'none'",
        "frame-ancestors 'none'",
        "base-uri 'self'",
    ] {
        assert!(
            csp.contains(directive),
            "CSP is missing `{directive}`:\n  {csp}"
        );
    }

    // No wildcard / open-scheme source anywhere, and no `eval`. Checked per
    // source token so `http://ipc.localhost` (an exact host) still passes.
    for directive in csp.split(';') {
        for source in directive.split_whitespace().skip(1) {
            assert!(
                !matches!(source, "*" | "http:" | "https:" | "data:*")
                    && !source.contains("unsafe-eval"),
                "CSP source `{source}` is too broad:\n  {csp}"
            );
        }
    }
    // Scripts specifically: bundled files only. Inline script is where a
    // transcript-rendered injection would land.
    let script_src = csp
        .split(';')
        .map(str::trim)
        .find(|d| d.starts_with("script-src"))
        .expect("script-src is present");
    assert_eq!(
        script_src, "script-src 'self'",
        "script-src must stay exactly `'self'`"
    );
}

/// YV46: the committed config must never name a specific certificate. Only the
/// ad-hoc pseudo-identity `"-"` (or nothing at all) is allowed here — a pinned
/// `Apple Development: …` cert signs every contributor build with a personal
/// certificate that only validates on the machines in that dev profile, and CI
/// runners that do not hold it fail the codesign step outright. Release signing
/// comes from `APPLE_SIGNING_IDENTITY`, which the Tauri CLI applies over this
/// value (crates/tauri-cli ENVIRONMENT_VARIABLES.md).
#[test]
fn no_personal_signing_identity_is_committed() {
    let config: serde_json::Value =
        serde_json::from_str(TAURI_CONF).expect("tauri.conf.json is valid JSON");
    let macos = &config["bundle"]["macOS"];
    match &macos["signingIdentity"] {
        serde_json::Value::Null => {}
        serde_json::Value::String(id) => assert_eq!(
            id, "-",
            "only ad-hoc signing may be committed; set APPLE_SIGNING_IDENTITY at release time"
        ),
        other => panic!("signingIdentity must be a string or absent, got {other}"),
    }
    // Ad-hoc or not, the entitlements still have to be applied to the bundle.
    assert_eq!(
        macos["entitlements"].as_str(),
        Some("Entitlements.plist"),
        "the macOS bundle must keep its entitlements"
    );
}

/// YV60: `llama-cpp-2` compiles and statically links its own vendored `ggml`
/// into the sidecar. A caret range would let a `cargo update` swap that inference
/// engine — and the ggml it links — underneath a signed, notarized bundle
/// without a reviewable diff. It must be an EXACT `=x.y.z` pin from crates.io,
/// never a git source.
#[test]
fn polish_sidecar_pins_llama_cpp_exactly() {
    let line = POLISH_CARGO_TOML
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("llama-cpp-2"))
        .expect("yap-polish depends on llama-cpp-2");
    assert!(
        !line.contains("git = ") && !line.contains("path = "),
        "llama-cpp-2 must come from crates.io: {line}"
    );
    let version = line
        .split("version = \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_else(|| panic!("llama-cpp-2 has no `version = `: {line}"));
    let exact = version
        .strip_prefix('=')
        .unwrap_or_else(|| panic!("llama-cpp-2 must be pinned with `=`, got `{version}`"));
    let parts: Vec<&str> = exact.split('.').collect();
    assert_eq!(parts.len(), 3, "expected an exact x.y.z pin, got `{exact}`");
    assert!(
        parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
        "expected an exact x.y.z pin, got `{exact}`"
    );
    // Metal is what keeps decode off the CPU; losing it silently triples the
    // polish latency the deadline is written against.
    assert!(
        line.contains("\"metal\""),
        "the sidecar must build with the metal backend: {line}"
    );
    // And the sidecar has to actually be bundled, or the app ships with a
    // polish stage it can never spawn.
    let config: serde_json::Value =
        serde_json::from_str(TAURI_CONF).expect("tauri.conf.json is valid JSON");
    let external: Vec<&str> = config["bundle"]["externalBin"]
        .as_array()
        .expect("bundle.externalBin lists the sidecar")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(
        external.contains(&"binaries/yap-polish"),
        "yap-polish must be bundled as a sidecar, got {external:?}"
    );
}

/// Every `git = ` dependency must carry `rev = `. Cargo.lock pins the build
/// today, but a branch dep re-resolves on any `cargo update` / lockfile
/// regeneration, and the release workflow does not pass `--locked`.
#[test]
fn git_dependencies_are_pinned_to_a_rev() {
    let mut checked = 0;
    for line in CARGO_TOML.lines() {
        let line = line.trim();
        if line.starts_with('#') || !line.contains("git = ") {
            continue;
        }
        checked += 1;
        assert!(
            !line.contains("branch = ") && !line.contains("tag = "),
            "git dependency tracks a moving ref: {line}"
        );
        let rev = line
            .split("rev = \"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_else(|| panic!("git dependency has no `rev = `: {line}"));
        assert_eq!(rev.len(), 40, "rev must be a full 40-char sha: {line}");
        assert!(
            rev.chars().all(|c| c.is_ascii_hexdigit()),
            "rev must be a sha: {line}"
        );
    }
    assert!(checked >= 2, "expected the git deps to still be here");
}
