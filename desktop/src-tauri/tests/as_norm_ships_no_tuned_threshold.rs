//! YV131 — this item ships no tuned decision threshold, and a machine says so.
//!
//! The rule this backlog is built on: every threshold is an OUTPUT of YV120's
//! harness, never an input copied from a vendor blog. YV131's answer is
//! stronger than "we measured ours" — it is "we added none". AS-norm changes
//! the ORDER of candidates and the SCALE of scores; the accept/reject bands
//! stay in cosine units where YV129 measured them.
//!
//! That claim is only worth anything if it cannot rot. The same posture YV126
//! and YV130 already took (`*_ships_no_tuned_threshold.rs`): scan the shipped
//! module and fail on a float literal that looks like a decision boundary.
//!
//! # Why a type is the real guard and this is the backstop
//!
//! The load-bearing protection is that [`NormalizedScore`] is not a
//! [`CosineSimilarity`], so a normalized score cannot be compared against an
//! enrollment band without a compile error. This file catches the other way in:
//! somebody introducing a NEW constant in normalized units — `if score > 1.8`.
//! A grep cannot understand code, so it errs toward complaining, and every
//! number that legitimately belongs here is listed below with its reason.

use std::path::PathBuf;

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
    ("0.5", "the symmetric average of the two AS-norm sides — the '/2' in (a+b)/2"),
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
         If it is a DECISION threshold it does not belong in this item at all — \
         YV131 ships none, and the bands live in YV129's cosine units. If it is \
         arithmetic, add it to ALLOWED with the reason:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn the_module_never_names_the_blog_bands() {
    // The specific numbers finding #16 objected to: 0.70 / 0.55, quoted from
    // OpenWhispr's post about a different pipeline. If they ever appear in this
    // module they were copied, because nothing here measures in cosine units.
    let src = module_source();
    for banned in ["0.70", "0.55", "0.7_", "0.65"] {
        for (n, line) in src.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            assert!(
                !line.contains(banned),
                "line {}: `{banned}` is a vendor-blog enrollment band, and this \
                 module does not make accept/reject decisions at all: {}",
                n + 1,
                line.trim()
            );
        }
    }
}

#[test]
fn the_tuned_hyperparameter_lives_in_the_manifest_not_the_code() {
    // K — how many cohort scores enter the mean and standard deviation — IS
    // tuned, on dev-clean, reported on dev-other. It is data about the shipped
    // cohort, so it travels with the cohort rather than being a constant
    // somebody can change without regenerating what it describes.
    let src = module_source();
    assert!(
        !src.contains("const TOP_K") && !src.contains("const DEFAULT_K"),
        "top-K belongs to the cohort manifest, not to the code — a K that can \
         drift from the cohort it was measured against is a silent regression"
    );

    let manifest: serde_json::Value =
        serde_json::from_str(wilson_voice_lib::speaker_asnorm::shipped_manifest_json()).unwrap();
    assert!(
        manifest["top_k"].as_u64().is_some(),
        "and it must actually be in the manifest"
    );
}

#[test]
fn normalized_scores_cannot_reach_a_cosine_band() {
    // The type-level guard, asserted as a property of the public API rather
    // than as prose: there is no conversion, in either direction, between the
    // unit this module scores in and the unit enrollment bands are expressed
    // in. If someone adds one, this catches it in review as a source change.
    let src = module_source();
    for forbidden in [
        "impl From<NormalizedScore> for CosineSimilarity",
        "impl From<CosineSimilarity> for NormalizedScore",
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
