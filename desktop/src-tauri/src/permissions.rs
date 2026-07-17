//! macOS TCC / trust helpers for Wilson Voice.
//!
//! Identity that must appear in System Settings is the **app bundle**:
//!   com.wilsonguenther.wilson-voice  ("Wilson Voice")
//! Never tell users to enable "Python" for this product.

use serde::Serialize;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReport {
    /// AXIsProcessTrusted — required for enigo Cmd+V paste
    pub accessibility: bool,
    /// Best-effort mic readiness (ffmpeg can open default device)
    pub microphone: bool,
    /// ffmpeg present on PATH
    pub ffmpeg_ok: bool,
    /// Python ASR venv + worker present
    pub asr_ok: bool,
    pub asr_detail: String,
    /// Human summary for UI
    pub summary: String,
    pub all_critical_ok: bool,
}

#[cfg(target_os = "macos")]
mod macos {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(
            options: core_foundation::dictionary::CFDictionaryRef,
        ) -> bool;
    }

    /// Check Accessibility trust. If `prompt`, macOS may show the system dialog.
    pub fn accessibility_trusted(prompt: bool) -> bool {
        if !prompt {
            return unsafe { AXIsProcessTrusted() };
        }
        // kAXTrustedCheckOptionPrompt
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let val = CFBoolean::true_value();
        let pairs = &[(key.as_CFType(), val.as_CFType())];
        let dict = CFDictionary::from_CFType_pairs(pairs);
        unsafe { AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef()) }
    }
}

#[cfg(not(target_os = "macos"))]
mod macos {
    pub fn accessibility_trusted(_prompt: bool) -> bool {
        true
    }
}

/// Probe whether ffmpeg can see the default avfoundation audio device.
/// First real capture by the .app binary is what creates the TCC Microphone row.
pub fn microphone_probe() -> (bool, String) {
    let ffmpeg = which_ffmpeg();
    if ffmpeg.is_none() {
        return (
            false,
            "ffmpeg not found (brew install ffmpeg). Required for mic capture.".into(),
        );
    }
    let ffmpeg = ffmpeg.unwrap();
    // List devices — does not always need mic permission; actual capture does.
    let out = Command::new(&ffmpeg)
        .args(["-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""])
        .output();
    match out {
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            // avfoundation prints devices to stderr
            let has_audio = stderr.to_lowercase().contains("audio")
                || stderr.contains("MacBook")
                || stderr.contains("Microphone")
                || stderr.contains(":0");
            if has_audio {
                (
                    true,
                    "Mic device visible. First Record may prompt for Microphone permission for Wilson Voice.".into(),
                )
            } else {
                (
                    false,
                    format!("No audio device listed by ffmpeg. stderr: {}", truncate(&stderr, 200)),
                )
            }
        }
        Err(e) => (false, format!("ffmpeg probe failed: {e}")),
    }
}

fn which_ffmpeg() -> Option<String> {
    if let Ok(p) = which_path("ffmpeg") {
        return Some(p);
    }
    for candidate in [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
    ] {
        if std::path::Path::new(candidate).is_file() {
            return Some(candidate.into());
        }
    }
    None
}

fn which_path(bin: &str) -> Result<String, ()> {
    let o = Command::new("/usr/bin/which").arg(bin).output().map_err(|_| ())?;
    if !o.status.success() {
        return Err(());
    }
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if s.is_empty() {
        Err(())
    } else {
        Ok(s)
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}

pub fn asr_probe(python: &std::path::Path, worker: &std::path::Path) -> (bool, String) {
    if !python.exists() {
        return (
            false,
            format!(
                "Missing ASR venv at {}. Run: cd ~/Desktop/wilson-voice && python3 -m venv .venv && .venv/bin/pip install mlx-whisper",
                python.display()
            ),
        );
    }
    if !worker.exists() {
        return (false, format!("Missing ASR worker at {}", worker.display()));
    }
    (true, format!("ASR ready ({})", python.display()))
}

pub fn report(
    python: &std::path::Path,
    worker: &std::path::Path,
    prompt_accessibility: bool,
) -> PermissionReport {
    let accessibility = macos::accessibility_trusted(prompt_accessibility);
    let (microphone, mic_detail) = microphone_probe();
    let ffmpeg_ok = which_ffmpeg().is_some();
    let (asr_ok, asr_detail) = asr_probe(python, worker);

    let mut parts = Vec::new();
    if !accessibility {
        parts.push("Accessibility OFF — paste will only copy to clipboard");
    }
    if !microphone {
        parts.push("Microphone not ready");
    }
    if !asr_ok {
        parts.push("ASR not ready");
    }
    if !ffmpeg_ok {
        parts.push("ffmpeg missing");
    }

    let all_critical_ok = accessibility && microphone && asr_ok && ffmpeg_ok;
    let summary = if all_critical_ok {
        "All critical permissions look good for Wilson Voice.".into()
    } else if parts.is_empty() {
        mic_detail
    } else {
        parts.join(" · ")
    };

    PermissionReport {
        accessibility,
        microphone,
        ffmpeg_ok,
        asr_ok,
        asr_detail,
        summary,
        all_critical_ok,
    }
}

/// Open the correct System Settings privacy pane (macOS 13+ / 15 URLs).
pub fn open_privacy_pane(pane: &str) -> Result<(), String> {
    // Prefer modern Settings URLs; fall back to legacy preference panes.
    let urls: &[&str] = match pane {
        "Microphone" => &[
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension",
        ],
        "Accessibility" => &[
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        ],
        "InputMonitoring" | "ListenEvent" => &[
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent",
        ],
        _ => &["x-apple.systempreferences:com.apple.preference.security"],
    };
    let mut last_err = String::new();
    for url in urls {
        match Command::new("open").arg(url).spawn() {
            Ok(_) => return Ok(()),
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(last_err)
}

/// Prompt user for Accessibility if not trusted (system dialog).
pub fn request_accessibility_prompt() -> bool {
    macos::accessibility_trusted(true)
}

pub fn is_accessibility_trusted() -> bool {
    macos::accessibility_trusted(false)
}
