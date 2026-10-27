//! Y6-E — the repo docs are a spec source, so the build keeps them honest.
//!
//! `docs/loop/HARNESS.md` names `PRODUCT.md`, `ROADMAP.md`, `ARCHITECTURE.md`
//! and `docs/` as the spec every loop agent reads. When this file landed,
//! ROADMAP's "what works today" table said the ASR was MLX Whisper, the hotkey
//! was a Carbon ⌘⇧V hold and the version was 0.4.1 — all three wrong since
//! YV34 — and README's install steps still built a Python venv. A wrong doc is
//! a wrong instruction that propagates, so each claim a doc makes about the
//! app is asserted against the app itself:
//!
//! * [`version_in_docs_matches_tauri_conf`] — ARCHITECTURE.md's app version and
//!   PRODUCT.md's feature-list heading equal `tauri.conf.json`'s `version`
//!   (which `package.json` and the crate version must also equal).
//! * [`default_binding_in_readme_matches_app_settings`] — the default dictation
//!   key README names is `AppSettings::default().ptt_binding` and its label.
//! * [`no_doc_mentions_a_deleted_subsystem`] — no spec doc (and no
//!   user-visible bundle description) names MLX, the Python ASR sidecar, the
//!   ffmpeg capture path or ⌘⇧V as anything but an opt-in.
//! * [`the_scanner_catches_the_shapes_it_was_written_for`] — the scanner above
//!   is proven on the exact stale lines it replaced, so an absence it reports
//!   is an absence of a pattern that CAN match.
//! * [`platform_floor_in_docs_matches_config_and_budgets`] — ARCHITECTURE.md's
//!   declared floor machine is `minimumSystemVersion` and the RAM the Y3-G
//!   budget (`tests/long_take_budget.rs::FLOOR_MACHINE_BYTES`) is set against.
//! * [`runtime_dependencies_are_shipped_or_managed`] — every runtime input is
//!   SHIPPED, MANAGED, CALLER-supplied or USER-granted; none is "assumed present".
//!
//! When a release bumps the version (docs/RELEASE.md step 1), ARCHITECTURE.md's
//! `**App version:**` line and PRODUCT.md's heading move with it, or this fails.

use std::fs;
use std::path::{Path, PathBuf};

use wilson_voice_lib::AppSettings;

/// The four top-level docs the harness reads as the spec.
const SPEC_DOCS: [&str; 4] = ["README.md", "ROADMAP.md", "ARCHITECTURE.md", "PRODUCT.md"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_root(rel: &str) -> String {
    let p = repo_root().join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn tauri_conf() -> serde_json::Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    serde_json::from_str(&fs::read_to_string(&p).expect("read tauri.conf.json"))
        .expect("tauri.conf.json parses")
}

/// The first back-ticked token on the (single) line that carries `key`.
/// Docs write machine-checked facts as `**Key:** \`value\``.
fn marker(doc_name: &str, doc: &str, key: &str) -> String {
    let lines: Vec<&str> = doc.lines().filter(|l| l.contains(key)).collect();
    assert_eq!(
        lines.len(),
        1,
        "{doc_name} must carry exactly one `{key}` line, found {}: {lines:?}",
        lines.len()
    );
    let after = &lines[0][lines[0].find(key).unwrap() + key.len()..];
    let start = after
        .find('`')
        .unwrap_or_else(|| panic!("{doc_name}: `{key}` line has no back-ticked value"));
    let rest = &after[start + 1..];
    let end = rest.find('`').expect("unterminated back-tick");
    rest[..end].to_string()
}

#[test]
fn version_in_docs_matches_tauri_conf() {
    let conf = tauri_conf();
    let version = conf["version"].as_str().expect("tauri.conf.json version");

    // The three places a release bumps must agree before the docs can.
    let pkg: serde_json::Value =
        serde_json::from_str(&read_root("desktop/package.json")).expect("package.json parses");
    assert_eq!(
        pkg["version"].as_str(),
        Some(version),
        "package.json vs tauri.conf.json"
    );
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        version,
        "src-tauri/Cargo.toml vs tauri.conf.json"
    );

    let arch = read_root("ARCHITECTURE.md");
    assert_eq!(
        marker("ARCHITECTURE.md", &arch, "**App version:**"),
        version,
        "ARCHITECTURE.md's **App version:** line is stale"
    );

    let product = read_root("PRODUCT.md");
    let heading = format!("## What Yap does at {version}");
    assert!(
        product.lines().any(|l| l.trim() == heading),
        "PRODUCT.md must carry the heading `{heading}` — its feature list is dated by the version it describes"
    );
}

#[test]
fn default_binding_in_readme_matches_app_settings() {
    let defaults = AppSettings::default();
    let readme = read_root("README.md");
    let key = "**Default dictation key:**";
    let line = readme
        .lines()
        .find(|l| l.contains(key))
        .unwrap_or_else(|| panic!("README.md must carry a `{key}` line"));

    let binding = format!("`ptt_binding = \"{}\"`", defaults.ptt_binding);
    assert!(
        line.contains(&binding),
        "README's default key line must name {binding} (AppSettings::default()), got: {line}"
    );
    let label = format!("**{}**", defaults.hotkey_label);
    assert!(
        line.contains(&label),
        "README's default key line must show the label {label} the app shows, got: {line}"
    );
    // The opt-in Carbon chord is off on a fresh install; README must not say otherwise.
    assert!(
        !defaults.keep_cmd_shift_v,
        "⌘⇧V became a default — update README and this test"
    );
}

/// Why a line is stale, or `None`. Lower-cased matching throughout, so a
/// re-capitalised claim cannot slip past.
fn stale_reason(line: &str) -> Option<&'static str> {
    let l = line.to_lowercase();
    if l.contains("mlx") {
        return Some("MLX — the Python/MLX ASR path was deleted by YV34");
    }
    for needle in [
        "asr_worker",
        "python sidecar",
        "python asr",
        ".venv",
        "venv ",
        "pip install",
        "python3",
    ] {
        if l.contains(needle) {
            return Some("the Python ASR sidecar / venv — deleted by YV34");
        }
    }
    if l.contains("ffmpeg") && !l.contains("no `ffmpeg`") && !l.contains("no ffmpeg") {
        return Some("the ffmpeg capture path — capture is in-process cpal");
    }
    if l.contains("⌥space") {
        return Some("⌥Space — not a dictation binding in this app");
    }
    if line.contains("⌘⇧V")
        && !["optional", "opt-in", "keep_cmd_shift_v", "registers no"]
            .iter()
            .any(|ok| l.contains(ok))
    {
        return Some("⌘⇧V named without saying it is opt-in — the default is fn⌃");
    }
    None
}

fn stale_lines(name: &str, text: &str) -> Vec<String> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| stale_reason(l).map(|why| format!("{name}:{}: {why}\n    {l}", i + 1)))
        .collect()
}

#[test]
fn no_doc_mentions_a_deleted_subsystem() {
    let mut hits = Vec::new();
    for doc in SPEC_DOCS {
        hits.extend(stale_lines(doc, &read_root(doc)));
    }
    // The bundle's own descriptions are user-visible copy, not just a doc.
    let conf = tauri_conf();
    for field in ["shortDescription", "longDescription"] {
        let text = conf["bundle"][field].as_str().unwrap_or_default();
        hits.extend(stale_lines(
            &format!("tauri.conf.json bundle.{field}"),
            text,
        ));
    }
    let roadmap = read_root("ROADMAP.md");
    if roadmap.contains("v0.4.1") {
        hits.push("ROADMAP.md: still dated v0.4.1".into());
    }
    assert!(
        hits.is_empty(),
        "docs describe a deleted subsystem:\n{}",
        hits.join("\n")
    );
}

#[test]
fn the_scanner_catches_the_shapes_it_was_written_for() {
    // Verbatim lines this item replaced (ROADMAP/README/PRODUCT at 0c5a0f3).
    let stale = [
        "| Hotkey | Carbon `⌘⇧V` hold (Tauri global-shortcut) |",
        "| ASR | MLX Whisper large-v3-turbo on Apple Silicon |",
        "- **Local ASR:** MLX Whisper (`large-v3-turbo`) via Python sidecar — audio never leaves your Mac",
        "cd .. && python3.13 -m venv .venv && .venv/bin/pip install mlx-whisper",
        "| Dictate | Hold **⌥Space** or **⌘⇧V**, or click floating **Dictate** / tray |",
        "→ python asr_worker (mlx_whisper large-v3-turbo)",
        "⌥Space hold → ffmpeg avfoundation mic → WAV",
        "Requires arm64 venv at `~/Desktop/wilson-voice/.venv` with `mlx-whisper`.",
        "**Honest stack language:** OpenAI Whisper weights via MLX on-device",
    ];
    for l in stale {
        assert!(stale_reason(l).is_some(), "scanner must flag: {l}");
    }
    // Lines the corrected docs legitimately carry.
    let fine = [
        "audio is decoded in-process (no `ffmpeg` binary)",
        "| Secondary hotkey | Optional ⌘⇧V Carbon + optional fn⌃ | Settings toggles |",
        "A launch an automated agent may make. It registers NO global hotkey (no ⌃⌘V / ⌘⇧V).",
        "**Default dictation key:** hold **fn⌃** (`ptt_binding = \"fn_control\"`).",
    ];
    for l in fine {
        assert_eq!(stale_reason(l), None, "scanner must not flag: {l}");
    }
}

#[test]
fn platform_floor_in_docs_matches_config_and_budgets() {
    let arch = read_root("ARCHITECTURE.md");
    let conf = tauri_conf();
    let min_os = conf["bundle"]["macOS"]["minimumSystemVersion"]
        .as_str()
        .expect("bundle.macOS.minimumSystemVersion");
    assert_eq!(
        marker("ARCHITECTURE.md", &arch, "**Floor macOS (dictation):**"),
        min_os,
        "ARCHITECTURE.md's floor macOS must be what Info.plist invites"
    );

    // Y3-G's memory budget is asserted against this machine; parse its constant.
    let budget =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/long_take_budget.rs"))
            .expect("read long_take_budget.rs");
    let decl = budget
        .lines()
        .find(|l| l.starts_with("const FLOOR_MACHINE_BYTES: u64 ="))
        .expect("long_take_budget.rs declares FLOOR_MACHINE_BYTES");
    let expr = decl.split('=').nth(1).unwrap().trim().trim_end_matches(';');
    let bytes: u64 = expr
        .split('*')
        .map(|t| {
            t.trim()
                .replace('_', "")
                .parse::<u64>()
                .expect("integer factor")
        })
        .product();

    let ram = marker("ARCHITECTURE.md", &arch, "**Floor RAM:**");
    let gb: u64 = ram
        .strip_suffix(" GB")
        .unwrap_or_else(|| panic!("**Floor RAM:** must read `<n> GB`, got `{ram}`"))
        .parse()
        .expect("integer GB");
    assert_eq!(
        gb * 1024 * 1024 * 1024,
        bytes,
        "ARCHITECTURE.md floor RAM vs FLOOR_MACHINE_BYTES"
    );

    let chip = marker("ARCHITECTURE.md", &arch, "**Floor chip:**");
    assert!(
        chip.contains("M1"),
        "floor chip must be the Apple-silicon M1 the budgets name, got `{chip}`"
    );
}

#[test]
fn runtime_dependencies_are_shipped_or_managed() {
    let arch = read_root("ARCHITECTURE.md");
    let section = arch
        .split("## Runtime Dependencies")
        .nth(1)
        .expect("ARCHITECTURE.md has a Runtime Dependencies section");
    let section = section.split("\n## ").next().unwrap();
    let rows: Vec<&str> = section
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Dependency") && !l.starts_with("|---"))
        .collect();

    for required in [
        "ASR model",
        "Polish model",
        "Silero VAD model",
        "`yap-polish` sidecar",
        "`yap-diarize` sidecar",
        "sherpa-onnx prebuilt archive",
    ] {
        assert!(
            rows.iter().any(|r| r.contains(required)),
            "Runtime Dependencies must carry a row for {required}"
        );
    }
    for r in &rows {
        let how = r.split('|').nth(2).unwrap_or_default();
        assert!(
            ["SHIPPED", "MANAGED", "CALLER", "USER", "BUILD"]
                .iter()
                .any(|k| how.contains(k)),
            "every runtime dependency is SHIPPED, MANAGED, BUILD-time, CALLER- or USER-supplied: {r}"
        );
        assert!(
            !r.to_lowercase().contains("assumed"),
            "nothing is assumed present: {r}"
        );
    }
    for hatch in ["SHERPA_ONNX_ARCHIVE_DIR", "SHERPA_ONNX_LIB_DIR"] {
        assert!(
            section.contains(hatch),
            "the sherpa escape hatch {hatch} is documented"
        );
    }
}
