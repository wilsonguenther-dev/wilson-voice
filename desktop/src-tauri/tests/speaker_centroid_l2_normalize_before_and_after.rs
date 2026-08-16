//! YV128 — the L2 discipline, on every update and not just the first.
//!
//! The epic plan's running mean is `new_avg = (avg × n + emb) / (n + 1)` and is
//! silent on normalisation. Both halves matter and they fail differently:
//!
//! * **Before averaging.** Raw embeddings are not unit vectors, and their
//!   magnitudes vary with how loud the speaker was. Averaging them unnormalised
//!   weights the loud samples more heavily than the quiet ones — a centroid that
//!   drifts toward whichever recording had the most gain, which is not a
//!   property of the voice.
//! * **After averaging.** The mean of two unit vectors is shorter than 1 (equal
//!   to 1 only when they are identical). Feed that back in as the next `avg` and
//!   the shrink compounds: the centroid keeps its DIRECTION but loses magnitude
//!   every update, and the growing gap between its scale and the samples' is
//!   exactly the kind of thing that is fine in `f64` arithmetic and not fine in
//!   the `f32` dot products this is stored and compared in.
//!
//! This file asserts the norm after EVERY update — the "not just the first"
//! half of the acceptance criterion — and then measures the same sequence
//! through the unnormalised formula, so the assertion above is a demonstrated
//! difference rather than a claim.

mod support;

use support::{open_db, temp_dir};
use wilson_voice_lib::speaker_profiles::{l2_norm, Centroid, NormalizedEmbedding};

const MODEL: &str = "wespeaker-en-voxceleb-campplus";
const DIM: usize = 192;

/// Deterministic embeddings whose magnitudes vary wildly — from 0.02 to well
/// over 100 — which is the input the discipline exists for. A test that fed in
/// unit vectors would pass with the normalisation deleted.
fn raw_sample(i: usize) -> Vec<f32> {
    let scale = match i % 5 {
        0 => 0.02,
        1 => 1.0,
        2 => 37.5,
        3 => 0.4,
        _ => 128.0,
    };
    (0..DIM)
        .map(|d| {
            let x = (((d * 17 + i * 101) % 257) as f32 / 257.0) - 0.5;
            // A shared direction with per-sample wobble: real enrolment samples
            // of one voice are close but never identical.
            (x + 0.35 * ((d % 7) as f32 - 3.0) / 3.0) * scale
        })
        .collect()
}

#[test]
fn the_centroid_is_a_unit_vector_after_every_update() {
    let first = NormalizedEmbedding::new(&raw_sample(0)).unwrap();
    assert!(
        (first.l2_norm() - 1.0).abs() < 1e-5,
        "ingestion normalises: the raw sample's norm was {}",
        l2_norm(&raw_sample(0))
    );

    let mut centroid = Centroid::first("laptop_mic_near", first);
    for i in 1..40 {
        let sample = NormalizedEmbedding::new(&raw_sample(i)).unwrap();
        centroid = centroid.updated_with(&sample).unwrap();
        let norm = centroid.vector.l2_norm();
        assert!(
            (norm - 1.0).abs() < 1e-5,
            "after update {i} the centroid's L2 norm is {norm}, not 1.0"
        );
        assert_eq!(centroid.sample_count, (i + 1) as u32);
        assert_eq!(centroid.condition_key, "laptop_mic_near");
    }
}

/// The mutation this discipline exists to prevent, run for real: the SAME
/// sequence through the plan's formula with neither normalisation step. If the
/// omission were harmless this test could not be written.
#[test]
fn the_unnormalized_formula_drifts_on_the_same_sequence() {
    // (a) no normalisation at all — the plan's formula, verbatim.
    let mut avg: Vec<f64> = raw_sample(0).iter().map(|v| *v as f64).collect();
    for i in 1..40 {
        let s = raw_sample(i);
        let n = i as f64;
        avg = avg
            .iter()
            .zip(s.iter())
            .map(|(a, x)| (a * n + *x as f64) / (n + 1.0))
            .collect();
    }
    let raw_norm = avg.iter().map(|v| v * v).sum::<f64>().sqrt();
    assert!(
        (raw_norm - 1.0).abs() > 0.1,
        "the unnormalised running mean should be nowhere near unit length; it \
         measured {raw_norm}, which would make this whole item pointless"
    );

    // (b) normalised on the way IN but not renormalised after — the subtler
    // half, and the one a reviewer is most likely to wave through.
    let mut avg: Vec<f64> = NormalizedEmbedding::new(&raw_sample(0))
        .unwrap()
        .as_slice()
        .iter()
        .map(|v| *v as f64)
        .collect();
    for i in 1..40 {
        let s = NormalizedEmbedding::new(&raw_sample(i)).unwrap();
        let n = i as f64;
        avg = avg
            .iter()
            .zip(s.as_slice().iter())
            .map(|(a, x)| (a * n + *x as f64) / (n + 1.0))
            .collect();
    }
    let half_norm = avg.iter().map(|v| v * v).sum::<f64>().sqrt();
    assert!(
        (half_norm - 1.0).abs() > 1e-3,
        "averaging unit vectors without renormalising must shrink the result; \
         it measured {half_norm}"
    );

    // And the shipped path, on the identical sequence, does not.
    let mut centroid = Centroid::first(
        "laptop_mic_near",
        NormalizedEmbedding::new(&raw_sample(0)).unwrap(),
    );
    for i in 1..40 {
        centroid = centroid
            .updated_with(&NormalizedEmbedding::new(&raw_sample(i)).unwrap())
            .unwrap();
    }
    assert!((centroid.vector.l2_norm() - 1.0).abs() < 1e-5);
}

/// Through the database, where the vector makes a round trip via a BLOB on
/// every update: read, fold, renormalise, write.
///
/// The assertion is deliberately against the RAW BYTES ON DISK rather than
/// against what `get_speaker_profile` hands back. `from_blob` normalises what it
/// reads — which is the right behaviour for a read path, and which would also
/// make this test pass with the renormalisation deleted from the update. What
/// has to be true is that the stored centroid is itself unit length, because
/// that is the value every subsequent update folds the next sample into.
#[test]
fn the_stored_centroid_stays_unit_length_across_a_round_trip_per_update() {
    let dir = temp_dir("l2-round-trip");
    let db = open_db(&dir);
    let profile = db
        .create_speaker_profile("Wilson", DIM as u32, MODEL, true)
        .unwrap();

    for i in 0..25 {
        let sample = NormalizedEmbedding::new(&raw_sample(i)).unwrap();
        let updated = db
            .observe_speaker_embedding(&profile.id, "laptop_mic_near", &sample)
            .unwrap();
        assert_eq!(updated.sample_count, (i + 1) as u32);

        let stored = db.get_speaker_profile(&profile.id).unwrap().unwrap();
        assert_eq!(stored.centroids.len(), 1, "one condition, one centroid");
        assert_eq!(stored.centroids[0].sample_count, (i + 1) as u32);

        let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
        let blob: Vec<u8> = conn
            .query_row(
                "SELECT embedding FROM speaker_centroids WHERE profile_id = ?1",
                rusqlite::params![profile.id],
                |r| r.get(0),
            )
            .unwrap();
        let on_disk: Vec<f32> = blob
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let norm = l2_norm(&on_disk);
        assert!(
            (norm - 1.0).abs() < 1e-5,
            "after stored update {i} the bytes on disk have norm {norm}, and \
             those bytes are what the NEXT update averages into"
        );
    }
}

/// A second condition does not dilute the first: enrolling on AirPods leaves
/// the laptop-mic centroid, and its sample count, exactly where they were. This
/// is the reason `sample_count` lives on the centroid and not on the profile.
#[test]
fn a_second_condition_does_not_touch_the_first() {
    let dir = temp_dir("l2-two-conditions");
    let db = open_db(&dir);
    let profile = db
        .create_speaker_profile("Wilson", DIM as u32, MODEL, true)
        .unwrap();

    for i in 0..6 {
        db.observe_speaker_embedding(
            &profile.id,
            "laptop_mic_near",
            &NormalizedEmbedding::new(&raw_sample(i)).unwrap(),
        )
        .unwrap();
    }
    let laptop_before = db
        .get_speaker_profile(&profile.id)
        .unwrap()
        .unwrap()
        .centroids
        .into_iter()
        .find(|c| c.condition_key == "laptop_mic_near")
        .unwrap();

    for i in 100..103 {
        db.observe_speaker_embedding(
            &profile.id,
            "bluetooth_near",
            &NormalizedEmbedding::new(&raw_sample(i)).unwrap(),
        )
        .unwrap();
    }

    let stored = db.get_speaker_profile(&profile.id).unwrap().unwrap();
    assert_eq!(stored.centroids.len(), 2);
    let laptop_after = stored
        .centroids
        .iter()
        .find(|c| c.condition_key == "laptop_mic_near")
        .unwrap();
    assert_eq!(
        laptop_after, &laptop_before,
        "the bluetooth samples must not have moved the laptop centroid at all"
    );
    let bt = stored
        .centroids
        .iter()
        .find(|c| c.condition_key == "bluetooth_near")
        .unwrap();
    assert_eq!(bt.sample_count, 3);
    assert_eq!(laptop_after.sample_count, 6);
    assert!((bt.vector.l2_norm() - 1.0).abs() < 1e-5);
}

/// The discipline has to hold at the OTHER door too.
///
/// `NormalizedEmbedding::new` is only "the only constructor" for as long as
/// nobody derives `Deserialize` on the type: a derived impl deserialises the
/// wire's `Vec<f32>` and wraps it, calling nothing. That is a second
/// constructor with none of the checks, and it is not hypothetical — the
/// derive shipped in this file's first version, and `serde_json::from_str`
/// produced a "normalised" embedding with an L2 norm of 5.
///
/// This asserts the three inputs the constructor refuses stay refused through
/// serde, and that a vector it accepts arrives unit-length rather than as the
/// raw numbers on the wire. It fails loudly the moment anyone swaps the
/// hand-written impl back for a derive — which is exactly what YV129 might be
/// tempted to do when it parses the sidecar's `{"embedding": [...]}`.
#[test]
fn serde_deserialisation_cannot_bypass_the_normalising_constructor() {
    // The case that made the module's headline claim false: norm 5, not 1.
    let parsed: NormalizedEmbedding =
        serde_json::from_str("[3.0,4.0]").expect("a finite non-zero vector deserialises");
    assert!(
        (parsed.l2_norm() - 1.0).abs() < 1e-6,
        "serde must normalise like the constructor does; got norm {} for [3.0, 4.0]",
        parsed.l2_norm()
    );
    assert_eq!(parsed.as_slice(), &[0.6, 0.8]);

    // Every input `new` refuses has to be refused here too, by name.
    for wire in ["[]", "[0.0,0.0]", "[1.0,null]"] {
        assert!(
            serde_json::from_str::<NormalizedEmbedding>(wire).is_err(),
            "serde accepted {wire}, which NormalizedEmbedding::new refuses"
        );
    }

    // And a real embedding survives the round trip as a unit vector, so the
    // hand-written pair is usable at the boundary YV129 needs, not just safe.
    let raw = raw_sample(2);
    let json = serde_json::to_string(&NormalizedEmbedding::new(&raw).unwrap()).unwrap();
    let back: NormalizedEmbedding = serde_json::from_str(&json).unwrap();
    assert_eq!(back, NormalizedEmbedding::new(&raw).unwrap());
    assert!((back.l2_norm() - 1.0).abs() < 1e-5);
    assert!(
        (l2_norm(&raw) - 1.0).abs() > 1.0,
        "the fixture must be far from unit-length or this test proves nothing"
    );
}
