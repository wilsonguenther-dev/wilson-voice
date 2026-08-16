//! YV131 — this item ships exactly one decision band, and it is measured.
//!
//! This file used to be called `as_norm_ships_no_tuned_threshold.rs`, and the
//! rename is the honest part of this change. "We added no threshold" is a
//! stronger-sounding claim than "we measured ours", and it was the wrong claim:
//! a module that only reorders candidates cannot convert an absolute band into
//! a relative one, which is the thing merged finding #21 asks for and the thing
//! Wilson's laptop-vs-AirPods case needs. So YV131 does ship a band, in AS-norm
//! units, and the rule this backlog actually runs on is unchanged —
//!
//!   **every threshold is an OUTPUT of YV120's harness, never an input copied
//!   from a vendor blog.**
//!
//! That is what this file enforces now. Three things have to hold:
//!
//! 1. The band is DATA, not a code constant — it lives in the cohort manifest
//!    beside the rows it was measured against, with the split and rule that
//!    produced it, so it cannot drift from the cohort or be edited without
//!    regenerating what it describes.
//! 2. No OTHER float in the shipped module is a decision boundary. The scan
//!    below is the tripwire; the allow-list carries a reason per number.
//! 3. The vendor-blog cosine bands still appear nowhere, and there is still no
//!    conversion between a normalized score and a cosine similarity — the band
//!    is compared in the unit it was measured in, which is the entire point.

use std::path::PathBuf;

use wilson_voice_lib::speaker_asnorm::{shipped_manifest_json, ImpostorCohort};

/// The SHIPPED half of the module: everything above `#[cfg(test)]`.
///
/// The cut matters and YV128 already learned why. A unit-test block is full of
/// numbers — `0.9` as a sample score, `1e-3` as a norm tolerance — none of which
/// compile into the app and none of which decide anything about a real speaker.
/// Scanning them makes the tripwire cry wolf, and a tripwire that cries wolf
/// gets its allow-list padded until it catches nothing. Cutting at the attribute
/// LINE, not at a prose mention of it, keeps the scan pointed at shipped code.
fn module_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/speaker_asnorm.rs");
    let full = std::fs::read_to_string(&path).expect("speaker_asnorm.rs is readable");
    let cut = full
        .lines()
        .position(|l| l.trim_start().starts_with("#[cfg(test)]"))
        .expect("the module has a #[cfg(test)] block; if it lost one, this scan lost its boundary");
    full.lines().take(cut).collect::<Vec<_>>().join("\n")
}

/// Numbers that are allowed to appear in the module, each with the reason it is
/// not a decision threshold. Anything else with a decimal point fails the scan.
const ALLOWED: &[(&str, &str)] = &[
    ("1e-6", "DEGENERATE_SPREAD: the width of f32 arithmetic, not a decision"),
    ("1.0", "used only to test a norm against unity in assertions/normalization"),
    ("0.0", "zero"),
    ("2.0", "exponent in a variance sum"),
];

/// Lines that are prose, not code.
fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("*") || t.starts_with("#[")
}

#[test]
fn no_float_literal_in_the_module_is_an_undocumented_decision_boundary() {
    let src = module_source();
    let mut offenders = Vec::new();

    for (n, line) in src.lines().enumerate() {
        if is_comment(line) {
            continue;
        }
        // Every float-looking literal on this line.
        let mut chars = line.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if !c.is_ascii_digit() {
                continue;
            }
            // Don't re-scan the middle of a number or an identifier.
            if i > 0 {
                let prev = line[..i].chars().next_back().unwrap();
                if prev.is_alphanumeric() || prev == '_' || prev == '.' {
                    continue;
                }
            }
            let rest = &line[i..];
            let end = rest
                .find(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == 'e' || ch == '-'))
                .unwrap_or(rest.len());
            let token = rest[..end].trim_end_matches(['-', '.', 'e']);
            if !token.contains('.') && !token.contains('e') {
                continue; // an integer: an index, a count, a byte width
            }
            if ALLOWED.iter().any(|(a, _)| *a == token) {
                continue;
            }
            offenders.push(format!("line {}: `{}` in `{}`", n + 1, token, line.trim()));
            break;
        }
    }

    assert!(
        offenders.is_empty(),
        "speaker_asnorm.rs grew a float constant that is not in the allow-list. \
         If it is a DECISION boundary it does not belong in the source at all — \
         the one this item ships lives in the cohort manifest with the split and \
         rule that measured it. If it is arithmetic, add it to ALLOWED with the \
         reason:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn the_module_never_names_the_blog_bands() {
    // The specific numbers finding #16 objected to: 0.70 / 0.55, quoted from
    // OpenWhispr's post about a different pipeline. If they ever appear in this
    // module they were copied — nothing here is measured in cosine units, and
    // the band that IS shipped is a z-score that came out of a sweep.
    let src = module_source();
    for banned in ["0.70", "0.55", "0.7_", "0.65"] {
        for (n, line) in src.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            assert!(
                !line.contains(banned),
                "line {}: `{banned}` is a vendor-blog enrollment band, and every \
                 decision number in this item came out of YV120's harness: {}",
                n + 1,
                line.trim()
            );
        }
    }
}

#[test]
fn the_tuned_numbers_live_in_the_manifest_not_the_code() {
    // K, the distinctness gate and the admission band are all tuned. They are
    // data about the shipped cohort, so they travel with the cohort rather than
    // being constants somebody can change without regenerating what they
    // describe — a band that drifts from the rows it was measured against is a
    // silent accuracy regression, and the type system cannot see it.
    let src = module_source();
    for forbidden in ["const TOP_K", "const DEFAULT_K", "const BAND", "const ADMISSION"] {
        assert!(
            !src.contains(forbidden),
            "`{forbidden}` puts a tuned number in the source, away from the \
             cohort it was measured against"
        );
    }

    let manifest: serde_json::Value = serde_json::from_str(shipped_manifest_json()).unwrap();
    assert!(manifest["top_k"].as_u64().is_some(), "K must be in the manifest");
    assert!(
        manifest["tuning"]["admission"]["normalized_band"].as_f64().is_some(),
        "the admission band must be in the manifest"
    );
    assert!(
        manifest["tuning"]["distinctness"]["gate"].as_f64().is_some(),
        "the distinctness gate must be in the manifest"
    );

    // And the band the app admits with is the one the transcript describes.
    let cohort = ImpostorCohort::shipped().expect("shipped cohort decodes");
    assert_eq!(
        cohort.admission_band().get(),
        manifest["tuning"]["admission"]["normalized_band"].as_f64().unwrap() as f32,
    );
}

#[test]
fn normalized_scores_cannot_reach_a_cosine_band() {
    // The type-level guard, asserted as a property of the public API rather
    // than as prose: there is no conversion, in either direction, between the
    // unit this module scores in and the unit YV129's bands are expressed in.
    // A band in this item had to be MEASURED in normalized units; it could not
    // be converted from a cosine one, and that is not a limitation, it is the
    // reason the item exists.
    let src = module_source();
    for forbidden in [
        "impl From<NormalizedScore> for CosineSimilarity",
        "impl From<CosineSimilarity> for NormalizedScore",
        "impl From<CosineSimilarity> for NormalizedBand",
        "fn as_cosine",
        "fn to_cosine",
    ] {
        assert!(
            !src.contains(forbidden),
            "`{forbidden}` would let an AS-norm z-score be compared against a \
             cosine enrollment band, which is the exact mixed-unit hazard \
             YV120 built two newtypes to prevent"
        );
    }
}
