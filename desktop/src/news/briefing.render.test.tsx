// yap-1106-news-home-briefing — the briefing card, rendered statically.
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));

import { NewsBriefingView } from "./NewsBriefing";
import { buildBriefing, type CachedStory } from "./briefing";
import { duoAt, duoTurns } from "./duo";

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

describe("yap-1106-news-duo-delivery card", () => {
  const turns = duoTurns(briefing, ["NVDA"], "professional");
  const T0 = 1_000_000;

  it("news_duo_card_shows_cinder_bringing_the_paper_first", () => {
    const html = render({ duo: { frame: duoAt(T0, T0, false, turns.length), turns }, onSkipDuo: noop });
    expect(html).toContain('data-news-duo="arriving"');
    expect(html).toContain("data-news-duo-skip");
    // The finished summary lines wait until the delivery is over.
    expect(html).not.toContain('data-news-says="cinder"');
  });

  it("news_duo_card_takes_turns_cinder_names_yappy_explains", () => {
    const first = render({ duo: { frame: duoAt(T0, T0, true, turns.length), turns } });
    expect(first).toContain('data-news-says="cinder"');
    expect(first).toContain('data-news-turn="0"');
    expect(first).toContain("Dev news!");
    expect(first).toContain('data-news-talking="yes"');
    expect(first).toContain('data-news-story-current="yes"');
    expect(first).toContain("story 1 of 3");
    const second = render({ duo: { frame: { ...duoAt(T0, T0, true, turns.length), turn: 1, speaker: "yappy" }, turns } });
    expect(second).toContain('data-news-says="yappy"');
    expect(second).toContain("worth a look.");
    const third = render({ duo: { frame: { ...duoAt(T0, T0, true, turns.length), turn: 3, speaker: "yappy" }, turns } });
    // No explained line for story b: Yappy reads the headline in the chosen voice.
    expect(third).toContain("In brief: Headline b");
  });

  it("news_duo_card_done_shows_the_summary_lines", () => {
    const html = render({ duo: { frame: duoAt(null, T0, false, turns.length), turns } });
    expect(html).toContain('data-news-says="cinder"');
    expect(html).not.toContain("data-news-duo-skip");
    expect(html).not.toContain("data-news-turn=");
  });
});
