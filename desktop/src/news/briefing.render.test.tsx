// yap-1106-news-home-briefing — the briefing card, rendered statically.
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));

import { invoke } from "@tauri-apps/api/core";
import appCss from "../App.css?raw";
import tokensCss from "../tokens.css?raw";
import { DUO_ART, DUO_AT, DUO_SCALE, NewsBriefingView, PAIR_BOX, cachedBriefing, composeBriefing, paintDuo, paperLabel } from "./NewsBriefing";
import { buildBriefing, quietNow, type CachedStory } from "./briefing";
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
    // One duo canvas holds both friends, as on the approved page (news.html §02 #duo).
    expect(html).toContain('data-news-sprite="duo"');
    expect(html).toContain('data-news-pair="cinder yappy"');
    expect(html.match(/<canvas/g)?.length).toBe(1);
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
    expect(first).toContain('data-news-talking="cinder"');
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

describe("fix-l3b3-2 briefing makes no cloud call when quiet", () => {
  const cachedRows = [story("a", 4), story("b", 3), story("c", 2), story("d", 1)];
  const calm = { meetingRecording: false, screenSharing: false, doNotDisturb: false };
  const quietCases = [
    ["a meeting", { ...calm, meetingRecording: true }],
    ["a screen share", { ...calm, screenSharing: true }],
    ["Do Not Disturb", { ...calm, doNotDisturb: true }],
  ] as const;

  function mockBackend() {
    const calls: string[] = [];
    const impl = async (cmd: string): Promise<unknown> => {
      calls.push(cmd);
      if (cmd === "news_briefing_cached") return cachedRows;
      if (cmd === "news_explain_top") {
        return { lines: [{ urlKey: "a", url: "https://example.com/a", headline: "Headline a", line: "worth a look.", explained: true }], fallback: "none" };
      }
      return null;
    };
    vi.mocked(invoke).mockImplementation(impl as unknown as typeof invoke);
    return calls;
  }

  for (const [name, signals] of quietCases) {
    it(`news_briefing_cache_delivery_never_explains_during_${name.replace(/\W+/g, "_")}`, async () => {
      const calls = mockBackend();
      const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
      const b = await cachedBriefing(quietNow(signals), "professional", NOW);
      expect(calls).toContain("news_briefing_cached");
      expect(calls).not.toContain("news_explain_top");
      expect(info).toHaveBeenCalledWith("news briefing explain_held reason=quiet");
      info.mockRestore();
      expect(b).not.toBeNull();
      // The headlines stand alone: up to 3 stories, each with its source link.
      expect(b!.stories).toHaveLength(3);
      const html = render({ briefing: b, movers: [] });
      expect(html).toContain('data-news-stories="3"');
      expect((html.match(/data-news-source=/g) ?? []).length).toBe(3);
      expect(html).toContain("Headline a");
      expect(html).not.toContain("worth a look.");
    });
  }

  it("news_briefing_refresh_delivery_never_explains_while_quiet", async () => {
    const calls = mockBackend();
    const b = await composeBriefing(cachedRows, quietNow({ ...calm, meetingRecording: true }), undefined, NOW);
    expect(calls).not.toContain("news_explain_top");
    expect(b.stories).toHaveLength(3);
  });

  it("news_briefing_explains_when_nothing_is_quiet", async () => {
    const calls = mockBackend();
    const b = await cachedBriefing(quietNow(calm), "professional", NOW);
    expect(calls).toEqual(["news_briefing_cached", "news_explain_top"]);
    expect(b!.stories).toHaveLength(3);
    expect(render({ briefing: b, movers: [] })).toContain("worth a look.");
  });
});

describe("yap-1107-parity-news — the pair matches the approved card's duo canvas", () => {
  it("one duo canvas, 104x44 art shown at 208x88, a whole-number scale", () => {
    // news.html §02: var DW = 104, DH = 44; dv.style.width = DW * 2 + "px".
    expect(DUO_ART).toEqual({ w: 104, h: 44 });
    expect(DUO_SCALE).toBe(2);
    expect(PAIR_BOX).toEqual({ w: 208, h: 88 });
    expect(Number.isInteger(PAIR_BOX.w / DUO_ART.w) && Number.isInteger(PAIR_BOX.h / DUO_ART.h)).toBe(true);
  });

  it("friends stand where the page stamps them, fully inside the art canvas", () => {
    // FR.stamp(o, "yappy", 2, 6, ...); FR.stamp(o, "cinder", 54, 2, ...) on a 104x44 canvas; art boxes 40x36 and 46x42.
    expect(DUO_AT).toEqual({ yappy: { x: 2, y: 6 }, cinder: { x: 54, y: 2 } });
    expect(DUO_AT.yappy.x + 40 <= DUO_ART.w && DUO_AT.yappy.y + 36 <= DUO_ART.h).toBe(true);
    expect(DUO_AT.cinder.x + 46 <= DUO_ART.w && DUO_AT.cinder.y + 42 <= DUO_ART.h).toBe(true);
  });

  it("paintDuo fails soft (false + one log line) when the canvas has no 2d context", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const fake = { width: 0, height: 0, style: {} as Record<string, string>, getContext: () => null } as unknown as HTMLCanvasElement;
    expect(paintDuo(fake, null, false, 2)).toBe(false);
    expect(fake.width).toBe(416);
    expect(fake.height).toBe(176);
    expect(warn).toHaveBeenCalledWith("sprites paint-failed name=news-duo reason=no-2d-context");
    warn.mockRestore();
  });
});

describe("yap-1107-parity-news — the header matches the approved .bhead (batch-1 bounce)", () => {
  it("the amber tag names the paper: morning from 6 AM until 4 PM, evening otherwise, one width", () => {
    expect(paperLabel(new Date(2026, 10, 7, 8, 2).getTime())).toBe("morning paper");
    expect(paperLabel(new Date(2026, 10, 7, 6, 0).getTime())).toBe("morning paper");
    expect(paperLabel(new Date(2026, 10, 7, 15, 59).getTime())).toBe("morning paper");
    expect(paperLabel(new Date(2026, 10, 7, 16, 0).getTime())).toBe("evening paper");
    expect(paperLabel(new Date(2026, 10, 7, 23, 30).getTime())).toBe("evening paper");
    expect(paperLabel(new Date(2026, 10, 7, 5, 59).getTime())).toBe("evening paper");
    // both labels are 13 characters, like the page's "sample stories" slot keeps one width
    expect("morning paper".length).toBe("evening paper".length);
  });

  it("the time column carries the clock and the amber .q tag, as on news.html §02", () => {
    const html = render();
    expect(html).toContain('class="news-briefing-clock"');
    expect(html).toMatch(/class="news-briefing-paper" data-news-paper="true">morning paper<\/span>/);
  });

  it("the settled bubble is one paragraph holding both speakers, like the page's .bsay", () => {
    const html = render();
    const said = html.match(/<p data-news-said="pair">([\s\S]*?)<\/p>/);
    expect(said).not.toBeNull();
    expect(said![1]).toContain('data-news-says="cinder"');
    expect(said![1]).toContain('data-news-says="yappy"');
    expect(said![1]).toContain("<b>Cinder:</b>");
    expect(said![1]).toContain("<b>Yappy:</b>");
  });

  it("the tag, bubble and title rules use the page's sizes and the news tokens", () => {
    const app = appCss;
    const tokens = tokensCss;
    expect(tokens).toContain("--news-quote-ink: #a06a00;");
    expect(tokens).toContain("--news-quote-bg: #fff4d6;");
    const rule = (sel: string) => {
      const at = app.indexOf(sel + " {");
      expect(at, sel).toBeGreaterThan(-1);
      return app.slice(at, app.indexOf("}", at));
    };
    const paper = rule(".news-briefing .news-briefing-paper");
    expect(paper).toContain("font: 500 11px var(--font-mono);");
    expect(paper).toContain("color: var(--news-quote-ink);");
    expect(paper).toContain("background: var(--news-quote-bg);");
    expect(paper).toContain("border-radius: 6px;");
    expect(paper).toContain("padding: 2px 6px;");
    expect(rule(".news-briefing .news-briefing-says")).toContain("line-height: 1.55;");
    expect(rule(".news-briefing .news-briefing-says h3")).toContain("font: inherit;");
    expect(rule(".news-briefing .news-briefing-clock")).toContain("min-width: 8ch;");
  });
});
