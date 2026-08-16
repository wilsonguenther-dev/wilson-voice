//! YV122 acceptance — both models really open, and the embedding width is the
//! one the model reports.
//!
//! ```sh
//! cargo test --test sherpa_load_smoke -- --nocapture
//! ```
//!
//! # The measurement this file exists for, and the correction it forced
//!
//! Audit finding #19 says the schema's `embedding_dim DEFAULT 512` is wrong
//! because "CAM++ (the recommended model) emits 192-dim embeddings", and this
//! backlog's YV122 spec carries that forward as its first acceptance criterion:
//! *"asserts `embedding_dim == 192` … the first real assertion that finding
//! #19's correction holds against the actual shipped model, not just the plan's
//! arithmetic."*
//!
//! It does not hold. The shipped model reports **512**:
//!
//! ```text
//! wespeaker_en_voxceleb_CAM++.onnx                    (29,292,684 B) -> 512
//! 3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx                  -> 512
//! wespeaker_en_voxceleb_resnet34.onnx                                -> 256
//! ```
//!
//! and the model file says so about itself. Its own ONNX metadata carries
//! `output_dim: 512`, `framework: wespeaker`, `sample_rate: 16000`, next to the
//! upstream URL it was converted from — primary source, inside the bytes this
//! app will ship, not a number quoted from anywhere.
//!
//! So finding #19's *mechanism* is right and its *number* is wrong, in the
//! direction that matters: the plan's original `DEFAULT 512` was accidentally
//! correct, and asserting 192 here would have hard-coded a width no model in
//! the recommended set has. That is exactly the failure the mechanism prevents
//! — **the dimension is whatever the child measured** — and this file is the
//! first place in the epic where the mechanism was pointed at real bytes and
//! immediately caught the spec. YV128's `speaker_profiles` schema must consume
//! the reported value; it must not write 192 down either.
//!
//! The ResNet34 arm below is the non-vacuity control: same sidecar, same code
//! path, **256**. A build that answered with a constant would have to answer
//! the same number twice.

#[path = "support/diarize_models.rs"]
mod models;

use std::path::Path;

use wilson_voice_lib::diarize_protocol::{DiarizeRequest, ERR_MODEL_LOAD_FAILED};

/// The width `wespeaker_en_voxceleb_CAM++.onnx` reports, measured on the file
/// itself and cross-checked against the `output_dim` in its own ONNX metadata.
/// Not a plan number, and deliberately not 192 — see the module docs.
const CAM_PLUS_PLUS_DIM: u32 = 512;
/// The width `wespeaker_en_voxceleb_resnet34.onnx` reports. Present only so the
/// assertion above cannot be satisfied by a constant.
const RESNET34_DIM: u32 = 256;
/// The width the plan and audit finding #19 predicted. Named so the assertion
/// that it is NOT the answer is legible rather than implicit.
const PREDICTED_DIM: u32 = 192;

/// The refusal path, with **zero model bytes** — so this runs in CI, where the
/// models are not vendored yet.
///
/// Two claims, and the second is the one sherpa made newly dangerous.
///
/// 1. Bytes that are not a model produce `model_load_failed` and no width. The
///    fixture is a `.tar.bz2` header under an `.onnx` name, which is the exact
///    shape of YV123's failure mode: the vendored segmentation model ships as
///    an archive, and an extraction step that silently no-ops hands this
///    sidecar the archive.
/// 2. **stdout stays pure JSON.** sherpa-onnx's C++ core logs
///    (`speaker-embedding-extractor.cc:Validate:40 Please provide a speaker
///    embedding extractor model`). It logs to *stderr* — measured, not assumed
///    — and this is the standing check, because one such line on stdout would
///    be handed to the parent's de-multiplexer as a response.
#[test]
fn the_sidecar_refuses_bad_model_bytes_and_keeps_stdout_pure_json() {
    let dir = models::scratch("load-smoke-refusal");
    let archive = dir.join("segmentation.onnx");
    std::fs::write(&archive, b"BZh91AY&SY\x00\x00\x00\x00").expect("write fixture");

    let mut sidecar = models::Spawned::start();
    let response = sidecar.ask(&DiarizeRequest::load_models(1, &archive, &archive));
    assert_eq!(response.err_tag(), Some(ERR_MODEL_LOAD_FAILED));
    assert_eq!(response.embedding_dim, None, "a refusal reports no width");

    // …and one that is not there at all is still a different answer.
    let missing = Path::new("/nonexistent/yap-diarize-fixture/segmentation.onnx");
    let response = sidecar.ask(&DiarizeRequest::load_models(2, missing, missing));
    assert_ne!(
        response.err_tag(),
        Some(ERR_MODEL_LOAD_FAILED),
        "'not there' and 'bad bytes' are the two answers YV123 has to tell apart"
    );

    sidecar.assert_stdout_is_only_protocol();
    assert_eq!(
        sidecar.stdout_lines().len(),
        3,
        "one readiness line and one response per request, nothing else"
    );
}

/// The real thing: both models open, and the width comes off the model.
///
/// Skips with [`models::MODELS_ABSENT`] until YV123 vendors the files.
#[test]
fn sherpa_loads_both_models_and_reports_the_measured_embedding_width() {
    let Some((segmentation, embedding)) = models::models() else {
        return;
    };

    let mut sidecar = models::Spawned::start();
    let response = sidecar.ask(&DiarizeRequest::load_models(1, &segmentation, &embedding));
    assert!(
        response.ok,
        "both models must open: {:?}",
        response.err_tag()
    );
    let dim = response
        .clone()
        .into_embedding_dim()
        .expect("a successful load reports its width");
    eprintln!("MEASURED embedding_dim = {dim} for {}", embedding.display());

    assert_eq!(
        dim, CAM_PLUS_PLUS_DIM,
        "the shipped CAM++ reports {CAM_PLUS_PLUS_DIM}; its own ONNX metadata \
         carries `output_dim: {CAM_PLUS_PLUS_DIM}`"
    );
    assert_ne!(
        dim, PREDICTED_DIM,
        "audit finding #19 predicted {PREDICTED_DIM}. It is wrong about this \
         model, and the value above is what the file says about itself — which \
         is the entire reason the width is REPORTED by the child rather than \
         written down on the Rust side"
    );

    // Non-vacuity: a different model, a different number, same code path.
    let alt = models::models_root().join(models::EMBEDDING_ALT_RELATIVE);
    if alt.is_file() {
        let response = sidecar.ask(&DiarizeRequest::load_models(2, &segmentation, &alt));
        let alt_dim = response
            .into_embedding_dim()
            .expect("the alternative model loads too");
        eprintln!("MEASURED embedding_dim = {alt_dim} for {}", alt.display());
        assert_eq!(alt_dim, RESNET34_DIM);
        assert_ne!(
            alt_dim, dim,
            "two models, two widths — a sidecar answering with a constant \
             cannot pass both halves of this test"
        );
    } else {
        eprintln!(
            "non-vacuity control absent ({}), skipping the second-width arm",
            alt.display()
        );
    }

    // The production path is also the one that must not pollute stdout.
    sidecar.assert_stdout_is_only_protocol();
}
