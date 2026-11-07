// yap-1106-news-prefs-onboarding — the three surfaces, rendered statically.
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));

import { NewsStepView } from "./NewsOnboardingStep";
import { NewsHomeCardView } from "./NewsHomeCard";
import { NewsSettingsView } from "../views/settings/News";
import { EMPTY_PREFS, NEWS_PRIVACY_LINE, TOPIC_CHIPS, type NewsView } from "./newsPrefs";

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
    expect(html).toMatch(/<button class="primary" disabled=""[^>]*data-news-save/);
    expect(html).not.toMatch(/disabled=""[^>]*data-news-skip/);
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
