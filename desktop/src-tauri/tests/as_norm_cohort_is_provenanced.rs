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

    // The spec asked for the byte count to be stated explicitly, because "small
    // enough to embed in the binary" is a number and not a feeling. It is
    // asserted against the MANIFEST rather than against a literal copied into
    // this file: the cohort is regenerable, the gate can drop a row, and a
    // hard-coded 163,840 turns a legitimate rebuild into a test failure that
    // teaches nobody anything. What must not drift is payload vs manifest vs
    // decoded shape, and that is what is checked.
    assert_eq!(shipped_payload().len(), cohort.len() * cohort.dim() * 4);
    // The width's provenance is the pinned artefact itself: the file
    // `src/catalog.json` pins by sha256 `c46fad10…` carries ONNX metadata
    // `output_dim = 512` and a graph output `embs [B, 512]`, reproducible from
    // the digest with `onnx.load`. The plan guessed 192 and audit finding #19
    // repeated it; neither describes this file. Nothing here depends on YV122,
    // which is unmerged.
    assert_eq!(cohort.dim(), 512, "the pinned CAM++ ONNX declares output_dim = 512");
    assert!(
        shipped_payload().len() < 512 * 1024,
        "the whole argument for compiling this in rather than downloading it is \
         that it is small: {} bytes",
        shipped_payload().len()
    );
    assert!(cohort.len() >= 40, "a cohort of {} voices is not a cohort", cohort.len());

    assert_eq!(m["count"].as_u64().unwrap() as usize, cohort.len());
    assert_eq!(m["dim"].as_u64().unwrap() as usize, cohort.dim());
    assert_eq!(m["top_k"].as_u64().unwrap() as usize, cohort.top_k());
    assert!(cohort.top_k() <= cohort.len(), "top-K cannot exceed the cohort");
}

#[test]
fn every_tuned_number_carries_the_split_and_rule_that_chose_it() {
    // The eval discipline this backlog runs on, asserted rather than promised:
    // a tuned number without a transcript is a number somebody liked. There are
    // three in this item and each one has to say where it came from.
    let m = manifest();
    let tuning = &m["tuning"];

    for (key, value_field) in [
        ("design", "top_k"),
        ("distinctness", "gate"),
        ("admission", "normalized_band"),
    ] {
        let block = &tuning[key];
        assert!(
            block[value_field].is_number(),
            "tuning.{key}.{value_field} must carry the value that ships"
        );
        assert!(
            block["chosen_on"].as_str().is_some_and(|s| !s.is_empty()),
            "tuning.{key} must name the split it was chosen on"
        );
        assert!(
            block["rule"].as_str().is_some_and(|s| s.len() > 20),
            "tuning.{key} must state the rule, not just the answer"
        );
    }

    // The one that decides whether the whole item is measured or fitted.
    let measurement: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/yap23-asnorm-measurement.json"),
        )
        .expect("measurement transcript is committed"),
    )
    .expect("measurement parses");
    assert_ne!(
        tuning["design"]["chosen_on"].as_str().unwrap(),
        measurement["report_subset"].as_str().unwrap(),
        "the design was chosen on the split it is reported on — that is fitting, \
         not measuring, and this item made that mistake once already"
    );
    assert_eq!(
        tuning["design"]["top_k"].as_u64().unwrap(),
        m["top_k"].as_u64().unwrap(),
        "the K in the transcript and the K the decoder reads must be one number"
    );
}

#[test]
fn the_distinctness_gate_is_measured_and_actually_holds() {
    // The gate was a hand-picked 0.97 in this item's first draft, and the
    // shipped cohort's worst pair sat at 0.9536 — inside the band the same PR
    // used to condemn a rejected synthetic cohort. A gate nobody measured
    // cannot adjudicate that. Now it is a stated quantile of a distribution
    // measured on a split the cohort does not draw from, the rows that exceeded
    // it were dropped, and both facts are checked here against the bytes that
    // shipped rather than against the generator's word.
    let m = manifest();
    let d = &m["tuning"]["distinctness"];
    let gate = d["gate"].as_f64().expect("gate") as f32;

    let cross = &d["cross_speaker"];
    assert!(
        cross["n"].as_u64().unwrap() >= 1000,
        "a quantile of a few dozen pairs is not a distribution"
    );
    assert!(
        (gate - cross["p999"].as_f64().unwrap() as f32).abs() < 1e-4,
        "the gate must BE the quantile the rule names, not a number near it"
    );

    // The gate has to sit BETWEEN the two distributions it separates: above
    // where ordinary strangers land, below where a voice meets itself.
    //
    // And it cannot separate them cleanly, which is worth stating rather than
    // asserting away. Measured on the tuning split, the same-speaker minimum
    // sits BELOW the cross-speaker 99.9th percentile — the two distributions
    // overlap, so no cosine gate anywhere can both admit every genuinely
    // distinct pair and reject every duplicate. This asserts the gate is
    // between the two MEDIANS, which is the strongest true claim, and the
    // manifest carries both full distributions so the overlap is visible
    // instead of implied.
    let same = &d["same_speaker"];
    assert!(
        (gate as f64) > cross["median"].as_f64().unwrap(),
        "a gate below the cross-speaker median would reject ordinary strangers"
    );
    assert!(
        (gate as f64) < same["median"].as_f64().unwrap(),
        "a gate above the same-speaker median would not catch a typical duplicate"
    );
    assert!(
        same["min"].as_f64().is_some() && cross["max"].as_f64().is_some(),
        "both tails must be published — the overlap between them is the reason \
         this gate is a heuristic and not a proof"
    );

    // And it holds on the shipped bytes.
    let cohort = ImpostorCohort::shipped().unwrap();
    let payload = shipped_payload();
    let entries = m["entries"].as_array().unwrap();
    let rows: Vec<Vec<f32>> = payload
        .chunks_exact(cohort.dim() * 4)
        .map(|r| {
            r.chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect()
        })
        .collect();
    let mut over = 0;
    for i in 0..rows.len() {
        for j in (i + 1)..rows.len() {
            if entries[i]["speaker"] == entries[j]["speaker"] {
                continue;
            }
            let dot: f32 = rows[i].iter().zip(&rows[j]).map(|(a, b)| a * b).sum();
            if dot > gate {
                over += 1;
            }
        }
    }
    assert_eq!(over, 0, "{over} cross-speaker pairs exceed the measured gate {gate:.4}");
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
    assert_eq!(entries.len(), m["count"].as_u64().unwrap() as usize);
    let speakers: std::collections::BTreeSet<_> =
        entries.iter().map(|e| e["speaker"].as_i64().unwrap()).collect();
    assert!(speakers.len() >= 20, "{} distinct voices is thin", speakers.len());

    let conditions: std::collections::BTreeSet<_> = entries
        .iter()
        .map(|e| e["condition"].as_str().unwrap())
        .collect();
    assert!(
        conditions.len() >= 2,
        "a single-condition cohort has nothing condition-appropriate for the \
         adaptive top-K to select"
    );
    let declared: std::collections::BTreeSet<_> = m["conditions"]
        .as_array()
        .expect("declared conditions")
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert_eq!(conditions, declared, "the entries and the manifest must agree on conditions");
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
    let gate = m["tuning"]["distinctness"]["gate"].as_f64().unwrap() as f32;
    assert!(
        worst < gate,
        "cohort entries {} and {} are {worst:.4} similar, above the measured \
         gate {gate:.4} — that is one voice in two slots, not two voices",
        worst_pair.0,
        worst_pair.1
    );
    assert_eq!(
        (m["max_cross_speaker_similarity"].as_f64().unwrap() as f32 * 10_000.0).round(),
        (worst * 10_000.0).round(),
        "the manifest's recorded worst pair must match the shipped bytes"
    );
}
