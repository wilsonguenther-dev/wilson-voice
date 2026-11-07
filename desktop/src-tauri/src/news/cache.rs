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
//! Telemetry: `news rank fetched=… added=… dup_url=… dup_title=… seen=…
//! off_topic=… expired=… kept=… ms=…` per ingest, `news seen_marked count=…`,
//! `news cache_cleared reason=… rows=…`. No titles or URLs are ever logged.

use std::collections::HashSet;
use std::time::Instant;

use rusqlite::{params, Connection};
use serde::Serialize;

use super::feeds::{Source, Story};
use super::rank::{self, DedupeCounts, Keyed, RankedStory};
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
    Ok(rank::rank(kept, prefs, now_ms, limit))
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
        let p = prefs(&["tech"], &[], &[]);
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
}
