//! yap-1106-fb-14-news-quote-key — Yappy News: an optional own key for the
//! quote feed.
//!
//! Decision (delegated by Wilson, "I'll leave that up to you"): the built-in
//! free sources stay the default and nothing needs a key. Settings → Yappy
//! News → Advanced accepts ONE optional key for Finnhub or Alpha Vantage. With
//! it, tickers get prices at that provider's limit (never more than once per
//! ticker per 30 min, see [`super::feeds::QuoteFeed::limits`]); without it the
//! free tier applies ([`super::feeds::QuoteFeed::Free`]).
//!
//! **Where the key lives.** Only in the login Keychain, service
//! `<bundle id>.news` ([`keychain_service_for`]) so it can never collide with a
//! model-provider key (`.byom`) or the license seat (`.license`), and a test
//! channel keeps its own. Account `news-quotes@<provider host>`
//! ([`account_for`]): the key is bound to the one host it may be sent to. The
//! settings store holds only WHICH provider is chosen ([`PROVIDER_KEY`]). The
//! webview only ever sees "Key saved" plus the last four characters.
//!
//! Telemetry: `news quote_key_saved provider=…`, `news quote_key_cleared`,
//! `news quote_key_rejected reason=…`, `news quote_key_read_failed provider=…`.
//! Never the key, never a URL.

use serde::{Deserialize, Serialize};

use super::feeds::{QuoteFeed, ALPHA_VANTAGE_HOST, FINNHUB_HOST};
use crate::keychain::{self, SecretStore};
use crate::model_providers::SettingsKv;

/// Settings row naming the chosen provider (`finnhub` / `alpha_vantage`, or
/// empty). Never the key.
pub const PROVIDER_KEY: &str = "news.quote_provider";
/// Appended to the running bundle id to name this Keychain service.
pub const KEYCHAIN_SERVICE_SUFFIX: &str = ".news";
/// The Keychain account prefix (`news-quotes@<host>`).
pub const ACCOUNT_ID: &str = "news-quotes";
/// The label beside the key field (acceptance wording, verbatim).
pub const LEAVES_LABEL: &str = "Leaves this Mac: only to that provider";
/// Shortest and longest key we accept (both providers issue 16–40 chars).
pub const MIN_KEY_LEN: usize = 8;
pub const MAX_KEY_LEN: usize = 128;

/// The Keychain service for a running bundle id.
pub fn keychain_service_for(bundle_id: &str) -> String {
    format!("{}{KEYCHAIN_SERVICE_SUFFIX}", bundle_id.trim())
}

/// A quote provider the user can bring a key for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuoteProvider {
    Finnhub,
    AlphaVantage,
}

impl QuoteProvider {
    pub const ALL: [QuoteProvider; 2] = [QuoteProvider::Finnhub, QuoteProvider::AlphaVantage];

    pub fn id(self) -> &'static str {
        match self {
            QuoteProvider::Finnhub => "finnhub",
            QuoteProvider::AlphaVantage => "alpha_vantage",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id.trim())
    }

    /// The provider's own name (a data vendor, not an AI vendor).
    pub fn label(self) -> &'static str {
        match self {
            QuoteProvider::Finnhub => "Finnhub",
            QuoteProvider::AlphaVantage => "Alpha Vantage",
        }
    }

    /// The one host this provider's key is ever sent to.
    pub fn host(self) -> &'static str {
        match self {
            QuoteProvider::Finnhub => FINNHUB_HOST,
            QuoteProvider::AlphaVantage => ALPHA_VANTAGE_HOST,
        }
    }

    /// One plain sentence about how often prices refresh with this key.
    pub fn refresh_line(self) -> &'static str {
        match self {
            QuoteProvider::Finnhub => {
                "Free plan: 60 calls a minute. Yap checks each ticker at most every 30 minutes."
            }
            QuoteProvider::AlphaVantage => {
                "Free plan: 25 calls a day. Yap checks one ticker an hour, each at most every 30 minutes."
            }
        }
    }

    /// The feed a refresh uses with this provider's key.
    pub fn feed(self, key: String) -> QuoteFeed {
        match self {
            QuoteProvider::Finnhub => QuoteFeed::Finnhub(key),
            QuoteProvider::AlphaVantage => QuoteFeed::AlphaVantage(key),
        }
    }
}

/// The Keychain account for a provider's key, bound to its host.
pub fn account_for(p: QuoteProvider) -> String {
    keychain::account_for(ACCOUNT_ID, p.host())
}

/// One provider as the Advanced panel lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: QuoteProvider,
    pub label: &'static str,
    pub host: &'static str,
    pub refresh_line: &'static str,
}

/// What Settings → Yappy News → Advanced shows. Never the key itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteKeyView {
    /// The chosen provider, if any.
    pub provider: Option<QuoteProvider>,
    /// A key for that provider is in the Keychain.
    pub saved: bool,
    /// The last four characters of the saved key ("" when none).
    pub hint: String,
    /// `free`, `finnhub` or `alpha_vantage` — what the next refresh uses.
    pub tier: &'static str,
    pub leaves_label: &'static str,
    pub providers: Vec<ProviderInfo>,
    /// A Keychain read failure, as one sentence.
    pub error: Option<String>,
}

/// The chosen provider, from the settings store.
pub fn selected(kv: &dyn SettingsKv) -> Option<QuoteProvider> {
    kv.kv_get(PROVIDER_KEY)
        .and_then(|s| QuoteProvider::from_id(&s))
}

fn providers() -> Vec<ProviderInfo> {
    QuoteProvider::ALL
        .into_iter()
        .map(|p| ProviderInfo {
            id: p,
            label: p.label(),
            host: p.host(),
            refresh_line: p.refresh_line(),
        })
        .collect()
}

/// The saved key for the chosen provider, trimmed, if any.
fn saved_key(
    kv: &dyn SettingsKv,
    store: &dyn SecretStore,
) -> (Option<QuoteProvider>, Result<Option<String>, String>) {
    let Some(p) = selected(kv) else {
        return (None, Ok(None));
    };
    let key = store
        .get(&account_for(p))
        .map(|k| k.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()));
    (Some(p), key)
}

/// The Advanced panel's view.
pub fn view(kv: &dyn SettingsKv, store: &dyn SecretStore) -> QuoteKeyView {
    let (provider, key) = saved_key(kv, store);
    let (saved, hint, tier, error) = match (provider, key) {
        (Some(p), Ok(Some(k))) => (true, keychain::key_hint(&k), p.feed(k).tier(), None),
        (_, Ok(_)) => (false, String::new(), QuoteFeed::Free.tier(), None),
        (_, Err(e)) => (false, String::new(), QuoteFeed::Free.tier(), Some(e)),
    };
    QuoteKeyView {
        provider,
        saved,
        hint,
        tier,
        leaves_label: LEAVES_LABEL,
        providers: providers(),
        error,
    }
}

/// Check a typed key. The error never repeats the key.
pub fn validate_key(key: &str) -> Result<String, String> {
    let k = key.trim();
    if k.is_empty() {
        return Err("Paste the key from your quote provider first.".into());
    }
    if k.chars().count() < MIN_KEY_LEN || k.chars().count() > MAX_KEY_LEN {
        return Err(format!(
            "That doesn't look like a quote key: keys are {MIN_KEY_LEN} to {MAX_KEY_LEN} characters."
        ));
    }
    if !k
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(
            "That doesn't look like a quote key: use only the letters and numbers the provider gave you."
                .into(),
        );
    }
    Ok(k.to_string())
}

/// Save the key for `provider` in the Keychain and choose that provider. One
/// key at a time: the other provider's key is deleted.
pub fn save(
    kv: &dyn SettingsKv,
    store: &dyn SecretStore,
    provider: QuoteProvider,
    key: &str,
) -> Result<QuoteKeyView, String> {
    let key = match validate_key(key) {
        Ok(k) => k,
        Err(e) => {
            log::info!(
                "news quote_key_rejected provider={} reason=invalid",
                provider.id()
            );
            return Err(e);
        }
    };
    store.set(&account_for(provider), &key)?;
    for other in QuoteProvider::ALL.into_iter().filter(|o| *o != provider) {
        store.delete(&account_for(other))?;
    }
    kv.kv_set(PROVIDER_KEY, provider.id()).map_err(|_| {
        "Yap saved the key but could not remember the provider. Try again.".to_string()
    })?;
    log::info!("news quote_key_saved provider={}", provider.id());
    Ok(view(kv, store))
}

/// Remove every quote key and go back to the free tier.
pub fn clear(kv: &dyn SettingsKv, store: &dyn SecretStore) -> Result<QuoteKeyView, String> {
    for p in QuoteProvider::ALL {
        store.delete(&account_for(p))?;
    }
    kv.kv_set(PROVIDER_KEY, "")
        .map_err(|_| "Yap removed the key but could not update settings. Try again.".to_string())?;
    log::info!("news quote_key_cleared");
    Ok(view(kv, store))
}

/// The feed the next refresh uses: the user's own key if one is saved, else
/// the free tier. A Keychain failure falls back to free (and is logged).
pub fn feed(kv: &dyn SettingsKv, store: &dyn SecretStore) -> QuoteFeed {
    match saved_key(kv, store) {
        (Some(p), Ok(Some(k))) => p.feed(k),
        (Some(p), Err(_)) => {
            log::warn!("news quote_key_read_failed provider={}", p.id());
            QuoteFeed::Free
        }
        _ => QuoteFeed::Free,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keychain::MemoryKeychain;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemKv(RefCell<HashMap<String, String>>);
    impl SettingsKv for MemKv {
        fn kv_get(&self, key: &str) -> Option<String> {
            self.0.borrow().get(key).cloned()
        }
        fn kv_set(&self, key: &str, value: &str) -> Result<(), String> {
            self.0.borrow_mut().insert(key.into(), value.into());
            Ok(())
        }
    }

    const KEY: &str = "QKSENTINEL0123456789WXYZ";

    fn store() -> MemoryKeychain {
        MemoryKeychain::new(keychain_service_for("com.wilsonguenther.wilson-voice"))
    }

    #[test]
    fn free_tier_is_the_default_and_needs_no_key() {
        let (kv, ks) = (MemKv::default(), store());
        let v = view(&kv, &ks);
        assert_eq!(v.provider, None);
        assert!(!v.saved);
        assert_eq!(v.tier, "free");
        assert_eq!(v.leaves_label, "Leaves this Mac: only to that provider");
        assert_eq!(v.providers.len(), 2);
        assert_eq!(feed(&kv, &ks), QuoteFeed::Free);
    }

    #[test]
    fn the_key_lives_only_in_the_keychain() {
        let (kv, ks) = (MemKv::default(), store());
        let v = save(&kv, &ks, QuoteProvider::Finnhub, &format!("  {KEY} ")).unwrap();
        assert!(v.saved);
        assert_eq!(v.hint, "WXYZ");
        assert_eq!(v.tier, "finnhub");
        assert_eq!(ks.service(), "com.wilsonguenther.wilson-voice.news");
        assert_eq!(ks.accounts(), vec!["news-quotes@finnhub.io".to_string()]);
        assert_eq!(
            ks.get("news-quotes@finnhub.io").unwrap().as_deref(),
            Some(KEY)
        );
        // The settings store holds the provider id only; the webview view
        // never carries the key.
        assert!(kv.0.borrow().values().all(|val| !val.contains(KEY)));
        assert_eq!(kv.kv_get(PROVIDER_KEY).as_deref(), Some("finnhub"));
        assert!(!serde_json::to_string(&v).unwrap().contains(KEY));
        assert_eq!(feed(&kv, &ks), QuoteFeed::Finnhub(KEY.into()));
    }

    #[test]
    fn switching_provider_keeps_one_key_bound_to_its_host() {
        let (kv, ks) = (MemKv::default(), store());
        save(&kv, &ks, QuoteProvider::Finnhub, KEY).unwrap();
        let v = save(&kv, &ks, QuoteProvider::AlphaVantage, "AVKEY12345").unwrap();
        assert_eq!(v.provider, Some(QuoteProvider::AlphaVantage));
        assert_eq!(v.tier, "alpha_vantage");
        assert_eq!(
            ks.accounts(),
            vec!["news-quotes@www.alphavantage.co".to_string()]
        );
        assert_eq!(feed(&kv, &ks), QuoteFeed::AlphaVantage("AVKEY12345".into()));
    }

    #[test]
    fn clearing_returns_to_the_free_tier() {
        let (kv, ks) = (MemKv::default(), store());
        save(&kv, &ks, QuoteProvider::AlphaVantage, KEY).unwrap();
        let v = clear(&kv, &ks).unwrap();
        assert!(!v.saved);
        assert_eq!(v.provider, None);
        assert_eq!(v.tier, "free");
        assert!(ks.accounts().is_empty());
        assert_eq!(feed(&kv, &ks), QuoteFeed::Free);
    }

    #[test]
    fn a_chosen_provider_without_a_key_is_still_free() {
        let (kv, ks) = (MemKv::default(), store());
        kv.kv_set(PROVIDER_KEY, "finnhub").unwrap();
        let v = view(&kv, &ks);
        assert_eq!(v.provider, Some(QuoteProvider::Finnhub));
        assert_eq!(v.tier, "free");
        assert_eq!(feed(&kv, &ks), QuoteFeed::Free);
    }

    #[test]
    fn bad_keys_are_refused_without_being_echoed() {
        let (kv, ks) = (MemKv::default(), store());
        for bad in ["", "short", "has space inside 1234", "emoji🙂0123456789"] {
            let e = save(&kv, &ks, QuoteProvider::Finnhub, bad).unwrap_err();
            assert!(bad.trim().is_empty() || !e.contains(bad.trim()), "{e}");
        }
        assert!(ks.accounts().is_empty());
        assert_eq!(kv.kv_get(PROVIDER_KEY), None);
    }

    #[test]
    fn provider_ids_round_trip() {
        for p in QuoteProvider::ALL {
            assert_eq!(QuoteProvider::from_id(p.id()), Some(p));
            assert_eq!(
                serde_json::to_string(&p).unwrap(),
                format!("\"{}\"", p.id())
            );
            assert!(p.feed("k".into()).host() == Some(p.host()));
        }
        assert_eq!(QuoteProvider::from_id("stooq"), None);
        assert_eq!(QuoteProvider::from_id(""), None);
    }
}
