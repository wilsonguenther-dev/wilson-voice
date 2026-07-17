//! ASR paths that never touch ~/Desktop during dictation.
//!
//! macOS TCC: if the app execs a Python on the Desktop, it prompts
//! "access files in your Desktop folder" on every transcription. That is
//! unacceptable UX. Worker + venv live under Application Support instead.

use std::path::{Path, PathBuf};
use std::process::Command;

const WORKER_SOURCE: &str = include_str!("../../../python/asr_worker.py");

fn support_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("WilsonVoice")
}

/// Ensure asr_worker.py exists under Application Support (no Desktop read).
pub fn ensure_worker() -> PathBuf {
    let dir = support_dir();
    let _ = std::fs::create_dir_all(&dir);
    let dest = dir.join("asr_worker.py");
    // Rewrite if missing or older/shorter than embedded (simple freshness)
    let need_write = match std::fs::metadata(&dest) {
        Ok(m) => m.len() as usize != WORKER_SOURCE.len(),
        Err(_) => true,
    };
    if need_write {
        if let Err(e) = std::fs::write(&dest, WORKER_SOURCE) {
            log::error!("failed to write asr_worker.py: {e}");
        } else {
            log::info!("seeded ASR worker at {}", dest.display());
        }
    }
    dest
}

/// Python interpreters that do **not** require Desktop folder permission.
pub fn resolve_python() -> PathBuf {
    let support = support_dir();
    let candidates = [
        support.join("venv/bin/python"),
        support.join("venv/bin/python3"),
        // Homebrew / system — not on Desktop
        PathBuf::from("/opt/homebrew/bin/python3.13"),
        PathBuf::from("/opt/homebrew/bin/python3.12"),
        PathBuf::from("/opt/homebrew/bin/python3"),
        PathBuf::from("/usr/local/bin/python3"),
        PathBuf::from("/usr/bin/python3"),
    ];
    for c in &candidates {
        if c.is_file() {
            return c.clone();
        }
    }
    // Last resort: PATH lookup (still not Desktop)
    if let Ok(out) = Command::new("/usr/bin/which").arg("python3").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() && !p.contains("/Desktop/") {
                return PathBuf::from(p);
            }
        }
    }
    // Placeholder path for error messages
    support.join("venv/bin/python")
}

pub fn resolve_paths() -> (PathBuf, PathBuf) {
    let worker = ensure_worker();
    let python = resolve_python();
    log::info!(
        "ASR paths python={} worker={} (Desktop never used in hot path)",
        python.display(),
        worker.display()
    );
    (python, worker)
}

/// One-time: create Application Support venv and install mlx-whisper.
/// Optionally seed from Desktop monorepo venv **only if user already granted Desktop**.
pub fn setup_local_venv() -> Result<String, String> {
    let support = support_dir();
    let venv = support.join("venv");
    let py_out = venv.join("bin/python");

    if py_out.is_file() {
        // Verify mlx available
        let check = Command::new(&py_out)
            .args(["-c", "import mlx_whisper; print('ok')"])
            .output()
            .map_err(|e| e.to_string())?;
        if check.status.success() {
            return Ok(format!("ASR venv ready at {}", venv.display()));
        }
    }

    let bootstrap = [
        "/opt/homebrew/bin/python3.13",
        "/opt/homebrew/bin/python3.12",
        "/opt/homebrew/bin/python3",
        "/usr/bin/python3",
    ]
    .into_iter()
    .find(|p| Path::new(p).is_file())
    .ok_or_else(|| "No system python3 found (install Homebrew python3)".to_string())?;

    let _ = std::fs::remove_dir_all(&venv);
    let status = Command::new(bootstrap)
        .args(["-m", "venv", venv.to_str().unwrap()])
        .status()
        .map_err(|e| format!("venv create failed: {e}"))?;
    if !status.success() {
        return Err("python -m venv failed".into());
    }

    let pip = venv.join("bin/pip");
    let status = Command::new(&pip)
        .args(["install", "-U", "pip", "mlx-whisper"])
        .status()
        .map_err(|e| format!("pip install failed: {e}"))?;
    if !status.success() {
        return Err("pip install mlx-whisper failed (need network once)".into());
    }

    ensure_worker();
    Ok(format!(
        "Created local ASR venv at {} with mlx-whisper",
        venv.display()
    ))
}

pub fn python_has_mlx(python: &Path) -> bool {
    Command::new(python)
        .args(["-c", "import mlx_whisper"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
