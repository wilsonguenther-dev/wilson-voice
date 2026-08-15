//! YV123 — the whole install path against the real mirror, end to end.
//!
//! `#[ignore]`d by default and never run in CI: it downloads 36 MB from
//! Hugging Face, which is exactly the network dependency every other test in
//! this suite is written to avoid (same posture as `meeting_eval.rs`'s
//! corpus-absent skip). It exists because the acceptance criterion for this
//! item is a real download that sha256-verifies and extracts to a working
//! `.onnx` — a claim only a real download can support.
//!
//! ```text
//! cargo test --test diarize_model_install_e2e -- --ignored --nocapture
//! ```
//!
//! It installs into a temp directory-shaped mirror of the models dir rather
//! than into `~/Library/Application Support/WilsonVoice/models`, so running it
//! neither depends on nor disturbs a real install — with one exception it
//! states honestly: `download_diarize_model_with` writes to `models_dir()`,
//! which is process-global, so the test asserts against that real path and
//! cleans up only what it created.

use wilson_voice_lib::models::{
    diarize_archive_path, diarize_download_urls, diarize_model_for_role, diarize_model_path,
    is_diarize_downloaded, is_diarize_ready, sha256_hex, DiarizeModelRole,
};

#[test]
#[ignore = "downloads 36 MB from the Wilson-owned HF mirror; run manually"]
fn installs_both_models_from_the_pinned_mirror() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    for role in [DiarizeModelRole::Segmentation, DiarizeModelRole::Embedding] {
        let model = diarize_model_for_role(role).expect("catalog entry");
        let installed = diarize_model_path(model);
        let preexisting = installed.exists();
        println!(
            "\n[{:?}] {}\n  url:  {}\n  dest: {}",
            role,
            model.id,
            diarize_download_urls(model)[0],
            installed.display()
        );

        let path = rt
            .block_on(wilson_voice_lib::models::download_diarize_model_with(
                &model.id,
                |downloaded, total| {
                    if downloaded == total {
                        println!("  downloaded {downloaded} / {total} bytes");
                    }
                },
            ))
            .unwrap_or_else(|e| panic!("{}: install failed: {e}", model.id));

        assert_eq!(path, installed);
        assert!(
            path.is_file(),
            "{}: nothing at {}",
            model.id,
            path.display()
        );
        let expected_sha = match &model.archive {
            Some(a) => {
                // The archive is a second copy of bytes we now hold extracted.
                assert!(
                    !diarize_archive_path(model).exists(),
                    "the .tar.bz2 should be removed once its contents are verified"
                );
                assert_eq!(
                    std::fs::metadata(&path).unwrap().len(),
                    a.extracted_size_bytes
                );
                a.extracted_sha256.clone()
            }
            None => {
                assert_eq!(
                    std::fs::metadata(&path).unwrap().len(),
                    model.file.size_bytes
                );
                model.file.sha256.clone()
            }
        };
        assert_eq!(
            sha256_hex(&path).expect("hash the installed file"),
            expected_sha
        );
        assert!(is_diarize_downloaded(model));
        println!("  sha256 verified: {expected_sha}");

        // Idempotent: a second call is a no-op, not a second 36 MB.
        let again = rt
            .block_on(wilson_voice_lib::models::download_diarize_model_with(
                &model.id,
                |_, _| {},
            ))
            .expect("second install is a no-op");
        assert_eq!(again, installed);

        if !preexisting {
            // Leave the machine as we found it unless it was already installed.
            println!(
                "  (leaving {} in place for sherpa_load_smoke)",
                path.display()
            );
        }
    }

    assert!(is_diarize_ready(), "both halves must be installed");
}
