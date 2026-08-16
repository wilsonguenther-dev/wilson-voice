//! YV131 — the shipped impostor cohort is what its manifest says it is.
//!
//! A cohort is 160 KB of opaque floats. Nothing about looking at it tells you
//! which model produced it, from whose voices, or whether the manifest beside it
//! still describes it — and every one of those being wrong produces a working
//! build that scores nonsense confidently. So the manifest is the trust anchor,
//! the same posture `models.rs` takes for downloaded weights, and this file is
//! where the anchor is actually checked rather than assumed.
//!
//! The check that matters most is the digest. The realistic failure is not
//! corruption in transit — it is a human regenerating the payload and
//! committing it without the manifest, or the reverse. That produces two files
//! that disagree, and it is invisible in review.

use sha2::{Digest, Sha256};
use wilson_voice_lib::speaker_asnorm::{shipped_manifest_json, shipped_payload, ImpostorCohort};

fn manifest() -> serde_json::Value {
    serde_json::from_str(shipped_manifest_json()).expect("cohort manifest parses")
}

#[test]
fn the_payload_hashes_to_what_the_manifest_claims() {
    let m = manifest();
    let actual = format!("{:x}", Sha256::digest(shipped_payload()));
    assert_eq!(
        actual,
        m["sha256"].as_str().expect("manifest carries a sha256"),
        "the committed cohort payload and its manifest disagree — one of them \
         was regenerated without the other. Re-run \
         scripts/yv131-build-impostor-cohort.py and commit both."
    );
    assert_eq!(
        shipped_payload().len(),
        m["bytes"].as_u64().unwrap() as usize,
        "manifest byte count does not describe the payload"
    );
}

#[test]
fn the_cohort_is_the_size_and_shape_the_module_documents() {
    let m = manifest();
    let cohort = ImpostorCohort::shipped().expect("shipped cohort decodes");

    // The number the spec asked to be stated explicitly, because "small enough
    // to embed in the binary" is a number and not a feeling.
    assert_eq!(shipped_payload().len(), 163_840, "cohort is 163,840 bytes = 160 KiB");
    assert_eq!(cohort.len(), 80, "80 entries");
    assert_eq!(cohort.dim(), 512, "CAM++ measures 512 wide, not the plan's guessed 192");
    assert_eq!(cohort.len() * cohort.dim() * 4, shipped_payload().len());

    assert_eq!(m["count"].as_u64().unwrap() as usize, cohort.len());
    assert_eq!(m["dim"].as_u64().unwrap() as usize, cohort.dim());
    assert_eq!(m["top_k"].as_u64().unwrap() as usize, cohort.top_k());
    assert!(cohort.top_k() <= cohort.len(), "top-K cannot exceed the cohort");
}

#[test]
fn every_cohort_row_is_a_unit_vector() {
    // Cosine similarity against a non-unit row is still a number, and still
    // wrong. The generator L2-normalizes; this is where that stops being a
    // promise in a docstring.
    let cohort = ImpostorCohort::shipped().unwrap();
    let payload = shipped_payload();
    let dim = cohort.dim();
    for (i, row) in payload.chunks_exact(dim * 4).enumerate() {
        let norm: f32 = row
            .chunks_exact(4)
            .map(|b| {
                let v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                v * v
            })
            .sum::<f32>()
            .sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-3,
            "cohort row {i} has L2 norm {norm}, not 1.0"
        );
    }
}

#[test]
fn the_cohort_was_built_by_the_model_the_app_downloads() {
    // The single most consequential field. Embeddings from two different models
    // are two different coordinate systems; a cohort from the wrong one is not
    // a worse cohort, it is a random one. The digest here must be the CAM++
    // entry in catalog.json — the file the app actually fetches and verifies.
    let m = manifest();
    let cohort_model = m["embedder"]["model_sha256"].as_str().expect("embedder digest");

    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../src/catalog.json")).expect("catalog parses");
    let campp = catalog["diarize_models"]
        .as_array()
        .expect("diarize_models")
        .iter()
        .find(|m| m["role"] == "embedding")
        .expect("catalog carries an embedding model");

    assert_eq!(
        cohort_model,
        campp["file"]["sha256"].as_str().unwrap(),
        "the cohort was embedded by a different model than the one the app \
         downloads — every score computed against it would be meaningless"
    );

    let cohort = ImpostorCohort::shipped().unwrap();
    assert!(cohort.require_embedder(cohort_model).is_ok());
}

#[test]
fn the_voices_are_attributed_and_license_clear() {
    // CC-BY-4.0 requires attribution, and a cohort of unattributed voices is a
    // licensing problem shipped in a binary. Only embeddings travel — 512 floats
    // per speaker, from which no audio can be reconstructed — but the corpus
    // still has to be named.
    let m = manifest();
    let src = &m["source"];
    assert_eq!(src["license"].as_str().unwrap(), "CC-BY-4.0");
    assert!(src["attribution"].as_str().unwrap().contains("LibriSpeech"));
    assert!(src["corpus"].as_str().unwrap().contains("LibriSpeech"));
    assert!(src["url"].as_str().unwrap().contains("openslr.org"));

    let entries = m["entries"].as_array().expect("per-entry provenance");
    assert_eq!(entries.len(), 80);
    let speakers: std::collections::BTreeSet<_> =
        entries.iter().map(|e| e["speaker"].as_i64().unwrap()).collect();
    assert_eq!(speakers.len(), 40, "40 distinct speakers, each under 2 conditions");

    let conditions: std::collections::BTreeSet<_> = entries
        .iter()
        .map(|e| e["condition"].as_str().unwrap())
        .collect();
    assert_eq!(
        conditions.len(),
        2,
        "a single-condition cohort has nothing condition-appropriate for the \
         adaptive top-K to select"
    );
}

#[test]
fn no_two_cohort_speakers_are_the_same_voice() {
    // The cohort's one substantive invariant: N slots must hold N voices.
    // Duplicates are precisely what adaptive top-K selects, so a cohort with one
    // voice in it three times normalizes against that voice three times over.
    // The generator asserts this at build time; this asserts it about the bytes
    // that actually shipped.
    let cohort = ImpostorCohort::shipped().unwrap();
    let m = manifest();
    let entries = m["entries"].as_array().unwrap();
    let payload = shipped_payload();
    let dim = cohort.dim();

    let rows: Vec<Vec<f32>> = payload
        .chunks_exact(dim * 4)
        .map(|r| {
            r.chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect()
        })
        .collect();

    let mut worst = 0.0f32;
    let mut worst_pair = (0usize, 0usize);
    for i in 0..rows.len() {
        for j in (i + 1)..rows.len() {
            // Two conditions of ONE speaker are meant to be close.
            if entries[i]["speaker"] == entries[j]["speaker"] {
                continue;
            }
            let dot: f32 = rows[i].iter().zip(&rows[j]).map(|(a, b)| a * b).sum();
            if dot > worst {
                worst = dot;
                worst_pair = (i, j);
            }
        }
    }
    assert!(
        worst < 0.97,
        "cohort entries {} and {} are {worst:.4} similar — that is one voice in \
         two slots, not two voices",
        worst_pair.0,
        worst_pair.1
    );
    assert_eq!(
        (m["max_cross_speaker_similarity"].as_f64().unwrap() as f32 * 10_000.0).round(),
        (worst * 10_000.0).round(),
        "the manifest's recorded worst pair must match the shipped bytes"
    );
}
