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

use wilson_voice_lib::speaker_profiles::{
    best_match, Centroid, NormalizedEmbedding, ProfileCentroids, SpeakerProfile,
};

const MODEL: &str = "wespeaker-en-voxceleb-campplus";

fn unit(values: &[f32]) -> NormalizedEmbedding {
    NormalizedEmbedding::new(values).unwrap()
}

fn profile(id: &str, name: &str, dim: u32) -> SpeakerProfile {
    SpeakerProfile {
        id: id.to_string(),
        display_name: name.to_string(),
        embedding_dim: dim,
        embedding_model: MODEL.to_string(),
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
    let m = best_match(std::slice::from_ref(&entry), &probe).expect("a match");
    assert_eq!(m.profile_id, "p-wilson");
    assert_eq!(
        m.condition_key, "bluetooth_near",
        "the closer centroid must win, and must say which condition it was"
    );
    assert!(m.similarity.get() > 0.98, "{:?}", m.similarity);

    // Symmetrically, a laptop-mic recording matches the laptop centroid.
    let probe = unit(&[0.99, 0.15, 0.0, 0.0]);
    let m = best_match(std::slice::from_ref(&entry), &probe).unwrap();
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
    let m = best_match(&profiles, &probe).expect("a match");
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
        best_match(&first_only, &probe).unwrap().profile_id,
        "p-a",
        "if this were also p-b the fixture would not be testing anything"
    );

    // Order of the profiles in the list changes nothing.
    let reversed = vec![b, a];
    assert_eq!(best_match(&reversed, &probe).unwrap().profile_id, "p-b");
}

/// Nothing to score against is `None`, not a zero-similarity match against
/// whatever happened to be first.
#[test]
fn no_centroids_is_no_match() {
    let probe = unit(&[1.0, 0.0, 0.0, 0.0]);
    assert!(best_match(&[], &probe).is_none());
    assert!(best_match(
        &[ProfileCentroids {
            profile: profile("p-empty", "Never heard", 4),
            centroids: vec![],
        }],
        &probe
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
    let m = best_match(&[other_space.clone(), comparable], &probe).expect("a match");
    assert_eq!(m.profile_id, "p-ok");

    // With nothing comparable at all, the honest answer is None.
    assert!(best_match(&[other_space], &probe).is_none());
}

/// The eval-discipline tripwire for this item: `speaker_profiles.rs` ranks and
/// reports, and never decides. Every threshold in this epic is an OUTPUT of
/// YV120's harness measured against a fixture (YV129 owns the enrollment bands),
/// so a tuned similarity constant appearing in this module would be the exact
/// vendor-blog failure the backlog forbids — and the obvious place for one to
/// arrive is here, next to the function that produces the scores.
#[test]
fn no_tuned_similarity_constant_ships_in_speaker_profiles() {
    let src = include_str!("../src/speaker_profiles.rs");
    for (i, line) in src.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") || !trimmed.contains("const ") {
            continue;
        }
        let bytes: Vec<char> = line.chars().collect();
        let has_decimal = bytes
            .windows(3)
            .any(|w| w[0].is_ascii_digit() && w[1] == '.' && w[2].is_ascii_digit());
        assert!(
            !has_decimal,
            "line {}: a decimal constant in speaker_profiles.rs is a threshold \
             nobody measured — thresholds are YV129's, tuned against YV120's \
             harness:\n{line}",
            i + 1
        );
    }
    // And the two band numbers the plan quoted from a vendor blog appear
    // nowhere in the module, in any form.
    for quoted in ["0.70", "0.55", "0.7f", "0.55f"] {
        assert!(
            !src.contains(quoted),
            "{quoted} is OpenWhispr's number for a different pipeline; it may \
             not appear in this module"
        );
    }
}
