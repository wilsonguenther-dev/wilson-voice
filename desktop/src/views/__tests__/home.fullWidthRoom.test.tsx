/**
 * yap-1106-home-full-width-room (FB-07 + home-contained-scroll-footer) — the
 * approved round-two Home, rendered. The pure rules (whole-number room scale,
 * 5 takes then "See all", 50 per History page, the meeting label) are pinned
 * through home/homeLayout.ts; the markup through renderToStaticMarkup with a
 * shell context, the same harness as home.walk.test.tsx. Pixel evidence at 900
 * and 1400 px window heights is scripts/home-shots.mjs (headless Chrome).
 */
import { describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined), emit: vi.fn(async () => undefined) }));

const shell: { state: Record<string, unknown> } = { state: {} };
vi.mock("../../appShell", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../appShell")>();
  return { ...actual, useAppShell: () => makeCtx(shell.state) };
});

import { AppContext, type AppCtx } from "../../appShell";
import type { FailedDictation } from "../../appTypes";
import App from "../../App";
import Home, { NeedsAttention } from "../Home";
import {
  HISTORY_PAGE, HOME_RECENT, ROOM_H, ROOM_W, meetingClock, meetingRoomLabel, pageRangeLabel, recentTakes,
  requestHistoryFocus, roomBox, roomScale, seeAllLabel, showNeedsAttention, statsLine, takeHistoryFocus,
} from "../../home/homeLayout";
import privacySrc from "../settings/Privacy.tsx?raw";
import homeSrc from "../Home.tsx?raw";
import historySrc from "../History.tsx?raw";
import appCss from "../../App.css?raw";

type State = Record<string, unknown>;
function makeCtx(state: State): AppCtx {
  return new Proxy(state, {
    get(target, key: string) {
      if (key in target) return target[key];
      return () => undefined;
    },
  }) as unknown as AppCtx;
}

const insights = (o: Partial<{ totalSessions: number; wordsToday: number; streakDays: number }> = {}) => ({
  totalWords: 0, totalSessions: o.totalSessions ?? 0, wordsToday: o.wordsToday ?? 0, sessionsToday: 0, avgWpm: 0,
  streakDays: o.streakDays ?? 0, longestStreak: 0, wordsLast7: [], topApps: [], avgAsrSeconds: 0,
});
const take = (i: number) => ({
  id: `t${i}`, text: `take number ${i} words`, backend: "native", asrSeconds: 0.4, wordCount: 4,
  createdAt: `2026-11-06T09:${String(59 - i).padStart(2, "0")}:00Z`,
});
const failedRow = (id: string, error: string): FailedDictation => ({
  id, wavPath: "/tmp/x.wav", speechSeconds: 6.7, error, createdAt: "2026-11-06T09:25:00Z",
});
const base = (over: State = {}): State => ({
  nav: "home", booting: false, bootError: null, bootErrors: new Map(), settings: { onboarded: true },
  insights: insights(), perms: null, history: [], meetings: [], scratch: [], dictionary: [],
  failed: [], toast: null, update: null, query: "", toasts: [], retrying: null, retryAll: null, retryPhase: null,
  meetingStatus: { recording: false, elapsedSeconds: 0, captureAvailable: true },
  status: { modelReady: true, recording: false, busy: false, message: "", lastError: null },
  modelSetup: { ready: true, ribbon: null }, licenseChip: null, buyPrompt: null, consentOpen: false,
  supportOpen: false, supportPreview: null, needsPerms: false, pillClass: "status-pill ready",
  installedVersion: null, settingsTab: "companion", engine: null,
  ...over,
});
const home = (state: State) =>
  renderToStaticMarkup(<AppContext.Provider value={makeCtx(state)}>{(<Home />) as ReactElement}</AppContext.Provider>);
const app = (state: State) => {
  shell.state = state;
  return renderToStaticMarkup(<App />);
};
const noop = () => undefined;
const attention = (failed: FailedDictation[]) =>
  renderToStaticMarkup(
    <NeedsAttention failed={failed} retrying={null} retryAll={null} onRetry={noop} onRetryAll={noop} onDiscard={noop} />,
  );

describe("the room: full column width at a whole-number scale (option A)", () => {
  it("picks 5x, 4x or 3x — the largest that fits — never a fraction", () => {
    expect(roomScale(1400)).toBe(5);
    expect(roomScale(ROOM_W * 5)).toBe(5);
    expect(roomScale(ROOM_W * 5 - 1)).toBe(4);
    expect(roomScale(ROOM_W * 4)).toBe(4);
    expect(roomScale(ROOM_W * 4 - 1)).toBe(3);
    expect(roomScale(400)).toBe(3);
    expect(roomScale(Number.NaN)).toBe(3);
    expect(roomScale(undefined)).toBe(3);
    for (let w = 0; w <= 2400; w += 7) expect(Number.isInteger(roomScale(w))).toBe(true);
  });
  it("the stage is the room's height at that scale", () => {
    expect(roomBox(1100)).toEqual({ scale: 5, width: 960, height: 640 });
    expect(roomBox(800)).toEqual({ scale: 4, width: 768, height: ROOM_H * 4 });
  });
  it("the room has no size cap any more", () => {
    const rule = appCss.slice(appCss.indexOf(".yappy-house {"), appCss.indexOf("}", appCss.indexOf(".yappy-house {")));
    expect(rule).not.toMatch(/max-width/);
    expect(appCss).not.toMatch(/\.yh-stage\s*\{[^}]*max-height/);
  });
});

describe("greeting, Hold-fn card and stat boxes are removed; one quiet stats line", () => {
  it("renders the stats as one line under the room, real numbers only", () => {
    const html = home(base({ insights: insights({ totalSessions: 288, wordsToday: 2632, streakDays: 17 }), history: [take(0)] }));
    expect(html).toContain('data-home="stats"');
    expect(html).toContain("2,632 words today · 17 day streak · 288 takes");
    expect(html).not.toContain("yh-stat");
    expect(html).not.toContain("record-btn");
    expect(html).not.toContain("double-tap hands-free");
    expect(html.indexOf("yappy-house")).toBeLessThan(html.indexOf('data-home="stats"'));
    expect(statsLine({})).toEqual([]);
    expect(statsLine({ totalTakes: 1 })).toEqual(["1 take"]);
  });
  it("App titles the page Home with no greeting", () => {
    const html = app(base({ insights: insights({ totalSessions: 3 }) }));
    expect(html).toContain("<h1>Home</h1>");
    expect(html).not.toContain("Welcome");
  });
});

describe("the latest take shows live under the room with Copy and Open", () => {
  it("renders the newest take's card with Copy, Open and Paste again", () => {
    const html = home(base({ history: [take(0), take(1)], insights: insights({ totalSessions: 2 }) }));
    const card = html.slice(html.indexOf('data-home="latest"'));
    expect(card).toContain("Last take");
    expect(card).toContain("take number 0 words");
    expect(card).toMatch(/>Copy</);
    expect(card).toMatch(/>Open</);
    expect(html.indexOf('data-home="stats"')).toBeLessThan(html.indexOf('data-home="latest"'));
  });
  it("Open hands the take to History once", () => {
    requestHistoryFocus("t0");
    expect(takeHistoryFocus()).toBe("t0");
    expect(takeHistoryFocus()).toBeNull();
  });
});

describe("Needs attention: failed and cancelled takes, hidden when empty", () => {
  it("renders nothing at all when there is nothing to attend to", () => {
    expect(attention([])).toBe("");
    expect(showNeedsAttention([])).toBe(false);
    expect(showNeedsAttention(null)).toBe(false);
    expect(home(base({ history: [take(0)] }))).not.toContain('data-home="needs-attention"');
  });
  it("shows a cancelled take and a failed one, each with Retry and Discard", () => {
    const html = attention([failedRow("c", "Cancelled"), failedRow("f", "Engine could not transcribe")]);
    expect(html).toContain("Needs attention");
    expect(html).toContain("Cancelled take");
    expect(html).toContain("Could not transcribe");
    expect(html.match(/>Retry</g)?.length).toBe(2);
    expect(html.match(/>Discard</g)?.length).toBe(2);
    const page = home(base({ history: [take(0)], failed: [failedRow("c", "Cancelled")] }));
    expect(page).toContain('data-home="needs-attention"');
    expect(page).not.toContain("Failed and unfinished dictations");
  });
});

describe('"Delete all recordings" moved to Settings → Privacy', () => {
  it("is gone from Home and present in Privacy", () => {
    expect(home(base({ history: [take(0)] }))).not.toContain("Delete all recordings");
    expect(homeSrc).not.toContain("deleteAllRecordings");
    expect(privacySrc).toContain("Delete all recordings");
    expect(privacySrc).toContain("onClick={deleteAllRecordings}");
  });
});

describe("during a meeting: no floating timer on Home, Hoot in the room", () => {
  it("labels the room 'Recording meeting · mm:ss'", () => {
    expect(meetingClock(336)).toBe("05:36");
    expect(meetingClock(3725)).toBe("1:02:05");
    expect(meetingRoomLabel({ recording: true, elapsedSeconds: 336 })).toBe("Recording meeting · 05:36");
    expect(meetingRoomLabel({ recording: false, elapsedSeconds: 336 })).toBeNull();
    const html = home(base({ history: [take(0)], meetingStatus: { recording: true, elapsedSeconds: 336, captureAvailable: true } }));
    expect(html).toContain("Recording meeting · 05:36");
    expect(html).toContain('data-room-label="meeting"');
    expect(html).toContain("Hoot the owl sits on the desk lamp");
    // The only clock on Home is the room label: no meeting badge or timer markup.
    expect(html.match(/05:36/g)?.length).toBe(1);
    expect(html).not.toMatch(/meeting-badge|meeting-timer|float/);
  });
});

describe("contained scroll and a footer that never moves", () => {
  it("only .content scrolls, overscroll contained; the shell never scrolls", () => {
    expect(appCss).toMatch(/\.content\s*\{[^}]*overflow:\s*auto[^}]*overscroll-behavior:\s*contain/);
    expect(appCss).toMatch(/\.shell\s*\{[^}]*overflow:\s*hidden/);
  });
  it("the footer carries engine state, version, Privacy, Help and Send feedback, outside .content", () => {
    const html = app(base());
    const foot = html.slice(html.indexOf("data-app-footer"));
    expect(foot).toContain("Engine ready · on this Mac");
    expect(foot).toMatch(/Yap \S+/);
    for (const label of [">Privacy<", ">Help<", ">Send feedback<"]) expect(foot).toContain(label);
    expect(html.indexOf('class="content"')).toBeLessThan(html.indexOf("data-app-footer"));
    expect(html.indexOf("data-app-footer")).toBeGreaterThan(html.lastIndexOf('class="sidebar"'));
  });
});

describe("Home shows the last 5 takes then 'See all'; History pages 50 at a time", () => {
  it("lists the latest plus at most 5 more, then a See all row, and nothing that loads more", () => {
    const history = Array.from({ length: 12 }, (_, i) => take(i));
    const html = home(base({ history, insights: insights({ totalSessions: 288 }) }));
    const list = html.slice(html.indexOf('data-home="recent"'), html.indexOf('data-home="see-all"'));
    expect(list.match(/class="recent-row"/g)?.length).toBe(HOME_RECENT);
    expect(html).toContain("See all 288 →");
    expect(html).not.toContain("Show older takes");
    expect(html).not.toContain("history-show-older");
    expect(recentTakes(history)).toHaveLength(5);
    expect(seeAllLabel(null, "")).toBe("See all →");
    expect(seeAllLabel(288, "pills")).toBe("See every match in History");
  });
  it("History asks for one page of 50 (+1 to know an older page exists), with Newer / Older, never on scroll", () => {
    expect(HISTORY_PAGE).toBe(50);
    expect(historySrc).toContain("limit: HISTORY_PAGE + 1");
    expect(historySrc).toContain("before");
    expect(historySrc).not.toMatch(/onScroll|IntersectionObserver/);
    expect(pageRangeLabel(1, 50)).toBe("Takes 51–100");
    expect(pageRangeLabel(0, 0)).toBe("No takes");
  });
  it("the sidebar has a History item", () => {
    expect(app(base())).toMatch(/<span>History<\/span>/);
  });
});
