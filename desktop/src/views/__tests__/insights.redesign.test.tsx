/**
 * yap-1106-insights-redesign — the screen leads with three headline numbers and
 * one sentence, puts the charts second, ends with a meetings section, and every
 * chart owns its loading / empty / error state. Rendered like
 * insights.speedSetup.test.tsx (renderToStaticMarkup inside AppContext, no DOM).
 */
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));

import { AppContext, type AppCtx } from "../../appShell";
import Insights from "../Insights";

const meetings = {
  totalMeetings: 2, meetingsLast7: 1, totalSeconds: 5400, completeMeetings: 2, partialMeetings: 0,
  failedMeetings: 0, segmentsIndexed: 40, meetingsWithAudio: 2, audioRetentionDays: 30,
};

const insightsRow = (over: Record<string, unknown> = {}) => ({
  totalWords: 4000, totalSessions: 40, wordsToday: 0, sessionsToday: 0, avgWpm: 140,
  streakDays: 5, longestStreak: 9,
  wordsLast7: [{ date: "2026-11-03", words: 300, sessions: 3 }],
  topApps: [{ app: "Notes", words: 1000, sessions: 10 }], avgAsrSeconds: 0.42,
  p50PipelineMs: 640, p95PipelineMs: 1840, speechSecondsTotal: 600, wpmSampleSessions: 40,
  ...over,
});

const ctx = (over: Record<string, unknown> = {}) =>
  new Proxy(
    {
      booting: false, bootError: null, bootErrors: {}, daily30: [], hasActivity: false,
      heat: { cols: 0, months: [], cells: [], max: 0 }, heatSummary: { words: 0, sessions: 0, best: null },
      monthlySeries: [], monthlyView: { quiet: 0, rows: [], max: 1 }, maxApp: 1, maxDaily30: 1, maxWeek: 1,
      settings: null,
      status: { hotkeys: { carbon: { state: "registered" }, fnTap: { state: "delivering" } }, accessibility: true },
      insights: insightsRow(),
      ...over,
    } as Record<string, unknown>,
    { get: (t, k: string) => (k in t ? t[k] : () => undefined) },
  ) as unknown as AppCtx;

const render = (over: Record<string, unknown> = {}) =>
  renderToStaticMarkup(
    <AppContext.Provider value={ctx(over)}>
      <Insights />
    </AppContext.Provider>,
  );

const chart = (html: string, id: string) => {
  const m = html.match(new RegExp(`data-chart="${id}" data-chart-state="(\\w+)"`));
  return m ? m[1] : null;
};

describe("Insights redesign: numbers and a sentence first", () => {
  it("leads with three headline numbers before any chart", () => {
    const html = render({ insights: insightsRow({ meetings }) });
    const lead = html.indexOf('data-headline="week"');
    expect(lead).toBeGreaterThan(-1);
    expect(html.indexOf('data-headline="saved"')).toBeGreaterThan(lead);
    expect(html.indexOf('data-headline="meetings"')).toBeGreaterThan(lead);
    expect(html.indexOf("data-chart=")).toBeGreaterThan(html.indexOf('data-headline="meetings"'));
    expect(html).toContain("words this week");
    expect(html).toContain("1 h 30 min");
    expect(html).toContain("hours in meetings");
  });

  it("writes one insight sentence", () => {
    const html = render();
    expect(html.match(/data-insight-sentence/g)).toHaveLength(1);
    expect(html).toContain("You have dictated 5 days in a row; your best is 9.");
  });

  it("puts the meetings section after the charts and speed and setup last", () => {
    const html = render({ insights: insightsRow({ meetings }) });
    const charts = html.lastIndexOf('data-chart="apps"');
    expect(html.indexOf("insights-meetings")).toBeGreaterThan(charts);
    expect(html.indexOf("data-speed-setup")).toBeGreaterThan(html.indexOf("insights-meetings"));
  });
});

describe("Insights redesign: every chart has empty, loading and error states", () => {
  it("keeps each chart frame with the empty copy when there is no series", () => {
    const html = render({ insights: insightsRow({ wordsLast7: [], topApps: [] }) });
    for (const id of ["week", "daily30", "year", "months", "apps"]) expect(chart(html, id)).toBe("empty");
    expect(html).toContain("Not enough takes yet; this fills in after a few days.");
  });

  it("shows a skeleton per chart and dashes for the numbers while loading", () => {
    const html = render({ booting: true, insights: null });
    expect(html).toContain('data-loading-state="insights"');
    for (const id of ["week", "daily30", "year", "months", "apps"]) expect(chart(html, id)).toBe("loading");
    expect(html).toContain("chart-skeleton");
    expect(html.match(/<div class="stat-n">—<\/div>/g)).toHaveLength(3);
  });

  it("fails one chart at a time with a Retry, the rest still render", () => {
    const html = render({
      bootErrors: { monthly_series: "boom" },
      daily30: [{ date: "2026-11-03", words: 300 }],
    });
    expect(chart(html, "months")).toBe("error");
    expect(chart(html, "daily30")).toBe("ready");
    expect(chart(html, "week")).toBe("ready");
    expect(html).toContain("Couldn&#x27;t load this chart.");
    expect(html).toContain(">Retry</button>");
  });

  it("makes each ready chart one tab stop with a live region", () => {
    const html = render({ daily30: [{ date: "2026-11-03", words: 300 }] });
    expect(html).toMatch(/class="chart-points" tabindex="0" role="group" aria-roledescription="chart"/);
    expect(html).toContain('aria-live="polite"');
  });
});

describe("Insights redesign: meetings section", () => {
  it("shows hours, and says people are not known yet when speakers are unnamed", () => {
    const html = render({ insights: insightsRow({ meetings }) });
    expect(html).toContain("meeting-strip");
    expect(html).toContain("1.5");
    expect(chart(html, "people")).toBe("empty");
    expect(html).toContain("People show up here once Yap knows who is talking in your meetings.");
  });

  it("lists top people by talk time when the backend names them", () => {
    const topPeople = [
      { name: "Alex", seconds: 600, meetings: 1 },
      { name: "Sam", seconds: 1800, meetings: 2 },
    ];
    const html = render({ insights: insightsRow({ meetings: { ...meetings, topPeople } }) });
    expect(chart(html, "people")).toBe("ready");
    expect(html.indexOf(">Sam<")).toBeLessThan(html.indexOf(">Alex<"));
    expect(html).toContain("30 min");
  });

  it("offers a way to Meetings when none are recorded", () => {
    const html = render({ insights: insightsRow({ meetings: { ...meetings, totalMeetings: 0 } }) });
    expect(html).toContain("No meetings recorded yet.");
    expect(html).toContain("Go to Meetings");
  });
});
