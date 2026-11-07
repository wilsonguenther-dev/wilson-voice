//! Yappy News, the fetch side. What the user asked for lives in
//! [`crate::news_prefs`]; this module turns those prefs into requests to
//! public feeds ([`feeds`]), then dedupes, ranks and caches the stories on
//! this Mac ([`rank`], [`cache`]). Nothing here runs unless the cloud option
//! is on and at least one topic is set ([`feeds::gate`]).

pub mod cache;
pub mod feeds;
pub mod rank;

/// yap-1106-news-rank-cache — one refresh end to end: fetch (gated), fold
/// into the cache, return the ranked stories. Blocking: run it on a blocking
/// thread. `Err` is the gate's reason tag (`cloud_off`, `disabled`,
/// `no_topics`) or a storage error.
pub fn refresh_ranked(
    db: &crate::db::Database,
    quote_key: Option<&str>,
    limit: usize,
) -> Result<(feeds::FetchReport, cache::IngestReport), String> {
    let prefs = crate::news_prefs::load(db);
    let report = feeds::refresh_now(db, quote_key).map_err(|c| c.tag().to_string())?;
    let now = crate::news_prefs::now_ms();
    let ingest = db
        .with_news_cache(|c| cache::ingest(c, &report.stories, &prefs, now, limit))
        .inspect_err(|e| log::warn!("news cache_failed op=ingest error=\"{e}\""))?;
    Ok((report, ingest))
}
/// yap-1107-news-honours-cloud-key — the connect refresh end to end: the
/// first time the gate opens (a cloud key saved or the mode moved to the
/// cloud, with a topic set) fetch once and fold the stories into the cache.
/// `None` = nothing owed ([`feeds::refresh_on_connect`]). Blocking: run it on
/// a blocking thread.
pub fn refresh_on_connect_ranked(
    db: &crate::db::Database,
    quote: &feeds::QuoteFeed,
    limit: usize,
) -> Option<Result<(feeds::FetchReport, cache::IngestReport), String>> {
    let prefs = crate::news_prefs::load(db);
    let report = match feeds::refresh_on_connect_now(db, quote)? {
        Ok(r) => r,
        Err(c) => return Some(Err(c.tag().to_string())),
    };
    let now = crate::news_prefs::now_ms();
    Some(
        db.with_news_cache(|c| cache::ingest(c, &report.stories, &prefs, now, limit))
            .inspect_err(|e| log::warn!("news cache_failed op=ingest error=\"{e}\""))
            .map(|ingest| (report, ingest)),
    )
}

/// yap-1106-fb-14-news-quote-key — the optional own quote-feed key (Keychain).
pub mod quote_key;
