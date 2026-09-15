//! Y4-H acceptance — **every field of `AppSettings` survives a relaunch, and a
//! new field cannot be added without this test noticing.**
//!
//! The 2026-07-24 senior audit's top user-facing bug was "half of Settings
//! never persist (incl. PTT remap)". The shape of that bug is always the same:
//! a field is added to the struct, the UI writes it, and something between the
//! UI and the disk drops it — a hand-written serializer, a field missing from a
//! `save` path, a map or an `Option` nobody thought about. `tests/settings_kv.rs`
//! covers the meeting KV rows; NOTHING covered `settings.json` field by field.
//!
//! So this file drives the REAL store — `wilson_voice_lib::write_settings_file`
//! then `wilson_voice_lib::load_settings`, against a real file on disk that is
//! written, dropped and re-read — rather than a serde round-trip in memory that
//! a broken writer would still pass.
//!
//! Two of the fields are here by name because they are the classic casualties:
//!
//! * `polish_styles` is a `BTreeMap` — maps are the field a hand-rolled writer
//!   forgets, and the tone dial silently reverting to "default" on every
//!   relaunch is precisely the reported bug class; and
//! * `calibration_sample` is an `Option<String>` — `None` and "absent" look the
//!   same on disk, so a writer that skips `None` and a reader that defaults a
//!   missing key can hide a dropped `Some`.
//!
//! And the guard that makes it stay true: `field_count_forces_this_test_to_be_updated`
//! asserts the SERIALIZED field count against a constant. Add a field to
//! `AppSettings` without populating it below and that assert fails with the new
//! count, which is the only reliable way to make a future author read this file.

use std::collections::BTreeMap;

use wilson_voice_lib::{load_settings, write_settings_file, AppSettings};

/// A private temp dir for one test. `std::env::temp_dir()` plus the test name —
/// no cross-test collisions, and nothing outside the temp dir is touched.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yap-y4h-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// The number of fields `AppSettings` serializes.
///
/// THIS CONSTANT IS THE POINT. When it fails, do not just bump it: add the new
/// field to `fully_populated()` with a non-default value and assert it in
/// `every_app_settings_field_round_trips`, THEN bump it.
const APP_SETTINGS_FIELD_COUNT: usize = 31;

/// Every field set to something that is NOT its default. A field that happens
/// to equal its default proves nothing — the store could drop it and the
/// assertion would still pass.
fn fully_populated() -> AppSettings {
    let mut polish_styles = BTreeMap::new();
    polish_styles.insert("email".to_string(), "formal".to_string());
    polish_styles.insert("chat".to_string(), "very casual".to_string());
    polish_styles.insert("notes".to_string(), "casual".to_string());

    AppSettings {
        // Y4-A — the CURRENT schema version. This fixture stands for "a store
        // the running app wrote", so it must not be a stale one: a store
        // pinned below CURRENT is by definition migrated on load, and this
        // test asserts a byte-for-byte round trip, not a migration.
        schema_version: 2,
        language: "fr".into(),
        auto_paste: false,
        hotkey_label: "fn".into(),
        show_floating_pill: false,
        ptt_binding: "fn".into(),
        command_binding: "option".into(),
        keep_cmd_shift_v: true,
        pill_style: "yappy".into(),
        pill_position: "right".into(),
        companion_tone: "rude".into(),
        dictation_mode: "email".into(),
        cleanup_level: "high".into(),
        // Y4-A — both non-default (the shipped values are `false`), so the
        // round-trip proves the store actually carries them. These two are
        // the provenance bit the v1 -> v2 migration writes and the one-shot
        // flag that shows the "formatting is on now" line exactly once.
        cleanup_level_set_by_user: true,
        formatting_notice_pending: true,
        snippet_scope: "utterance".into(),
        polish_model: "qwen2.5-1.5b-instruct".into(),
        polish_deadline_ms: 2500,
        polish_styles,
        signature: "Wilson — drivia.consulting".into(),
        signature_mode: "auto".into(),
        denoise: false,
        mute_while_dictating: false,
        onboarded: true,
        calibration_sample: Some("the quick brown fox".into()),
        native_model: "ggml-small.en".into(),
        preload_model: true,
        autostart: true,
        check_updates: false,
        skipped_update_version: Some("9.9.9".into()),
        legacy_json_migrated: true,
    }
}

/// The struct as a JSON object, for field-by-field comparison without writing
/// 29 `assert_eq!` lines that the next author would have to remember to extend.
fn as_object(s: &AppSettings) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::to_value(s).expect("AppSettings serializes") {
        serde_json::Value::Object(map) => map,
        other => panic!("AppSettings must serialize to a JSON object, got {other:?}"),
    }
}

/// The guard. A new field that is not populated above makes this fail with the
/// real count, which is the message a future author needs.
#[test]
fn field_count_forces_this_test_to_be_updated() {
    let defaults = as_object(&AppSettings::default());
    assert_eq!(
        defaults.len(),
        APP_SETTINGS_FIELD_COUNT,
        "AppSettings now serializes {} fields, not {APP_SETTINGS_FIELD_COUNT}. \
         A field was added or removed: populate it in `fully_populated()` with a \
         NON-DEFAULT value, make sure `every_app_settings_field_round_trips` \
         covers it, and only then update APP_SETTINGS_FIELD_COUNT. Fields now: {:?}",
        defaults.len(),
        defaults.keys().collect::<Vec<_>>()
    );

    // And the populated struct must differ from the defaults in EVERY field —
    // otherwise a field could be "covered" by a value the store never had to
    // carry. `schema_version` is the one exception: its only valid value is the
    // current one, and the migration test below is what proves it moves.
    let populated = as_object(&fully_populated());
    let same: Vec<&String> = defaults
        .iter()
        .filter(|(k, v)| k.as_str() != "schemaVersion" && populated.get(*k) == Some(*v))
        .map(|(k, _)| k)
        .collect();
    assert!(
        same.is_empty(),
        "these fields are still at their default in `fully_populated()`, so the \
         round-trip test cannot tell a dropped field from a kept one: {same:?}"
    );
}

/// The headline: write every field, throw the store away, read it back.
#[test]
fn every_app_settings_field_round_trips() {
    let dir = temp_dir("round-trip");
    let path = dir.join("settings.json");
    let written = fully_populated();

    write_settings_file(&path, &written).expect("settings written");

    // The store is a real file; this is what a relaunch sees.
    let raw = std::fs::read_to_string(&path).expect("settings.json exists on disk");
    assert!(
        raw.contains("polishStyles"),
        "polish_styles must reach the file — a map is the field a writer drops"
    );
    assert!(
        raw.contains("calibrationSample"),
        "calibration_sample (an Option) must reach the file"
    );

    let read_back = load_settings(&path);

    let before = as_object(&written);
    let after = as_object(&read_back);
    let mut dropped: Vec<String> = Vec::new();
    for (key, value) in &before {
        if after.get(key) != Some(value) {
            dropped.push(format!("{key}: wrote {value:?}, read {:?}", after.get(key)));
        }
    }
    assert!(
        dropped.is_empty(),
        "these settings did not survive a relaunch: {dropped:#?}"
    );
    assert_eq!(
        after.len(),
        APP_SETTINGS_FIELD_COUNT,
        "the reloaded struct must still carry every field"
    );

    // Spelled out for the two named casualties, so a failure names the bug
    // rather than a diff of 29 keys.
    assert_eq!(
        read_back.polish_styles, written.polish_styles,
        "the tone dial (polish_styles, a BTreeMap) must survive a relaunch"
    );
    assert_eq!(
        read_back.calibration_sample, written.calibration_sample,
        "calibration_sample (an Option) must survive a relaunch"
    );
    assert_eq!(read_back.polish_deadline_ms, 2500);
    assert_eq!(read_back.cleanup_level, "high");
    assert_eq!(read_back.signature_mode, "auto");
    assert_eq!(read_back.snippet_scope, "utterance");
}

/// A second write must not lose the first one's fields either — the bug also
/// shows up as "it persisted once, then a later save reset half of it".
#[test]
fn a_second_save_does_not_drop_the_first_ones_fields() {
    let dir = temp_dir("second-save");
    let path = dir.join("settings.json");

    write_settings_file(&path, &fully_populated()).expect("first write");
    let mut next = load_settings(&path);
    next.language = "de".into();
    write_settings_file(&path, &next).expect("second write");

    let read_back = load_settings(&path);
    assert_eq!(read_back.language, "de", "the change itself landed");
    assert_eq!(
        read_back.polish_styles,
        fully_populated().polish_styles,
        "an unrelated save must not clear the tone dial"
    );
    assert_eq!(
        read_back.calibration_sample,
        fully_populated().calibration_sample
    );
    assert_eq!(read_back.signature, fully_populated().signature);
}

/// The migration half: a store written before the new fields existed loads with
/// the documented defaults for what it lacks, and keeps every value it carries.
///
/// The blob below is deliberately a PRE-`schemaVersion` object (version 0), so
/// `apply_settings_migrations` runs — the path that has the power to reset
/// things and must not use it on fields the user actually set.
#[test]
fn old_blob_migrates_without_resetting_carried_fields() {
    let dir = temp_dir("migrate");
    let path = dir.join("settings.json");

    // No `schemaVersion`; no `polishStyles`, `polishModel`, `polishDeadlineMs`,
    // `signature`, `signatureMode`, `snippetScope`, `calibrationSample` — every
    // one of them added after this blob was written.
    let old = serde_json::json!({
        "language": "fr",
        "autoPaste": false,
        "hotkeyLabel": "fn",
        "showFloatingPill": false,
        "pttBinding": "fn",
        "cleanupLevel": "medium",
        "companionTone": "rude",
        "pillStyle": "yappy",
        "denoise": false,
        "onboarded": true,
        "nativeModel": "ggml-small.en"
    });
    std::fs::write(&path, serde_json::to_string_pretty(&old).unwrap()).expect("old blob written");

    let loaded = load_settings(&path);
    let defaults = AppSettings::default();

    // (a) Everything the old blob carried is still there.
    assert_eq!(loaded.language, "fr");
    assert!(!loaded.auto_paste);
    assert_eq!(loaded.hotkey_label, "fn");
    assert!(!loaded.show_floating_pill);
    assert_eq!(loaded.ptt_binding, "fn");
    assert_eq!(loaded.cleanup_level, "medium");
    assert_eq!(loaded.companion_tone, "rude");
    assert_eq!(loaded.pill_style, "yappy");
    assert!(!loaded.denoise);
    assert!(loaded.onboarded);

    // (b) Everything it did not carry is at its DOCUMENTED default, not empty
    //     and not garbage.
    assert_eq!(loaded.polish_styles, defaults.polish_styles);
    assert!(
        loaded.polish_styles.is_empty(),
        "no tone overrides by default"
    );
    assert_eq!(
        loaded.polish_model, "",
        "the polish model is OFF by default"
    );
    assert_eq!(loaded.polish_deadline_ms, defaults.polish_deadline_ms);
    assert_eq!(loaded.signature, "");
    assert_eq!(loaded.signature_mode, "off");
    assert_eq!(loaded.snippet_scope, "inline");
    assert_eq!(loaded.calibration_sample, None);
    assert_eq!(loaded.command_binding, defaults.command_binding);
    assert_eq!(loaded.pill_position, defaults.pill_position);

    // (c) The migration stamped the current version and rewrote the file once,
    //     so a relaunch is a no-op rather than a second migration.
    assert_eq!(loaded.schema_version, AppSettings::default().schema_version);
    let rewritten = std::fs::read_to_string(&path).expect("the file was rewritten in place");
    assert!(
        rewritten.contains("schemaVersion"),
        "the migrated store carries its version"
    );
    let again = load_settings(&path);
    assert_eq!(again.language, "fr", "a relaunch changes nothing");
    assert_eq!(again.cleanup_level, "medium");
    assert_eq!(again.schema_version, loaded.schema_version);
}
