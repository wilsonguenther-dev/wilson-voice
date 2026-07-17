use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use uuid::Uuid;

pub struct ActiveRecording {
    pub child: Child,
    pub wav_path: PathBuf,
}

pub fn start_recording(dir: PathBuf) -> Result<ActiveRecording, String> {
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let wav_path = dir.join(format!("{}.wav", Uuid::new_v4()));

    // macOS: avfoundation audio device ":0" = default mic
    // Grant Microphone to "Wilson Voice" in System Settings → Privacy.
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "avfoundation",
            "-i",
            ":0",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
            wav_path.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "ffmpeg failed to start (is ffmpeg installed?): {e}. \
                 Grant Microphone to Wilson Voice in System Settings → Privacy & Security."
            )
        })?;

    // Surface immediate mic/device failures
    std::thread::sleep(std::time::Duration::from_millis(300));
    if let Ok(Some(status)) = child.try_wait() {
        let err = child
            .stderr
            .take()
            .and_then(|mut s| {
                use std::io::Read;
                let mut buf = String::new();
                let _ = s.read_to_string(&mut buf);
                Some(buf)
            })
            .unwrap_or_default();
        return Err(format!(
            "Mic capture exited immediately (code {:?}). {}\n\
             Grant Microphone permission to Wilson Voice / Terminal, then retry.",
            status.code(),
            err.trim()
        ));
    }

    Ok(ActiveRecording { child, wav_path })
}

pub fn stop_recording(mut active: ActiveRecording) -> Result<PathBuf, String> {
    // Graceful interrupt so ffmpeg flushes the WAV header
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-INT", &active.child.id().to_string()])
            .status();
        // Wait up to ~2s for clean exit
        for _ in 0..20 {
            if let Ok(Some(_)) = active.child.try_wait() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    let _ = active.child.kill();
    let _ = active.child.wait();
    std::thread::sleep(std::time::Duration::from_millis(120));

    if !active.wav_path.exists() {
        return Err("No audio file produced — check Microphone permission for Wilson Voice".into());
    }
    let meta = std::fs::metadata(&active.wav_path).map_err(|e| e.to_string())?;
    if meta.len() < 1000 {
        return Err(format!(
            "Audio too short ({} bytes) — hold ⌥Space longer or check mic level",
            meta.len()
        ));
    }
    Ok(active.wav_path)
}
