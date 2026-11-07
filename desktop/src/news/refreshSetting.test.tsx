// yap-1106-news-refresh-setting — Settings → Yappy News → "How often".
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));

import { invoke } from "@tauri-apps/api/core";
import { NewsSettingsView } from "../views/settings/News";
import {
  CADENCE_OPTIONS,
  DEFAULT_CADENCE,
  EMPTY_PREFS,
  REFRESH_FLOOR_MS,
  cadenceOf,
  nextTapAllowedAt,
  refreshNews,
  saveCadence,
  type NewsView,
} from "./newsPrefs";

const noop = () => undefined;
const view = (over: Partial<NewsView> = {}): NewsView => ({
  ...EMPTY_PREFS,
  topics: ["tech"],
  cloudConnected: true,
  active: true,
  offer: "accepted",
  onboardingStepDue: false,
  homeCardDue: false,
  cloudConnectedAtMs: 1,
  lastRefreshMs: null,
  ...over,
});

function render(v: NewsView, extra: Partial<Parameters<typeof NewsSettingsView>[0]> = {}) {
  return renderToStaticMarkup(
    <NewsSettingsView
      view={v}
      draft={v}
      saving={false}
      onChange={noop}
      onSave={noop}
      onRevert={noop}
      onConnect={noop}
      onCadence={noop}
      onRefresh={noop}
      {...extra}
    />,
  );
}

describe("yap-1106-news-refresh-setting", () => {
  it("news_refresh_setting_offers_the_four_choices_in_order_with_twice_a_day_default", () => {
    expect(CADENCE_OPTIONS.map((o) => o.label)).toEqual([
      "Twice a day",
      "Every few hours",
      "Only when I open Home",
      "Only when I tap refresh",
    ]);
    expect(DEFAULT_CADENCE).toBe("twice_a_day");
    expect(cadenceOf(view())).toBe("twice_a_day");
    expect(cadenceOf(view({ refreshCadence: "on_tap" }))).toBe("on_tap");
    expect(cadenceOf(null)).toBe("twice_a_day");
    const html = render(view());
    expect(html).toContain("How often");
    for (const o of CADENCE_OPTIONS) expect(html).toContain(`value="${o.id}"`);
    expect(html).toContain('data-news-cadence="twice_a_day"');
    // React writes `checked` before `value`, so match the radio by both, in any order.
    expect(html).toMatch(/<input(?=[^>]*value="twice_a_day")(?=[^>]*checked="")[^>]*>/);
    expect(html).not.toMatch(/<input(?=[^>]*value="every_few_hours")(?=[^>]*checked="")[^>]*>/);
  });

  it("news_refresh_setting_shows_the_last_refresh_time_next_to_it", () => {
    const html = render(view({ refreshCadence: "every_few_hours", lastRefreshMs: Date.now() - 45 * 60_000 }));
    const fieldset = html.slice(html.indexOf('data-news-cadence="every_few_hours"'));
    const end = fieldset.indexOf("</fieldset>");
    const inside = fieldset.slice(0, end);
    expect(inside).toContain("Last refreshed 45 min ago.");
    expect(inside).toContain("data-news-refresh");
    expect(inside).toContain("Never more often than every 30 minutes");
  });

  it("news_refresh_now_waits_out_the_30_minute_floor", () => {
    const now = 1_000_000_000_000;
    expect(nextTapAllowedAt(null, now)).toBeNull();
    expect(nextTapAllowedAt(now - 10 * 60_000, now)).toBe(now - 10 * 60_000 + REFRESH_FLOOR_MS);
    expect(nextTapAllowedAt(now - REFRESH_FLOOR_MS, now)).toBeNull();
    const recent = render(view({ lastRefreshMs: Date.now() - 5 * 60_000 }));
    expect(recent).toMatch(/<button[^>]*disabled=""[^>]*data-news-refresh/);
    const stale = render(view({ lastRefreshMs: Date.now() - 2 * 3_600_000 }));
    expect(stale).not.toMatch(/<button[^>]*disabled=""[^>]*data-news-refresh/);
  });

  it("news_refresh_setting_is_disabled_without_the_cloud", () => {
    const html = render(view({ cloudConnected: false, active: false }));
    expect(html).toMatch(/value="twice_a_day" checked="" disabled=""|disabled=""[^>]*value="twice_a_day"/);
  });

  it("news_refresh_setting_calls_the_backend_commands", async () => {
    await saveCadence("on_home_open");
    expect(invoke).toHaveBeenCalledWith("set_news_cadence", { cadence: "on_home_open" });
    await refreshNews("tap");
    expect(invoke).toHaveBeenCalledWith("news_refresh", { trigger: "tap" });
  });
});
