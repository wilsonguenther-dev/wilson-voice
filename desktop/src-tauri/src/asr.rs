//! Local ASR client.
//!
//! Prefer a **warm daemon** (`asr_worker.py --serve`) so MLX weights stay mapped.
//! Cold one-shot spawn is fallback only.

use parking_lot::Mutex;
use serde::Deserialize;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

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
    stdout: BufReader<std::process::ChildStdout>,
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
            "Refusing ASR path on Desktop ({label}={p}). Reinstall Wilson Voice so ASR uses Application Support."
        ));
    }
    Ok(())
}

fn apply_env(cmd: &mut Command, home: &Path) {
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
        .env_remove("PYTHONPATH")
        .env("PYTHONUNBUFFERED", "1");
}

fn spawn_daemon(python: &Path, worker: &Path) -> Result<WarmDaemon, String> {
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
    apply_env(&mut cmd, &home);

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
    let mut stdout = BufReader::new(stdout);

    // Wait for ready line
    let mut ready = String::new();
    stdout
        .read_line(&mut ready)
        .map_err(|e| format!("daemon ready read: {e}"))?;
    if !ready.contains("ready") && !ready.contains("\"ok\": true") {
        log::warn!("daemon ready line unexpected: {ready}");
    }
    log::info!("warm ASR daemon up pid={}", child.id());

    Ok(WarmDaemon {
        child,
        stdin,
        stdout,
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

    let mut resp = String::new();
    // Block until one full JSON line (model may take a few seconds first time)
    daemon
        .stdout
        .read_line(&mut resp)
        .map_err(|e| format!("daemon read: {e}"))?;
    if resp.trim().is_empty() {
        return Err("daemon returned empty response".into());
    }
    serde_json::from_str(resp.trim()).map_err(|e| format!("daemon JSON: {e} raw={resp}"))
}

fn with_daemon<F, T>(python: &Path, worker: &Path, f: F) -> Result<T, String>
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
        *guard = Some(spawn_daemon(python, worker)?);
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
        match with_daemon(&python, &worker, |d| {
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
        let _ = with_daemon(&python, &worker, |d| {
            request_line(d, &serde_json::json!({"cmd": "ping"}))
        });
    });
}

/// Lightweight health probe (spawns daemon if needed).
pub fn ping_daemon(python: &Path, worker: &Path) -> Result<bool, String> {
    let r = with_daemon(python, worker, |d| {
        request_line(d, &serde_json::json!({"cmd": "ping"}))
    })?;
    Ok(r.ok)
}

pub fn run_asr(
    python: &Path,
    worker: &Path,
    wav: &Path,
    model: &str,
    language: &str,
) -> Result<AsrOutput, String> {
    refuse_desktop("python", &python.to_string_lossy())?;
    refuse_desktop("worker", &worker.to_string_lossy())?;
    refuse_desktop("wav", &wav.to_string_lossy())?;

    // Prefer warm daemon
    let warm = with_daemon(python, worker, |d| {
        let req = serde_json::json!({
            "cmd": "transcribe",
            "wav": wav.to_string_lossy(),
            "model": model,
            "language": language,
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
    apply_env(&mut cmd, &home);
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
    serde_json::from_str(line).map_err(|e| format!("bad ASR JSON: {e}\nstdout={stdout}\nstderr={stderr}"))
}
