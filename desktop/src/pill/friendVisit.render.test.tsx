// yap-1106-friend-visits — the shipping view, rendered without a DOM
// (renderToStaticMarkup, like backtrackCaught.test.tsx). Effects do not run
// here, so this proves what the markup is for a visitor on screen and that the
// quiet rule hides even a pinned one; WHEN a visit happens is friendVisits.test.ts.
import { afterEach, describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

// fix-l3b8-1 — the pill's take count comes from `get_insights`; mock the IPC.
const { invoke } = vi.hoisted(() => ({
  invoke: vi.fn((..._args: unknown[]): Promise<unknown> => Promise.resolve(null)),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(() => Promise.resolve()),
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import { FriendVisit, INSIGHTS_COMMAND, pillTakeTracker } from "./FriendVisit";
import { matrixFriendVisitScene } from "./forced";
import {
  EMPTY_LEDGER,
  decideVisit,
  forcedFriendVisit,
  recordVisit,
  type VisitContext,
  type VisitDecision,
} from "./friendVisits";

describe("friend visit — the view", () => {
  it("draws nothing when nobody is visiting", () => {
    const html = renderToStaticMarkup(
      <FriendVisit meetingRecording={false} meetingNotesReady={false} occupant="yappy" />,
    );
    expect(html).toBe("");
  });

  it("draws the visitor decoratively, with no hitbox", () => {
    const html = renderToStaticMarkup(
      <FriendVisit meetingRecording={false} meetingNotesReady={false} occupant="yappy" forced="tidy" />,
    );
    expect(html).toContain('data-friend-visit="tidy"');
    expect(html).toContain('aria-hidden="true"');
    expect(html).toContain("<canvas");
  });

  it("hides even a pinned visitor while a meeting records", () => {
    const html = renderToStaticMarkup(
      <FriendVisit meetingRecording={true} meetingNotesReady={false} occupant="yappy" forced="tidy" />,
    );
    expect(html).toBe("");
  });

  it("the matrix row pins Tidy on an idle pill", () => {
    const row = matrixFriendVisitScene();
    expect(row.phase).toBe("idle");
    expect(row.id).toBe("friend-visit");
    expect(forcedFriendVisit("?" + row.query)).toBe("tidy");
  });
});

describe("friend visits — the pill reads the real take count on mount", () => {
  const MORNING = new Date(2026, 10, 7, 9, 0, 0).getTime();
  const calm: Omit<VisitContext, "takesSoFar"> = {
    meetingRecording: false,
    screenSharing: false,
    doNotDisturb: false,
    occupant: "yappy",
    cloudConnected: true,
  };
  const why = (d: VisitDecision) => (d.show ? "shown" : d.reason);
  /** Answer get_insights with `total()`; anything else resolves null. */
  const insightsReturn = (total: () => number) =>
    invoke.mockImplementation((...args: unknown[]) =>
      Promise.resolve(args[0] === INSIGHTS_COMMAND ? { totalSessions: total(), totalWords: 0 } : null),
    );

  afterEach(() => {
    invoke.mockReset();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("totalSessions=3000: the after-50 rules apply from the first event (shown, too-soon, daily-cap)", async () => {
    insightsReturn(() => 3000);
    // The old local counter says 0 takes; it must not drive the ladder.
    vi.stubGlobal("localStorage", { getItem: vi.fn(() => "0"), setItem: vi.fn() });
    const t = pillTakeTracker();
    expect(await t.seed()).toBe("insights");
    expect(invoke).toHaveBeenCalledWith(INSIGHTS_COMMAND);
    expect(t.takes()).toBe(3000);
    const ctx: VisitContext = { ...calm, takesSoFar: t.takes() };
    const first = decideVisit("word-added", ctx, EMPTY_LEDGER, MORNING);
    expect(why(first)).toBe("shown");
    if (!first.show) throw new Error("expected Gulp's visit");
    const ledger = recordVisit(EMPTY_LEDGER, first.visit);
    expect(why(decideVisit("backtrack-caught", ctx, ledger, MORNING + 10 * 60 * 1000))).toBe("too-soon");
    expect(why(decideVisit("battery-low", ctx, ledger, MORNING + 3 * 60 * 60 * 1000))).toBe("daily-cap");
  });

  it("totalSessions=99: one transcript raises the milestone visit and the next does not", async () => {
    let real = 99;
    insightsReturn(() => real);
    const t = pillTakeTracker();
    await t.seed();
    real = 100;
    const crossed = await t.transcript();
    expect(crossed).toBe(true);
    expect(why(decideVisit("milestone", { ...calm, takesSoFar: t.takes() }, EMPTY_LEDGER, MORNING))).toBe("shown");
    real = 101;
    expect(await t.transcript()).toBe(false);
  });

  it("the local counter is only the fallback, with a warn line, when get_insights fails", async () => {
    invoke.mockImplementation(() => Promise.reject(new Error("ipc down")));
    vi.stubGlobal("localStorage", { getItem: vi.fn(() => "64"), setItem: vi.fn() });
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const t = pillTakeTracker();
    expect(await t.seed()).toBe("fallback");
    expect(t.takes()).toBe(64);
    expect(warn).toHaveBeenCalledWith("pill friend_visit takes_fallback reason=insights-unavailable");
  });
});
