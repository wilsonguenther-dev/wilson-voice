//! macOS TCC / trust helpers for Wilson Voice.
//!
//! Identity that must appear in System Settings is the **app bundle**:
//!   com.wilsonguenther.wilson-voice  ("Wilson Voice")
//! Yap execs no helper interpreter, so that bundle is the ONLY row users ever
//! need to enable.

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
    /// A speech model is on disk, so the embedded engine can transcribe
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

/// Probe default input device via cpal (same stack as recording).
/// First successful capture creates the TCC Microphone row for Wilson Voice.
pub fn microphone_probe() -> (bool, String) {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    match host.default_input_device() {
        Some(dev) => {
            let name = dev
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| "default mic".into());
            match dev.default_input_config() {
                Ok(cfg) => (
                    true,
                    format!(
                        "Mic ready: {name} ({} Hz). If Yap is missing from System Settings → Microphone, click Dictate once to trigger the prompt.",
                        cfg.sample_rate()
                    ),
                ),
                Err(e) => (
                    false,
                    format!(
                        "Mic found ({name}) but config failed: {e}. Toggle Microphone for Yap in System Settings."
                    ),
                ),
            }
        }
        None => (
            false,
            "No input device. Click Dictate once so macOS prompts for Microphone, then enable Yap in System Settings → Privacy → Microphone.".into(),
        ),
    }
}

/// Can this install transcribe? YV34: the embedded GGUF engine is the ONLY
/// path, so a downloaded model (`native_ready`) is the whole answer. Purely a
/// value check — it spawns nothing, which is what makes the 1.2 s onboarding
/// poll safe (the old probe exec'd an interpreter on every tick).
pub fn asr_probe(native_ready: bool) -> (bool, String) {
    if native_ready {
        (
            true,
            "Speech model downloaded — embedded engine ready".into(),
        )
    } else {
        (
            false,
            "No speech model on disk yet — click Get a speech model.".into(),
        )
    }
}

pub fn report(prompt_accessibility: bool, native_ready: bool) -> PermissionReport {
    let accessibility = macos::accessibility_trusted(prompt_accessibility);
    let (microphone, mic_detail) = microphone_probe();
    let ffmpeg_ok = true; // no longer required — cpal in-process
    let (asr_ok, asr_detail) = asr_probe(native_ready);

    let mut parts = Vec::new();
    if !accessibility {
        parts.push(
            "Accessibility OFF — enable for FN hold + auto-paste (Privacy → Accessibility → Yap)",
        );
    }
    if !microphone {
        parts.push("Microphone not ready");
    }
    if !asr_ok {
        parts.push("Speech model needed");
    }

    // Mic + ASR are critical for dictation. Accessibility only for paste (clipboard always works).
    let all_critical_ok = microphone && asr_ok;
    let summary = if all_critical_ok {
        "All critical permissions look good for Yap.".into()
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
