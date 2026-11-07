/**
 * yap-1106-voice-modes — Yappy's three voices (Best friend, Badass,
 * Professional) plus Rose as a fourth. Pins the acceptance:
 *   1. Settings → Yappy's voice offers Best friend, Badass, Professional (+ Rose).
 *   2. Each voice has take lines for up to 5 words / about 21 / about 61 / an
 *      essay, plus during-take lines.
 *   3. At least 40 distinct lines per voice (companions page, section 5).
 *   4. No line repeats within the last 20 takes.
 */
import { describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined), emit: vi.fn(async () => undefined) }));

import { AppContext, type AppCtx } from "../appShell";
import Companion, { curseToggleShown } from "../views/settings/Companion";
import { advanceLive, createLiveState, DEFAULT_LIVE, LIVE_TIERS } from "./live";
import {
  CURSE_EVERY, EMPTY_MEMORY, curseAllowed, curseBlock, seededRandom, RECENT_TAKES, TAKE_BANDS, VOICES, VOICE_OPTIONS,
  distinctLines, duringLines, fillLine, isStrongest, parseMemory, pickTakeLine,
  rememberTakeLine, takeBand, typingSaved, voiceFor,
  type PickOpts, type TakeLineMemory, type VoiceId,
} from "./voices";

const THREE: VoiceId[] = ["bestfriend", "badass", "professional"];
const ALL: VoiceId[] = ["bestfriend", "badass", "professional", "rose"];

type State = Record<string, unknown>;
const makeCtx = (state: State): AppCtx =>
  new Proxy(state, {
    get: (t, k: string) => (k in t ? t[k] : () => undefined),
  }) as unknown as AppCtx;
const render = (node: ReactElement, state: State) =>
  renderToStaticMarkup(<AppContext.Provider value={makeCtx(state)}>{node}</AppContext.Provider>);

/** Deterministic word counts: a long run of tiny takes, then a mix of every size. */
function wordSeries(n: number): number[] {
  const out: number[] = [];
  let x = 12345;
  for (let i = 0; i < n; i++) {
    if (i < 40) { out.push(3); continue; }            // 40 tiny takes in a row
    x = (x * 1103515245 + 12345) % 2147483648;
    out.push([2, 21, 61, 420][x % 4] + (x % 7));
  }
  return out;
}

/** Run `n` takes through the picker, threading the memory like the pill does. */
function runTakes(voice: VoiceId, n: number, opts: Omit<PickOpts, "nonce"> = {}) {
  let mem: TakeLineMemory = { ...EMPTY_MEMORY, recent: [] };
  const keys: string[] = [];
  const texts: string[] = [];
  const sources: string[] = [];
  wordSeries(n).forEach((w, i) => {
    const p = pickTakeLine(w, voice, mem, { ...opts, nonce: i });
    keys.push(p.key); texts.push(p.text); sources.push(p.source);
    mem = rememberTakeLine(mem, p, opts.today);
  });
  return { keys, texts, sources, mem };
}

describe("Settings → Yappy's voice", () => {
  it("offers Best friend, Badass and Professional, with Rose as a fourth, in that order", () => {
    const html = render(<Companion />, { settings: {}, saveSettings: () => undefined });
    expect(html).toContain("Yappy&#x27;s voice");
    const at = ["Best friend", "Badass", "Professional", "Rose"].map((n) => html.indexOf(`<strong>${n}</strong>`));
    at.forEach((i) => expect(i).toBeGreaterThan(-1));
    expect([...at].sort((a, b) => a - b)).toEqual(at);
    expect(VOICE_OPTIONS.map(([, v]) => v)).toEqual(ALL);
  });

  it("marks the saved voice; old ids carry over and unknown reads as Best friend", () => {
    const pressed = (tone?: string) => {
      const html = render(<Companion />, { settings: { companionTone: tone }, saveSettings: () => undefined });
      return [...html.matchAll(/aria-pressed="true" data-voice="([a-z]+)"/g)].map((m) => m[1]);
    };
    expect(pressed(undefined)).toEqual(["bestfriend"]);
    expect(pressed("rude")).toEqual(["badass"]);
    expect(pressed("professional")).toEqual(["professional"]);
    expect(pressed("rose")).toEqual(["rose"]);
    expect(voiceFor("friendly")).toBe("bestfriend");
    expect(voiceFor("nonsense")).toBe("bestfriend");
  });
});

describe("the voices' lines (companions page, section 5)", () => {
  it("cuts takes at up to 5 words / about 21 / about 61 / an essay", () => {
    expect([0, 5, 6, 21, 40, 41, 61, 150, 151, 900].map(takeBand)).toEqual([
      "tiny", "tiny", "paragraph", "paragraph", "paragraph", "memo", "memo", "memo", "essay", "essay",
    ]);
    expect(takeBand(Number.NaN)).toBe("tiny");
  });

  it.each(ALL)("%s has lines for every take size and during-take lines", (voice) => {
    for (const band of TAKE_BANDS) expect(VOICES[voice].take[band].length).toBeGreaterThanOrEqual(7);
    expect(VOICES[voice].during.length).toBeGreaterThanOrEqual(16);
  });

  it.each(ALL)("%s has at least 40 distinct lines", (voice) => {
    expect(distinctLines(voice).size).toBeGreaterThanOrEqual(40);
  });

  it("the three voices are distinct from each other", () => {
    for (const a of THREE) for (const b of THREE) {
      if (a === b) continue;
      const shared = [...distinctLines(a)].filter((l) => distinctLines(b).has(l));
      expect(shared).toEqual([]);
    }
  });

  it("only Badass has curse lines; the strongest word is among them", () => {
    expect(VOICES.badass.curse?.some(isStrongest)).toBe(true);
    for (const v of ["bestfriend", "professional", "rose"] as VoiceId[]) expect(VOICES[v].curse).toBeUndefined();
  });

  it("Professional states real numbers, never the sample ones", () => {
    expect(fillLine("Inserted, {w} words.", 4)).toBe("Inserted, 4 words.");
    expect(typingSaved(61)).toBe("92 s");
    expect(typingSaved(200)).toBe("5 min");
    for (const t of runTakes("professional", 200).texts) expect(t).not.toMatch(/[{}]/);
  });
});

describe("no line repeats within the last 20 takes", () => {
  it.each(ALL)("%s, 300 takes including 40 tiny ones in a row", (voice) => {
    const { keys, texts } = runTakes(voice, 300);
    for (let i = 0; i < keys.length; i++) {
      const window = keys.slice(Math.max(0, i - RECENT_TAKES), i);
      expect(window).not.toContain(keys[i]);
      const shown = texts.slice(Math.max(0, i - RECENT_TAKES), i);
      expect(shown).not.toContain(texts[i]);
    }
  });

  it("Badass with curse mode on still never repeats inside 20 takes", () => {
    const { keys } = runTakes("badass", 400, { curse: true, today: "2026-11-06" });
    for (let i = 0; i < keys.length; i++) expect(keys.slice(Math.max(0, i - RECENT_TAKES), i)).not.toContain(keys[i]);
  });

  it("prefers the take's own size while it has a fresh line", () => {
    const { sources } = runTakes("bestfriend", 7);
    expect(sources.every((s) => s === "band")).toBe(true);
    const p = pickTakeLine(400, "bestfriend", { ...EMPTY_MEMORY, recent: [] }, { nonce: 0 });
    expect(VOICES.bestfriend.take.essay).toContain(p.key);
  });

  it("memory keeps only the last 20 and survives a malformed store", () => {
    const { mem } = runTakes("badass", 50);
    expect(mem.recent.length).toBe(RECENT_TAKES);
    expect(parseMemory(JSON.stringify(mem))).toEqual(mem);
    expect(parseMemory("{nope")).toEqual({ recent: [], strongDay: null });
    expect(parseMemory(null)).toEqual({ recent: [], strongDay: null });
  });
});

describe("curse mode (Badass only, off by default)", () => {
  const curseSet = new Set(VOICES.badass.curse);
  it("is off by default", () => {
    expect(runTakes("badass", 200).keys.filter((k) => curseSet.has(k))).toEqual([]);
  });
  it("swaps in about one take in four when on", () => {
    const n = runTakes("badass", 400, { curse: true, today: "2026-11-06" }).keys.filter((k) => curseSet.has(k)).length;
    expect(n / 400).toBeGreaterThan(0.15);
    expect(n / 400).toBeLessThanOrEqual(1 / CURSE_EVERY + 0.01);
  });
  it("never curses in another voice", () => {
    for (const v of ["bestfriend", "professional", "rose"] as VoiceId[]) {
      expect(runTakes(v, 200, { curse: true }).keys.filter((k) => curseSet.has(k))).toEqual([]);
    }
  });
  it("says the strongest word at most once a day", () => {
    const keys = runTakes("badass", 400, { curse: true, today: "2026-11-06" }).keys;
    expect(keys.filter(isStrongest).length).toBeLessThanOrEqual(1);
    const pick = (strongDay: string | null) =>
      Array.from({ length: 100 }, (_, i) =>
        pickTakeLine(30, "badass", { recent: [], strongDay }, { nonce: i * CURSE_EVERY, curse: true, today: "2026-11-06" }).key);
    expect(pick(null).some(isStrongest)).toBe(true);            // available on a fresh day
    expect(pick("2026-11-05").some(isStrongest)).toBe(true);    // yesterday's does not count
    expect(pick("2026-11-06").some(isStrongest)).toBe(false);   // already said today
    const after = rememberTakeLine({ recent: [], strongDay: null }, { key: VOICES.badass.curse![9] }, "2026-11-06");
    expect(after.strongDay).toBe("2026-11-06");
  });
});

describe("curse setting (yap-1106-curse-setting)", () => {
  const curseSet = new Set(VOICES.badass.curse);
  const companion = (settings: State) => render(<Companion />, { settings, saveSettings: () => undefined });

  it("the toggle is shown only under Badass", () => {
    for (const tone of [undefined, "friendly", "professional", "rose", "nonsense"]) {
      expect(companion({ companionTone: tone })).not.toContain("data-curse-setting");
      expect(curseToggleShown(tone)).toBe(false);
    }
    expect(companion({ companionTone: "rude" })).toContain("data-curse-setting");
    expect(curseToggleShown("rude")).toBe(true);
  });

  it("the toggle is off by default and shows a saved choice", () => {
    const toggle = (html: string) => html.match(/<input[^>]*data-curse-toggle[^>]*>/)?.[0] ?? "";
    expect(toggle(companion({ companionTone: "rude" }))).not.toBe("");
    expect(toggle(companion({ companionTone: "rude" }))).not.toContain("checked");
    expect(toggle(companion({ companionTone: "rude", companionCurse: false }))).not.toContain("checked");
    expect(toggle(companion({ companionTone: "rude", companionCurse: true }))).toContain("checked");
  });

  it("the gate: Badass + toggle on only, never Professional, never in a meeting or a screen share", () => {
    const base = { enabled: true, voice: "badass" as VoiceId, meeting: false, screenShared: false };
    expect(curseAllowed(base)).toBe(true);
    expect(curseBlock(base)).toBeNull();
    expect(curseBlock({ ...base, enabled: false })).toBe("off");
    for (const v of ["bestfriend", "professional", "rose"] as VoiceId[]) {
      expect(curseAllowed({ ...base, voice: v })).toBe(false);
      expect(curseBlock({ ...base, voice: v })).toBe("voice");
    }
    expect(curseBlock({ ...base, meeting: true })).toBe("meeting");
    expect(curseBlock({ ...base, screenShared: true })).toBe("screen_shared");
    expect(curseAllowed({ ...base, meeting: true, screenShared: true })).toBe(false);
  });

  it("seededRandom is replayable and stays in [0, 1)", () => {
    const a = seededRandom(7), b = seededRandom(7), c = seededRandom(8);
    const xs = Array.from({ length: 50 }, () => a());
    expect(Array.from({ length: 50 }, () => b())).toEqual(xs);
    expect(Array.from({ length: 50 }, () => c())).not.toEqual(xs);
    xs.forEach((x) => { expect(x).toBeGreaterThanOrEqual(0); expect(x).toBeLessThan(1); });
  });

  it.each([1, 42, 2026])("with seeded random %i, about 1 line in 4 is a curse line", (seed) => {
    const n = 800;
    const keys = runTakes("badass", n, { curse: true, today: "2026-11-06", rand: seededRandom(seed) }).keys;
    const share = keys.filter((k) => curseSet.has(k)).length / n;
    expect(share).toBeGreaterThan(0.17);
    expect(share).toBeLessThan(0.33);
    // Same seed, same takes.
    expect(runTakes("badass", n, { curse: true, today: "2026-11-06", rand: seededRandom(seed) }).keys).toEqual(keys);
  });

  it("with seeded random, the strongest word is said at most once a day, and again the next day", () => {
    let mem: TakeLineMemory = { recent: [], strongDay: null };
    const rand = seededRandom(99);
    const perDay: Record<string, number> = {};
    for (const today of ["2026-11-06", "2026-11-07", "2026-11-08"]) {
      for (let i = 0; i < 300; i++) {
        const p = pickTakeLine(30, "badass", mem, { nonce: i, curse: true, today, rand });
        if (isStrongest(p.key)) perDay[today] = (perDay[today] ?? 0) + 1;
        mem = rememberTakeLine(mem, p, today);
      }
    }
    for (const today of ["2026-11-06", "2026-11-07", "2026-11-08"]) expect(perDay[today]).toBe(1);
  });

  it("with seeded random, never curses in Professional (or any non-Badass voice), nor with the gate shut", () => {
    for (const v of ["bestfriend", "professional", "rose"] as VoiceId[]) {
      const keys = runTakes(v, 400, { curse: true, today: "2026-11-06", rand: seededRandom(3) }).keys;
      expect(keys.filter((k) => curseSet.has(k))).toEqual([]);
    }
    const shut = curseAllowed({ enabled: true, voice: "badass", meeting: true, screenShared: false });
    const keys = runTakes("badass", 400, { curse: shut, today: "2026-11-06", rand: seededRandom(3) }).keys;
    expect(keys.filter((k) => curseSet.has(k))).toEqual([]);
  });
});

describe("during-take lines (live.ts advanceLive with a voice)", () => {
  const speech = (loud = 0.05, quiet = 0.008) => (t: number): number => {
    if (t % 8 > 7.4) return quiet;
    const s = Math.sin(t * Math.PI * 2 * 3.5);
    return s > 0 ? quiet + (loud - quiet) * s : quiet;
  };
  const talk = (seconds: number, voice?: VoiceId) => {
    const state = createLiveState();
    const said: string[] = [];
    const level = speech();
    for (let t = 0; t < seconds; t += 1 / 60) {
      const f = advanceLive(state, 1 / 60, level(t), "friendly", DEFAULT_LIVE, voice);
      if (f.say) said.push(f.say);
    }
    return said;
  };

  it("slices every voice's escalating list across the live tiers", () => {
    for (const v of ALL) {
      const slices = LIVE_TIERS.map((_, i) => duringLines(v, i, LIVE_TIERS.length));
      expect(slices.flat()).toEqual(VOICES[v].during);
      slices.forEach((s) => expect(s.length).toBeGreaterThanOrEqual(3));
    }
  });

  it.each(THREE)("%s talks in its own words for a whole long take, never the same line twice in a row", (voice) => {
    const said = talk(210, voice);
    const own = new Set(VOICES[voice].during);
    expect(said.length).toBeGreaterThanOrEqual(6);
    said.forEach((l) => expect(own.has(l)).toBe(true));
    for (let i = 1; i < said.length; i++) expect(said[i]).not.toBe(said[i - 1]);
  });

  it("without a voice, the old tone lines are unchanged", () => {
    expect(talk(30)).toEqual(talk(30, undefined));
    expect(talk(30).some((l) => VOICES.bestfriend.during.includes(l))).toBe(false);
  });
});
