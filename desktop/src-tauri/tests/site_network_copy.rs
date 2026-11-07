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
