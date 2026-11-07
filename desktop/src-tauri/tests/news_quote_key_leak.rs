//! yap-1106-fb-14-news-quote-key — the user's own quote key is never logged
//! and never sent anywhere but its provider's host.
//!
//! A sentinel key is saved through the in-memory Keychain for each provider,
//! then a full Yappy News refresh runs (Google News + Hacker News + quotes)
//! against a recording transport. Non-vacuous: exactly one request carries
//! the key, and that request's host is the provider's. Every log record, the
//! settings rows, the Advanced view and the refresh report are swept for it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::Url;
use wilson_voice_lib::keychain::{MemoryKeychain, SecretStore};
use wilson_voice_lib::model_providers::{
    self, ProviderConfig, ProviderKind, ProviderSettings, SettingsKv,
};
use wilson_voice_lib::news::feeds::{self, FeedError, FeedHttp, RateLimiter, Source};
use wilson_voice_lib::news::quote_key::{self, QuoteProvider};
use wilson_voice_lib::news_prefs::{self, NewsPrefs};

const SENTINEL: &str = "YAPQUOTEKEYSENTINEL0123WXYZ";

static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;
impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, r: &log::Record<'_>) {
        LOGS.lock()
            .unwrap()
            .push(format!("{} {} {}", r.level(), r.target(), r.args()));
    }
    fn flush(&self) {}
}

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

/// One recorded request: which source, the URL, and the headers sent.
type Recorded = (Source, String, Vec<(String, String)>);

/// Records every request; a refused key (401) for quotes, empty feeds else.
#[derive(Default)]
struct Recorder(RefCell<Vec<Recorded>>);
impl FeedHttp for Recorder {
    fn get(
        &self,
        source: Source,
        url: &str,
        headers: &[(&str, &str)],
        _timeout: Duration,
    ) -> Result<String, FeedError> {
        self.0.borrow_mut().push((
            source,
            url.to_string(),
            headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ));
        match source {
            Source::Quotes => Err(FeedError::Status { code: 401 }),
            Source::GoogleNews => Ok("<rss><channel></channel></rss>".into()),
            Source::HackerNews => Ok(r#"{"hits":[]}"#.into()),
        }
    }
}

fn setup(kv: &MemKv) {
    let remote = ProviderConfig {
        id: "remote".into(),
        kind: ProviderKind::OpenAiCompatible,
        label: "Remote".into(),
        base_url: "https://api.example.com/v1".into(),
        model: "m".into(),
        context_tokens: 8192,
        timeout_s: 30,
    };
    let s = ProviderSettings {
        providers: vec![ProviderConfig::bundled(), remote],
        notes_provider: String::new(),
        polish_provider: "remote".into(),
    };
    model_providers::save(kv, &s).expect("connect cloud");
    let prefs = NewsPrefs {
        enabled: true,
        topics: vec!["tech".into(), "developer".into()],
        tickers: vec!["NVDA".into()],
        custom: vec!["rust".into()],
    };
    kv.kv_set(
        news_prefs::PREFS_KEY,
        &serde_json::to_string(&prefs).unwrap(),
    )
    .unwrap();
}

#[test]
fn quote_key_is_never_logged_or_sent_off_its_provider_host() {
    let _ = log::set_boxed_logger(Box::new(Capture));
    log::set_max_level(log::LevelFilter::Trace);

    for provider in QuoteProvider::ALL {
        let kv = MemKv::default();
        setup(&kv);
        let ks = MemoryKeychain::new(quote_key::keychain_service_for(
            "com.wilsonguenther.wilson-voice",
        ));
        let view = quote_key::save(&kv, &ks, provider, SENTINEL).expect("save key");
        assert_eq!(
            ks.get(&quote_key::account_for(provider))
                .unwrap()
                .as_deref(),
            Some(SENTINEL)
        );

        let feed = quote_key::feed(&kv, &ks);
        let http = Recorder::default();
        let mut lim = RateLimiter::new();
        let report =
            feeds::refresh_with(&kv, &http, &mut lim, 10 * 3_600_000, &feed).expect("gate open");

        let calls = http.0.borrow();
        let carrying: Vec<_> = calls
            .iter()
            .filter(|(_, url, headers)| {
                url.contains(SENTINEL)
                    || headers
                        .iter()
                        .any(|(k, v)| k.contains(SENTINEL) || v.contains(SENTINEL))
            })
            .collect();
        assert_eq!(
            carrying.len(),
            1,
            "{provider:?}: the key really travelled, once"
        );
        for (source, url, _) in &carrying {
            assert_eq!(*source, Source::Quotes);
            let host = Url::parse(url).unwrap().host_str().unwrap().to_string();
            assert_eq!(host, provider.host(), "{provider:?}: key went off-host");
        }
        assert!(calls.iter().any(|c| c.0 == Source::GoogleNews));
        assert!(calls.iter().any(|c| c.0 == Source::HackerNews));

        // Nowhere else: settings rows, the Advanced view, the report, Debug.
        assert!(kv.0.borrow().values().all(|v| !v.contains(SENTINEL)));
        assert!(!serde_json::to_string(&view).unwrap().contains(SENTINEL));
        assert!(!serde_json::to_string(&report).unwrap().contains(SENTINEL));
        assert!(!format!("{feed:?}").contains(SENTINEL));
        quote_key::clear(&kv, &ks).expect("clear");
        assert!(ks.get(&quote_key::account_for(provider)).unwrap().is_none());
    }

    let logs = LOGS.lock().unwrap();
    assert!(
        logs.iter().any(|l| l.contains("news quote_key_saved")),
        "non-vacuous: the logger saw this module's lines"
    );
    assert!(logs.iter().any(|l| l.contains("news quotes tier=")));
    for line in logs.iter() {
        assert!(!line.contains(SENTINEL), "key in a log line: {line}");
        assert!(!line.contains("apikey="), "a keyed URL was logged: {line}");
    }
}
