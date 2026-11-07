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
//!
//! yap24-NT9 adds `--transcribe-meeting <mic.wav> [--sys <sys.wav>]`: the
//! headless half of the notetaker proof. It files the WAVs as a meeting row in a
//! SCRATCH state root (`YAP_DATA_DIR` is required, like `--smoke`), runs the
//! shipped meeting pipeline worker over them, writes notes through the same
//! notes pass the Meetings card uses when the notes model is installed, exports
//! the Markdown through the same writer, and prints one JSON report on stdout.
//! `scripts/notetaker-e2e.sh` is the gate that drives it.
//!
//! SP-03 adds `--bench <wav> [<wav>…] [--no-cleanup]`: the dictation pipeline
//! a released take runs — release-side DSP + VAD, the warm decode, the cleanup
//! pipeline (the LLM polish is always skipped headless) and no paste — timed
//! stage by stage over each file, printed as ONE JSON object on stdout. It
//! benches the model the app would select (the shipped default on a fresh data
//! dir), found installed or under `YAP_MODEL_DIR`, and NEVER downloads one: no
//! model is exit 2 with one sentence.
//! `scripts/qa/speed-bench.sh` drives it and keeps the history in
//! docs/BUDGETS.md (`speed_bench`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{dictation, models, record, transcription::TranscriptionManager};

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
/// yap24-NT11 — headless AEC3 pre-pass over two WAVs on disk.
const AEC_PREPASS_FLAG: &str = "--aec-prepass";
/// yap-call-0 — `wilson-voice --probe-calls` prints the current
/// `call_signals::CallSnapshot` as JSON and exits. It reads, never records.
/// Window titles are PRIVATE: each is `{ pattern_id, len }`, never the text.
const PROBE_CALLS_FLAG: &str = "--probe-calls";
/// Developer-only, at a terminal: also print each browser window title's text.
/// `scripts/call-walk.sh` NEVER passes it, and nothing in the app does.
const RAW_TITLES_FLAG: &str = "--raw-titles";

fn run_aec_prepass(args: &[String]) -> i32 {
    let [mic, sys, out] = args else {
        eprintln!("usage: {AEC_PREPASS_FLAG} <mic.wav> <sys.wav> <out.wav>");
        return 2;
    };
    let (mic, sys, out) = (Path::new(mic), Path::new(sys), Path::new(out));
    let lag = crate::meeting_aec::aec_coupling(mic, sys);
    match crate::meeting_aec::aec_prepass(mic, sys, out) {
        Ok(report) => {
            let body = serde_json::json!({ "coupling_lag_ms": lag, "report": report });
            println!("{body}");
            0
        }
        Err(e) => {
            eprintln!("aec pre-pass failed: {e}");
            1
        }
    }
}

/// Download progress is reported to stderr at most once per this many bytes —
/// stdout stays clean so it holds ONLY the transcript.
const PROGRESS_STEP: u64 = 8 * 1024 * 1024;

/// Inspect argv for a headless command. `Some(exit_code)` means the command was
/// handled and the process must exit WITHOUT starting the Tauri app; `None`
/// means a normal GUI launch.
pub fn handle_args() -> Option<i32> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| {
        a == TRANSCRIBE_MEETING_FLAG || a.starts_with(&format!("{TRANSCRIBE_MEETING_FLAG}="))
    }) {
        return Some(transcribe_meeting_cmd(&argv));
    }
    // yap24-NT11 — `--aec-prepass <mic.wav> <sys.wav> <out.wav>`: the meeting
    // AEC3 pre-pass on audio on disk, for scripts/eval/aec-real-call.sh. Prints
    // ONE JSON object of numbers (coupling lag, attenuation, wall time) on
    // stdout; never transcript text.
    if argv.first().map(String::as_str) == Some(AEC_PREPASS_FLAG) {
        return Some(run_aec_prepass(&argv[1..]));
    }
    if argv.iter().any(|a| a == CAPABILITIES_FLAG) {
        return Some(print_capabilities());
    }
    if argv.iter().any(|a| a == PROBE_CALLS_FLAG) {
        return Some(probe_calls(argv.iter().any(|a| a == RAW_TITLES_FLAG)));
    }
    // SP-03 — `--bench <wav…>`: per-stage ms + RTF as JSON, no paste.
    match bench_args(&argv) {
        Ok(Some(bench)) => return Some(run_bench(&bench)),
        Ok(None) => {}
        Err(e) => {
            eprintln!("{e}");
            return Some(2);
        }
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
    match ensure_model()
        .and_then(|(id, path)| Ok((id, crate::engine_owner::probe_capabilities(&path)?)))
    {
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

/// Whose Accessibility grant a CLI run reports. TCC attributes a process
/// started from a terminal to the terminal (the responsible process), so a run
/// with a tty, or with the terminal's environment, reports TERMINAL's
/// `AXIsProcessTrusted`, not Yap's.
fn cli_ax_source() -> crate::call_signals::AxSource {
    use crate::call_signals::AxSource;
    // SAFETY: isatty on the standard descriptors has no preconditions.
    let tty = unsafe { libc::isatty(0) == 1 || libc::isatty(1) == 1 || libc::isatty(2) == 1 };
    if tty || std::env::var_os("TERM_PROGRAM").is_some() {
        AxSource::Terminal
    } else {
        AxSource::Unknown
    }
}

/// `wilson-voice --probe-calls [--raw-titles]` — one CallSnapshot as JSON on
/// stdout. Exit 0 printed, 1 the snapshot could not be taken (below 14.4, the
/// process list unreadable), 2 not macOS.
fn probe_calls(raw_titles: bool) -> i32 {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();
    #[cfg(target_os = "macos")]
    {
        use crate::call_signals;
        let started = Instant::now();
        let ax_source = cli_ax_source();
        let snap = call_signals::sys::snapshot(call_signals::catalog(), ax_source, raw_titles, None);
        match snap.and_then(|s| {
            serde_json::to_string_pretty(&s)
                .map(|json| (s, json))
                .map_err(|e| e.to_string())
        }) {
            Ok((s, json)) => {
                println!("{json}");
                log::info!(
                    "call_signals probe_calls procs={} apps={} ax_source={:?} titles={} raw_titles={raw_titles} ms={}",
                    s.procs.len(),
                    s.running_apps.len(),
                    s.ax_source,
                    s.browser_titles.as_ref().map_or(-1, |t| t.len() as i64),
                    started.elapsed().as_millis()
                );
                0
            }
            Err(e) => {
                log::error!("call_signals probe_calls_failed error={e:?}");
                1
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (raw_titles, cli_ax_source());
        log::error!("call_signals probe_calls_failed error=\"macOS only\"");
        2
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
/// `--bench` never comes through here: it uses [`bench_model`], which cannot
/// download.
fn ensure_model() -> Result<(String, PathBuf), String> {
    if let Some(model) = selected_model() {
        if models::is_downloaded(model) {
            let path = models::model_path(model)
                .ok_or_else(|| format!("catalog model '{}' lists no files", model.id))?;
            eprintln!("using selected model {}", model.id);
            return Ok((model.id.clone(), path));
        }
    }
    let (id, path) = resolve_model(
        ModelFetch::Allow,
        models::smallest_model(),
        None,
        &models::models_dir(),
        &crate::data_dir(),
        download_catalog_model,
    )?;
    eprintln!("using model {id}");
    Ok((id, path))
}

/// Fetch one catalog model (resumable, sha256-verified) into the active
/// models dir, with progress on stderr. Reached only through
/// [`resolve_model`] under [`ModelFetch::Allow`].
fn download_catalog_model(model: &models::CatalogModel) -> Result<PathBuf, String> {
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
    ))
}

/// Whether a model lookup may fetch a missing model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelFetch {
    /// `--transcribe-file` and friends: a cold machine still completes.
    Allow,
    /// `--bench`: measure what is installed, never install anything.
    Never,
}

/// SP-03 fix — the bench measures, it never installs: with no model on disk a
/// bench run used to pull ~46 MB into whatever data dir was active (Wilson's
/// real one when `YAP_DATA_DIR` is unset).
const BENCH_MODEL_FETCH: ModelFetch = ModelFetch::Never;
/// A folder that already holds catalog model files (the loop's shared cache),
/// so a scratch `YAP_DATA_DIR` can bench the shipped model with no copy and no
/// download. Same variable the meeting evals read.
const MODEL_DIR_ENV: &str = "YAP_MODEL_DIR";
/// Exit code when `--bench` finds no installed model (and fetched nothing).
const EXIT_BENCH_NO_MODEL: i32 = 2;

/// `model`'s default-quant file when it is installed in full — under
/// `override_dir` first, then the app's `models_dir` — by the same full-size
/// check [`models::is_downloaded`] makes.
fn installed_model_file(
    model: &models::CatalogModel,
    override_dir: Option<&Path>,
    models_dir: &Path,
) -> Option<PathBuf> {
    let file = model.default_file()?;
    override_dir
        .into_iter()
        .chain(std::iter::once(models_dir))
        .map(|dir| dir.join(&file.filename))
        .find(|p| {
            std::fs::metadata(p)
                .map(|m| m.is_file() && m.len() == file.size_bytes)
                .unwrap_or(false)
        })
}

/// Find `model` on disk; when it is missing, `download` it only under
/// [`ModelFetch::Allow`]. Under [`ModelFetch::Never`] the answer is ONE
/// sentence naming the data dir by basename (never a full path), and
/// `download` is never called.
fn resolve_model<D>(
    fetch: ModelFetch,
    model: &models::CatalogModel,
    override_dir: Option<&Path>,
    models_dir: &Path,
    data_dir: &Path,
    download: D,
) -> Result<(String, PathBuf), String>
where
    D: FnOnce(&models::CatalogModel) -> Result<PathBuf, String>,
{
    if let Some(path) = installed_model_file(model, override_dir, models_dir) {
        return Ok((model.id.clone(), path));
    }
    match fetch {
        ModelFetch::Allow => download(model).map(|path| (model.id.clone(), path)),
        ModelFetch::Never => {
            let filename = model
                .default_file()
                .map(|f| f.filename.as_str())
                .unwrap_or("its model file");
            Err(format!(
                "bench refused: {} is not installed in data dir {} (set {MODEL_DIR_ENV} to a \
                 folder holding {filename}) and --bench never downloads a model",
                model.id,
                basename(data_dir)
            ))
        }
    }
}

/// PURE. The model `--bench` times: the one the app would select — the
/// settings' `native_model`, else the shipped default (the catalog's
/// recommendation, `AppSettings::default().native_model`), never the smallest.
fn bench_candidate(
    selected: Option<&'static models::CatalogModel>,
) -> &'static models::CatalogModel {
    selected.unwrap_or_else(models::recommended_model)
}

/// The installed model `--bench` runs on, honouring `YAP_MODEL_DIR`. Never a
/// download ([`BENCH_MODEL_FETCH`]).
fn bench_model() -> Result<(String, PathBuf), String> {
    let override_dir = std::env::var_os(MODEL_DIR_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    resolve_model(
        BENCH_MODEL_FETCH,
        bench_candidate(selected_model()),
        override_dir.as_deref(),
        &models::models_dir(),
        &crate::data_dir(),
        download_catalog_model,
    )
}

// ── SP-03 · `--bench` ──────────────────────────────────────────────────────────

/// Flag that switches the binary into the speed bench.
const BENCH_FLAG: &str = "--bench";
/// Skip the cleanup pipeline (decode-only numbers).
const BENCH_NO_CLEANUP_FLAG: &str = "--no-cleanup";

/// Parsed `--bench` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BenchArgs {
    wavs: Vec<PathBuf>,
    cleanup: bool,
}

/// `--bench <wav> [<wav>…]`: every argument after the flag up to the next
/// `--flag` is a WAV. `Ok(None)` when the flag is absent; an error when it is
/// given no file, so a typo never benches nothing and exits 0.
fn bench_args(argv: &[String]) -> Result<Option<BenchArgs>, String> {
    let Some(at) = argv.iter().position(|a| a == BENCH_FLAG) else {
        return Ok(None);
    };
    let wavs: Vec<PathBuf> = argv[at + 1..]
        .iter()
        .take_while(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .collect();
    if wavs.is_empty() {
        return Err(format!("{BENCH_FLAG} requires at least one .wav file"));
    }
    Ok(Some(BenchArgs {
        wavs,
        cleanup: !argv.iter().any(|a| a == BENCH_NO_CLEANUP_FLAG),
    }))
}

/// Lower-case words, punctuation dropped (apostrophes kept), for scoring.
fn normalized_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Word error rate of `hypothesis` against `reference`: word-level edit
/// distance over the reference length. `0.0` is a perfect match; an empty
/// reference scores `0.0` for an empty hypothesis and `1.0` otherwise.
fn word_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let r = normalized_words(reference);
    let h = normalized_words(hypothesis);
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    // One rolling row of the Levenshtein table over words.
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    let mut cur = vec![0usize; h.len() + 1];
    for (i, rw) in r.iter().enumerate() {
        cur[0] = i + 1;
        for (j, hw) in h.iter().enumerate() {
            let sub = prev[j] + usize::from(rw != hw);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[h.len()] as f64 / r.len() as f64
}

/// The reference transcript beside a fixture: `<stem>.txt` next to `<stem>.wav`.
fn reference_text(wav: &Path) -> Option<String> {
    std::fs::read_to_string(wav.with_extension("txt")).ok()
}

/// Milliseconds as an integer, from an `Instant`.
fn ms_since(t: Instant) -> i64 {
    t.elapsed().as_millis() as i64
}

/// One file through the release-side pipeline on the WARM engine. Returns the
/// take's JSON row. The release clock starts once the leveled samples exist —
/// in the app that leveling streams during capture, before the key is up.
#[allow(clippy::too_many_arguments)]
fn bench_take(
    manager: &TranscriptionManager,
    model_id: &str,
    model_path: &Path,
    settings: &crate::AppSettings,
    vad: Option<&crate::vad::WarmVad>,
    wav: &Path,
    cleanup: bool,
    window: &mut crate::latency::RtfWindow,
) -> Result<serde_json::Value, String> {
    if !wav.is_file() {
        return Err(format!("no such wav file: {}", basename(wav)));
    }
    let raw = record::read_wav_16k_mono(wav)?;
    let audio_s = raw.len() as f64 / f64::from(record::TARGET_RATE);
    let leveled = record::bench_capture_chain(&raw);
    drop(raw);
    // SP-05 — the streaming decode the app runs WHILE a take records, over the
    // same leveled audio, reported next to the batch numbers.
    let stream_row = bench_stream(manager, model_id, model_path, settings, &leveled, wav);

    // SP-06 — the fade, RNNoise and Silero stream during capture in the app,
    // so the bench streams them here, before the release clock, too.
    let streamed = record::bench_capture_stream(&leveled, settings.denoise, vad);
    let t_release = Instant::now();
    let dsp = record::bench_release_streamed(&leveled, settings.denoise, vad, streamed)?;
    let dsp_ms = ms_since(t_release);
    let decoded_s = dsp.samples.len() as f64 / f64::from(record::TARGET_RATE);
    let asr = crate::transcribe_native(
        manager,
        model_id,
        model_path,
        dsp.samples,
        &settings.language_prefs(),
        None,
        &mut crate::transcribe_progress::NoObserver,
    )?;
    let t_cleanup = Instant::now();
    if cleanup {
        // The shipped default cleanup (see `cleanup_report`); the LLM polish
        // stage is never reached headless.
        let _ = cleanup_report(&asr.text);
    }
    let cleanup_ms = ms_since(t_cleanup);
    let release_text_ms = ms_since(t_release);

    let rtf = crate::latency::decode_rtf(asr.decode_ms, decoded_s);
    window.push(rtf);
    let release_rtf = crate::latency::decode_rtf(release_text_ms, audio_s);
    let wer = reference_text(wav).map(|r| word_error_rate(&r, &asr.text));
    log::info!(
        "bench take file={} audio_s={audio_s:.1} release_text_ms={release_text_ms} \
         asr_decode_ms={} rtf={rtf:.3}",
        basename(wav),
        asr.decode_ms
    );
    Ok(serde_json::json!({
        "file": basename(wav),
        "audio_s": (audio_s * 10.0).round() / 10.0,
        "decoded_s": (decoded_s * 10.0).round() / 10.0,
        "voiced_s": (dsp.voiced_seconds * 10.0).round() / 10.0,
        "speech_present": dsp.speech_present,
        "dsp_ms": dsp_ms,
        "asr_load_ms": asr.load_ms,
        "asr_decode_ms": asr.decode_ms,
        "cleanup_ms": cleanup_ms,
        "release_text_ms": release_text_ms,
        "rtf": rtf,
        "release_rtf": release_rtf,
        "chunks": asr.chunks,
        "chunks_failed": asr.chunks_failed,
        "degraded": asr.degraded,
        "words": normalized_words(&asr.text).len(),
        "wer": wer,
        "stream": stream_row,
    }))
}

/// SP-05 — a bench window decoder: the same pinned interactive decode the app
/// streams with.
struct BenchWindowDecoder {
    manager: TranscriptionManager,
    model_id: String,
    model_path: PathBuf,
    language: String,
}

impl crate::stream_decode::WindowDecoder for BenchWindowDecoder {
    fn decode(&self, samples: Vec<f32>) -> Result<String, String> {
        self.manager.transcribe_window_on(
            &self.model_id,
            &self.model_path,
            samples,
            Some(self.language.clone()),
            None,
        )
    }
}

/// SP-05 — stream `leveled` through [`crate::stream_decode::StreamDecoder`]
/// as capture would, then time the release (in-flight window + tail only).
///
/// Paced on the decode: after each committed window the feed waits for it to
/// finish, which is what real time looks like whenever decode runs faster than
/// speech (the measured RTF is ~20x). `release_text_ms` is the number the SP-05
/// targets are written against (≤ 300 ms at 240 s, ≤ 150 ms at 20 s);
/// `wer_vs_batch` is computed by the caller's reference when one exists.
fn bench_stream(
    manager: &TranscriptionManager,
    model_id: &str,
    model_path: &Path,
    settings: &crate::AppSettings,
    leveled: &[f32],
    wav: &Path,
) -> serde_json::Value {
    use crate::stream_decode::{StreamConfig, StreamDecoder, StreamOutcome, WindowDecoder};
    let prefs = settings.language_prefs();
    let Some(language) =
        crate::stream_decode::stream_language(&prefs.primary, prefs.set.len(), prefs.auto)
    else {
        return serde_json::json!({ "outcome": "skipped", "reason": "auto_language" });
    };
    let decoder: Arc<dyn WindowDecoder> = Arc::new(BenchWindowDecoder {
        manager: manager.clone(),
        model_id: model_id.to_string(),
        model_path: model_path.to_path_buf(),
        language,
    });
    let take = crate::stream_decode::next_take_id();
    let mut stream = StreamDecoder::start(take, StreamConfig::default(), decoder, None);
    let capture = Instant::now();
    // 10 ms blocks, the capture consumer's order of magnitude.
    for block in leveled.chunks(160) {
        stream.push(block, 1.0);
        while stream.backlog() > 0 && !stream.is_degraded() {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    stream.seal(leveled.len());
    let capture_ms = ms_since(capture);
    let t_release = Instant::now();
    let outcome = stream.finish();
    let release_text_ms = ms_since(t_release);
    match outcome {
        StreamOutcome::Streamed(s) => {
            let wer = reference_text(wav).map(|r| word_error_rate(&r, &s.text));
            log::info!(
                "bench stream file={} windows={} release_text_ms={release_text_ms} tail_s={:.2}",
                basename(wav),
                s.windows,
                s.tail_seconds
            );
            serde_json::json!({
                "outcome": "streamed",
                "windows": s.windows,
                "tail_s": (s.tail_seconds * 100.0).round() / 100.0,
                "tail_ms": s.tail_ms,
                "release_text_ms": release_text_ms,
                "capture_decode_ms": s.capture_decode_ms,
                "capture_ms": capture_ms,
                "words": normalized_words(&s.text).len(),
                "wer": wer,
            })
        }
        StreamOutcome::NotUsed(reason) => serde_json::json!({
            "outcome": "not_used",
            "reason": reason,
            "release_text_ms": release_text_ms,
        }),
        StreamOutcome::Degraded(reason) => serde_json::json!({
            "outcome": "degraded",
            "reason": reason,
            "release_text_ms": release_text_ms,
        }),
    }
}

/// `wilson-voice --bench <wav…>` — load the model once (reported as
/// `cold_load_ms`), then run every file through [`bench_take`] on the warm
/// engine and print ONE JSON object. Exit 0 only when every file produced
/// text; a failed file keeps its row with an `error` and no numbers. Exit 2
/// (one stderr sentence, nothing fetched) when the model is not installed.
fn run_bench(args: &BenchArgs) -> i32 {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();
    // The SHIPPED defaults (denoise, language set), never the user's settings —
    // the same "do not depend on app state" rule `--cleanup` follows. The model
    // is the one the app would select, installed (or under `YAP_MODEL_DIR`) —
    // never downloaded.
    let settings = crate::AppSettings::default();
    let (model_id, model_path) = match bench_model() {
        Ok(m) => m,
        Err(e) => {
            log::warn!("bench model_missing fetch=never");
            eprintln!("{e}");
            return EXIT_BENCH_NO_MODEL;
        }
    };
    eprintln!("bench using installed model {model_id}");
    log::info!("bench model_found model={model_id}");
    let manager = TranscriptionManager::new();
    let t_load = Instant::now();
    if let Err(e) = manager.load(&model_id, &model_path) {
        log::warn!("bench model_load_failed");
        eprintln!("bench failed: {e}");
        return 1;
    }
    let cold_load_ms = ms_since(t_load);
    // Silero only when its model is already on disk — never a download. The
    // VAD is the one the app warms at launch; its absence is the energy-VAD
    // path the app also runs, and the JSON says which one was timed.
    let vad_path = crate::vad::model_path(&crate::data_dir());
    let vad = if vad_path.is_file() {
        crate::vad::WarmVad::load(&vad_path).ok()
    } else {
        None
    };
    let mut window = crate::latency::RtfWindow::new();
    let mut takes = Vec::with_capacity(args.wavs.len());
    let mut failed = 0usize;
    for wav in &args.wavs {
        match bench_take(
            &manager,
            &model_id,
            &model_path,
            &settings,
            vad.as_ref(),
            wav,
            args.cleanup,
            &mut window,
        ) {
            Ok(row) => takes.push(row),
            Err(e) => {
                failed += 1;
                log::warn!("bench take_failed file={}", basename(wav));
                takes.push(serde_json::json!({ "file": basename(wav), "error": e }));
            }
        }
    }
    let report = serde_json::json!({
        "bench": "sp-03",
        "model": model_id,
        "backend": "native",
        "cold_load_ms": cold_load_ms,
        "vad": if vad.is_some() { "silero" } else { "energy" },
        "denoise": settings.denoise,
        "cleanup": args.cleanup,
        "polish": "skipped",
        "paste": false,
        "rtf_p50": window.p50(),
        "rtf_last": window.last(),
        "takes": takes,
    });
    println!("{report}");
    log::info!(
        "bench done takes={} failed={failed} cold_load_ms={cold_load_ms}",
        args.wavs.len()
    );
    i32::from(failed > 0)
}

// ── yap24-NT9 · `--transcribe-meeting` ─────────────────────────────────────────

/// Switches the binary into the headless meeting proof.
const TRANSCRIBE_MEETING_FLAG: &str = "--transcribe-meeting";
/// The optional second (system-audio) track of a two-track meeting.
const SYS_FLAG: &str = "--sys";
/// The optional meeting title (default "Headless meeting").
const TITLE_FLAG: &str = "--title";
/// The `source` column a CLI-filed meeting carries, so it can never be mistaken
/// for a recorded one.
const CLI_MEETING_SOURCE: &str = "cli";
/// A two-hour meeting decodes in well under this on any supported Mac; past it
/// the worker is wedged and the proof fails rather than hangs.
const MEETING_TIMEOUT: Duration = Duration::from_secs(45 * 60);
/// EX_CONFIG — the run was refused before it touched anything.
const EXIT_REFUSED: i32 = 78;

/// `--flag value` / `--flag=value`. `Ok(None)` when the flag is absent; `Err`
/// when it is present with no value (or another flag where the value belongs).
fn flag_value(argv: &[String], flag: &str) -> Result<Option<String>, String> {
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        if a == flag {
            return match it.next() {
                Some(v) if !v.starts_with("--") && !v.trim().is_empty() => Ok(Some(v.clone())),
                _ => Err(format!("{flag} requires a value")),
            };
        }
        if let Some(v) = a.strip_prefix(&format!("{flag}=")) {
            if v.trim().is_empty() {
                return Err(format!("{flag} requires a value"));
            }
            return Ok(Some(v.to_string()));
        }
    }
    Ok(None)
}

/// PURE. Why a `--transcribe-meeting` run must not start, if it must not: the
/// proof writes a meeting row, segments, notes and an export, so it only ever
/// runs against a scratch root — never the real meeting history.
fn meeting_root_refusal(env_root: Option<&str>) -> Option<String> {
    let paths = crate::app_paths::resolve(env_root);
    if !paths.overridden {
        return Some(format!(
            "{TRANSCRIBE_MEETING_FLAG} requires {}=<a scratch state root>; it is unset, so this run \
             would write into the real meeting history. Refusing to start.",
            crate::DATA_DIR_ENV
        ));
    }
    if crate::app_paths::is_default_root(&paths.root) {
        return Some(format!(
            "{TRANSCRIBE_MEETING_FLAG} was given {} = the default state root; the proof never \
             touches the real meeting history. Refusing to start.",
            crate::DATA_DIR_ENV
        ));
    }
    None
}

/// What one headless meeting run produced — printed as one JSON object.
#[derive(Debug)]
struct MeetingReport {
    id: String,
    state: String,
    duration_seconds: f64,
    segments_mic: usize,
    segments_sys: usize,
    tracks: usize,
    asr_model: Option<String>,
    /// The #250 notes phase the run ended in (`NotesPhase::as_str`):
    /// `notes_ready` when the notes model is installed and wrote notes,
    /// `notes_queued` when it is not (the job waits for the model — the
    /// headless proof never downloads it), `notes_failed` otherwise.
    notes: &'static str,
    notes_error: Option<String>,
    summary: bool,
    export: PathBuf,
    transcribe_ms: u64,
    notes_ms: u64,
}

impl MeetingReport {
    fn passed(&self) -> bool {
        self.state == crate::meetings::MeetingState::Complete.as_str()
            && self.segments_mic > 0
            && (self.tracks < 2 || self.segments_sys > 0)
            && self.notes != crate::summary_jobs::NotesPhase::Failed.as_str()
    }

    fn json(&self) -> String {
        serde_json::json!({
            "id": self.id,
            "state": self.state,
            "durationSeconds": self.duration_seconds,
            "tracks": self.tracks,
            "segmentsMic": self.segments_mic,
            "segmentsSys": self.segments_sys,
            "asrModel": self.asr_model,
            "notes": self.notes,
            "notesError": self.notes_error,
            "summary": self.summary,
            "export": self.export.display().to_string(),
            "transcribeMs": self.transcribe_ms,
            "notesMs": self.notes_ms,
        })
        .to_string()
    }
}

fn basename(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn transcribe_meeting_cmd(argv: &[String]) -> i32 {
    // Our own lines (and the diagnostics durations) at INFO on stderr; every
    // other module stays at WARN so stdout holds ONLY the JSON report.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(
        "warn,wilson_voice_lib::cli=info,wilson_voice_lib::diagnostics=info",
    ))
    .try_init();
    if let Some(refusal) = meeting_root_refusal(std::env::var(crate::DATA_DIR_ENV).ok().as_deref()) {
        log::error!("cli transcribe_meeting_refused reason=data_dir — {refusal}");
        return EXIT_REFUSED;
    }
    let parsed = (|| -> Result<(PathBuf, Option<PathBuf>, String), String> {
        let mic = flag_value(argv, TRANSCRIBE_MEETING_FLAG)?
            .ok_or_else(|| format!("{TRANSCRIBE_MEETING_FLAG} requires a path to the mic .wav"))?;
        let sys = flag_value(argv, SYS_FLAG)?.map(PathBuf::from);
        let title = flag_value(argv, TITLE_FLAG)?.unwrap_or_else(|| "Headless meeting".into());
        Ok((PathBuf::from(mic), sys, title))
    })();
    let (mic, sys, title) = match parsed {
        Ok(v) => v,
        Err(e) => {
            log::error!("cli transcribe_meeting_refused reason=usage — {e}");
            return 2;
        }
    };
    match run_meeting(&mic, sys.as_deref(), &title) {
        Ok(report) => {
            println!("{}", report.json());
            if report.passed() {
                log::info!(
                    "cli transcribe_meeting_finish id={} state={} segments_mic={} segments_sys={} notes={} transcribe_ms={} notes_ms={}",
                    report.id, report.state, report.segments_mic, report.segments_sys,
                    report.notes, report.transcribe_ms, report.notes_ms
                );
                0
            } else {
                log::error!(
                    "cli transcribe_meeting_failed id={} state={} segments_mic={} segments_sys={} notes={}",
                    report.id, report.state, report.segments_mic, report.segments_sys, report.notes
                );
                1
            }
        }
        Err(e) => {
            log::error!("cli transcribe_meeting_failed reason={e}");
            1
        }
    }
}

fn wav_seconds(path: &Path) -> Result<f64, String> {
    if !path.is_file() {
        return Err(format!("no such wav file: {}", basename(path)));
    }
    let samples = record::read_wav_16k_mono(path)?;
    Ok(samples.len() as f64 / 16_000.0)
}

/// The pipeline the app installs at launch (lib.rs), minus what a headless run
/// has no use for: no dictation competes for the engine (`NoDemand`), nothing
/// listens to progress but the log, and notes run synchronously afterwards
/// instead of on the notes worker. ASR resolution is the app's own
/// (`resolve_meeting_model` over the pinned model); the Silero VAD is used when
/// its file is in the scratch root and the fixed clock otherwise — never a
/// download.
fn headless_deps(
    db: Arc<crate::db::Database>,
    progress_dir: PathBuf,
    manager: TranscriptionManager,
) -> crate::meeting_pipeline::PipelineDeps {
    use crate::{meeting_asr, meeting_pipeline};
    let vad_path = crate::vad::model_path(&crate::data_dir());
    meeting_pipeline::PipelineDeps {
        db,
        progress_dir,
        asr: Arc::new(move || {
            let pinned = models::recommended_model();
            let (model_id, model_path) = meeting_pipeline::resolve_meeting_model(
                &pinned.id,
                "",
                "en",
                &crate::native_model_ready,
            )?;
            Ok(meeting_pipeline::MeetingEngine::new(
                model_id.clone(),
                Box::new(meeting_asr::WarmEngineChunkAsr::new(
                    manager.clone(),
                    model_id,
                    model_path,
                    Some("en".to_string()),
                    None,
                )),
            ))
        }),
        vad: Arc::new(move || {
            if !vad_path.is_file() {
                return Err("no Silero VAD in the scratch root".to_string());
            }
            crate::vad::WarmVad::load(&vad_path)
                .map(|v| Arc::new(v) as Arc<dyn meeting_asr::VoiceActivity + Send + Sync>)
        }),
        demand: Arc::new(meeting_asr::NoDemand),
        quit: Arc::new(|| false),
        sink: Arc::new(|e: &meeting_pipeline::MeetingProgressEvent| {
            log::debug!("cli meeting_progress state={} percent={}", e.state, e.percent);
        }),
        diarize: crate::meeting_diarize::DiarizeBackend::Shipped,
        after_transcript: None,
        config: meeting_asr::MeetingAsrConfig::default(),
        start_delay: Duration::ZERO,
    }
}

fn run_meeting(mic: &Path, sys: Option<&Path>, title: &str) -> Result<MeetingReport, String> {
    use crate::meetings::{MeetingKind, MeetingState};
    use crate::meeting_pipeline::{self, MIC_TRACK, SYSTEM_TRACK};
    use crate::summary_jobs::{CancelToken, NotesPhase};

    let mic_secs = wav_seconds(mic)?;
    let sys_secs = sys.map(wav_seconds).transpose()?;
    let duration = sys_secs.map_or(mic_secs, |s| s.max(mic_secs));
    let tracks = if sys.is_some() { 2 } else { 1 };
    log::info!(
        "cli transcribe_meeting_start tracks={tracks} mic={} duration_s={duration:.1}",
        basename(mic)
    );

    let root = crate::data_dir();
    std::fs::create_dir_all(&root).map_err(|e| format!("create the scratch root: {e}"))?;
    let db = Arc::new(crate::db::Database::open(root.join("wilson_voice.db"))?);
    // A two-track meeting is a call (the other side arrives on its own track);
    // one track is a room. Stored at INSERT, as the start surface does.
    let kind = if sys.is_some() { MeetingKind::Virtual } else { MeetingKind::InPerson };
    let row = db.create_meeting_with_kind(title, CLI_MEETING_SOURCE, kind)?;
    let id = row.id.clone();
    // The inputs are COPIED into the scratch root's meetings dir under the
    // row's id, exactly where a capture lands them — the pipeline (and audio
    // retention) may treat them as the meeting's own, and a committed fixture
    // must never be one of them.
    let dir = crate::meetings_dir();
    let mic_copy = dir.join(format!("{id}.t0.wav"));
    std::fs::copy(mic, &mic_copy).map_err(|e| format!("copy {}: {e}", basename(mic)))?;
    db.finish_meeting(&id, duration, Some(&mic_copy))?;
    if let Some(sys) = sys {
        let sys_copy = dir.join(format!("{id}.t1.wav"));
        std::fs::copy(sys, &sys_copy).map_err(|e| format!("copy {}: {e}", basename(sys)))?;
        db.set_meeting_sys_wav_path(&id, Some(&sys_copy))?;
    }
    db.set_meeting_state(&id, MeetingState::Transcribing, None)?;

    // The shipped worker, spawned the way lib.rs spawns it.
    let transcribe_start = Instant::now();
    // The engine is held HERE as well as by the worker, so it can be unloaded
    // before the process exits: a model still resident at `exit()` trips
    // ggml-metal's residency-set assert in its static destructor (SIGABRT,
    // exit 134) after every line of the report was already printed.
    let manager = TranscriptionManager::new();
    let pipeline =
        meeting_pipeline::spawn(headless_deps(Arc::clone(&db), dir, manager.clone()));
    pipeline.enqueue(&id);
    let drained = pipeline.wait_idle(MEETING_TIMEOUT);
    manager.unload();
    if !drained {
        return Err(format!(
            "the meeting pipeline did not drain within {} s",
            MEETING_TIMEOUT.as_secs()
        ));
    }
    let transcribe_ms = transcribe_start.elapsed().as_millis() as u64;
    crate::diagnostics::record_duration("cli", "meeting_transcribe", transcribe_ms);

    let meeting = db.get_meeting(&id)?.ok_or("the meeting row vanished")?;
    let segments = db.list_meeting_segments(&id)?;
    let segments_mic = segments.iter().filter(|s| s.track == MIC_TRACK).count();
    let segments_sys = segments.iter().filter(|s| s.track == SYSTEM_TRACK).count();

    // Notes: through the app's own notes pass when the notes model is on disk;
    // otherwise the #250 job would sit queued for the model, which is what we
    // report (never an error, never a download).
    let notes_start = Instant::now();
    let (notes, notes_error) = if !crate::summary_jobs::transcript_settled(&meeting.state)
        || segments.is_empty()
    {
        (NotesPhase::Queued.as_str(), None)
    } else {
        match crate::summarize::plan_summary_model("", &crate::summary_model_on_disk) {
            crate::summarize::SummaryModelPlan::Download(m) => {
                log::info!("cli notes_waiting model={m} reason=not_installed");
                (NotesPhase::Queued.as_str(), None)
            }
            crate::summarize::SummaryModelPlan::Use(m) => {
                match crate::summarize_meeting_with(&db, &m, &id, &CancelToken::new()) {
                    Ok(_) => (NotesPhase::Ready.as_str(), None),
                    Err(e) => {
                        let reason = e.plain();
                        log::warn!("cli notes_failed model={m} reason={reason}");
                        (NotesPhase::Failed.as_str(), Some(reason))
                    }
                }
            }
        }
    };
    let notes_ms = notes_start.elapsed().as_millis() as u64;
    crate::diagnostics::record_duration("cli", "meeting_notes", notes_ms);

    let export = crate::write_meeting_markdown(&db, &id, &crate::data_dir())?;
    let meeting = db.get_meeting(&id)?.ok_or("the meeting row vanished")?;
    let asr_model = db.meeting_asr_model(&id)?;
    Ok(MeetingReport {
        id,
        state: meeting.state.clone(),
        duration_seconds: meeting.duration_seconds,
        segments_mic,
        segments_sys,
        tracks,
        asr_model,
        notes,
        notes_error,
        summary: meeting.summary.as_deref().is_some_and(|s| !s.trim().is_empty()),
        export: PathBuf::from(export.path),
        transcribe_ms,
        notes_ms,
    })
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

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// SP-03 — `--bench` takes every path up to the next flag, honours
    /// `--no-cleanup` anywhere, is absent without the flag, and refuses an
    /// empty file list instead of benching nothing.
    #[test]
    fn bench_args_collect_wavs_until_the_next_flag() {
        assert_eq!(bench_args(&args(&["--transcribe-file", "a.wav"])), Ok(None));
        assert_eq!(
            bench_args(&args(&["--bench", "a.wav", "b.wav", "--no-cleanup"])),
            Ok(Some(BenchArgs {
                wavs: vec![PathBuf::from("a.wav"), PathBuf::from("b.wav")],
                cleanup: false,
            }))
        );
        assert!(
            bench_args(&args(&["--bench", "c.wav"]))
                .unwrap()
                .unwrap()
                .cleanup
        );
        assert!(bench_args(&args(&["--bench"])).is_err());
        assert!(bench_args(&args(&["--bench", "--no-cleanup"])).is_err());
    }

    /// SP-03 — the bench's accuracy guard: case and punctuation never count,
    /// one substitution in four words is 0.25, insertions and deletions count.
    #[test]
    fn word_error_rate_scores_words_not_punctuation() {
        assert_eq!(word_error_rate("Hello, world.", "hello world"), 0.0);
        assert_eq!(
            word_error_rate("the quick brown fox", "the quick red fox"),
            0.25
        );
        assert_eq!(word_error_rate("one two", "one two three"), 0.5);
        assert_eq!(word_error_rate("one two", ""), 1.0);
        assert_eq!(word_error_rate("", ""), 0.0);
        assert_eq!(word_error_rate("don't stop", "Don't stop!"), 0.0);
    }

    /// yap24-NT9 — both spellings parse, a missing value is an error rather
    /// than the next flag swallowed as a path.
    #[test]
    fn meeting_flags_parse_both_spellings_and_refuse_a_missing_value() {
        let a = args(&["--transcribe-meeting", "m.wav", "--sys=s.wav"]);
        assert_eq!(flag_value(&a, TRANSCRIBE_MEETING_FLAG).unwrap().as_deref(), Some("m.wav"));
        assert_eq!(flag_value(&a, SYS_FLAG).unwrap().as_deref(), Some("s.wav"));
        assert_eq!(flag_value(&a, TITLE_FLAG).unwrap(), None);
        let bad = args(&["--transcribe-meeting", "--sys", "s.wav"]);
        assert!(flag_value(&bad, TRANSCRIBE_MEETING_FLAG).is_err());
    }

    /// yap24-NT9 — the proof refuses to run without a scratch root, and on the
    /// default root itself; a scratch root passes.
    #[test]
    fn transcribe_meeting_refuses_unset_and_default_roots() {
        assert!(meeting_root_refusal(None).is_some());
        assert!(meeting_root_refusal(Some("   ")).is_some());
        let default = crate::app_paths::resolve(None).root;
        assert!(meeting_root_refusal(Some(&default.display().to_string())).is_some());
        let scratch = std::env::temp_dir().join("yap-nt9-scratch-root");
        assert!(meeting_root_refusal(Some(&scratch.display().to_string())).is_none());
    }

    /// yap24-NT9 — the two-track meeting fixture the notetaker proof runs is
    /// committed (force-added past the `*.wav` ignore), 16 kHz mono, and small.
    #[test]
    fn two_track_meeting_fixture_is_committed_and_small() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("meeting-two-track");
        for track in ["track0.wav", "track1.wav"] {
            let wav = dir.join(track);
            let bytes = std::fs::metadata(&wav).expect("fixture committed").len();
            assert!(bytes < 300 * 1024, "{track} too big: {bytes} bytes");
            let secs = record::read_wav_16k_mono(&wav).unwrap().len() as f64 / 16_000.0;
            assert!((2.0..15.0).contains(&secs), "{track}: {secs} s");
        }
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

    /// A scratch dir for one test, removed by the caller.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-cli-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// SP-03 fix — `--bench` runs under `ModelFetch::Never`: with no model in
    /// `YAP_MODEL_DIR` or the models dir it refuses with ONE sentence naming the
    /// data dir by basename, and the downloader is never called. A wrong-size
    /// (partial) file does not count as installed either.
    #[test]
    fn bench_model_lookup_never_calls_the_downloader() {
        assert_eq!(BENCH_MODEL_FETCH, ModelFetch::Never);
        let root = scratch("bench-nodl");
        let data = root.join("yap-bench-state");
        let models_dir = data.join("models");
        let cache = root.join("cache");
        std::fs::create_dir_all(&models_dir).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        let model = bench_candidate(None);
        let file = model.default_file().expect("shipped default lists a file");
        // A truncated file in the cache is not an installed model.
        std::fs::write(cache.join(&file.filename), b"partial").unwrap();
        let mut calls = 0u32;
        let got = resolve_model(
            BENCH_MODEL_FETCH,
            model,
            Some(&cache),
            &models_dir,
            &data,
            |_| {
                calls += 1;
                Err("downloader called".into())
            },
        );
        let err = got.expect_err("no installed model must refuse");
        assert_eq!(calls, 0, "--bench called the downloader");
        assert!(err.contains("yap-bench-state"), "names the data dir: {err}");
        let root_str = root.to_string_lossy().into_owned();
        assert!(!err.contains(root_str.as_str()), "no full path: {err}");
        assert!(err.contains("never downloads"), "{err}");
        assert_eq!(err.matches(". ").count(), 0, "one sentence: {err}");
        assert!(
            std::fs::read_dir(&models_dir).unwrap().next().is_none(),
            "wrote nothing"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// SP-03 fix — an installed full-size file under `YAP_MODEL_DIR` is used
    /// as-is (still no download), and the models dir is the fallback.
    #[test]
    fn bench_model_lookup_uses_the_installed_file_first_in_the_override_dir() {
        let root = scratch("bench-found");
        let data = root.join("state");
        let models_dir = data.join("models");
        let cache = root.join("cache");
        std::fs::create_dir_all(&models_dir).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        let model = bench_candidate(None);
        let file = model.default_file().unwrap();
        // Sparse, full-size: the size check is what "installed" means.
        let f = std::fs::File::create(cache.join(&file.filename)).unwrap();
        f.set_len(file.size_bytes).unwrap();
        drop(f);
        let mut calls = 0u32;
        let (id, path) = resolve_model(
            BENCH_MODEL_FETCH,
            model,
            Some(&cache),
            &models_dir,
            &data,
            |_| {
                calls += 1;
                Err("downloader called".into())
            },
        )
        .expect("installed model is found");
        assert_eq!(calls, 0);
        assert_eq!(id, model.id);
        assert_eq!(path, cache.join(&file.filename));
        // Moved into the models dir (no override): still found there.
        std::fs::rename(cache.join(&file.filename), models_dir.join(&file.filename)).unwrap();
        let (_, path) = resolve_model(BENCH_MODEL_FETCH, model, None, &models_dir, &data, |_| {
            Err("downloader called".into())
        })
        .expect("models dir fallback");
        assert_eq!(path, models_dir.join(&file.filename));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// SP-03 fix — with no selection the bench times the SHIPPED default (the
    /// model a fresh install selects), not the smallest catalog model.
    #[test]
    fn bench_candidate_is_the_shipped_default_model() {
        assert_eq!(
            bench_candidate(None).id,
            crate::AppSettings::default().native_model
        );
    }

    /// The download path stays reachable for `--transcribe-file` (`Allow`).
    #[test]
    fn allow_fetch_calls_the_downloader_once_when_missing() {
        let root = scratch("allow-dl");
        let models_dir = root.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();
        let model = models::smallest_model();
        let mut calls = 0u32;
        let got = resolve_model(ModelFetch::Allow, model, None, &models_dir, &root, |_| {
            calls += 1;
            Ok(PathBuf::from("fetched"))
        });
        assert_eq!(calls, 1);
        assert_eq!(got.unwrap().1, PathBuf::from("fetched"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
