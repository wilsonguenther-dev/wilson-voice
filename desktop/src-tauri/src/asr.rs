//! Local ASR client.
//!
//! Prefer a **warm daemon** (`asr_worker.py --serve`) so MLX weights stay mapped.
//! Cold one-shot spawn is fallback only.

use parking_lot::Mutex;
use serde::Deserialize;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

/// Per-request response timeouts. A hung worker can't block dictation forever;
/// on timeout the daemon is killed + respawned. Split by command so an
/// interactive transcribe surfaces a stuck decode quickly, while a first-run
/// model download (which happens off the hot path during preload) gets a long
/// leash instead of being mis-killed mid-download.
const TRANSCRIBE_TIMEOUT_SECS: u64 = 30;
const PRELOAD_TIMEOUT_SECS: u64 = 900;
const CONTROL_TIMEOUT_SECS: u64 = 15; // ping / status / unknown

#[derive(Debug, Deserialize)]
pub struct AsrResult {
    pub ok: bool,
    pub text: Option<String>,
    pub backend: Option<String>,
    pub seconds: Option<f64>,
    pub error: Option<String>,
}

pub struct AsrOutput {
    pub text: String,
    pub backend: String,
    pub seconds: f64,
}

struct WarmDaemon {
    child: Child,
    stdin: ChildStdin,
    /// Lines from a dedicated reader thread, so reads can be bounded by a timeout.
    rx: Receiver<io::Result<String>>,
    python: PathBuf,
    worker: PathBuf,
}

static DAEMON: OnceLock<Mutex<Option<WarmDaemon>>> = OnceLock::new();

fn daemon_slot() -> &'static Mutex<Option<WarmDaemon>> {
    DAEMON.get_or_init(|| Mutex::new(None))
}

fn asr_env_home() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("WilsonVoice")
}

fn refuse_desktop(label: &str, p: &str) -> Result<(), String> {
    if p.contains("/Desktop/") || p.contains("/Desktop") {
        return Err(format!(
            "Refusing ASR path on Desktop ({label}={p}). Reinstall Yap so ASR uses Application Support."
        ));
    }
    Ok(())
}

fn apply_env(cmd: &mut Command, home: &Path, model: Option<&str>) {
    let cache = home.join("cache");
    let hf = cache.join("huggingface");
    let tmp = home.join("tmp");
    let _ = std::fs::create_dir_all(&hf);
    let _ = std::fs::create_dir_all(&tmp);
    cmd.current_dir(home)
        .env(
            "HOME",
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")),
        )
        .env("HF_HOME", &hf)
        .env("HUGGINGFACE_HUB_CACHE", hf.join("hub"))
        .env("TRANSFORMERS_CACHE", hf.join("transformers"))
        .env("XDG_CACHE_HOME", &cache)
        .env("TMPDIR", &tmp)
        .env("TEMP", &tmp)
        .env("TMP", &tmp)
        // YV20/M4: the app promise is "nothing leaves this machine" — never send
        // HuggingFace usage telemetry, on any launch.
        .env("HF_HUB_DISABLE_TELEMETRY", "1")
        .env_remove("PYTHONPATH")
        .env("PYTHONUNBUFFERED", "1");

    // YV20/M4: once the model weights are already in the HF cache, go fully
    // offline so a *warm* launch makes ZERO huggingface.co calls (no metadata
    // revalidation). Only the very first download — when the snapshot is absent —
    // is allowed to reach the network, so first-run model download still works.
    let hub = hf.join("hub");
    let offline = match model {
        Some(m) => hf_snapshot_present(&hub, m),
        None => hf_hub_has_snapshot(&hub),
    };
    if offline {
        cmd.env("HF_HUB_OFFLINE", "1");
    }
}

/// HF hub cache directory name for a repo id: `"org/name"` → `"models--org--name"`.
fn hf_repo_dir(model: &str) -> String {
    format!("models--{}", model.replace('/', "--"))
}

/// A snapshots dir counts as populated when it holds at least one revision subdir
/// (i.e. a completed download), meaning the weights are already local.
fn snapshots_populated(snapshots: &Path) -> bool {
    match std::fs::read_dir(snapshots) {
        Ok(entries) => entries.filter_map(|e| e.ok()).any(|e| e.path().is_dir()),
        Err(_) => false,
    }
}

/// True when a completed snapshot for `model` exists under the HF hub cache
/// (`<hub>/models--…/snapshots/<rev>/`).
fn hf_snapshot_present(hub: &Path, model: &str) -> bool {
    if model.is_empty() {
        return false;
    }
    snapshots_populated(&hub.join(hf_repo_dir(model)).join("snapshots"))
}

/// True when the HF hub cache already holds ANY model snapshot. Used only when
/// the target model isn't known (e.g. a bare control ping) — offline is safe then.
fn hf_hub_has_snapshot(hub: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(hub) else {
        return false;
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("models--"))
        .any(|e| snapshots_populated(&e.path().join("snapshots")))
}

fn spawn_daemon(python: &Path, worker: &Path, model: Option<&str>) -> Result<WarmDaemon, String> {
    refuse_desktop("python", &python.to_string_lossy())?;
    refuse_desktop("worker", &worker.to_string_lossy())?;
    if !python.exists() {
        return Err(format!(
            "ASR Python missing at {} — Permissions → Install local ASR.",
            python.display()
        ));
    }
    if !worker.exists() {
        return Err(format!("ASR worker missing: {}", worker.display()));
    }

    let home = asr_env_home();
    let mut cmd = Command::new(python);
    cmd.arg(worker)
        .arg("--serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    apply_env(&mut cmd, &home, model);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn warm ASR daemon: {e}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "daemon stdin missing".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "daemon stdout missing".to_string())?;

    // Read stdout on a dedicated thread → channel, so request_line can bound the
    // wait with recv_timeout instead of blocking forever on a hung worker. The
    // thread exits on EOF (child died) or when the receiver is dropped (respawn).
    let (tx, rx) = mpsc::channel::<io::Result<String>>();
    thread::Builder::new()
        .name("wv-asr-reader".into())
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        })
        .map_err(|e| format!("spawn asr reader: {e}"))?;

    // Wait for the ready line (bounded — the daemon prints it right after startup).
    match rx.recv_timeout(Duration::from_secs(15)) {
        Ok(Ok(ready)) => {
            if !ready.contains("ready") && !ready.contains("\"ok\": true") {
                log::warn!("daemon ready line unexpected: {ready}");
            }
        }
        Ok(Err(e)) => return Err(format!("daemon ready read: {e}")),
        Err(_) => return Err("daemon did not signal ready in time".into()),
    }
    log::info!("warm ASR daemon up pid={}", child.id());

    Ok(WarmDaemon {
        child,
        stdin,
        rx,
        python: python.to_path_buf(),
        worker: worker.to_path_buf(),
    })
}

fn request_line(daemon: &mut WarmDaemon, req: &serde_json::Value) -> Result<AsrResult, String> {
    let line = format!("{req}\n");
    daemon
        .stdin
        .write_all(line.as_bytes())
        .map_err(|e| format!("daemon write: {e}"))?;
    daemon
        .stdin
        .flush()
        .map_err(|e| format!("daemon flush: {e}"))?;

    // Bounded wait — a hung worker can't block dictation forever. On timeout the
    // error propagates and with_daemon kills + respawns the daemon. Timeout is
    // chosen per command (transcribe = short/interactive, preload = long/download).
    let timeout_secs = match req.get("cmd").and_then(|c| c.as_str()) {
        Some("transcribe") => TRANSCRIBE_TIMEOUT_SECS,
        Some("preload") => PRELOAD_TIMEOUT_SECS,
        _ => CONTROL_TIMEOUT_SECS,
    };
    let resp = match daemon.rx.recv_timeout(Duration::from_secs(timeout_secs)) {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => return Err(format!("daemon read: {e}")),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(format!("daemon read timed out after {timeout_secs}s"))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err("daemon reader disconnected (worker died)".into())
        }
    };
    if resp.trim().is_empty() {
        return Err("daemon returned empty response".into());
    }
    // YV20/M2: never embed the raw response — it carries the transcript body.
    // Log the parse error + payload size only, never the text.
    serde_json::from_str(resp.trim())
        .map_err(|e| format!("daemon JSON: {e} (payload {} bytes)", resp.trim().len()))
}

fn with_daemon<F, T>(python: &Path, worker: &Path, model: Option<&str>, f: F) -> Result<T, String>
where
    F: FnOnce(&mut WarmDaemon) -> Result<T, String>,
{
    let slot = daemon_slot();
    let mut guard = slot.lock();

    // Restart if missing or dead
    let need_spawn = match guard.as_mut() {
        None => true,
        Some(d) => match d.child.try_wait() {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => true,
        },
    };
    if need_spawn {
        if let Some(mut old) = guard.take() {
            let _ = old.stdin.write_all(b"{\"cmd\":\"quit\"}\n");
            let _ = old.child.kill();
            let _ = old.child.wait();
        }
        *guard = Some(spawn_daemon(python, worker, model)?);
    }

    let daemon = guard.as_mut().unwrap();
    match f(daemon) {
        Ok(v) => Ok(v),
        Err(e) => {
            // Kill broken daemon so next call respawns
            log::warn!("daemon request failed, resetting: {e}");
            if let Some(mut old) = guard.take() {
                let _ = old.child.kill();
                let _ = old.child.wait();
            }
            Err(e)
        }
    }
}

/// Background preload so first Dictate is not cold.
pub fn preload_async(python: PathBuf, worker: PathBuf, model: String, language: String) {
    std::thread::spawn(move || {
        // Small delay so UI/hotkeys register first
        std::thread::sleep(Duration::from_millis(600));
        match with_daemon(&python, &worker, Some(&model), |d| {
            let req = serde_json::json!({
                "cmd": "preload",
                "model": model,
                "language": language,
            });
            request_line(d, &req)
        }) {
            Ok(r) if r.ok => log::info!(
                "ASR preload ok backend={} secs={:?}",
                r.backend.unwrap_or_default(),
                r.seconds
            ),
            Ok(r) => log::warn!("ASR preload soft-fail: {:?}", r.error),
            Err(e) => log::warn!("ASR preload error: {e}"),
        }
        // Keepalive ping so we know daemon is still up after load
        let _ = with_daemon(&python, &worker, Some(&model), |d| {
            request_line(d, &serde_json::json!({"cmd": "ping"}))
        });
    });
}

pub fn run_asr(
    python: &Path,
    worker: &Path,
    wav: &Path,
    model: &str,
    language: &str,
    vocab: &[String],
) -> Result<AsrOutput, String> {
    refuse_desktop("python", &python.to_string_lossy())?;
    refuse_desktop("worker", &worker.to_string_lossy())?;
    refuse_desktop("wav", &wav.to_string_lossy())?;

    // Prefer warm daemon. `vocab` biases decoding toward the user's terms
    // (Whisper initial_prompt); the worker joins it most-frequent-last.
    let warm = with_daemon(python, worker, Some(model), |d| {
        let req = serde_json::json!({
            "cmd": "transcribe",
            "wav": wav.to_string_lossy(),
            "model": model,
            "language": language,
            "vocab": vocab,
        });
        request_line(d, &req)
    });

    let parsed = match warm {
        Ok(p) => p,
        Err(e) => {
            log::warn!("warm daemon failed ({e}); one-shot fallback");
            run_asr_oneshot(python, worker, wav, model, language)?
        }
    };

    if !parsed.ok {
        return Err(parsed.error.unwrap_or_else(|| "ASR failed".into()));
    }
    let text = parsed.text.unwrap_or_default();
    if text.trim().is_empty() {
        return Err("Empty transcript".into());
    }
    Ok(AsrOutput {
        text,
        backend: parsed.backend.unwrap_or_else(|| "mlx".into()),
        seconds: parsed.seconds.unwrap_or(0.0),
    })
}

fn run_asr_oneshot(
    python: &Path,
    worker: &Path,
    wav: &Path,
    model: &str,
    language: &str,
) -> Result<AsrResult, String> {
    if !python.exists() {
        return Err(format!(
            "ASR Python missing at {} — open Permissions → Install local ASR.",
            python.display()
        ));
    }
    let home = asr_env_home();
    let mut cmd = Command::new(python);
    cmd.arg(worker)
        .arg(wav)
        .arg("--model")
        .arg(model)
        .arg("--language")
        .arg(language);
    apply_env(&mut cmd, &home, Some(model));
    let output = cmd
        .output()
        .map_err(|e| format!("failed to spawn ASR worker: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim().starts_with('{'))
        .unwrap_or(stdout.trim());
    // YV20/M2: stdout carries the transcript body — never log it. Report the
    // parse error + stdout size only; stderr is worker diagnostics (no transcript).
    serde_json::from_str(line).map_err(|e| {
        format!(
            "bad ASR JSON: {e} (stdout {} bytes)\nstderr={stderr}",
            stdout.len()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_dir_maps_slashes() {
        assert_eq!(
            hf_repo_dir("mlx-community/whisper-large-v3-turbo"),
            "models--mlx-community--whisper-large-v3-turbo"
        );
        assert_eq!(hf_repo_dir("bare"), "models--bare");
    }

    #[test]
    fn offline_only_when_snapshot_present() {
        // Isolated temp hub cache — no snapshot yet → must stay ONLINE so the
        // first-run download still works.
        let hub = std::env::temp_dir().join(format!("yv20-hub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&hub);
        let model = "mlx-community/whisper-large-v3-turbo";
        assert!(!hf_snapshot_present(&hub, model), "no cache dir → online");
        assert!(!hf_hub_has_snapshot(&hub), "empty hub → online");

        // Materialize a completed snapshot revision → now offline is safe.
        let rev = hub
            .join(hf_repo_dir(model))
            .join("snapshots")
            .join("deadbeef");
        std::fs::create_dir_all(&rev).unwrap();
        assert!(hf_snapshot_present(&hub, model), "populated snapshot → offline");
        assert!(hf_hub_has_snapshot(&hub), "hub has a model → offline");
        // An unknown/other model is still treated as not-cached (allow download).
        assert!(!hf_snapshot_present(&hub, "other/model"));
        assert!(!hf_snapshot_present(&hub, ""));

        let _ = std::fs::remove_dir_all(&hub);
    }
}
