//! YV128 — finding #21: a profile is all of its conditions, not its first one.
//!
//! "Recognise me on a different microphone" is the case Wilson asked about, and
//! a single running-mean centroid cannot answer it: embeddings of one voice on a
//! laptop mic and on AirPods do not sit near each other, so their average sits
//! near neither. The schema's answer is many centroids per profile, keyed by a
//! coarse condition; the MATCHING half of that answer is that a probe is scored
//! against every centroid of every profile and the best one wins.
//!
//! The interesting failure is not "does the right profile win when it is
//! obviously right" — it is "does a profile whose FIRST/primary centroid looks
//! worse still win on its second". That is the test below with two profiles, and
//! it is unsatisfiable by any implementation that keeps one canonical vector per
//! person.
//!
//! The other half of "every centroid of every profile" is which profiles are in
//! that set at all. Two skips scope it, and they are separate mechanisms rather
//! than one: a centroid of another WIDTH cannot be scored (`cosine_similarity`
//! panics), and a profile enrolled under another embedding MODEL must not be
//! scored even when the widths agree — two 192-dim models are two 192-dim
//! spaces. Each has its own test below, because a width check alone silently
//! returns a near-perfect similarity for a voice the system has never heard.

use wilson_voice_lib::speaker_profiles::{
    best_match, Centroid, NormalizedEmbedding, ProfileCentroids, SpeakerProfile,
};

const MODEL: &str = "wespeaker-en-voxceleb-campplus";

fn unit(values: &[f32]) -> NormalizedEmbedding {
    NormalizedEmbedding::new(values).unwrap()
}

fn profile(id: &str, name: &str, dim: u32) -> SpeakerProfile {
    profile_under(id, name, dim, MODEL)
}

fn profile_under(id: &str, name: &str, dim: u32, model: &str) -> SpeakerProfile {
    SpeakerProfile {
        id: id.to_string(),
        display_name: name.to_string(),
        embedding_dim: dim,
        embedding_model: model.to_string(),
        locked: true,
        is_me: false,
        created_at: "2026-08-15T00:00:00+00:00".to_string(),
        updated_at: "2026-08-15T00:00:00+00:00".to_string(),
    }
}

/// The acceptance line: two centroids under two condition keys, and a probe
/// closest to the SECOND one.
#[test]
fn a_probe_matches_the_second_centroid_when_that_is_the_closer_one() {
    let entry = ProfileCentroids {
        profile: profile("p-wilson", "Wilson", 4),
        centroids: vec![
            Centroid::first("laptop_mic_near", unit(&[1.0, 0.0, 0.0, 0.0])),
            Centroid::first("bluetooth_near", unit(&[0.0, 1.0, 0.0, 0.0])),
        ],
    };

    // Recorded on AirPods: near the bluetooth centroid, nowhere near the laptop
    // one. A profile represented by its first centroid would score this at
    // ~0.15 and a profile represented by the AVERAGE of the two would score it
    // at ~0.81 — neither of which is the answer that is actually available.
    let probe = unit(&[0.15, 0.99, 0.0, 0.0]);
    let m = best_match(std::slice::from_ref(&entry), &probe, MODEL).expect("a match");
    assert_eq!(m.profile_id, "p-wilson");
    assert_eq!(
        m.condition_key, "bluetooth_near",
        "the closer centroid must win, and must say which condition it was"
    );
    assert!(m.similarity.get() > 0.98, "{:?}", m.similarity);

    // Symmetrically, a laptop-mic recording matches the laptop centroid.
    let probe = unit(&[0.99, 0.15, 0.0, 0.0]);
    let m = best_match(std::slice::from_ref(&entry), &probe, MODEL).unwrap();
    assert_eq!(m.condition_key, "laptop_mic_near");
}

/// The non-vacuous version. Profile A's first centroid is the closest FIRST
/// centroid; profile B's second centroid is the closest centroid overall. Any
/// implementation that compares one canonical vector per profile returns A.
#[test]
fn the_best_centroid_wins_even_when_the_best_first_centroid_belongs_to_someone_else() {
    let probe = unit(&[1.0, 0.2, 0.0, 0.0]);

    let a = ProfileCentroids {
        profile: profile("p-a", "Aidan", 4),
        centroids: vec![
            // ~0.83 against the probe: the best FIRST centroid in the set.
            Centroid::first("laptop_mic_near", unit(&[1.0, 1.0, 0.0, 0.0])),
            Centroid::first("bluetooth_near", unit(&[0.0, 0.0, 1.0, 0.0])),
        ],
    };
    let b = ProfileCentroids {
        profile: profile("p-b", "Jeisil", 4),
        centroids: vec![
            // ~0.20 — worse than A's first centroid.
            Centroid::first("laptop_mic_near", unit(&[0.0, 1.0, 0.0, 0.0])),
            // ~0.999 — the best centroid in the set, on B's SECOND condition.
            Centroid::first("bluetooth_near", unit(&[1.0, 0.25, 0.0, 0.0])),
        ],
    };

    let profiles = vec![a.clone(), b.clone()];
    let m = best_match(&profiles, &probe, MODEL).expect("a match");
    assert_eq!(
        m.profile_id, "p-b",
        "scoring must run across every centroid of every profile, not the \
         first/primary one per profile"
    );
    assert_eq!(m.condition_key, "bluetooth_near");

    // The mutation, stated: comparing only each profile's first centroid picks
    // the other person. This is what makes the assertion above load-bearing.
    let first_only: Vec<ProfileCentroids> = profiles
        .iter()
        .map(|p| ProfileCentroids {
            profile: p.profile.clone(),
            centroids: p.centroids[..1].to_vec(),
        })
        .collect();
    assert_eq!(
        best_match(&first_only, &probe, MODEL).unwrap().profile_id,
        "p-a",
        "if this were also p-b the fixture would not be testing anything"
    );

    // Order of the profiles in the list changes nothing.
    let reversed = vec![b, a];
    assert_eq!(
        best_match(&reversed, &probe, MODEL).unwrap().profile_id,
        "p-b"
    );
}

/// Nothing to score against is `None`, not a zero-similarity match against
/// whatever happened to be first.
#[test]
fn no_centroids_is_no_match() {
    let probe = unit(&[1.0, 0.0, 0.0, 0.0]);
    assert!(best_match(&[], &probe, MODEL).is_none());
    assert!(best_match(
        &[ProfileCentroids {
            profile: profile("p-empty", "Never heard", 4),
            centroids: vec![],
        }],
        &probe,
        MODEL
    )
    .is_none());
}

/// A profile enrolled under a different embedding width is a profile in a
/// different vector space. It is SKIPPED, not compared: `cosine_similarity`
/// panics on mismatched lengths by design, and truncating to make the shapes
/// agree would invent a number across two unrelated spaces.
#[test]
fn a_profile_of_another_width_is_skipped_not_compared() {
    let probe = unit(&[1.0, 0.2, 0.0, 0.0]);
    let other_space = ProfileCentroids {
        profile: profile("p-256", "Enrolled on another model", 3),
        centroids: vec![Centroid::first("laptop_mic_near", unit(&[1.0, 0.2, 0.0]))],
    };
    let comparable = ProfileCentroids {
        profile: profile("p-ok", "Wilson", 4),
        centroids: vec![Centroid::first(
            "laptop_mic_near",
            unit(&[0.0, 0.0, 0.0, 1.0]),
        )],
    };

    // Does not panic, and the only comparable profile is the answer even though
    // it scores far worse than the incomparable one would have.
    let m = best_match(&[other_space.clone(), comparable], &probe, MODEL).expect("a match");
    assert_eq!(m.profile_id, "p-ok");

    // With nothing comparable at all, the honest answer is None.
    assert!(best_match(&[other_space], &probe, MODEL).is_none());
}

/// The case width CANNOT catch, and the reason `best_match` needs the probe's
/// model id rather than inferring space identity from a length.
///
/// Two 192-dim models are two 192-dim spaces, not one. CAM++ and any other
/// wespeaker 192-dim embedder — or this same catalog id re-vendored to
/// different weights after YV123 — agree on every dimension count and on
/// nothing else, so a cosine between them is arithmetic that happens to land in
/// `[-1, 1]`. Without this skip, the fixture below returns ~1.0 for a voice the
/// system has never heard, and YV129 puts an auto-confirm band on that number.
#[test]
fn a_profile_of_another_model_at_the_same_width_is_skipped_not_compared() {
    let probe = unit(&[1.0, 0.2, 0.0, 0.0]);

    // Same width as the probe (4), same condition key, and a centroid sitting
    // almost exactly on top of it — a perfect score, in a space the probe was
    // never measured in.
    let other_model = ProfileCentroids {
        profile: profile_under(
            "p-other-model",
            "Not comparable",
            4,
            "some-other-192dim-model",
        ),
        centroids: vec![Centroid::first(
            "laptop_mic_near",
            unit(&[1.0, 0.2, 0.0, 0.0]),
        )],
    };
    let comparable = ProfileCentroids {
        profile: profile("p-ok", "Wilson", 4),
        centroids: vec![Centroid::first(
            "laptop_mic_near",
            unit(&[0.0, 0.0, 0.0, 1.0]),
        )],
    };

    // The far worse — but real — score is the answer.
    let m = best_match(&[other_model.clone(), comparable.clone()], &probe, MODEL).expect("a match");
    assert_eq!(
        m.profile_id, "p-ok",
        "a same-width profile enrolled under a different embedding model must \
         be skipped, not scored"
    );

    // The mutation, stated: without the model check that fixture scores ~1.0
    // and wins outright. This is what makes the assertion above load-bearing.
    let mutation = best_match(
        &[other_model.clone(), comparable],
        &probe,
        "some-other-192dim-model",
    )
    .expect("a match");
    assert_eq!(mutation.profile_id, "p-other-model");
    assert!(
        mutation.similarity.get() > 0.99,
        "the fixture is only meaningful because the incomparable centroid \
         scores near-perfectly: {:?}",
        mutation.similarity
    );

    // With nothing enrolled under the probe's model, the honest answer is None
    // — not the best number available from some other space.
    assert!(best_match(&[other_model], &probe, MODEL).is_none());
}

/// The eval-discipline tripwire for this item: `speaker_profiles.rs` ranks and
/// reports, and never decides. Every threshold in this epic is an OUTPUT of
/// YV120's harness measured against a fixture (YV129 owns the enrollment bands
/// and puts `match_cluster` in this same file), so a tuned similarity constant
/// appearing in this module would be the exact vendor-blog failure the backlog
/// forbids — and the obvious place for one to arrive is here, next to the
/// function that produces the scores.
///
/// **It scans values, not `const` declarations.** A threshold does not need the
/// keyword: `let band = 0.62_f32;` inside `best_match`, or `similarity.get() >
/// 0.62` inline in a comparison, is the same defect and neither contains
/// `const`. So every non-comment line of the SHIPPED half of the module (above
/// `#[cfg(test)]`) is scanned for numeric literals, and the only ones permitted
/// are the arithmetic identities the centroid math actually needs — `0.0` (the
/// zero-norm guard) and `1.0` (the `n + 1` of the running mean). Any other
/// number is a measurement, and measurements come from the harness.
///
/// The test-module half is deliberately out of scope: its `[3.0, 4.0]` fixtures
/// are inputs, not decisions. The cut is the first LINE that is exactly the
/// `cfg(test)` attribute — a mention of it inside a doc comment is prose and
/// does not move the boundary — and if that line ever disappears the scan fails
/// closed with a named error rather than silently covering nothing.
#[test]
fn no_tuned_similarity_constant_ships_in_speaker_profiles() {
    let src = include_str!("../src/speaker_profiles.rs");

    // Arithmetic identities, not measurements. Anything outside this list in
    // shipped code is a number somebody chose.
    const ALLOWED: [&str; 2] = ["0.0", "1.0"];
    const MARKER: &str = "#[cfg(test)]";

    let shipped: Vec<&str> = src.lines().take_while(|l| l.trim() != MARKER).collect();
    assert!(
        shipped.len() < src.lines().count(),
        "no line of speaker_profiles.rs is exactly `{MARKER}` — this scan no \
         longer knows where the shipped half of the module ends"
    );
    assert!(
        shipped.iter().any(|l| l.contains("pub fn best_match")),
        "best_match must be inside the scanned half, or this tripwire is not \
         watching the function that produces the scores"
    );

    let mut offset = 0usize;
    for (i, line) in shipped.iter().copied().enumerate() {
        let line_start = offset;
        offset += line.len() + 1;
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        // A trailing comment on a code line is prose too.
        let code = match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        };
        for literal in numeric_literals(code) {
            assert!(
                ALLOWED.contains(&literal.as_str()),
                "line {} (byte {}): `{}` in the shipped half of \
                 speaker_profiles.rs is a threshold nobody measured — this \
                 module ranks and reports, and every band is YV129's, tuned \
                 against YV120's harness:\n{line}",
                i + 1,
                line_start,
                literal
            );
        }
    }

    // And the two band numbers the plan quoted from a vendor blog appear
    // nowhere in the module, in any form — including its tests and its prose.
    for quoted in ["0.70", "0.55", "0.7f", "0.55f"] {
        assert!(
            !src.contains(quoted),
            "{quoted} is OpenWhispr's number for a different pipeline; it may \
             not appear in this module"
        );
    }
}

/// Every numeric literal in a line of Rust, normalised enough to compare
/// against an allowlist: `0.62_f32` and `0.62f32` both read as `0.62`, and
/// `6e-1` reads as `6e-1` (unallowlisted, so a scientific-notation spelling
/// cannot smuggle a band past the scan).
///
/// Integers with no fractional or exponent part are ignored — `4` bytes per
/// `f32` and `192` in a doc string are not thresholds, and flagging them would
/// make the tripwire noisy enough that the next person deletes it.
fn numeric_literals(code: &str) -> Vec<String> {
    let chars: Vec<char> = code.chars().collect();
    let mut found = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        // Not the tail of an identifier (`f32`, `sha256`, `MIGRATION_5`).
        let starts_a_token = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        let mut j = i;
        while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '_') {
            j += 1;
        }
        let mut literal: String = chars[i..j].iter().filter(|c| **c != '_').collect();
        let mut is_numeric = false;
        // Fractional part.
        if j + 1 < chars.len() && chars[j] == '.' && chars[j + 1].is_ascii_digit() {
            let mut k = j + 1;
            while k < chars.len() && (chars[k].is_ascii_digit() || chars[k] == '_') {
                k += 1;
            }
            literal.push('.');
            literal.extend(chars[j + 1..k].iter().filter(|c| **c != '_'));
            j = k;
            is_numeric = true;
        }
        // Exponent part.
        if j < chars.len() && (chars[j] == 'e' || chars[j] == 'E') {
            let mut k = j + 1;
            if k < chars.len() && (chars[k] == '+' || chars[k] == '-') {
                k += 1;
            }
            if k < chars.len() && chars[k].is_ascii_digit() {
                while k < chars.len() && (chars[k].is_ascii_digit() || chars[k] == '_') {
                    k += 1;
                }
                literal.extend(chars[j..k].iter().filter(|c| **c != '_'));
                j = k;
                is_numeric = true;
            }
        }
        if starts_a_token && is_numeric {
            found.push(literal);
        }
        i = j.max(i + 1);
    }
    found
}
