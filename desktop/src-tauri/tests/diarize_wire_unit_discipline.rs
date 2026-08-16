//! YV122 acceptance — the distance/similarity discipline, checked mechanically.
//!
//! ```sh
//! cargo test --test diarize_wire_unit_discipline
//! ```
//!
//! Pure: no model, no sidecar, no audio. Everything here is either a type the
//! compiler checks or a scan of the four files that make up the diarization
//! surface.
//!
//! # The bug this is aimed at
//!
//! Merged finding #20: `sherpa_onnx::FastClusteringConfig.threshold` is a
//! cosine **distance** (smaller = more similar) and the plan's enrollment bands
//! are cosine **similarities** (larger = more similar). Read one as the other
//! and clustering ends up looser than the identity decision it feeds — and
//! every number involved stays inside `[0, 1]`, so nothing crashes, nothing
//! looks wrong, and the meeting is simply labeled with the wrong number of
//! people.
//!
//! YV120 answered with newtypes. YV121's adversarial review then found the hole
//! in that answer: a newtype closes every call site **except the one that
//! spends it**, and `CosineSimilarity::from_distance(clustering).get()` written
//! at the spend point compiled and left the suite green. Two things close it,
//! and both are here:
//!
//! * **The types, at the boundaries a scan cannot see.** `DiarizePool::diarize`
//!   takes a `CosineDistance` and nothing else — asserted as a function
//!   pointer, so a signature change is a compile error in this file rather than
//!   a silently looser API.
//! * **A census of every bare `f32` that claims to be a threshold.** There are
//!   exactly two in the whole surface, both at a boundary where the newtype
//!   physically cannot exist — JSON has no newtypes, and `yap-diarize` does not
//!   compile `diarize_metrics` at all. A third one is a failure, by name and by
//!   file, so the exemptions cannot quietly grow.
//!
//! The behavioural half — that the number which crosses the wire is the
//! distance, not its similarity twin — lives in `diarize_sidecar_pool.rs` and
//! `diarize_embed_smoke.rs`, which read the value back off the far side.

use std::path::Path;
use std::time::Duration;

use wilson_voice_lib::diarize::{DiarizeError, DiarizePool};
use wilson_voice_lib::diarize_metrics::{CosineDistance, CosineSimilarity};
use wilson_voice_lib::diarize_protocol::DiarizeSegment;

// ── The types, checked by the compiler ─────────────────────────────────────

/// The clustering API takes a DISTANCE, and the embedding floor takes a
/// `Duration`. Written as a function pointer so that widening either one to
/// `f32` — or, worse, dropping the floor back to a default — does not compile.
///
/// The floor is here for the same reason the distance is: "2.0" is a plausible
/// value in seconds and in milliseconds, and a bare float that means seconds is
/// how the wrong one of those reaches an extractor.
#[allow(clippy::type_complexity)] // the point IS the exact signature
const _CLUSTERING_API_TAKES_A_DISTANCE_AND_A_DURATION: fn(
    &DiarizePool,
    &Path,
    CosineDistance,
    Duration,
) -> Result<
    Vec<DiarizeSegment>,
    DiarizeError,
> = DiarizePool::diarize;

/// Enrollment carries the same floor, and it is not optional there either.
const _ENROLLMENT_API_TAKES_A_DURATION: fn(
    &DiarizePool,
    &Path,
    Duration,
) -> Result<Vec<f32>, DiarizeError> = DiarizePool::embed;

/// The one sanctioned conversion, in each direction, and nothing else. If a
/// second `from_*` appears these signatures stop being the whole story, and the
/// census below is what notices.
const _DISTANCE_FROM_SIMILARITY: fn(CosineSimilarity) -> CosineDistance =
    CosineDistance::from_similarity;
const _SIMILARITY_FROM_DISTANCE: fn(CosineDistance) -> CosineSimilarity =
    CosineSimilarity::from_distance;

// ── The census ─────────────────────────────────────────────────────────────

/// The whole diarization surface: the two files that CAN see the newtypes, and
/// the two that cannot.
const SURFACE: [(&str, &str); 4] = [
    ("diarize.rs", include_str!("../src/diarize.rs")),
    (
        "diarize_metrics.rs",
        include_str!("../src/diarize_metrics.rs"),
    ),
    (
        "diarize_protocol.rs",
        include_str!("../src/diarize_protocol.rs"),
    ),
    ("main.rs", include_str!("../../yap-diarize/src/main.rs")),
];

/// The files where a bare `f32` threshold is a BUG, because `CosineDistance`
/// and `CosineSimilarity` are in scope and the compiler would have enforced it.
const NEWTYPE_AWARE: [&str; 2] = ["diarize.rs", "diarize_metrics.rs"];

/// The complete set of sanctioned bare-`f32` threshold parameters: `(file,
/// function, parameter)`.
///
/// Both are boundaries where the newtype cannot exist, not places it was
/// skipped:
///
/// * `diarize_protocol.rs::diarize` — JSON has no newtypes, and this file is
///   compiled by BOTH binaries, so it may depend on nothing but `serde` and
///   `std` (its own header says so). This is the constructor
///   `DiarizePool::diarize` spends its `CosineDistance` into.
/// * `main.rs::clustering_from` — `yap-diarize` does not compile
///   `diarize_metrics`; this is the four-line function that hands the wire
///   value to `FastClusteringConfig.threshold`, which is a distance on both
///   sides, so the correct body performs no conversion at all.
const SANCTIONED: [(&str, &str, &str); 2] = [
    (
        "diarize_protocol.rs",
        "diarize",
        "clustering_distance_threshold",
    ),
    (
        "main.rs",
        "clustering_from",
        "clustering_distance_threshold",
    ),
];

/// A parameter name that claims to be on one side of the distance/similarity
/// line.
fn names_a_unit(param: &str) -> bool {
    let lower = param.to_ascii_lowercase();
    ["threshold", "distance", "similarity"]
        .iter()
        .any(|needle| lower.contains(needle))
}

/// Strip line comments, so a doc comment naming `threshold: f32` is not a
/// finding.
fn without_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every `(function, parameter)` in `source` whose declared type is a bare
/// `f32`/`f64` and whose name claims a unit.
fn bare_float_unit_params(source: &str) -> Vec<(String, String)> {
    let code = without_comments(source);
    let bytes = code.as_bytes();
    let mut found = Vec::new();
    let mut at = 0usize;
    while let Some(rel) = code[at..].find("fn ") {
        let start = at + rel;
        // `fn` must be a word, not the tail of `impl Fn` or an identifier.
        let preceded_by_word = start > 0 && {
            let prev = bytes[start - 1] as char;
            prev.is_alphanumeric() || prev == '_'
        };
        at = start + 3;
        if preceded_by_word {
            continue;
        }
        let Some(open_rel) = code[at..].find('(') else {
            break;
        };
        let name: String = code[at..at + open_rel]
            .split(['<', ' '])
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        let open = at + open_rel;
        // Match the parameter list's parentheses.
        let mut depth = 0i32;
        let mut close = open;
        for (i, ch) in code[open..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        if close <= open {
            break;
        }
        let params = &code[open + 1..close];
        let mut depth = 0i32;
        for part in params.split(|c| {
            match c {
                '<' | '(' | '[' => depth += 1,
                '>' | ')' | ']' => depth -= 1,
                _ => {}
            }
            c == ',' && depth == 0
        }) {
            let Some((raw_name, raw_type)) = part.split_once(':') else {
                continue;
            };
            let param = raw_name.trim().trim_start_matches("mut ").trim();
            let declared = raw_type.trim();
            if (declared == "f32" || declared == "f64") && names_a_unit(param) {
                found.push((name.clone(), param.to_string()));
            }
        }
        at = close;
    }
    found
}

/// The census: exactly the two sanctioned exemptions, and nothing else.
#[test]
fn no_unsanctioned_bare_float_threshold_exists_on_the_diarization_surface() {
    let mut census: Vec<(String, String, String)> = Vec::new();
    for (file, source) in SURFACE {
        for (function, param) in bare_float_unit_params(source) {
            census.push((file.to_string(), function, param));
        }
    }
    census.sort();

    let mut expected: Vec<(String, String, String)> = SANCTIONED
        .iter()
        .map(|(f, g, p)| (f.to_string(), g.to_string(), p.to_string()))
        .collect();
    expected.sort();

    assert_eq!(
        census, expected,
        "every bare `f32` that names a threshold, a distance or a similarity \
         must be one of the two boundaries where the newtype cannot exist. \
         Anything else belongs in `CosineDistance` or `CosineSimilarity`, \
         which the compiler then keeps straight (merged finding #20)"
    );

    // The files that CAN see the newtypes must have none at all.
    for file in NEWTYPE_AWARE {
        assert!(
            !census.iter().any(|(f, _, _)| f == file),
            "{file} has `CosineDistance`/`CosineSimilarity` in scope; a bare \
             float threshold there is a choice, not a boundary"
        );
    }
}

/// The production half of a source file — everything above `#[cfg(test)]`.
///
/// A test fixture that states a floor is the CORRECT shape (a caller has to
/// state one), so a scan that counted it would be a scan nobody could satisfy.
fn production_half<'a>(source: &'a str, file: &str) -> &'a str {
    let (production, tests) = source.split_once("#[cfg(test)]").unwrap_or((source, ""));
    // Non-vacuity for the split itself: a file whose test module moved, or was
    // renamed, would silently shrink this scan to nothing.
    if file == "diarize.rs" || file == "main.rs" {
        assert!(
            !tests.is_empty(),
            "{file} has no `#[cfg(test)]` module any more — this scan just \
             became the whole file, or nothing"
        );
    }
    production
}

/// **No `min_embed_seconds` default exists anywhere on this surface.**
///
/// The gate that shipped in YV122's first three rounds did not discriminate:
/// measured on the shipped CAM++, `audio_too_short` fires only below ~10
/// samples, so a 0.2 s span came back as a full-width vector that scored 0.2516
/// against its own speaker's reference while this roster's average *stranger*
/// scores 0.5789. `embed_span`'s doc comment nonetheless told YV129 that an
/// empty vector was the "too short" signal.
///
/// The fix is a real floor, and the floor is the CALLER's — the same posture
/// YV126 takes with the clustering distance, and for the same reason: the sweep
/// in this PR proves a floor is necessary and cannot say where it goes, because
/// the only speech in this repo is macOS `say` output this very item measured an
/// EER of 0.272 on and then forbade tuning against.
///
/// This test is what stops the number being invented anyway. A `const` or
/// `static` naming an embedding floor, in production code, in any of the four
/// files that make up the diarization surface, is a build failure.
#[test]
fn no_default_embedding_floor_is_written_down_anywhere() {
    let mut declared: Vec<String> = Vec::new();
    for (file, source) in SURFACE {
        for line in production_half(source, file).lines() {
            let code = line.split("//").next().unwrap_or_default().trim();
            if !(code.starts_with("const ") || code.starts_with("static ")) {
                continue;
            }
            let Some(name) = code
                .trim_start_matches("const ")
                .trim_start_matches("static ")
                .split(':')
                .next()
            else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if lower.contains("embed") || lower.contains("floor") || lower.contains("min_dur") {
                declared.push(format!("{file}: {code}"));
            }
        }
    }
    assert!(
        declared.is_empty(),
        "the shortest span worth embedding is the caller's number, not this \
         crate's — nothing here has measured one on real speech. Declared: \
         {declared:?}"
    );

    // Non-vacuity: the scanner sees a floor when one is written down.
    let planted = "const MIN_EMBED_SECONDS: Duration = Duration::from_secs(2);\n#[cfg(test)]\n";
    let seen: Vec<&str> = production_half(planted, "planted")
        .lines()
        .filter(|l| l.contains("MIN_EMBED"))
        .collect();
    assert_eq!(
        seen.len(),
        1,
        "the scan's own input must contain the defect"
    );

    // …and the sanctioned durations in `diarize.rs` are still there, so the
    // rule above is a rule about embedding floors and not a ban on constants.
    let pool = include_str!("../src/diarize.rs");
    for liveness in ["READY_BUDGET", "IDLE_UNLOAD", "DEFAULT_REQUEST_DEADLINE"] {
        assert!(
            pool.contains(liveness),
            "{liveness} is a liveness budget, not an accuracy number, and this \
             test must not have swept it away"
        );
    }
}

/// The scanner is not vacuous: it finds a planted violation.
///
/// A census that always returns the same two entries would pass the test above
/// forever. This plants the exact defect the census exists to catch — a public
/// helper taking a bare `f32` threshold — and requires the scanner to see it.
#[test]
fn the_census_would_catch_a_new_bare_float_threshold() {
    let planted = r#"
        //! a doc comment mentioning threshold: f32 must not count
        impl DiarizePool {
            /// nor must this one: similarity: f64
            pub fn set_clustering(&self, threshold: f32) {}
            pub fn matches(&self, score: CosineSimilarity) -> bool { true }
            pub fn window(&self, seconds: f32) -> f32 { seconds }
            pub fn spread(&self, distance: f64, other: CosineDistance) -> f64 { distance }
        }
    "#;
    let found = bare_float_unit_params(planted);
    assert_eq!(
        found,
        vec![
            ("set_clustering".to_string(), "threshold".to_string()),
            ("spread".to_string(), "distance".to_string()),
        ],
        "the scanner must see a bare-float threshold and a bare-float distance, \
         must ignore a newtyped parameter, an unrelated `seconds: f32`, and \
         anything inside a comment"
    );
}

/// The child performs no conversion, and the parent spends the newtype at every
/// call site.
///
/// Two source-level claims the type system cannot make:
///
/// * `clustering_from` is where the wire value meets sherpa. A distance is a
///   distance on both sides, so a correct body contains no arithmetic at all —
///   in particular no `1.0 -` and no `from_similarity`, the two spellings of
///   finding #20.
/// * Every `DiarizeRequest::diarize(` in `diarize.rs` is fed from a newtype's
///   `.get()`. The struct field is a bare `f32`, so a caller that had one lying
///   around could pass it directly and nothing downstream would object.
#[test]
fn the_threshold_is_never_converted_between_the_api_and_sherpa() {
    let child = without_comments(include_str!("../../yap-diarize/src/main.rs"));
    let body = child
        .split_once("fn clustering_from")
        .expect("the sidecar's one conversion point")
        .1;
    let body = &body[..body.find("\n}").expect("the function ends")];
    for banned in ["1.0 -", "1.0-", "from_similarity", "from_distance"] {
        assert!(
            !body.contains(banned),
            "`{banned}` in `clustering_from`: the wire field and \
             `FastClusteringConfig.threshold` are BOTH cosine distances, so any \
             conversion here is the inversion finding #20 describes"
        );
    }

    let parent = without_comments(include_str!("../src/diarize.rs"));
    let calls: Vec<&str> = parent
        .match_indices("DiarizeRequest::diarize(")
        .map(|(i, needle)| {
            // Balanced parens, not the first `)`: the id argument is
            // `next_id()`, and stopping there would read a call that spends a
            // newtype three characters later as one that does not.
            let open = i + needle.len() - 1;
            let mut depth = 0i32;
            let mut end = parent.len();
            for (offset, ch) in parent[open..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = open + offset + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            &parent[i..end]
        })
        .collect();
    assert!(
        !calls.is_empty(),
        "diarize.rs must build a diarize request somewhere"
    );
    for call in calls {
        assert!(
            call.contains(".get()"),
            "the threshold argument must be spent from a newtype, never passed \
             as a loose float: {call}"
        );
        assert!(
            !call.contains("from_distance") && !call.contains("from_similarity"),
            "a conversion on the way into the request is the inversion itself: \
             {call}"
        );
    }
}
