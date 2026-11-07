// yap-1106-friend-visits — every quiet rule, driven by a fake clock.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  DAILY_VISITS_QUIETER,
  DAILY_VISITS_QUIETEST,
  EMPTY_LEDGER,
  CALL_LIVE_PHASES,
  EVENT_OWNER,
  FRIEND_EVENTS,
  QUIETER_AFTER_TAKES,
  QUIETEST_AFTER_TAKES,
  ROOM_ONLY,
  VISIT_GAP_QUIETER_MS,
  VISIT_MAX_MS,
  VISIT_QUIETER_MS,
  callLive,
  dailyVisitCap,
  dayKey,
  decideVisit,
  forcedFriendVisit,
  milestoneReached,
  parseLedger,
  parseQuietState,
  quietReason,
  recordVisit,
  serializeLedger,
  visitDurationMs,
  visitLogLine,
  visitRemainingMs,
  visitShowing,
  type Visit,
  type VisitContext,
  type VisitDecision,
  type VisitLedger,
} from "./friendVisits";
import { COMPANION_VISIT_CAP_MS } from "./meetingScene";

const H = 60 * 60 * 1000;
/** 9:00 local on 2026-11-06 — local, so day boundaries hold in any time zone. */
const MORNING = new Date(2026, 10, 6, 9, 0, 0).getTime();

const calm: VisitContext = {
  meeting: false,
  screenSharing: false,
  doNotDisturb: false,
  takesSoFar: 10,
  occupant: "yappy",
  cloudConnected: true,
};

const shown = (d: VisitDecision): Visit => {
  if (!d.show) throw new Error(`expected a visit, got ${d.reason}`);
  return d.visit;
};
const reason = (d: VisitDecision) => (d.show ? "shown" : d.reason);

/** Ask for a visit at the fake clock's now, record it if shown. */
function at(event: string, ctx: VisitContext, ledger: VisitLedger, current: Visit | null = null) {
  const d = decideVisit(event, ctx, ledger, Date.now(), current);
  return { d, ledger: d.show ? recordVisit(ledger, d.visit) : ledger };
}

describe("friend visits — the approved quiet rules", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(MORNING);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("a friend appears only for its own event", () => {
    expect(shown(decideVisit("backtrack-caught", calm, EMPTY_LEDGER, Date.now())).friend).toBe("tidy");
    expect(shown(decideVisit("word-added", calm, EMPTY_LEDGER, Date.now())).friend).toBe("gulp");
    expect(shown(decideVisit("battery-low", calm, EMPTY_LEDGER, Date.now())).friend).toBe("shelly");
    expect(shown(decideVisit("meeting-notes-ready", calm, EMPTY_LEDGER, Date.now())).friend).toBe("hoot");
    expect(shown(decideVisit("news-ready", calm, EMPTY_LEDGER, Date.now())).friend).toBe("cinder");
    // Asking for a friend on someone else's event is refused.
    expect(reason(decideVisit("backtrack-caught", calm, EMPTY_LEDGER, Date.now(), null, "gulp"))).toBe("not-own-event");
    expect(reason(decideVisit("backtrack-caught", calm, EMPTY_LEDGER, Date.now(), null, "tidy"))).toBe("shown");
    // No event, no visit.
    expect(reason(decideVisit("take-finished", calm, EMPTY_LEDGER, Date.now()))).toBe("unknown-event");
    expect(reason(decideVisit("", calm, EMPTY_LEDGER, Date.now()))).toBe("unknown-event");
  });

  it("room-only friends never visit, and no event is owned by one", () => {
    for (const e of FRIEND_EVENTS) expect(ROOM_ONLY).not.toContain(EVENT_OWNER[e]);
    expect(forcedFriendVisit("?friend=mochi")).toBeNull();
    expect(forcedFriendVisit("?friend=sal")).toBeNull();
    expect(forcedFriendVisit("?friend=ember")).toBeNull();
    expect(forcedFriendVisit("?friend=tidy")).toBe("tidy");
    expect(forcedFriendVisit("")).toBeNull();
  });

  it("whoever sits in the pill never visits it; Yappy visits for milestones only when someone else sits", () => {
    expect(reason(decideVisit("meeting-notes-ready", { ...calm, occupant: "hoot" }, EMPTY_LEDGER, Date.now()))).toBe("occupant");
    expect(reason(decideVisit("milestone", calm, EMPTY_LEDGER, Date.now()))).toBe("occupant");
    expect(shown(decideVisit("milestone", { ...calm, occupant: "hoot" }, EMPTY_LEDGER, Date.now())).friend).toBe("yappy");
    expect(milestoneReached(100)).toBe(true);
    expect(milestoneReached(1000)).toBe(true);
    expect(milestoneReached(99)).toBe(false);
    expect(milestoneReached(101)).toBe(false);
  });

  it("Cinder visits only while the cloud is connected", () => {
    expect(reason(decideVisit("news-ready", { ...calm, cloudConnected: false }, EMPTY_LEDGER, Date.now()))).toBe("cloud-off");
  });

  it("a visit lasts at most 4 s and is gone when the fake clock passes it", () => {
    expect(VISIT_MAX_MS).toBe(4000);
    expect(VISIT_MAX_MS).toBe(COMPANION_VISIT_CAP_MS);
    const v = shown(decideVisit("backtrack-caught", calm, EMPTY_LEDGER, Date.now()));
    expect(v.untilMs - v.startMs).toBeLessThanOrEqual(4000);
    expect(visitShowing(v, calm, Date.now())).toBe(true);
    vi.advanceTimersByTime(3999);
    expect(visitShowing(v, calm, Date.now())).toBe(true);
    expect(visitRemainingMs(v, Date.now())).toBe(1);
    vi.advanceTimersByTime(1);
    expect(visitShowing(v, calm, Date.now())).toBe(false);
    expect(visitRemainingMs(v, Date.now())).toBe(0);
    expect(visitRemainingMs(null, Date.now())).toBe(0);
    for (const t of [0, 49, 50, 199, 200, 5000]) expect(visitDurationMs(t)).toBeLessThanOrEqual(4000);
  });

  it("each friend visits once a day; the next local day it may come back", () => {
    let l = at("backtrack-caught", calm, EMPTY_LEDGER).ledger;
    vi.advanceTimersByTime(3 * H);
    const again = at("backtrack-caught", calm, l);
    expect(reason(again.d)).toBe("already-today");
    // Another friend's first visit today is still allowed.
    l = at("word-added", calm, l).ledger;
    expect(l.count).toBe(2);
    // 23:59 the same day: still no.
    vi.setSystemTime(new Date(2026, 10, 6, 23, 59, 0).getTime());
    expect(reason(at("backtrack-caught", calm, l).d)).toBe("already-today");
    // 00:01 the next day: Tidy is back.
    vi.setSystemTime(new Date(2026, 10, 7, 0, 1, 0).getTime());
    const next = at("backtrack-caught", calm, l);
    expect(reason(next.d)).toBe("shown");
    expect(next.ledger.count).toBe(1);
    expect(next.ledger.day).toBe(dayKey(Date.now()));
  });

  it("one friend at a time: a second event while one is on screen waits its turn", () => {
    const first = at("backtrack-caught", calm, EMPTY_LEDGER);
    const v = shown(first.d);
    vi.advanceTimersByTime(1000);
    expect(reason(at("word-added", calm, first.ledger, v).d)).toBe("visit-showing");
    vi.advanceTimersByTime(3000);
    expect(reason(at("word-added", calm, first.ledger, v).d)).toBe("shown");
  });

  it("never during a meeting, screen sharing or Do Not Disturb", () => {
    expect(reason(decideVisit("backtrack-caught", { ...calm, meeting: true }, EMPTY_LEDGER, Date.now()))).toBe("meeting");
    expect(reason(decideVisit("backtrack-caught", { ...calm, screenSharing: true }, EMPTY_LEDGER, Date.now()))).toBe("screen-sharing");
    expect(reason(decideVisit("backtrack-caught", { ...calm, doNotDisturb: true }, EMPTY_LEDGER, Date.now()))).toBe("do-not-disturb");
    // Hoot does not visit during the meeting either (he is at the tape machine).
    expect(reason(decideVisit("meeting-notes-ready", { ...calm, meeting: true }, EMPTY_LEDGER, Date.now()))).toBe("meeting");
    expect(quietReason({ meeting: true, screenSharing: true, doNotDisturb: true })).toBe("meeting");
    expect(quietReason({ meeting: false, screenSharing: false, doNotDisturb: false })).toBeNull();
    // A quiet event that was skipped does not use up the friend's day.
    const l = at("backtrack-caught", { ...calm, doNotDisturb: true }, EMPTY_LEDGER).ledger;
    expect(l).toEqual(EMPTY_LEDGER);
    expect(reason(at("backtrack-caught", calm, l).d)).toBe("shown");
  });

  it("a live call is quiet like a meeting, from the call-offer phase", () => {
    for (const phase of CALL_LIVE_PHASES) expect(callLive({ phase })).toBe(true);
    expect(callLive({ phase: "idle" })).toBe(false);
    expect(callLive({ phase: "end_prompt" })).toBe(false);
    expect(callLive(null)).toBe(false);
    expect(callLive("recording")).toBe(false);
    expect(callLive({ phase: 3 })).toBe(false);
    // The pill folds a live call into `meeting`, so a friend stays away.
    const inCall = { ...calm, meeting: callLive({ phase: "chip" }) };
    expect(reason(decideVisit("backtrack-caught", inCall, EMPTY_LEDGER, Date.now()))).toBe("meeting");
    // When the call ends the friend may come (the fake clock moved on, the day was not used).
    vi.advanceTimersByTime(H);
    const after = { ...calm, meeting: callLive({ phase: "end_prompt" }) };
    expect(reason(decideVisit("backtrack-caught", after, EMPTY_LEDGER, Date.now()))).toBe("shown");
  });

  it("a visit on screen leaves the moment a meeting, a share or Do Not Disturb starts", () => {
    const v = shown(decideVisit("backtrack-caught", calm, EMPTY_LEDGER, Date.now()));
    vi.advanceTimersByTime(500);
    expect(visitShowing(v, calm, Date.now())).toBe(true);
    expect(visitShowing(v, { ...calm, meeting: true }, Date.now())).toBe(false);
    expect(visitShowing(v, { ...calm, screenSharing: true }, Date.now())).toBe(false);
    expect(visitShowing(v, { ...calm, doNotDisturb: true }, Date.now())).toBe(false);
    expect(visitShowing(null, calm, Date.now())).toBe(false);
  });

  it("before take 50 every friend may visit once today", () => {
    expect(dailyVisitCap(QUIETER_AFTER_TAKES - 1)).toBe(Infinity);
    let l = EMPTY_LEDGER;
    const ctx = { ...calm, takesSoFar: QUIETER_AFTER_TAKES - 1 };
    for (const e of ["backtrack-caught", "word-added", "battery-low", "meeting-notes-ready", "news-ready"]) {
      vi.advanceTimersByTime(5 * 60 * 1000);
      const r = at(e, ctx, l);
      expect(reason(r.d)).toBe("shown");
      l = r.ledger;
    }
    expect(l.count).toBe(5);
  });

  it("after 50 takes friends visit less: fewer a day, spaced out, and shorter", () => {
    const ctx = { ...calm, takesSoFar: QUIETER_AFTER_TAKES };
    expect(dailyVisitCap(QUIETER_AFTER_TAKES)).toBe(DAILY_VISITS_QUIETER);
    expect(visitDurationMs(QUIETER_AFTER_TAKES)).toBe(VISIT_QUIETER_MS);
    expect(visitDurationMs(QUIETER_AFTER_TAKES - 1)).toBe(VISIT_MAX_MS);
    let r = at("backtrack-caught", ctx, EMPTY_LEDGER);
    expect(shown(r.d).untilMs - shown(r.d).startMs).toBe(VISIT_QUIETER_MS);
    // Ten minutes later another friend's event is too soon.
    vi.advanceTimersByTime(10 * 60 * 1000);
    expect(reason(at("word-added", ctx, r.ledger).d)).toBe("too-soon");
    // Two hours after the first, Gulp may come.
    vi.setSystemTime(MORNING + VISIT_GAP_QUIETER_MS);
    r = at("word-added", ctx, r.ledger);
    expect(reason(r.d)).toBe("shown");
    // The day's cap of two is spent: Shelly waits for tomorrow.
    vi.setSystemTime(MORNING + 3 * VISIT_GAP_QUIETER_MS);
    expect(reason(at("battery-low", ctx, r.ledger).d)).toBe("daily-cap");
    vi.setSystemTime(new Date(2026, 10, 7, 9, 0, 0).getTime());
    expect(reason(at("battery-low", ctx, r.ledger).d)).toBe("shown");
  });

  it("after 200 takes one visit a day in all", () => {
    const ctx = { ...calm, takesSoFar: QUIETEST_AFTER_TAKES };
    expect(dailyVisitCap(QUIETEST_AFTER_TAKES)).toBe(DAILY_VISITS_QUIETEST);
    const r = at("backtrack-caught", ctx, EMPTY_LEDGER);
    expect(reason(r.d)).toBe("shown");
    vi.advanceTimersByTime(5 * H);
    expect(reason(at("word-added", ctx, r.ledger).d)).toBe("daily-cap");
  });

  it("the ledger survives a relaunch and a malformed row is an empty ledger", () => {
    const r = at("backtrack-caught", calm, EMPTY_LEDGER);
    const back = parseLedger(serializeLedger(r.ledger));
    expect(back).toEqual(r.ledger);
    expect(reason(at("backtrack-caught", calm, back).d)).toBe("already-today");
    expect(parseLedger(null)).toEqual(EMPTY_LEDGER);
    expect(parseLedger("{nope")).toEqual(EMPTY_LEDGER);
    expect(parseLedger("42")).toEqual(EMPTY_LEDGER);
    expect(parseLedger(JSON.stringify({ day: "x", count: 3, lastDay: { tidy: "bad", mochi: "2026-11-06", gulp: "2026-11-06" }, lastAtMs: "1" })))
      .toEqual({ day: "", count: 0, lastDay: { gulp: "2026-11-06" }, lastAtMs: null });
  });

  it("a clock that runs backwards or is not a number never throws", () => {
    expect(dayKey(Number.NaN)).toBe(dayKey(0));
    const ctx = { ...calm, takesSoFar: QUIETER_AFTER_TAKES };
    const l = at("backtrack-caught", ctx, EMPTY_LEDGER).ledger;
    // The Mac's clock was set back an hour: the gap rule does not lock friends out.
    vi.setSystemTime(MORNING - H);
    expect(reason(at("word-added", ctx, l).d)).toBe("shown");
    expect(parseLedger("")).toEqual(EMPTY_LEDGER);
    expect(parseLedger(JSON.stringify({ day: "2026-11-06", count: 1 }))).toEqual({ day: "2026-11-06", count: 1, lastDay: {}, lastAtMs: null });
    expect(dailyVisitCap(Number.NaN)).toBe(Infinity);
  });

  it("the quiet payload reads unknown fields as off", () => {
    expect(parseQuietState({ screenSharing: true })).toEqual({ screenSharing: true, doNotDisturb: false });
    expect(parseQuietState({ doNotDisturb: true, screenSharing: "yes" })).toEqual({ screenSharing: false, doNotDisturb: true });
    expect(parseQuietState(null)).toEqual({ screenSharing: false, doNotDisturb: false });
  });

  it("one log line per decision, with no free text", () => {
    const d = decideVisit("backtrack-caught", calm, EMPTY_LEDGER, Date.now());
    expect(visitLogLine("backtrack-caught", d)).toBe("pill friend_visit shown friend=tidy event=backtrack-caught ms=4000");
    expect(visitLogLine("battery-low", { show: false, reason: "do-not-disturb" })).toBe(
      "pill friend_visit skipped event=battery-low reason=do-not-disturb",
    );
    expect(visitLogLine("a b/c\"d", { show: false, reason: "unknown-event" })).toBe("pill friend_visit skipped event=abcd reason=unknown-event");
    expect(visitLogLine("", { show: false, reason: "unknown-event" })).toBe("pill friend_visit skipped event=none reason=unknown-event");
  });
});
