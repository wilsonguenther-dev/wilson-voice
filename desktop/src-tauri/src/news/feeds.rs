//! yap-1106-news-feeds — fetch Yappy News from three public feeds.
//!
//! The gate comes first: [`gate`] refuses (and [`refresh`] makes ZERO
//! requests) unless the cloud option is connected, the user's switch is on and
//! at least one topic, ticker or custom word is set. Topics leave this Mac only
//! as search words in a feed URL; nothing dictated is ever sent, and no URL
//! (which carries those words) is ever logged.
//!
//! ## Sources, terms and limits (rechecked 2026-11-07 for this item)
//!
//! * **Google News search RSS** — `news.google.com/rss/search?q=…&hl=en-US&gl=US&ceid=US:en`.
//!   No key. The feed's own `<copyright>` element (read live 2026-11-07) says
//!   it is "made available solely for the purpose of rendering Google News
//!   results within a personal feed reader for personal, non-commercial use".
//!   Yap renders it inside the user's own app for that user, one search per
//!   topic, linking every story to its source; whether a paid app counts as
//!   "non-commercial" is a launch question for Wilson (publisher RSS feeds are
//!   the design page's fallback). No published rate limit, so we hold
//!   ourselves to one fetch per query per 30 min, 8 queries per refresh, 24 per
//!   hour.
//! * **Hacker News Search API** — `hn.algolia.com/api/v1/search?tags=front_page`.
//!   No key. The API page states a limit of 10,000 requests per hour per IP;
//!   we use one front-page request per 30 min, at most 6 an hour.
//! * **Stock quotes** — Finnhub `finnhub.io/api/v1/quote?symbol=…`, key sent in
//!   the `X-Finnhub-Token` header (a keyless call answered 401 on 2026-11-07).
//!   The design page's keyless option, Stooq CSV, is gone: `stooq.com/q/l/`
//!   answers 404 and every other Stooq path is behind a JavaScript browser
//!   check (probed 2026-11-07). Finnhub's free tier is documented as 60 calls a
//!   minute; its docs page is script-rendered and was not re-read here, so the
//!   free-tier terms must be confirmed before launch. We call each ticker at
//!   most once per 30 min (the design's "quote check every 30 min"), 12 per
//!   refresh, 60 an hour, and every quote is marked delayed. With no key the
//!   quote source is skipped with `error=no_key` and makes no request.
//!
//! A 429 from any source backs that source off for an hour.
//!
//! Telemetry: one `news fetch source=… ms=… count=… error=…` line per source
//! per refresh (`news.fetch {source, ms, count, error}`), plus
//! `news fetch_skipped reason=…` when the gate is closed.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::model_providers::SettingsKv;
use crate::news_prefs::{self, NewsPrefs};

/// Largest feed body we read. A Google News search page is ~100 KB.
pub const MAX_BODY: usize = 2 * 1024 * 1024;
/// Stories kept per Google News query.
pub const GOOGLE_PER_QUERY: usize = 5;
/// Stories kept from the Hacker News front page.
pub const HN_TOP: usize = 10;
/// How long a source stays quiet after it answers 429.
pub const BACKOFF_MS: i64 = 60 * 60 * 1000;
const HOUR_MS: i64 = 60 * 60 * 1000;
const THIRTY_MIN_MS: i64 = 30 * 60 * 1000;
const USER_AGENT: &str = "Yap/1 (news reader; one request per topic per 30 min)";

/// The three public feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    GoogleNews,
    HackerNews,
    Quotes,
}

impl Source {
    pub const ALL: [Source; 3] = [Source::GoogleNews, Source::HackerNews, Source::Quotes];

    pub fn tag(self) -> &'static str {
        match self {
            Source::GoogleNews => "google_news",
            Source::HackerNews => "hacker_news",
            Source::Quotes => "quotes",
        }
    }

    /// The `net::http_client` purpose (one support-log line per request).
    pub fn purpose(self) -> &'static str {
        match self {
            Source::GoogleNews => "news-google",
            Source::HackerNews => "news-hn",
            Source::Quotes => "news-quotes",
        }
    }

    pub fn limits(self) -> Limits {
        match self {
            Source::GoogleNews => Limits {
                timeout: Duration::from_secs(8),
                per_query_ms: THIRTY_MIN_MS,
                per_hour: 24,
                per_refresh: 8,
            },
            Source::HackerNews => Limits {
                timeout: Duration::from_secs(8),
                per_query_ms: THIRTY_MIN_MS,
                per_hour: 6,
                per_refresh: 1,
            },
            Source::Quotes => Limits {
                timeout: Duration::from_secs(6),
                per_query_ms: THIRTY_MIN_MS,
                per_hour: 60,
                per_refresh: news_prefs::MAX_TICKERS,
            },
        }
    }
}

/// Per-source timeout and self-imposed rate limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Whole-request deadline.
    pub timeout: Duration,
    /// The same query (search words, ticker, front page) at most this often.
    pub per_query_ms: i64,
    /// Requests to this source in any rolling hour.
    pub per_hour: usize,
    /// Requests to this source in one refresh.
    pub per_refresh: usize,
}

/// Why a feed request failed or was not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    Timeout { secs: u64 },
    Unreachable,
    Status { code: u16 },
    RateLimited,
    TooLarge,
    BadBody,
    NoKey,
    Client(String),
}

impl FeedError {
    /// The `error=` value of the telemetry line.
    pub fn tag(&self) -> &'static str {
        match self {
            FeedError::Timeout { .. } => "timeout",
            FeedError::Unreachable => "unreachable",
            FeedError::Status { .. } => "status",
            FeedError::RateLimited => "rate_limited",
            FeedError::TooLarge => "too_large",
            FeedError::BadBody => "bad_body",
            FeedError::NoKey => "no_key",
            FeedError::Client(_) => "client",
        }
    }

    /// One sentence for a person.
    pub fn plain(&self) -> String {
        match self {
            FeedError::Timeout { secs } => format!("the news feed did not answer within {secs} s"),
            FeedError::Unreachable => "the news feed could not be reached".into(),
            FeedError::Status { code } => format!("the news feed answered {code}"),
            FeedError::RateLimited => {
                "the news feed asked us to slow down; trying again in an hour".into()
            }
            FeedError::TooLarge => "the news feed sent more than we read".into(),
            FeedError::BadBody => "the news feed sent something we could not read".into(),
            FeedError::NoKey => "stock quotes need a quote key in Settings".into(),
            FeedError::Client(_) => "the network is not available to Yap".into(),
        }
    }
}

/// Why [`refresh`] made no request at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Closed {
    /// No provider that leaves this Mac: the feature does not exist.
    CloudOff,
    /// The user's own Yappy News switch is off.
    Disabled,
    /// No topic, ticker or custom word.
    NoTopics,
}

impl Closed {
    pub fn tag(self) -> &'static str {
        match self {
            Closed::CloudOff => "cloud_off",
            Closed::Disabled => "disabled",
            Closed::NoTopics => "no_topics",
        }
    }
}

/// The gate every request passes: cloud on, switch on, one topic set.
pub fn gate(kv: &dyn SettingsKv) -> Result<NewsPrefs, Closed> {
    if !news_prefs::cloud_connected(kv) {
        return Err(Closed::CloudOff);
    }
    let prefs = news_prefs::load(kv);
    if !prefs.enabled {
        return Err(Closed::Disabled);
    }
    if !prefs.has_any_topic() {
        return Err(Closed::NoTopics);
    }
    Ok(prefs)
}

/// The one HTTP seam of this module, so a test can count requests.
pub trait FeedHttp {
    fn get(
        &self,
        source: Source,
        url: &str,
        headers: &[(&str, &str)],
        timeout: Duration,
    ) -> Result<String, FeedError>;
}

/// The shipping transport: a client from [`crate::net::http_client`], a
/// per-request deadline, a capped body. Call from a blocking thread.
pub struct NetHttp;

impl FeedHttp for NetHttp {
    fn get(
        &self,
        source: Source,
        url: &str,
        headers: &[(&str, &str)],
        timeout: Duration,
    ) -> Result<String, FeedError> {
        news_feed_get(source, url, headers, timeout)
    }
}

/// The ONE network call site of Yappy News — allowlisted by name in
/// `tests/no_outbound_on_the_dictation_path.rs` (`NETWORK_CALL_SITES`, purpose
/// "Yappy News") and listed in PRIVACY.md. It is a free function with a unique
/// name, not the trait's `get`, so the take-path sweep (which forbids every
/// allowlisted network fn name on the dictation path) does not have to forbid
/// the word `get`. Only [`refresh`] reaches it, and only after [`gate`] opened.
fn news_feed_get(
    source: Source,
    url: &str,
    headers: &[(&str, &str)],
    timeout: Duration,
) -> Result<String, FeedError> {
    let client = crate::net::http_client(source.purpose()).map_err(FeedError::Client)?;
    let secs = timeout.as_secs();
    tauri::async_runtime::block_on(async {
        let mut req = client
            .get(url)
            .timeout(timeout)
            .header(reqwest::header::USER_AGENT, USER_AGENT);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.send().await.map_err(|e| {
            if e.is_timeout() {
                FeedError::Timeout { secs }
            } else {
                FeedError::Unreachable
            }
        })?;
        let code = resp.status().as_u16();
        if code == 429 {
            return Err(FeedError::RateLimited);
        }
        if !resp.status().is_success() {
            return Err(FeedError::Status { code });
        }
        if resp.content_length().is_some_and(|n| n > MAX_BODY as u64) {
            return Err(FeedError::TooLarge);
        }
        let bytes = resp.bytes().await.map_err(|e| {
            if e.is_timeout() {
                FeedError::Timeout { secs }
            } else {
                FeedError::BadBody
            }
        })?;
        if bytes.len() > MAX_BODY {
            return Err(FeedError::TooLarge);
        }
        String::from_utf8(bytes.to_vec()).map_err(|_| FeedError::BadBody)
    })
}

/// Why a request was held back by our own limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Throttle {
    TooSoon,
    HourCap,
    BackedOff,
}

/// Per-source rate limits, in memory for the life of the process.
#[derive(Debug, Default)]
pub struct RateLimiter {
    last: HashMap<(Source, String), i64>,
    sent: HashMap<Source, Vec<i64>>,
    backoff_until: HashMap<Source, i64>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// May `source` be asked for `query` at `now_ms`?
    pub fn check(&self, source: Source, query: &str, now_ms: i64) -> Result<(), Throttle> {
        let lim = source.limits();
        if self
            .backoff_until
            .get(&source)
            .is_some_and(|until| now_ms < *until)
        {
            return Err(Throttle::BackedOff);
        }
        if let Some(last) = self.last.get(&(source, query.to_string())) {
            if now_ms - *last < lim.per_query_ms {
                return Err(Throttle::TooSoon);
            }
        }
        let in_hour = self
            .sent
            .get(&source)
            .map(|v| v.iter().filter(|t| now_ms - **t < HOUR_MS).count())
            .unwrap_or(0);
        if in_hour >= lim.per_hour {
            return Err(Throttle::HourCap);
        }
        Ok(())
    }

    /// Record a request that was sent (whatever it answered).
    pub fn note(&mut self, source: Source, query: &str, now_ms: i64) {
        self.last.insert((source, query.to_string()), now_ms);
        let v = self.sent.entry(source).or_default();
        v.retain(|t| now_ms - *t < HOUR_MS);
        v.push(now_ms);
    }

    /// The source answered 429: leave it alone for [`BACKOFF_MS`].
    pub fn back_off(&mut self, source: Source, now_ms: i64) {
        self.backoff_until.insert(source, now_ms + BACKOFF_MS);
    }
}

/// One story from a feed. Every story links to its source.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Story {
    pub source: Source,
    /// The topic id, ticker or custom word that found it.
    pub topic: String,
    pub title: String,
    pub url: String,
    /// The publisher ("The Guardian"); Hacker News has none.
    pub outlet: Option<String>,
    /// How many outlets carry it (Google News groups them); at least 1.
    pub outlets: usize,
    pub published_ms: Option<i64>,
    /// Hacker News points and comments.
    pub points: Option<i64>,
    pub comments: Option<i64>,
}

/// One stock quote. Always marked delayed (design rule: "Markets show
/// 'delayed' on every quote").
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub symbol: String,
    pub price: f64,
    pub change_pct: f64,
    pub prev_close: f64,
    pub as_of_ms: Option<i64>,
    pub delayed: bool,
}

/// What one source did in one refresh: the `news.fetch` telemetry record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceOutcome {
    pub source: Source,
    pub ms: u64,
    pub count: usize,
    pub requests: usize,
    pub throttled: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchReport {
    pub stories: Vec<Story>,
    pub quotes: Vec<Quote>,
    pub outcomes: Vec<SourceOutcome>,
    /// Requests actually sent across all sources.
    pub requests: usize,
}

/// The Google News search words for a topic chip; `developer` goes to Hacker
/// News instead.
pub fn google_query(topic_id: &str) -> Option<&'static str> {
    match topic_id {
        "stocks" => Some("stock market"),
        "business" => Some("business"),
        "tech" => Some("technology"),
        "science" => Some("science"),
        "world" => Some("world news"),
        "sports" => Some("sports"),
        "culture" => Some("culture"),
        _ => None,
    }
}

/// Google News search feed URL for `words`, limited to the last day.
pub fn google_news_url(words: &str) -> Option<String> {
    let q = format!("{} when:1d", words.trim());
    Url::parse_with_params(
        "https://news.google.com/rss/search",
        &[
            ("q", q.as_str()),
            ("hl", "en-US"),
            ("gl", "US"),
            ("ceid", "US:en"),
        ],
    )
    .ok()
    .map(|u| u.to_string())
}

pub fn hn_front_page_url(n: usize) -> String {
    format!("https://hn.algolia.com/api/v1/search?tags=front_page&hitsPerPage={n}")
}

pub fn quote_url(symbol: &str) -> Option<String> {
    Url::parse_with_params("https://finnhub.io/api/v1/quote", &[("symbol", symbol)])
        .ok()
        .map(|u| u.to_string())
}

/// Decode the XML/HTML entities a feed uses.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let name = &tail[1..end];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => {
                if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                } else if let Some(dec) = name.strip_prefix('#') {
                    dec.parse::<u32>().ok().and_then(char::from_u32)
                } else {
                    None
                }
            }
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text of the first `<tag ...>…</tag>` in `block`, CDATA unwrapped and
/// entities decoded. Also returns the open tag's attribute text.
fn tag_text<'a>(block: &'a str, tag: &str) -> Option<(&'a str, String)> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut from = 0;
    loop {
        let start = from + block[from..].find(&open)?;
        let after = &block[start + open.len()..];
        // `<source` must not match `<sourceX`.
        let next = after.chars().next()?;
        if next != '>' && next != ' ' && next != '/' {
            from = start + open.len();
            continue;
        }
        let gt = after.find('>')?;
        let attrs = &after[..gt];
        if attrs.ends_with('/') {
            return Some((attrs, String::new()));
        }
        let body = &after[gt + 1..];
        let end = body.find(&close)?;
        let mut inner = body[..end].trim();
        if let Some(c) = inner.strip_prefix("<![CDATA[") {
            inner = c.strip_suffix("]]>").unwrap_or(c);
            return Some((attrs, inner.trim().to_string()));
        }
        return Some((attrs, decode_entities(inner).trim().to_string()));
    }
}

fn is_web_url(u: &str) -> bool {
    u.starts_with("https://") || u.starts_with("http://")
}

/// Parse a Google News search RSS body into at most `max` stories.
pub fn parse_google_rss(body: &str, topic: &str, max: usize) -> Vec<Story> {
    let mut out = Vec::new();
    for chunk in body.split("<item>").skip(1) {
        if out.len() >= max {
            break;
        }
        let block = chunk.split("</item>").next().unwrap_or("");
        let Some((_, title)) = tag_text(block, "title") else {
            continue;
        };
        let Some((_, link)) = tag_text(block, "link") else {
            continue;
        };
        if title.is_empty() || !is_web_url(&link) {
            continue;
        }
        let outlet = tag_text(block, "source")
            .map(|(_, t)| t)
            .filter(|t| !t.is_empty());
        // Google appends " - Outlet" to every title.
        let trimmed = outlet
            .as_deref()
            .and_then(|o| title.strip_suffix(o))
            .and_then(|t| t.strip_suffix(" - "))
            .map(str::to_string);
        let title = trimmed.unwrap_or(title);
        let published_ms = tag_text(block, "pubDate")
            .and_then(|(_, d)| chrono::DateTime::parse_from_rfc2822(&d).ok())
            .map(|d| d.timestamp_millis());
        let outlets = tag_text(block, "description")
            .map(|(_, d)| d.matches("<li>").count())
            .unwrap_or(0)
            .max(1);
        out.push(Story {
            source: Source::GoogleNews,
            topic: topic.to_string(),
            title,
            url: link,
            outlet,
            outlets,
            published_ms,
            points: None,
            comments: None,
        });
    }
    out
}

#[derive(Deserialize)]
struct HnPage {
    #[serde(default)]
    hits: Vec<HnHit>,
}

#[derive(Deserialize)]
struct HnHit {
    #[serde(rename = "objectID")]
    object_id: String,
    title: Option<String>,
    url: Option<String>,
    points: Option<i64>,
    num_comments: Option<i64>,
    created_at_i: Option<i64>,
}

/// Parse a Hacker News Search API page. A story without a link (Ask HN)
/// links to its discussion.
pub fn parse_hn(body: &str, topic: &str, max: usize) -> Result<Vec<Story>, FeedError> {
    let page: HnPage = serde_json::from_str(body).map_err(|_| FeedError::BadBody)?;
    Ok(page
        .hits
        .into_iter()
        .filter_map(|h| {
            let title = h.title.filter(|t| !t.trim().is_empty())?;
            let url = h
                .url
                .filter(|u| is_web_url(u))
                .unwrap_or_else(|| format!("https://news.ycombinator.com/item?id={}", h.object_id));
            Some(Story {
                source: Source::HackerNews,
                topic: topic.to_string(),
                title: title.trim().to_string(),
                url,
                outlet: None,
                outlets: 1,
                published_ms: h.created_at_i.map(|s| s * 1000),
                points: h.points,
                comments: h.num_comments,
            })
        })
        .take(max)
        .collect())
}

#[derive(Deserialize)]
struct FinnhubQuote {
    #[serde(default)]
    c: f64,
    #[serde(default)]
    dp: Option<f64>,
    #[serde(default)]
    pc: f64,
    #[serde(default)]
    t: Option<i64>,
}

/// Parse a quote answer. An unknown symbol answers all zeros: `Ok(None)`.
pub fn parse_quote(body: &str, symbol: &str) -> Result<Option<Quote>, FeedError> {
    let q: FinnhubQuote = serde_json::from_str(body).map_err(|_| FeedError::BadBody)?;
    if q.c <= 0.0 || !q.c.is_finite() {
        return Ok(None);
    }
    let change_pct = match q.dp {
        Some(dp) if dp.is_finite() => dp,
        _ if q.pc > 0.0 => (q.c - q.pc) / q.pc * 100.0,
        _ => 0.0,
    };
    Ok(Some(Quote {
        symbol: symbol.to_string(),
        price: q.c,
        change_pct,
        prev_close: q.pc,
        as_of_ms: q.t.filter(|t| *t > 0).map(|t| t * 1000),
        delayed: true,
    }))
}

/// Running totals for one source in one refresh.
struct Tally {
    source: Source,
    started: Instant,
    count: usize,
    requests: usize,
    throttled: usize,
    error: Option<FeedError>,
}

impl Tally {
    fn new(source: Source) -> Self {
        Self {
            source,
            started: Instant::now(),
            count: 0,
            requests: 0,
            throttled: 0,
            error: None,
        }
    }

    fn finish(self) -> SourceOutcome {
        let ms = self.started.elapsed().as_millis() as u64;
        let error = self.error.as_ref().map(|e| e.tag().to_string());
        log::info!(
            "news fetch source={} ms={} count={} requests={} throttled={} error={}",
            self.source.tag(),
            ms,
            self.count,
            self.requests,
            self.throttled,
            error.as_deref().unwrap_or("none"),
        );
        SourceOutcome {
            source: self.source,
            ms,
            count: self.count,
            requests: self.requests,
            throttled: self.throttled,
            error,
        }
    }
}

/// Send one request if the limiter allows it. `Ok(None)` = held back.
fn fetch_one(
    http: &dyn FeedHttp,
    limiter: &mut RateLimiter,
    tally: &mut Tally,
    query: &str,
    url: &str,
    headers: &[(&str, &str)],
    now_ms: i64,
) -> Option<String> {
    let source = tally.source;
    if tally.requests >= source.limits().per_refresh {
        tally.throttled += 1;
        return None;
    }
    if limiter.check(source, query, now_ms).is_err() {
        tally.throttled += 1;
        return None;
    }
    limiter.note(source, query, now_ms);
    tally.requests += 1;
    match http.get(source, url, headers, source.limits().timeout) {
        Ok(body) => Some(body),
        Err(e) => {
            if e == FeedError::RateLimited {
                limiter.back_off(source, now_ms);
            }
            tally.error = Some(e);
            None
        }
    }
}

/// Fetch every source the prefs ask for. Makes NO request when [`gate`]
/// refuses. `quote_key` is the user's quote-feed key, if any.
pub fn refresh(
    kv: &dyn SettingsKv,
    http: &dyn FeedHttp,
    limiter: &mut RateLimiter,
    now_ms: i64,
    quote_key: Option<&str>,
) -> Result<FetchReport, Closed> {
    let prefs = match gate(kv) {
        Ok(p) => p,
        Err(why) => {
            log::info!("news fetch_skipped reason={}", why.tag());
            return Err(why);
        }
    };
    let mut report = FetchReport::default();
    let mut seen: HashSet<String> = HashSet::new();

    // Google News: one search per topic chip, then each custom word.
    let mut g = Tally::new(Source::GoogleNews);
    let mut searches: Vec<(String, String)> = prefs
        .topics
        .iter()
        .filter_map(|t| google_query(t).map(|q| (t.clone(), q.to_string())))
        .collect();
    searches.extend(prefs.custom.iter().map(|c| (c.clone(), c.clone())));
    for (topic, words) in &searches {
        let Some(url) = google_news_url(words) else {
            continue;
        };
        if let Some(body) = fetch_one(http, limiter, &mut g, words, &url, &[], now_ms) {
            for s in parse_google_rss(&body, topic, GOOGLE_PER_QUERY) {
                if seen.insert(s.url.clone()) {
                    g.count += 1;
                    report.stories.push(s);
                }
            }
        }
    }

    // Hacker News: the front page, for the developer chip.
    let mut h = Tally::new(Source::HackerNews);
    if prefs.topics.iter().any(|t| t == "developer") {
        let url = hn_front_page_url(HN_TOP);
        if let Some(body) = fetch_one(http, limiter, &mut h, "front_page", &url, &[], now_ms) {
            match parse_hn(&body, "developer", HN_TOP) {
                Ok(stories) => {
                    for s in stories {
                        if seen.insert(s.url.clone()) {
                            h.count += 1;
                            report.stories.push(s);
                        }
                    }
                }
                Err(e) => h.error = Some(e),
            }
        }
    }

    // Quotes: one call per ticker, only with a key.
    let mut q = Tally::new(Source::Quotes);
    if !prefs.tickers.is_empty() {
        match quote_key.map(str::trim).filter(|k| !k.is_empty()) {
            None => q.error = Some(FeedError::NoKey),
            Some(key) => {
                for sym in &prefs.tickers {
                    let Some(url) = quote_url(sym) else {
                        continue;
                    };
                    let headers = [("X-Finnhub-Token", key)];
                    if let Some(body) =
                        fetch_one(http, limiter, &mut q, sym, &url, &headers, now_ms)
                    {
                        match parse_quote(&body, sym) {
                            Ok(Some(quote)) => {
                                q.count += 1;
                                report.quotes.push(quote);
                            }
                            Ok(None) => {}
                            Err(e) => q.error = Some(e),
                        }
                    }
                }
            }
        }
    }

    for t in [g, h, q] {
        report.requests += t.requests;
        report.outcomes.push(t.finish());
    }
    let any_ok = report
        .outcomes
        .iter()
        .any(|o| o.requests > 0 && o.error.is_none());
    if any_ok {
        if let Err(e) = news_prefs::record_refresh(kv, now_ms) {
            log::warn!("news fetch record_refresh_failed error=\"{e}\"");
        }
    }
    Ok(report)
}

/// The process's one limiter, so every caller shares the same budget.
pub fn shared_limiter() -> &'static Mutex<RateLimiter> {
    static LIMITER: OnceLock<Mutex<RateLimiter>> = OnceLock::new();
    LIMITER.get_or_init(|| Mutex::new(RateLimiter::new()))
}

/// [`refresh`] over the real network with the shared limiter. Blocking: run
/// it on a blocking thread, never inside an async task.
pub fn refresh_now(kv: &dyn SettingsKv, quote_key: Option<&str>) -> Result<FetchReport, Closed> {
    let mut limiter = shared_limiter().lock().unwrap_or_else(|p| p.into_inner());
    refresh(kv, &NetHttp, &mut limiter, news_prefs::now_ms(), quote_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_providers::{self, ProviderConfig, ProviderKind, ProviderSettings};
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

    /// One recorded request: source, URL, headers, timeout.
    type Call = (Source, String, Vec<(String, String)>, Duration);

    /// Counts every request; answers from canned bodies per source.
    #[derive(Default)]
    struct FakeHttp {
        calls: RefCell<Vec<Call>>,
        google: Option<Result<String, FeedError>>,
        hn: Option<Result<String, FeedError>>,
        quote: Option<Result<String, FeedError>>,
    }
    impl FakeHttp {
        fn count(&self) -> usize {
            self.calls.borrow().len()
        }
        fn count_for(&self, s: Source) -> usize {
            self.calls.borrow().iter().filter(|c| c.0 == s).count()
        }
    }
    impl FeedHttp for FakeHttp {
        fn get(
            &self,
            source: Source,
            url: &str,
            headers: &[(&str, &str)],
            timeout: Duration,
        ) -> Result<String, FeedError> {
            self.calls.borrow_mut().push((
                source,
                url.to_string(),
                headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                timeout,
            ));
            let canned = match source {
                Source::GoogleNews => &self.google,
                Source::HackerNews => &self.hn,
                Source::Quotes => &self.quote,
            };
            canned.clone().unwrap_or(Err(FeedError::Unreachable))
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

    fn set_prefs(kv: &MemKv, enabled: bool, topics: &[&str], tickers: &[&str], custom: &[&str]) {
        let p = NewsPrefs {
            enabled,
            topics: topics.iter().map(|s| s.to_string()).collect(),
            tickers: tickers.iter().map(|s| s.to_string()).collect(),
            custom: custom.iter().map(|s| s.to_string()).collect(),
        };
        kv.kv_set(news_prefs::PREFS_KEY, &serde_json::to_string(&p).unwrap())
            .unwrap();
    }

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><rss version="2.0"><channel><title>"tech" - Google News</title>
<item><title>US tech stocks slide - The Guardian</title><link>https://news.google.com/rss/articles/ABC?oc=5</link><guid isPermaLink="false">ABC</guid><pubDate>Fri, 06 Nov 2026 20:38:00 GMT</pubDate><description>&lt;ol&gt;&lt;li&gt;&lt;a href="x"&gt;a&lt;/a&gt;&lt;/li&gt;&lt;li&gt;b&lt;/li&gt;&lt;li&gt;c&lt;/li&gt;&lt;/ol&gt;</description><source url="https://www.theguardian.com">The Guardian</source></item>
<item><title><![CDATA[Chips &amp; cloud - Reuters]]></title><link>https://news.google.com/rss/articles/DEF?oc=5</link><pubDate>not a date</pubDate><source url="https://www.reuters.com">Reuters</source></item>
<item><title>No link here</title></item>
<item><title>Bad scheme</title><link>javascript:alert(1)</link></item>
</channel></rss>"#;

    const HN: &str = r#"{"hits":[
{"objectID":"1","title":"A desktop app framework ships 3.0","url":"https://example.com/a","points":412,"num_comments":90,"created_at_i":1794027251},
{"objectID":"2","title":"Ask HN: What are you building?","url":null,"points":50,"num_comments":300,"created_at_i":1794027000},
{"objectID":"3","title":null,"url":"https://example.com/c"}
],"nbHits":3}"#;

    const QUOTE: &str =
        r#"{"c":120.5,"d":3.6,"dp":3.08,"h":121,"l":117,"o":118,"pc":116.9,"t":1794000000}"#;

    // ---- the gate: no request unless cloud on AND a topic is set ----

    #[test]
    fn no_request_when_cloud_is_off() {
        let kv = MemKv::default();
        set_prefs(&kv, true, &["tech", "developer"], &["NVDA"], &["rust"]);
        let http = FakeHttp::default();
        let mut lim = RateLimiter::new();
        let r = refresh(&kv, &http, &mut lim, 1_000_000, Some("k"));
        assert_eq!(r, Err(Closed::CloudOff));
        assert_eq!(http.count(), 0, "cloud off must make zero requests");
        assert!(kv.kv_get(news_prefs::LAST_REFRESH_KEY).is_none());
    }

    #[test]
    fn no_request_without_any_topic() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        set_prefs(&kv, true, &[], &[], &[]);
        let http = FakeHttp::default();
        let mut lim = RateLimiter::new();
        assert_eq!(
            refresh(&kv, &http, &mut lim, 1_000_000, Some("k")),
            Err(Closed::NoTopics)
        );
        assert_eq!(http.count(), 0, "no topic must make zero requests");
    }

    #[test]
    fn no_request_with_default_prefs_even_when_cloud_on() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        let http = FakeHttp::default();
        let mut lim = RateLimiter::new();
        assert_eq!(
            refresh(&kv, &http, &mut lim, 1_000_000, None),
            Err(Closed::NoTopics)
        );
        assert_eq!(http.count(), 0);
    }

    #[test]
    fn no_request_when_user_switched_news_off() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        set_prefs(&kv, false, &["tech"], &[], &[]);
        let http = FakeHttp::default();
        let mut lim = RateLimiter::new();
        assert_eq!(
            refresh(&kv, &http, &mut lim, 1_000_000, None),
            Err(Closed::Disabled)
        );
        assert_eq!(http.count(), 0);
    }

    #[test]
    fn requests_once_cloud_is_on_and_a_topic_is_set() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        set_prefs(&kv, true, &["tech"], &[], &[]);
        let http = FakeHttp {
            google: Some(Ok(RSS.into())),
            ..Default::default()
        };
        let mut lim = RateLimiter::new();
        let r = refresh(&kv, &http, &mut lim, 1_000_000, None).expect("gate open");
        assert_eq!(http.count(), 1);
        assert_eq!(r.requests, 1);
        assert_eq!(r.stories.len(), 2);
        let call = &http.calls.borrow()[0];
        assert!(call
            .1
            .starts_with("https://news.google.com/rss/search?q=technology"));
        assert_eq!(call.3, Source::GoogleNews.limits().timeout);
        assert_eq!(
            kv.kv_get(news_prefs::LAST_REFRESH_KEY).as_deref(),
            Some("1000000")
        );
    }

    // ---- the three sources ----

    #[test]
    fn fetches_all_three_sources() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        set_prefs(
            &kv,
            true,
            &["developer", "business"],
            &["NVDA", "AAPL"],
            &["small business loans"],
        );
        let http = FakeHttp {
            google: Some(Ok(RSS.into())),
            hn: Some(Ok(HN.into())),
            quote: Some(Ok(QUOTE.into())),
            ..Default::default()
        };
        let mut lim = RateLimiter::new();
        let r = refresh(&kv, &http, &mut lim, 5_000_000, Some("secret")).unwrap();
        assert_eq!(
            http.count_for(Source::GoogleNews),
            2,
            "business + one custom word"
        );
        assert_eq!(http.count_for(Source::HackerNews), 1);
        assert_eq!(http.count_for(Source::Quotes), 2);
        assert_eq!(r.quotes.len(), 2);
        assert!(r.quotes.iter().all(|q| q.delayed));
        // The same Google stories under two queries are kept once.
        assert_eq!(
            r.stories
                .iter()
                .filter(|s| s.source == Source::GoogleNews)
                .count(),
            2
        );
        assert_eq!(
            r.stories
                .iter()
                .filter(|s| s.source == Source::HackerNews)
                .count(),
            2
        );
        // The quote key travels in a header, never in the URL.
        for c in http.calls.borrow().iter().filter(|c| c.0 == Source::Quotes) {
            assert!(!c.1.contains("secret"));
            assert_eq!(
                c.2,
                vec![("X-Finnhub-Token".to_string(), "secret".to_string())]
            );
        }
        assert_eq!(r.outcomes.len(), 3);
        assert!(r.outcomes.iter().all(|o| o.error.is_none()));
    }

    #[test]
    fn quotes_without_a_key_make_no_request() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        set_prefs(&kv, true, &[], &["NVDA"], &[]);
        let http = FakeHttp::default();
        let mut lim = RateLimiter::new();
        let r = refresh(&kv, &http, &mut lim, 1_000_000, None).unwrap();
        assert_eq!(http.count(), 0);
        let q = r
            .outcomes
            .iter()
            .find(|o| o.source == Source::Quotes)
            .unwrap();
        assert_eq!(q.error.as_deref(), Some("no_key"));
        assert!(kv.kv_get(news_prefs::LAST_REFRESH_KEY).is_none());
    }

    #[test]
    fn every_source_has_a_timeout_and_limits() {
        for s in Source::ALL {
            let l = s.limits();
            assert!(l.timeout > Duration::ZERO && l.timeout <= Duration::from_secs(10));
            assert!(
                l.per_query_ms >= THIRTY_MIN_MS,
                "design: never more than every 30 min"
            );
            assert!(l.per_hour > 0 && l.per_refresh > 0);
            assert!(l.per_refresh <= l.per_hour);
        }
    }

    // ---- rate limits ----

    #[test]
    fn same_query_is_not_refetched_within_thirty_minutes() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        set_prefs(&kv, true, &["tech", "developer"], &[], &[]);
        let http = FakeHttp {
            google: Some(Ok(RSS.into())),
            hn: Some(Ok(HN.into())),
            ..Default::default()
        };
        let mut lim = RateLimiter::new();
        let t0 = 10 * HOUR_MS;
        refresh(&kv, &http, &mut lim, t0, None).unwrap();
        assert_eq!(http.count(), 2);
        let again = refresh(&kv, &http, &mut lim, t0 + 29 * 60 * 1000, None).unwrap();
        assert_eq!(http.count(), 2, "inside 30 min nothing is re-sent");
        assert_eq!(again.requests, 0);
        assert!(again.outcomes.iter().any(|o| o.throttled > 0));
        refresh(&kv, &http, &mut lim, t0 + 31 * 60 * 1000, None).unwrap();
        assert_eq!(http.count(), 4);
    }

    #[test]
    fn per_refresh_cap_holds_google_to_eight_queries() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        let custom: Vec<String> = (0..10).map(|i| format!("word{i}")).collect();
        let custom_refs: Vec<&str> = custom.iter().map(String::as_str).collect();
        set_prefs(&kv, true, &["tech", "science"], &[], &custom_refs);
        let http = FakeHttp {
            google: Some(Ok(RSS.into())),
            ..Default::default()
        };
        let mut lim = RateLimiter::new();
        let r = refresh(&kv, &http, &mut lim, HOUR_MS, None).unwrap();
        assert_eq!(http.count_for(Source::GoogleNews), 8);
        let g = r
            .outcomes
            .iter()
            .find(|o| o.source == Source::GoogleNews)
            .unwrap();
        assert_eq!(g.throttled, 4);
    }

    #[test]
    fn hour_cap_and_backoff() {
        let mut lim = RateLimiter::new();
        let t = 100 * HOUR_MS;
        for i in 0..6 {
            let q = format!("q{i}");
            assert!(lim.check(Source::HackerNews, &q, t).is_ok());
            lim.note(Source::HackerNews, &q, t);
        }
        assert_eq!(
            lim.check(Source::HackerNews, "new", t),
            Err(Throttle::HourCap)
        );
        assert!(lim.check(Source::HackerNews, "new", t + HOUR_MS).is_ok());
        lim.back_off(Source::Quotes, t);
        assert_eq!(
            lim.check(Source::Quotes, "NVDA", t + 1),
            Err(Throttle::BackedOff)
        );
        assert!(lim.check(Source::Quotes, "NVDA", t + BACKOFF_MS).is_ok());
    }

    #[test]
    fn a_429_backs_the_source_off_and_is_reported() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        set_prefs(&kv, true, &["tech"], &[], &[]);
        let http = FakeHttp {
            google: Some(Err(FeedError::RateLimited)),
            ..Default::default()
        };
        let mut lim = RateLimiter::new();
        let r = refresh(&kv, &http, &mut lim, HOUR_MS, None).unwrap();
        let g = r
            .outcomes
            .iter()
            .find(|o| o.source == Source::GoogleNews)
            .unwrap();
        assert_eq!(g.error.as_deref(), Some("rate_limited"));
        assert_eq!(
            lim.check(Source::GoogleNews, "other", HOUR_MS + 1),
            Err(Throttle::BackedOff)
        );
        assert!(kv.kv_get(news_prefs::LAST_REFRESH_KEY).is_none());
    }

    #[test]
    fn a_timeout_is_reported_with_the_source() {
        let kv = MemKv::default();
        connect_cloud(&kv);
        set_prefs(&kv, true, &["developer"], &[], &[]);
        let http = FakeHttp {
            hn: Some(Err(FeedError::Timeout { secs: 8 })),
            ..Default::default()
        };
        let mut lim = RateLimiter::new();
        let r = refresh(&kv, &http, &mut lim, HOUR_MS, None).unwrap();
        let h = r
            .outcomes
            .iter()
            .find(|o| o.source == Source::HackerNews)
            .unwrap();
        assert_eq!(h.error.as_deref(), Some("timeout"));
        assert_eq!(h.count, 0);
        assert!(FeedError::Timeout { secs: 8 }.plain().contains("8 s"));
    }

    // ---- parsers ----

    #[test]
    fn parses_google_news_rss() {
        let s = parse_google_rss(RSS, "tech", 10);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].title, "US tech stocks slide");
        assert_eq!(s[0].outlet.as_deref(), Some("The Guardian"));
        assert_eq!(s[0].outlets, 3);
        assert_eq!(s[0].published_ms, Some(1_793_997_480_000));
        assert_eq!(s[1].title, "Chips &amp; cloud");
        assert_eq!(s[1].outlet.as_deref(), Some("Reuters"));
        assert_eq!(s[1].published_ms, None);
        assert_eq!(s[1].outlets, 1);
        assert_eq!(parse_google_rss(RSS, "tech", 1).len(), 1);
        assert!(parse_google_rss("<html>not a feed</html>", "tech", 5).is_empty());
    }

    #[test]
    fn parses_hacker_news() {
        let s = parse_hn(HN, "developer", 10).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].points, Some(412));
        assert_eq!(s[0].published_ms, Some(1_794_027_251_000));
        assert_eq!(s[1].url, "https://news.ycombinator.com/item?id=2");
        assert_eq!(parse_hn("nope", "developer", 10), Err(FeedError::BadBody));
    }

    #[test]
    fn parses_a_quote_and_skips_unknown_symbols() {
        let q = parse_quote(QUOTE, "NVDA").unwrap().unwrap();
        assert_eq!(q.symbol, "NVDA");
        assert!((q.change_pct - 3.08).abs() < 1e-9);
        assert!(q.delayed);
        assert_eq!(q.as_of_ms, Some(1_794_000_000_000));
        let zero = r#"{"c":0,"d":null,"dp":null,"h":0,"l":0,"o":0,"pc":0,"t":0}"#;
        assert_eq!(parse_quote(zero, "ZZZZ").unwrap(), None);
        let no_dp = r#"{"c":110,"pc":100}"#;
        assert!((parse_quote(no_dp, "X").unwrap().unwrap().change_pct - 10.0).abs() < 1e-9);
    }

    #[test]
    fn decodes_entities_and_builds_encoded_urls() {
        assert_eq!(
            decode_entities("a &amp; b &lt;c&gt; &#39;d&#x27; &bogus; & e"),
            "a & b <c> 'd' &bogus; & e"
        );
        let u = google_news_url("small business & loans").unwrap();
        assert!(u.starts_with("https://news.google.com/rss/search?q="));
        assert!(!u.contains(' '));
        assert!(u.contains("%26"));
        assert!(u.ends_with("&hl=en-US&gl=US&ceid=US%3Aen"));
        assert_eq!(
            quote_url("BRK.B").unwrap(),
            "https://finnhub.io/api/v1/quote?symbol=BRK.B"
        );
    }
}
