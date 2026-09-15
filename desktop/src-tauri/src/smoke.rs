//! Y0-D — `--smoke`: the launch mode an automated agent is allowed to use.
//!
//! A normal Yap launch does three things that make it unsafe to start from a
//! script on Wilson's machine: it opens the SQLite history and `settings.json`
//! under the shared state root, it installs a system-wide keyboard tap plus four
//! global shortcut registrations, and it synthesizes ⌘V into whatever window
//! happens to be frontmost. Two lanes doing that at once — with Wilson typing in
//! a third window — is a corrupted history and a paste into his editor.
//!
//! `--smoke` is the launch that cannot do any of it:
//!
//! 1. it registers NO global hotkey (neither the CGEvent PTT tap nor the
//!    `global_shortcut` accelerators),
//! 2. it synthesizes NO paste and no delete keystroke (transcripts still land on
//!    the clipboard, so nothing is lost — that is a refusal, not a degradation),
//! 3. it REFUSES TO START at all unless [`crate::DATA_DIR_ENV`] points somewhere
//!    outside the default state root, and
//! 4. each start refusal exits NON-ZERO with a named message, instead of quietly
//!    falling back to the real data dir.
//!
//! Rule (3) is the one that matters: a smoke run that silently used the default
//! root would be reading and writing Wilson's real dictation history, which is
//! the entire failure this mode exists to prevent.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// The flag that requests smoke mode.
pub const SMOKE_FLAG: &str = "--smoke";

/// Exit status for a refused `--smoke` launch. `EX_CONFIG` from `sysexits.h`:
/// the request was well-formed, the environment it was made in was not.
pub const EXIT_REFUSED: i32 = 78;

/// Logged (at WARN) when smoke mode declines to install a global hotkey.
pub const HOTKEY_REFUSAL: &str =
    "--smoke: refusing to register any global hotkey (no PTT tap, no ⌃⌘V/⌃⌘M/⌃⌘Z/⌘⇧V)";

/// Logged (at WARN) when smoke mode declines to synthesize ⌘V.
pub const PASTE_REFUSAL: &str =
    "--smoke: refusing to synthesize a paste — transcript left on the clipboard";

/// Logged (at WARN) when smoke mode declines to synthesize a Delete keystroke.
pub const DELETE_REFUSAL: &str =
    "--smoke: refusing to synthesize a keystroke into the frontmost app";

/// Why a `--smoke` launch was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// `YAP_DATA_DIR` was unset, empty or whitespace.
    DataDirUnset,
    /// `YAP_DATA_DIR` resolved to the default state root (or inside it), which
    /// is exactly the history a smoke run must not touch.
    DataDirIsDefaultRoot(PathBuf),
}

impl Refusal {
    /// Stable, greppable name for this refusal — what a harness matches on.
    pub fn name(&self) -> &'static str {
        match self {
            Refusal::DataDirUnset => "SMOKE_REFUSED_DATA_DIR_UNSET",
            Refusal::DataDirIsDefaultRoot(_) => "SMOKE_REFUSED_DATA_DIR_IS_DEFAULT_ROOT",
        }
    }

    /// The message printed on stderr before the process exits.
    pub fn message(&self) -> String {
        match self {
            Refusal::DataDirUnset => format!(
                "{}: {SMOKE_FLAG} requires {} to name a scratch state root. \
                 It is unset, so this launch would read and write the real dictation history. Refusing to start.",
                self.name(),
                crate::DATA_DIR_ENV
            ),
            Refusal::DataDirIsDefaultRoot(root) => format!(
                "{}: {SMOKE_FLAG} was given {}={} , which is the default state root. \
                 A smoke run must not touch the real dictation history. Refusing to start.",
                self.name(),
                crate::DATA_DIR_ENV,
                root.display()
            ),
        }
    }

    /// Never zero — a refused smoke launch must fail its caller.
    pub fn exit_code(&self) -> i32 {
        EXIT_REFUSED
    }
}

/// Decide, from argv and the override value, whether this launch is a smoke run.
///
/// `Ok(false)` — a normal launch, nothing changes. `Ok(true)` — a smoke launch
/// that may proceed. `Err(_)` — a smoke launch that must exit non-zero.
///
/// Pure: no environment read, no process exit, no logging, so it is drivable
/// from a test.
pub fn preflight(argv: &[String], env_root: Option<&str>) -> Result<bool, Refusal> {
    if !argv.iter().any(|a| a == SMOKE_FLAG) {
        return Ok(false);
    }
    let paths = crate::app_paths::resolve(env_root);
    if !paths.overridden {
        return Err(Refusal::DataDirUnset);
    }
    if crate::app_paths::is_default_root(&paths.root) {
        return Err(Refusal::DataDirIsDefaultRoot(paths.root));
    }
    Ok(true)
}

static ARMED: AtomicBool = AtomicBool::new(false);

/// Arm smoke mode for the rest of the process. Called once, from `run()`, only
/// after [`preflight`] returned `Ok(true)`. There is no disarm: a process that
/// started as a smoke run stays one.
pub fn arm() {
    ARMED.store(true, Ordering::SeqCst);
}

/// Is this process a smoke run?
pub fn is_armed() -> bool {
    ARMED.load(Ordering::SeqCst)
}

/// May this process install a global hotkey? False for the whole life of a
/// smoke run.
pub fn global_hotkeys_allowed() -> bool {
    !is_armed()
}

/// May this process synthesize a keystroke (⌘V, Delete) into another app?
pub fn paste_allowed() -> bool {
    !is_armed()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smoke_argv() -> Vec<String> {
        vec!["--some-other-flag".into(), SMOKE_FLAG.into()]
    }

    #[test]
    fn no_flag_is_a_normal_launch_even_with_no_override() {
        assert_eq!(preflight(&[], None), Ok(false));
        assert_eq!(
            preflight(&["--transcribe-file".to_string()], None),
            Ok(false)
        );
    }

    #[test]
    fn a_scratch_root_is_accepted() {
        assert_eq!(
            preflight(&smoke_argv(), Some("/tmp/yap-smoke-unit")),
            Ok(true)
        );
    }

    #[test]
    fn refusals_are_named_and_non_zero() {
        for refusal in [
            Refusal::DataDirUnset,
            Refusal::DataDirIsDefaultRoot(crate::app_paths::default_root()),
        ] {
            assert_ne!(refusal.exit_code(), 0);
            assert!(refusal.message().contains(crate::DATA_DIR_ENV));
            assert!(refusal.message().contains(refusal.name()));
        }
    }
}
