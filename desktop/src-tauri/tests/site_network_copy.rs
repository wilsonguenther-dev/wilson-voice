//! yap-1106-lic-06 — the marketing site and the privacy page tell the truth
//! about licensing network calls.
//!
//! Since 0.9.3 Yap makes no "background download of the public
//! refunded-licenses list". Licensing is one activation call to
//! drivia.consulting, a token refresh about once a month, and one call on
//! Remove from this Mac (see `license_client.rs`). These tests read
//! `site/dist/index.html` and `site/dist/privacy.html` from the repo and fail
//! if the stale claim comes back or the true one goes missing.

use std::path::PathBuf;

fn site_page(name: &str) -> String {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = crate_dir
        .parent()
        .and_then(|d| d.parent())
        .expect("src-tauri sits two levels under the repo root")
        .join("site/dist")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

/// Lower-case and collapse whitespace, so a line wrap cannot hide a phrase.
fn normalized(html: &str) -> String {
    html.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

const STALE: &[&str] = &[
    "refunded-licenses list",
    "refunded licenses list",
    "list of refunded licenses",
    "refund list",
    "background download of the public",
];

fn assert_no_stale_claim(name: &str) {
    let page = normalized(&site_page(name));
    // POSITIVE CONTROL — the page was really read, not an empty string.
    assert!(
        page.len() > 2_000 && page.contains("</html>"),
        "{name} looks truncated ({} bytes)",
        page.len()
    );
    for needle in STALE {
        assert!(
            !page.contains(needle),
            "{name} still claims Yap downloads a refunded-licenses list ({needle:?}); 0.9.3 makes no such call"
        );
    }
}

#[test]
fn index_drops_the_refunded_licenses_list_claim() {
    assert_no_stale_claim("index.html");
}

#[test]
fn privacy_drops_the_refunded_licenses_list_claim() {
    assert_no_stale_claim("privacy.html");
}

#[test]
fn index_names_drivia_consulting_activation_and_monthly_refresh() {
    let page = normalized(&site_page("index.html"));
    assert!(
        page.contains("one request to drivia.consulting"),
        "index.html must say activation is one request to drivia.consulting"
    );
    assert!(
        page.contains("one call to drivia.consulting when you activate it"),
        "index.html's complete list of network calls must name the drivia.consulting activation call"
    );
    assert!(
        page.contains("about once a month"),
        "index.html must say the token is refreshed about once a month"
    );
    assert!(
        page.contains("license activation"),
        "the hero spec row must list license activation under Network"
    );
}

#[test]
fn privacy_names_drivia_consulting_activation_and_monthly_refresh() {
    let page = normalized(&site_page("privacy.html"));
    assert!(
        page.contains("licenses come from drivia.consulting"),
        "privacy.html must say licenses come from drivia.consulting"
    );
    assert!(
        page.contains("one request activates it on this mac"),
        "privacy.html must say activation is one request"
    );
    assert!(
        page.contains("about once a month"),
        "privacy.html must say the token is refreshed about once a month"
    );
}

/// Every file under `dir` (recursively), skipping build output and deps.
fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "node_modules" || name == "target" || name == "dist-app" || name.starts_with('.')
        {
            continue;
        }
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// SCOPE RULE — every purchase link is the drivia.consulting flow
/// (https://drivia.consulting/yap), never the retired Stripe Payment Link.
#[test]
fn no_old_stripe_payment_link_in_desktop_or_site() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = crate_dir
        .parent()
        .and_then(|d| d.parent())
        .expect("src-tauri sits two levels under the repo root")
        .to_path_buf();
    // Built from parts so this file does not match itself.
    let needle = ["buy", ".stripe", ".com"].concat();
    let mut files = Vec::new();
    for root in [
        "desktop/src",
        "desktop/src-tauri/src",
        "desktop/src-tauri/tests",
        "site",
    ] {
        walk(&repo.join(root), &mut files);
    }
    // POSITIVE CONTROL — the walk really found the site pages.
    assert!(
        files.iter().any(|p| p.ends_with("site/dist/index.html")),
        "walk did not reach site/dist/index.html ({} files)",
        files.len()
    );
    let mut hits = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue; // binary asset
        };
        if text.contains(&needle) {
            hits.push(path.display().to_string());
        }
    }
    assert!(
        hits.is_empty(),
        "the old Stripe Payment Link is still referenced; use https://drivia.consulting/yap: {hits:?}"
    );
    // And the site's buy button is the new flow.
    assert!(
        site_page("index.html").contains("https://drivia.consulting/yap"),
        "index.html's buy button must point at https://drivia.consulting/yap"
    );
}
