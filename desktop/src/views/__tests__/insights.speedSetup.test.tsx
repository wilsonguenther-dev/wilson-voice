/**
 * yap-uiux-2026-10-16-I1 — the daily walk row on Insights: "Engine & latency"
 * spoke in p50/p95 "hold→clipboard", "utt" and a paragraph about Whisper/MLX.
 * The fix collapses it behind a closed disclosure, in plain words, with no
 * paragraph. Rendered the same way as viewStates.render.test.tsx
 * (renderToStaticMarkup inside AppContext, no DOM, no new dependency).
 */
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));

import { AppContext, type AppCtx } from "../../appShell";
import Insights from "../Insights";

const ctx = (over: Record<string, unknown> = {}) =>
  new Proxy(
    {
      booting: false, bootError: null, daily30: [], hasActivity: false,
      heat: { cols: 0, months: [], cells: [], max: 0 }, heatSummary: { words: 0, sessions: 0, best: null },
      monthlySeries: [], monthlyView: { quiet: 0, rows: [], max: 1 }, maxApp: 1, maxDaily30: 1, maxWeek: 1,
      settings: null,
      status: { hotkeys: { carbon: { state: "registered" }, fnTap: { state: "delivering" } }, accessibility: true },
      insights: {
        totalWords: 4200, totalSessions: 40, wordsToday: 0, sessionsToday: 0, avgWpm: 140,
        streakDays: 3, longestStreak: 9, wordsLast7: [], topApps: [], avgAsrSeconds: 0.42,
        p50PipelineMs: 640, p95PipelineMs: 1840, speechSecondsTotal: 732, wpmSampleSessions: 40,
      },
      ...over,
    } as Record<string, unknown>,
    { get: (t, k: string) => (k in t ? t[k] : () => undefined) },
  ) as unknown as AppCtx;

const render = () =>
  renderToStaticMarkup(
    <AppContext.Provider value={ctx()}>
      <Insights />
    </AppContext.Provider>,
  );

describe("Insights: speed and setup reads in plain words, collapsed", () => {
  it("lives in a closed <details>, not an always-open panel", () => {
    const html = render();
    expect(html).toMatch(/<details[^>]*data-speed-setup/);
    expect(html).not.toMatch(/<details[^>]*\bopen\b/);
    expect(html).toContain("text usually lands in 0.64 s");
  });

  it("drops the jargon and the paragraph", () => {
    const html = render();
    for (const jargon of ["Engine &amp; latency", "hold→clipboard", "p50", "p95", " utt", "MLX", "Whisper", "Avg ASR"])
      expect(html).not.toContain(jargon);
  });

  it("names every row plainly", () => {
    const html = render();
    expect(html).toContain("Usual wait for text");
    expect(html).toContain("1.8 s");
    expect(html).toContain("12 min across 40 takes");
    expect(html).toContain("0.4 s per take");
    expect(html).toContain("allowed");
  });
});
