//! yap-1106-news-home-briefing — the Home briefing's backend half.
//!
//! The story refresh itself is [`super::refresh_if_due`] (the "first open
//! after 6 AM and after 4 PM" slots live in [`super::schedule`]). This module
//! adds what only the briefing needs:
//!
//! * the market quote check: while Home is open in US market hours, the
//!   followed tickers are checked every 30 minutes ([`quote_check`]), and a
//!   ticker only earns a line when it moves 3% or more ([`movers`]); every
//!   quote is marked delayed;
//! * nothing is fetched during a meeting, a screen share, Do Not Disturb or
//!   low battery (the same [`super::schedule::Quiet`] rules);
//! * the story link opener only opens plain web links ([`openable_url`]).
//!
//! News lives on Home only: nothing in this module talks to the pill.

use std::sync::atomic::{AtomicI64, Ordering};

use serde::Serialize;

use super::feeds::{self, Quote, QuoteFeed};
use super::schedule::{self, Cadence, Decision, Kind, Quiet, Trigger};

const MINUTE_MS: i64 = 60 * 1000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;

/// A ticker earns a line only when it moves this much (news design page,
/// section 3: "a line only if a ticker moves 3% or more").
pub const MOVE_PCT: f64 = 3.0;
/// US market hours, New York local time: 9:30 AM to 4:00 PM, Monday to Friday.
pub const MARKET_OPEN_MS: i64 = 9 * HOUR_MS + 30 * MINUTE_MS;
pub const MARKET_CLOSE_MS: i64 = 16 * HOUR_MS;
/// The longest link the opener accepts.
pub const MAX_URL_LEN: usize = 2048;

/// Days since 1970-01-01 for a proleptic Gregorian date (month 1..=12).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The year of a day number (days since 1970-01-01).
fn year_of_day(z: i64) -> i64 {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let y = yoe + era * 400;
    if mp >= 10 {
        y + 1
    } else {
        y
    }
}

/// 0 = Sunday ... 6 = Saturday (1970-01-01 was a Thursday).
pub fn weekday(day: i64) -> i64 {
    (day + 4).rem_euclid(7)
}

/// The day number of the `n`th Sunday of month `m` in year `y`.
fn nth_sunday(y: i64, m: i64, n: i64) -> i64 {
    let first = days_from_civil(y, m, 1);
    first + (7 - weekday(first)) % 7 + 7 * (n - 1)
}

/// New York's offset from UTC at `utc_ms`: -4 h from the second Sunday of
/// March (2 AM local) to the first Sunday of November (2 AM local), else -5 h.
pub fn ny_offset_ms(utc_ms: i64) -> i64 {
    let y = year_of_day(utc_ms.div_euclid(DAY_MS));
    let start = nth_sunday(y, 3, 2) * DAY_MS + 7 * HOUR_MS;
    let end = nth_sunday(y, 11, 1) * DAY_MS + 6 * HOUR_MS;
    if utc_ms >= start && utc_ms < end {
        -4 * HOUR_MS
    } else {
        -5 * HOUR_MS
    }
}

/// Is the US stock market open at `utc_ms`? Weekdays 9:30 AM to 4 PM New York
/// time. Exchange holidays are not modelled: on one, the check runs and the
/// feed returns the last close, which moves nothing.
pub fn market_open(utc_ms: i64) -> bool {
    let local = utc_ms + ny_offset_ms(utc_ms);
    let day = local.div_euclid(DAY_MS);
    let wd = weekday(day);
    if wd == 0 || wd == 6 {
        return false;
    }
    let into = local - day * DAY_MS;
    (MARKET_OPEN_MS..MARKET_CLOSE_MS).contains(&into)
}

/// The quote check's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteCheck {
    Fetch,
    /// The log tag: a quiet reason (`meeting`, `screen_share`, `dnd`,
    /// `low_battery`), `no_tickers`, `free_tier`, `market_closed`, or a
    /// schedule tag (`floor`, `not_this_trigger`, `not_due`).
    Skip(&'static str),
}

/// May the quote check run now? Quiet rules first (nothing is fetched during
/// a meeting, screen share, Do Not Disturb or low battery), then: tickers are
/// followed, a price feed is set up (the free tier makes no price request;
/// its tickers ride the story refresh as headline searches), the market is
/// open, and the user's "How often" plus the 30-minute floor allow it.
#[allow(clippy::too_many_arguments)]
pub fn quote_check(
    cadence: Cadence,
    trigger: Trigger,
    last_ms: Option<i64>,
    now_ms: i64,
    utc_offset_ms: i64,
    quiet: Quiet,
    tickers: usize,
    keyed: bool,
) -> QuoteCheck {
    if let Some(r) = quiet.reason() {
        return QuoteCheck::Skip(r);
    }
    if tickers == 0 {
        return QuoteCheck::Skip("no_tickers");
    }
    if !keyed {
        return QuoteCheck::Skip("free_tier");
    }
    if !market_open(now_ms) {
        return QuoteCheck::Skip("market_closed");
    }
    match schedule::decide(
        cadence,
        Kind::Quotes,
        trigger,
        last_ms,
        now_ms,
        utc_offset_ms,
        Quiet::default(),
    ) {
        Decision::Fetch => QuoteCheck::Fetch,
        Decision::Skip(s) => QuoteCheck::Skip(s.tag()),
    }
}

/// May the explain step make its cloud call now? fix-l3b3-2: a meeting,
/// screen share, Do Not Disturb or low battery holds it, exactly as it holds
/// the story refresh and the quote check. `Err` is the quiet reason tag
/// (`meeting`, `screen_share`, `dnd`, `low_battery`); the caller then shows
/// the headlines alone and sends nothing.
pub fn explain_allowed(quiet: Quiet) -> Result<(), &'static str> {
    match quiet.reason() {
        Some(r) => Err(r),
        None => Ok(()),
    }
}

/// The quotes worth a line: moved [`MOVE_PCT`] or more either way, biggest
/// move first. Every one is marked delayed, whatever the feed said.
pub fn movers(quotes: &[Quote]) -> Vec<Quote> {
    let mut out: Vec<Quote> = quotes
        .iter()
        .filter(|q| q.change_pct.is_finite() && q.change_pct.abs() >= MOVE_PCT)
        .cloned()
        .map(|mut q| {
            q.delayed = true;
            q
        })
        .collect();
    out.sort_by(|a, b| {
        b.change_pct
            .abs()
            .partial_cmp(&a.change_pct.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    out
}

/// When the last quote check ran in this process (`0` = never). The 30-minute
/// floor needs only this session's memory: a fresh launch may check once.
static LAST_QUOTES_MS: AtomicI64 = AtomicI64::new(0);

pub fn last_quotes_ms() -> Option<i64> {
    match LAST_QUOTES_MS.load(Ordering::Relaxed) {
        0 => None,
        v => Some(v),
    }
}

fn record_quotes(now_ms: i64) {
    LAST_QUOTES_MS.store(now_ms, Ordering::Relaxed);
}

/// What one quote check did, for the `news_quotes_check` command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotesOutcome {
    pub ran: bool,
    pub skipped: Option<&'static str>,
    /// Only the tickers that moved 3% or more, each marked delayed.
    pub movers: Vec<Quote>,
    /// How many tickers were checked.
    pub checked: usize,
}

/// The quote check end to end: decide, fetch the followed tickers' prices,
/// keep the movers. Blocking: run it on a blocking thread. `Err` is the news
/// gate's reason tag (`cloud_off`, `disabled`, `no_topics`).
pub fn check_quotes_if_due(
    db: &crate::db::Database,
    quote: &QuoteFeed,
    trigger: Trigger,
    quiet: Quiet,
    now_ms: i64,
    utc_offset_ms: i64,
) -> Result<QuotesOutcome, String> {
    let prefs = feeds::gate(db).map_err(|c| c.tag().to_string())?;
    let cadence = crate::news_prefs::load_cadence(db);
    let verdict = quote_check(
        cadence,
        trigger,
        last_quotes_ms(),
        now_ms,
        utc_offset_ms,
        quiet,
        prefs.tickers.len(),
        quote.host().is_some(),
    );
    if let QuoteCheck::Skip(why) = verdict {
        log::info!(
            "news quotes_skipped trigger={} cadence={} reason={why}",
            trigger.as_str(),
            cadence.as_str()
        );
        return Ok(QuotesOutcome {
            ran: false,
            skipped: Some(why),
            movers: Vec::new(),
            checked: 0,
        });
    }
    let started = std::time::Instant::now();
    record_quotes(now_ms);
    let quotes = feeds::refresh_quotes_now(db, quote).map_err(|c| c.tag().to_string())?;
    let moved = movers(&quotes);
    log::info!(
        "news quotes_done trigger={} tier={} ms={} tickers={} quotes={} movers={}",
        trigger.as_str(),
        quote.tier(),
        started.elapsed().as_millis(),
        prefs.tickers.len(),
        quotes.len(),
        moved.len()
    );
    Ok(QuotesOutcome {
        ran: true,
        skipped: None,
        movers: moved,
        checked: quotes.len(),
    })
}

/// Only a plain web link may be handed to the system opener: `http(s)://`,
/// no spaces or control characters, not absurdly long.
pub fn openable_url(url: &str) -> bool {
    let u = url.trim();
    let lower = u.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && u.len() > "https://".len()
        && u.len() <= MAX_URL_LEN
        && !u.chars().any(|c| c.is_whitespace() || c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2025-11-07 00:00 UTC, a Friday (the schedule tests' fake clock).
    const DAY0: i64 = 1_762_473_600_000;
    const CALM: Quiet = Quiet {
        in_meeting: false,
        screen_sharing: false,
        do_not_disturb: false,
        low_battery: false,
    };

    fn q(sym: &str, pct: f64) -> Quote {
        Quote {
            symbol: sym.to_string(),
            price: 100.0,
            change_pct: pct,
            prev_close: 100.0,
            as_of_ms: None,
            delayed: false,
        }
    }

    /// Friday 2025-11-07, 10:00 AM New York (15:00 UTC, standard time).
    const OPEN: i64 = DAY0 + 15 * HOUR_MS;

    #[test]
    fn civil_days_anchor_on_the_fake_clock() {
        assert_eq!(days_from_civil(2025, 11, 7) * DAY_MS, DAY0);
        assert_eq!(weekday(days_from_civil(2025, 11, 7)), 5);
        assert_eq!(year_of_day(days_from_civil(2025, 12, 31)), 2025);
        assert_eq!(year_of_day(days_from_civil(2026, 1, 1)), 2026);
        assert_eq!(year_of_day(days_from_civil(2024, 2, 29)), 2024);
    }

    #[test]
    fn new_york_switches_to_daylight_time_on_the_second_sunday_of_march() {
        // 2025: Sunday March 9 at 2 AM local = 07:00 UTC.
        let start = days_from_civil(2025, 3, 9) * DAY_MS + 7 * HOUR_MS;
        assert_eq!(ny_offset_ms(start - 1), -5 * HOUR_MS);
        assert_eq!(ny_offset_ms(start), -4 * HOUR_MS);
        // ... and back on the first Sunday of November (Nov 2, 06:00 UTC).
        let end = days_from_civil(2025, 11, 2) * DAY_MS + 6 * HOUR_MS;
        assert_eq!(ny_offset_ms(end - 1), -4 * HOUR_MS);
        assert_eq!(ny_offset_ms(end), -5 * HOUR_MS);
    }

    #[test]
    fn market_hours_are_weekdays_nine_thirty_to_four_new_york_time() {
        assert!(market_open(OPEN));
        assert!(!market_open(DAY0 + 14 * HOUR_MS)); // 9:00 AM
        assert!(market_open(DAY0 + 14 * HOUR_MS + 30 * MINUTE_MS)); // 9:30 AM
        assert!(!market_open(DAY0 + 21 * HOUR_MS)); // 4:00 PM
        assert!(!market_open(OPEN + DAY_MS)); // Saturday
        assert!(!market_open(OPEN + 2 * DAY_MS)); // Sunday
        assert!(market_open(OPEN + 3 * DAY_MS)); // Monday
                                                 // Summer: Tuesday 2025-07-01, 10:00 AM EDT = 14:00 UTC.
        let july = days_from_civil(2025, 7, 1) * DAY_MS;
        assert!(market_open(july + 14 * HOUR_MS));
        assert!(!market_open(july + 13 * HOUR_MS));
    }

    #[test]
    fn home_briefing_quotes_every_30_minutes_while_home_is_open_in_market_hours() {
        let check = |last: Option<i64>, now: i64| {
            quote_check(
                Cadence::TwiceADay,
                Trigger::HomeTick,
                last,
                now,
                0,
                CALM,
                2,
                true,
            )
        };
        assert_eq!(check(None, OPEN), QuoteCheck::Fetch);
        assert_eq!(
            check(Some(OPEN), OPEN + 29 * MINUTE_MS),
            QuoteCheck::Skip("floor")
        );
        assert_eq!(check(Some(OPEN), OPEN + 30 * MINUTE_MS), QuoteCheck::Fetch);
        // Outside market hours nothing is fetched.
        assert_eq!(
            check(None, DAY0 + 22 * HOUR_MS),
            QuoteCheck::Skip("market_closed")
        );
    }

    #[test]
    fn home_briefing_fetches_nothing_during_meetings_share_dnd_or_low_battery() {
        let quiet = [
            (
                Quiet {
                    in_meeting: true,
                    ..CALM
                },
                "meeting",
            ),
            (
                Quiet {
                    screen_sharing: true,
                    ..CALM
                },
                "screen_share",
            ),
            (
                Quiet {
                    do_not_disturb: true,
                    ..CALM
                },
                "dnd",
            ),
            (
                Quiet {
                    low_battery: true,
                    ..CALM
                },
                "low_battery",
            ),
        ];
        for (qt, tag) in quiet {
            let v = quote_check(
                Cadence::TwiceADay,
                Trigger::HomeTick,
                None,
                OPEN,
                0,
                qt,
                3,
                true,
            );
            assert_eq!(v, QuoteCheck::Skip(tag));
            // The story refresh obeys the same rules.
            assert_eq!(
                schedule::decide(
                    Cadence::TwiceADay,
                    Kind::Stories,
                    Trigger::HomeOpened,
                    None,
                    OPEN,
                    0,
                    qt
                ),
                Decision::Skip(schedule::Skip::Quiet(tag))
            );
        }
    }

    #[test]
    fn explain_makes_no_cloud_call_during_meetings_share_dnd_or_low_battery() {
        assert_eq!(explain_allowed(CALM), Ok(()));
        let quiet = [
            (
                Quiet {
                    in_meeting: true,
                    ..CALM
                },
                "meeting",
            ),
            (
                Quiet {
                    screen_sharing: true,
                    ..CALM
                },
                "screen_share",
            ),
            (
                Quiet {
                    do_not_disturb: true,
                    ..CALM
                },
                "dnd",
            ),
            (
                Quiet {
                    low_battery: true,
                    ..CALM
                },
                "low_battery",
            ),
        ];
        for (qt, tag) in quiet {
            assert_eq!(explain_allowed(qt), Err(tag));
        }
        // A quiet explain step is the headlines alone: the cloud call is the
        // closure, and it never runs.
        let mut called = false;
        let mut call = |_: &str, _: &str| -> Result<String, &'static str> {
            called = true;
            Ok(String::new())
        };
        fn gated(
            qt: Quiet,
            call: &mut dyn FnMut(&str, &str) -> Result<String, &'static str>,
        ) -> Result<String, &'static str> {
            explain_allowed(qt)?;
            call("system", "user")
        }
        assert_eq!(
            gated(
                Quiet {
                    in_meeting: true,
                    ..CALM
                },
                &mut call
            ),
            Err("meeting")
        );
        assert!(!called);
    }

    #[test]
    fn quote_check_needs_tickers_and_a_price_feed() {
        let v = |tickers: usize, keyed: bool| {
            quote_check(
                Cadence::TwiceADay,
                Trigger::HomeTick,
                None,
                OPEN,
                0,
                CALM,
                tickers,
                keyed,
            )
        };
        assert_eq!(v(0, true), QuoteCheck::Skip("no_tickers"));
        assert_eq!(v(2, false), QuoteCheck::Skip("free_tier"));
        // "Only when I tap refresh" never checks on its own.
        assert_eq!(
            quote_check(
                Cadence::OnTap,
                Trigger::HomeTick,
                None,
                OPEN,
                0,
                CALM,
                2,
                true
            ),
            QuoteCheck::Skip("not_this_trigger")
        );
    }

    #[test]
    fn home_briefing_refreshes_after_6am_and_after_4pm_slots() {
        let open = |last: Option<i64>, now: i64| {
            schedule::decide(
                Cadence::TwiceADay,
                Kind::Stories,
                Trigger::HomeOpened,
                last,
                now,
                0,
                CALM,
            )
        };
        let six = DAY0 + 6 * HOUR_MS;
        let four = DAY0 + 16 * HOUR_MS;
        assert_eq!(open(Some(DAY0 - HOUR_MS), six + MINUTE_MS), Decision::Fetch);
        assert_eq!(
            open(Some(six + MINUTE_MS), four - HOUR_MS),
            Decision::Skip(schedule::Skip::NotDue)
        );
        assert_eq!(
            open(Some(six + MINUTE_MS), four + MINUTE_MS),
            Decision::Fetch
        );
    }

    #[test]
    fn quotes_are_delayed_and_only_movers_of_three_percent_get_a_line() {
        let got = movers(&[
            q("AAPL", 1.2),
            q("NVDA", 3.1),
            q("TSLA", -4.0),
            q("X", f64::NAN),
        ]);
        let syms: Vec<&str> = got.iter().map(|m| m.symbol.as_str()).collect();
        assert_eq!(syms, vec!["TSLA", "NVDA"]);
        assert!(got.iter().all(|m| m.delayed));
    }

    #[test]
    fn story_opener_only_takes_plain_web_links() {
        assert!(openable_url("https://news.ycombinator.com/item?id=1"));
        assert!(openable_url("http://example.com/a"));
        assert!(!openable_url("file:///etc/passwd"));
        assert!(!openable_url("javascript:alert(1)"));
        assert!(!openable_url("-a Calculator"));
        assert!(!openable_url("https://"));
        assert!(!openable_url("https://a.com/x y"));
        assert!(!openable_url(&format!(
            "https://a.com/{}",
            "a".repeat(MAX_URL_LEN)
        )));
    }

    #[test]
    fn news_never_goes_to_the_pill() {
        // The briefing's backend half emits nothing and names no pill surface.
        let src = include_str!("briefing.rs");
        let body = src.split("#[cfg(test)]").next().unwrap_or("");
        for banned in [".emit(", "emit_to(", concat!("pill", "_")] {
            assert!(!body.contains(banned), "briefing.rs must not use {banned}");
        }
    }
}
