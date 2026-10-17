/**
 * Y5-B fix round (#192) - the three BLOCKING findings, rendered.
 *
 * 1. Permissions: `allCriticalOk` is mic + ASR only, so it must not alone
 *    license "Nothing to fix here" while Accessibility is OFF.
 * 2. bootError is one channel for refreshAll's 11 calls + loadHistory; a later
 *    refresh that fails must not replace a view whose data already loaded.
 * 3. Settings' LoadingState must be reachable through the real <App/> shell
 *    (it was hidden behind `nav === "settings" && settings && <Settings/>`).
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
import App from "../../App";
import Permissions, { PERMS_ALL_CLEAR_SUMMARY, permsAllClear } from "../Permissions";
import Home from "../Home";
import Dictionary from "../Dictionary";
import Scratchpad from "../Scratchpad";
import Insights from "../Insights";

type State = Record<string, unknown>;

function makeCtx(state: State): AppCtx {
  return new Proxy(state, {
    get(target, key: string) {
      if (key in target) return target[key];
      if (/^set[A-Z]/.test(key)) return () => undefined;
      return () => undefined;
    },
  }) as unknown as AppCtx;
}

function render(node: ReactElement, state: State): string {
  return renderToStaticMarkup(
    <AppContext.Provider value={makeCtx(state)}>{node}</AppContext.Provider>,
  );
}

const insightsRow = (over: State = {}) => ({
  totalWords: 0, totalSessions: 0, wordsToday: 0, sessionsToday: 0, avgWpm: 0,
  streakDays: 0, longestStreak: 0, wordsLast7: [], topApps: [], avgAsrSeconds: 0,
  ...over,
});
const LATER_REFRESH_FAILED = "list_crash_events: no such table: crash_events";

describe("Permissions: no false all-clear", () => {
  const permsRow = (over: State = {}) => ({
    accessibility: false, microphone: true, microphoneAuth: "authorized", inputMonitoring: "granted",
    audioCapture: "unknown", grants: [], asrOk: true, asrDetail: "ok", allCriticalOk: true,
    summary: "Accessibility OFF — enable for FN hold + auto-paste (Privacy → Accessibility → Yap)",
    ...over,
  });
  const ctx = (perms: unknown): State => ({
    booting: false, permsError: null, perms, modelSetup: { ready: true, ribbon: null }, status: { modelReady: true },
    sysAudio: null, sysAudioBusy: false, sysAudioGate: { available: false, requirement: "macOS 14.4 or later" },
  });

  it("Accessibility OFF with allCriticalOk=true keeps the Rust summary", () => {
    expect(permsAllClear(permsRow() as never)).toBe(false);
    const html = render(<Permissions />, ctx(permsRow()));
    expect(html).not.toContain("Nothing to fix here");
    expect(html).toContain("Accessibility OFF");
  });

  it("an Input Monitoring nag with Accessibility on still is not all-clear", () => {
    const p = permsRow({ accessibility: true, summary: "Input Monitoring denied" });
    expect(permsAllClear(p as never)).toBe(false);
    expect(render(<Permissions />, ctx(p))).not.toContain("Nothing to fix here");
  });

  it("shows the all-clear only for the Rust all-clear report", () => {
    const p = permsRow({ accessibility: true, summary: PERMS_ALL_CLEAR_SUMMARY });
    expect(permsAllClear(p as never)).toBe(true);
    expect(render(<Permissions />, ctx(p))).toContain("Nothing to fix here");
  });
});

describe("a later refreshAll failure does not replace loaded views", () => {
  const entry = { id: "a", text: "first take here", backend: "native", asrSeconds: 0.4, wordCount: 3, createdAt: "2026-10-01T09:00:00Z" };
  const loaded = (): State => ({
    booting: false, bootError: LATER_REFRESH_FAILED, settings: { onboarded: true },
    history: [entry], insights: insightsRow({ totalSessions: 1 }), failed: [], feedbackEdits: {}, query: "",
    queryRef: { current: null }, diffId: null, fixingId: null, fixDraft: "", retrying: null, clearing: false,
    status: { modelReady: true, recording: false, busy: false, hotkeys: { carbon: { state: "registered" }, fnTap: { state: "delivering" } }, accessibility: true },
    dictionary: [{ id: "d1", term: "Kubernetes", preferred: null }], candidates: [], newTerm: "", newPreferred: "",
    scratch: [], activeNoteId: null, noteTitle: "Scratchpad", noteBody: "", scratchComposing: false,
    daily30: [], hasActivity: true, heat: { cols: 0, months: [], cells: [], max: 0 },
    heatSummary: { words: 0, sessions: 0, best: null }, monthlySeries: [], monthlyView: { quiet: 0, rows: [], max: 1 },
    maxApp: 1, maxDaily30: 1, maxWeek: 1,
  });

  it("Home keeps its history", () => {
    const html = render(<Home />, loaded());
    expect(html).not.toContain("data-error-state");
    expect(html).toContain("first take here");
  });
  it("Dictionary keeps its terms", () => {
    const html = render(<Dictionary />, loaded());
    expect(html).not.toContain("data-error-state");
    expect(html).toContain("Kubernetes");
  });
  it("Scratchpad and Insights do not show an error state", () => {
    expect(render(<Scratchpad />, loaded())).not.toContain("data-error-state");
    expect(render(<Insights />, loaded())).not.toContain("data-error-state");
  });
  it("a view whose data never landed still shows its error", () => {
    const html = render(<Home />, { ...loaded(), insights: null, settings: null });
    expect(html).toContain('data-error-state="home"');
  });
});

describe("App shell: Settings' loading state is reachable", () => {
  it("nav=settings during boot renders the Settings loading state through <App/>", () => {
    shell.state = {
      nav: "settings", booting: true, bootError: null, settings: null, insights: null, perms: null,
      history: [], meetings: [], scratch: [], dictionary: [], failed: [], toast: null, update: null,
      status: { modelReady: false, recording: false, busy: false, message: "", hotkeys: { carbon: { state: "registered" }, fnTap: { state: "delivering" } }, accessibility: true },
      modelSetup: { ready: false, ribbon: null }, licenseChip: null, buyPrompt: null, consentOpen: false, supportOpen: false,
      supportPreview: null, needsPerms: false, pillClass: "", userName: "", installedVersion: "0.0.0",
      settingsTab: "companion",
    };
    const html = renderToStaticMarkup(<App />);
    expect(html).toContain('data-loading-state="settings"');
  });
});
