//! Headless CLI (YV32) — `wilson-voice --transcribe-file <wav>`.
//!
//! Modeled on Handy's headless transcribe mode: the same embedded engine the
//! app dictates with, driven from the terminal with no window, no tray, and no
//! Tauri runtime. It is the app's end-to-end gate — one command proves the real
//! pipeline (wav → 16 kHz mono samples → GGUF engine → text) works on a machine,
//! and it doubles as a scriptable transcriber.
//!
//! Model selection mirrors the app: the user's selected `native_model` when it
//! is already downloaded, otherwise the catalog's SMALLEST model, fetched once
//! (sha256-verified) so a cold machine still completes.

use std::path::{Path, PathBuf};

use crate::{models, record, transcription::TranscriptionManager};

/// Flag that switches the binary into headless transcribe mode.
const TRANSCRIBE_FILE_FLAG: &str = "--transcribe-file";
/// Download progress is reported to stderr at most once per this many bytes —
/// stdout stays clean so it holds ONLY the transcript.
const PROGRESS_STEP: u64 = 8 * 1024 * 1024;

/// Inspect argv for a headless command. `Some(exit_code)` means the command was
/// handled and the process must exit WITHOUT starting the Tauri app; `None`
/// means a normal GUI launch.
pub fn handle_args() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let wav = if arg == TRANSCRIBE_FILE_FLAG {
            match args.next() {
                Some(path) => path,
                None => {
                    eprintln!("{TRANSCRIBE_FILE_FLAG} requires a path to a .wav file");
                    return Some(2);
                }
            }
        } else if let Some(path) = arg.strip_prefix(&format!("{TRANSCRIBE_FILE_FLAG}=")) {
            path.to_string()
        } else {
            continue;
        };
        return Some(transcribe_file(Path::new(&wav)));
    }
    None
}

/// Run one headless transcription, printing the text to stdout. Diagnostics go
/// to stderr; the exit code is 0 on success, 1 on failure.
fn transcribe_file(wav: &Path) -> i32 {
    // Warnings only, on stderr — `RUST_LOG=info` turns on the engine's own logs
    // without ever polluting the transcript on stdout.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();
    match run_transcribe(wav) {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(e) => {
            eprintln!("transcribe failed: {e}");
            1
        }
    }
}

fn run_transcribe(wav: &Path) -> Result<String, String> {
    if !wav.is_file() {
        return Err(format!("no such wav file: {}", wav.display()));
    }
    let samples = record::read_wav_16k_mono(wav)?;
    let (model_id, model_path) = ensure_model()?;
    // Same warm-engine lifecycle the app uses (off-main load, panic containment,
    // bounded transcribe) — one process, one transcription, then exit.
    let manager = TranscriptionManager::new();
    manager.load(&model_id, &model_path)?;
    // Autodetect the language: the gate must transcribe the same way on any
    // machine, so it deliberately does NOT read the app's language setting
    // (unlike the model selection, which mirrors the app on purpose).
    let text = manager.transcribe(samples, None)?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("empty transcript".into());
    }
    Ok(text)
}

/// The selected `native_model` from the app's settings, if any is written yet.
fn selected_model() -> Option<&'static models::CatalogModel> {
    let path = crate::data_dir().join("settings.json");
    if !path.exists() {
        return None;
    }
    // YV41: go through the salvaging loader — a headless run must honour the
    // same selection the app shows, instead of ignoring a whole settings file
    // over one field it cannot parse.
    models::catalog_model(&crate::load_settings(&path).native_model)
}

/// Resolve a usable model file: the selection when it's on disk, else the
/// smallest catalog model — downloaded (once, verified) if it isn't there.
fn ensure_model() -> Result<(String, PathBuf), String> {
    if let Some(model) = selected_model() {
        if models::is_downloaded(model) {
            let path = models::model_path(model)
                .ok_or_else(|| format!("catalog model '{}' lists no files", model.id))?;
            eprintln!("using selected model {}", model.id);
            return Ok((model.id.clone(), path));
        }
    }
    let model = models::smallest_model();
    let path = models::model_path(model)
        .ok_or_else(|| format!("catalog model '{}' lists no files", model.id))?;
    if !models::is_downloaded(model) {
        let mb = model.default_file().map(|f| f.size_bytes).unwrap_or(0) as f64 / 1e6;
        eprintln!("downloading {} ({mb:.0} MB) — one time…", model.name);
        let mut last: u64 = 0;
        tauri::async_runtime::block_on(models::download_model_with(
            &model.id,
            |downloaded, total| {
                if downloaded >= total || downloaded.saturating_sub(last) >= PROGRESS_STEP {
                    last = downloaded;
                    let pct = if total > 0 {
                        downloaded as f64 * 100.0 / total as f64
                    } else {
                        0.0
                    };
                    eprintln!("  {pct:.0}% ({downloaded}/{total} bytes)");
                }
            },
        ))?;
    }
    eprintln!("using model {}", model.id);
    Ok((model.id.clone(), path))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain GUI launch (no flag) must never be intercepted — `cargo test`
    /// itself runs with unrelated argv, which is exactly the case covered here.
    #[test]
    fn ignores_argv_without_the_flag() {
        assert!(handle_args().is_none());
    }

    /// The fixture the E2E gate transcribes is committed, mono/16 kHz, and small.
    #[test]
    fn e2e_fixture_is_committed_and_small() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("quick-brown-fox-16k.wav");
        let bytes = std::fs::metadata(&fixture)
            .expect("fixture committed")
            .len();
        assert!(bytes < 300 * 1024, "fixture too big: {bytes} bytes");
        assert!(record::read_wav_16k_mono(&fixture).is_ok());
    }
}
