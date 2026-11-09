// yap-1106-news-home-briefing — the briefing's pure rules.
import { describe, expect, it } from "vitest";
import {
  BRIEFING_HIDDEN_KEY,
  BRIEFING_MAX,
  BRIEFING_STORE_KEY,
  QUOTE_CHECK_MS,
  buildBriefing,
  cadenceLine,
  hiddenToday,
  hideToday,
  isNewBriefing,
  loadStoredBriefing,
  localDay,
  pairLines,
  quietNow,
  quoteLine,
  ranksHigher,
  saveStoredBriefing,
  sourceLabel,
  topStories,
  topicTag,
  type CachedStory,
} from "./briefing";

const story = (key: string, score: number, over: Partial<CachedStory> = {}): CachedStory => ({
  source: "google_news",
  topic: "business",
  title: `Headline ${key}`,
  url: `https://example.com/${key}`,
  outlet: "The Paper",
  outlets: 1,
  points: null,
  score,
  urlKey: key,
  ...over,
});

function memStore() {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v), m };
}

const NOW = new Date(2026, 10, 7, 8, 2).getTime();

describe("yap-1106-news-home-briefing", () => {
  it("home_briefing_shows_up_to_three_stories_best_first", () => {
    const top = topStories([story("a", 1), story("b", 5), story("c", 3), story("d", 4), { bogus: true }]);
    expect(BRIEFING_MAX).toBe(3);
    expect(top.map((s) => s.urlKey)).toEqual(["b", "d", "c"]);
    expect(topStories(null)).toEqual([]);
    const b = buildBriefing([story("a", 1), story("b", 2), story("c", 3), story("d", 4)], null, NOW);
    expect(b.stories).toHaveLength(3);
  });

  it("home_briefing_uses_yappys_line_only_when_explained", () => {
    const b = buildBriefing(
      [story("a", 3), story("b", 2)],
      [
        { urlKey: "a", url: "https://example.com/a", headline: "Headline a", line: "cheaper to borrow again.", explained: true },
        { urlKey: "b", url: "https://example.com/b", headline: "Headline b", line: "Headline b", explained: false },
      ],
      NOW,
    );
    expect(b.stories[0].line).toBe("cheaper to borrow again.");
    expect(b.stories[1].line).toBe("");
    expect(b.stories.every((s) => s.url.startsWith("https://"))).toBe(true);
  });

  it("home_briefing_refresh_replaces_only_when_something_new_ranks_higher", () => {
    const morning = buildBriefing([story("a", 9), story("b", 7), story("c", 5)], null, NOW);
    expect(ranksHigher(null, [story("a", 1)])).toBe(true);
    expect(ranksHigher(morning, [])).toBe(false);
    // The same stories again: no repeat briefing.
    expect(ranksHigher(morning, [story("a", 9), story("b", 7), story("c", 5)])).toBe(false);
    // Something new, but below every morning story: keep the morning card.
    expect(ranksHigher(morning, [story("a", 9), story("b", 7), story("z", 4)])).toBe(false);
    // Something new that outranks a morning story: a new briefing.
    expect(ranksHigher(morning, [story("a", 9), story("z", 8), story("b", 7)])).toBe(true);
    // A card with room left takes any new story.
    const short = buildBriefing([story("a", 9)], null, NOW);
    expect(ranksHigher(short, [story("a", 9), story("y", 1)])).toBe(true);
  });

  it("home_briefing_carries_the_why_this_story_line", () => {
    // yap-1107-news-no-off-topic-stories — the backend's line rides through.
    const [w] = buildBriefing([story("w", 1, { why: "From The Verge (Tech) · matches Tech" })], null, NOW).stories;
    expect(w.why).toBe("From The Verge (Tech) · matches Tech");
    const [old] = buildBriefing([story("o", 1)], null, NOW).stories;
    expect(old.why ?? null).toBeNull();
  });

  it("home_briefing_every_story_names_its_source_and_market_quotes_are_delayed", () => {
    const [hn] = buildBriefing([story("h", 1, { source: "hacker_news", topic: "developer", points: 412, outlet: null })], null, NOW).stories;
    expect(sourceLabel(hn, [])).toBe("Hacker News · 412 points");
    expect(topicTag(hn, [])).toBe("DEV");
    const [g] = buildBriefing([story("g", 1, { outlets: 3 })], null, NOW).stories;
    expect(sourceLabel(g, [])).toBe("Google News · 3 outlets");
    expect(topicTag(g, [])).toBe("BIZ");
    const [m] = buildBriefing([story("m", 1, { topic: "NVDA" })], null, NOW).stories;
    expect(topicTag(m, ["NVDA"])).toBe("STOCK");
    expect(sourceLabel(m, ["NVDA"])).toContain("delayed");
    expect(quoteLine({ symbol: "NVDA", price: 1, changePct: 3.14, delayed: true })).toBe("NVDA up 3.1% · delayed");
    expect(quoteLine({ symbol: "TSLA", price: 1, changePct: -4, delayed: false })).toBe("TSLA down 4.0% · delayed");
  });

  it("home_briefing_tickers_are_checked_every_30_minutes", () => {
    expect(QUOTE_CHECK_MS).toBe(30 * 60_000);
  });

  it("home_briefing_holds_during_meetings_share_and_dnd", () => {
    const calm = { meetingRecording: false, screenSharing: false, doNotDisturb: false };
    expect(quietNow(calm)).toBe(false);
    expect(quietNow({ ...calm, meetingRecording: true })).toBe(true);
    expect(quietNow({ ...calm, screenSharing: true })).toBe(true);
    expect(quietNow({ ...calm, doNotDisturb: true })).toBe(true);
  });

  it("home_briefing_cinder_flies_only_for_a_new_briefing", () => {
    const a = buildBriefing([story("a", 1)], null, NOW);
    const b = buildBriefing([story("b", 1)], null, NOW + 1);
    expect(isNewBriefing(null, a)).toBe(true);
    expect(isNewBriefing(a, a)).toBe(false);
    expect(isNewBriefing(a, b)).toBe(true);
    expect(isNewBriefing(a, buildBriefing([], null, NOW + 2))).toBe(false);
  });

  it("home_briefing_header_lines_cinder_names_yappy_explains", () => {
    const b = buildBriefing([story("a", 3, { topic: "NVDA" }), story("b", 2), story("c", 1)], null, NOW);
    const l = pairLines(b, ["NVDA"]);
    expect(l.cinder).toBe("three today, one about your stocks!");
    expect(l.yappy).toBe("headlines below, links to every story.");
    expect(cadenceLine("twice_a_day")).toBe("Updated twice a day");
  });

  it("home_briefing_cache_lasts_the_day_and_hide_lasts_today", () => {
    const st = memStore();
    const b = buildBriefing([story("a", 1)], null, NOW);
    saveStoredBriefing(st, b);
    expect(st.m.has(BRIEFING_STORE_KEY)).toBe(true);
    expect(loadStoredBriefing(st, NOW + 60_000)?.id).toBe(b.id);
    expect(loadStoredBriefing(st, NOW + 24 * 3600_000)).toBeNull();
    st.setItem(BRIEFING_STORE_KEY, "{not json");
    expect(loadStoredBriefing(st, NOW)).toBeNull();
    expect(hiddenToday(st, NOW)).toBe(false);
    hideToday(st, NOW);
    expect(st.m.get(BRIEFING_HIDDEN_KEY)).toBe(localDay(NOW));
    expect(hiddenToday(st, NOW)).toBe(true);
    expect(hiddenToday(st, NOW + 24 * 3600_000)).toBe(false);
  });
});

describe("fix-l3b3-4-news-new-chips-fetch-and-match", () => {
  it("news_new_chips_each_have_their_own_tag", () => {
    const want: Record<string, string> = { ai: "AI", design: "DESIGN", startups: "STARTUP", crypto: "CRYPTO", local: "LOCAL" };
    for (const [topic, tag] of Object.entries(want)) {
      expect(topicTag({ topic, source: "google_news" }, [])).toBe(tag);
    }
    // Legacy picks keep their tags.
    expect(topicTag({ topic: "tech", source: "google_news" }, [])).toBe("TECH");
    expect(topicTag({ topic: "world", source: "google_news" }, [])).toBe("WORLD");
    expect(topicTag({ topic: "culture", source: "google_news" }, [])).toBe("CULTURE");
  });
});
