//! yap-1106-news-refresh-setting — WHEN Yappy News may fetch.
//!
//! One pure decision, [`decide`], for every refresh path: the user's
//! "How often" ([`Cadence`]), what asked ([`Trigger`]), the last time this
//! kind of check ran, the clock, and the quiet signals. It never does I/O,
//! so the tests drive it with a fake clock. Two rules hold whatever is
//! chosen:
//!
//! * the 30-minute floor — nothing is fetched within 30 minutes of the last
//!   fetch of the same kind, a tap included;
//! * the quiet rules — nothing is fetched during a meeting, a screen share,
//!   Do Not Disturb or low battery (the same quiet rules as the friends).
//!
//! Market quote checks ([`Kind::Quotes`]) follow the same setting. The one
//! difference is the approved default (news design page, section 3): with
//! "Twice a day", while Home is open the tickers are checked every 30 minutes.
//! yap-1106-news-home-briefing's scheduler calls [`decide`] for every tick.

use serde::Serialize;

pub use crate::news_prefs::Cadence;

const MINUTE_MS: i64 = 60 * 1000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;

/// The floor: never more often than every 30 minutes, whatever is chosen.
pub const FLOOR_MS: i64 = 30 * MINUTE_MS;
/// "Every few hours".
pub const FEW_HOURS_MS: i64 = 3 * HOUR_MS;
/// "Twice a day": the first chance after 6 AM and after 4 PM, local time.
pub const MORNING_SLOT_MS: i64 = 6 * HOUR_MS;
pub const AFTERNOON_SLOT_MS: i64 = 16 * HOUR_MS;

/// What is being refreshed. Each kind keeps its own "last ran" stamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Stories,
    Quotes,
}

/// What asked for the refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// A background timer, Home closed.
    Timer,
    /// Home was just opened.
    HomeOpened,
    /// Home is open; the periodic tick.
    HomeTick,
    /// The user tapped refresh.
    Tap,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Timer => "timer",
            Trigger::HomeOpened => "home_opened",
            Trigger::HomeTick => "home_tick",
            Trigger::Tap => "tap",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            Trigger::Timer,
            Trigger::HomeOpened,
            Trigger::HomeTick,
            Trigger::Tap,
        ]
        .into_iter()
        .find(|t| t.as_str() == s.trim())
    }

    fn automatic(self) -> bool {
        !matches!(self, Trigger::Tap)
    }
}

/// The quiet signals. Any one of them means nothing is fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Quiet {
    pub in_meeting: bool,
    pub screen_sharing: bool,
    pub do_not_disturb: bool,
    pub low_battery: bool,
}

impl Quiet {
    /// The first quiet reason, as a log tag.
    pub fn reason(&self) -> Option<&'static str> {
        if self.in_meeting {
            Some("meeting")
        } else if self.screen_sharing {
            Some("screen_share")
        } else if self.do_not_disturb {
            Some("dnd")
        } else if self.low_battery {
            Some("low_battery")
        } else {
            None
        }
    }
}

/// Why a refresh did not run. `tag` is the log value; `plain` is what a tap
/// shows the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Skip {
    /// Within 30 minutes of the last fetch.
    Floor,
    /// A meeting, screen share, Do Not Disturb or low battery.
    Quiet(&'static str),
    /// The chosen cadence does not refresh on this trigger.
    NotThisTrigger,
    /// The cadence refreshes on this trigger, just not yet.
    NotDue,
}

impl Skip {
    pub fn tag(self) -> &'static str {
        match self {
            Skip::Floor => "floor",
            Skip::Quiet(r) => r,
            Skip::NotThisTrigger => "not_this_trigger",
            Skip::NotDue => "not_due",
        }
    }

    pub fn plain(self) -> &'static str {
        match self {
            Skip::Floor => "News was refreshed less than 30 minutes ago. Try again a little later.",
            Skip::Quiet("meeting") => "News waits until your meeting is over.",
            Skip::Quiet("screen_share") => "News waits while you're sharing your screen.",
            Skip::Quiet("dnd") => "News waits while Do Not Disturb is on.",
            Skip::Quiet(_) => "News waits until your Mac has more battery.",
            Skip::NotThisTrigger | Skip::NotDue => {
                "News isn't due yet. Change how often in Settings, Yappy News."
            }
        }
    }
}

/// The decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Fetch,
    Skip(Skip),
}

/// The most recent "twice a day" slot (6 AM or 4 PM local) at or before
/// `now_ms`, in epoch ms. `utc_offset_ms` is the local offset from UTC.
pub fn last_slot_ms(now_ms: i64, utc_offset_ms: i64) -> i64 {
    let local = now_ms + utc_offset_ms;
    let day = local.div_euclid(DAY_MS) * DAY_MS;
    let into = local - day;
    let slot_local = if into >= AFTERNOON_SLOT_MS {
        day + AFTERNOON_SLOT_MS
    } else if into >= MORNING_SLOT_MS {
        day + MORNING_SLOT_MS
    } else {
        day - DAY_MS + AFTERNOON_SLOT_MS
    };
    slot_local - utc_offset_ms
}

/// May this kind of check run now?
///
/// `last_ms` is the last time this kind ran (`None` = never).
pub fn decide(
    cadence: Cadence,
    kind: Kind,
    trigger: Trigger,
    last_ms: Option<i64>,
    now_ms: i64,
    utc_offset_ms: i64,
    quiet: Quiet,
) -> Decision {
    if let Some(r) = quiet.reason() {
        return Decision::Skip(Skip::Quiet(r));
    }
    // The floor holds for every choice and every trigger, a tap included.
    if let Some(last) = last_ms {
        if now_ms - last < FLOOR_MS {
            return Decision::Skip(Skip::Floor);
        }
    }
    if !trigger.automatic() {
        return Decision::Fetch;
    }
    let due_after = |gap: i64| match last_ms {
        None => Decision::Fetch,
        Some(last) if now_ms - last >= gap => Decision::Fetch,
        Some(_) => Decision::Skip(Skip::NotDue),
    };
    match cadence {
        Cadence::OnTap => Decision::Skip(Skip::NotThisTrigger),
        Cadence::OnHomeOpen => match trigger {
            Trigger::HomeOpened => Decision::Fetch,
            _ => Decision::Skip(Skip::NotThisTrigger),
        },
        Cadence::EveryFewHours => due_after(FEW_HOURS_MS),
        Cadence::TwiceADay => {
            if kind == Kind::Quotes && trigger == Trigger::HomeTick {
                // Approved default: tickers every 30 min while Home is open
                // (the floor above already enforced the 30 minutes).
                return Decision::Fetch;
            }
            match last_ms {
                None => Decision::Fetch,
                Some(last) if last < last_slot_ms(now_ms, utc_offset_ms) => Decision::Fetch,
                Some(_) => Decision::Skip(Skip::NotDue),
            }
        }
    }
}

/// When an automatic (timer) check of `kind` is next worth asking about, in
/// epoch ms, for the scheduler's sleep. `None` = this cadence never refreshes
/// on its own (only on open or on a tap). Never earlier than the floor.
pub fn next_due_ms(
    cadence: Cadence,
    last_ms: Option<i64>,
    now_ms: i64,
    utc_offset_ms: i64,
) -> Option<i64> {
    let floor = last_ms.map(|l| l + FLOOR_MS).unwrap_or(now_ms).max(now_ms);
    match cadence {
        Cadence::OnTap | Cadence::OnHomeOpen => None,
        Cadence::EveryFewHours => Some(
            last_ms
                .map(|l| l + FEW_HOURS_MS)
                .unwrap_or(now_ms)
                .max(floor),
        ),
        Cadence::TwiceADay => {
            let Some(last) = last_ms else {
                return Some(now_ms);
            };
            if last < last_slot_ms(now_ms, utc_offset_ms) {
                return Some(floor);
            }
            // The next slot after now.
            let local = now_ms + utc_offset_ms;
            let day = local.div_euclid(DAY_MS) * DAY_MS;
            let into = local - day;
            let next_local = if into < MORNING_SLOT_MS {
                day + MORNING_SLOT_MS
            } else if into < AFTERNOON_SLOT_MS {
                day + AFTERNOON_SLOT_MS
            } else {
                day + DAY_MS + MORNING_SLOT_MS
            };
            Some((next_local - utc_offset_ms).max(floor))
        }
    }
}

/// The local offset from UTC right now, in ms (for [`decide`]).
pub fn local_utc_offset_ms() -> i64 {
    i64::from(chrono::Local::now().offset().local_minus_utc()) * 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = MINUTE_MS;
    const HOUR: i64 = HOUR_MS;
    /// A fake clock: 2025-11-07 00:00 UTC (a UTC midnight), offset 0.
    const DAY0: i64 = 1_762_473_600_000;
    const UTC: i64 = 0;
    const CALM: Quiet = Quiet {
        in_meeting: false,
        screen_sharing: false,
        do_not_disturb: false,
        low_battery: false,
    };

    fn at(h: i64, m: i64) -> i64 {
        DAY0 + h * HOUR + m * MIN
    }

    fn ask(c: Cadence, k: Kind, t: Trigger, last: Option<i64>, now: i64) -> Decision {
        decide(c, k, t, last, now, UTC, CALM)
    }

    const ALL_TRIGGERS: [Trigger; 4] = [
        Trigger::Timer,
        Trigger::HomeOpened,
        Trigger::HomeTick,
        Trigger::Tap,
    ];

    #[test]
    fn day0_is_a_utc_midnight() {
        assert_eq!(DAY0 % DAY_MS, 0);
    }

    #[test]
    fn news_refresh_never_more_often_than_every_30_minutes_whatever_is_chosen() {
        for c in Cadence::ALL {
            for k in [Kind::Stories, Kind::Quotes] {
                for t in ALL_TRIGGERS {
                    // Walk a fake clock minute by minute for two days; every
                    // fetch is stamped. No two fetches are < 30 min apart.
                    let mut last: Option<i64> = None;
                    let mut now = at(0, 0);
                    while now < at(48, 0) {
                        if ask(c, k, t, last, now) == Decision::Fetch {
                            if let Some(l) = last {
                                assert!(
                                    now - l >= FLOOR_MS,
                                    "{c:?} {k:?} {t:?} fetched {} min apart",
                                    (now - l) / MIN
                                );
                            }
                            last = Some(now);
                        }
                        now += MIN;
                    }
                    // And the floor holds 29 minutes after any fetch.
                    assert_eq!(
                        ask(c, k, t, Some(at(10, 0)), at(10, 29)),
                        Decision::Skip(Skip::Floor),
                        "{c:?} {k:?} {t:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn news_refresh_quiet_rules_hold_for_every_choice() {
        let quiets = [
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
        for c in Cadence::ALL {
            for t in ALL_TRIGGERS {
                for (q, tag) in quiets {
                    let d = decide(c, Kind::Stories, t, None, at(9, 0), UTC, q);
                    assert_eq!(d, Decision::Skip(Skip::Quiet(tag)), "{c:?} {t:?}");
                    assert_eq!(Skip::Quiet(tag).tag(), tag);
                    assert!(Skip::Quiet(tag).plain().ends_with('.'));
                }
            }
        }
    }

    #[test]
    fn news_refresh_twice_a_day_fetches_after_6_am_and_after_4_pm() {
        let c = Cadence::TwiceADay;
        // Refreshed at 7 AM: not again until 4 PM.
        let last = Some(at(7, 0));
        assert_eq!(
            ask(c, Kind::Stories, Trigger::Timer, last, at(9, 0)),
            Decision::Skip(Skip::NotDue)
        );
        assert_eq!(
            ask(c, Kind::Stories, Trigger::HomeOpened, last, at(15, 59)),
            Decision::Skip(Skip::NotDue)
        );
        assert_eq!(
            ask(c, Kind::Stories, Trigger::HomeOpened, last, at(16, 0)),
            Decision::Fetch
        );
        // Refreshed at 5 PM: next is after 6 AM tomorrow, not at midnight.
        let last = Some(at(17, 0));
        assert_eq!(
            ask(c, Kind::Stories, Trigger::Timer, last, at(23, 0)),
            Decision::Skip(Skip::NotDue)
        );
        assert_eq!(
            ask(c, Kind::Stories, Trigger::Timer, last, at(29, 59)),
            Decision::Skip(Skip::NotDue)
        );
        assert_eq!(
            ask(c, Kind::Stories, Trigger::Timer, last, at(30, 0)),
            Decision::Fetch
        );
        // Never refreshed: fetch at once.
        assert_eq!(
            ask(c, Kind::Stories, Trigger::Timer, None, at(3, 0)),
            Decision::Fetch
        );
        // Over two days of minute ticks: exactly twice a day.
        let mut last = Some(at(-1, 0));
        let mut n = 0;
        let mut now = at(0, 0);
        while now < at(48, 0) {
            if ask(c, Kind::Stories, Trigger::Timer, last, now) == Decision::Fetch {
                last = Some(now);
                n += 1;
            }
            now += MIN;
        }
        assert_eq!(n, 4);
        assert_eq!(
            next_due_ms(c, Some(at(7, 0)), at(9, 0), UTC),
            Some(at(16, 0))
        );
        assert_eq!(
            next_due_ms(c, Some(at(17, 0)), at(18, 0), UTC),
            Some(at(30, 0))
        );
    }

    #[test]
    fn news_refresh_twice_a_day_slots_follow_the_local_clock() {
        // UTC-6: 6 AM local is 12:00 UTC.
        let off = -6 * HOUR;
        assert_eq!(last_slot_ms(at(12, 0), off), at(12, 0));
        assert_eq!(last_slot_ms(at(11, 59), off), at(-2, 0)); // 4 PM local yesterday
        let d = decide(
            Cadence::TwiceADay,
            Kind::Stories,
            Trigger::Timer,
            Some(at(11, 0)),
            at(12, 0),
            off,
            CALM,
        );
        assert_eq!(d, Decision::Fetch);
    }

    #[test]
    fn news_refresh_every_few_hours_is_three_hours() {
        let c = Cadence::EveryFewHours;
        let last = Some(at(9, 0));
        assert_eq!(
            ask(c, Kind::Stories, Trigger::Timer, last, at(11, 59)),
            Decision::Skip(Skip::NotDue)
        );
        assert_eq!(
            ask(c, Kind::Stories, Trigger::Timer, last, at(12, 0)),
            Decision::Fetch
        );
        assert_eq!(
            ask(c, Kind::Stories, Trigger::HomeOpened, last, at(12, 0)),
            Decision::Fetch
        );
        assert_eq!(next_due_ms(c, last, at(10, 0), UTC), Some(at(12, 0)));
        let mut last = None;
        let mut n = 0;
        let mut now = at(0, 0);
        while now < at(24, 0) {
            if ask(c, Kind::Stories, Trigger::Timer, last, now) == Decision::Fetch {
                last = Some(now);
                n += 1;
            }
            now += MIN;
        }
        assert_eq!(n, 8);
    }

    #[test]
    fn news_refresh_only_when_i_open_home() {
        let c = Cadence::OnHomeOpen;
        let last = Some(at(9, 0));
        assert_eq!(
            ask(c, Kind::Stories, Trigger::Timer, last, at(20, 0)),
            Decision::Skip(Skip::NotThisTrigger)
        );
        assert_eq!(
            ask(c, Kind::Stories, Trigger::HomeTick, last, at(20, 0)),
            Decision::Skip(Skip::NotThisTrigger)
        );
        assert_eq!(
            ask(c, Kind::Stories, Trigger::HomeOpened, last, at(9, 20)),
            Decision::Skip(Skip::Floor)
        );
        assert_eq!(
            ask(c, Kind::Stories, Trigger::HomeOpened, last, at(9, 30)),
            Decision::Fetch
        );
        assert_eq!(next_due_ms(c, last, at(10, 0), UTC), None);
    }

    #[test]
    fn news_refresh_only_when_i_tap_refresh() {
        let c = Cadence::OnTap;
        for t in [Trigger::Timer, Trigger::HomeOpened, Trigger::HomeTick] {
            assert_eq!(
                ask(c, Kind::Stories, t, None, at(9, 0)),
                Decision::Skip(Skip::NotThisTrigger)
            );
            assert_eq!(
                ask(c, Kind::Quotes, t, None, at(9, 0)),
                Decision::Skip(Skip::NotThisTrigger)
            );
        }
        assert_eq!(
            ask(c, Kind::Stories, Trigger::Tap, None, at(9, 0)),
            Decision::Fetch
        );
        assert_eq!(
            ask(c, Kind::Stories, Trigger::Tap, Some(at(8, 45)), at(9, 0)),
            Decision::Skip(Skip::Floor)
        );
        assert_eq!(next_due_ms(c, None, at(9, 0), UTC), None);
    }

    #[test]
    fn news_refresh_market_quotes_follow_the_same_setting() {
        // Default: tickers every 30 min while Home is open.
        let c = Cadence::TwiceADay;
        assert_eq!(
            ask(
                c,
                Kind::Quotes,
                Trigger::HomeTick,
                Some(at(10, 0)),
                at(10, 30)
            ),
            Decision::Fetch
        );
        assert_eq!(
            ask(
                c,
                Kind::Quotes,
                Trigger::HomeTick,
                Some(at(10, 0)),
                at(10, 29)
            ),
            Decision::Skip(Skip::Floor)
        );
        // Stories on the same tick wait for the next slot.
        assert_eq!(
            ask(
                c,
                Kind::Stories,
                Trigger::HomeTick,
                Some(at(10, 0)),
                at(10, 30)
            ),
            Decision::Skip(Skip::NotDue)
        );
        // Other choices: quotes obey the same rule as stories.
        for c in [Cadence::EveryFewHours, Cadence::OnHomeOpen, Cadence::OnTap] {
            for t in ALL_TRIGGERS {
                for (last, now) in [
                    (Some(at(10, 0)), at(10, 45)),
                    (Some(at(10, 0)), at(14, 0)),
                    (None, at(9, 0)),
                ] {
                    assert_eq!(
                        ask(c, Kind::Quotes, t, last, now),
                        ask(c, Kind::Stories, t, last, now),
                        "{c:?} {t:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn news_refresh_trigger_names_round_trip() {
        for t in ALL_TRIGGERS {
            assert_eq!(Trigger::parse(t.as_str()), Some(t));
        }
        assert_eq!(Trigger::parse("cron"), None);
    }
}
