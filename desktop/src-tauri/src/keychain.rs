//! yap-byom-0 — API keys for a model provider you choose, in the macOS Keychain.
//!
//! A key is a secret the user typed once. It lives in exactly ONE place: a
//! generic-password item in the login Keychain. It never reaches SQLite, the
//! settings store, `yap.log`, a crash report, a support bundle or the webview
//! after it is saved — the UI only ever sees "Key saved" and the last four
//! characters ([`key_hint`]).
//!
//! **Service name.** Derived from the RUNNING bundle id ([`service_for`]): the
//! shipped app reads and writes `com.wilsonguenther.wilson-voice.byom`, the
//! Yap Test channel reads and writes its own bundle id + `.byom` — the same
//! rule PR 248 set for the test channel's data dir, so a test build can never
//! read, overwrite or delete the real app's keys.
//!
//! **Account.** `<provider id>@<host>` ([`account_for`]). The host is part of
//! the account on purpose: a key is bound to the host it was entered for, so
//! pointing a provider at a different host cannot carry the old key there.
//! `model_providers::upsert_provider` deletes the item when the host changes,
//! and the user re-enters the key for the new host.
//!
//! The store is behind [`SecretStore`] so CI never writes a real Keychain item:
//! the tests use [`MemoryKeychain`]; the app uses [`MacKeychain`].

use std::collections::HashMap;
use std::sync::Mutex;

/// The service name the SHIPPED app uses (`com.wilsonguenther.wilson-voice` +
/// [`SERVICE_SUFFIX`]). Spelled out so a grep for the production service finds
/// it; [`service_for`] derives the same string from the bundle id.
pub const PRODUCTION_SERVICE: &str = "com.wilsonguenther.wilson-voice.byom";

/// Appended to the running bundle id to name the Keychain service.
pub const SERVICE_SUFFIX: &str = ".byom";

/// The Keychain service for a running bundle id.
pub fn service_for(bundle_id: &str) -> String {
    format!("{}{SERVICE_SUFFIX}", bundle_id.trim())
}

/// The Keychain account for a provider: its id, bound to its host.
pub fn account_for(provider_id: &str, host: &str) -> String {
    format!(
        "{}@{}",
        provider_id.trim(),
        host.trim().to_ascii_lowercase()
    )
}

/// What the UI may show of a saved key: its last four characters, never more.
/// A key of four characters or fewer shows nothing at all.
pub fn key_hint(key: &str) -> String {
    let chars: Vec<char> = key.trim().chars().collect();
    if chars.len() <= 4 {
        return String::new();
    }
    chars[chars.len() - 4..].iter().collect()
}

/// Set / get / delete one secret. `get` of a missing item is `Ok(None)`, and
/// `delete` of a missing item is `Ok(())` — both are the normal state of a
/// provider that never had a key.
pub trait SecretStore: Send + Sync {
    /// The service name this store reads and writes.
    fn service(&self) -> &str;
    fn set(&self, account: &str, secret: &str) -> Result<(), String>;
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// An in-memory store for tests: no Keychain write ever happens in CI.
pub struct MemoryKeychain {
    service: String,
    items: Mutex<HashMap<String, String>>,
}

impl MemoryKeychain {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            items: Mutex::new(HashMap::new()),
        }
    }

    /// Every account currently holding a secret (tests assert on this).
    pub fn accounts(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .items
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        v.sort();
        v
    }
}

impl SecretStore for MemoryKeychain {
    fn service(&self) -> &str {
        &self.service
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        self.items
            .lock()
            .map_err(|_| "keychain lock poisoned".to_string())?
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        Ok(self
            .items
            .lock()
            .map_err(|_| "keychain lock poisoned".to_string())?
            .get(account)
            .cloned())
    }
    fn delete(&self, account: &str) -> Result<(), String> {
        self.items
            .lock()
            .map_err(|_| "keychain lock poisoned".to_string())?
            .remove(account);
        Ok(())
    }
}

/// The real store: the login Keychain, through security-framework's
/// generic-password API. Errors carry the OSStatus, never the secret.
pub struct MacKeychain {
    service: String,
}

impl MacKeychain {
    /// The store for the running bundle id (see [`service_for`]).
    pub fn for_bundle(bundle_id: &str) -> Self {
        Self {
            service: service_for(bundle_id),
        }
    }
}

/// `errSecItemNotFound` — the item does not exist.
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(target_os = "macos")]
impl SecretStore for MacKeychain {
    fn service(&self) -> &str {
        &self.service
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        security_framework::passwords::set_generic_password(
            &self.service,
            account,
            secret.as_bytes(),
        )
        .map_err(|e| {
            log::error!("keychain set failed status={}", e.code());
            format!(
                "Yap could not save the key in your Keychain (status {}).",
                e.code()
            )
        })
    }
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        match security_framework::passwords::get_generic_password(&self.service, account) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "The saved key is not valid text; enter it again.".to_string()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => {
                log::error!("keychain get failed status={}", e.code());
                Err(format!(
                    "Yap could not read the key from your Keychain (status {}).",
                    e.code()
                ))
            }
        }
    }
    fn delete(&self, account: &str) -> Result<(), String> {
        match security_framework::passwords::delete_generic_password(&self.service, account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => {
                log::error!("keychain delete failed status={}", e.code());
                Err(format!(
                    "Yap could not delete the key from your Keychain (status {}).",
                    e.code()
                ))
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl SecretStore for MacKeychain {
    fn service(&self) -> &str {
        &self.service
    }
    fn set(&self, _account: &str, _secret: &str) -> Result<(), String> {
        Err("The Keychain is only available on macOS.".into())
    }
    fn get(&self, _account: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn delete(&self, _account: &str) -> Result<(), String> {
        Ok(())
    }
}
