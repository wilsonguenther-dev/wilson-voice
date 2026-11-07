import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));

import { invoke } from "@tauri-apps/api/core";
import {
  EMPTY_PREFS,
  MAX_TICKERS,
  addCustom,
  addTicker,
  dismissNews,
  hasAnyTopic,
  homeCardDue,
  lastRefreshLine,
  newsStepDue,
  normalizeTicker,
  prefsOf,
  removeTicker,
  samePrefs,
  saveNews,
  statusLine,
  toggleTopic,
  topicLabels,
  type NewsView,
} from "./newsPrefs";

const view = (over: Partial<NewsView> = {}): NewsView => ({
  ...EMPTY_PREFS,
  cloudConnected: true,
  active: false,
  offer: "unseen",
  onboardingStepDue: true,
  homeCardDue: true,
  cloudConnectedAtMs: 1,
  lastRefreshMs: null,
  ...over,
});

describe("yap-1106 news prefs — where the offer shows", () => {
  it("news_step_and_card_need_the_cloud", () => {
    expect(newsStepDue(view())).toBe(true);
    expect(homeCardDue(view())).toBe(true);
    expect(newsStepDue(view({ cloudConnected: false }))).toBe(false);
    expect(homeCardDue(view({ cloudConnected: false }))).toBe(false);
    expect(newsStepDue(null)).toBe(false);
    expect(homeCardDue(null)).toBe(false);
  });
  it("news_card_is_one_time_once_answered", () => {
    expect(homeCardDue(view({ offer: "declined", homeCardDue: false }))).toBe(false);
    expect(homeCardDue(view({ offer: "accepted", homeCardDue: false }))).toBe(false);
  });
});

describe("yap-1106 news prefs — the picker's edits", () => {
  it("news_topic_chips_toggle", () => {
    const on = toggleTopic(EMPTY_PREFS, "developer");
    expect(on.topics).toEqual(["developer"]);
    expect(toggleTopic(on, "developer").topics).toEqual([]);
  });
  it("news_tickers_normalize_and_validate", () => {
    expect(normalizeTicker(" $nvda ")).toBe("NVDA");
    expect(normalizeTicker("brk.b")).toBe("BRK.B");
    expect(normalizeTicker("1abc")).toBeNull();
    const r = addTicker(EMPTY_PREFS, "aapl");
    expect(r.ok && r.prefs.tickers).toEqual(["AAPL"]);
    const bad = addTicker(EMPTY_PREFS, "no way");
    expect(bad.ok).toBe(false);
    if (!bad.ok) expect(bad.error).toMatch(/doesn't look like a ticker\.$/);
    const full = { ...EMPTY_PREFS, tickers: Array.from({ length: MAX_TICKERS }, (_, i) => `T${i}`) };
    expect(addTicker(full, "ZZ").ok).toBe(false);
    expect(removeTicker({ ...EMPTY_PREFS, tickers: ["AAPL", "NVDA"] }, "AAPL").tickers).toEqual(["NVDA"]);
  });
  it("news_custom_topics_trim_and_dedupe", () => {
    const r = addCustom(EMPTY_PREFS, "  Austin   real estate ");
    expect(r.ok && r.prefs.custom).toEqual(["Austin real estate"]);
    if (r.ok) {
      const again = addCustom(r.prefs, "austin REAL estate");
      expect(again.ok && again.prefs.custom).toEqual(["Austin real estate"]);
    }
    expect(addCustom(EMPTY_PREFS, "x").ok).toBe(false);
  });
  it("news_labels_and_dirty_check", () => {
    const p = { ...EMPTY_PREFS, topics: ["stocks", "developer"], tickers: ["NVDA"], custom: ["Tauri releases"] };
    expect(topicLabels(p)).toEqual(["Developer", "Stocks", "NVDA", "Tauri releases"]);
    expect(hasAnyTopic(p)).toBe(true);
    expect(hasAnyTopic(EMPTY_PREFS)).toBe(false);
    expect(samePrefs(p, prefsOf(view(p)))).toBe(true);
    expect(samePrefs(p, { ...p, enabled: false })).toBe(false);
  });
});

describe("yap-1106 news prefs — Settings lines", () => {
  it("news_status_line_on_off", () => {
    expect(statusLine(view({ cloudConnected: false }))).toMatch(/needs the cloud option/);
    expect(statusLine(view({ enabled: false, topics: ["tech"] }))).toMatch(/^Off\./);
    expect(statusLine(view())).toMatch(/at least one topic/);
    expect(statusLine(view({ topics: ["tech"], active: true }))).toBe("On.");
  });
  it("news_last_refresh_line", () => {
    const now = new Date(2026, 10, 7, 9, 0).getTime();
    expect(lastRefreshLine(null, now)).toBe("Not refreshed yet.");
    expect(lastRefreshLine(now - 10_000, now)).toBe("Last refreshed just now.");
    expect(lastRefreshLine(now - 12 * 60_000, now)).toBe("Last refreshed 12 min ago.");
    expect(lastRefreshLine(now - 3 * 3_600_000, now)).toMatch(/^Last refreshed today at /);
    expect(lastRefreshLine(now - 3 * 86_400_000, now)).not.toMatch(/today/);
  });
});

describe("yap-1106 news prefs — the commands", () => {
  it("news_save_and_dismiss_call_the_one_record", async () => {
    await saveNews({ ...EMPTY_PREFS, topics: ["tech"] }, "settings");
    expect(invoke).toHaveBeenCalledWith("set_news_prefs", {
      prefs: { ...EMPTY_PREFS, topics: ["tech"] },
      source: "settings",
    });
    await dismissNews("home");
    expect(invoke).toHaveBeenCalledWith("dismiss_news_offer", { source: "home" });
  });
});
