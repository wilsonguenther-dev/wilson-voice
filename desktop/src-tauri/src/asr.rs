use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Deserialize)]
pub struct AsrResult {
    pub ok: bool,
    pub text: Option<String>,
    pub backend: Option<String>,
    pub seconds: Option<f64>,
    pub error: Option<String>,
}

/// Caches + cwd must stay off ~/Desktop or macOS spams "access Desktop folder".
fn asr_env_home() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("WilsonVoice")
}

pub fn run_asr(
    python: &Path,
    worker: &Path,
    wav: &Path,
    model: &str,
    language: &str,
) -> Result<AsrOutput, String> {
    // Hard deny: never exec anything that lives under Desktop (TCC Files&Folders spam).
    let py_s = python.to_string_lossy();
    let wk_s = worker.to_string_lossy();
    let wav_s = wav.to_string_lossy();
    for (label, p) in [("python", py_s.as_ref()), ("worker", wk_s.as_ref()), ("wav", wav_s.as_ref())]
    {
        if p.contains("/Desktop/") || p.contains("/Desktop") {
            return Err(format!(
                "Refusing ASR path on Desktop ({label}={p}). Reinstall Wilson Voice so ASR uses Application Support."
            ));
        }
    }

    if !python.exists() {
        return Err(format!(
            "ASR Python missing at {} — open Permissions → Install local ASR (one-time, Application Support only).",
            python.display()
        ));
    }
    if !worker.exists() {
        return Err(format!("ASR worker missing: {}", worker.display()));
    }

    let home = asr_env_home();
    let cache = home.join("cache");
    let _ = std::fs::create_dir_all(&cache);
    let hf = cache.join("huggingface");
    let tmp = home.join("tmp");
    let _ = std::fs::create_dir_all(&hf);
    let _ = std::fs::create_dir_all(&tmp);

    // Isolate child completely: cwd + HF/XDG caches under Application Support.
    // Never inherit a Desktop cwd from a developer shell.
    let output = Command::new(python)
        .arg(worker)
        .arg(wav)
        .arg("--model")
        .arg(model)
        .arg("--language")
        .arg(language)
        .current_dir(&home)
        .env("HOME", dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")))
        .env("HF_HOME", &hf)
        .env("HUGGINGFACE_HUB_CACHE", hf.join("hub"))
        .env("TRANSFORMERS_CACHE", hf.join("transformers"))
        .env("XDG_CACHE_HOME", &cache)
        .env("TMPDIR", &tmp)
        .env("TEMP", &tmp)
        .env("TMP", &tmp)
        .env_remove("PYTHONPATH") // never pick up monorepo/Desktop packages
        .output()
        .map_err(|e| format!("failed to spawn ASR worker: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim().starts_with('{'))
        .unwrap_or(stdout.trim());

    let parsed: AsrResult = serde_json::from_str(line).map_err(|e| {
        format!(
            "bad ASR JSON: {e}\nstdout={stdout}\nstderr={stderr}"
        )
    })?;

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

pub struct AsrOutput {
    pub text: String,
    pub backend: String,
    pub seconds: f64,
}
