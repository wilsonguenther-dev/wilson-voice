//! yap-1106-lic-02 — every Buy goes to https://drivia.consulting/yap, never a
//! Stripe Payment Link.
//!
//! HARD RULE (Wilson): no purchase surface — code, copy, the marketing site or
//! a test — may name the old Payment Link host. This walks `desktop/` and
//! `site/` from the crate root and fails on any file that contains it. The
//! needle is assembled with `concat!` so this file does not match itself.
//!
//! Skipped: dependency and build output (`node_modules`, `target`, `.git`,
//! `desktop/dist`, `gen`) and files over 8 MiB (models, bundles). Everything
//! else is read as bytes, so a binary asset cannot hide the string either.

use std::path::{Path, PathBuf};

const NEEDLE: &str = concat!("buy.", "stripe", ".com");
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const SKIP_DIRS: &[&str] = &["node_modules", "target", ".git", "gen"];

fn walk(dir: &Path, skip: &[PathBuf], out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if SKIP_DIRS.contains(&name.as_ref()) || skip.iter().any(|s| s == &path) {
                continue;
            }
            walk(&path, skip, out);
        } else if meta.is_file() && meta.len() <= MAX_BYTES {
            out.push(path);
        }
    }
}

fn hits_under(root: &Path, skip: &[PathBuf]) -> (usize, Vec<String>) {
    let mut files = Vec::new();
    walk(root, skip, &mut files);
    let needle = NEEDLE.as_bytes();
    let hits = files
        .iter()
        .filter(|p| {
            std::fs::read(p)
                .map(|b| b.windows(needle.len()).any(|w| w == needle))
                .unwrap_or(false)
        })
        .map(|p| p.display().to_string())
        .collect();
    (files.len(), hits)
}

#[test]
fn no_stripe_payment_link_in_desktop_or_site() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let desktop = crate_dir
        .parent()
        .expect("src-tauri has a parent")
        .to_path_buf();
    let site = desktop.parent().expect("desktop has a parent").join("site");

    let (desktop_files, desktop_hits) = hits_under(&desktop, &[desktop.join("dist")]);
    let (site_files, site_hits) = hits_under(&site, &[]);

    // POSITIVE CONTROL — the walk actually reached the files that matter, so a
    // green run is not a walk that found nothing to read.
    assert!(
        desktop_files > 50,
        "the walk over {} read only {desktop_files} files",
        desktop.display()
    );
    assert!(
        site_files >= 1 && site.join("dist/index.html").is_file(),
        "the walk over {} did not reach site/dist/index.html",
        site.display()
    );

    let hits: Vec<String> = desktop_hits.into_iter().chain(site_hits).collect();
    assert!(
        hits.is_empty(),
        "a Stripe Payment Link host is named in: {hits:?}. Every Buy goes to \
         https://drivia.consulting/yap (license::PURCHASE_PAGE_URL)."
    );
}

#[test]
fn site_buy_button_points_at_the_purchase_page() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let index = crate_dir.join("../../site/dist/index.html");
    let html = std::fs::read_to_string(&index).expect("site/dist/index.html is readable");
    let buy = html
        .find("Buy Yap")
        .expect("site/dist/index.html has no \"Buy Yap\" button");
    let anchor_start = html[..buy]
        .rfind("<a ")
        .expect("\"Buy Yap\" is not inside a link");
    let anchor = &html[anchor_start..buy];
    assert!(
        anchor.contains(r#"href="https://drivia.consulting/yap""#),
        "the site's Buy Yap link must point at https://drivia.consulting/yap, got: {anchor}"
    );
}
