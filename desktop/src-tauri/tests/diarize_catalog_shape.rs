//! YV123 — the diarization catalog entries, asserted the way `models.rs`'s own
//! catalog tests assert the ASR and polish lists.
//!
//! The point of Wilson's closed O6 decision is that the diarization bytes come
//! from a repo HE owns, at a revision HE pinned, verified against a sha256
//! compiled into this binary — not from a vendor's GitHub release at runtime.
//! Every assertion below is one of those four words: owned, pinned, verified,
//! not-at-runtime.

use wilson_voice_lib::models::{
    catalog_model, diarize_download_bytes, diarize_download_urls, diarize_extract_dir,
    diarize_model, diarize_model_for_role, diarize_model_path, diarize_models, models_dir,
    polish_model, ArchiveKind, DiarizeModelRole,
};

/// The mirror Wilson created for this item. Named here as well as in the
/// catalog so that re-pointing the catalog at a vendor release (or at anyone
/// else's account) fails a test rather than a code review.
const MIRROR_REPO: &str = "wilsonguenther/yap-diarize-models";
const MIRROR_REVISION: &str = "c0f5026b16bf2cac9b5f9e6e2a36da6c6a8628ec";

fn is_lower_hex(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

#[test]
fn every_entry_is_owned_pinned_and_hash_verified() {
    let models = diarize_models();
    assert_eq!(models.len(), 2, "segmentation + embedding, no more");

    for m in models {
        assert!(!m.id.is_empty());
        assert!(!m.name.is_empty());
        assert!(!m.description.is_empty());
        assert!(!m.license.is_empty(), "{}: license must be stated", m.id);

        // Owned.
        assert_eq!(m.repo, MIRROR_REPO, "{}: not the Wilson-owned mirror", m.id);

        // Pinned — a 40-char commit sha, never `main` or any other moving ref.
        assert_eq!(m.revision.len(), 40, "{}: revision must be a commit sha", m.id);
        assert!(is_lower_hex(&m.revision), "{}: revision must be a sha", m.id);
        assert_eq!(m.revision, MIRROR_REVISION, "{}: unexpected revision", m.id);

        // Verified — 64-char lowercase hex, on the bytes that are downloaded.
        assert_eq!(m.file.sha256.len(), 64, "{}: bad sha256", m.id);
        assert!(is_lower_hex(&m.file.sha256), "{}: sha256 must be lowercase hex", m.id);
        assert!(m.file.size_bytes > 0, "{}: no size", m.id);

        // Not reachable as anything else: an ONNX graph offered as a
        // transcription engine, or as a polish LLM, is a bug.
        assert!(catalog_model(&m.id).is_none(), "{} leaked into the ASR list", m.id);
        assert!(polish_model(&m.id).is_none(), "{} leaked into the polish list", m.id);
    }

    // Exactly one of each role, and the ids the sidecar's `load_models` names.
    let seg = diarize_model_for_role(DiarizeModelRole::Segmentation).expect("a segmentation model");
    let emb = diarize_model_for_role(DiarizeModelRole::Embedding).expect("an embedding model");
    assert_eq!(seg.id, "sherpa-onnx-pyannote-segmentation-3-0");
    assert_eq!(emb.id, "wespeaker-en-voxceleb-campplus");
    assert_eq!(diarize_model(&seg.id).map(|m| m.id.as_str()), Some(seg.id.as_str()));
    assert!(diarize_model("no-such-model").is_none());
}

/// The one structural difference from every other catalog entry in this file:
/// the segmentation model is published ONLY as a `.tar.bz2`, so its entry
/// carries the extraction contract and the embedding entry does not.
#[test]
fn only_the_segmentation_entry_declares_an_archive() {
    let seg = diarize_model_for_role(DiarizeModelRole::Segmentation).expect("a segmentation model");
    let archive = seg
        .archive
        .as_ref()
        .expect("the segmentation entry must declare an archive");
    assert_eq!(archive.kind, ArchiveKind::TarBz2);
    assert!(seg.file.filename.ends_with(".tar.bz2"), "{}", seg.file.filename);
    assert!(
        archive.extracted_path.ends_with(".onnx"),
        "the extracted file must be the graph: {}",
        archive.extracted_path
    );
    // The SECOND hash gate: the archive's bytes are verified before
    // extraction, the extracted graph's bytes after.
    assert_eq!(archive.extracted_sha256.len(), 64);
    assert!(is_lower_hex(&archive.extracted_sha256));
    assert!(archive.extracted_size_bytes > 0);
    assert!(
        archive.extracted_size_bytes < seg.file.size_bytes * 4,
        "an extracted size that dwarfs the archive is a red flag, not a catalog entry"
    );

    let emb = diarize_model_for_role(DiarizeModelRole::Embedding).expect("an embedding model");
    assert!(emb.archive.is_none(), "the embedding model ships as a plain .onnx");
    assert!(emb.file.filename.ends_with(".onnx"), "{}", emb.file.filename);
}

#[test]
fn download_urls_are_the_pinned_mirror_and_nothing_else() {
    for m in diarize_models() {
        let urls = diarize_download_urls(m);
        assert_eq!(
            urls,
            vec![format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                m.repo, m.revision, m.file.filename
            )],
            "{}: exactly one URL, the pinned mirror",
            m.id
        );
        // O6 closed against runtime downloads from the vendor's release page.
        assert!(!urls[0].contains("github.com"), "{}: vendor release URL", m.id);
        assert!(urls[0].contains(&m.revision), "{}: URL does not pin the revision", m.id);
    }
}

#[test]
fn installed_paths_live_under_the_models_dir() {
    let seg = diarize_model_for_role(DiarizeModelRole::Segmentation).expect("segmentation");
    let emb = diarize_model_for_role(DiarizeModelRole::Embedding).expect("embedding");

    // The archive unpacks into its own subdirectory — it brings a LICENSE, the
    // upstream export scripts and a quantized sibling, none of which belong
    // loose next to the GGUFs.
    assert!(diarize_extract_dir().starts_with(models_dir()));
    assert_eq!(
        diarize_model_path(seg),
        diarize_extract_dir().join(&seg.archive.as_ref().unwrap().extracted_path)
    );
    // The plain .onnx installs exactly like a polish GGUF.
    assert_eq!(diarize_model_path(emb), models_dir().join(&emb.file.filename));
    assert!(diarize_model_path(emb).starts_with(models_dir()));
}

/// Plan finding #19: the original schema hardcoded a 512-dim embedding, which is
/// wrong for every model this backlog ships. The fix is that the dimension comes
/// from the loaded graph at `load_models` time — so a number in the catalog
/// would re-create exactly the second source of truth the finding removed.
#[test]
fn the_catalog_never_states_an_embedding_dimension() {
    let src = include_str!("../src/catalog.json");
    assert!(
        !src.contains("embedding_dim"),
        "the embedding dimension is reported by the sidecar at load time, never written here"
    );
}

/// The combined download the Notetaker's model-missing copy has to state
/// (YV132's row 13). Computed from the catalog so the copy and the bytes can
/// never disagree — 7.0 MB + 29.3 MB, the REAL asset sizes.
#[test]
fn combined_download_size_is_the_real_asset_sizes() {
    assert_eq!(diarize_download_bytes(), 6_958_444 + 29_292_684);
    let mb = diarize_download_bytes() as f64 / 1_000_000.0;
    assert!((36.0..37.0).contains(&mb), "unexpected combined size: {mb:.1} MB");
}
