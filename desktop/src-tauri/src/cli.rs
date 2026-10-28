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

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

    let export = crate::write_meeting_markdown(&db, &id)?;
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
}
