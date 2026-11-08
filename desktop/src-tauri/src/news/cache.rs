//! yap-1106-news-rank-cache — the on-Mac story cache and seen-set.
//!
//! Two tables, both disposable and LOCAL ONLY:
//!
//! * `news_cache` — one row per story, keyed by [`rank::url_key`], kept for
//!   [`TTL_MS`] (24 h) from when it was fetched. Reads never return an expired
//!   row and every [`ingest`] prunes them first.
//! * `news_seen` — the url and title keys of stories already delivered, so a
//!   repeat (same link, or the same headline from another outlet) is skipped
//!   for [`SEEN_TTL_MS`].
//!
//! Turning Yappy News off deletes both ([`apply_prefs`], called from the
//! `set_news_prefs` command).
//!
//! The schema is created with `IF NOT EXISTS` on every open
//! ([`ensure_schema`], from `Database::open_inner`) instead of a numbered step
//! of the `user_version` ladder: the tables are a throwaway cache (dropping
//! them loses nothing), and a ladder number would collide with other schema
//! work landing in parallel.
//!
//! * `news_class` (yap-1107-news-no-off-topic-stories) — the cloud's
//!   category label per url key, so a story is sent to the cloud once
//!   ([`classify_batch`]); kept as long as a cached story.
//!
//! Telemetry: `news classify candidates=… cached=… sent=… labelled=…
//! calls=… ms=… fallback=…` per refresh with undeclared stories, `news rank fetched=… added=… dup_url=… dup_title=… seen=…
//! off_topic=… expired=… kept=… ms=…` per ingest, `news seen_marked count=…`,
//! `news cache_cleared reason=… rows=…`. No titles or URLs are ever logged.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use rusqlite::{params, Connection};
use serde::Serialize;

use super::feeds::{Source, Story};
use super::rank::{self, DedupeCounts, Keyed, RankedStory};
use super::sources::{self, Category, Labels};
use crate::news_prefs::NewsPrefs;

const HOUR_MS: i64 = 60 * 60 * 1000;
/// How long a fetched story stays in the cache.
pub const TTL_MS: i64 = 24 * HOUR_MS;
/// How long a delivered story stays in the seen-set.
pub const SEEN_TTL_MS: i64 = 7 * 24 * HOUR_MS;
/// Hard bound on cached rows; the oldest fetched go first.
pub const MAX_ROWS: usize = 500;
/// Default number of stories a refresh hands back.
pub const DEFAULT_LIMIT: usize = 20;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS news_cache (
  url_key TEXT PRIMARY KEY,
  title_key TEXT NOT NULL,
  source TEXT NOT NULL,
  topic TEXT NOT NULL,
  title TEXT NOT NULL,
  url TEXT NOT NULL,
  outlet TEXT,
  outlets INTEGER NOT NULL DEFAULT 1,
  published_ms INTEGER,
  points INTEGER,
  comments INTEGER,
  snippet TEXT,
  fetched_ms INTEGER NOT NULL,
  expires_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_news_cache_expires ON news_cache(expires_ms);
CREATE TABLE IF NOT EXISTS news_seen (
  key TEXT PRIMARY KEY,
  seen_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS news_class (
  url_key TEXT PRIMARY KEY,
  category TEXT NOT NULL,
  classified_ms INTEGER NOT NULL
);
"#;

pub fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)?;
    // yap-1106-news-explain — a cache made before the `snippet` column.
    let has_snippet: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('news_cache') WHERE name = 'snippet'",
        [],
        |r| r.get(0),
    )?;
    if has_snippet == 0 {
        conn.execute_batch("ALTER TABLE news_cache ADD COLUMN snippet TEXT")?;
    }
    Ok(())
}

fn source_from_tag(tag: &str) -> Source {
    match tag {
        "hacker_news" => Source::HackerNews,
        "quotes" => Source::Quotes,
        _ => Source::GoogleNews,
    }
}

/// Delete expired stories and seen keys older than [`SEEN_TTL_MS`].
pub fn prune(conn: &Connection, now_ms: i64) -> rusqlite::Result<usize> {
    let stories = conn.execute(
        "DELETE FROM news_cache WHERE expires_ms <= ?1",
        params![now_ms],
    )?;
    let seen = conn.execute(
        "DELETE FROM news_seen WHERE seen_ms <= ?1",
        params![now_ms - SEEN_TTL_MS],
    )?;
    // Labels live as long as the story they label could be cached.
    conn.execute(
        "DELETE FROM news_class WHERE classified_ms <= ?1",
        params![now_ms - TTL_MS],
    )?;
    Ok(stories + seen)
}

/// Unexpired cached stories, oldest fetched first.
pub fn cached(conn: &Connection, now_ms: i64) -> rusqlite::Result<Vec<Story>> {
    let mut stmt = conn.prepare(
        "SELECT source, topic, title, url, outlet, outlets, published_ms, points, comments, snippet
         FROM news_cache WHERE expires_ms > ?1 ORDER BY fetched_ms ASC, rowid ASC",
    )?;
    let rows = stmt.query_map(params![now_ms], |r| {
        let source: String = r.get(0)?;
        let outlets: i64 = r.get(5)?;
        Ok(Story {
            source: source_from_tag(&source),
            topic: r.get(1)?,
            title: r.get(2)?,
            url: r.get(3)?,
            outlet: r.get(4)?,
            outlets: outlets.max(1) as usize,
            published_ms: r.get(6)?,
            points: r.get(7)?,
            comments: r.get(8)?,
            snippet: r.get(9)?,
        })
    })?;
    rows.collect()
}

/// Every key in the seen-set.
pub fn seen_keys(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT key FROM news_seen")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

fn is_seen(seen: &HashSet<String>, k: &Keyed) -> bool {
    seen.contains(&k.url_key) || (!k.title_key.is_empty() && seen.contains(&k.title_key))
}

/// Remember delivered stories (both keys) so later refreshes skip them.
pub fn mark_seen(
    conn: &Connection,
    stories: &[RankedStory],
    now_ms: i64,
) -> rusqlite::Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut n = 0usize;
    {
        let mut stmt =
            tx.prepare("INSERT OR REPLACE INTO news_seen (key, seen_ms) VALUES (?1, ?2)")?;
        for s in stories {
            for key in [&s.url_key, &s.title_key] {
                if !key.is_empty() {
                    n += stmt.execute(params![key, now_ms])?;
                }
            }
        }
    }
    tx.commit()?;
    log::info!("news seen_marked count={}", stories.len());
    Ok(n)
}

/// The ranked view of the cache: unexpired, not seen, deduped, on topic.
pub fn ranked(
    conn: &Connection,
    prefs: &NewsPrefs,
    now_ms: i64,
    limit: usize,
) -> rusqlite::Result<(Vec<RankedStory>, usize)> {
    let seen = seen_keys(conn)?;
    let items: Vec<Keyed> = cached(conn, now_ms)?
        .into_iter()
        .map(rank::keyed)
        .filter(|k| !is_seen(&seen, k))
        .collect();
    let (kept, _) = rank::dedupe(items);
    let labels = cloud_labels(conn)?;
    Ok(rank::rank_with(kept, prefs, now_ms, limit, &labels))
}

/// yap-1107-news-no-off-topic-stories — every cached cloud label, by url key.
pub fn cloud_labels(conn: &Connection) -> rusqlite::Result<Labels> {
    let mut stmt = conn.prepare("SELECT url_key, category FROM news_class")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out: Labels = HashMap::new();
    for row in rows {
        let (key, tag) = row?;
        if let Some(c) = Category::parse(&tag) {
            out.insert(key, c);
        }
    }
    Ok(out)
}

/// One fetched story waiting for a cloud label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub url_key: String,
    pub title: String,
    pub outlet: Option<String>,
}

/// What one classification batch did — the `news classify` record.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifyReport {
    /// Fetched stories with no declared source category.
    pub candidates: usize,
    /// Of those, already labelled (no request for them).
    pub cached: usize,
    pub sent: usize,
    /// Cloud calls made: 0 or 1 per refresh.
    pub calls: usize,
    pub labelled: usize,
    pub fallback: Option<String>,
    pub ms: u64,
}

/// The stories the cloud should label: undeclared Google News stories not
/// labelled yet, one per url key, at most [`sources::CLASSIFY_MAX`].
pub fn classify_pending(
    conn: &Connection,
    stories: &[Story],
) -> rusqlite::Result<(Vec<Pending>, ClassifyReport)> {
    let labels = cloud_labels(conn)?;
    let mut report = ClassifyReport::default();
    let mut keys = HashSet::new();
    let mut pending = Vec::new();
    for s in stories {
        if sources::declared_category(s.source, s.outlet.as_deref()).is_some() {
            continue;
        }
        let key = rank::url_key(&s.url);
        if !keys.insert(key.clone()) {
            continue;
        }
        report.candidates += 1;
        if labels.contains_key(&key) {
            report.cached += 1;
        } else if pending.len() < sources::CLASSIFY_MAX {
            pending.push(Pending {
                url_key: key,
                title: s.title.clone(),
                outlet: s.outlet.clone(),
            });
        }
    }
    Ok((pending, report))
}

/// The ONE cloud call of a batch (none when nothing is pending). A failed
/// call labels nothing and the keywords decide; its tag is the fallback.
pub fn ask_cloud(
    pending: &[Pending],
    report: &mut ClassifyReport,
    call: &mut dyn FnMut(&str, &str) -> Result<String, &'static str>,
) -> Vec<Option<Category>> {
    if pending.is_empty() {
        return Vec::new();
    }
    let items: Vec<(String, Option<String>)> = pending
        .iter()
        .map(|p| (p.title.clone(), p.outlet.clone()))
        .collect();
    let (system, user) = sources::classify_prompt(&items);
    report.sent = pending.len();
    report.calls = 1;
    match call(&system, &user) {
        Ok(text) => sources::parse_labels(&text, pending.len()),
        Err(tag) => {
            report.fallback = Some(tag.to_string());
            vec![None; pending.len()]
        }
    }
}

/// Write the labels the cloud gave. Returns how many were written.
pub fn store_labels(
    conn: &Connection,
    pending: &[Pending],
    labels: &[Option<Category>],
    now_ms: i64,
) -> rusqlite::Result<usize> {
    let mut n = 0usize;
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO news_class (url_key, category, classified_ms)
             VALUES (?1, ?2, ?3)",
        )?;
        for (p, label) in pending.iter().zip(labels.iter()) {
            if let Some(c) = label {
                n += stmt.execute(params![p.url_key, c.tag(), now_ms])?;
            }
        }
    }
    tx.commit()?;
    Ok(n)
}

pub fn log_classify(report: &ClassifyReport) {
    log::info!(
        "news classify candidates={} cached={} sent={} labelled={} calls={} ms={} fallback={}",
        report.candidates,
        report.cached,
        report.sent,
        report.labelled,
        report.calls,
        report.ms,
        report.fallback.as_deref().unwrap_or("none")
    );
}

/// Pending -> one call -> stored labels, on one connection (tests; the app
/// splits it so no lock is held during the call, see `news::classify_cloud`).
pub fn classify_batch(
    conn: &Connection,
    stories: &[Story],
    now_ms: i64,
    call: &mut dyn FnMut(&str, &str) -> Result<String, &'static str>,
) -> rusqlite::Result<ClassifyReport> {
    let t0 = Instant::now();
    let (pending, mut report) = classify_pending(conn, stories)?;
    let labels = ask_cloud(&pending, &mut report, call);
    report.labelled = store_labels(conn, &pending, &labels, now_ms)?;
    report.ms = t0.elapsed().as_millis() as u64;
    if report.candidates > 0 {
        log_classify(&report);
    }
    Ok(report)
}

/// What one [`ingest`] did — the `news rank` telemetry record.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestReport {
    /// Stories the feeds returned.
    pub fetched: usize,
    /// New rows written to the cache.
    pub added: usize,
    pub dup_url: usize,
    pub dup_title: usize,
    /// Fetched stories skipped because they were already delivered.
    pub seen: usize,
    pub off_topic: usize,
    /// Rows pruned for age.
    pub expired: usize,
    /// The ranked stories, best first.
    pub stories: Vec<RankedStory>,
}

/// Fold a refresh into the cache and return the ranked stories. Prunes
/// expired rows, skips seen stories, dedupes the new ones against each other
/// AND against what is cached (the cached copy wins), writes the survivors
/// with a 24 h expiry, bounds the table at [`MAX_ROWS`], then ranks.
pub fn ingest(
    conn: &Connection,
    fresh: &[Story],
    prefs: &NewsPrefs,
    now_ms: i64,
    limit: usize,
) -> rusqlite::Result<IngestReport> {
    let t0 = Instant::now();
    let mut report = IngestReport {
        fetched: fresh.len(),
        ..IngestReport::default()
    };
    report.expired = prune(conn, now_ms)?;
    let seen = seen_keys(conn)?;
    let old: Vec<Keyed> = cached(conn, now_ms)?.into_iter().map(rank::keyed).collect();
    let old_keys: HashSet<String> = old.iter().map(|k| k.url_key.clone()).collect();
    let mut all = old;
    for s in fresh {
        let k = rank::keyed(s.clone());
        if is_seen(&seen, &k) {
            report.seen += 1;
        } else {
            all.push(k);
        }
    }
    let (kept, DedupeCounts { dup_url, dup_title }) = rank::dedupe(all);
    report.dup_url = dup_url;
    report.dup_title = dup_title;

    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO news_cache
             (url_key, title_key, source, topic, title, url, outlet, outlets,
              published_ms, points, comments, fetched_ms, expires_ms, snippet)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        )?;
        for k in kept.iter().filter(|k| !old_keys.contains(&k.url_key)) {
            let s = &k.story;
            report.added += stmt.execute(params![
                k.url_key,
                k.title_key,
                s.source.tag(),
                s.topic,
                s.title,
                s.url,
                s.outlet,
                s.outlets as i64,
                s.published_ms,
                s.points,
                s.comments,
                now_ms,
                now_ms + TTL_MS,
                s.snippet,
            ])?;
        }
        tx.execute(
            "DELETE FROM news_cache WHERE url_key NOT IN
             (SELECT url_key FROM news_cache ORDER BY fetched_ms DESC, rowid DESC LIMIT ?1)",
            params![MAX_ROWS as i64],
        )?;
    }
    tx.commit()?;

    let (stories, off_topic) = ranked(conn, prefs, now_ms, limit)?;
    report.off_topic = off_topic;
    report.stories = stories;
    log::info!(
        "news rank fetched={} added={} dup_url={} dup_title={} seen={} off_topic={} expired={} kept={} ms={}",
        report.fetched,
        report.added,
        report.dup_url,
        report.dup_title,
        report.seen,
        report.off_topic,
        report.expired,
        report.stories.len(),
        t0.elapsed().as_millis()
    );
    Ok(report)
}

/// Delete every cached story and the seen-set. Returns rows removed.
pub fn clear(conn: &Connection, reason: &str) -> rusqlite::Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let stories = tx.execute("DELETE FROM news_cache", [])?;
    let seen = tx.execute("DELETE FROM news_seen", [])?;
    tx.execute("DELETE FROM news_class", [])?;
    tx.commit()?;
    log::info!("news cache_cleared reason={reason} rows={}", stories + seen);
    Ok(stories + seen)
}

/// After a prefs save: the user's switch off deletes the cache. Returns rows
/// removed (0 when news stays on).
pub fn apply_prefs(conn: &Connection, prefs: &NewsPrefs) -> rusqlite::Result<usize> {
    if prefs.enabled {
        return Ok(0);
    }
    clear(conn, "feature_off")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::news::rank::tests::{prefs, story, HOUR, NOW};

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        ensure_schema(&c).unwrap();
        c
    }

    fn count(c: &Connection, table: &str) -> i64 {
        c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn ingest_caches_with_a_24h_ttl() {
        let c = db();
        let p = prefs(&["tech"], &[], &[]);
        let r = ingest(
            &c,
            &[story(
                "tech",
                "Chip maker posts record quarter",
                "https://a.com/1",
                None,
                Some(1),
            )],
            &p,
            NOW,
            10,
        )
        .unwrap();
        assert_eq!(r.added, 1);
        assert_eq!(r.stories.len(), 1);
        let expires: i64 = c
            .query_row("SELECT expires_ms - fetched_ms FROM news_cache", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(expires, TTL_MS);
        assert_eq!(TTL_MS, 24 * HOUR);
        // Still served just before 24 h, gone (and pruned) just after.
        assert_eq!(cached(&c, NOW + TTL_MS - 1).unwrap().len(), 1);
        assert!(cached(&c, NOW + TTL_MS).unwrap().is_empty());
        let later = ingest(&c, &[], &p, NOW + TTL_MS + 1, 10).unwrap();
        assert_eq!(later.expired, 1);
        assert_eq!(count(&c, "news_cache"), 0);
    }

    #[test]
    fn ingest_dedupes_by_url_and_title_against_the_cache() {
        let c = db();
        // "Rocket lands on a barge" is Science by its own words: the search
        // stamp "tech" no longer decides, so Science is picked too.
        let p = prefs(&["tech", "science"], &[], &[]);
        ingest(
            &c,
            &[story(
                "tech",
                "Chip maker posts record quarter on demand",
                "https://a.com/1",
                None,
                Some(1),
            )],
            &p,
            NOW,
            10,
        )
        .unwrap();
        let r = ingest(
            &c,
            &[
                story(
                    "tech",
                    "Something else",
                    "https://www.a.com/1/?utm_source=rss",
                    None,
                    Some(1),
                ),
                story(
                    "tech",
                    "Chip maker posts a record quarter on demand - Reuters",
                    "https://reuters.com/9",
                    Some("Reuters"),
                    Some(1),
                ),
                story(
                    "tech",
                    "Rocket lands on a barge again",
                    "https://b.com/2",
                    None,
                    Some(2),
                ),
            ],
            &p,
            NOW + HOUR,
            10,
        )
        .unwrap();
        assert_eq!((r.dup_url, r.dup_title, r.added), (1, 1, 1));
        assert_eq!(count(&c, "news_cache"), 2);
        assert_eq!(r.stories.len(), 2);
    }

    #[test]
    fn seen_set_skips_repeats() {
        let c = db();
        let p = prefs(&["tech"], &[], &[]);
        let first = ingest(
            &c,
            &[story(
                "tech",
                "Chip maker posts record quarter on demand",
                "https://a.com/1",
                None,
                Some(1),
            )],
            &p,
            NOW,
            10,
        )
        .unwrap();
        mark_seen(&c, &first.stories, NOW).unwrap();
        // Delivered: no longer in the ranked view even though it is cached.
        assert!(ranked(&c, &p, NOW, 10).unwrap().0.is_empty());
        // The same link and the same headline from another outlet are skipped.
        let again = ingest(
            &c,
            &[
                story(
                    "tech",
                    "Chip maker posts record quarter on demand",
                    "https://a.com/1?utm_campaign=x",
                    None,
                    Some(1),
                ),
                story(
                    "tech",
                    "Chip maker posts record quarter on demand - AP",
                    "https://apnews.com/7",
                    Some("AP"),
                    Some(1),
                ),
                story(
                    "tech",
                    "A brand new gadget launches",
                    "https://b.com/3",
                    None,
                    Some(1),
                ),
            ],
            &p,
            NOW + HOUR,
            10,
        )
        .unwrap();
        assert_eq!(again.seen, 2);
        assert_eq!(again.stories.len(), 1);
        assert_eq!(again.stories[0].story.url, "https://b.com/3");
        // The seen-set itself expires after SEEN_TTL_MS.
        prune(&c, NOW + SEEN_TTL_MS).unwrap();
        assert_eq!(count(&c, "news_seen"), 0);
    }

    #[test]
    fn ranked_scores_topic_match_and_freshness() {
        let c = db();
        let p = prefs(&["tech"], &[], &["fusion"]);
        let r = ingest(
            &c,
            &[
                story(
                    "tech",
                    "Old gadget review roundup",
                    "https://a.com/old",
                    None,
                    Some(20),
                ),
                story(
                    "tech",
                    "New gadget review roundup today",
                    "https://a.com/new",
                    None,
                    Some(0),
                ),
                story(
                    "sports",
                    "Cup final tonight",
                    "https://a.com/sport",
                    None,
                    Some(0),
                ),
            ],
            &p,
            NOW,
            10,
        )
        .unwrap();
        assert_eq!(r.off_topic, 1);
        let urls: Vec<&str> = r.stories.iter().map(|s| s.story.url.as_str()).collect();
        assert_eq!(urls, vec!["https://a.com/new", "https://a.com/old"]);
        assert!(r.stories[0].score > r.stories[1].score);
    }

    #[test]
    fn turning_the_feature_off_deletes_the_cache() {
        let c = db();
        let mut p = prefs(&["tech"], &[], &[]);
        let r = ingest(
            &c,
            &[
                story(
                    "tech",
                    "Chip maker posts record quarter",
                    "https://a.com/1",
                    None,
                    Some(1),
                ),
                story(
                    "tech",
                    "Rocket lands on a barge again",
                    "https://a.com/2",
                    None,
                    Some(1),
                ),
            ],
            &p,
            NOW,
            10,
        )
        .unwrap();
        mark_seen(&c, &r.stories[..1], NOW).unwrap();
        assert_eq!(apply_prefs(&c, &p).unwrap(), 0, "news on keeps the cache");
        assert_eq!(count(&c, "news_cache"), 2);
        p.enabled = false;
        assert_eq!(apply_prefs(&c, &p).unwrap(), 4);
        assert_eq!(count(&c, "news_cache"), 0);
        assert_eq!(count(&c, "news_seen"), 0);
    }

    #[test]
    fn database_open_creates_the_cache_and_off_clears_it() {
        let db = crate::db::Database::open_in_memory().unwrap();
        let mut p = prefs(&["tech"], &[], &[]);
        let added = db
            .with_news_cache(|c| {
                ingest(
                    c,
                    &[story(
                        "tech",
                        "Chip maker posts record quarter",
                        "https://a.com/1",
                        None,
                        Some(1),
                    )],
                    &p,
                    NOW,
                    10,
                )
                .map(|r| r.added)
            })
            .unwrap();
        assert_eq!(added, 1);
        p.enabled = false;
        assert_eq!(db.with_news_cache(|c| apply_prefs(c, &p)).unwrap(), 1);
        assert_eq!(
            db.with_news_cache(|c| Ok(count(c, "news_cache"))).unwrap(),
            0
        );
    }

    #[test]
    fn one_cloud_call_per_batch_and_none_for_cached_urls() {
        use crate::news::sources::ClassifiedBy;
        use std::cell::Cell;
        let c = db();
        // Distinct headlines (no dedupe), none declared by their outlet.
        let titles = [
            "Quiet morning across the valley",
            "Officials review harbor expansion plan",
            "Crowds gather downtown for parade",
            "Late vote delays county budget",
            "Bakery owner marks fifty years",
        ];
        let mut all: Vec<Story> = titles
            .iter()
            .enumerate()
            .map(|(i, t)| {
                story(
                    "tech",
                    t,
                    &format!("https://news.example/{i}"),
                    Some("Reuters"),
                    Some(1),
                )
            })
            .collect();
        // A declared outlet never goes to the cloud.
        all.push(story(
            "tech",
            "Putt drops on the last hole",
            "https://pgatour.com/1",
            Some("PGA TOUR"),
            Some(1),
        ));
        let calls = Cell::new(0usize);
        let lines = Cell::new(0usize);
        let mut call = |_system: &str, user: &str| -> Result<String, &'static str> {
            calls.set(calls.get() + 1);
            lines.set(user.lines().count());
            Ok("1. tech\n2. sports\n3. business\n4. world\n5. other".to_string())
        };
        let first = classify_batch(&c, &all, NOW, &mut call).unwrap();
        assert_eq!(calls.get(), 1, "N stories => one call");
        assert_eq!(lines.get(), 5, "only undeclared stories are sent");
        assert_eq!(
            (
                first.candidates,
                first.cached,
                first.sent,
                first.calls,
                first.labelled
            ),
            (5, 0, 5, 1, 5)
        );
        assert_eq!(count(&c, "news_class"), 5);

        let second = classify_batch(&c, &all, NOW + HOUR, &mut call).unwrap();
        assert_eq!(calls.get(), 1, "second refresh => no call for cached urls");
        assert_eq!((second.cached, second.calls, second.sent), (5, 0, 0));

        // The cached label decides: only the story labelled tech is kept.
        let r = ingest(&c, &all, &prefs(&["tech"], &[], &[]), NOW, 10).unwrap();
        assert_eq!(r.stories.len(), 1);
        assert_eq!(r.stories[0].story.url, "https://news.example/0");
        assert_eq!(r.stories[0].classified_by, ClassifiedBy::Cloud);
        assert_eq!(r.off_topic, 5);

        // Feature off deletes the labels too.
        clear(&c, "test").unwrap();
        assert_eq!(count(&c, "news_class"), 0);
    }

    #[test]
    fn a_failed_cloud_call_falls_through_to_the_keywords() {
        let c = db();
        let s = vec![story(
            "world",
            "Nvidia unveils new chip",
            "https://news.example/chip",
            Some("Reuters"),
            Some(1),
        )];
        let mut call = |_: &str, _: &str| -> Result<String, &'static str> { Err("offline") };
        let r = classify_batch(&c, &s, NOW, &mut call).unwrap();
        assert_eq!(r.fallback.as_deref(), Some("offline"));
        assert_eq!((r.calls, r.labelled), (1, 0));
        let out = ingest(&c, &s, &prefs(&["tech"], &[], &[]), NOW, 10).unwrap();
        assert_eq!(out.stories.len(), 1);
        // Found by the "world" search, shown as Tech: its own category.
        assert_eq!(out.stories[0].story.topic, "tech");
    }
}
