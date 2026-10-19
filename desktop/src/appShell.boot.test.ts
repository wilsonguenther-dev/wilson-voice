/**
 * yap-p0-insertion-bridge-fallback DO 3 — one failing backend call must not
 * blank the app as "Bridge error".
 *
 * These cases drive the REAL `useAppShell().refreshAll` (not a re-implementation
 * of it): `useState` is swapped for an index-keyed store while a probe component
 * renders the hook, so the setters `refreshAll` calls land somewhere a second
 * render can read back. Effects never run under renderToStaticMarkup, so the
 * only boot pass is the one each test calls by hand. The resulting shell is then
 * rendered through the real <App/> and the real view.
 *
 * On unmodified main (Promise.all) the first case is RED: one rejected
 * `list_crash_events` rejected the whole batch, `settings` never landed, and the
 * status line became `Bridge error: …` behind a full-screen "UI failed to load
 * backend bridge".
 */
import { describe, expect, it, vi } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

const fake = vi.hoisted(() => ({
  store: [] as unknown[],
  idx: 0,
  on: false,
  fail: new Set<string>(),
  ctx: null as unknown,
  calls: [] as string[],
}));

vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  const useState = (init: unknown) => {
    if (!fake.on) return actual.useState(init);
    const i = fake.idx++;
    if (!(i in fake.store)) {
      fake.store[i] = typeof init === "function" ? (init as () => unknown)() : init;
    }
    const set = (v: unknown) => {
      fake.store[i] = typeof v === "function" ? (v as (p: unknown) => unknown)(fake.store[i]) : v;
    };
    return [fake.store[i], set];
  };
  return { ...actual, default: { ...actual, useState }, useState };
});

const insightsRow = {
  totalWords: 12, totalSessions: 3, wordsToday: 12, sessionsToday: 3, avgWpm: 140,
  streakDays: 1, longestStreak: 1, wordsLast7: [], topApps: [], avgAsrSeconds: 0.4,
};
const RESPONSES: Record<string, unknown> = {
  get_settings: { onboarded: true, auto_paste: true },
  get_status: { recording: false, busy: false, message: "Ready", lastError: null },
  get_insights: insightsRow,
  daily_series: [],
  monthly_series: [],
  list_dictionary: [],
  list_dict_candidates: [],
  list_snippets: [],
  list_scratch: [],
  list_failed_dictations: [],
  list_crash_events: [],
  get_history: [],
};

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    fake.calls.push(cmd);
    if (fake.fail.has(cmd)) throw `${cmd}: boom`;
    return cmd in RESPONSES ? RESPONSES[cmd] : null;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
  emit: vi.fn(async () => undefined),
}));
vi.mock("./appShell", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./appShell")>();
  return {
    ...actual,
    useAppShell: () => (fake.ctx ? (fake.ctx as ReturnType<typeof actual.useAppShell>) : actual.useAppShell()),
  };
});

import { AppContext, useAppShell, type AppCtx } from "./appShell";
import App from "./App";
import Dictionary from "./views/Dictionary";

/** Render the hook, run ONE real boot pass with `failing` rejected, read the shell back. */
async function bootWith(failing: string[]): Promise<AppCtx> {
  fake.store = [];
  fake.ctx = null;
  fake.calls = [];
  fake.fail = new Set(failing);
  let ctx: AppCtx | null = null;
  const Probe = () => {
    ctx = useAppShell();
    return null;
  };
  const renderProbe = (): AppCtx => {
    fake.on = true;
    fake.idx = 0;
    try {
      renderToStaticMarkup(createElement(Probe));
    } finally {
      fake.on = false;
    }
    return ctx as unknown as AppCtx;
  };
  await renderProbe().refreshAll();
  return renderProbe();
}

function renderApp(ctx: AppCtx): string {
  fake.ctx = ctx;
  try {
    return renderToStaticMarkup(createElement(App));
  } finally {
    fake.ctx = null;
  }
}

describe("boot: Promise.allSettled, one command at a time", () => {
  it("one rejected invoke leaves the rest rendered and no Bridge error status", async () => {
    const ctx = await bootWith(["list_crash_events"]);
    expect(fake.calls).toContain("list_crash_events");
    // Everything that succeeded landed.
    expect(ctx.settings).not.toBeNull();
    expect(ctx.insights).toEqual(insightsRow);
    expect(ctx.status.message).toBe("Ready");
    expect(String(ctx.status.message)).not.toMatch(/Bridge error/);
    // Only the full-screen trigger (get_settings) feeds bootError.
    expect(ctx.bootError).toBeNull();
    // The failure is still recorded — against the one command that failed.
    expect(ctx.bootErrors.list_crash_events).toContain("boom");
    expect(Object.keys(ctx.bootErrors)).toEqual(["list_crash_events"]);
    const html = renderApp(ctx);
    expect(html).not.toContain("Bridge error");
    expect(html).not.toContain("UI failed to load backend bridge");
  });

  it("get_settings rejection shows the full-screen retry", async () => {
    const ctx = await bootWith(["get_settings"]);
    expect(ctx.settings).toBeNull();
    expect(ctx.bootError).toContain("get_settings: boom");
    const html = renderApp(ctx);
    expect(html).toContain("UI failed to load backend bridge");
    expect(html).toContain("get_settings: boom");
    expect(html).toContain("Retry");
  });

  it("a failed list_dictionary is the Dictionary view's own inline error + retry", async () => {
    const ctx = await bootWith(["list_dictionary"]);
    expect(ctx.settings).not.toBeNull();
    expect(ctx.bootError).toBeNull();
    expect(String(ctx.status.message)).not.toMatch(/Bridge error/);
    const html = renderToStaticMarkup(
      createElement(AppContext.Provider, { value: ctx }, createElement(Dictionary)),
    );
    expect(html).toContain('data-error-state="dictionary"');
    expect(html).toContain("list_dictionary: boom");
    // ...and the shell around it is not the full-screen bridge failure.
    expect(renderApp(ctx)).not.toContain("UI failed to load backend bridge");
  });
});
