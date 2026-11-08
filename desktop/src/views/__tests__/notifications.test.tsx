/**
 * WF-17 — Settings › Notifications: three quiet-mode switches (Tips, Feature
 * news, Milestones), each reflecting `mutedPillCategories`, and no switch for
 * errors, permission problems or recovery messages (they always show).
 */
import { describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined), emit: vi.fn(async () => undefined) }));

import { AppContext, type AppCtx } from "../../appShell";
import { SETTINGS_TABS } from "../../appTypes";
import Notifications from "../settings/Notifications";

type State = Record<string, unknown>;
const makeCtx = (state: State): AppCtx =>
  new Proxy(state, {
    get: (t, k: string) => (k in t ? t[k] : () => undefined),
  }) as unknown as AppCtx;
const render = (node: ReactElement, state: State) =>
  renderToStaticMarkup(<AppContext.Provider value={makeCtx(state)}>{node}</AppContext.Provider>);

/** The `<label data-quiet-switch="…">` for `id`, as rendered markup. */
const switchHtml = (html: string, id: string): string => {
  const start = html.indexOf(`data-quiet-switch="${id}"`);
  expect(start, `switch ${id} rendered`).toBeGreaterThan(-1);
  return html.slice(start, html.indexOf("</label>", start));
};

describe("WF-17 Settings › Notifications", () => {
  it("is a Settings tab", () => {
    expect(SETTINGS_TABS.map((t) => t.id)).toContain("notifications");
    expect(SETTINGS_TABS.find((t) => t.id === "notifications")?.label).toBe("Notifications");
  });

  it("settings_notifications_has_three_switches_all_on_by_default", () => {
    const html = render(<Notifications />, { settings: {}, saveSettings: () => undefined });
    // Three category switches, all on, plus yap-1107-reveal-stays-until-read's
    // "Keep the reveal open until I close it" (default off).
    expect((html.match(/data-quiet-switch="/g) ?? []).length).toBe(4);
    for (const id of ["tip", "news", "milestone"]) expect(switchHtml(html, id)).toContain("checked");
    expect(switchHtml(html, "keep-reveal-open")).not.toContain("checked");
    expect(html).toContain("Keep the reveal open until I close it");
    expect(html).toContain("Tips");
    expect(html).toContain("Feature news");
    expect(html).toContain("Milestones");
    // Errors are not a switch: the pane says they always show instead.
    expect(html).not.toMatch(/data-quiet-switch="(error|permission|recovery|status)"/);
    expect(html).toMatch(/Errors, permission problems and recovery messages always show/);
  });

  it("a muted category renders its switch off, the rest stay on", () => {
    const html = render(<Notifications />, {
      settings: { mutedPillCategories: ["milestone", "error"] },
      saveSettings: () => undefined,
    });
    expect(switchHtml(html, "milestone")).not.toContain("checked");
    expect(switchHtml(html, "tip")).toContain("checked");
    expect(switchHtml(html, "news")).toContain("checked");
  });

  it("renders nothing until settings load", () => {
    expect(render(<Notifications />, { settings: null })).toBe("");
  });
});
