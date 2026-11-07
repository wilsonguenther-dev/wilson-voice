// yap-1106-news-duo-delivery — Cinder and Yappy deliver the news together:
// the turn script, the one shared clock, reduce motion and the quiet hold.
import { describe, expect, it } from "vitest";
import { CINDER_FLIGHT_MS } from "../home/studio/studio";
import { buildBriefing, type CachedStory } from "./briefing";
import {
  CINDER_LINE_MAX,
  DUO_CINDER_TURN_MS,
  DUO_HANDOVER_MS,
  DUO_YAPPY_TURN_MS,
  cinderTopicLine,
  duoAt,
  duoLengthMs,
  duoTurns,
  skippedArrival,
  yappyTurnLine,
} from "./duo";

const story = (key: string, score: number, over: Partial<CachedStory> = {}): CachedStory => ({
  source: "google_news",
  topic: "tech",
  title: `Headline ${key}`,
  url: `https://example.com/${key}`,
  outlet: null,
  outlets: 1,
  score,
  urlKey: key,
  ...over,
});
const NOW = new Date(2026, 10, 7, 8, 2).getTime();
const briefing = buildBriefing(
  [story("a", 3, { topic: "developer" }), story("b", 2, { topic: "NVDA" }), story("c", 1, { topic: "  quantum error correction in superconducting qubits  " })],
  [{ urlKey: "a", url: "https://example.com/a", headline: "Headline a", line: "a new compiler, twice as fast.", explained: true }],
  NOW,
);

describe("yap-1106-news-duo-delivery turns", () => {
  it("news_duo_cinder_names_the_topic_then_yappy_explains", () => {
    const t = duoTurns(briefing, ["NVDA"], "friendly");
    expect(t.map((x) => x.who)).toEqual(["cinder", "yappy", "cinder", "yappy", "cinder", "yappy"]);
    expect(t[0]).toEqual({ who: "cinder", text: "Dev news!", urlKey: "a" });
    expect(t[1].text).toBe("a new compiler, twice as fast.");
    expect(t[2].text).toBe("Your stocks: NVDA!");
    expect(t[4].text.startsWith("About quantum")).toBe(true);
    for (const x of t.filter((x) => x.who === "cinder")) expect(x.text.length).toBeLessThanOrEqual(CINDER_LINE_MAX);
    expect(duoTurns(null, [], "friendly")).toEqual([]);
  });

  it("news_duo_cinder_line_is_one_short_line", () => {
    expect(cinderTopicLine({ topic: "stocks", source: "google_news" }, [])).toBe("Your stocks!");
    expect(cinderTopicLine({ topic: "x", source: "quotes" }, [])).toBe("Your stocks!");
    expect(cinderTopicLine({ topic: "   ", source: "google_news" }, [])).toBe("Next one!");
    expect(cinderTopicLine({ topic: "world", source: "google_news" }, [])).toBe("World news!");
    const long = cinderTopicLine({ topic: "a very long custom topic\nwith a newline in it", source: "google_news" }, []);
    expect(long.length).toBeLessThanOrEqual(CINDER_LINE_MAX);
    expect(long).not.toContain("\n");
    expect(long.endsWith("…")).toBe(true);
  });

  it("news_duo_yappy_explains_in_the_chosen_voice", () => {
    const bare = { headline: "Chip maker beats estimates", line: "" };
    expect(yappyTurnLine(bare, "friendly")).toBe("okay so: Chip maker beats estimates");
    expect(yappyTurnLine(bare, "rude")).toBe("real talk: Chip maker beats estimates");
    expect(yappyTurnLine(bare, "professional")).toBe("In brief: Chip maker beats estimates");
    expect(yappyTurnLine(bare, "rose")).toBe("for you, love: Chip maker beats estimates");
    expect(yappyTurnLine(bare, undefined)).toBe("okay so: Chip maker beats estimates");
    // The explain step already wrote the line in the chosen voice: it is used as is.
    expect(yappyTurnLine({ headline: "h", line: "  beat the street.  " }, "professional")).toBe("beat the street.");
  });
});

describe("yap-1106-news-duo-delivery clock", () => {
  const T0 = 1_000_000;
  it("news_duo_flies_to_the_desk_hands_over_then_takes_turns", () => {
    expect(duoAt(T0, T0, false, 6)).toMatchObject({ stage: "flying", flight: 0, turn: -1, speaker: null, nextChangeMs: CINDER_FLIGHT_MS });
    expect(duoAt(T0, T0 + CINDER_FLIGHT_MS / 2, false, 6).flight).toBeCloseTo(0.5);
    const hand = duoAt(T0, T0 + CINDER_FLIGHT_MS + DUO_HANDOVER_MS / 2, false, 6);
    expect(hand).toMatchObject({ stage: "handover", flight: 1, speaker: null });
    expect(hand.handover).toBeCloseTo(0.5);
    const start = T0 + CINDER_FLIGHT_MS + DUO_HANDOVER_MS;
    expect(duoAt(T0, start, false, 6)).toMatchObject({ stage: "turns", turn: 0, speaker: "cinder", handover: 1, nextChangeMs: DUO_CINDER_TURN_MS });
    expect(duoAt(T0, start + DUO_CINDER_TURN_MS, false, 6)).toMatchObject({ turn: 1, speaker: "yappy", nextChangeMs: DUO_YAPPY_TURN_MS });
    expect(duoAt(T0, start + DUO_CINDER_TURN_MS + DUO_YAPPY_TURN_MS, false, 6)).toMatchObject({ turn: 2, speaker: "cinder" });
    expect(duoAt(T0, T0 + duoLengthMs(6, false), false, 6)).toMatchObject({ stage: "done", speaker: null, nextChangeMs: null });
    expect(duoLengthMs(6, false)).toBe(CINDER_FLIGHT_MS + DUO_HANDOVER_MS + 3 * (DUO_CINDER_TURN_MS + DUO_YAPPY_TURN_MS));
  });

  it("news_duo_reduce_motion_side_by_side_no_flight", () => {
    const f = duoAt(T0, T0, true, 2);
    expect(f).toMatchObject({ stage: "turns", flight: 1, handover: 1, turn: 0, speaker: "cinder" });
    expect(duoAt(T0, T0 + DUO_CINDER_TURN_MS, true, 2).speaker).toBe("yappy");
    expect(duoLengthMs(2, true)).toBe(DUO_CINDER_TURN_MS + DUO_YAPPY_TURN_MS);
    expect(duoAt(T0, T0 + duoLengthMs(2, true), true, 2).stage).toBe("done");
  });

  it("news_duo_never_during_quiet_no_arrival_means_nothing_moves", () => {
    // Home passes no arrival time while a meeting records, the screen is shared or DND is on.
    for (const a of [null, undefined, Number.NaN]) expect(duoAt(a, T0, false, 6)).toMatchObject({ stage: "done", flight: 1, speaker: null });
    expect(duoAt(T0 + 5000, T0, false, 6).stage).toBe("done");
    expect(duoAt(T0, Number.NaN, false, 6).stage).toBe("done");
    expect(duoAt(T0, T0 + CINDER_FLIGHT_MS + DUO_HANDOVER_MS, false, 0).stage).toBe("done");
    expect(duoLengthMs(-3, false)).toBe(CINDER_FLIGHT_MS + DUO_HANDOVER_MS);
  });

  it("news_duo_skip_reads_as_finished", () => {
    expect(duoAt(skippedArrival(T0, 6, false), T0, false, 6).stage).toBe("done");
    expect(duoAt(skippedArrival(T0, 2, true), T0, true, 2).stage).toBe("done");
  });
});
