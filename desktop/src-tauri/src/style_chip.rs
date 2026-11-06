//! WF-20 — the per-app style chip and its short override.
//!
//! While a take is listening the pill shows which of Yap's six app-aware modes
//! it picked for the frontmost app ("Slack · casual"). Clicking the chip cycles
//! the mode FOR THAT APP ONLY, and the pick holds for [`OVERRIDE_TTL_MS`]
//! (15 minutes) before it reverts to what the app match would choose.
//!
//! Everything here is pure and takes the clock as an argument (`now_ms`), so the
//! expiry and the per-app scoping are proved with a fake clock in the unit tests
//! below — no sleeps, no wall time. `lib.rs` owns the one [`StyleOverrides`]
//! (in `AppState`), feeds it [`monotonic_ms`], and applies [`StyleOverrides::get`]
//! after `dictation::resolve_mode_with_context` on the live take paths.
//!
//! The chip exists ONLY when app-aware modes are on (`dictation_mode` is
//! `"auto"`). A user who picked a fixed mode in Settings has said which mode
//! every app gets; a chip that cycles it per app would contradict them, so
//! [`chip_for`] hides it and [`StyleOverrides::get`] is never consulted
//! ([`effective_mode`]).
//!
//! PRIVACY: overrides live in memory only (a relaunch forgets them) and the log
//! lines this feature writes carry the mode, never the app name.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;

use serde::Serialize;

use crate::dictation::{self, DictationMode};

/// The Tauri event carrying a [`StyleChip`] (styleChip.ts `STYLE_CHIP_EVENT`).
pub const STYLE_CHIP_EVENT: &str = "style_chip";

/// How long a chip pick holds for its app: 15 minutes, the same window Wispr's
/// quick style switcher uses.
pub const OVERRIDE_TTL_MS: u64 = 15 * 60 * 1000;

/// The six app-aware modes, in the order a click cycles them. `List` is not
/// here: it is never inferred from an app, only forced from Settings.
pub const CYCLE: [DictationMode; 6] = [
    DictationMode::Email,
    DictationMode::Document,
    DictationMode::Notes,
    DictationMode::Code,
    DictationMode::Chat,
    DictationMode::Plain,
];

/// Milliseconds on a monotonic clock that starts the first time it is read.
/// Monotonic so a wall-clock change can never stretch or cut an override.
pub fn monotonic_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Are app-aware modes on? Mirrors the "user picked a fixed mode" test inside
/// `dictation::resolve_mode_with_context`: anything that is not one of the fixed
/// Settings values defers to the app match, so it is app-aware.
pub fn app_aware_on(dictation_mode_setting: &str) -> bool {
    !matches!(
        dictation_mode_setting.trim().to_lowercase().as_str(),
        "plain" | "list" | "email" | "code" | "notes"
    )
}

/// The mode a click moves to. A mode outside [`CYCLE`] (`List`) starts it.
pub fn next_mode(mode: DictationMode) -> DictationMode {
    match CYCLE.iter().position(|m| *m == mode) {
        Some(i) => CYCLE[(i + 1) % CYCLE.len()],
        None => CYCLE[0],
    }
}

/// The word the chip shows for a mode. `Chat` reads "casual" because that is
/// what it does to the text (no trailing period, no reflow); the rest are the
/// mode's own name.
pub fn mode_word(mode: DictationMode) -> &'static str {
    match mode {
        DictationMode::Email => "email",
        DictationMode::Document => "document",
        DictationMode::Notes => "notes",
        DictationMode::Code => "code",
        DictationMode::Chat => "casual",
        DictationMode::Plain => "plain",
        DictationMode::List => "list",
    }
}

/// The key an override is stored under: the app's name, trimmed and
/// lowercased, so "Slack" and "slack " are the same app. Empty means "no app",
/// which never holds an override.
pub fn app_key(app_name: &str) -> String {
    app_name.trim().to_lowercase()
}

/// One app's pick and the monotonic ms it stops applying at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pick {
    mode: DictationMode,
    expires_at_ms: u64,
}

/// Every live chip override, keyed by [`app_key`]. In memory only.
#[derive(Debug, Default)]
pub struct StyleOverrides {
    picks: HashMap<String, Pick>,
}

impl StyleOverrides {
    pub fn new() -> Self {
        Self::default()
    }

    /// The override for `app_name` at `now_ms`, if one is still running. An
    /// expired pick reads as `None` (the expiry is exclusive: at exactly
    /// `expires_at_ms` the pick is over).
    pub fn get(&self, app_name: &str, now_ms: u64) -> Option<DictationMode> {
        let key = app_key(app_name);
        if key.is_empty() {
            return None;
        }
        self.picks
            .get(&key)
            .filter(|p| now_ms < p.expires_at_ms)
            .map(|p| p.mode)
    }

    /// Ms left on `app_name`'s override at `now_ms`; `None` when there is none.
    pub fn remaining_ms(&self, app_name: &str, now_ms: u64) -> Option<u64> {
        let key = app_key(app_name);
        self.picks
            .get(&key)
            .filter(|p| now_ms < p.expires_at_ms)
            .map(|p| p.expires_at_ms - now_ms)
    }

    /// One click on the chip: move `app_name` to the mode after `current` (what
    /// the chip showed) and hold it for [`OVERRIDE_TTL_MS`] from `now_ms`. A
    /// click RESTARTS the window. Returns the new mode; `None` (and nothing is
    /// stored) when there is no app to scope the pick to.
    pub fn cycle(
        &mut self,
        app_name: &str,
        current: DictationMode,
        now_ms: u64,
    ) -> Option<DictationMode> {
        let key = app_key(app_name);
        if key.is_empty() {
            return None;
        }
        self.prune(now_ms);
        let mode = next_mode(current);
        self.picks.insert(
            key,
            Pick {
                mode,
                expires_at_ms: now_ms.saturating_add(OVERRIDE_TTL_MS),
            },
        );
        Some(mode)
    }

    /// Drop every pick that has expired at `now_ms`, so the map never grows
    /// with apps the user stopped visiting.
    pub fn prune(&mut self, now_ms: u64) {
        self.picks.retain(|_, p| now_ms < p.expires_at_ms);
    }

    /// How many picks are stored (expired ones included until the next prune).
    pub fn len(&self) -> usize {
        self.picks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.picks.is_empty()
    }
}

/// The mode a take actually uses: the resolved mode, replaced by the app's
/// running chip pick — but only while app-aware modes are on. A fixed Settings
/// mode always wins, exactly as it does in `resolve_mode_with_context`.
pub fn effective_mode(
    setting: &str,
    app_name: &str,
    resolved: DictationMode,
    overrides: &StyleOverrides,
    now_ms: u64,
) -> DictationMode {
    if !app_aware_on(setting) {
        return resolved;
    }
    overrides.get(app_name, now_ms).unwrap_or(resolved)
}

/// What the pill's chip renders. serde camelCase for styleChip.ts `StyleChip`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleChip {
    /// False when app-aware modes are off or no app is frontmost: the pill
    /// renders nothing.
    pub visible: bool,
    /// The frontmost app's display name ("Slack"). Local only — never logged.
    pub app: String,
    /// The mode's chip word ("casual").
    pub mode: &'static str,
    /// "Slack · casual" — the whole chip text.
    pub label: String,
    /// True while a chip pick (not the app match) chose the mode.
    pub overridden: bool,
    /// Ms until the pick reverts; `None` when not overridden.
    pub expires_in_ms: Option<u64>,
}

impl StyleChip {
    pub fn hidden() -> Self {
        Self {
            visible: false,
            app: String::new(),
            mode: "",
            label: String::new(),
            overridden: false,
            expires_in_ms: None,
        }
    }
}

/// The chip for `app_name` under `setting` at `now_ms`. The app-match mode is
/// `dictation::resolve_mode` (the listening pill has no caret context yet, so
/// that is the honest "what Yap picked for this app"), replaced by a running
/// pick.
pub fn chip_for(
    setting: &str,
    app_name: Option<&str>,
    overrides: &StyleOverrides,
    now_ms: u64,
) -> StyleChip {
    let app = app_name.map(str::trim).unwrap_or_default();
    if !app_aware_on(setting) || app.is_empty() {
        return StyleChip::hidden();
    }
    let picked = overrides.get(app, now_ms);
    let mode = picked.unwrap_or_else(|| dictation::resolve_mode(setting, app));
    let word = mode_word(mode);
    StyleChip {
        visible: true,
        app: app.to_string(),
        mode: word,
        label: format!("{app} \u{00b7} {word}"),
        overridden: picked.is_some(),
        expires_in_ms: overrides.remaining_ms(app, now_ms),
    }
}

/// The current chip mode for `app_name` (pick, else app match) — what a click
/// cycles FROM, so the click always moves one step past what the user saw.
pub fn shown_mode(
    setting: &str,
    app_name: &str,
    overrides: &StyleOverrides,
    now_ms: u64,
) -> DictationMode {
    overrides
        .get(app_name, now_ms)
        .unwrap_or_else(|| dictation::resolve_mode(setting, app_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60 * 1000;

    #[test]
    fn override_expires_on_time_with_a_fake_clock() {
        let mut o = StyleOverrides::new();
        let t0 = 1_000_000;
        let picked = o.cycle("Slack", DictationMode::Chat, t0);
        assert_eq!(picked, Some(DictationMode::Plain));
        assert_eq!(o.get("Slack", t0), Some(DictationMode::Plain));
        assert_eq!(o.get("Slack", t0 + 14 * MIN), Some(DictationMode::Plain));
        assert_eq!(
            o.get("Slack", t0 + OVERRIDE_TTL_MS - 1),
            Some(DictationMode::Plain)
        );
        // Exactly 15 minutes later the pick is over, and stays over.
        assert_eq!(o.get("Slack", t0 + OVERRIDE_TTL_MS), None);
        assert_eq!(o.get("Slack", t0 + 16 * MIN), None);
        assert_eq!(o.remaining_ms("Slack", t0 + 5 * MIN), Some(10 * MIN));
        assert_eq!(o.remaining_ms("Slack", t0 + OVERRIDE_TTL_MS), None);
    }

    #[test]
    fn override_only_affects_that_app() {
        let mut o = StyleOverrides::new();
        let t0 = 42;
        o.cycle("Slack", DictationMode::Chat, t0);
        assert_eq!(o.get("Slack", t0 + MIN), Some(DictationMode::Plain));
        // Same app, different spelling of its name: same pick.
        assert_eq!(o.get(" slack ", t0 + MIN), Some(DictationMode::Plain));
        // Every other app is untouched.
        assert_eq!(o.get("Mail", t0 + MIN), None);
        assert_eq!(o.get("Visual Studio Code", t0 + MIN), None);
        assert_eq!(
            effective_mode("auto", "Mail", DictationMode::Email, &o, t0 + MIN),
            DictationMode::Email
        );
        assert_eq!(
            effective_mode("auto", "Slack", DictationMode::Chat, &o, t0 + MIN),
            DictationMode::Plain
        );
        // ...and after the window, Slack is back to its app match too.
        assert_eq!(
            effective_mode(
                "auto",
                "Slack",
                DictationMode::Chat,
                &o,
                t0 + OVERRIDE_TTL_MS
            ),
            DictationMode::Chat
        );
    }

    #[test]
    fn a_click_restarts_the_window_and_steps_one_mode() {
        let mut o = StyleOverrides::new();
        o.cycle("Slack", DictationMode::Chat, 0);
        let shown = shown_mode("auto", "Slack", &o, 10 * MIN);
        assert_eq!(shown, DictationMode::Plain);
        assert_eq!(
            o.cycle("Slack", shown, 10 * MIN),
            Some(DictationMode::Email)
        );
        // The second click's window runs from the second click.
        assert_eq!(
            o.get("Slack", 10 * MIN + 14 * MIN),
            Some(DictationMode::Email)
        );
        assert_eq!(o.get("Slack", 10 * MIN + OVERRIDE_TTL_MS), None);
    }

    #[test]
    fn cycle_walks_all_six_modes_and_wraps() {
        let mut m = DictationMode::Email;
        let mut seen = Vec::new();
        for _ in 0..6 {
            seen.push(m);
            m = next_mode(m);
        }
        assert_eq!(seen, CYCLE.to_vec());
        assert_eq!(m, DictationMode::Email);
        assert_eq!(next_mode(DictationMode::List), DictationMode::Email);
    }

    #[test]
    fn no_app_never_holds_an_override() {
        let mut o = StyleOverrides::new();
        assert_eq!(o.cycle("  ", DictationMode::Plain, 0), None);
        assert!(o.is_empty());
        assert_eq!(o.get("", 0), None);
    }

    #[test]
    fn prune_drops_expired_picks() {
        let mut o = StyleOverrides::new();
        o.cycle("Slack", DictationMode::Chat, 0);
        o.cycle("Mail", DictationMode::Email, 5 * MIN);
        o.prune(OVERRIDE_TTL_MS);
        assert_eq!(o.len(), 1);
        assert_eq!(
            o.get("Mail", OVERRIDE_TTL_MS),
            Some(DictationMode::Document)
        );
    }

    #[test]
    fn chip_hides_when_app_aware_modes_are_off() {
        let mut o = StyleOverrides::new();
        o.cycle("Slack", DictationMode::Chat, 0);
        for fixed in ["plain", "list", "email", "code", "notes", " Email "] {
            assert!(!app_aware_on(fixed), "{fixed}");
            let chip = chip_for(fixed, Some("Slack"), &o, MIN);
            assert!(!chip.visible, "{fixed}");
            assert_eq!(chip, StyleChip::hidden());
            // ...and a running pick never leaks into a fixed-mode take.
            assert_eq!(
                effective_mode(fixed, "Slack", DictationMode::Email, &o, MIN),
                DictationMode::Email
            );
        }
        // No frontmost app: nothing to show either.
        assert!(!chip_for("auto", None, &o, MIN).visible);
        assert!(!chip_for("auto", Some(""), &o, MIN).visible);
    }

    #[test]
    fn chip_shows_the_app_and_its_mode_while_auto() {
        let o = StyleOverrides::new();
        assert!(app_aware_on("auto"));
        let chip = chip_for("auto", Some("Slack"), &o, 0);
        assert!(chip.visible);
        assert_eq!(chip.label, "Slack \u{00b7} casual");
        assert_eq!(chip.mode, "casual");
        assert!(!chip.overridden);
        assert_eq!(chip.expires_in_ms, None);

        let mut o = StyleOverrides::new();
        o.cycle("Slack", DictationMode::Chat, 0);
        let chip = chip_for("auto", Some("Slack"), &o, 5 * MIN);
        assert_eq!(chip.label, "Slack \u{00b7} plain");
        assert!(chip.overridden);
        assert_eq!(chip.expires_in_ms, Some(10 * MIN));
        // After the window the chip is back on the app match.
        let chip = chip_for("auto", Some("Slack"), &o, OVERRIDE_TTL_MS);
        assert_eq!(chip.mode, "casual");
        assert!(!chip.overridden);
    }

    #[test]
    fn chip_serializes_camel_case_for_the_pill() {
        let chip = chip_for("auto", Some("Slack"), &StyleOverrides::new(), 0);
        let json = serde_json::to_string(&chip).unwrap();
        assert!(json.contains("\"expiresInMs\":null"), "{json}");
        assert!(json.contains("\"overridden\":false"), "{json}");
        assert!(json.contains("\"label\":\"Slack · casual\""), "{json}");
    }
}
