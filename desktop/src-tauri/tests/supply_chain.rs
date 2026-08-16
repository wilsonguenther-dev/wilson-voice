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

/// YV122: `yap-diarize` is bundled, and both halves of `sherpa-onnx` are pinned
/// exactly — including the `-sys` crate, which pinning `sherpa-onnx` does not
/// pin.
///
/// The bundling half is the same failure `yap-polish` would have had: an app
/// that ships a diarization path it can never spawn.
///
/// The pinning half replaces YV121's "carries no inference crate yet", which
/// was the guard that made THIS commit a deliberate edit to two files rather
/// than a silent one to a manifest. `sherpa-onnx` statically links its own
/// vendored onnxruntime beside the one `vad-rs`/`ort` already links into the
/// app, and — unlike every other dependency in this tree — its `-sys` crate
/// DOWNLOADS a prebuilt native archive at build time when `SHERPA_ONNX_LIB_DIR`
/// is unset (plan §2.4's supply-chain note; YV123 vendors it).
///
/// That download is why the second pin is load-bearing rather than tidy.
/// `sherpa-onnx`'s own manifest asks for `sherpa-onnx-sys = { version =
/// "1.13.4" }`, a CARET range, and the sys crate's `build.rs` builds its release
/// URL out of its OWN `CARGO_PKG_VERSION`. So an unpinned sys crate silently
/// changes which prebuilt onnxruntime is fetched into a signed bundle, one
/// level below where the `=x.y.z` discipline was looking. (It also does not
/// compile: sys 1.13.5 added a field to a struct `sherpa-onnx` 1.13.4
/// initialises exhaustively.)
#[test]
fn diarize_sidecar_pins_sherpa_onnx_exactly() {
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
    // grep for "sherpa" would pass a manifest that ALSO grew `ort`, which is
    // the one crate that must never be in this link unit twice.
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
        vec![
            "serde".to_string(),
            "serde_json".to_string(),
            "sherpa-onnx".to_string(),
            "sherpa-onnx-sys".to_string(),
        ],
        "the diarization sidecar's whole dependency surface, and no second \
         inference engine"
    );

    // Neither half may come from a git source or a local path — CI builds with
    // a plain `cargo build`, so a moved branch head would enter a notarized
    // bundle with no reviewable diff.
    for line in DIARIZE_CARGO_TOML
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("sherpa-onnx"))
    {
        assert!(
            !line.contains("git = ") && !line.contains("path = "),
            "sherpa-onnx must come from crates.io: {line}"
        );
    }

    // Both exact, and both the SAME version — a matched pair is the only
    // configuration the wrapper's exhaustive struct initialisers compile
    // against, and the only one whose native archive URL is predictable.
    let wrapper = exact_pin(DIARIZE_CARGO_TOML, "sherpa-onnx");
    let sys = exact_pin(DIARIZE_CARGO_TOML, "sherpa-onnx-sys");
    assert_eq!(
        wrapper, sys,
        "sherpa-onnx and sherpa-onnx-sys must be pinned to one version; \
         the sys crate's build.rs downloads \
         `sherpa-onnx-v{{sys_version}}-<target>-static-lib.tar.bz2`, so a skew \
         here silently swaps the linked onnxruntime"
    );

    // The manifest states the range; the lockfile decides what links. Assert
    // the resolved graph, or the pin above is a comment.
    for crate_name in ["sherpa-onnx", "sherpa-onnx-sys"] {
        assert_eq!(
            locked_version(crate_name),
            wrapper,
            "Cargo.lock resolved {crate_name} away from the pin"
        );
    }

    // The link mode is decided in exactly one place. Naming `static` (or
    // `shared`) a second time here is how both end up set, which the sys build
    // script rejects outright — and a silent flip to `shared` would put a
    // `.dylib` beside a notarized binary that never bundles it.
    let sys_line = DIARIZE_CARGO_TOML
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("sherpa-onnx-sys = "))
        .expect("the sys pin is declared");
    assert!(
        sys_line.contains("default-features = false") && !sys_line.contains("features = ["),
        "sherpa-onnx-sys' link mode comes from sherpa-onnx's own `default = \
         [\"static\"]`, never from a second feature list: {sys_line}"
    );

    // The sidecar itself still has no build script of its own.
    assert!(
        !DIARIZE_CARGO_TOML.contains("[build-dependencies]"),
        "the diarization sidecar has no build script of its own"
    );

    // And the app is still the OTHER link unit. `vad-rs`/`ort` already put one
    // statically-linked onnxruntime in `wilson-voice`; sherpa-onnx brings a
    // second. CI proves they never meet by building both, but a manifest line
    // is where it would happen, so it is asserted here too.
    //
    // Asserted over the manifest with COMMENTS STRIPPED, and that is a
    // correction rather than a refinement. The first version of this line was
    // `!CARGO_TOML.contains("sherpa-onnx")`, and it went red on this branch for
    // a reason that has nothing to do with linking: YV123 landed a comment in
    // `src-tauri/Cargo.toml` explaining that the segmentation model ships as
    // `sherpa-onnx-pyannote-segmentation-3-0.tar.bz2`. A guard that a *comment*
    // can trip is a guard somebody deletes the day it cries wolf — and the same
    // substring check would have been satisfied by commenting a real dependency
    // out and re-adding it under `[target.'cfg(...)'.dependencies]`. The
    // property is about the resolved manifest, so the check is too.
    let app_lines: Vec<&str> = CARGO_TOML
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && l.contains("sherpa"))
        .collect();
    assert!(
        app_lines.is_empty(),
        "sherpa-onnx belongs to the sidecar's link unit — moving it into \
         src-tauri is the duplicate-symbol failure that forced yap-polish out \
         of process: {app_lines:?}"
    );
    assert_eq!(
        sherpa_onnx_pin_verdict(CARGO_TOML),
        SherpaPin::Absent,
        "the app declares no sherpa-onnx dependency"
    );
    // Non-vacuity, both ways: the check must see a real declaration, and must
    // NOT see the comment that broke its first version.
    assert!(
        !"[dependencies]\nsherpa-onnx = \"=1.13.4\"\n"
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#') && l.contains("sherpa"))
            .collect::<Vec<_>>()
            .is_empty(),
        "the scan must still catch a dependency that really is declared"
    );
    assert!(
        CARGO_TOML.contains("sherpa-onnx-pyannote-segmentation-3-0"),
        "the YV123 comment this check had to learn to ignore is still there — \
         without it, the correction above is untested"
    );
}

/// YV123 — the vendoring half of Wilson's closed O6 decision, guarded.
///
/// O6 closed as: *"VENDOR the diarization models — Wilson-owned HF mirror,
/// pinned revisions + sha256, archive-extraction path in models.rs,
/// supply-chain tests extended. No direct vendor-release downloads at
/// runtime."* Four things have to stay true for that to mean anything, and each
/// one fails silently on its own:
///
/// **(a) the inference crate is exactly pinned and fetches nothing at build
/// time.** `sherpa-onnx`'s build script downloads a prebuilt archive when
/// `SHERPA_ONNX_LIB_DIR` is unset — a build-time network fetch, the category
/// plan finding #23 flagged as invisible to every compile-time manifest check
/// in this file. See the note on `sherpa_onnx_pin_verdict` for why this half is
/// written as a rule with its own falsification test rather than as a literal
/// assertion today.
///
/// **(b) the vendored archive's sha256 is a compiled-in constant.** A re-vendor
/// that updates `catalog.json` and not this file (or the reverse) fails here,
/// loudly, instead of shipping stale bytes past a hash nobody re-read.
///
/// **(c) `binaries/yap-diarize` is bundled** — true since YV121, re-asserted
/// now that there is a model for that sidecar to load.
///
/// **(d) the bzip2 decoder is the pure-Rust one, in the RESOLVED graph.** The
/// manifest saying `bzip2-rs` proves nothing if a feature flag or a transitive
/// edge pulled `bzip2-sys`/libbz2 in anyway — and a second native C library in
/// this binary is the exact hazard that forced both sidecars out of process.
#[test]
fn diarize_sidecar_vendors_sherpa_onnx_and_pins_the_archive() {
    // (a) -------------------------------------------------------------------
    match sherpa_onnx_pin_verdict(DIARIZE_CARGO_TOML) {
        SherpaPin::Absent => {
            // YV122 is the item that adds the crate. Until it lands, the
            // ASSERTION that has to hold is the one YV121 already makes (the
            // manifest carries serde + serde_json and nothing else) — and the
            // rule below is proven against fixtures instead of against a line
            // that is not there yet, so it is not a check that silently passes
            // on an empty file.
            // Comments in that manifest discuss sherpa-onnx at length; a
            // DEPENDENCY on it is what would have to be recognised as a pin.
            let declared: Vec<&str> = DIARIZE_CARGO_TOML
                .lines()
                .map(str::trim)
                .filter(|l| !l.starts_with('#') && l.contains("sherpa"))
                .collect();
            assert!(
                declared.is_empty(),
                "a sherpa-onnx dependency exists but was not recognised as a pin: {declared:?}"
            );
        }
        SherpaPin::Exact(version) => assert_eq!(
            version, "1.13.4",
            "sherpa-onnx was verified API-by-API against 1.13.4; re-verify before moving it"
        ),
        SherpaPin::Rejected(why) => panic!("sherpa-onnx pin is not acceptable: {why}"),
    }
    // Whatever the sidecar depends on, it must not fetch it at build time.
    assert!(
        !DIARIZE_CARGO_TOML.contains("[build-dependencies]"),
        "the diarization sidecar has no build script, and therefore nothing that \
         downloads at build time. `sherpa-onnx` vendoring is done by exporting \
         SHERPA_ONNX_LIB_DIR in ci.yml/release.yml, not by letting its build \
         script reach the network."
    );

    // (b) -------------------------------------------------------------------
    // The bytes Wilson vendored, hashed at vendoring time and pasted here. The
    // catalog is the other copy; this test is what makes them one number.
    const SEGMENTATION_ARCHIVE_SHA256: &str =
        "24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488";
    const SEGMENTATION_MODEL_ONNX_SHA256: &str =
        "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079";
    const EMBEDDING_ONNX_SHA256: &str =
        "c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef";
    const MIRROR_REVISION: &str = "c0f5026b16bf2cac9b5f9e6e2a36da6c6a8628ec";

    let seg = wilson_voice_lib::models::diarize_model_for_role(
        wilson_voice_lib::models::DiarizeModelRole::Segmentation,
    )
    .expect("the segmentation model is in the catalog");
    let emb = wilson_voice_lib::models::diarize_model_for_role(
        wilson_voice_lib::models::DiarizeModelRole::Embedding,
    )
    .expect("the embedding model is in the catalog");
    assert_eq!(
        seg.file.sha256, SEGMENTATION_ARCHIVE_SHA256,
        "the vendored .tar.bz2 changed without this constant changing — re-hash \
         the mirrored asset and update BOTH, or the app installs bytes nobody re-verified"
    );
    assert_eq!(
        seg.archive
            .as_ref()
            .expect("the segmentation entry declares an archive")
            .extracted_sha256,
        SEGMENTATION_MODEL_ONNX_SHA256
    );
    assert_eq!(emb.file.sha256, EMBEDDING_ONNX_SHA256);
    for m in [seg, emb] {
        assert_eq!(m.revision, MIRROR_REVISION, "{}: unpinned revision", m.id);
        assert_eq!(
            m.repo, "wilsonguenther/yap-diarize-models",
            "{}: the models must come from the Wilson-owned mirror, never a vendor release",
            m.id
        );
    }

    // (c) -------------------------------------------------------------------
    let config: serde_json::Value =
        serde_json::from_str(TAURI_CONF).expect("tauri.conf.json is valid JSON");
    let external: Vec<&str> = config["bundle"]["externalBin"]
        .as_array()
        .expect("bundle.externalBin lists the sidecars")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(
        external.contains(&"binaries/yap-diarize"),
        "the models are useless without the sidecar that loads them, got {external:?}"
    );

    // (d) -------------------------------------------------------------------
    // Asserted over the closure of THIS app's package in the lockfile, not over
    // the whole file. That is a deliberate correction to the spec's own
    // acceptance criterion, which reads `grep -rn "bzip2-sys\|libbz2"
    // Cargo.lock` returns no match. That grep is true on this branch and stops
    // being true the moment YV122 (#137) merges — `sherpa-onnx-sys` declares
    // `bzip2 = "0.4"` as a BUILD-dependency to unpack its own prebuilt archive
    // on the build host, which puts `bzip2`/`bzip2-sys` in the shared
    // `desktop/Cargo.lock` while linking neither of them into anything that
    // ships. A whole-file grep would then fail for a reason that has nothing to
    // do with the property it is guarding, and the usual fix for a check that
    // cries wolf is to delete it.
    //
    // The property actually worth guarding is: nothing in the app binary — the
    // one that already statically links ggml and, through `vad-rs`/`ort`,
    // onnxruntime — decompresses through a native C library. So: walk the
    // lockfile graph from `wilson-voice` and assert over what it reaches.
    let reachable = lockfile_closure("wilson-voice");
    assert!(
        reachable.contains("bzip2-rs"),
        "the pure-Rust bzip2 decoder is gone from this app's resolved graph — either \
         the extraction path was deleted, or it now decodes through something else"
    );
    assert!(
        reachable.contains("tar"),
        "the tar reader is gone from this app's resolved graph"
    );
    for banned in ["bzip2-sys", "bzip2", "libbz2-rs-sys"] {
        assert!(
            !reachable.contains(banned),
            "`{banned}` is reachable from `wilson-voice` in Cargo.lock. The bzip2 decoder \
             must stay pure Rust: this binary already statically links ggml and (through \
             vad-rs/ort) onnxruntime, and a third native C library — for ONE 7 MB archive — \
             is the same double-link hazard that forced yap-polish and yap-diarize out of \
             process.\nreachable: {reachable:?}"
        );
    }
}

/// Every package reachable from `root` in `Cargo.lock`, by name.
///
/// The lockfile lists each package's resolved dependencies but not their KIND,
/// so this closure is "everything the resolver put under `root`", which is
/// exactly the right granularity for the question above: `sherpa-onnx-sys`'s
/// build-time `bzip2` sits under the SIDECAR's root (`yap-diarize`), a separate
/// link unit in a separate process, and never appears under this one.
fn lockfile_closure(root: &str) -> std::collections::BTreeSet<String> {
    // name -> its dependency names
    let mut graph: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut name: Option<String> = None;
    let mut in_deps = false;
    for line in CARGO_LOCK.lines().map(str::trim) {
        if line == "[[package]]" {
            name = None;
            in_deps = false;
        } else if let Some(rest) = line.strip_prefix("name = \"") {
            name = rest.strip_suffix('"').map(str::to_string);
        } else if line == "dependencies = [" {
            in_deps = true;
        } else if in_deps {
            if line == "]" {
                in_deps = false;
            } else if let Some(dep) = line
                .trim_matches(|c| c == '"' || c == ',')
                .split(' ')
                .next()
            {
                if let Some(owner) = &name {
                    graph
                        .entry(owner.clone())
                        .or_default()
                        .push(dep.to_string());
                }
            }
        }
    }
    assert!(
        graph.contains_key(root),
        "`{root}` is not a package in Cargo.lock — the closure walk would be vacuous"
    );

    let mut seen: std::collections::BTreeSet<String> = Default::default();
    let mut queue = vec![root.to_string()];
    while let Some(next) = queue.pop() {
        if !seen.insert(next.clone()) {
            continue;
        }
        for dep in graph.get(&next).into_iter().flatten() {
            queue.push(dep.clone());
        }
    }
    seen
}

/// The closure walk, falsified: a package the app really does depend on has to
/// be in it, and a workspace sibling that it does NOT depend on must not be.
/// Without this, "no bzip2-sys is reachable" could be a sentence about an empty
/// set — which is exactly how the whole-file grep it replaces would have failed.
#[test]
fn the_lockfile_closure_walk_is_not_an_empty_set() {
    let app = lockfile_closure("wilson-voice");
    for expected in ["tauri", "rusqlite", "reqwest", "sha2", "tar", "bzip2-rs"] {
        assert!(
            app.contains(expected),
            "{expected} must be reachable from wilson-voice"
        );
    }
    // The sidecars are separate link units: the app does not depend on them,
    // which is the entire reason a build-time `bzip2` under `yap-diarize` is
    // not a native C library in this binary.
    assert!(
        !app.contains("yap-polish"),
        "the app must not link the polish sidecar"
    );
    assert!(
        !app.contains("yap-diarize"),
        "the app must not link the diarize sidecar"
    );
    // And a name that is in no graph at all stays out.
    assert!(!app.contains("definitely-not-a-real-crate"));
}

/// What a manifest says about `sherpa-onnx`.
#[derive(Debug, PartialEq)]
enum SherpaPin {
    /// Not declared. YV122 is the item that adds it; see the test above.
    Absent,
    /// `sherpa-onnx = "=x.y.z"` from crates.io.
    Exact(String),
    /// Declared, but not in a form this supply chain accepts.
    Rejected(String),
}

/// The pin RULE, as a function, so it can be falsified against fixtures instead
/// of only against whatever the manifest happens to say today.
///
/// This matters because of ordering: YV122 (which adds the crate) is open as
/// PR #137 and is not in `main`, so a literal `assert!(manifest.contains(
/// "sherpa-onnx = \"=1.13.4\""))` written here would either fail this branch's
/// own CI or — if written as a `contains` guard — be a check that passes
/// because there is nothing to check. Encoding the rule and testing the rule is
/// the version that is true before AND after #137 merges: the moment the line
/// appears, this test starts asserting it, and `sherpa_onnx_pin_rule_rejects_
/// everything_but_an_exact_crates_io_pin` proves the rule is not a rubber stamp.
fn sherpa_onnx_pin_verdict(manifest: &str) -> SherpaPin {
    let Some(line) = manifest
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find(|l| l.starts_with("sherpa-onnx"))
    else {
        return SherpaPin::Absent;
    };
    if line.contains("git = ") || line.contains("path = ") {
        return SherpaPin::Rejected(format!("must come from crates.io: {line}"));
    }
    let Some(version) = line.split('"').nth(1) else {
        return SherpaPin::Rejected(format!("no version string: {line}"));
    };
    let Some(exact) = version.strip_prefix('=') else {
        return SherpaPin::Rejected(format!("must be pinned with `=`, got `{version}`"));
    };
    let parts: Vec<&str> = exact.split('.').collect();
    if parts.len() != 3
        || !parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return SherpaPin::Rejected(format!("expected an exact x.y.z pin, got `{exact}`"));
    }
    SherpaPin::Exact(exact.to_string())
}

/// The falsification half of the rule above. Without this, "the pin is
/// acceptable" would be a sentence with no test behind it until YV122 lands.
#[test]
fn sherpa_onnx_pin_rule_rejects_everything_but_an_exact_crates_io_pin() {
    assert_eq!(
        sherpa_onnx_pin_verdict("[dependencies]\nserde = \"1\"\n"),
        SherpaPin::Absent
    );
    assert_eq!(
        sherpa_onnx_pin_verdict("sherpa-onnx = \"=1.13.4\"\n"),
        SherpaPin::Exact("1.13.4".to_string())
    );
    assert_eq!(
        sherpa_onnx_pin_verdict(
            "sherpa-onnx = { version = \"=1.13.4\", features = [\"static\"] }\n"
        ),
        SherpaPin::Exact("1.13.4".to_string())
    );
    // A caret range lets `cargo update` move a statically linked onnxruntime
    // under a signed, notarized bundle with no reviewable diff.
    assert!(matches!(
        sherpa_onnx_pin_verdict("sherpa-onnx = \"1.13.4\"\n"),
        SherpaPin::Rejected(_)
    ));
    assert!(matches!(
        sherpa_onnx_pin_verdict("sherpa-onnx = \"=1.13\"\n"),
        SherpaPin::Rejected(_)
    ));
    // A git source is a moving target that `tests/supply_chain.rs`'s own
    // `git_dependencies_are_pinned_to_a_rev` cannot see from the app manifest.
    assert!(matches!(
        sherpa_onnx_pin_verdict(
            "sherpa-onnx = { git = \"https://github.com/k2-fsa/sherpa-onnx\", rev = \"abc\" }\n"
        ),
        SherpaPin::Rejected(_)
    ));
    // A commented-out line is not a dependency.
    assert_eq!(
        sherpa_onnx_pin_verdict("# sherpa-onnx = \"=1.13.4\"\n"),
        SherpaPin::Absent
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
