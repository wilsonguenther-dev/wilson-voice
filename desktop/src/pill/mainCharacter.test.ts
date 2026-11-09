/**
 * yap-1106-main-character-switch — jobs never move (companions.html section 04).
 *
 * Meetings are always Hoot's: Hoot in the pill records himself (no visit),
 * anyone else in the pill and Hoot still visits the tape. Yappy out of the
 * pill stays in the studio and visits only for milestones: the 100th take,
 * the longest take of the week, a new streak record. Fake clock throughout.
 */
import { describe, expect, it } from "vitest";
import {
  EMPTY_LEDGER,
  EMPTY_TAKE_STATS,
  EVENT_OWNER,
  LONGEST_TAKE_MIN_S,
  LONGEST_WEEK_MIN_TAKES,
  YAPPY_MILESTONES,
  decideVisit,
  milestoneLine,
  noteTake,
  parseTakeStats,
  serializeTakeStats,
  takeSpeechSeconds,
  weekKey,
  type FriendEvent,
  type TakeStats,
  type VisitContext,
} from "./friendVisits";
import { SCENE_LAYOUT, meetingCast, meetingFrameKey, meetingScene } from "./meetingScene";

const DAY = 24 * 60 * 60 * 1000;
/** Wednesday 2026-11-04 10:00 local. */
const WED = new Date(2026, 10, 4, 10, 0, 0).getTime();
const calm: VisitContext = {
  meetingRecording: false, screenSharing: false, doNotDisturb: false, takesSoFar: 10, occupant: "hoot", cloudConnected: false,
};

/** Run `n` takes of `secs` each through noteTake, starting from `s`. */
function takes(s: TakeStats, n: number, secs: number, at: number, startCount = 1): { stats: TakeStats; hits: (FriendEvent | null)[] } {
  const hits: (FriendEvent | null)[] = [];
  let stats = s;
  for (let i = 0; i < n; i++) {
    const r = noteTake(stats, startCount + i, secs, at + i * 60_000);
    stats = r.stats;
    hits.push(r.milestone);
  }
  return { stats, hits };
}

describe("meetings stay Hoot's job", () => {
  it("hoot_in_the_pill_records_the_meeting_himself_without_a_visit", () => {
    expect(meetingCast("hoot")).toEqual({ main: "hoot", hootVisits: false });
    const m = meetingScene({ elapsedS: 600, occupant: "hoot" });
    expect(m.hoot.present).toBe(true);
    expect(m.hoot.visiting).toBe(false);
    expect(m.cast.main).toBe("hoot");
    // His headphones carry the recording light, at the main spot.
    expect(m.cup).toEqual({ ...SCENE_LAYOUT.mainCup });
    expect(m.recLight.on).toBe(true);
  });

  it("anyone_else_in_the_pill_and_hoot_still_visits_the_tape", () => {
    for (const id of ["yappy", "tidy", "gulp", "shelly"]) {
      expect(meetingCast(id).hootVisits, id).toBe(true);
      expect(meetingScene({ elapsedS: 60, occupant: id }).hoot.visiting, id).toBe(true);
    }
    // No occupant (Yappy's own stage) is the old scene, cup on Yappy.
    const old = meetingScene({ elapsedS: 60 });
    expect(old.cast.main).toBe("yappy");
    expect(old.cup).toEqual({ ...SCENE_LAYOUT.cup });
    expect(meetingCast("").main).toBe("yappy");
    expect(meetingCast(null).main).toBe("yappy");
  });

  it("switching_who_sits_in_the_pill_repaints_the_meeting_scene", () => {
    expect(meetingFrameKey({ elapsedS: 300, occupant: "hoot" })).not.toBe(meetingFrameKey({ elapsedS: 300 }));
    expect(meetingFrameKey({ elapsedS: 300, occupant: "yappy" })).toBe(meetingFrameKey({ elapsedS: 300 }));
  });

  it("hoot_in_the_pill_never_visits_for_his_own_notes", () => {
    const d = decideVisit("meeting-notes-ready", { ...calm, occupant: "hoot" }, EMPTY_LEDGER, WED);
    expect(d.show ? "shown" : d.reason).toBe("occupant");
  });
});

describe("Yappy visits only for milestones when he is not in the pill", () => {
  it("every_yappy_milestone_is_owned_by_yappy_and_only_his_events_bring_him", () => {
    expect(YAPPY_MILESTONES).toEqual(["milestone", "longest-take-week", "streak-record"]);
    for (const e of YAPPY_MILESTONES) expect(EVENT_OWNER[e]).toBe("yappy");
    const yappyEvents = (Object.keys(EVENT_OWNER) as FriendEvent[]).filter((e) => EVENT_OWNER[e] === "yappy");
    expect(yappyEvents.sort()).toEqual([...YAPPY_MILESTONES].sort());
  });

  it("yappy_visits_a_friend_in_the_pill_for_each_milestone_and_never_his_own_pill", () => {
    for (const e of YAPPY_MILESTONES) {
      const d = decideVisit(e, { ...calm, occupant: "tidy" }, EMPTY_LEDGER, WED);
      expect(d.show && d.visit.friend, e).toBe("yappy");
      const own = decideVisit(e, { ...calm, occupant: "yappy" }, EMPTY_LEDGER, WED);
      expect(own.show ? "shown" : own.reason, e).toBe("occupant");
      expect(milestoneLine(e, 100)).toMatch(/— Yappy$/);
    }
    expect(milestoneLine("milestone", 100)).toBe("that's a hundred! — Yappy");
    expect(milestoneLine("word-added", 100)).toBeNull();
  });

  it("the_100th_take_is_a_milestone", () => {
    expect(noteTake(EMPTY_TAKE_STATS, 100, 3, WED).milestone).toBe("milestone");
    expect(noteTake(EMPTY_TAKE_STATS, 99, 3, WED).milestone).toBeNull();
    expect(noteTake(EMPTY_TAKE_STATS, 101, 3, WED).milestone).toBeNull();
  });

  it("the_longest_take_of_the_week_is_a_milestone_after_a_few_takes", () => {
    const warm = takes(EMPTY_TAKE_STATS, LONGEST_WEEK_MIN_TAKES, 12, WED);
    // The opening takes of a week never count as "longest".
    expect(warm.hits.every((h) => h === null)).toBe(true);
    expect(noteTake(warm.stats, 10, 30, WED + DAY / 4).milestone).toBe("longest-take-week");
    // Shorter, equal, or too short to matter: no visit.
    expect(noteTake(warm.stats, 10, 12, WED + DAY / 4).milestone).toBeNull();
    expect(noteTake(warm.stats, 10, LONGEST_TAKE_MIN_S - 1, WED + DAY / 4).milestone).toBeNull();
    // A new week starts the count again.
    const nextWeek = WED + 7 * DAY;
    expect(weekKey(nextWeek)).not.toBe(weekKey(WED));
    expect(noteTake(warm.stats, 10, 60, nextWeek).milestone).toBeNull();
  });

  it("a_new_streak_record_is_a_milestone", () => {
    let s = EMPTY_TAKE_STATS;
    const out: (FriendEvent | null)[] = [];
    for (let d = 0; d < 4; d++) {
      const r = noteTake(s, 10 + d, 2, WED + d * DAY);
      s = r.stats;
      out.push(r.milestone);
    }
    // Day 3 beats the best (2) and reaches the minimum; day 4 beats day 3.
    expect(out).toEqual([null, null, "streak-record", "streak-record"]);
    expect(s.streak).toBe(4);
    expect(s.bestStreak).toBe(4);
    // A gap breaks the streak; matching the old best is not a record.
    const gap = noteTake(s, 20, 2, WED + 6 * DAY);
    expect(gap.stats.streak).toBe(1);
    expect(gap.milestone).toBeNull();
    // A second take the same day neither grows the streak nor re-fires.
    const again = noteTake(s, 21, 2, WED + 3 * DAY + 60_000);
    expect(again.stats.streak).toBe(4);
    expect(again.milestone).toBeNull();
  });

  it("take_stats_round_trip_and_malformed_rows_read_as_empty", () => {
    const s = takes(EMPTY_TAKE_STATS, 3, 20, WED).stats;
    expect(parseTakeStats(serializeTakeStats(s))).toEqual(s);
    for (const raw of [null, "", "{", "[]", JSON.stringify({ week: "nope", streak: -2 })]) {
      expect(parseTakeStats(raw)).toEqual({ ...EMPTY_TAKE_STATS });
    }
    expect(takeSpeechSeconds({ speechSeconds: 14.5 })).toBe(14.5);
    expect(takeSpeechSeconds({ speech_seconds: 3 })).toBe(3);
    expect(takeSpeechSeconds({ wordCount: 9 })).toBeNull();
    expect(takeSpeechSeconds(null)).toBeNull();
  });
});
