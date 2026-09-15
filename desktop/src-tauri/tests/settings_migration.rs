//! Y4-A — the v1 → v2 settings migration, proven on a REAL settings.json.
//!
//! ## Why this file exists
//!
//! Y4-A raises the shipped `cleanup_level` from `"light"` to `"medium"` so a
//! fresh install reaches `CleanupLevel::runs_format()`. A changed default only
//! ever reaches a machine that has never saved settings — and the machine that
//! reported "formatting does not work whatsoever" is not one of those. Measured
//! on the reporter's own install, `~/Library/Application Support/WilsonVoice/
//! settings.json` holds `"cleanupLevel": "light"` at `"schemaVersion": 1`. A fix
//! that only reaches fresh installs does not fix the complaint that motivated
//! it, so the stored value has to move too.
//!
//! ## Why the migration is UNCONDITIONAL, and why that is not reckless
//!
//! The obvious guard — "migrate only a value the user never explicitly set" —
//! is unimplementable against a v1 store, which is the same thing as doing
//! nothing:
//!
//! * `save_settings` persists the WHOLE `AppSettings` struct, so `cleanupLevel`
//!   is present in every store from the first save onward. Presence proves
//!   nothing.
//! * v1 records no provenance anywhere. There is no bit, anywhere on disk,
//!   separating a `"light"` the user picked from the `"light"` the app shipped.
//!
//! So v1 → v2 rewrites `"light"` with no condition — the old value was a
//! default nobody could be shown to have chosen — and v2 starts recording
//! provenance (`cleanup_level_set_by_user`, written only by the settings
//! command behind the picker) so the NEXT default change can be honest. That is
//! what [`an_explicit_user_choice_is_never_rewritten`] pins.
//!
//! These tests drive `load_settings` over a real file in a temp dir rather than
//! calling `apply_settings_migrations` on a struct, because the claim is about
//! what happens to a store on disk at launch — including the atomic rewrite.

use std::path::PathBuf;
use wilson_voice_lib::load_settings;

/// A unique scratch directory per test — no shared fixture, no ordering.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yap-settings-migration-{name}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn write(path: &std::path::Path, json: &str) {
    std::fs::write(path, json).expect("write settings.json");
}

fn stored(path: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("read back"))
        .expect("settings.json is still valid JSON after the migration")
}

/// THE ITEM'S CENTRAL CLAIM. A v1 store carrying `"light"` — the exact shape of
/// the reporter's install — comes back as `"medium"`, is rewritten to disk at
/// v2, and arms the one-line notice.
///
/// This is red without the migration: `serde` deserialises the stored
/// `"light"` straight into the struct, so a raised DEFAULT alone changes
/// nothing at all for this file.
#[test]
fn a_v1_store_with_light_loads_as_medium() {
    let dir = scratch("v1-light");
    let path = dir.join("settings.json");
    // A v1 store with the level the app used to ship. `autoPaste` rides along
    // to prove the migration is surgical: it rewrites one field, not the file.
    write(
        &path,
        r#"{"schemaVersion":1,"cleanupLevel":"light","autoPaste":false,"pillStyle":"classic"}"#,
    );

    let settings = load_settings(&path);

    assert_eq!(
        settings.cleanup_level, "medium",
        "a v1 store's \"light\" must be raised to \"medium\": it never reached the formatting \
         stage, which is the whole defect Y4-A fixes"
    );
    assert!(
        settings.formatting_notice_pending,
        "the migration changed behaviour the user will SEE, so it must arm the one line that \
         explains it and says where to turn it off"
    );
    assert!(
        !settings.cleanup_level_set_by_user,
        "the migration must not claim the user chose \"medium\" — it chose it FOR them, and \
         forging that bit would immunise the value against every future migration"
    );
    assert!(
        !settings.auto_paste,
        "the migration rewrote an unrelated stored field; it must carry every other value through \
         untouched"
    );

    // The rewrite actually landed, so the next launch is a no-op rather than a
    // migration that runs forever.
    let on_disk = stored(&path);
    assert_eq!(on_disk["schemaVersion"], 2, "store rewritten at v2");
    assert_eq!(on_disk["cleanupLevel"], "medium", "rewritten on disk");

    // Idempotent: loading the rewritten file again changes nothing.
    let again = load_settings(&path);
    assert_eq!(again.cleanup_level, "medium");
    assert_eq!(stored(&path)["schemaVersion"], 2);
}

/// The other half of the contract: once provenance EXISTS, it is honoured.
///
/// A v2 store is already at the current schema version, so
/// `apply_settings_migrations` returns before any arm runs — a user who picked
/// a level keeps it, including the one v1 → v2 rewrote for everyone else.
#[test]
fn an_explicit_user_choice_is_never_rewritten() {
    let dir = scratch("v2-chosen");
    let path = dir.join("settings.json");
    // A user who deliberately moved the picker to Light and saved.
    write(
        &path,
        r#"{"schemaVersion":2,"cleanupLevel":"light","cleanupLevelSetByUser":true}"#,
    );

    let settings = load_settings(&path);

    assert_eq!(
        settings.cleanup_level, "light",
        "a level the user chose, recorded as chosen at v2, must survive every launch — the \
         unconditional v1 arm is a statement about UNKNOWABLE provenance, not a licence to \
         overrule a known one"
    );
    assert!(
        settings.cleanup_level_set_by_user,
        "the provenance bit must round-trip, or the next default change re-opens this question"
    );
    assert!(
        !settings.formatting_notice_pending,
        "nothing changed, so there is nothing to tell the user about"
    );
    assert_eq!(
        stored(&path)["cleanupLevel"],
        "light",
        "the file itself must be untouched"
    );

    // "none" is verbatim mode — a real user need, and the value most likely to
    // be chosen on purpose. It is never touched at ANY version, because only
    // "light" moves.
    let none_dir = scratch("v1-none");
    let none_path = none_dir.join("settings.json");
    write(&none_path, r#"{"schemaVersion":1,"cleanupLevel":"none"}"#);
    let none = load_settings(&none_path);
    assert_eq!(
        none.cleanup_level, "none",
        "verbatim mode must survive the v1 arm: only \"light\" is migrated"
    );
    assert!(
        !none.formatting_notice_pending,
        "nothing was changed for this user, so no notice"
    );
}
