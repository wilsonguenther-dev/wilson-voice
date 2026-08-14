//! YV93 — the `Capabilities()` spike (plan finding #11), as a command rather
//! than a one-off.
//!
//! Finding #11: *"`transcribe_cpp`'s `max_timestamp_kind` is per-model and
//! nobody has confirmed what the shipped Parakeet build returns."* The chunker's
//! whole primary-dedupe path depends on the answer, so the answer has to be
//! obtainable by anyone at any time — including after a catalog bump that moves
//! to a different GGUF.
//!
//! ```sh
//! cargo run --bin wilson-voice -- --asr-capabilities
//! ```
//!
//! prints, among others, the two lines this file asserts the shape of:
//!
//! ```text
//! max_timestamp_kind: <none|segment|word|token>
//! max_audio_ms: <integer>
//! ```
//!
//! With no model downloaded the probe would fetch one, which is not something a
//! test should do on a cold machine — so this test runs it only when a model is
//! already on disk, and prints a skip line otherwise. The FORMAT of the report
//! is unit-tested in `asr_engine` and needs no model at all.

use std::path::PathBuf;
use std::process::Command;

use wilson_voice_lib::models;

const NO_MODEL: &str = "no ASR model downloaded, skipping the capabilities probe";

fn downloaded_model() -> Option<PathBuf> {
    models::catalog()
        .models
        .iter()
        .find(|m| models::is_downloaded(m))
        .and_then(models::model_path)
}

#[test]
fn the_capabilities_probe_prints_max_timestamp_kind_and_max_audio_ms() {
    if downloaded_model().is_none() {
        println!("{NO_MODEL}");
        return;
    }
    let out = Command::new(env!("CARGO_BIN_EXE_wilson-voice"))
        .arg("--asr-capabilities")
        .output()
        .expect("the probe runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "probe failed ({}): {}",
        out.status,
        stderr.trim()
    );
    println!("{}", stdout.trim());

    let kind = stdout
        .lines()
        .find_map(|l| l.strip_prefix("max_timestamp_kind: "))
        .expect("a max_timestamp_kind line");
    assert!(
        ["none", "segment", "word", "token"].contains(&kind.trim()),
        "unexpected timestamp kind '{kind}'"
    );
    let audio_ms = stdout
        .lines()
        .find_map(|l| l.strip_prefix("max_audio_ms: "))
        .expect("a max_audio_ms line");
    audio_ms
        .trim()
        .parse::<i64>()
        .unwrap_or_else(|e| panic!("max_audio_ms '{audio_ms}' is not an integer: {e}"));
}

/// The other half of the spike: whatever the model says its ceiling is, the
/// shipped chunk geometry has to be inside it. `max_audio_ms == 0` is the
/// crate's "no practical limit" sentinel.
#[test]
fn the_chunk_geometry_fits_inside_the_models_audio_ceiling() {
    if downloaded_model().is_none() {
        println!("{NO_MODEL}");
        return;
    }
    let out = Command::new(env!("CARGO_BIN_EXE_wilson-voice"))
        .arg("--asr-capabilities")
        .output()
        .expect("the probe runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let max_audio_ms: i64 = stdout
        .lines()
        .find_map(|l| l.strip_prefix("max_audio_ms: "))
        .expect("a max_audio_ms line")
        .trim()
        .parse()
        .expect("an integer");
    if max_audio_ms == 0 {
        return; // no practical limit
    }
    let widest_ms =
        (wilson_voice_lib::meeting_asr::ChunkConfig::default().max_decode_seconds() * 1000.0) as i64;
    assert!(
        widest_ms <= max_audio_ms,
        "the widest chunk ({widest_ms} ms) is longer than the model accepts ({max_audio_ms} ms)"
    );
}
