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

use crate::{asr_engine, dictation, models, record, transcription::TranscriptionManager};

/// Flag that switches the binary into headless transcribe mode.
const TRANSCRIBE_FILE_FLAG: &str = "--transcribe-file";
/// YV93 — ask for the ALIGNMENT too, printed as one JSON object on stdout.
/// Diagnostics still go to stderr, so stdout stays machine-readable.
const TIMED_FLAG: &str = "--timed";
/// Y7-A — after the transcript, run it through the cleanup pipeline at the
/// SHIPPED default settings (`AppSettings::default()`) and print ONE JSON object
/// `{level, mode, stages, raw, cleaned}` on stdout instead of the bare text.
/// This is what lets `scripts/smoke-headless.sh` show the formatting the app
/// would apply, from the built binary, with no window.
const CLEANUP_FLAG: &str = "--cleanup";
/// YV93 — the `Capabilities()` spike (plan finding #11): load the shipped model
/// and print what it says it can do, `max_timestamp_kind` and `max_audio_ms`
/// above all. No decode, no audio.
const CAPABILITIES_FLAG: &str = "--asr-capabilities";
/// Download progress is reported to stderr at most once per this many bytes —
/// stdout stays clean so it holds ONLY the transcript.
const PROGRESS_STEP: u64 = 8 * 1024 * 1024;

/// Inspect argv for a headless command. `Some(exit_code)` means the command was
/// handled and the process must exit WITHOUT starting the Tauri app; `None`
/// means a normal GUI launch.
pub fn handle_args() -> Option<i32> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == CAPABILITIES_FLAG) {
        return Some(print_capabilities());
    }
    let timed = argv.iter().any(|a| a == TIMED_FLAG);
    let cleanup = argv.iter().any(|a| a == CLEANUP_FLAG);
    if cleanup && timed {
        eprintln!("{CLEANUP_FLAG} and {TIMED_FLAG} are mutually exclusive");
        return Some(2);
    }
    // yap-lifecycle DO 6 — `--soak <runs>` with `--transcribe-file`: one
    // process, `runs` full load → transcribe → unload cycles (the lifecycle the
    // app repeats for days: lazy load at a take, idle unload later).
    let soak = match soak_runs(&argv) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("{e}");
            return Some(2);
        }
    };
    let mut args = argv.into_iter();
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
        if let Some(runs) = soak {
            return Some(soak_transcribe(Path::new(&wav), runs));
        }
        return Some(transcribe_file(Path::new(&wav), timed, cleanup));
    }
    None
}

/// `wilson-voice --asr-capabilities` — the YV93 spike, as a command anyone can
/// re-run when the shipped model changes.
fn print_capabilities() -> i32 {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();
    match ensure_model().and_then(|(id, path)| {
        let engine = asr_engine::load(&path)?;
        Ok((id, asr_engine::capabilities(&engine)))
    }) {
        Ok((id, caps)) => {
            println!("model: {id}");
            println!("{}", caps.report());
            0
        }
        Err(e) => {
            eprintln!("capabilities probe failed: {e}");
            1
        }
    }
}

/// Run one headless transcription, printing the text to stdout. Diagnostics go
/// to stderr; the exit code is 0 on success, 1 on failure.
fn transcribe_file(wav: &Path, timed: bool, cleanup: bool) -> i32 {
    // Warnings only, on stderr — `RUST_LOG=info` turns on the engine's own logs
    // without ever polluting the transcript on stdout.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();
    match run_transcribe(wav, timed) {
        Ok(text) if cleanup => {
            println!("{}", cleanup_report(&text));
            0
        }
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

/// Y7-A — run `raw` through the cleanup pipeline exactly as a live take with the
/// SHIPPED default settings would (`AppSettings::default()`: level, mode,
/// structure switch, tone dial), and return one JSON object. Headless means no
/// user dictionary, no pauses, no paste target and no LLM stage (`High` would
/// report it skipped), the same "do not depend on app state" rule the
/// transcription itself follows.
fn cleanup_report(raw: &str) -> String {
    let settings = crate::AppSettings::default();
    let level = dictation::CleanupLevel::from_setting(&settings.cleanup_level);
    let mode = dictation::resolve_mode_with_context(&settings.dictation_mode, "", None, None);
    let structure_format = dictation::structure_format_for_take(
        settings.structure_enabled && settings.dictation_mode.trim() != "plain",
        &settings.structure_app_overrides,
        "",
        mode,
    );
    let style = crate::polish::PolishConfig::from_settings(&settings, mode).style;
    let (cleaned, trace) = dictation::run_cleanup_structured_traced(
        raw,
        level,
        mode,
        structure_format,
        style,
        &[],
        |t| t.to_string(),
        |_| Err("headless"),
    );
    serde_json::json!({
        "level": settings.cleanup_level,
        "mode": crate::polish::mode_tag(mode),
        "stages": trace.stages_that_ran(),
        "raw": raw,
        "cleaned": cleaned,
    })
    .to_string()
}

const SOAK_FLAG: &str = "--soak";

/// Parse `--soak <runs>` / `--soak=<runs>`. `Ok(None)` when absent.
fn soak_runs(argv: &[String]) -> Result<Option<u32>, String> {
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        let v = if a == SOAK_FLAG {
            it.next().cloned()
        } else if let Some(v) = a.strip_prefix(&format!("{SOAK_FLAG}=")) {
            Some(v.to_string())
        } else {
            continue;
        };
        return match v.as_deref().map(str::parse::<u32>) {
            Some(Ok(n)) if n > 0 => Ok(Some(n)),
            _ => Err(format!("{SOAK_FLAG} requires a positive run count")),
        };
    }
    Ok(None)
}

/// The headless soak: `runs` load → transcribe → unload cycles of the SAME
/// `TranscriptionManager` the app holds. After each cycle it prints
/// `soak iter=<n>` on stdout and then blocks for one line on stdin, so the
/// soak script can sample this pid's RSS with `ps` at a quiet point (EOF on
/// stdin means "don't wait"). Exit 0 only when every cycle produced text.
fn soak_transcribe(wav: &Path, runs: u32) -> i32 {
    use std::io::{BufRead, Write};
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();
    let run = || -> Result<(), String> {
        if !wav.is_file() {
            return Err(format!("no such wav file: {}", wav.display()));
        }
        let samples = record::read_wav_16k_mono(wav)?;
        let (model_id, model_path) = ensure_model()?;
        let manager = TranscriptionManager::new();
        let stdin = std::io::stdin();
        let mut line = String::new();
        for i in 1..=runs {
            manager.load(&model_id, &model_path)?;
            let text = manager.transcribe(samples.clone(), None, None)?;
            if text.trim().is_empty() {
                return Err(format!("iteration {i}: empty transcript"));
            }
            manager.unload();
            println!("soak iter={i} chars={}", text.trim().len());
            let _ = std::io::stdout().flush();
            line.clear();
            let _ = stdin.lock().read_line(&mut line);
        }
        Ok(())
    };
    match run() {
        Ok(()) => {
            println!("soak done runs={runs}");
            0
        }
        Err(e) => {
            eprintln!("soak failed: {e}");
            1
        }
    }
}

fn run_transcribe(wav: &Path, timed: bool) -> Result<String, String> {
    if !wav.is_file() {
        return Err(format!("no such wav file: {}", wav.display()));
    }
    let samples = record::read_wav_16k_mono(wav)?;
    let (model_id, model_path) = ensure_model()?;
    // Same warm-engine lifecycle the app uses (off-main load, panic containment,
    // bounded transcribe) — one process, one transcription, then exit.
    let manager = TranscriptionManager::new();
    manager.load(&model_id, &model_path)?;
    // Autodetect the language and decode unbiased: the gate must transcribe the
    // same way on any machine, so it deliberately does NOT read the app's
    // language setting or its dictionary (unlike the model selection, which
    // mirrors the app on purpose).
    if timed {
        // YV93: the same decode with its alignment kept. Printed as JSON so the
        // eval harness scores the MODEL's timestamps rather than the harness's
        // own arithmetic over window starts.
        let transcript = manager.transcribe_timed(samples, None, None)?;
        if transcript.text.trim().is_empty() {
            return Err("empty transcript".into());
        }
        return serde_json::to_string(&transcript).map_err(|e| format!("encode transcript: {e}"));
    }
    let text = manager.transcribe(samples, None, None)?;
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

    /// Y7-A — the `--cleanup` report runs at the level the app SHIPS with, and
    /// it really runs the pipeline: the raw text is echoed verbatim and the
    /// cleaned text differs from it (the spoken filler is dropped). Fails if the
    /// report ever hard-codes a level or stops running the stages.
    #[test]
    fn cleanup_report_uses_the_shipped_default_level_and_formats() {
        let raw = "um the quick brown fox jumps over the lazy dog";
        let report: serde_json::Value = serde_json::from_str(&cleanup_report(raw)).unwrap();
        assert_eq!(
            report["level"].as_str().unwrap(),
            crate::AppSettings::default().cleanup_level
        );
        assert_eq!(report["raw"].as_str().unwrap(), raw);
        let cleaned = report["cleaned"].as_str().unwrap();
        assert_ne!(cleaned, raw, "cleanup changed nothing: {report}");
        assert_eq!(cleaned, "the quick brown fox jumps over the lazy dog");
        assert!(report["stages"].as_str().unwrap().contains("backtrack"));
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
