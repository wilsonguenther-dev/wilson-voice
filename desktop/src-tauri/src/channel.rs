//! TC-1 — the build CHANNEL: the one switch that turns a Yap build into
//! "Yap Test".
//!
//! WHY. Wilson wants to run a PR head before it clears review without touching
//! his installed, signed `/Applications/Yap.app`. macOS keys every TCC grant to
//! the signature's designated requirement, which includes the bundle id, and a
//! second bundle carrying the production id in `/Applications` is banned (the
//! duplicate-bundle scan in `perm_truth` exists because of it). So a test build
//! has to be a DIFFERENT app: its own bundle id, its own product name, its own
//! state root (history, settings, license, models, logs), and no updater — a
//! test build that auto-updated would replace itself with a release.
//!
//! HOW. `YAP_CHANNEL=test` at BUILD time. `build.rs` turns it into the
//! `yap_channel_test` cfg, so [`CURRENT`] is a compile-time constant and every
//! derived value below moves together — there is no runtime switch a stray
//! environment variable could flip on an installed production app. The bundle
//! side (identifier, productName, Info.plist names, updater artifacts) is the
//! matching `tauri build --config` overlay written by `scripts/test-build.sh`;
//! [`Profile::identifier`] and that overlay must agree, and the script asserts
//! it against the built bundle.

/// Which channel this binary was compiled for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// The shipped app. Every value is byte-identical to pre-TC-1 builds.
    Production,
    /// `scripts/test-build.sh` — "Yap Test.app", local only, never updates.
    Test,
}

/// The compiled channel. `YAP_CHANNEL=test` at build time selects [`Channel::Test`].
#[cfg(yap_channel_test)]
pub const CURRENT: Channel = Channel::Test;
/// The compiled channel. `YAP_CHANNEL=test` at build time selects [`Channel::Test`].
#[cfg(not(yap_channel_test))]
pub const CURRENT: Channel = Channel::Production;

/// Everything a channel decides, derived together so they can never disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// `CFBundleIdentifier` — must match the `tauri build --config` overlay.
    pub identifier: &'static str,
    /// `productName` / `CFBundleName`.
    pub product_name: &'static str,
    /// Directory name under `~/Library/Application Support`. Production's is
    /// FROZEN: renaming it orphans every install's SQLite history.
    pub data_dir_name: &'static str,
    /// The rotating log, relative to the state root.
    pub log_relpath: &'static str,
    /// May this build contact an update endpoint or install an update?
    pub updater_enabled: bool,
}

impl Channel {
    /// Parse the build-time `YAP_CHANNEL` value. Anything but `test` is production.
    pub fn from_name(name: Option<&str>) -> Channel {
        match name.map(str::trim) {
            Some("test") => Channel::Test,
            _ => Channel::Production,
        }
    }

    /// The profile this channel resolves to.
    pub const fn profile(self) -> Profile {
        match self {
            Channel::Production => Profile {
                identifier: "com.wilsonguenther.wilson-voice",
                product_name: "Yap",
                data_dir_name: "WilsonVoice",
                log_relpath: "logs/yap.log",
                updater_enabled: true,
            },
            Channel::Test => Profile {
                identifier: "com.wilsonguenther.wilson-voice.test",
                product_name: "Yap Test",
                data_dir_name: "WilsonVoice-Test",
                log_relpath: "logs/yap.log",
                updater_enabled: false,
            },
        }
    }

    /// `"production"` / `"test"`.
    pub const fn name(self) -> &'static str {
        match self {
            Channel::Production => "production",
            Channel::Test => "test",
        }
    }
}

/// The compiled profile.
pub const fn current() -> Profile {
    CURRENT.profile()
}

/// The git head a test build was made from (`YAP_BUILD_SHA`), if any.
pub const BUILD_SHA: Option<&str> = option_env!("YAP_BUILD_SHA");
/// The branch / PR label a test build was made from (`YAP_BUILD_BRANCH`), if any.
pub const BUILD_BRANCH: Option<&str> = option_env!("YAP_BUILD_BRANCH");

/// The chip text the UI shows on a test build — `None` on production.
/// The frontend renders the same string from the same build-time variables
/// (`src/buildChannel.ts`); this copy goes to the log at startup.
pub fn chip_text(channel: Channel, sha: Option<&str>, branch: Option<&str>) -> Option<String> {
    match channel {
        Channel::Production => None,
        Channel::Test => {
            let sha = sha.map(|s| s.chars().take(7).collect::<String>());
            let sha = sha.filter(|s| !s.is_empty()).unwrap_or_else(|| "local".into());
            Some(match branch.map(str::trim).filter(|b| !b.is_empty()) {
                Some(b) => format!("TEST BUILD {sha} · {b}"),
                None => format!("TEST BUILD {sha}"),
            })
        }
    }
}

/// One startup line naming the channel, so a log can prove which app wrote it.
pub fn startup_line() -> String {
    let p = current();
    let chip = chip_text(CURRENT, BUILD_SHA, BUILD_BRANCH).unwrap_or_else(|| "-".into());
    format!(
        "channel: {} · bundle id {} · product {} · updater {} · chip {}",
        CURRENT.name(),
        p.identifier,
        p.product_name,
        if p.updater_enabled { "enabled" } else { "DISABLED" },
        chip
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_profile_is_byte_identical_to_what_shipped() {
        let p = Channel::Production.profile();
        assert_eq!(p.identifier, "com.wilsonguenther.wilson-voice");
        assert_eq!(p.product_name, "Yap");
        assert_eq!(p.data_dir_name, "WilsonVoice", "the data dir name is frozen");
        assert_eq!(p.log_relpath, "logs/yap.log");
        assert!(p.updater_enabled);
    }

    #[test]
    fn the_test_switch_moves_identifier_data_dir_log_and_updater_together() {
        let prod = Channel::Production.profile();
        let test = Channel::Test.profile();
        assert_ne!(test.identifier, prod.identifier);
        assert_eq!(test.identifier, "com.wilsonguenther.wilson-voice.test");
        assert_eq!(test.product_name, "Yap Test");
        assert_ne!(test.data_dir_name, prod.data_dir_name);

        let prod_root = crate::app_paths::root_for(Channel::Production);
        let test_root = crate::app_paths::root_for(Channel::Test);
        assert!(test_root.ends_with("WilsonVoice-Test"));
        assert!(
            !test_root.starts_with(&prod_root),
            "the test root must not sit inside the production root"
        );
        let prod_log = prod_root.join(prod.log_relpath);
        let test_log = test_root.join(test.log_relpath);
        assert_ne!(test_log, prod_log, "the two channels must never share a log");
        assert!(test_log.starts_with(&test_root));

        assert!(!test.updater_enabled, "a test build must never update itself");
    }

    #[test]
    fn only_the_exact_word_test_selects_the_test_channel() {
        assert_eq!(Channel::from_name(Some("test")), Channel::Test);
        assert_eq!(Channel::from_name(Some(" test ")), Channel::Test);
        for other in [None, Some(""), Some("TEST"), Some("prod"), Some("beta")] {
            assert_eq!(Channel::from_name(other), Channel::Production, "{other:?}");
        }
    }

    #[test]
    fn chip_text_names_sha_and_branch_and_is_absent_on_production() {
        assert_eq!(chip_text(Channel::Production, Some("abc1234"), Some("main")), None);
        assert_eq!(
            chip_text(Channel::Test, Some("7e3276cdeadbeef"), Some("pr-240")).as_deref(),
            Some("TEST BUILD 7e3276c · pr-240")
        );
        assert_eq!(
            chip_text(Channel::Test, None, None).as_deref(),
            Some("TEST BUILD local")
        );
    }

    #[test]
    fn compiled_channel_matches_the_build_time_variable() {
        // `cargo test` with YAP_CHANNEL unset must compile production.
        let expected = Channel::from_name(option_env!("YAP_CHANNEL"));
        assert_eq!(CURRENT, expected);
    }
}
