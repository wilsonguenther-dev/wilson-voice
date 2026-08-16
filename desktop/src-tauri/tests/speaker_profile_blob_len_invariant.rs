//! YV128 — finding #19's second half: `blob.len() == embedding_dim * 4`,
//! asserted on **read**.
//!
//! The interesting corruption is not the one you write — the writer always has
//! the vector in hand and gets the length right by construction. It is the one
//! already on disk: a half-completed write, a file restored from a partial
//! backup, a row written by a build whose model had a different width. A read
//! path that trusts the length either truncates (producing a shorter vector
//! that then scores plausibly-low against everything, and quietly stops
//! recognising somebody) or indexes past the end (a panic in the middle of
//! opening a meeting).
//!
//! So the check runs at USE time, every time, and says which two numbers
//! disagreed.

mod support;

use support::{open_db, temp_dir};
use wilson_voice_lib::speaker_profiles::{EmbeddingError, NormalizedEmbedding, BYTES_PER_F32};

const MODEL: &str = "wespeaker-en-voxceleb-campplus";
const DIM: u32 = 192;

/// A deterministic, non-unit-length vector — deliberately not normalised, so
/// the ingestion half of the L2 discipline is doing real work in every test
/// that uses this.
fn raw_embedding(seed: u32, dim: u32) -> Vec<f32> {
    (0..dim)
        .map(|i| {
            let x = ((i * 31 + seed * 7919) % 211) as f32 / 211.0;
            (x - 0.5) * 9.0
        })
        .collect()
}

#[test]
fn a_truncated_blob_is_rejected_on_read_with_both_numbers() {
    let dir = temp_dir("blob-truncated");
    let path = dir.join("wilson_voice.db");
    let db = open_db(&dir);

    let profile = db
        .create_speaker_profile("Wilson", DIM, MODEL, true)
        .unwrap();
    let sample = NormalizedEmbedding::new(&raw_embedding(1, DIM)).unwrap();
    db.observe_speaker_embedding(&profile.id, "laptop_mic_near", &sample)
        .unwrap();

    // Sanity: the row is the right length before anybody corrupts it.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        let blob: Vec<u8> = conn
            .query_row(
                "SELECT embedding FROM speaker_centroids WHERE profile_id = ?1",
                rusqlite::params![profile.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(blob.len(), DIM as usize * BYTES_PER_F32);
    }
    drop(db);

    // Now truncate it the way a half-finished write would. Written as raw
    // bytes rather than through SQLite's `substr`, so the test is about the
    // read path and not about SQLite's blob/text coercion rules.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        let short = vec![0x11u8; 500];
        conn.execute(
            "UPDATE speaker_centroids SET embedding = ?2 WHERE profile_id = ?1",
            rusqlite::params![profile.id, short],
        )
        .unwrap();
    }

    let db = open_db(&dir);
    let err = db
        .list_speaker_profiles()
        .expect_err("a truncated centroid must be refused, not silently shortened");
    assert!(
        err.contains("500") && err.contains("768") && err.contains("192"),
        "the error must name the bytes present, the bytes required and the \
         dimension they came from: {err}"
    );
    assert!(
        err.contains(&profile.id) && err.contains("laptop_mic_near"),
        "…and the row it happened on: {err}"
    );

    // The same failure from the single-profile read, not just the list.
    assert!(db.get_speaker_profile(&profile.id).is_err());
}

/// A blob that is too LONG is equally refused. This is the case a width change
/// produces (a 256-dim model writing into a profile enrolled at 192), and it is
/// the one a naive `chunks_exact` read would silently survive by ignoring the
/// tail.
#[test]
fn an_overlong_blob_is_rejected_too() {
    let dir = temp_dir("blob-overlong");
    let path = dir.join("wilson_voice.db");
    let db = open_db(&dir);

    let profile = db
        .create_speaker_profile("Wilson", DIM, MODEL, true)
        .unwrap();
    let sample = NormalizedEmbedding::new(&raw_embedding(2, DIM)).unwrap();
    db.observe_speaker_embedding(&profile.id, "bluetooth_near", &sample)
        .unwrap();
    drop(db);

    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        let doubled = vec![0x22u8; DIM as usize * BYTES_PER_F32 * 2];
        conn.execute(
            "UPDATE speaker_centroids SET embedding = ?2 WHERE profile_id = ?1",
            rusqlite::params![profile.id, doubled],
        )
        .unwrap();
    }

    let db = open_db(&dir);
    let err = db
        .list_speaker_profiles()
        .expect_err("1536 bytes is not 768");
    assert!(err.contains("1536") && err.contains("768"), "{err}");
}

/// The invariant at the level it is defined: a pure function, no database.
#[test]
fn from_blob_is_the_invariant() {
    let e = NormalizedEmbedding::new(&raw_embedding(3, DIM)).unwrap();
    let blob = e.to_blob();
    assert_eq!(blob.len(), DIM as usize * BYTES_PER_F32);

    // Right length: round-trips, and comes back a unit vector.
    let back = NormalizedEmbedding::from_blob(&blob, DIM as usize).unwrap();
    assert!((back.l2_norm() - 1.0).abs() < 1e-5);
    assert_eq!(back.dim(), DIM as usize);

    // One byte short, one byte long, and a plausible-but-wrong dimension.
    assert_eq!(
        NormalizedEmbedding::from_blob(&blob[..blob.len() - 1], DIM as usize),
        Err(EmbeddingError::BlobLength {
            embedding_dim: 192,
            expected_bytes: 768,
            actual_bytes: 767,
        })
    );
    let mut longer = blob.clone();
    longer.push(0);
    assert_eq!(
        NormalizedEmbedding::from_blob(&longer, DIM as usize),
        Err(EmbeddingError::BlobLength {
            embedding_dim: 192,
            expected_bytes: 768,
            actual_bytes: 769,
        })
    );
    assert!(NormalizedEmbedding::from_blob(&blob, 256).is_err());
    assert!(NormalizedEmbedding::from_blob(&blob, 0).is_err());
}

/// The write side refuses the same mismatch before it can become a stored row:
/// a sample of the wrong width for the profile it is being folded into is an
/// error naming both numbers, not a silent no-op and not a truncation.
#[test]
fn a_sample_of_the_wrong_width_never_reaches_the_table() {
    let dir = temp_dir("blob-wrong-width-write");
    let db = open_db(&dir);

    let profile = db
        .create_speaker_profile("Wilson", DIM, MODEL, true)
        .unwrap();
    let wrong = NormalizedEmbedding::new(&raw_embedding(4, 256)).unwrap();
    let err = db
        .observe_speaker_embedding(&profile.id, "laptop_mic_near", &wrong)
        .expect_err("256 dims into a 192-dim profile");
    assert!(err.contains("192") && err.contains("256"), "{err}");

    assert!(
        db.get_speaker_profile(&profile.id)
            .unwrap()
            .unwrap()
            .centroids
            .is_empty(),
        "the refused sample must not have been stored"
    );
}
