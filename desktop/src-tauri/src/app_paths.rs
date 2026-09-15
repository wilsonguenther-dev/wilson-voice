//! Y0-D — the ONE place Yap's state root is decided.
//!
//! Every stateful thing the app owns — the SQLite history, `settings.json`, the
//! downloaded ASR/diarization models, the recordings scratch dir, the failed-take
//! recovery dir and the meetings dir — hangs off a SINGLE root. Until this module
//! that root was re-derived, independently, at two sites (`crate::data_dir` and
//! `models::models_dir`), both of them hardcoding `dirs::data_dir()/WilsonVoice`.
//!
//! That is fine for one human with one install. It is not fine for automation:
//! two concurrent lane launches plus Wilson's own installed copy read and write
//! ONE history file, ONE settings store and ONE models dir. So the root is now
//! resolved ONCE, from an optional environment override
//! ([`crate::DATA_DIR_ENV`]), into an [`AppPaths`] value everything else reads.
//!
//! The override ADDS a path; it renames nothing. With the variable unset the
//! resolved root is byte-identical to what shipped, because renaming the data
//! directory would orphan every existing install's SQLite history.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The default state root — `<Application Support>/WilsonVoice`.
///
/// NEVER rename this directory: the SQLite history, the license file and every
/// downloaded model live under it, and a rename orphans all of them.
pub fn default_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("WilsonVoice")
}

/// Every directory and file path the app derives from its state root.
///
/// Constructed by [`resolve`]; read (never re-derived) by `crate::data_dir`,
/// `crate::recovery_dir`, `crate::meetings_dir` and `models::models_dir`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// The state root itself.
    pub root: PathBuf,
    /// Scratch WAVs for in-flight dictations (swept at every startup).
    pub recordings: PathBuf,
    /// Failed takes parked for retry (YV52).
    pub recovery: PathBuf,
    /// Meeting audio (YV95) — never swept with `recordings`.
    pub meetings: PathBuf,
    /// Downloaded ASR + diarization models.
    pub models: PathBuf,
    /// The SQLite history database.
    pub db: PathBuf,
    /// The settings store.
    pub settings: PathBuf,
    /// True when [`crate::DATA_DIR_ENV`] supplied the root, false when it is the
    /// shipped default. The `--smoke` preflight refuses to launch on `false`.
    pub overridden: bool,
}

/// Derive every path from an optional override value.
///
/// Pure: no environment read, no filesystem write. An override that is absent,
/// empty or whitespace-only is treated as ABSENT — an exported-but-empty
/// variable is a misconfigured harness, not a request to use `""` as a root.
pub fn resolve(env_root: Option<&str>) -> AppPaths {
    let (root, overridden) = match env_root.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => (PathBuf::from(s), true),
        None => (default_root(), false),
    };
    AppPaths {
        recordings: root.join("recordings"),
        recovery: root.join("recovery"),
        meetings: root.join("meetings"),
        models: root.join("models"),
        db: root.join("wilson_voice.db"),
        settings: root.join("settings.json"),
        root,
        overridden,
    }
}

/// Read [`crate::DATA_DIR_ENV`] and resolve. Deliberately NOT memoized so a test
/// can drive it with a value it just exported; the process-wide answer is
/// [`paths`].
pub fn resolve_from_env() -> AppPaths {
    resolve(std::env::var(crate::DATA_DIR_ENV).ok().as_deref())
}

/// Is `candidate` the default root, or inside it?
///
/// Both sides are canonicalized when they exist, so `/tmp/x` and `/private/tmp/x`
/// on macOS compare equal instead of silently passing the `--smoke` preflight.
pub fn is_default_root(candidate: &Path) -> bool {
    let norm = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let default = norm(&default_root());
    let candidate = norm(candidate);
    candidate == default || candidate.starts_with(&default)
}

static PATHS: OnceLock<AppPaths> = OnceLock::new();

/// The process's resolved paths. Read once from the environment, then frozen —
/// so a variable changed mid-run can never move the history out from under an
/// open SQLite connection.
pub fn paths() -> &'static AppPaths {
    PATHS.get_or_init(resolve_from_env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_override_is_the_shipped_default_root() {
        let p = resolve(None);
        assert_eq!(p.root, default_root());
        assert!(!p.overridden);
        assert!(p.root.ends_with("WilsonVoice"), "data dir name is frozen");
    }

    #[test]
    fn an_empty_override_is_treated_as_absent() {
        assert_eq!(resolve(Some("")).root, default_root());
        assert_eq!(resolve(Some("   ")).root, default_root());
        assert!(!resolve(Some(" ")).overridden);
    }

    #[test]
    fn an_override_moves_every_derived_path() {
        let p = resolve(Some("/tmp/yap-unit-root"));
        assert!(p.overridden);
        for derived in [
            &p.recordings,
            &p.recovery,
            &p.meetings,
            &p.models,
            &p.db,
            &p.settings,
        ] {
            assert!(
                derived.starts_with("/tmp/yap-unit-root"),
                "{} escaped the override",
                derived.display()
            );
        }
    }

    #[test]
    fn the_default_root_and_its_children_are_recognised() {
        assert!(is_default_root(&default_root()));
        assert!(is_default_root(&default_root().join("models")));
        assert!(!is_default_root(Path::new("/tmp/yap-unit-root")));
    }
}
