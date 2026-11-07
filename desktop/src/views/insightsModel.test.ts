import { afterEach, describe, expect, it, vi } from "vitest";
import type { Insights, MeetingStats } from "../appTypes";
import {
  CHART_EMPTY_COPY,
  CHART_ERROR_COPY,
  TOP_PEOPLE_LIMIT,
  TYPING_WPM,
  chartState,
  formatMinutes,
  headlineNumbers,
  insightSentence,
  meetingHours,
  nextPointIndex,
  pointAnnouncement,
  timeSavedMinutes,
  topPeople,
  uiLog,
  uiLogLine,
  wordsThisWeek,
} from "./insightsModel";

const day = (date: string, words: number) => ({ date, words, sessions: words > 0 ? 1 : 0 });

const meetings = (over: Partial<MeetingStats> = {}): MeetingStats => ({
  totalMeetings: 3, meetingsLast7: 1, totalSeconds: 5400, completeMeetings: 3, partialMeetings: 0,
  failedMeetings: 0, segmentsIndexed: 90, meetingsWithAudio: 3, audioRetentionDays: 30, ...over,
});

const insights = (over: Partial<Insights> = {}): Insights => ({
  totalWords: 4000, totalSessions: 40, wordsToday: 0, sessionsToday: 0, avgWpm: 140,
  streakDays: 1, longestStreak: 4,
  wordsLast7: [day("2026-11-01", 100), day("2026-11-02", 200), day("2026-11-03", 0)],
  topApps: [{ app: "Notes", words: 1000, sessions: 10 }],
  avgAsrSeconds: 0.4, speechSecondsTotal: 600, ...over,
});

afterEach(() => vi.restoreAllMocks());

describe("chartState", () => {
  it("orders error over data over loading over empty", () => {
    expect(chartState({ loading: true, error: "boom", hasData: true })).toBe("error");
    expect(chartState({ loading: true, hasData: true })).toBe("ready");
    expect(chartState({ loading: true, hasData: false })).toBe("loading");
    expect(chartState({ loading: false, error: null, hasData: false })).toBe("empty");
  });
  it("uses the spec copy", () => {
    expect(CHART_EMPTY_COPY).toBe("Not enough takes yet; this fills in after a few days.");
    expect(CHART_ERROR_COPY).toBe("Couldn't load this chart.");
  });
});

describe("headline numbers", () => {
  it("sums the week and ignores negative noise", () => {
    expect(wordsThisWeek([day("a", 5), day("b", -3), day("c", 7)])).toBe(12);
    expect(wordsThisWeek(undefined)).toBe(0);
  });

  it("time saved = typing time minus measured speaking time, never negative", () => {
    // 4000 words at 40 wpm = 100 min typing; 600 s = 10 min speaking.
    expect(timeSavedMinutes(4000, 600, 140)).toBeCloseTo(90);
    // No measured speech: fall back to the speaking rate.
    expect(timeSavedMinutes(1400, undefined, 140)).toBeCloseTo(1400 / TYPING_WPM - 10);
    // Speaking slower than typing still never reads as negative.
    expect(timeSavedMinutes(40, 600, 140)).toBe(0);
    expect(timeSavedMinutes(0, 600, 140)).toBe(0);
    // A zero rate falls back to the typing rate, so it saves nothing.
    expect(timeSavedMinutes(400, 0, 0)).toBe(0);
  });

  it("formats minutes the way a person says them", () => {
    expect(formatMinutes(0.2)).toBe("under a minute");
    expect(formatMinutes(12)).toBe("12 min");
    expect(formatMinutes(120)).toBe("2 h");
    expect(formatMinutes(125)).toBe("2 h 5 min");
  });

  it("formats meeting hours", () => {
    expect(meetingHours(undefined)).toBe("0");
    expect(meetingHours(meetings({ totalSeconds: 5400 }))).toBe("1.5");
    expect(meetingHours(meetings({ totalSeconds: 3600 * 14.4 }))).toBe("14");
  });

  it("shows three numbers, dashes while loading", () => {
    const loading = headlineNumbers(null);
    expect(loading.map((h) => h.key)).toEqual(["week", "saved", "meetings"]);
    expect(loading.every((h) => h.value === "—")).toBe(true);
    const ready = headlineNumbers(insights({ meetings: meetings() }));
    expect(ready.map((h) => h.value)).toEqual(["300", "1 h 30 min", "1.5"]);
    expect(ready[1].hint).toContain(`${TYPING_WPM} words a minute`);
  });
});

describe("insight sentence", () => {
  const twoWeeks = (prior: number, recent: number) =>
    Array.from({ length: 14 }, (_, i) => ({ date: `d${i}`, words: i < 7 ? prior : recent }));

  it("is null while loading", () => {
    expect(insightSentence(null, [])).toBeNull();
  });

  it("leads with a real week-over-week change", () => {
    const ins = insights({ wordsLast7: [day("x", 700)] });
    expect(insightSentence(ins, twoWeeks(50, 100))).toBe("You said 100% more this week than the week before.");
    const less = insights({ wordsLast7: [day("x", 175)] });
    expect(insightSentence(less, twoWeeks(50, 25))).toBe("You said 50% less this week than the week before.");
  });

  it("falls to the streak when the change is small", () => {
    const ins = insights({ wordsLast7: [day("x", 350)], streakDays: 5, longestStreak: 9 });
    expect(insightSentence(ins, twoWeeks(50, 50))).toBe("You have dictated 5 days in a row; your best is 9.");
    const best = insights({ streakDays: 9, longestStreak: 9 });
    expect(insightSentence(best, [])).toBe("You have dictated 9 days in a row, your longest run yet.");
  });

  it("then where the words go, then the total, then meetings", () => {
    expect(insightSentence(insights({ topApps: [{ app: "Slack", words: 2000, sessions: 5 }] }), [])).toBe(
      "Most of your words go to Slack: 50% of everything you have said.",
    );
    expect(insightSentence(insights({ topApps: [] }), [])).toBe("You have dictated 4,000 words with Yap so far.");
    expect(
      insightSentence(insights({ totalWords: 0, topApps: [], wordsLast7: [], meetings: meetings({ totalMeetings: 1 }) }), []),
    ).toBe("You have recorded 1 meeting with Yap so far.");
    expect(insightSentence(insights({ totalWords: 0, topApps: [], wordsLast7: [] }), [])).toBeNull();
  });
});

describe("top people", () => {
  it("is null (not yet) when the backend does not name speakers", () => {
    expect(topPeople(meetings())).toBeNull();
    expect(topPeople(undefined)).toBeNull();
  });

  it("sorts by talk time, drops blanks and silence, caps the list", () => {
    const people = [
      { name: "Alex", seconds: 60, meetings: 1 },
      { name: "  ", seconds: 900, meetings: 2 },
      { name: "Sam", seconds: 300, meetings: 2 },
      { name: "Quiet", seconds: 0, meetings: 1 },
      ...Array.from({ length: 6 }, (_, i) => ({ name: `P${i}`, seconds: 10 + i, meetings: 1 })),
    ];
    const top = topPeople(meetings({ topPeople: people }))!;
    expect(top).toHaveLength(TOP_PEOPLE_LIMIT);
    expect(top.map((p) => p.name).slice(0, 3)).toEqual(["Sam", "Alex", "P5"]);
    expect(topPeople(meetings({ topPeople: [] }))).toEqual([]);
  });
});

describe("keyboard and announcements", () => {
  it("moves through points with arrows, Home and End", () => {
    expect(nextPointIndex(-1, "ArrowRight", 5)).toBe(0);
    expect(nextPointIndex(-1, "ArrowLeft", 5)).toBe(4);
    expect(nextPointIndex(2, "ArrowDown", 5)).toBe(3);
    expect(nextPointIndex(4, "ArrowRight", 5)).toBe(4);
    expect(nextPointIndex(0, "ArrowUp", 5)).toBe(0);
    expect(nextPointIndex(3, "Home", 5)).toBe(0);
    expect(nextPointIndex(0, "End", 5)).toBe(4);
    expect(nextPointIndex(9, "ArrowLeft", 5)).toBe(3);
    expect(nextPointIndex(0, "Tab", 5)).toBeNull();
    expect(nextPointIndex(0, "ArrowRight", 0)).toBeNull();
  });

  it("announces a point in words", () => {
    expect(pointAnnouncement("Tuesday", 1204)).toBe("Tuesday, 1,204 words");
    expect(pointAnnouncement("Tuesday", 1)).toBe("Tuesday, 1 word");
  });
});

describe("telemetry line", () => {
  it("is ui <event> key=value, no spaces in values", () => {
    expect(uiLogLine("insights_view")).toBe("ui insights_view");
    expect(uiLogLine("insights_chart_retry", { chart: "daily 30", n: 2, ok: true })).toBe(
      "ui insights_chart_retry chart=daily_30 n=2 ok=true",
    );
  });
  it("writes one console line and never throws", () => {
    const spy = vi.spyOn(console, "info").mockImplementation(() => {});
    uiLog("insights_chart_error", { chart: "months" });
    expect(spy).toHaveBeenCalledWith("ui insights_chart_error chart=months");
    spy.mockImplementation(() => {
      throw new Error("no console");
    });
    expect(() => uiLog("insights_view")).not.toThrow();
  });
});
