//! yap-1107-globe-key-conflict-guard — the Globe key guard.
//!
//! Wilson, 2026-11-07: a lone Fn release at the end of a take opened the macOS
//! emoji picker. On Macs with a Globe key, macOS acts on its own when Fn goes
//! down and comes back up with no other key, per
//! `com.apple.HIToolbox AppleFnUsageType`: 0 Do Nothing, 1 Change Input
//! Source, 2 Show Emoji & Symbols, 3 Start Dictation; unset behaves as 2.
//! Yap's fn tap is LISTEN-ONLY (`ptt_macos::MAIN_TAP_OPTIONS`), so it cannot
//! stop that.
//!
//! This module is the guard: a PURE detector ([`conflict`]), a reader/writer
//! of that ONE preference behind a trait ([`DefaultsRunner`]) so tests inject
//! values and never touch the real preference, and the consented, reversible
//! fix ([`turn_off`] / [`put_back`]). The write never runs without the user's
//! button press (the only callers are the two Tauri commands below, invoked by
//! the "Turn it off" / "Put it back" buttons), and no other key or domain is
//! ever written.
//!
//! Research (see docs/loop/evidence/yap-1107-globe-key-conflict-guard.md):
//! * A restart is documented as required for this key to take effect
//!   (nix-darwin `system.defaults.hitoolbox.AppleFnUsageType`), so the line
//!   after "Turn it off" says so plainly.
//! * Swallowing the lone-Fn release with an ACTIVE tap could not be proven
//!   headless, so it is NOT shipped; the tap stays listen-only.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The ONE preference domain this module reads or writes.
pub const DOMAIN: &str = "com.apple.HIToolbox";
/// The ONE key this module reads or writes.
pub const KEY: &str = "AppleFnUsageType";
/// "Do Nothing".
pub const OFF_VALUE: i64 = 0;
/// The file under the app's data dir that remembers the value before "Turn it off".
pub const PRIOR_FILE: &str = "globe-key-prior.json";

pub const EMOJI_LINE: &str =
    "Your Mac opens the emoji picker on a lone Fn tap; Yap can turn that off";
pub const DICTATION_LINE: &str =
    "Your Mac starts macOS Dictation on a lone Fn tap; Yap can turn that off";
pub const TURNED_OFF_LINE: &str =
    "A lone Fn tap now does nothing. macOS applies this fully after you restart your Mac.";
pub const TURN_OFF_LABEL: &str = "Turn it off";
pub const PUT_BACK_LABEL: &str = "Put it back";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// Unset or 2: Show Emoji & Symbols.
    EmojiPicker,
    /// 3: Start Dictation (macOS's own, not Yap's).
    Dictation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub kind: ConflictKind,
    /// The value read (`None` = unset).
    pub pref: Option<i64>,
    /// The one plain line the UI shows (product-named by the caller).
    pub line: String,
}

/// Does the bound push-to-talk / toggle shortcut include Fn?
///
/// Every `ptt_binding` value parses to an fn binding
/// (`ptt_macos::PttBinding::from_settings`: `fn`, `fn_control`, `both`, and
/// any unknown string falls back to fn⌃), and the always-global chords
/// (`hotkeys::ChordSetting`: code + ⌃/⌥/⌘) cannot hold Fn. So today the answer
/// is always yes; the function exists so the detector's input stays explicit
/// if a non-fn push-to-talk ever lands.
pub fn binding_uses_fn(_ptt_binding: &str) -> bool {
    true
}

/// PURE: the conflict, if any, between an Fn shortcut and the lone-Fn action.
///
/// Conflict exactly for unset, 2 (emoji picker) and 3 (dictation) when Fn is
/// bound. 0 does nothing. 1 switches the input source only when the user has
/// two or more input sources, which this guard cannot show fires, so it is no
/// conflict (no milder line). Unknown values are no conflict.
pub fn conflict(binding_uses_fn: bool, pref: Option<i64>) -> Option<Conflict> {
    if !binding_uses_fn {
        return None;
    }
    let (kind, line) = match pref {
        None | Some(2) => (ConflictKind::EmojiPicker, EMOJI_LINE),
        Some(3) => (ConflictKind::Dictation, DICTATION_LINE),
        _ => return None,
    };
    Some(Conflict {
        kind,
        pref,
        line: line.to_string(),
    })
}

/// Runs `defaults <args>`. `Ok((exit_ok, stdout))`, or `Err` when it could not
/// start. The seam tests use to inject values and record command lines.
pub trait DefaultsRunner {
    fn run(&self, args: &[String]) -> Result<(bool, String), String>;
}

/// The real runner: `/usr/bin/defaults`.
pub struct SystemDefaults;

impl DefaultsRunner for SystemDefaults {
    fn run(&self, args: &[String]) -> Result<(bool, String), String> {
        if !cfg!(target_os = "macos") {
            return Err("The Globe key setting exists only on macOS.".into());
        }
        let out = std::process::Command::new("/usr/bin/defaults")
            .args(args)
            .output()
            .map_err(|e| format!("Could not run defaults: {e}"))?;
        Ok((
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        ))
    }
}

/// `defaults read <domain> AppleFnUsageType`.
pub fn read_args(domain: &str) -> Vec<String> {
    vec!["read".into(), domain.into(), KEY.into()]
}

/// `defaults write <domain> AppleFnUsageType -int 0` — what "Turn it off" runs.
pub fn write_off_args(domain: &str) -> Vec<String> {
    vec![
        "write".into(),
        domain.into(),
        KEY.into(),
        "-int".into(),
        OFF_VALUE.to_string(),
    ]
}

/// What "Put it back" runs: write the recorded value, or delete the key when
/// it was unset before.
pub fn restore_args(domain: &str, prior: Option<i64>) -> Vec<String> {
    match prior {
        Some(v) => vec![
            "write".into(),
            domain.into(),
            KEY.into(),
            "-int".into(),
            v.to_string(),
        ],
        None => vec!["delete".into(), domain.into(), KEY.into()],
    }
}

/// Read the value. A non-zero exit means unset (`Ok(None)`).
pub fn read_pref(r: &dyn DefaultsRunner, domain: &str) -> Result<Option<i64>, String> {
    let (ok, out) = r.run(&read_args(domain))?;
    if !ok {
        return Ok(None);
    }
    out.trim()
        .parse::<i64>()
        .map(Some)
        .map_err(|_| "The Globe key setting holds a value Yap does not recognise.".to_string())
}

/// What "Turn it off" recorded: the value before, `None` = it was unset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriorRecord {
    pub prior: Option<i64>,
}

pub fn prior_path(data_dir: &Path) -> PathBuf {
    data_dir.join(PRIOR_FILE)
}

pub fn load_prior(path: &Path) -> Option<PriorRecord> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save_prior(path: &Path, rec: PriorRecord) -> Result<(), String> {
    let raw = serde_json::to_string(&rec).map_err(|e| e.to_string())?;
    std::fs::write(path, raw).map_err(|e| format!("Could not record the current setting: {e}"))
}

/// What the Permissions row renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobeStatus {
    /// The value read just now (`None` = unset).
    pub pref: Option<i64>,
    pub conflict: Option<Conflict>,
    /// The one line to show, `None` = show nothing.
    pub line: Option<String>,
    /// "Turn it off" is offered (a conflict exists).
    pub can_turn_off: bool,
    /// "Put it back" is offered (Yap changed it and recorded the prior value).
    pub can_put_back: bool,
    pub turn_off_label: &'static str,
    pub put_back_label: &'static str,
}

/// PURE: the row from a reading and the recorded prior value.
pub fn status(
    binding_uses_fn: bool,
    pref: Option<i64>,
    prior: Option<PriorRecord>,
    product: &str,
) -> GlobeStatus {
    let named = |s: &str| crate::permissions::name_product(s, product);
    let mut c = conflict(binding_uses_fn, pref);
    if let Some(c) = c.as_mut() {
        c.line = named(&c.line);
    }
    let can_put_back = prior.is_some();
    let line = match (&c, can_put_back) {
        (Some(c), _) => Some(c.line.clone()),
        (None, true) if pref == Some(OFF_VALUE) => Some(named(TURNED_OFF_LINE)),
        _ => None,
    };
    GlobeStatus {
        pref,
        can_turn_off: c.is_some(),
        conflict: c,
        line,
        can_put_back,
        turn_off_label: TURN_OFF_LABEL,
        put_back_label: PUT_BACK_LABEL,
    }
}

/// "Turn it off" — the button press is the consent. Records the prior value
/// (only the first time, so "Put it back" restores the user's original),
/// writes 0, re-reads, returns the new row.
pub fn turn_off(
    r: &dyn DefaultsRunner,
    domain: &str,
    prior_file: &Path,
    binding_uses_fn: bool,
    product: &str,
) -> Result<GlobeStatus, String> {
    let before = read_pref(r, domain)?;
    if load_prior(prior_file).is_none() {
        save_prior(prior_file, PriorRecord { prior: before })?;
    }
    let (ok, _) = r.run(&write_off_args(domain))?;
    if !ok {
        return Err("macOS refused the change to the Globe key setting.".into());
    }
    let after = read_pref(r, domain)?;
    log::info!(
        "globe_key turn_off prior={} after={}",
        fmt_pref(before),
        fmt_pref(after)
    );
    Ok(status(
        binding_uses_fn,
        after,
        load_prior(prior_file),
        product,
    ))
}

/// "Put it back" — restores the recorded value (or deletes the key if it was
/// unset), re-reads, and forgets the record once the value matches.
pub fn put_back(
    r: &dyn DefaultsRunner,
    domain: &str,
    prior_file: &Path,
    binding_uses_fn: bool,
    product: &str,
) -> Result<GlobeStatus, String> {
    let Some(rec) = load_prior(prior_file) else {
        return Err("Yap has no earlier Globe key setting to put back.".into());
    };
    let (ok, _) = r.run(&restore_args(domain, rec.prior))?;
    // `defaults delete` of a key that is already gone exits non-zero; the
    // re-read below is the truth either way.
    if !ok && rec.prior.is_some() {
        return Err("macOS refused to restore the Globe key setting.".into());
    }
    let after = read_pref(r, domain)?;
    let restored = after == rec.prior;
    if restored {
        let _ = std::fs::remove_file(prior_file);
    }
    log::info!(
        "globe_key put_back prior={} after={} restored={restored}",
        fmt_pref(rec.prior),
        fmt_pref(after)
    );
    if !restored {
        return Err("The Globe key setting did not change back. Try Put it back again.".into());
    }
    Ok(status(binding_uses_fn, after, None, product))
}

fn fmt_pref(p: Option<i64>) -> String {
    p.map_or_else(|| "unset".to_string(), |v| v.to_string())
}

fn ptt_uses_fn() -> bool {
    // The PTT binding string is irrelevant today (see `binding_uses_fn`).
    binding_uses_fn("fn_control")
}

fn product() -> String {
    crate::perm_truth::product_name()
}

fn live_prior_file() -> PathBuf {
    prior_path(&crate::data_dir())
}

fn surfaced<T>(event: &str, r: Result<T, String>) -> Result<T, String> {
    if let Err(e) = &r {
        log::warn!("globe_key {event}_failed reason={e:?}");
    }
    // Surfaced as one sentence (toastError), named for the running product.
    r.map_err(|e| crate::permissions::named(&e))
}

/// Read-only: the row for the Permissions page. Never writes.
#[tauri::command]
pub async fn globe_key_status() -> Result<GlobeStatus, String> {
    let r = read_pref(&SystemDefaults, DOMAIN).map(|pref| {
        let s = status(
            ptt_uses_fn(),
            pref,
            load_prior(&live_prior_file()),
            &product(),
        );
        log::info!(
            "globe_key status pref={} conflict={}",
            fmt_pref(pref),
            s.conflict.is_some()
        );
        s
    });
    surfaced("status", r)
}

/// The "Turn it off" button. The ONLY caller of the write.
#[tauri::command]
pub async fn globe_key_turn_off() -> Result<GlobeStatus, String> {
    let r = turn_off(
        &SystemDefaults,
        DOMAIN,
        &live_prior_file(),
        ptt_uses_fn(),
        &product(),
    );
    surfaced("turn_off", r)
}

/// The "Put it back" button.
#[tauri::command]
pub async fn globe_key_put_back() -> Result<GlobeStatus, String> {
    let r = put_back(
        &SystemDefaults,
        DOMAIN,
        &live_prior_file(),
        ptt_uses_fn(),
        &product(),
    );
    surfaced("put_back", r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A fake `defaults` over one in-memory value; records every command line.
    struct Fake {
        value: RefCell<Option<i64>>,
        calls: RefCell<Vec<String>>,
        refuse_write: bool,
    }

    impl Fake {
        fn new(v: Option<i64>) -> Self {
            Self {
                value: RefCell::new(v),
                calls: RefCell::new(Vec::new()),
                refuse_write: false,
            }
        }
        fn lines(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl DefaultsRunner for Fake {
        fn run(&self, args: &[String]) -> Result<(bool, String), String> {
            self.calls
                .borrow_mut()
                .push(format!("defaults {}", args.join(" ")));
            assert_eq!(args.get(2).map(String::as_str), Some(KEY), "only {KEY}");
            match args[0].as_str() {
                "read" => Ok(match *self.value.borrow() {
                    Some(v) => (true, format!("{v}\n")),
                    None => (false, String::new()),
                }),
                "write" => {
                    if self.refuse_write {
                        return Ok((false, String::new()));
                    }
                    assert_eq!(args[3], "-int");
                    *self.value.borrow_mut() = Some(args[4].parse().unwrap());
                    Ok((true, String::new()))
                }
                "delete" => {
                    let had = self.value.borrow_mut().take().is_some();
                    Ok((had, String::new()))
                }
                other => panic!("unexpected defaults verb {other}"),
            }
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "yap-globe-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const TEST_DOMAIN: &str = "com.wilsonguenther.yap.globe-test";

    #[test]
    fn globe_conflict_matrix_over_every_value_with_and_without_fn() {
        let cases: [(Option<i64>, Option<ConflictKind>); 5] = [
            (None, Some(ConflictKind::EmojiPicker)),
            (Some(0), None),
            (Some(1), None),
            (Some(2), Some(ConflictKind::EmojiPicker)),
            (Some(3), Some(ConflictKind::Dictation)),
        ];
        for (pref, want) in cases {
            assert_eq!(
                conflict(true, pref).map(|c| c.kind),
                want,
                "fn bound, {pref:?}"
            );
            assert_eq!(conflict(false, pref), None, "fn not bound, {pref:?}");
        }
        assert_eq!(conflict(true, Some(9)), None, "unknown value");
        assert_eq!(conflict(true, None).unwrap().line, EMOJI_LINE);
        assert!(conflict(true, Some(3)).unwrap().line.contains("Dictation"));
    }

    #[test]
    fn globe_every_ptt_binding_uses_fn() {
        for b in ["fn", "globe", "fn_control", "fn_or_fn_control", "both", ""] {
            assert!(binding_uses_fn(b), "{b}");
        }
    }

    #[test]
    fn globe_read_maps_nonzero_exit_to_unset() {
        assert_eq!(read_pref(&Fake::new(None), TEST_DOMAIN), Ok(None));
        assert_eq!(read_pref(&Fake::new(Some(2)), TEST_DOMAIN), Ok(Some(2)));
    }

    #[test]
    fn globe_turn_off_then_put_back_rechecks_the_line_from_unset() {
        let dir = scratch("unset");
        let prior = prior_path(&dir);
        let fake = Fake::new(None);
        let before = status(true, read_pref(&fake, TEST_DOMAIN).unwrap(), None, "Yap");
        assert_eq!(before.line.as_deref(), Some(EMOJI_LINE));
        assert!(before.can_turn_off && !before.can_put_back);

        let after = turn_off(&fake, TEST_DOMAIN, &prior, true, "Yap").unwrap();
        assert_eq!(after.pref, Some(0));
        assert_eq!(after.conflict, None);
        assert_eq!(after.line.as_deref(), Some(TURNED_OFF_LINE));
        assert!(after.can_put_back && !after.can_turn_off);
        assert_eq!(load_prior(&prior), Some(PriorRecord { prior: None }));

        let back = put_back(&fake, TEST_DOMAIN, &prior, true, "Yap").unwrap();
        assert_eq!(back.pref, None);
        assert_eq!(back.line.as_deref(), Some(EMOJI_LINE));
        assert!(!back.can_put_back);
        assert!(!prior.exists(), "record forgotten once restored");
        // The first read is the status line above; turn_off reads again to
        // record the prior value before it writes.
        assert_eq!(
            fake.lines(),
            vec![
                format!("defaults read {TEST_DOMAIN} {KEY}"),
                format!("defaults read {TEST_DOMAIN} {KEY}"),
                format!("defaults write {TEST_DOMAIN} {KEY} -int 0"),
                format!("defaults read {TEST_DOMAIN} {KEY}"),
                format!("defaults delete {TEST_DOMAIN} {KEY}"),
                format!("defaults read {TEST_DOMAIN} {KEY}"),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn globe_put_back_restores_a_recorded_value_and_keeps_the_original() {
        let dir = scratch("dictation");
        let prior = prior_path(&dir);
        let fake = Fake::new(Some(3));
        turn_off(&fake, TEST_DOMAIN, &prior, true, "Yap").unwrap();
        // A second press must not overwrite the original with 0.
        turn_off(&fake, TEST_DOMAIN, &prior, true, "Yap").unwrap();
        assert_eq!(load_prior(&prior), Some(PriorRecord { prior: Some(3) }));
        let back = put_back(&fake, TEST_DOMAIN, &prior, true, "Yap").unwrap();
        assert_eq!(back.pref, Some(3));
        assert_eq!(back.conflict.map(|c| c.kind), Some(ConflictKind::Dictation));
        assert!(fake
            .lines()
            .contains(&format!("defaults write {TEST_DOMAIN} {KEY} -int 3")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn globe_status_never_writes_and_refused_write_is_an_error() {
        let fake = Fake::new(Some(2));
        let _ = status(true, read_pref(&fake, TEST_DOMAIN).unwrap(), None, "Yap");
        assert!(fake.lines().iter().all(|l| l.starts_with("defaults read ")));

        let dir = scratch("refused");
        let mut refusing = Fake::new(Some(2));
        refusing.refuse_write = true;
        assert!(turn_off(&refusing, TEST_DOMAIN, &prior_path(&dir), true, "Yap").is_err());
        assert_eq!(*refusing.value.borrow(), Some(2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn globe_put_back_without_a_record_is_an_error_and_runs_nothing() {
        let dir = scratch("norecord");
        let fake = Fake::new(Some(0));
        assert!(put_back(&fake, TEST_DOMAIN, &prior_path(&dir), true, "Yap").is_err());
        assert!(fake.lines().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn globe_lines_are_product_named() {
        let s = status(true, None, None, "Yap Test");
        assert_eq!(
            s.line.as_deref(),
            Some("Your Mac opens the emoji picker on a lone Fn tap; Yap Test can turn that off")
        );
    }

    #[test]
    fn globe_value_one_and_zero_show_nothing_without_a_record() {
        assert_eq!(status(true, Some(1), None, "Yap").line, None);
        assert_eq!(status(true, Some(0), None, "Yap").line, None);
    }
}
