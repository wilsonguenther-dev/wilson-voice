use serde::Deserialize;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Deserialize)]
pub struct AsrResult {
    pub ok: bool,
    pub text: Option<String>,
    pub backend: Option<String>,
    pub seconds: Option<f64>,
    pub error: Option<String>,
}

pub fn run_asr(
    python: &Path,
    worker: &Path,
    wav: &Path,
    model: &str,
    language: &str,
) -> Result<AsrOutput, String> {
    if !python.exists() {
        return Err(format!(
            "Python venv missing at {} — run: cd ~/Desktop/wilson-voice && python3.13 -m venv .venv && .venv/bin/pip install -e . mlx-whisper",
            python.display()
        ));
    }
    if !worker.exists() {
        return Err(format!("ASR worker missing: {}", worker.display()));
    }

    let output = Command::new(python)
        .arg(worker)
        .arg(wav)
        .arg("--model")
        .arg(model)
        .arg("--language")
        .arg(language)
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
