// yap-1106-news-home-briefing — the briefing card, rendered statically.
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));

import { NewsBriefingView } from "./NewsBriefing";
import { buildBriefing, type CachedStory } from "./briefing";

const noop = () => undefined;
const story = (key: string, score: number, over: Partial<CachedStory> = {}): CachedStory => ({
  source: "google_news",
  topic: "business",
  title: `Headline ${key}`,
  url: `https://example.com/${key}`,
  outlet: null,
  outlets: 3,
  score,
  urlKey: key,
  ...over,
});
const NOW = new Date(2026, 10, 7, 8, 2).getTime();
const briefing = buildBriefing(
  [story("a", 4, { source: "hacker_news", topic: "developer", points: 412 }), story("b", 3, { topic: "NVDA" }), story("c", 2), story("d", 1)],
  [{ urlKey: "a", url: "https://example.com/a", headline: "Headline a", line: "worth a look.", explained: true }],
  NOW,
);

function render(over: Partial<Parameters<typeof NewsBriefingView>[0]> = {}) {
  return renderToStaticMarkup(
    <NewsBriefingView
      phase="ready"
      briefing={briefing}
      movers={[{ symbol: "NVDA", price: 1, changePct: 3.1, delayed: true }]}
      tickers={["NVDA"]}
      topics={["developer", "business"]}
      cadence="twice_a_day"
      onOpen={noop}
      onHide={noop}
      onRetry={noop}
      onEditTopics={noop}
      {...over}
    />,
  );
}

describe("yap-1106-news-home-briefing card", () => {
  it("home_briefing_card_renders_up_to_three_stories_with_source_links", () => {
    const html = render();
    expect(html).toContain('data-news-stories="3"');
    expect((html.match(/data-news-story=/g) ?? []).length).toBe(3);
    expect((html.match(/data-news-source=/g) ?? []).length).toBe(3);
    expect(html).toContain("Hacker News · 412 points · open ↗");
    expect(html).toContain("worth a look.");
    expect(html).not.toContain("Headline d");
  });

  it("home_briefing_card_marks_quotes_delayed", () => {
    const html = render();
    expect(html).toContain("NVDA up 3.1% · delayed");
    expect(html).toContain("quotes delayed");
  });

  it("home_briefing_card_has_the_pair_header_topics_and_hide_for_today", () => {
    const html = render();
    expect(html).toContain('data-news-sprite="cinder"');
    expect(html).toContain('data-news-sprite="yappy"');
    expect(html).toContain('data-news-says="cinder"');
    expect(html).toContain("Updated twice a day");
    expect(html).toContain("Hide for today");
    expect(html).toContain("Developer");
  });

  it("home_briefing_card_empty_and_error_states", () => {
    const empty = render({ phase: "empty", briefing: null, movers: [] });
    expect(empty).toContain("data-news-briefing-empty");
    const err = render({ phase: "error", briefing: null, movers: [] });
    expect(err).toContain("data-news-briefing-retry");
    expect(err).toContain('role="alert"');
  });

  it("news_never_goes_to_the_pill", () => {
    const src = import.meta.glob(["./NewsBriefing.tsx", "./briefing.ts"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;
    expect(Object.keys(src)).toHaveLength(2);
    for (const [file, text] of Object.entries(src)) {
      expect(text, file).not.toMatch(/from "\.\.\/pill\//);
      expect(text, file).not.toMatch(/\bemit\(|emitTo\(|pill_/);
    }
  });
});
