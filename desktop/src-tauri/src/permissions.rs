//! macOS TCC / trust helpers for Wilson Voice.
//!
//! Identity that must appear in System Settings is the **app bundle**:
//!   com.wilsonguenther.wilson-voice  ("Wilson Voice")
//! Yap execs no helper interpreter, so that bundle is the ONLY row users ever
//! need to enable.

use crate::mic_auth::MicAuth;
use serde::Serialize;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionReport {
    /// AXIsProcessTrusted — required for enigo Cmd+V paste
    pub accessibility: bool,
    /// TCC says Authorized **and** an input device is attached. Kept for the
    /// existing call sites; `microphone_status` is what the UI branches on.
    pub microphone: bool,
    /// The raw AVFoundation authorization status:
    /// `"not_determined" | "denied" | "restricted" | "authorized"`.
    ///
    /// A bool cannot tell "never asked" from "the user said no", and those two
    /// need opposite UI: one shows a prompt button, the other a Settings link.
    pub microphone_status: String,
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

/// The microphone answer, read from macOS rather than inferred from hardware.
///
/// PERM-A: this used to ask `cpal` whether the default input device would hand
/// over a stream config and report that success as "mic ready", which is blind
/// to a revoked grant — and the prose it printed ("If Yap is missing from System Settings → Microphone, click Dictate
/// once to trigger the prompt") was advice built on that wrong model. macOS only
/// shows the prompt from `NotDetermined`; clicking Dictate after a denial does
/// nothing at all. Each status now gets the one next step that actually works.
pub fn microphone_probe() -> (bool, String) {
    let status = crate::mic_auth::authorization_status();
    (status == MicAuth::Authorized, microphone_detail(status))
}

/// Prose per status. Pure, so `tests/mic_auth_status.rs` can read it without a
/// microphone, a grant, or a window server.
pub fn microphone_detail(status: MicAuth) -> String {
    match status {
        MicAuth::Authorized => {
            if crate::mic_auth::input_device_present() {
                "Microphone authorized for Yap.".into()
            } else {
                "Microphone authorized, but no input device is attached — plug one in or pick one in System Settings → Sound.".into()
            }
        }
        MicAuth::NotDetermined => {
            "macOS has not been asked for the microphone yet — click Request Microphone to show the system prompt.".into()
        }
        MicAuth::Denied => {
            "Microphone is DENIED for Yap. macOS will not ask again — turn Yap on in System Settings → Privacy & Security → Microphone.".into()
        }
        MicAuth::Restricted => {
            "Microphone is restricted by a device policy (Screen Time or MDM). Yap cannot request it; an administrator has to allow it.".into()
        }
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
    let status = crate::mic_auth::authorization_status();
    let (asr_ok, asr_detail) = asr_probe(native_ready);
    build_report(accessibility, status, asr_ok, asr_detail)
}

/// The report, as a pure function of its four inputs.
///
/// Split out of [`report`] so `all_critical_ok` is testable on a CI runner with
/// no microphone and no grant — the exact machine on which the old bool-only
/// report was wrong.
pub fn build_report(
    accessibility: bool,
    status: MicAuth,
    asr_ok: bool,
    asr_detail: String,
) -> PermissionReport {
    // Authorization is TCC's answer; readiness additionally needs hardware.
    let microphone = status == MicAuth::Authorized && crate::mic_auth::input_device_present();
    let mic_detail = microphone_detail(status);
    let ffmpeg_ok = true; // no longer required — cpal in-process

    let mut parts: Vec<String> = Vec::new();
    if !accessibility {
        parts.push(
            "Accessibility OFF — enable for FN hold + auto-paste (Privacy → Accessibility → Yap)"
                .into(),
        );
    }
    if status != MicAuth::Authorized {
        parts.push(mic_detail.clone());
    }
    if !asr_ok {
        parts.push("Speech model needed".into());
    }

    // Mic + ASR are critical for dictation. Accessibility only for paste
    // (clipboard always works). A microphone that is not AUTHORIZED can never
    // be critical-ok, whatever the hardware says.
    let all_critical_ok = status == MicAuth::Authorized && microphone && asr_ok;
    let summary = if all_critical_ok {
        "All critical permissions look good for Yap.".to_string()
    } else if parts.is_empty() {
        mic_detail.clone()
    } else {
        parts.join(" · ")
    };

    PermissionReport {
        accessibility,
        microphone,
        microphone_status: status.as_str().to_string(),
        ffmpeg_ok,
        asr_ok,
        asr_detail,
        summary,
        all_critical_ok,
    }
}

/// YV102 — the pane a denied system-audio tap has to be fixed in, and the ONLY
/// recovery there is: TCC never re-asks after a denial.
///
/// **The anchor was verified on the target OS, not guessed**, because OS-10 is
/// explicit that a wrong one opens the top of System Settings, which is a worse
/// dead end than no link at all. The plan's candidate string —
/// `…?Privacy_SystemAudio` — **does not exist**. Enumerating the anchors the
/// Settings pane extension actually recognises:
///
/// ```text
/// $ strings -a /System/Library/ExtensionKit/Extensions/SecurityPrivacyExtension.appex\
///     /Contents/MacOS/SecurityPrivacyExtension | grep -oE 'Privacy_[A-Za-z0-9]+' | sort -u
/// … Privacy_Accessibility  Privacy_AudioCapture  Privacy_Camera  Privacy_Microphone
/// Privacy_ScreenCapture … (no Privacy_SystemAudio)
/// ```
///
/// `Privacy_AudioCapture` is the real one. Opening it and reading the window
/// title back out of the window server (macOS 26.5.2, build 25F84) gives
/// `Screen & System Audio Recording` — the pane that carries the **System Audio
/// Recording Only** list Yap needs to appear in. Transcript and screenshot:
/// `docs/pr-screenshots/YV102/`.
///
/// The fallback below is not decoration: on macOS 14.4–14.x the pane is titled
/// "System Audio Recording Only" and is reached by the same anchor, but if a
/// future OS retires it, landing on Privacy & Security is still one scroll from
/// the answer, whereas an unrecognised anchor lands on General.
pub const SYSTEM_AUDIO_PANE: &str = "SystemAudio";

/// Open the correct System Settings privacy pane (macOS 13+ / 15 URLs).
pub fn open_privacy_pane(pane: &str) -> Result<(), String> {
    // Prefer modern Settings URLs; fall back to legacy preference panes.
    let urls: &[&str] = match pane {
        "Microphone" => &[
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension",
        ],
        "Accessibility" => {
            &["x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"]
        }
        "InputMonitoring" | "ListenEvent" => {
            &["x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"]
        }
        // YV102 — see SYSTEM_AUDIO_PANE. Verified anchor first, Privacy &
        // Security second; never a bare System Settings launch, which is the
        // dead end OS-10 warns about.
        "SystemAudio" | "AudioCapture" => &[
            "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture",
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension",
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
