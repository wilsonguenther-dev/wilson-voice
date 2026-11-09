// yap-1106-news-prefs-onboarding — the three surfaces, rendered statically.
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));

import { NEWS_STEP_HEADING, NEWS_STEP_LEDE, NewsStepView } from "./NewsOnboardingStep";
import { NewsHomeCardView } from "./NewsHomeCard";
import { NewsSettingsView } from "../views/settings/News";
import { EMPTY_PREFS, NEWS_PRIVACY_LINE, TOPIC_CHIPS, topicLabel, topicLabels, watchCloudRoute, type NewsView } from "./newsPrefs";
import { CLOUD_ROUTE_CHANGED_EVENT } from "../meetings/consent";

const noop = () => undefined;
const view = (over: Partial<NewsView> = {}): NewsView => ({
  ...EMPTY_PREFS,
  cloudConnected: true,
  active: false,
  offer: "accepted",
  onboardingStepDue: false,
  homeCardDue: false,
  cloudConnectedAtMs: 1,
  lastRefreshMs: null,
  ...over,
});

describe("yap-1106 onboarding step", () => {
  it("news_onboarding_step_offers_chips_tickers_custom_and_not_now", () => {
    const html = renderToStaticMarkup(
      <NewsStepView value={EMPTY_PREFS} onChange={noop} busy={false} error={null} onSkip={noop} onSave={noop} onBack={noop} />,
    );
    for (const c of TOPIC_CHIPS) expect(html).toContain(`data-news-chip="${c.id}"`);
    expect(html).toContain("Tickers you follow");
    expect(html).toContain('aria-label="Add a ticker"');
    expect(html).toContain('aria-label="Add your own topic"');
    expect(html).toContain("Not now");
    expect(html).toContain("Keep me posted");
    // Nothing picked yet: saving is disabled, Not now is not.
    expect(html).toMatch(/<button class="primary news-onb-btn news-onb-btn--primary" disabled=""[^>]*data-news-save/);
    expect(html).not.toMatch(/disabled=""[^>]*data-news-skip/);
  });
});

describe("yap-1107 parity: the onboarding step matches the approved News page §01", () => {
  it("parity_news_onboarding_step_draws_the_approved_layout", () => {
    const picked = { ...EMPTY_PREFS, topics: ["developer", "stocks", "business"], tickers: ["AAPL", "NVDA", "TSLA"] };
    const html = renderToStaticMarkup(
      <NewsStepView value={picked} onChange={noop} busy={false} error={null} onSkip={noop} onSave={noop} onBack={noop} />,
    );
    expect(NEWS_STEP_HEADING).toBe("Want me to keep you posted?");
    expect(html).toContain(NEWS_STEP_HEADING);
    expect(html).toContain(NEWS_STEP_LEDE.replace(/'/g, "&#x27;"));
    expect(html).toContain("Step 4 of 6");
    expect(html).toContain("data-news-step-yappy");
    expect(html).toContain('data-news-variant="onboarding"');
    // The ten chips of the page, in its order.
    expect(TOPIC_CHIPS.map((c) => c.label)).toEqual([
      "Developer", "Stocks", "Business", "AI", "Design", "Startups", "Crypto", "Sports", "Science", "Local",
    ]);
    const order = [...html.matchAll(/data-news-chip="([a-z]+)"/g)].map((m) => m[1]);
    expect(order).toEqual(TOPIC_CHIPS.map((c) => c.id));
    expect((html.match(/aria-pressed="true"/g) || []).length).toBe(3);
    // Ticker tokens sit inside the one-line field, each a labelled remove.
    for (const t of picked.tickers) expect(html).toContain(`aria-label="Remove ${t}"`);
    expect(html).toContain('data-news-field="tickers"');
    expect(html).toContain('data-news-field="custom"');
    // Footer: Not now then Keep me posted; the page draws no Back button.
    expect(html.indexOf("data-news-skip")).toBeLessThan(html.indexOf("data-news-save"));
    expect(html).not.toContain(">Back<");
    expect(html).toMatch(/<button class="primary news-onb-btn news-onb-btn--primary"[^>]*data-news-save/);
  });

  it("parity_news_legacy_chips_keep_their_names", () => {
    expect(topicLabel("tech")).toBe("Tech");
    expect(topicLabel("ai")).toBe("AI");
    expect(topicLabel("mystery")).toBe("mystery");
    expect(topicLabels({ ...EMPTY_PREFS, topics: ["world", "local"] })).toEqual(["Local", "World"]);
  });
});

describe("yap-1106 Home card", () => {
  it("news_home_card_offers_then_opens_the_same_picker", () => {
    const closed = renderToStaticMarkup(
      <NewsHomeCardView open={false} value={EMPTY_PREFS} busy={false} onOpen={noop} onChange={noop} onSkip={noop} onSave={noop} />,
    );
    expect(closed).toContain("data-news-home-card");
    expect(closed).toContain("Pick topics");
    expect(closed).toContain("Not now");
    expect(closed).not.toContain("data-news-interests");
    const open = renderToStaticMarkup(
      <NewsHomeCardView open value={EMPTY_PREFS} busy={false} onOpen={noop} onChange={noop} onSkip={noop} onSave={noop} />,
    );
    expect(open).toContain("data-news-interests");
    expect(open).toContain("Keep me posted");
  });
});

describe("yap-1106 Settings → Yappy News", () => {
  it("news_settings_shows_on_off_topics_and_last_refresh", () => {
    const v = view({ active: true, topics: ["developer"], tickers: ["NVDA"], custom: ["Tauri releases"], lastRefreshMs: Date.now() - 5 * 60_000 });
    const html = renderToStaticMarkup(
      <NewsSettingsView view={v} draft={v} saving={false} onChange={noop} onSave={noop} onRevert={noop} onConnect={noop} />,
    );
    expect(html).toContain('data-settings-pane="news"');
    expect(html).toContain('data-news-status="on"');
    expect(html).toContain("Developer, NVDA, Tauri releases");
    expect(html).toContain("Last refreshed 5 min ago.");
    expect(html).toContain(NEWS_PRIVACY_LINE);
    expect(html).not.toContain("Connect it in Advanced");
  });
  it("news_settings_cloud_off_is_off_with_one_way_to_connect", () => {
    const v = view({ cloudConnected: false });
    const html = renderToStaticMarkup(
      <NewsSettingsView view={v} draft={v} saving={false} onChange={noop} onSave={noop} onRevert={noop} onConnect={noop} />,
    );
    expect(html).toContain('data-news-status="off"');
    expect(html).toContain("Connect it in Advanced");
    expect(html).toContain("Not refreshed yet.");
    expect(html).toContain("No topics yet.");
  });
});

describe("yap-1107 News honours a saved cloud key", () => {
  const settings = (v: NewsView) =>
    renderToStaticMarkup(
      <NewsSettingsView view={v} draft={v} saving={false} onChange={noop} onSave={noop} onRevert={noop} onConnect={noop} />,
    );

  it("news_settings_cloud_connected_enables_toggle_and_chips", () => {
    // The cloud-key route connected, no topic yet: off only for want of a topic.
    const html = settings(view({ cloudConnected: true, active: false }));
    expect(html).not.toContain("Connect it in Advanced");
    expect(html).not.toContain("needs the cloud option");
    expect(html).toContain("Off until you pick at least one topic.");
    expect(html).not.toMatch(/type="checkbox"[^>]*disabled=""/);
    expect(html).not.toMatch(/<fieldset class="news-group" disabled=""/);
    for (const c of TOPIC_CHIPS) expect(html).toContain(`data-news-chip="${c.id}"`);
  });

  it("news_settings_cloud_off_disables_toggle_and_chips", () => {
    const html = settings(view({ cloudConnected: false }));
    expect(html).toContain("Connect it in Advanced");
    expect(html).toContain("needs the cloud option");
    expect(html).toMatch(/type="checkbox"[^>]*disabled=""/);
    expect(html).toMatch(/<fieldset class="news-group" disabled=""/);
  });

  it("news_settings_key_saved_event_flips_the_view_without_remount", async () => {
    // One mounted view: its state setter is the watcher's callback. The key is
    // saved in Advanced (CloudRoute fires the event); the backend now says on.
    const target = new EventTarget();
    let current = view({ cloudConnected: false, cloudConnectedAtMs: null });
    expect(settings(current)).toContain("Connect it in Advanced");
    const answers = [view({ cloudConnected: true, cloudConnectedAtMs: 42 })];
    const load = vi.fn(async () => answers.shift() ?? current);
    const stop = watchCloudRoute((v) => (current = v), target, load);
    expect(load).not.toHaveBeenCalled();
    target.dispatchEvent(new Event(CLOUD_ROUTE_CHANGED_EVENT));
    await vi.waitFor(() => expect(current.cloudConnected).toBe(true));
    expect(load).toHaveBeenCalledTimes(1);
    const html = settings(current);
    expect(html).not.toContain("Connect it in Advanced");
    expect(html).not.toContain("needs the cloud option");
    expect(html).not.toMatch(/type="checkbox"[^>]*disabled=""/);
    // The key removed: the same view goes back to off.
    answers.push(view({ cloudConnected: false }));
    target.dispatchEvent(new Event(CLOUD_ROUTE_CHANGED_EVENT));
    await vi.waitFor(() => expect(current.cloudConnected).toBe(false));
    expect(settings(current)).toContain("Connect it in Advanced");
    // Unmounted: no more re-reads.
    stop();
    target.dispatchEvent(new Event(CLOUD_ROUTE_CHANGED_EVENT));
    expect(load).toHaveBeenCalledTimes(2);
  });
});
