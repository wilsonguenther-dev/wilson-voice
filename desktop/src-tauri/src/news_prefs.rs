//! yap-1106-news-prefs-onboarding — what the user wants Yappy News to cover.
//!
//! Three surfaces edit the SAME record: the onboarding step (only when the
//! cloud option is connected), the one-time Home card (the day the cloud is
//! connected, if onboarding never offered it), and Settings → Yappy News.
//!
//! "The cloud option" is a model provider that sends text off this Mac
//! (`model_providers::leaves_this_mac` is Yes for notes or polish), or the
//! "Cloud model (your key)" route: the cloud mode wants the cloud and a
//! cloud-key provider is saved (that route never becomes a task pick; it is
//! `CloudMode` + `ProviderKind::CloudKey`, see `cloud_route`). With it off
//! the feature does not exist: nothing here fetches anything, and the later
//! feed items gate every request on [`NewsView::active`].
//!
//! Everything lives in `settings_kv` on this Mac. The topic list never goes to
//! the cloud; later items send topics only as search words to public feeds.

use serde::{Deserialize, Serialize};

use crate::model_providers::{self, SettingsKv, Task};

/// The `settings_kv` rows this module owns.
pub const PREFS_KEY: &str = "news_prefs";
pub const OFFER_KEY: &str = "news_offer";
pub const CONNECTED_AT_KEY: &str = "news_cloud_connected_at_ms";
pub const LAST_REFRESH_KEY: &str = "news_last_refresh_ms";
/// yap-1106-news-refresh-setting — Settings → Yappy News → "How often". Its
/// own row, so a save from onboarding or the Home card (which only carry
/// topics) can never reset the user's choice.
pub const CADENCE_KEY: &str = "news_refresh_cadence";

/// The topic chips. Ids are stable; the UI owns the labels.
/// yap-1107-parity-news — the ten chips the approved News page draws, in its
/// order (design-2026-11-06/news.html §01).
pub const TOPIC_CHIPS: [&str; 10] = [
    "developer",
    "stocks",
    "business",
    "ai",
    "design",
    "startups",
    "crypto",
    "sports",
    "science",
    "local",
];
/// Chips an earlier build offered. A saved pick keeps working (it is still
/// fetched and shown); the picker just no longer offers them.
pub const LEGACY_TOPIC_CHIPS: [&str; 3] = ["tech", "world", "culture"];

/// True for any chip id a save may carry, current or legacy.
pub fn is_topic_chip(id: &str) -> bool {
    TOPIC_CHIPS.contains(&id) || LEGACY_TOPIC_CHIPS.contains(&id)
}
pub const MAX_TICKERS: usize = 12;
pub const MAX_CUSTOM: usize = 10;
pub const MAX_TICKER_LEN: usize = 10;
pub const MIN_CUSTOM_LEN: usize = 2;
pub const MAX_CUSTOM_LEN: usize = 60;

/// What the user picked. `enabled` is the user's own switch; whether news can
/// actually run is [`NewsView::active`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewsPrefs {
    pub enabled: bool,
    pub topics: Vec<String>,
    pub tickers: Vec<String>,
    pub custom: Vec<String>,
}

impl Default for NewsPrefs {
    fn default() -> Self {
        Self {
            enabled: true,
            topics: Vec::new(),
            tickers: Vec::new(),
            custom: Vec::new(),
        }
    }
}

impl NewsPrefs {
    pub fn has_any_topic(&self) -> bool {
        !(self.topics.is_empty() && self.tickers.is_empty() && self.custom.is_empty())
    }
}

/// yap-1106-news-refresh-setting — how often Yappy News refreshes. Whatever is
/// picked, [`crate::news::schedule`] never fetches more often than every 30
/// minutes and never during a meeting, screen share, Do Not Disturb or low
/// battery. Market quote checks follow the same choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cadence {
    /// The approved default: the first chance after 6 AM and after 4 PM.
    #[default]
    TwiceADay,
    /// Every three hours.
    EveryFewHours,
    /// Only when Home is opened.
    OnHomeOpen,
    /// Only when the user taps refresh.
    OnTap,
}

impl Cadence {
    pub const ALL: [Cadence; 4] = [
        Cadence::TwiceADay,
        Cadence::EveryFewHours,
        Cadence::OnHomeOpen,
        Cadence::OnTap,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Cadence::TwiceADay => "twice_a_day",
            Cadence::EveryFewHours => "every_few_hours",
            Cadence::OnHomeOpen => "on_home_open",
            Cadence::OnTap => "on_tap",
        }
    }

    /// Unknown text reads as `None`; [`load_cadence`] turns that into the default.
    pub fn parse(s: &str) -> Option<Self> {
        Cadence::ALL.into_iter().find(|c| c.as_str() == s.trim())
    }
}

/// Where the one-time offer (onboarding step or Home card) stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Offer {
    /// Never offered or never answered: the step / card is due.
    Unseen,
    /// "Not now". Settings → Yappy News is the way back in.
    Declined,
    /// Prefs were saved at least once.
    Accepted,
}

impl Offer {
    pub fn as_str(self) -> &'static str {
        match self {
            Offer::Unseen => "unseen",
            Offer::Declined => "declined",
            Offer::Accepted => "accepted",
        }
    }
    fn parse(raw: &str) -> Offer {
        match raw.trim() {
            "declined" => Offer::Declined,
            "accepted" => Offer::Accepted,
            _ => Offer::Unseen,
        }
    }
}

/// Everything the three surfaces render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsView {
    pub enabled: bool,
    /// A provider that leaves this Mac is picked for notes or polish.
    pub cloud_connected: bool,
    /// enabled AND cloud connected AND at least one topic: the ONLY state in
    /// which a later item may make a news request.
    pub active: bool,
    pub topics: Vec<String>,
    pub tickers: Vec<String>,
    pub custom: Vec<String>,
    pub offer: Offer,
    /// Show the onboarding step (cloud on, never answered).
    pub onboarding_step_due: bool,
    /// Show the one-time Home card (same condition; answering it clears it).
    pub home_card_due: bool,
    pub cloud_connected_at_ms: Option<i64>,
    pub last_refresh_ms: Option<i64>,
    /// yap-1106-news-refresh-setting — Settings → Yappy News → "How often".
    pub refresh_cadence: Cadence,
}

/// A refused save, as one plain sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewsPrefsError {
    BadTicker(String),
    TooManyTickers,
    BadTopic(String),
    TooManyTopics,
    Storage(String),
}

impl NewsPrefsError {
    pub fn plain(&self) -> String {
        match self {
            NewsPrefsError::BadTicker(t) => format!(
                "\"{t}\" doesn't look like a ticker. Use letters like NVDA or BRK.B."
            ),
            NewsPrefsError::TooManyTickers => {
                format!("Yappy can follow up to {MAX_TICKERS} tickers. Remove one to add another.")
            }
            NewsPrefsError::BadTopic(_) => format!(
                "Topics need {MIN_CUSTOM_LEN} to {MAX_CUSTOM_LEN} characters, like \"Austin real estate\"."
            ),
            NewsPrefsError::TooManyTopics => {
                format!("Yappy can follow up to {MAX_CUSTOM} of your own topics. Remove one to add another.")
            }
            NewsPrefsError::Storage(_) => {
                "Yap couldn't save your news topics. Try again in a moment.".into()
            }
        }
    }
    /// The log tag (never the user's text).
    pub fn tag(&self) -> &'static str {
        match self {
            NewsPrefsError::BadTicker(_) => "bad_ticker",
            NewsPrefsError::TooManyTickers => "too_many_tickers",
            NewsPrefsError::BadTopic(_) => "bad_topic",
            NewsPrefsError::TooManyTopics => "too_many_topics",
            NewsPrefsError::Storage(_) => "storage",
        }
    }
}

/// Is the cloud option connected? True when notes or polish runs on a
/// provider whose requests leave this Mac, or when the "Cloud model (your
/// key)" route is on (see [`cloud_key_route_on`]).
pub fn cloud_connected(kv: &dyn SettingsKv) -> bool {
    cloud_via(kv).is_some()
}

/// Which route makes the cloud option connected, for the log line:
/// `"task"` (a notes/polish pick leaves this Mac), `"cloud_key"` (the byom
/// route), or `None` when the cloud option is off.
fn cloud_via(kv: &dyn SettingsKv) -> Option<&'static str> {
    let s = model_providers::load(kv);
    let by_task = [Task::Notes, Task::Polish]
        .into_iter()
        .any(|t| model_providers::leaves_this_mac(&s.for_task(t)).is_yes());
    if by_task {
        Some("task")
    } else if cloud_key_route_on(kv, &s) {
        Some("cloud_key")
    } else {
        None
    }
}

/// fix-l3b3-3 — the byom "Cloud model (your key)" route counts as the cloud
/// option: the cloud mode wants the cloud (not `always_local`) and a
/// `ProviderKind::CloudKey` provider row is saved whose requests leave this
/// Mac. This module only has a `SettingsKv`, so it checks the provider row,
/// not the keychain; `save_cloud_key` deletes that row when the key is
/// cleared, so the row standing means a key was saved.
/// `model_providers::cloud_route_leaves` is the same rule with the key read.
fn cloud_key_route_on(kv: &dyn SettingsKv, s: &model_providers::ProviderSettings) -> bool {
    if !crate::cloud_route::CloudMode::load(kv).wants_cloud() {
        return false;
    }
    s.providers
        .iter()
        .filter(|p| p.kind == model_providers::ProviderKind::CloudKey)
        .any(|p| model_providers::leaves_this_mac(p).is_yes())
}

/// `$nvda ` → `NVDA`; `None` when it cannot be a ticker.
pub fn normalize_ticker(raw: &str) -> Option<String> {
    let t = raw.trim().trim_start_matches('$').to_ascii_uppercase();
    let first_ok = t.chars().next().is_some_and(|c| c.is_ascii_uppercase());
    let all_ok = t
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '.' || c == '-');
    (first_ok && all_ok && t.len() <= MAX_TICKER_LEN).then_some(t)
}

/// Trim and collapse whitespace; `None` when out of bounds or not printable.
pub fn normalize_custom(raw: &str) -> Option<String> {
    let t = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = t.chars().count();
    let printable = !t.chars().any(|c| c.is_control());
    (printable && (MIN_CUSTOM_LEN..=MAX_CUSTOM_LEN).contains(&n)).then_some(t)
}

/// Validate and tidy a save: unknown chips are dropped, tickers upper-cased,
/// custom topics trimmed, every list de-duplicated (case-insensitive) in order.
pub fn normalize(p: &NewsPrefs) -> Result<NewsPrefs, NewsPrefsError> {
    let mut topics: Vec<String> = Vec::new();
    for t in &p.topics {
        let id = t.trim().to_ascii_lowercase();
        if is_topic_chip(&id) && !topics.contains(&id) {
            topics.push(id);
        }
    }
    let mut tickers: Vec<String> = Vec::new();
    for raw in &p.tickers {
        let t =
            normalize_ticker(raw).ok_or_else(|| NewsPrefsError::BadTicker(raw.trim().into()))?;
        if !tickers.contains(&t) {
            tickers.push(t);
        }
    }
    if tickers.len() > MAX_TICKERS {
        return Err(NewsPrefsError::TooManyTickers);
    }
    let mut custom: Vec<String> = Vec::new();
    for raw in &p.custom {
        let t = normalize_custom(raw).ok_or_else(|| NewsPrefsError::BadTopic(raw.trim().into()))?;
        if !custom.iter().any(|c| c.eq_ignore_ascii_case(&t)) {
            custom.push(t);
        }
    }
    if custom.len() > MAX_CUSTOM {
        return Err(NewsPrefsError::TooManyTopics);
    }
    Ok(NewsPrefs {
        enabled: p.enabled,
        topics,
        tickers,
        custom,
    })
}

/// Read the prefs. A malformed row is logged and read as the default.
pub fn load(kv: &dyn SettingsKv) -> NewsPrefs {
    match kv.kv_get(PREFS_KEY) {
        Some(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            log::warn!("news_prefs load malformed key={PREFS_KEY} error={e}");
            NewsPrefs::default()
        }),
        None => NewsPrefs::default(),
    }
}

pub fn offer(kv: &dyn SettingsKv) -> Offer {
    kv.kv_get(OFFER_KEY)
        .map(|r| Offer::parse(&r))
        .unwrap_or(Offer::Unseen)
}

fn read_ms(kv: &dyn SettingsKv, key: &str) -> Option<i64> {
    kv.kv_get(key).and_then(|r| r.trim().parse::<i64>().ok())
}

/// Build the view. The first time the cloud is seen connected, the moment is
/// recorded (that is "the day the cloud is connected" the Home card keys on).
pub fn view(kv: &dyn SettingsKv, now_ms: i64) -> NewsView {
    let prefs = load(kv);
    let via = cloud_via(kv);
    let cloud = via.is_some();
    let mut connected_at = read_ms(kv, CONNECTED_AT_KEY);
    if cloud && connected_at.is_none() {
        match kv.kv_set(CONNECTED_AT_KEY, &now_ms.to_string()) {
            Ok(()) => {
                log::info!(
                    "news_prefs cloud_seen first=true via={}",
                    via.unwrap_or("none")
                );
                connected_at = Some(now_ms);
            }
            Err(e) => log::warn!("news_prefs cloud_seen failed error=\"{e}\""),
        }
    }
    let offer = offer(kv);
    let due = cloud && offer == Offer::Unseen;
    NewsView {
        enabled: prefs.enabled,
        cloud_connected: cloud,
        active: prefs.enabled && cloud && prefs.has_any_topic(),
        topics: prefs.topics,
        tickers: prefs.tickers,
        custom: prefs.custom,
        offer,
        onboarding_step_due: due,
        home_card_due: due,
        cloud_connected_at_ms: connected_at,
        last_refresh_ms: read_ms(kv, LAST_REFRESH_KEY),
        refresh_cadence: load_cadence(kv),
    }
}

/// The surface a save or dismiss came from, for the log line only.
pub fn source_tag(source: &str) -> &'static str {
    match source {
        "onboarding" => "onboarding",
        "home" => "home",
        "settings" => "settings",
        _ => "other",
    }
}

/// Save from any of the three surfaces. Marks the offer answered.
pub fn save(
    kv: &dyn SettingsKv,
    prefs: &NewsPrefs,
    source: &str,
    now_ms: i64,
) -> Result<NewsView, NewsPrefsError> {
    let src = source_tag(source);
    let clean = normalize(prefs).inspect_err(|e| {
        log::warn!("news_prefs save refused source={src} reason={}", e.tag());
    })?;
    let json = serde_json::to_string(&clean).map_err(|e| NewsPrefsError::Storage(e.to_string()))?;
    kv.kv_set(PREFS_KEY, &json)
        .and_then(|_| kv.kv_set(OFFER_KEY, Offer::Accepted.as_str()))
        .map_err(|e| {
            log::error!("news_prefs save failed source={src} reason=storage error=\"{e}\"");
            NewsPrefsError::Storage(e)
        })?;
    log::info!(
        "news_prefs saved source={src} enabled={} topics={} tickers={} custom={}",
        clean.enabled,
        clean.topics.len(),
        clean.tickers.len(),
        clean.custom.len()
    );
    Ok(view(kv, now_ms))
}

/// "Not now" on the onboarding step or the Home card. Never downgrades an
/// already-accepted offer.
pub fn dismiss_offer(
    kv: &dyn SettingsKv,
    source: &str,
    now_ms: i64,
) -> Result<NewsView, NewsPrefsError> {
    let src = source_tag(source);
    if offer(kv) != Offer::Accepted {
        kv.kv_set(OFFER_KEY, Offer::Declined.as_str())
            .map_err(|e| {
                log::error!("news_prefs dismiss failed source={src} error=\"{e}\"");
                NewsPrefsError::Storage(e)
            })?;
    }
    log::info!("news_prefs offer_dismissed source={src}");
    Ok(view(kv, now_ms))
}

/// yap-1106-news-refresh-setting — the saved "How often". A missing or
/// unknown row reads as the approved default, twice a day.
pub fn load_cadence(kv: &dyn SettingsKv) -> Cadence {
    match kv.kv_get(CADENCE_KEY) {
        Some(raw) => Cadence::parse(&raw).unwrap_or_else(|| {
            log::warn!("news_prefs cadence_malformed key={CADENCE_KEY}");
            Cadence::default()
        }),
        None => Cadence::default(),
    }
}

/// Save "How often" from Settings → Yappy News. Never touches the topics or
/// the offer.
pub fn save_cadence(
    kv: &dyn SettingsKv,
    cadence: Cadence,
    now_ms: i64,
) -> Result<NewsView, NewsPrefsError> {
    let before = load_cadence(kv);
    kv.kv_set(CADENCE_KEY, cadence.as_str()).map_err(|e| {
        log::error!(
            "news_prefs cadence_save_failed cadence={} reason=storage error=\"{e}\"",
            cadence.as_str()
        );
        NewsPrefsError::Storage(e)
    })?;
    log::info!(
        "news_prefs cadence_saved from={} to={}",
        before.as_str(),
        cadence.as_str()
    );
    Ok(view(kv, now_ms))
}

/// For the feed items: stamp a finished refresh (Settings shows it).
pub fn record_refresh(kv: &dyn SettingsKv, now_ms: i64) -> Result<(), String> {
    kv.kv_set(LAST_REFRESH_KEY, &now_ms.to_string())
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_providers::{ProviderConfig, ProviderKind, ProviderSettings};
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

    struct BrokenKv;
    impl SettingsKv for BrokenKv {
        fn kv_get(&self, _key: &str) -> Option<String> {
            None
        }
        fn kv_set(&self, _key: &str, _value: &str) -> Result<(), String> {
            Err("disk full".into())
        }
    }

    fn connect_cloud(kv: &MemKv) {
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
        model_providers::save(kv, &s).expect("save providers");
    }

    /// fix-l3b3-3 — the byom route as Settings leaves it after a key is
    /// pasted: a CloudKey provider row, no notes/polish pick, and `mode`.
    fn connect_cloud_key(kv: &MemKv, mode: crate::cloud_route::CloudMode) {
        let s = ProviderSettings {
            providers: vec![ProviderConfig::bundled(), ProviderConfig::cloud_key()],
            notes_provider: String::new(),
            polish_provider: String::new(),
        };
        model_providers::save(kv, &s).expect("save providers");
        crate::cloud_route::CloudMode::save(kv, mode).expect("save mode");
    }

    fn prefs(topics: &[&str], tickers: &[&str], custom: &[&str]) -> NewsPrefs {
        NewsPrefs {
            enabled: true,
            topics: topics.iter().map(|s| s.to_string()).collect(),
            tickers: tickers.iter().map(|s| s.to_string()).collect(),
            custom: custom.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn news_prefs_cloud_off_means_no_step_no_card_not_active() {
        let kv = MemKv::default();
        save(&kv, &prefs(&["developer"], &[], &[]), "settings", 1).unwrap();
        let v = view(&kv, 2);
        assert!(!v.cloud_connected);
        assert!(!v.onboarding_step_due);
        assert!(!v.home_card_due);
        assert!(!v.active, "news never runs without the cloud option");
        assert_eq!(v.cloud_connected_at_ms, None);
    }

    #[test]
    fn news_prefs_cloud_on_offers_step_and_card_once() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        let v = view(&kv, 1_000);
        assert!(v.cloud_connected);
        assert!(v.onboarding_step_due && v.home_card_due);
        assert_eq!(v.cloud_connected_at_ms, Some(1_000));
        // The first sighting is kept, not overwritten.
        assert_eq!(view(&kv, 5_000).cloud_connected_at_ms, Some(1_000));
        let v = dismiss_offer(&kv, "home", 6_000).unwrap();
        assert_eq!(v.offer, Offer::Declined);
        assert!(!v.home_card_due && !v.onboarding_step_due);
    }

    #[test]
    fn news_prefs_cloud_key_route_offers_step_and_card_once() {
        let kv = MemKv::default();
        connect_cloud_key(&kv, crate::cloud_route::CloudMode::CloudWhenOnline);
        assert!(
            cloud_connected(&kv),
            "the cloud-key route is the cloud option"
        );
        let v = view(&kv, 1_000);
        assert!(v.cloud_connected);
        assert!(v.onboarding_step_due && v.home_card_due);
        assert_eq!(v.cloud_connected_at_ms, Some(1_000));
        assert_eq!(view(&kv, 5_000).cloud_connected_at_ms, Some(1_000));
        // Once: a dismiss from either surface clears both.
        let v = dismiss_offer(&kv, "home", 6_000).unwrap();
        assert_eq!(v.offer, Offer::Declined);
        assert!(!v.home_card_due && !v.onboarding_step_due);
        assert!(view(&kv, 7_000).cloud_connected);
        assert!(!view(&kv, 7_000).home_card_due);
    }

    #[test]
    fn news_prefs_cloud_key_route_cloud_only_counts() {
        let kv = MemKv::default();
        connect_cloud_key(&kv, crate::cloud_route::CloudMode::CloudOnly);
        let v = view(&kv, 1_000);
        assert!(v.cloud_connected);
        assert!(v.onboarding_step_due && v.home_card_due);
    }

    #[test]
    fn news_prefs_cloud_key_always_local_is_not_offered() {
        let kv = MemKv::default();
        connect_cloud_key(&kv, crate::cloud_route::CloudMode::AlwaysLocal);
        save(&kv, &prefs(&["developer"], &[], &[]), "settings", 1).unwrap();
        let v = view(&kv, 2);
        assert!(!v.cloud_connected);
        assert!(!v.onboarding_step_due && !v.home_card_due);
        assert!(!v.active, "always local means news never runs");
        assert_eq!(v.cloud_connected_at_ms, None);
    }

    #[test]
    fn news_prefs_cloud_mode_without_key_row_is_not_offered() {
        // Mode on but the key was cleared (save_cloud_key deletes the row).
        let kv = MemKv::default();
        crate::cloud_route::CloudMode::save(&kv, crate::cloud_route::CloudMode::CloudWhenOnline)
            .unwrap();
        let v = view(&kv, 2);
        assert!(!v.cloud_connected);
        assert!(!v.onboarding_step_due && !v.home_card_due);
    }

    /// yap-1107-news-honours-cloud-key — the whole route as `save_cloud_key`
    /// leaves it: the key goes to a (fake) Keychain, the CloudKey provider row
    /// is upserted, no notes/polish pick names it. News turns on once a topic
    /// is set and the connect day is stamped exactly once; `always_local` or
    /// the row deleted (what an empty key does) turns it back off.
    #[test]
    fn news_prefs_saved_cloud_key_turns_news_on() {
        use crate::cloud_route::CloudMode;
        use crate::keychain::{MemoryKeychain, PRODUCTION_SERVICE};
        for mode in [CloudMode::CloudWhenOnline, CloudMode::CloudOnly] {
            let kv = MemKv::default();
            let keys = MemoryKeychain::new(PRODUCTION_SERVICE);
            let cfg = model_providers::upsert_provider(
                &kv,
                &keys,
                ProviderConfig::cloud_key(),
                Some("sk-UNIT-0123456789abcdefWXYZ"),
            )
            .expect("save cloud key");
            assert_eq!(model_providers::load(&kv).notes_provider, "");
            assert_eq!(model_providers::load(&kv).polish_provider, "");
            CloudMode::save(&kv, mode).expect("save mode");

            let v = view(&kv, 1_000);
            assert!(
                v.cloud_connected,
                "{mode:?}: a saved key is the cloud option"
            );
            assert!(!v.active, "no topic yet");
            assert_eq!(v.cloud_connected_at_ms, Some(1_000));

            let v = save(&kv, &prefs(&["developer"], &[], &[]), "settings", 2_000).unwrap();
            assert!(
                v.cloud_connected && v.active,
                "{mode:?}: on once a topic is set"
            );
            // Stamped once: later reads keep the first connect time.
            assert_eq!(view(&kv, 9_000).cloud_connected_at_ms, Some(1_000));
            assert!(!serde_json::to_string(&v).unwrap().contains("sk-UNIT"));

            // Always on this Mac: off, nothing runs.
            CloudMode::save(&kv, CloudMode::AlwaysLocal).unwrap();
            let v = view(&kv, 10_000);
            assert!(!v.cloud_connected && !v.active);

            // Back to the cloud, then the key removed: off again.
            CloudMode::save(&kv, mode).unwrap();
            assert!(view(&kv, 11_000).active);
            model_providers::delete_provider(&kv, &keys, &cfg.id).expect("remove key");
            let v = view(&kv, 12_000);
            assert!(
                !v.cloud_connected && !v.active,
                "{mode:?}: key removed is off"
            );
        }
    }

    #[test]
    fn news_prefs_not_now_skips_without_saving_topics() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        let v = dismiss_offer(&kv, "onboarding", 1).unwrap();
        assert!(v.topics.is_empty() && v.tickers.is_empty() && v.custom.is_empty());
        assert!(!v.active);
        assert_eq!(kv.kv_get(PREFS_KEY), None);
    }

    #[test]
    fn news_prefs_save_is_shared_by_every_surface_and_accepts_offer() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        let v = save(
            &kv,
            &prefs(
                &["stocks", "Developer"],
                &["$nvda", "aapl"],
                &["Austin  real estate"],
            ),
            "onboarding",
            1,
        )
        .unwrap();
        assert_eq!(v.offer, Offer::Accepted);
        assert!(v.active && !v.home_card_due);
        // Settings reads exactly what onboarding wrote.
        let s = view(&kv, 2);
        assert_eq!(s.topics, vec!["stocks", "developer"]);
        assert_eq!(s.tickers, vec!["NVDA", "AAPL"]);
        assert_eq!(s.custom, vec!["Austin real estate"]);
        // A later "Not now" never undoes an accepted offer.
        assert_eq!(
            dismiss_offer(&kv, "home", 3).unwrap().offer,
            Offer::Accepted
        );
        // Switching off keeps the topics and stops news.
        let mut off = load(&kv);
        off.enabled = false;
        let v = save(&kv, &off, "settings", 4).unwrap();
        assert!(!v.enabled && !v.active);
        assert_eq!(v.tickers, vec!["NVDA", "AAPL"]);
    }

    #[test]
    fn news_prefs_normalize_drops_unknown_chips_and_dedupes() {
        let p = normalize(&prefs(
            &["sports", "made-up", "sports"],
            &["NVDA", "nvda"],
            &["Tauri releases", "tauri RELEASES"],
        ))
        .unwrap();
        assert_eq!(p.topics, vec!["sports"]);
        assert_eq!(p.tickers, vec!["NVDA"]);
        assert_eq!(p.custom, vec!["Tauri releases"]);
    }

    #[test]
    fn news_prefs_rejects_bad_ticker_and_bad_topic() {
        assert_eq!(normalize_ticker("brk.b").as_deref(), Some("BRK.B"));
        assert_eq!(normalize_ticker("1ABC"), None);
        assert_eq!(normalize_ticker("NV DA"), None);
        assert_eq!(normalize_ticker("ABCDEFGHIJK"), None);
        assert!(matches!(
            normalize(&prefs(&[], &["??"], &[])),
            Err(NewsPrefsError::BadTicker(_))
        ));
        assert!(matches!(
            normalize(&prefs(&[], &[], &["x"])),
            Err(NewsPrefsError::BadTopic(_))
        ));
        let many: Vec<String> = (0..=MAX_TICKERS).map(|i| format!("T{i}")).collect();
        let p = NewsPrefs {
            tickers: many,
            ..NewsPrefs::default()
        };
        assert_eq!(normalize(&p), Err(NewsPrefsError::TooManyTickers));
        let many: Vec<String> = (0..=MAX_CUSTOM).map(|i| format!("topic {i}")).collect();
        let p = NewsPrefs {
            custom: many,
            ..NewsPrefs::default()
        };
        assert_eq!(normalize(&p), Err(NewsPrefsError::TooManyTopics));
    }

    #[test]
    fn news_prefs_last_refresh_shows_in_view() {
        let kv = MemKv::default();
        assert_eq!(view(&kv, 1).last_refresh_ms, None);
        record_refresh(&kv, 42).unwrap();
        assert_eq!(view(&kv, 2).last_refresh_ms, Some(42));
    }

    #[test]
    fn news_refresh_cadence_defaults_to_twice_a_day() {
        let kv = MemKv::default();
        assert_eq!(load_cadence(&kv), Cadence::TwiceADay);
        assert_eq!(view(&kv, 1).refresh_cadence, Cadence::TwiceADay);
        kv.kv_set(CADENCE_KEY, "hourly-ish").unwrap();
        assert_eq!(load_cadence(&kv), Cadence::TwiceADay);
    }

    #[test]
    fn news_refresh_cadence_saves_every_option_without_touching_topics() {
        let kv = MemKv::default();
        save(&kv, &prefs(&["tech"], &["NVDA"], &[]), "settings", 1).unwrap();
        for c in Cadence::ALL {
            let v = save_cadence(&kv, c, 2).unwrap();
            assert_eq!(v.refresh_cadence, c);
            assert_eq!(Cadence::parse(c.as_str()), Some(c));
            assert_eq!(v.topics, vec!["tech".to_string()]);
            assert_eq!(v.tickers, vec!["NVDA".to_string()]);
        }
        // A later topics save (onboarding/Home carry no cadence) keeps it.
        save(&kv, &prefs(&["science"], &[], &[]), "onboarding", 3).unwrap();
        assert_eq!(load_cadence(&kv), Cadence::OnTap);
        let json = serde_json::to_string(&Cadence::EveryFewHours).unwrap();
        assert_eq!(json, "\"every_few_hours\"");
    }

    #[test]
    fn news_refresh_cadence_storage_failure_is_one_sentence() {
        let e = save_cadence(&BrokenKv, Cadence::OnHomeOpen, 1).unwrap_err();
        assert_eq!(e.tag(), "storage");
        assert!(e.plain().ends_with('.'));
    }

    #[test]
    fn news_prefs_malformed_row_reads_default() {
        let kv = MemKv::default();
        kv.kv_set(PREFS_KEY, "{not json").unwrap();
        assert_eq!(load(&kv), NewsPrefs::default());
    }

    #[test]
    fn news_prefs_storage_failure_is_one_sentence() {
        let e = save(&BrokenKv, &prefs(&["tech"], &[], &[]), "settings", 1).unwrap_err();
        assert_eq!(e.tag(), "storage");
        assert!(e.plain().ends_with('.'));
        assert!(!e.plain().contains("disk full"));
    }

    #[test]
    fn parity_news_chips_match_the_approved_page_and_keep_legacy_picks() {
        assert_eq!(
            TOPIC_CHIPS,
            [
                "developer",
                "stocks",
                "business",
                "ai",
                "design",
                "startups",
                "crypto",
                "sports",
                "science",
                "local"
            ]
        );
        let p = prefs(&["AI", "tech", "nonsense", "local"], &[], &[]);
        let n = normalize(&p).unwrap();
        assert_eq!(
            n.topics,
            vec!["ai".to_string(), "tech".to_string(), "local".to_string()]
        );
        for id in TOPIC_CHIPS.iter().chain(LEGACY_TOPIC_CHIPS.iter()) {
            if *id != "developer" {
                assert!(
                    crate::news::feeds::google_query(id).is_some(),
                    "chip {id} has no search words"
                );
            }
        }
    }
}
