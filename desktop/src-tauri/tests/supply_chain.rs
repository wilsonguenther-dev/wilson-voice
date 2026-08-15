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
/// The diarization sidecar's manifest (YV121). Same argument as above: a
/// separate link unit, inside the same signed bundle.
const DIARIZE_CARGO_TOML: &str = include_str!("../../yap-diarize/Cargo.toml");
/// The workspace lockfile — the file that decides which code actually links,
/// as opposed to which ranges the manifests would accept (YV100).
const CARGO_LOCK: &str = include_str!("../../Cargo.lock");

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
        parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
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

/// YV121: `yap-diarize` is bundled, and it carries **no inference crate yet**.
///
/// Both halves are the item. The bundling half is the same failure `yap-polish`
/// would have had: an app that ships a diarization path it can never spawn.
///
/// The dependency half is the point of landing this sidecar before YV122. The
/// app already links onnxruntime statically through `vad-rs`/`ort`;
/// `sherpa-onnx` statically links its own vendored copy, and its build script
/// DOWNLOADS a prebuilt archive when `SHERPA_ONNX_LIB_DIR` is unset — a
/// build-time fetch, which is a category this file did not previously cover
/// (plan §2.4's own supply-chain note). So the scaffold ships with `serde` and
/// `serde_json` and nothing else, and this test is what makes YV122 adding the
/// crate a deliberate edit to two files rather than a silent one to a manifest.
#[test]
fn diarize_sidecar_is_bundled_and_carries_no_inference_crate_yet() {
    let config: serde_json::Value =
        serde_json::from_str(TAURI_CONF).expect("tauri.conf.json is valid JSON");
    let external: Vec<&str> = config["bundle"]["externalBin"]
        .as_array()
        .expect("bundle.externalBin lists the sidecars")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    for sidecar in ["binaries/yap-polish", "binaries/yap-diarize"] {
        assert!(
            external.contains(&sidecar),
            "{sidecar} must be bundled as a sidecar, got {external:?}"
        );
    }

    // The dependency list, read as a list rather than searched for a name — a
    // grep for "sherpa" would pass a manifest that grew `ort` instead.
    let deps: Vec<String> = DIARIZE_CARGO_TOML
        .lines()
        .map(str::trim)
        .skip_while(|l| *l != "[dependencies]")
        .skip(1)
        .take_while(|l| !l.starts_with('['))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split(['=', ' ']).next())
        .map(str::to_string)
        .collect();
    assert_eq!(
        deps,
        vec!["serde".to_string(), "serde_json".to_string()],
        "YV121's sidecar is the SHAPE, proven with zero model bytes and zero \
         onnxruntime. Adding an inference crate is YV122's job, and YV122 \
         updates this assertion in the same commit."
    );

    // And no build-time fetch has crept in through a build script.
    assert!(
        !DIARIZE_CARGO_TOML.contains("[build-dependencies]"),
        "the diarization sidecar has no build script, and therefore nothing that \
         downloads at build time"
    );
}

/// The exact `=x.y.z` pin a manifest line carries, or a panic naming the crate.
fn exact_pin(manifest: &str, crate_name: &str) -> String {
    let line = manifest
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with(&format!("{crate_name} = ")))
        .unwrap_or_else(|| panic!("{crate_name} is not declared in Cargo.toml"));
    let version = line
        .split('"')
        .nth(1)
        .unwrap_or_else(|| panic!("{crate_name} has no version string: {line}"));
    let exact = version
        .strip_prefix('=')
        .unwrap_or_else(|| panic!("{crate_name} must be pinned with `=`, got `{version}`"));
    let parts: Vec<&str> = exact.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "{crate_name}: expected x.y.z, got `{exact}`"
    );
    assert!(
        parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
        "{crate_name}: expected x.y.z, got `{exact}`"
    );
    exact.to_string()
}

/// The version `Cargo.lock` resolved a crate to.
fn locked_version(crate_name: &str) -> String {
    let needle = format!("name = \"{crate_name}\"");
    let mut lines = CARGO_LOCK.lines().map(str::trim);
    while let Some(line) = lines.next() {
        if line == needle {
            let version = lines
                .next()
                .unwrap_or_else(|| panic!("{crate_name} has no version line in Cargo.lock"));
            return version
                .strip_prefix("version = \"")
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or_else(|| panic!("unparsable Cargo.lock version line: {version}"))
                .to_string();
        }
    }
    panic!("{crate_name} is not in Cargo.lock — the aggregate/tap FFI cannot link without it");
}

/// YV100 (plan finding OS-6). `objc2-core-audio` is what stands between Yap and
/// hand-rolled `AudioHardwareCreateProcessTap` FFI, and it is the crate that
/// carries the macOS **14.4** symbol set YV101 weak-links. Two things have to be
/// true, and the finding is explicit that asserting only the first makes the
/// pin decorative:
///
/// 1. **The pin is exact.** A caret range lets `cargo update` move the symbol
///    set under a signed, notarized bundle with no reviewable diff.
/// 2. **The feature set is intact.** The crate ships 13 features with 12
///    default-on, and `AudioHardwareCreateProcessTap` is gated on
///    `AudioHardware` + `objc2` while `AudioDeviceCreateIOProcIDWithBlock` is
///    gated on `block2` + `dispatch2` + `objc2-core-audio-types`. A
///    `default-features = false` anywhere in the graph deletes those symbols,
///    and the failure surfaces as a compile error in a file nobody edited.
///
/// The block/queue/buffer crates are the finding's correction #1: they arrive
/// transitively through the default features and land in `Cargo.lock` on their
/// own. `syscapture.rs` names them directly (Rust cannot name a crate that is
/// not declared), so they are pinned here too — and this test asserts the
/// LOCKFILE agrees with those pins, which is the thing that decides what links.
#[test]
fn syscapture_pins_objc2_core_audio_exactly_and_asserts_its_feature_set() {
    let core_audio = exact_pin(CARGO_TOML, "objc2-core-audio");
    assert_eq!(
        core_audio, "0.3.2",
        "the tap was verified symbol-by-symbol against 0.3.2; re-verify before moving it"
    );
    assert_eq!(locked_version("objc2-core-audio"), core_audio);

    // The feature set. Not one dependency in this manifest may turn the default
    // features of these crates off — that is the switch that deletes the tap.
    for line in CARGO_TOML.lines().map(str::trim) {
        for crate_name in [
            "objc2-core-audio",
            "objc2-core-audio-types",
            "block2",
            "dispatch2",
        ] {
            if line.starts_with(&format!("{crate_name} = ")) {
                assert!(
                    !line.contains("default-features = false"),
                    "{crate_name} must keep its default features — `AudioHardware`, `objc2`, \
                     `block2`, `dispatch2` and `objc2-core-audio-types` are all default-on and \
                     all load-bearing: {line}"
                );
                assert!(
                    !line.contains("git = ") && !line.contains("path = "),
                    "{crate_name} must come from crates.io: {line}"
                );
            }
        }
    }

    // Correction #1: the block, the queue and the buffer/timestamp types the
    // IOProc registration needs, at versions inside the ranges
    // `objc2-core-audio` 0.3.2 itself declares
    // (block2 ">=0.6.1, <0.8.0", dispatch2 ">=0.3.0, <0.5.0",
    // objc2-core-audio-types "0.3.2").
    let block2 = locked_version("block2");
    assert_eq!(exact_pin(CARGO_TOML, "block2"), block2);
    let (major, minor) = split_major_minor(&block2);
    assert!(
        major == 0 && (6..8).contains(&minor),
        "block2 {block2} is outside the >=0.6.1, <0.8.0 range objc2-core-audio 0.3.2 declares"
    );

    let dispatch2 = locked_version("dispatch2");
    assert_eq!(exact_pin(CARGO_TOML, "dispatch2"), dispatch2);
    let (major, minor) = split_major_minor(&dispatch2);
    assert!(
        major == 0 && (3..5).contains(&minor),
        "dispatch2 {dispatch2} is outside the >=0.3.0, <0.5.0 range objc2-core-audio 0.3.2 declares"
    );

    let types = locked_version("objc2-core-audio-types");
    assert_eq!(exact_pin(CARGO_TOML, "objc2-core-audio-types"), types);
    assert_eq!(
        types, "0.3.2",
        "objc2-core-audio 0.3.2 declares objc2-core-audio-types 0.3.2"
    );
}

fn split_major_minor(version: &str) -> (u32, u32) {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    (major, minor)
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

/// THE TWO COPIES OF THE GATED-SYMBOL LIST MUST BE ONE LIST.
///
/// `scripts/assert-weak-linked-14_4-symbols.sh` (YV101) is what actually stands
/// over the release binary in CI, and `imp::AVAILABILITY_GATED_SYMBOLS` (YV100)
/// is what `dlsym` resolves at runtime and refuses on. They are the same four
/// names written twice, in two languages, and neither one can see the other.
///
/// Drift between them is silent in the worst possible direction. Add a fifth
/// availability-gated entry point to the Rust side and forget the script, and CI
/// keeps printing `PASS` while the new symbol goes into the binary as a hard
/// import — which is not a broken feature but a **launch failure of the whole
/// app** on every macOS that lacks it. The check would be green, and Yap would
/// not open.
///
/// This test existed on this branch as `tests/syscapture_no_hard_link.rs`,
/// against this branch's own copy of the script. YV101's script superseded that
/// copy and the file went with it; the assertion is the part that was worth
/// keeping, so it lives here — next to the other "the build must not quietly
/// change under us" checks — rather than being lost to a clean rebase.
#[test]
fn the_weak_link_ci_script_and_the_dlsym_list_are_the_same_four_symbols() {
    let script_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("repo root")
        .join("scripts/assert-weak-linked-14_4-symbols.sh");
    let script = std::fs::read_to_string(&script_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", script_path.display()));

    // The script names them with the Mach-O leading underscore; the Rust
    // constant carries the C names. Compare as sets of C names.
    let mut from_script: Vec<String> = script
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("_AudioHardware"))
        .map(|l| l.trim_start_matches('_').to_string())
        .collect();
    from_script.sort();
    from_script.dedup();

    let mut from_rust: Vec<String> = wilson_voice_lib::syscapture::imp::AVAILABILITY_GATED_SYMBOLS
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    from_rust.sort();

    assert_eq!(
        from_script, from_rust,
        "the CI weak-link script and `imp::AVAILABILITY_GATED_SYMBOLS` name different \
         symbols. Whichever one gained a name, the other must gain it too — a gated \
         symbol the script does not check can ship as a hard import with CI green, and \
         a hard import of a symbol the running macOS lacks is a dyld launch failure for \
         the entire app.\nscript: {from_script:?}\nrust:   {from_rust:?}"
    );

    // Non-vacuous in both directions: an empty parse would make the assertion
    // above pass against an empty Rust list, and a script that stopped naming
    // them at all is exactly the regression this guards.
    assert_eq!(
        from_rust.len(),
        4,
        "four availability-gated entry points are expected: {from_rust:?}"
    );
    assert!(
        script.contains("PASS")
            && script.contains("_AudioHardwareCreateProcessTap")
            && script.contains("_AudioHardwareDestroyAggregateDevice"),
        "the script no longer reads like the weak-link checker this test is pinned to"
    );
}
