//! YV101 — the macOS runtime gate for system-audio capture (plan finding OS-11).
//!
//! CoreAudio's process-tap API (`AudioHardwareCreateProcessTap` and friends)
//! does not exist on macOS 12 or 13, and the Notetaker's system-audio track
//! (22-B) is built on it. Yap's deployment floor stays at **12.0** — §2.1
//! refuses to raise it and `tauri.conf.json` still says `"12.0"` — so the
//! absence of those symbols has to be handled at *runtime*, twice over:
//!
//! 1. **Link time.** A *hard* import of a symbol the running OS does not have
//!    is a dyld failure for the **entire app**, not a disabled feature: Yap
//!    would not open at all for every macOS 12/13 user. `build.rs` therefore
//!    weak-links CoreAudio, and `scripts/assert-weak-linked-14_4-symbols.sh`
//!    (wired into CI as `ci: assert weak-linked 14.4 CoreAudio symbols`) fails
//!    the build if any of those symbols is ever a hard load-time requirement
//!    again. This module is the *other* half of that: a weak import binds to
//!    NULL on an old OS, so calling one without checking is a crash instead of
//!    a launch failure — strictly better, still unacceptable.
//! 2. **Call time.** Every entry point into a process tap must pass
//!    [`system_audio_gate`] first. The gate is a plain comparison against
//!    [`OsVersion::SYSTEM_AUDIO_MIN`], and it is deliberately *honest* rather
//!    than silent: the refusal carries the sentence the UI shows, in the same
//!    shape as YV93's English-only gate (`meeting_asr::MeetingUnavailable`).
//!
//! **Mic-only meeting recording (22-A) is not gated by any of this** and must
//! never be. It runs on the macOS 12 floor, and
//! `meeting_asr::meeting_availability` — the mic-only door — never consults
//! this module's verdict. `tests/meeting_availability_144_gate.rs` is what
//! keeps that true.
//!
//! ## Why 14.4 and not 14.2
//!
//! `AudioHardwareCreateProcessTap` first appears in macOS 14.2, but the plan
//! (§2.1) sets the supported floor at **14.4**, and this gate implements the
//! plan's floor, not the SDK's. Choosing the higher number is the conservative
//! direction: it can only ever refuse on machines the plan already considers
//! unsupported.

use std::fmt;

/// A macOS version as the three numbers `ProcessInfo` reports.
///
/// `Ord` is derived, and the field order is what makes it correct: comparison
/// is major, then minor, then patch. A string compare would put `"14.10"`
/// before `"14.4"` and `"26.0"` before `"14.4"` — both wrong, and both are
/// versions this app will actually meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OsVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl OsVersion {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// The floor the plan sets for the system-audio process tap (§2.1, OS-11).
    pub const SYSTEM_AUDIO_MIN: OsVersion = OsVersion::new(14, 4, 0);

    /// What [`current`] reports when the OS version cannot be read — off macOS,
    /// or if `ProcessInfo` ever answers something impossible.
    ///
    /// It is `0.0.0` on purpose: unknown sorts *below* every real version, so
    /// every gate in this module **fails closed** on it. An unreadable OS
    /// version can never be the reason a tap is attempted.
    pub const UNKNOWN: OsVersion = OsVersion::new(0, 0, 0);

    /// Is this an OS version we actually read, or the fail-closed placeholder?
    pub fn is_known(&self) -> bool {
        *self != OsVersion::UNKNOWN
    }

    /// Parse `"14"`, `"14.4"` or `"14.4.1"`. Anything else is `None`.
    ///
    /// Deliberately strict — this parses version *numbers*, not `sw_vers`
    /// output or marketing names. A caller with a messy string is a caller who
    /// should not be deciding whether to touch a 14.4-only API.
    pub fn parse(text: &str) -> Option<OsVersion> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let parts: Vec<&str> = text.split('.').collect();
        // One to three components. A fourth is not a macOS version.
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        let mut numbers = [0u32; 3];
        for (slot, part) in numbers.iter_mut().zip(parts) {
            let part = part.trim();
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            *slot = part.parse().ok()?;
        }
        Some(OsVersion::new(numbers[0], numbers[1], numbers[2]))
    }

    /// The version of the macOS this process is running on.
    ///
    /// `ProcessInfo.operatingSystemVersion` is the read OS-11 names. It is a
    /// *runtime* read — the whole point — so it reports the user's OS, not the
    /// SDK this binary was compiled against.
    pub fn current() -> OsVersion {
        #[cfg(target_os = "macos")]
        {
            let info = objc2_foundation::NSProcessInfo::processInfo();
            let v = info.operatingSystemVersion();
            if v.majorVersion <= 0 {
                return OsVersion::UNKNOWN;
            }
            OsVersion::new(
                v.majorVersion.max(0) as u32,
                v.minorVersion.max(0) as u32,
                v.patchVersion.max(0) as u32,
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            OsVersion::UNKNOWN
        }
    }
}

impl fmt::Display for OsVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.patch == 0 {
            write!(f, "{}.{}", self.major, self.minor)
        } else {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        }
    }
}

/// The one sentence the disabled system-audio affordance shows.
///
/// Held here, as a constant, so the copy exists in exactly one place: the
/// enum variant in `meeting_asr.rs` builds its message from this, and the
/// tests assert against this rather than a re-typed string.
pub const SYSTEM_AUDIO_REQUIREMENT: &str = "System audio capture requires macOS 14.4 or later";

/// Whether this machine can run a CoreAudio process tap at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemAudioGate {
    /// The OS is new enough. Says nothing about TCC — permission is a separate
    /// question with a separate answer (YV102), and this gate never guesses at
    /// it.
    Available,
    /// The OS is too old, and no setting on the machine can change that.
    RequiresMacOS14_4 { found: OsVersion },
}

impl SystemAudioGate {
    pub fn is_available(&self) -> bool {
        matches!(self, SystemAudioGate::Available)
    }
}

/// The gate itself: pure, total, and the only place the comparison is made.
pub fn system_audio_gate(os: OsVersion) -> SystemAudioGate {
    if os >= OsVersion::SYSTEM_AUDIO_MIN {
        SystemAudioGate::Available
    } else {
        SystemAudioGate::RequiresMacOS14_4 { found: os }
    }
}

/// [`system_audio_gate`] against the OS this process is running on.
///
/// This is the call every process-tap entry point makes before it touches a
/// weak-linked 14.4 symbol.
pub fn system_audio_gate_now() -> SystemAudioGate {
    system_audio_gate(OsVersion::current())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_shapes_processinfo_and_settings_produce() {
        assert_eq!(OsVersion::parse("14"), Some(OsVersion::new(14, 0, 0)));
        assert_eq!(OsVersion::parse("14.4"), Some(OsVersion::new(14, 4, 0)));
        assert_eq!(OsVersion::parse("14.4.1"), Some(OsVersion::new(14, 4, 1)));
        assert_eq!(OsVersion::parse("  15.0 "), Some(OsVersion::new(15, 0, 0)));
        for junk in [
            "",
            "  ",
            "macOS 14.4",
            "14.",
            ".4",
            "14.4.1.2",
            "14.x",
            "-1",
        ] {
            assert_eq!(OsVersion::parse(junk), None, "{junk:?} is not a version");
        }
    }

    #[test]
    fn ordering_is_numeric_not_lexicographic() {
        // Both of these are backwards under a string compare, and both are
        // versions this app meets: 14.10 shipped, and macOS 26 is current.
        assert!(OsVersion::parse("14.10").unwrap() > OsVersion::parse("14.4").unwrap());
        assert!(OsVersion::parse("26.0").unwrap() > OsVersion::parse("14.4").unwrap());
        assert!(OsVersion::parse("14.4.1").unwrap() > OsVersion::parse("14.4").unwrap());
        assert!(OsVersion::UNKNOWN < OsVersion::parse("12.0").unwrap());
    }

    #[test]
    fn display_drops_a_zero_patch() {
        assert_eq!(OsVersion::new(14, 4, 0).to_string(), "14.4");
        assert_eq!(OsVersion::new(14, 4, 1).to_string(), "14.4.1");
    }

    #[test]
    fn an_unreadable_os_version_fails_closed() {
        assert!(!OsVersion::UNKNOWN.is_known());
        assert_eq!(
            system_audio_gate(OsVersion::UNKNOWN),
            SystemAudioGate::RequiresMacOS14_4 {
                found: OsVersion::UNKNOWN
            }
        );
    }
}
