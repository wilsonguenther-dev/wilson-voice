//! yap-byom-0 — the provider model's contract.
//!
//! * the "Leaves this Mac" table (loopback = No, LAN IP = Yes, https host =
//!   Yes, loopback + an Ollama cloud model = Yes — ollama.com);
//! * a settings round-trip through the REAL `settings_kv` never contains a key;
//! * keychain set/get/delete through the in-memory fake (no Keychain write in
//!   CI), account = id + host, a host change deletes the key, and the service
//!   name follows the bundle id;
//! * Bundled is always present and cannot be deleted; unwired kinds are hidden.

mod support;

use support::{open_db, temp_dir};
use wilson_voice_lib::channel::Channel;
use wilson_voice_lib::keychain::{
    account_for, key_hint, service_for, MemoryKeychain, SecretStore, PRODUCTION_SERVICE,
};
use wilson_voice_lib::model_providers::{
    delete_provider, leaves_this_mac, list_view, load, set_task_provider, upsert_provider,
    ProviderConfig, ProviderError, ProviderKind, Task, BUNDLED_ID, DEFAULT_CONTEXT_TOKENS,
    PROVIDERS_KEY,
};

fn oa(id: &str, base_url: &str, model: &str) -> ProviderConfig {
    ProviderConfig {
        id: id.into(),
        kind: ProviderKind::OpenAiCompatible,
        label: format!("{id} label"),
        base_url: base_url.into(),
        model: model.into(),
        context_tokens: 0,
        timeout_s: 0,
    }
}

fn all_settings_rows(dir: &std::path::Path) -> Vec<(String, String)> {
    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    let mut st = conn.prepare("SELECT key, value FROM settings_kv").unwrap();
    let rows = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    rows
}

#[test]
fn leaves_this_mac_table() {
    let cases: &[(ProviderConfig, &str)] = &[
        (ProviderConfig::bundled(), "No"),
        (oa("a", "http://127.0.0.1:11434/v1", "llama3.2"), "No"),
        (oa("b", "http://localhost:11434/v1", "qwen2.5"), "No"),
        (oa("c", "http://[::1]:8080/v1", "qwen2.5"), "No"),
        (
            oa("d", "http://192.168.1.20:11434/v1", "llama3.2"),
            "Yes — 192.168.1.20",
        ),
        (
            oa("e", "https://openrouter.ai/api/v1", "openai/gpt-4o-mini"),
            "Yes — openrouter.ai",
        ),
        (
            oa("f", "http://127.0.0.1:11434/v1", "gpt-oss:120b-cloud"),
            "Yes — ollama.com",
        ),
        (
            oa("g", "http://localhost:11434/v1", "kimi-k2:cloud"),
            "Yes — ollama.com",
        ),
        (oa("h", "not a url", "m"), "Yes — an unknown host"),
    ];
    for (cfg, want) in cases {
        assert_eq!(leaves_this_mac(cfg).label(), *want, "{}", cfg.base_url);
    }
}

#[test]
fn a_settings_round_trip_never_contains_a_key() {
    let dir = temp_dir("byom-roundtrip");
    let db = open_db(&dir);
    let keys = MemoryKeychain::new(PRODUCTION_SERVICE);
    let key = "sk-ROUNDTRIP-0123456789abcdefQRST";
    let saved = upsert_provider(
        &db,
        &keys,
        oa(
            "openrouter",
            "https://openrouter.ai/api/v1",
            "openai/gpt-4o-mini",
        ),
        Some(key),
    )
    .expect("save");
    assert_eq!(saved.context_tokens, DEFAULT_CONTEXT_TOKENS);
    set_task_provider(&db, Task::Notes, "openrouter").unwrap();
    drop(db);

    // Reopen: the provider survives a relaunch.
    let db = open_db(&dir);
    let s = load(&db);
    assert_eq!(s.providers[0].id, BUNDLED_ID, "Bundled is always first");
    assert_eq!(s.find("openrouter"), Some(&saved));
    assert_eq!(s.notes_provider, "openrouter");
    assert_eq!(s.polish_provider, "", "empty = Bundled");

    let rows = all_settings_rows(&dir);
    assert!(rows.iter().any(|(k, _)| k == PROVIDERS_KEY), "{rows:?}");
    for (k, v) in &rows {
        assert!(
            !v.contains(key) && !v.contains("ROUNDTRIP"),
            "settings_kv {k} holds the key"
        );
    }
    // The key is in the (fake) Keychain, under id + host.
    assert_eq!(
        keys.accounts(),
        vec!["openrouter@openrouter.ai".to_string()]
    );
    assert_eq!(
        keys.get("openrouter@openrouter.ai").unwrap().as_deref(),
        Some(key)
    );

    // The view shows "saved" + last 4, and its JSON never holds the key.
    let view = list_view(&db, &keys);
    let row = view
        .providers
        .iter()
        .find(|p| p.id == "openrouter")
        .unwrap();
    assert!(row.key_saved);
    assert_eq!(row.key_hint, "QRST");
    assert_eq!(row.leaves_label, "Yes — openrouter.ai");
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains(key));
    assert_eq!(view.active_leaves.len(), 1, "notes leave, polish does not");
}

#[test]
fn keychain_set_get_delete_through_the_fake() {
    let keys = MemoryKeychain::new(PRODUCTION_SERVICE);
    let account = account_for("groq", "API.groq.com");
    assert_eq!(account, "groq@api.groq.com", "account = id + host");
    assert_eq!(keys.get(&account).unwrap(), None);
    keys.set(&account, "gsk_abc").unwrap();
    assert_eq!(keys.get(&account).unwrap().as_deref(), Some("gsk_abc"));
    keys.delete(&account).unwrap();
    assert_eq!(keys.get(&account).unwrap(), None);
    keys.delete(&account)
        .expect("deleting a missing item is not an error");
    assert_eq!(key_hint("sk-1234567890WXYZ"), "WXYZ");
    assert_eq!(key_hint("abcd"), "", "a 4-char key shows nothing");
}

#[test]
fn a_host_change_deletes_the_key() {
    let dir = temp_dir("byom-hostchange");
    let db = open_db(&dir);
    let keys = MemoryKeychain::new(PRODUCTION_SERVICE);
    upsert_provider(
        &db,
        &keys,
        oa("p", "https://api.example.com/v1", "m"),
        Some("sk-first-1111"),
    )
    .unwrap();
    // Same host, different path: the key stays.
    upsert_provider(&db, &keys, oa("p", "https://api.example.com/v2", "m"), None).unwrap();
    assert_eq!(keys.accounts(), vec!["p@api.example.com".to_string()]);
    // New host: the old key is gone and none is carried over.
    upsert_provider(
        &db,
        &keys,
        oa("p", "https://evil.example.net/v1", "m"),
        None,
    )
    .unwrap();
    assert!(keys.accounts().is_empty(), "{:?}", keys.accounts());
    let view = list_view(&db, &keys);
    assert!(
        !view
            .providers
            .iter()
            .find(|p| p.id == "p")
            .unwrap()
            .key_saved
    );
}

#[test]
fn the_service_name_follows_the_bundle_id() {
    let prod = Channel::Production.profile().identifier;
    let test = Channel::Test.profile().identifier;
    assert_eq!(service_for(prod), PRODUCTION_SERVICE);
    assert_eq!(PRODUCTION_SERVICE, "com.wilsonguenther.wilson-voice.byom");
    assert_eq!(
        service_for(test),
        "com.wilsonguenther.wilson-voice.test.byom"
    );
    assert_ne!(
        service_for(test),
        PRODUCTION_SERVICE,
        "a test build never touches real keys"
    );
    assert_eq!(
        wilson_voice_lib::keychain::MacKeychain::for_bundle(test).service(),
        service_for(test)
    );
}

#[test]
fn bundled_is_always_present_and_cannot_be_deleted() {
    let dir = temp_dir("byom-bundled");
    let db = open_db(&dir);
    let keys = MemoryKeychain::new(PRODUCTION_SERVICE);
    assert_eq!(load(&db).providers, vec![ProviderConfig::bundled()]);
    assert_eq!(
        delete_provider(&db, &keys, BUNDLED_ID),
        Err(ProviderError::CannotDeleteBundled)
    );
    assert_eq!(
        delete_provider(&db, &keys, ""),
        Err(ProviderError::CannotDeleteBundled)
    );
    assert!(upsert_provider(&db, &keys, ProviderConfig::bundled(), None).is_err());
    // A malformed stored list still yields Bundled.
    db.setting_set(PROVIDERS_KEY, "{not json").unwrap();
    assert_eq!(load(&db).providers[0].id, BUNDLED_ID);
    // Unwired kinds are hidden from the UI.
    assert!(ProviderKind::Bundled.is_wired());
    assert!(!ProviderKind::OpenAiCompatible.is_wired());
    assert_eq!(
        list_view(&db, &keys).wired_kinds,
        vec![ProviderKind::Bundled]
    );
}

#[test]
fn deleting_a_provider_deletes_its_key_and_resets_the_picks() {
    let dir = temp_dir("byom-delete");
    let db = open_db(&dir);
    let keys = MemoryKeychain::new(PRODUCTION_SERVICE);
    upsert_provider(
        &db,
        &keys,
        oa("x", "https://x.ai/v1", "grok"),
        Some("xai-12345678"),
    )
    .unwrap();
    set_task_provider(&db, Task::Polish, "x").unwrap();
    assert!(set_task_provider(&db, Task::Notes, "nope").is_err());
    delete_provider(&db, &keys, "x").unwrap();
    assert!(keys.accounts().is_empty());
    let s = load(&db);
    assert_eq!(s.polish_provider, "");
    assert_eq!(s.for_task(Task::Polish).id, BUNDLED_ID);
}
