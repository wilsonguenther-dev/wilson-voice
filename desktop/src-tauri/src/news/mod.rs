//! Yappy News, the fetch side. What the user asked for lives in
//! [`crate::news_prefs`]; this module turns those prefs into requests to
//! public feeds ([`feeds`]), then dedupes, ranks and caches the stories on
//! this Mac ([`rank`], [`cache`]). Nothing here runs unless the cloud option
//! is on and at least one topic is set ([`feeds::gate`]).

pub mod cache;
pub mod explain;
pub mod feeds;
pub mod rank;
/// yap-1106-news-refresh-setting — when a refresh may run.
pub mod schedule;

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
/// yap-1106-fb-14-news-quote-key — the optional own quote-feed key (Keychain).
pub mod quote_key;

/// yap-1106-news-refresh-setting — what one scheduled refresh did, for the
/// `news_refresh` command (Settings' "Refresh now" and Home's scheduler).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshOutcome {
    /// A fetch went out.
    pub ran: bool,
    /// Why not (`floor`, `meeting`, `screen_share`, `dnd`, `low_battery`,
    /// `not_this_trigger`, `not_due`), when it did not.
    pub skipped: Option<&'static str>,
    /// One plain sentence for a tap that did not run.
    pub message: Option<&'static str>,
    pub stories: Vec<rank::RankedStory>,
    pub quotes: Vec<feeds::Quote>,
    /// The fresh Settings view (it carries the last refresh time).
    pub view: crate::news_prefs::NewsView,
}

/// yap-1106-news-refresh-setting — refresh only when the user's "How often"
/// ([`crate::news_prefs::load_cadence`]), the 30-minute floor and the quiet
/// rules all allow it ([`schedule::decide`]). Quotes ride along with the same
/// refresh, so market quote checks follow the same setting. Blocking: run it
/// on a blocking thread. `Err` is the gate's reason tag or a storage error.
pub fn refresh_if_due(
    db: &crate::db::Database,
    quote: &feeds::QuoteFeed,
    trigger: schedule::Trigger,
    quiet: schedule::Quiet,
    now_ms: i64,
    utc_offset_ms: i64,
    limit: usize,
) -> Result<RefreshOutcome, String> {
    let cadence = crate::news_prefs::load_cadence(db);
    let before = crate::news_prefs::view(db, now_ms);
    let decision = schedule::decide(
        cadence,
        schedule::Kind::Stories,
        trigger,
        before.last_refresh_ms,
        now_ms,
        utc_offset_ms,
        quiet,
    );
    if let schedule::Decision::Skip(why) = decision {
        log::info!(
            "news refresh_skipped trigger={} cadence={} reason={}",
            trigger.as_str(),
            cadence.as_str(),
            why.tag()
        );
        return Ok(RefreshOutcome {
            ran: false,
            skipped: Some(why.tag()),
            message: Some(why.plain()),
            stories: Vec::new(),
            quotes: Vec::new(),
            view: before,
        });
    }
    let started = std::time::Instant::now();
    let prefs = crate::news_prefs::load(db);
    let report = feeds::refresh_now_with(db, quote).map_err(|c| {
        log::info!(
            "news refresh_closed trigger={} cadence={} reason={}",
            trigger.as_str(),
            cadence.as_str(),
            c.tag()
        );
        c.tag().to_string()
    })?;
    let ingest = db
        .with_news_cache(|c| cache::ingest(c, &report.stories, &prefs, now_ms, limit))
        .inspect_err(|e| log::warn!("news cache_failed op=ingest error=\"{e}\""))?;
    if let Err(e) = crate::news_prefs::record_refresh(db, now_ms) {
        log::warn!("news refresh_stamp_failed error=\"{e}\"");
    }
    log::info!(
        "news refresh_done trigger={} cadence={} ms={} requests={} stories={} quotes={}",
        trigger.as_str(),
        cadence.as_str(),
        started.elapsed().as_millis(),
        report.requests,
        ingest.stories.len(),
        report.quotes.len()
    );
    Ok(RefreshOutcome {
        ran: true,
        skipped: None,
        message: None,
        stories: ingest.stories,
        quotes: report.quotes,
        view: crate::news_prefs::view(db, now_ms),
    })
}
