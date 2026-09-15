//! UPD-A — the updater manifest has to live where the DMG lives, and it has to
//! keep the key that verifies it.
//!
//! The history this test exists to stop repeating is in the repo's own log:
//! `2eabf33` ("site: serve the DMG from Forge — repo went private, GitHub
//! release assets are no longer publicly downloadable") and then `734aa8c`
//! ("YV83 site + DMG served from Vercel, past Forge's 25MB edge cap"). The DMG
//! moved hosts twice; `plugins.updater.endpoints` moved neither time, so the
//! only configured endpoint was a GitHub release asset whose reachability is a
//! function of repo VISIBILITY — and `docs/loop/HARNESS.md` records flipping
//! this repo public/private as a recurring, deliberate procedure. An updater
//! that works only while the repo happens to be public is not an updater.
//!
//! So: the primary endpoint must be the host that serves the DMG
//! (`docs/DEPLOY-SITE.md` — Vercel project `yap`), and the GitHub URL stays on
//! as a SECOND endpoint, because Tauri tries endpoints in order and a
//! private-repo window should degrade to the primary rather than fail.
//!
//! The pubkey assertions are the other half. `plugins.updater.pubkey` is a
//! base64 minisign PUBLIC key — regenerating the keypair makes every installed
//! copy unable to verify any future update, and this repo has already paid for
//! one regeneration (YV82). Nothing here prints the value; a test that echoed
//! key material into CI output would be its own defect.
//!
//! Plain `cargo test` target on purpose: no network, no keychain, no bundle.

use std::path::{Path, PathBuf};

fn config_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json")
}

/// Parse the shipped config, or fail loudly. Every other test in this file
/// depends on this succeeding, which is itself the "the config parses"
/// assertion: a `tauri.conf.json` with a trailing comma or a stray quote builds
/// nothing, and finding that out here beats finding it out in `tauri build`.
fn config() -> serde_json::Value {
    let raw = std::fs::read_to_string(config_path())
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", config_path().display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", config_path().display()))
}

fn endpoints() -> Vec<String> {
    config()["plugins"]["updater"]["endpoints"]
        .as_array()
        .expect("plugins.updater.endpoints must be an array")
        .iter()
        .map(|v| {
            v.as_str()
                .expect("every updater endpoint must be a string")
                .to_string()
        })
        .collect()
}

/// A GitHub *release asset* URL — the shape whose reachability depends on repo
/// visibility. Deliberately narrower than "contains github.com": a link to the
/// repo's docs or its issue tracker is not an updater endpoint problem, and a
/// pattern that fired on any github.com URL would be proving the wrong thing.
fn is_github_release_asset(url: &str) -> bool {
    url.contains("github.com") && url.contains("/releases/")
}

#[test]
fn the_config_parses_and_declares_an_updater() {
    let c = config();
    assert!(
        c["plugins"]["updater"].is_object(),
        "plugins.updater is missing from tauri.conf.json"
    );
}

#[test]
fn at_least_two_endpoints() {
    let e = endpoints();
    assert!(
        e.len() >= 2,
        "expected a primary endpoint plus at least one fallback, found {}",
        e.len()
    );
}

#[test]
fn primary_endpoint_is_not_a_github_release_asset() {
    let e = endpoints();
    let primary = e.first().expect("no updater endpoints configured");
    assert!(
        !is_github_release_asset(primary),
        "the FIRST endpoint is a GitHub release asset ({primary}) — it stops \
         resolving the moment this repo is flipped private, which is a routine \
         procedure here. Put the host that serves the DMG first."
    );
    // The GitHub URL is not banned — it is the intended fallback. It just may
    // not be the one the updater depends on.
    assert!(
        e.iter().any(|u| is_github_release_asset(u)),
        "the GitHub release fallback was removed; keep it after the primary"
    );
}

#[test]
fn pubkey_is_present_and_not_a_placeholder() {
    let c = config();
    let key = c["plugins"]["updater"]["pubkey"]
        .as_str()
        .expect("plugins.updater.pubkey must be a string");
    let trimmed = key.trim();
    // No assertion in this test prints `key`.
    assert!(!trimmed.is_empty(), "plugins.updater.pubkey is empty");
    assert_ne!(
        trimmed, "PLACEHOLDER",
        "plugins.updater.pubkey is a placeholder"
    );
    assert!(
        trimmed.len() > 40,
        "plugins.updater.pubkey is {} chars — too short to be a minisign public key",
        trimmed.len()
    );
    assert!(
        !trimmed.contains("SECRET") && !trimmed.contains("secret key"),
        "plugins.updater.pubkey looks like a PRIVATE key — it must be the public half"
    );
}

/// Without `createUpdaterArtifacts` the bundler emits no `.app.tar.gz` and no
/// `.sig`, so every endpoint above would serve a manifest pointing at bytes
/// that were never produced.
#[test]
fn bundle_still_creates_updater_artifacts() {
    let c = config();
    assert_eq!(
        c["bundle"]["createUpdaterArtifacts"],
        serde_json::Value::Bool(true),
        "bundle.createUpdaterArtifacts must stay true or no update is ever built"
    );
}
