/**
 * yap-uiux-2026-10-16-HOME — the Home rows of docs/loop/UIUX-WALK-2026-10-16.md,
 * rendered. H1 greeting, H2 permissions-first empty Home, H4 search
 * placeholder, H6 a header status chip that always says something.
 * (H3 and H5 are CSS media queries; the smoke walk is their evidence.)
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
import Home from "../Home";

type State = Record<string, unknown>;

function makeCtx(state: State): AppCtx {
  return new Proxy(state, {
    get(target, key: string) {
      if (key in target) return target[key];
      return () => undefined;
    },
  }) as unknown as AppCtx;
}

const insights = (totalSessions: number) => ({
  totalWords: 0, totalSessions, wordsToday: 0, sessionsToday: 0, avgWpm: 0,
  streakDays: 0, longestStreak: 0, wordsLast7: [], topApps: [], avgAsrSeconds: 0,
});

const base = (over: State = {}): State => ({
  nav: "home", booting: false, bootError: null, bootErrors: new Map(), settings: { onboarded: true },
  insights: insights(0), perms: null, history: [], meetings: [], scratch: [], dictionary: [],
  failed: [], toast: null, update: null, query: "", flash: null, retryId: null,
  status: { modelReady: true, recording: false, busy: false, message: "", lastError: null },
  modelSetup: { ready: true, ribbon: null }, licenseChip: null, buyPrompt: null, consentOpen: false,
  supportOpen: false, supportPreview: null, needsPerms: false, pillClass: "status-pill ready",
  userName: "Smoke", installedVersion: "0.0.0", settingsTab: "companion",
  ...over,
});

function home(state: State): string {
  return renderToStaticMarkup(
    <AppContext.Provider value={makeCtx(state)}>{(<Home />) as ReactElement}</AppContext.Provider>,
  );
}
function app(state: State): string {
  shell.state = state;
  return renderToStaticMarkup(<App />);
}

describe("Home walk rows (yap-uiux-2026-10-16)", () => {
  it("H1: someone who has never dictated is welcomed, not welcomed back", () => {
    expect(app(base())).toContain("Welcome, Smoke");
    expect(app(base())).not.toContain("Welcome back");
    expect(app(base({ insights: insights(40) }))).toContain("Welcome back, Smoke");
  });

  it("H2: ungranted permissions put Grant permissions first, before the hotkey hint", () => {
    const html = home(base({ needsPerms: true }));
    expect(html).toContain("Grant permissions");
    expect(html.indexOf("Grant permissions")).toBeLessThan(html.indexOf("double-tap hands-free"));
    expect(html).not.toContain("No dictations yet");
    const granted = home(base({ needsPerms: false }));
    expect(granted).not.toContain("Grant permissions");
    expect(granted).toContain("No dictations yet");
  });

  it("H4: the search placeholder speaks to the user, not about SQLite", () => {
    const html = home(base());
    expect(html).not.toContain("FTS5");
    expect(html).toContain("Search everything you&#x27;ve said…");
  });

  it("H6: an empty bridge message never leaves an unlabeled status chip", () => {
    const html = app(base());
    expect(html).toMatch(/<div class="status-pill ready" role="status" aria-label="Yap status: Ready[^"]*" title="Ready[^"]*">Ready/);
    expect(app(base({ needsPerms: true }))).toContain('aria-label="Yap status: Needs permissions"');
  });
});
