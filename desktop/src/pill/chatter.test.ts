/**
 * yap-1106-chatter-setting — "Yappy talks": always / less over time / only
 * milestones. The policy is pure, so every rule is proved here without a
 * window; chatter.settings.test.tsx proves the Settings row and the wiring.
 */
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  allowAfterLine, allowDoneLine, allowDuringLine, allowVisit, chatterLog, chatterPolicy, chatterSentence,
  chatterStatus, CHATTER_MODES, CHATTER_OPTIONS, DEFAULT_CHATTER_MODE, isMilestoneTake, MILESTONE_TAKES,
  parseChatterMode, parseTakeCount, QUIET_BUBBLE_CHARS, QUIET_FROM_TAKE, QUIET_HOP_SCALE, shortenBubble,
  takeCountLabel, TAKES_SEEN_KEY,
} from "./chatter";
import { MILESTONE_AT } from "./nudge";

const src = (rel: string): string => readFileSync(new URL(rel, import.meta.url), "utf8") as string;

describe("Yappy talks — the setting", () => {
  it("offers always / less over time / only milestones, in that order, defaulting to less", () => {
    expect(CHATTER_MODES).toEqual(["always", "less", "milestones"]);
    expect(CHATTER_OPTIONS.map(([id, label]) => [id, label])).toEqual([
      ["always", "Always"], ["less", "Less over time"], ["milestones", "Only milestones"],
    ]);
    expect(DEFAULT_CHATTER_MODE).toBe("less");
  });

  it("reads anything unknown as the default", () => {
    expect(parseChatterMode("always")).toBe("always");
    expect(parseChatterMode("milestones")).toBe("milestones");
    for (const junk of [undefined, null, "", "never", "LESS", 3, {}]) expect(parseChatterMode(junk)).toBe("less");
  });

  it("parses a stored take count, and anything unreadable is 0", () => {
    expect(parseTakeCount("37")).toBe(37);
    expect(parseTakeCount(12.9)).toBe(12);
    for (const junk of [undefined, null, "", "  ", "abc", -4, NaN, Infinity]) expect(parseTakeCount(junk)).toBe(0);
  });

  it("shares the pill's take counter key", () => {
    expect(TAKES_SEEN_KEY).toBe("yap.live.takesSeen");
  });
});

describe("Yappy talks — the 50th-take quieting", () => {
  it("less over time: full show for takes 1..49, quiet from the 50th take", () => {
    expect(QUIET_FROM_TAKE).toBe(50);
    for (const n of [1, 20, 49]) expect(chatterPolicy("less", n).stage).toBe("full");
    for (const n of [50, 51, 200, 5000]) expect(chatterPolicy("less", n).stage).toBe("quiet");
  });

  it("from the 50th take: bubbles less often and shorter, smaller hops, fewer visits, no first-time tips", () => {
    const full = chatterPolicy("less", 49);
    const quiet = chatterPolicy("less", 50);
    // less often
    expect(quiet.repeatEvery).toBeGreaterThan(full.repeatEvery);
    expect(quiet.afterLines).toBeLessThan(full.afterLines);
    expect(quiet.doneEvery).toBeGreaterThan(full.doneEvery);
    // shorter
    expect(full.maxChars).toBeNull();
    expect(quiet.maxChars).toBe(QUIET_BUBBLE_CHARS);
    // smaller hops
    expect(full.hopScale).toBe(1);
    expect(quiet.hopScale).toBe(QUIET_HOP_SCALE);
    expect(quiet.hopScale).toBeLessThan(1);
    // friends visit less
    expect(quiet.visitShare).toBeLessThan(full.visitShare);
    // first-time tips stop
    expect(full.firstTimeTips).toBe(true);
    expect(quiet.firstTimeTips).toBe(false);
    // the tier lines (the world working) stay
    expect(quiet.tierLines).toBe(true);
  });

  it("always never quiets, however many takes", () => {
    for (const n of [1, 50, 500, 100_000]) {
      const p = chatterPolicy("always", n);
      expect(p.stage).toBe("full");
      expect(p.firstTimeTips).toBe(true);
      expect(p.hopScale).toBe(1);
    }
  });

  it("only milestones: no chatter while you talk, a line only on a milestone take", () => {
    const p = chatterPolicy("milestones", 7);
    expect(p.stage).toBe("milestones");
    expect(allowDuringLine(p, true, 0)).toBe(false);
    expect(allowDuringLine(p, false, 0)).toBe(false);
    expect(allowAfterLine(p, 0)).toBe(false);
    expect(allowDoneLine(p, 7)).toBe(false);
    expect(allowDoneLine(p, 100)).toBe(true);
    expect(p.firstTimeTips).toBe(false);
    expect(allowVisit(p, 0)).toBe(false);
  });

  it("the repeating chatter gap doubles once quiet: 1 repeating line in 2 is kept", () => {
    const quiet = chatterPolicy("less", 60);
    const kept = [0, 1, 2, 3, 4, 5].map((n) => allowDuringLine(quiet, false, n));
    expect(kept).toEqual([true, false, true, false, true, false]);
    expect(allowDuringLine(quiet, true, 1)).toBe(true);
    const full = chatterPolicy("less", 10);
    expect([0, 1, 2].map((n) => allowDuringLine(full, false, n))).toEqual([true, true, true]);
    expect(allowDuringLine(full, false, -3)).toBe(true);
  });

  it("finished takes: every take while full, every other take once quiet, milestones always", () => {
    const full = chatterPolicy("less", 10);
    expect([10, 11, 12].map((n) => allowDoneLine(full, n))).toEqual([true, true, true]);
    const quiet = chatterPolicy("less", 60);
    expect([60, 61, 62, 63].map((n) => allowDoneLine(quiet, n))).toEqual([true, false, true, false]);
    expect(allowDoneLine(quiet, 101)).toBe(false);
    expect(allowDoneLine(quiet, 1000)).toBe(true);
  });

  it("after-release lines: 2 while full, 1 once quiet", () => {
    const full = chatterPolicy("always", 1);
    expect([0, 1, 2].map((n) => allowAfterLine(full, n))).toEqual([true, true, false]);
    const quiet = chatterPolicy("less", 50);
    expect([0, 1].map((n) => allowAfterLine(quiet, n))).toEqual([true, false]);
  });

  it("friend visits: all while full, a quarter once quiet", () => {
    const full = chatterPolicy("less", 1);
    const quiet = chatterPolicy("less", 50);
    const rolls = Array.from({ length: 100 }, (_, i) => i / 100);
    expect(rolls.filter((r) => allowVisit(full, r)).length).toBe(100);
    expect(rolls.filter((r) => allowVisit(quiet, r)).length).toBe(25);
  });

  it("milestone takes include the pill's milestone nudges, the 50th and every 1000th", () => {
    for (const n of Object.values(MILESTONE_AT)) expect(isMilestoneTake(n)).toBe(true);
    for (const n of MILESTONE_TAKES) expect(isMilestoneTake(n)).toBe(true);
    expect(isMilestoneTake(2000)).toBe(true);
    for (const n of [0, -1, 2, 49, 51, 999, NaN, Infinity]) expect(isMilestoneTake(n)).toBe(false);
    expect(isMilestoneTake(100.4)).toBe(true);
  });
});

describe("Yappy talks — shorter bubbles", () => {
  it("leaves a short line alone and never cuts with no cap", () => {
    expect(shortenBubble("write a book already…", QUIET_BUBBLE_CHARS)).toBe("write a book already…");
    const long = "okay, this is a saga and it keeps going on";
    expect(shortenBubble(long, null)).toBe(long);
    expect(shortenBubble("", QUIET_BUBBLE_CHARS)).toBe("");
  });

  it("cuts a long line at a word, with an ellipsis, within the cap", () => {
    const out = shortenBubble("okay, this is a saga and it keeps going on", QUIET_BUBBLE_CHARS);
    expect(out).toBe("okay, this is a saga and it…");
    expect(out.length).toBeLessThanOrEqual(QUIET_BUBBLE_CHARS);
    expect(shortenBubble("hello, world, again and again", 14)).toBe("hello, world…");
    expect(shortenBubble("a".repeat(34), 10)).toBe(`${"a".repeat(9)}…`);
    expect(shortenBubble("!!!!!!!! more words", 5)).toBe("!!!!…");
  });
});

describe("Yappy talks — the one plain sentence and the take count", () => {
  it("less over time says exactly what changes from the 50th take", () => {
    expect(chatterSentence("less")).toBe(
      "From your 50th take, bubbles come less often and are shorter, hops get smaller, friends visit less, and first-time tips stop.",
    );
  });

  it("each mode is one sentence, never naming a vendor or model", () => {
    for (const m of CHATTER_MODES) {
      const s = chatterSentence(m);
      expect(s.split(/[.!?](\s|$)/).filter((x) => x && x.trim()).length).toBe(1);
      expect(s).not.toMatch(/\b(AI|model|GPT|Whisper|Claude|Gemini|OpenAI)\b/);
    }
    expect(chatterSentence("always")).toMatch(/every take/);
    expect(chatterSentence("milestones")).toMatch(/milestones/);
  });

  it("shows the take count that drives it", () => {
    expect(takeCountLabel(0)).toBe("Takes so far: 0");
    expect(takeCountLabel(37)).toBe("Takes so far: 37");
    expect(takeCountLabel(1234)).toBe("Takes so far: 1,234");
    expect(takeCountLabel(Number.NaN)).toBe("Takes so far: 0");
  });

  it("says where less over time stands", () => {
    expect(chatterStatus("less", 0)).toBe("49 takes until he talks less.");
    expect(chatterStatus("less", 48)).toBe("1 take until he talks less.");
    expect(chatterStatus("less", 49)).toBe("Yappy is talking less now.");
    expect(chatterStatus("less", 400)).toBe("Yappy is talking less now.");
    expect(chatterStatus("always", 400)).toBe("");
    expect(chatterStatus("milestones", 0)).toBe("");
  });

  it("logs one structured line per take decision, counts only", () => {
    expect(chatterLog("less", 50, chatterPolicy("less", 50))).toBe("pill chatter mode=less take=50 stage=quiet");
    expect(chatterLog("always", 3, chatterPolicy("always", 3))).toBe("pill chatter mode=always take=3 stage=full");
  });
});

describe("Yappy talks — the backend twin", () => {
  it("lib.rs stores the same three modes with the same default", () => {
    const rs = src("../../src-tauri/src/lib.rs");
    expect(rs).toContain('pub const YAPPY_TALKS_MODES: [&str; 3] = ["always", "less", "milestones"];');
    expect(rs).toMatch(/fn default_yappy_talks\(\) -> String \{\s*"less"\.into\(\)/);
  });
});
