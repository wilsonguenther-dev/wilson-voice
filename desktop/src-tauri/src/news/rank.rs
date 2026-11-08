//! yap-1106-news-rank-cache — dedupe and rank stories. Pure: no I/O, no
//! clock, no logging; [`super::cache`] owns the SQLite side and the telemetry.
//!
//! * **Dedupe** by URL ([`url_key`]: scheme, `www.`, fragment, trailing slash
//!   and tracking parameters dropped) and by title similarity ([`same_title`]:
//!   the outlet suffix Google adds is stripped, then word-set overlap).
//! * **Score** = topic match ([`topic_score`]) + freshness ([`freshness`]), with
//!   a small nudge for stories many outlets carry. A story that matches none of
//!   the user's interests scores nothing and is dropped.
//! * yap-1107-news-no-off-topic-stories — the topic match comes from the
//!   story's own category ([`sources::match_prefs`]: declared source, cached
//!   cloud label, offline keywords), never from the search that found it,
//!   and topic search words earn no title bonus ("world" in "World Wide
//!   Technology" scores nothing). Only tickers and own words do.

use std::cmp::Ordering;
use std::collections::HashSet;

use serde::Serialize;
use tauri::Url; // the url crate's type, re-exported; parsing only, no network client

use super::feeds::Story;
use super::sources::{self, Category, ClassifiedBy, Labels, Match};
use crate::news_prefs::NewsPrefs;

/// Word-set overlap (Jaccard) at or above which two titles are one story.
pub const TITLE_SIMILAR: f64 = 0.6;
/// Fuzzy title matching needs this many words on both sides; shorter titles
/// only merge on an exact match.
pub const MIN_FUZZY_WORDS: usize = 3;
/// Freshness halves every 6 hours.
pub const FRESH_HALF_LIFE_MS: i64 = 6 * 60 * 60 * 1000;
/// Freshness is worth up to this many points (a brand-new story).
pub const FRESH_WEIGHT: f64 = 2.0;
/// Freshness for a story with no publish time.
pub const UNKNOWN_AGE_FRESHNESS: f64 = 0.25;
/// Points for a story found by one of the user's own topics or words.
pub const TOPIC_BASE: f64 = 1.0;
/// Points per interest named in the title, counted up to [`MAX_TITLE_HITS`].
pub const TITLE_HIT: f64 = 0.5;
pub const MAX_TITLE_HITS: usize = 3;
/// Weight of `ln(outlets)`: a tiebreak, never enough to beat a topic hit.
pub const OUTLETS_WEIGHT: f64 = 0.1;

const TRACKING_PARAMS: [&str; 11] = [
    "fbclid", "gclid", "dclid", "msclkid", "ocid", "ref", "ref_src", "cmpid", "smid", "mc_cid",
    "mc_eid",
];

const STOP_WORDS: [&str; 24] = [
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "in", "is", "it", "its",
    "of", "on", "or", "that", "the", "to", "was", "will", "with",
];

fn is_tracking(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.starts_with("utm_") || TRACKING_PARAMS.contains(&k.as_str())
}

/// The URL two links to the same page share: no scheme, no `www.`, no
/// fragment, no trailing slash, no tracking parameters, the rest sorted.
pub fn url_key(raw: &str) -> String {
    let trimmed = raw.trim();
    let Ok(u) = Url::parse(trimmed) else {
        return trimmed.to_ascii_lowercase();
    };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let path = u.path().trim_end_matches('/');
    let mut kept: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(k, _)| !is_tracking(k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    kept.sort();
    let mut key = format!("{host}{path}");
    if !kept.is_empty() {
        let q: Vec<String> = kept.iter().map(|(k, v)| format!("{k}={v}")).collect();
        key.push('?');
        key.push_str(&q.join("&"));
    }
    key
}

/// The title without the " - Outlet" suffix Google News appends.
fn strip_outlet<'a>(title: &'a str, outlet: Option<&str>) -> &'a str {
    let t = title.trim();
    let Some(o) = outlet.map(str::trim).filter(|o| !o.is_empty()) else {
        return t;
    };
    for sep in [" - ", " | ", " \u{2014} ", " \u{2013} "] {
        if let Some(head) = t.strip_suffix(o).and_then(|h| h.strip_suffix(sep)) {
            if !head.trim().is_empty() {
                return head.trim();
            }
        }
    }
    t
}

/// Lowercase words of a title, stop words dropped, first occurrence order.
pub fn title_words(title: &str, outlet: Option<&str>) -> Vec<String> {
    let mut seen = HashSet::new();
    strip_outlet(title, outlet)
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| !w.is_empty() && !STOP_WORDS.contains(&w.as_str()))
        .filter(|w| seen.insert(w.clone()))
        .collect()
}

/// The seen-set key for a title: its words, in order.
pub fn title_key(title: &str, outlet: Option<&str>) -> String {
    title_words(title, outlet).join(" ")
}

/// Jaccard overlap of two word lists, 0.0 when either is empty.
pub fn similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let sa: HashSet<&String> = a.iter().collect();
    let sb: HashSet<&String> = b.iter().collect();
    let inter = sa.intersection(&sb).count() as f64;
    let union = sa.union(&sb).count() as f64;
    inter / union
}

/// Same story by title: identical words, or (both long enough) overlap at or
/// above [`TITLE_SIMILAR`].
pub fn same_title(a: &[String], b: &[String]) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    a.len() >= MIN_FUZZY_WORDS && b.len() >= MIN_FUZZY_WORDS && similarity(a, b) >= TITLE_SIMILAR
}

/// A story with its dedupe keys worked out once.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyed {
    pub story: Story,
    pub url_key: String,
    pub title_key: String,
    pub words: Vec<String>,
}

pub fn keyed(story: Story) -> Keyed {
    let words = title_words(&story.title, story.outlet.as_deref());
    Keyed {
        url_key: url_key(&story.url),
        title_key: words.join(" "),
        words,
        story,
    }
}

/// How many stories each dedupe rule removed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DedupeCounts {
    pub dup_url: usize,
    pub dup_title: usize,
}

/// Drop repeats, keeping the FIRST of each (callers put the cached copy
/// first). A dropped twin still counts: the keeper takes the larger outlet
/// count, the newer publish time and the larger points/comments.
pub fn dedupe(items: Vec<Keyed>) -> (Vec<Keyed>, DedupeCounts) {
    let mut kept: Vec<Keyed> = Vec::with_capacity(items.len());
    let mut counts = DedupeCounts::default();
    for item in items {
        let twin = kept.iter().position(|k| k.url_key == item.url_key);
        let twin = match twin {
            Some(i) => {
                counts.dup_url += 1;
                Some(i)
            }
            None => {
                let t = kept.iter().position(|k| same_title(&k.words, &item.words));
                if t.is_some() {
                    counts.dup_title += 1;
                }
                t
            }
        };
        match twin {
            Some(i) => {
                let k = &mut kept[i].story;
                k.outlets = k.outlets.max(item.story.outlets);
                k.published_ms = k.published_ms.max(item.story.published_ms);
                k.points = k.points.max(item.story.points);
                k.comments = k.comments.max(item.story.comments);
            }
            None => kept.push(item),
        }
    }
    (kept, counts)
}

/// Topic match: [`TOPIC_BASE`] when the story came from one of the user's
/// topics or custom words, plus [`TITLE_HIT`] per interest named in the title
/// (a custom word, a ticker, or a topic's search word). 0.0 = off topic.
pub fn topic_score(story: &Story, prefs: &NewsPrefs) -> f64 {
    topic_match(story, prefs, None).map_or(0.0, |(points, _)| points)
}

/// The topic points and the match, or `None` when the story's category is
/// none of the user's topics and it names none of their tickers or own
/// words. [`TOPIC_BASE`] for any match, plus [`TITLE_HIT`] per ticker or own
/// word the title names. Topic chips' search words never add a hit.
pub fn topic_match(
    story: &Story,
    prefs: &NewsPrefs,
    cloud: Option<Category>,
) -> Option<(f64, Match)> {
    let m = sources::match_prefs(story, prefs, cloud)?;
    let lower = story.title.to_lowercase();
    let words: HashSet<String> = title_words(&story.title, None).into_iter().collect();
    let mut hits = 0usize;
    for c in &prefs.custom {
        let c = c.trim().to_lowercase();
        if !c.is_empty() && lower.contains(&c) {
            hits += 1;
        }
    }
    for t in &prefs.tickers {
        if words.contains(&t.to_lowercase()) {
            hits += 1;
        }
    }
    Some((TOPIC_BASE + TITLE_HIT * hits.min(MAX_TITLE_HITS) as f64, m))
}

/// 1.0 for a story published now, halving every [`FRESH_HALF_LIFE_MS`];
/// [`UNKNOWN_AGE_FRESHNESS`] when the feed gave no time. A future time
/// (clock skew) counts as now.
pub fn freshness(published_ms: Option<i64>, now_ms: i64) -> f64 {
    match published_ms {
        None => UNKNOWN_AGE_FRESHNESS,
        Some(p) => {
            let age = now_ms.saturating_sub(p).max(0) as f64;
            0.5f64.powf(age / FRESH_HALF_LIFE_MS as f64)
        }
    }
}

/// The full score, or `None` for an off-topic story.
pub fn score(story: &Story, prefs: &NewsPrefs, now_ms: i64) -> Option<f64> {
    score_with(story, prefs, now_ms, None).map(|(s, _)| s)
}

/// [`score`] with the story's cached cloud label, returning the match too.
pub fn score_with(
    story: &Story,
    prefs: &NewsPrefs,
    now_ms: i64,
    cloud: Option<Category>,
) -> Option<(f64, Match)> {
    let (topic, m) = topic_match(story, prefs, cloud)?;
    let outlets = (story.outlets.max(1) as f64).ln();
    Some((
        topic + FRESH_WEIGHT * freshness(story.published_ms, now_ms) + OUTLETS_WEIGHT * outlets,
        m,
    ))
}

/// One ranked story, as the reader gets it. `urlKey`/`titleKey` go back to
/// [`super::cache::mark_seen`] once the story has been delivered.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedStory {
    #[serde(flatten)]
    pub story: Story,
    pub score: f64,
    pub url_key: String,
    pub title_key: String,
    /// yap-1107-news-no-off-topic-stories — the story's own category; the
    /// story's `topic` is the user's topic (or ticker / own word) it matched.
    pub category: Category,
    pub classified_by: ClassifiedBy,
    /// The card's "Why this story" line ([`sources::why_line`]).
    pub why: String,
}

/// Score, drop off-topic, sort best first (ties: newer, then URL so the
/// order is stable) and keep `limit`. Returns the stories and how many were
/// off topic.
pub fn rank(
    items: Vec<Keyed>,
    prefs: &NewsPrefs,
    now_ms: i64,
    limit: usize,
) -> (Vec<RankedStory>, usize) {
    rank_with(items, prefs, now_ms, limit, &Labels::new())
}

/// [`rank`] with the cached cloud labels (by url key). An off-topic story is
/// dropped and counted, never relabelled to the topic that searched for it.
pub fn rank_with(
    items: Vec<Keyed>,
    prefs: &NewsPrefs,
    now_ms: i64,
    limit: usize,
    labels: &Labels,
) -> (Vec<RankedStory>, usize) {
    let mut off_topic = 0usize;
    let mut out: Vec<RankedStory> = items
        .into_iter()
        .filter_map(|k| {
            let cloud = labels.get(&k.url_key).copied();
            match score_with(&k.story, prefs, now_ms, cloud) {
                Some((score, m)) => {
                    let why = sources::why_line(&k.story, &m);
                    let mut story = k.story;
                    story.topic = m.topic;
                    Some(RankedStory {
                        story,
                        score,
                        url_key: k.url_key,
                        title_key: k.title_key,
                        category: m.category,
                        classified_by: m.by,
                        why,
                    })
                }
                None => {
                    off_topic += 1;
                    None
                }
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| b.story.published_ms.cmp(&a.story.published_ms))
            .then_with(|| a.url_key.cmp(&b.url_key))
    });
    out.truncate(limit);
    (out, off_topic)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::news::feeds::Source;

    pub(crate) const NOW: i64 = 1_800_000_000_000;
    pub(crate) const HOUR: i64 = 60 * 60 * 1000;

    pub(crate) fn story(
        topic: &str,
        title: &str,
        url: &str,
        outlet: Option<&str>,
        age_h: Option<i64>,
    ) -> Story {
        Story {
            source: Source::GoogleNews,
            topic: topic.to_string(),
            title: title.to_string(),
            url: url.to_string(),
            outlet: outlet.map(str::to_string),
            outlets: 1,
            published_ms: age_h.map(|h| NOW - h * HOUR),
            points: None,
            comments: None,
            snippet: None,
        }
    }

    pub(crate) fn prefs(topics: &[&str], tickers: &[&str], custom: &[&str]) -> NewsPrefs {
        NewsPrefs {
            enabled: true,
            topics: topics.iter().map(|s| s.to_string()).collect(),
            tickers: tickers.iter().map(|s| s.to_string()).collect(),
            custom: custom.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn url_key_drops_scheme_www_fragment_slash_and_tracking() {
        let a = url_key("https://www.Example.com/news/story/?utm_source=x&id=7&fbclid=abc#top");
        let b = url_key("http://example.com/news/story?id=7");
        assert_eq!(a, b);
        assert_eq!(b, "example.com/news/story?id=7");
        assert_ne!(
            url_key("https://example.com/a"),
            url_key("https://example.com/b")
        );
    }

    #[test]
    fn dedupe_removes_same_url() {
        let items = vec![
            keyed(story(
                "tech",
                "Chip maker posts record quarter",
                "https://a.com/x?utm_medium=rss",
                None,
                Some(1),
            )),
            keyed(story(
                "tech",
                "Totally different words entirely here",
                "https://www.a.com/x/",
                None,
                Some(2),
            )),
        ];
        let (kept, counts) = dedupe(items);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            counts,
            DedupeCounts {
                dup_url: 1,
                dup_title: 0
            }
        );
    }

    #[test]
    fn dedupe_removes_similar_titles_across_outlets() {
        let mut second = story(
            "tech",
            "Chip maker posts a record quarter as AI demand surges - Reuters",
            "https://reuters.com/y",
            Some("Reuters"),
            Some(1),
        );
        second.outlets = 4;
        let items = vec![
            keyed(story(
                "tech",
                "Chip maker posts record quarter as AI demand surges - The Verge",
                "https://theverge.com/x",
                Some("The Verge"),
                Some(3),
            )),
            keyed(second),
            keyed(story(
                "tech",
                "Rocket lands on barge again",
                "https://c.com/z",
                None,
                Some(1),
            )),
        ];
        let (kept, counts) = dedupe(items);
        assert_eq!(kept.len(), 2);
        assert_eq!(
            counts,
            DedupeCounts {
                dup_url: 0,
                dup_title: 1
            }
        );
        // The keeper takes the twin's larger outlet count and newer time.
        assert_eq!(kept[0].story.outlets, 4);
        assert_eq!(kept[0].story.published_ms, Some(NOW - HOUR));
        assert_eq!(
            kept[0].title_key,
            "chip maker posts record quarter ai demand surges"
        );
    }

    #[test]
    fn short_titles_only_merge_when_identical() {
        let a = title_words("Apple stock", None);
        let b = title_words("Apple stock falls", None);
        assert!(!same_title(&a, &b));
        assert!(same_title(&a, &title_words("APPLE: stock!", None)));
    }

    #[test]
    fn topic_score_matches_interests_and_drops_off_topic() {
        let p = prefs(&["tech"], &["NVDA"], &["electric cars"]);
        let found_by_topic = story(
            "tech",
            "Quiet day for gadgets",
            "https://a.com/1",
            None,
            Some(1),
        );
        let named_in_title = story(
            "world",
            "NVDA and electric cars lead technology rally",
            "https://a.com/2",
            None,
            Some(1),
        );
        let off_topic = story(
            "sports",
            "Local team wins the cup",
            "https://a.com/3",
            None,
            Some(1),
        );
        assert_eq!(topic_score(&found_by_topic, &p), TOPIC_BASE);
        // Category Stocks (not picked): kept by the ticker and own word,
        // which each add a hit; "technology" (a topic search word) adds none.
        assert_eq!(
            topic_score(&named_in_title, &p),
            TOPIC_BASE + TITLE_HIT * 2.0
        );
        assert_eq!(topic_score(&off_topic, &p), 0.0);
        assert!(score(&off_topic, &p, NOW).is_none());
    }

    #[test]
    fn topic_search_words_add_no_title_hit() {
        let p = prefs(&["tech", "world", "business"], &[], &[]);
        // The event name is a deny phrase: Sports, so it scores nothing.
        let wwt = story(
            "tech",
            "World Wide Technology posts record revenue",
            "https://a.com/wwt",
            Some("Reuters"),
            Some(1),
        );
        assert_eq!(topic_score(&wwt, &p), 0.0);
        // Matches on its category; "world" and "technology" add no hit.
        let leaders = story(
            "world",
            "World leaders discuss technology at summit",
            "https://a.com/w",
            Some("Reuters"),
            Some(1),
        );
        assert_eq!(topic_score(&leaders, &p), TOPIC_BASE);
    }

    #[test]
    fn freshness_halves_every_six_hours() {
        assert!((freshness(Some(NOW), NOW) - 1.0).abs() < 1e-9);
        assert!((freshness(Some(NOW - 6 * HOUR), NOW) - 0.5).abs() < 1e-9);
        assert!((freshness(Some(NOW - 12 * HOUR), NOW) - 0.25).abs() < 1e-9);
        assert_eq!(freshness(Some(NOW + HOUR), NOW), 1.0);
        assert_eq!(freshness(None, NOW), UNKNOWN_AGE_FRESHNESS);
    }

    #[test]
    fn rank_orders_by_topic_match_then_freshness_and_limits() {
        let p = prefs(&["tech"], &[], &["fusion"]);
        let items = vec![
            keyed(story(
                "tech",
                "Old gadget review roundup",
                "https://a.com/old",
                None,
                Some(20),
            )),
            keyed(story(
                "tech",
                "New gadget review roundup today",
                "https://a.com/new",
                None,
                Some(0),
            )),
            keyed(story(
                "fusion",
                "Fusion reactor hits milestone",
                "https://a.com/fusion",
                None,
                Some(10),
            )),
            keyed(story(
                "sports",
                "Cup final tonight",
                "https://a.com/sport",
                None,
                Some(0),
            )),
        ];
        let (ranked, off) = rank(items, &p, NOW, 3);
        assert_eq!(off, 1);
        assert_eq!(ranked[0].why, "From the news (Tech) \u{b7} matches Tech");
        let urls: Vec<&str> = ranked.iter().map(|r| r.story.url.as_str()).collect();
        // fusion: topic 1.0 + title hit 0.5 + fresh 2*0.31; new: 1.0 + 2*1.0; old: 1.0 + 2*0.1
        assert_eq!(
            urls,
            vec![
                "https://a.com/new",
                "https://a.com/fusion",
                "https://a.com/old"
            ]
        );
        let (two, _) = rank(
            vec![keyed(story("tech", "a b c", "https://a.com/1", None, None))],
            &p,
            NOW,
            0,
        );
        assert!(two.is_empty());
    }
}
