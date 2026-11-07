/**
 * yap-1106-voice-modes — Yappy's three voices (Best friend, Badass,
 * Professional) plus Rose as a fourth. Pins the acceptance:
 *   1. Settings → Yappy's voice offers Best friend, Badass, Professional (+ Rose).
 *   2. Each voice has take lines for up to 5 words / about 21 / about 61 / an
 *      essay, plus during-take lines.
 *   3. At least 40 distinct lines per voice (companions page, section 5).
 *   4. No line repeats within the last 20 takes.
 * fix-b6-1-done-line-no-false-paste:
 *   5. pasted:false never yields a paste-claiming line (4 voices x 4 bands, 200 takes, no repeat inside 20).
 *   6. A take that received insert-copy-card ends in done with a non-paste-claim line.
 *   7. The pill voice_line log carries pasted=<bool> and no transcript text.
 */
import { describe, expect, it, vi } from "vitest";
// @ts-expect-error -- node builtin, untyped here; the test runs under Node (as in purity.test.ts).
import { readFileSync } from "node:fs";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined), emit: vi.fn(async () => undefined) }));

import { AppContext, type AppCtx } from "../appShell";
import Companion, { curseToggleShown } from "../views/settings/Companion";
import { isLiveCall, type CallOfferView } from "./callOfferModel";
import { advanceLive, createLiveState, DEFAULT_LIVE, LIVE_TIERS, reduceTakeStage, type LivePhase, type TakeStageName } from "./live";
import { INSERT_COPY_CARD_EVENT } from "./copyCardModel";
import {
  CURSE_EVERY, EMPTY_MEMORY, PASTE_CLAIMS, curseAllowed, curseBlock, seededRandom, RECENT_TAKES, TAKE_BANDS, VOICES, VOICE_OPTIONS,
  claimsPaste, distinctLines, duringLines, fillLine, isStrongest, noteTakeSignal, parseMemory, pickTakeLine,
  rememberTakeLine, takeBand, takeDoneLine, typingSaved, voiceFor,
  type PickOpts, type TakeLineMemory, type TakePasteSignal, type VoiceId,
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
    expect(curseBlock({ ...base, meeting: true })).toBe("in_meeting");
    expect(curseBlock({ ...base, screenShared: true })).toBe("screen_shared");
    expect(curseAllowed({ ...base, meeting: true, screenShared: true })).toBe(false);
  });

  it("a live call (any phase but idle) counts as a screen-share state and shuts the gate", () => {
    const view = (phase: CallOfferView["phase"]): CallOfferView => ({
      phase, offerId: null, appName: "Zoom", bundleId: null, copy: null, siteLine: null, yourSideOnly: false, countdownMsLeft: 0,
    });
    expect(isLiveCall(null)).toBe(false);
    expect(isLiveCall(view("idle"))).toBe(false);
    for (const p of ["offering", "chip", "recording", "end_prompt"] as CallOfferView["phase"][]) {
      expect(isLiveCall(view(p))).toBe(true);
      expect(curseAllowed({ enabled: true, voice: "badass", meeting: false, screenShared: isLiveCall(view(p)) })).toBe(false);
    }
    expect(curseAllowed({ enabled: true, voice: "badass", meeting: false, screenShared: isLiveCall(view("idle")) })).toBe(true);
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

describe("fix-b6-1-done-line-no-false-paste — no paste claim the Copy card contradicts", () => {
  /** One word count per band: up to 5 / about 21 / about 61 / an essay. */
  const BAND_WORDS: Record<string, number> = { tiny: 3, paragraph: 21, memo: 61, essay: 420 };
  const TODAY = "2026-11-06";

  it("tags exactly the approved lines that say pasted/inserted, and every one is real copy", () => {
    const takeLines = (v: VoiceId) => TAKE_BANDS.flatMap((b) => VOICES[v].take[b]);
    const every = ALL.flatMap(takeLines);
    for (const claim of PASTE_CLAIMS) expect(every).toContain(claim);
    // Drift guard: a new line that says it pasted/inserted must be tagged too.
    const sayPaste = /\b(paste|pasted|pasting|insert|inserted)\b/i;
    for (const l of [...every, ...(VOICES.badass.curse ?? [])]) expect(PASTE_CLAIMS.has(l)).toBe(sayPaste.test(l));
    expect(claimsPaste("Inserted, 12 words.")).toBe(true);           // filled template
    expect(claimsPaste("Pasted at cursor.")).toBe(true);
    expect(claimsPaste("Captured, ready to go.")).toBe(false);
  });

  it("defaults to pasted:true, so the approved claims are still said when the text landed", () => {
    const empty = (): TakeLineMemory => ({ recent: [], strongDay: null });
    expect(pickTakeLine(3, "professional", empty(), { nonce: 0 }).text).toBe("Inserted, 3 words.");
    expect(pickTakeLine(3, "professional", empty(), { nonce: 0, pasted: true }).key).toBe("Inserted, {w} words.");
    const off = pickTakeLine(3, "professional", empty(), { nonce: 0, pasted: false });
    expect(claimsPaste(off.text)).toBe(false);
    expect(off.source).toBe("band");
  });

  it.each(ALL)("%s: pasted:false never yields a paste claim in any band over 200 takes, and never repeats inside 20", (voice) => {
    for (const band of TAKE_BANDS) {
      const w = BAND_WORDS[band];
      expect(takeBand(w)).toBe(band);
      // Curse off, curse on by nonce, and curse on with the pill's random draw (seeded here).
      for (const mode of ["off", "nonce", "rand"] as const) {
        const curse = mode !== "off";
        const rand = mode === "rand" ? seededRandom(41) : undefined;
        let mem: TakeLineMemory = { recent: [], strongDay: null };
        const keys: string[] = [];
        const texts: string[] = [];
        for (let i = 0; i < 200; i++) {
          const p = pickTakeLine(w, voice, mem, { nonce: i, pasted: false, curse, rand, today: TODAY });
          expect(PASTE_CLAIMS.has(p.key)).toBe(false);
          expect(claimsPaste(p.text)).toBe(false);
          expect(keys.slice(Math.max(0, i - RECENT_TAKES), i)).not.toContain(p.key);
          expect(texts.slice(Math.max(0, i - RECENT_TAKES), i)).not.toContain(p.text);
          keys.push(p.key); texts.push(p.text);
          mem = rememberTakeLine(mem, p, TODAY);
        }
      }
    }
  });

  it("memory full of earlier pasted takes still yields a fresh non-claim line", () => {
    for (const voice of ALL) {
      const { mem } = runTakes(voice, 60);
      const p = pickTakeLine(3, voice, mem, { nonce: 61, pasted: false });
      expect(claimsPaste(p.text)).toBe(false);
      expect(mem.recent).not.toContain(p.key);
    }
  });

  it("folds the take's paste signals: listening clears, the Copy card and paste_not_confirmed set, paste_confirmed keeps", () => {
    const fold = (signals: TakePasteSignal[], start = false) => signals.reduce(noteTakeSignal, start);
    expect(fold(["listening"], true)).toBe(false);
    expect(fold(["listening", "insert-copy-card"])).toBe(true);
    expect(fold(["listening", "paste_not_confirmed"])).toBe(true);
    expect(fold(["listening", "paste_confirmed"])).toBe(false);
    expect(fold(["listening", "insert-copy-card", "paste_confirmed"])).toBe(true);   // a card on screen is the answer
    expect(fold(["listening", "insert-copy-card", "listening"])).toBe(false);        // the next take starts clean
  });

  it("a take that received insert-copy-card ends in done with a non-paste-claim line", () => {
    expect(INSERT_COPY_CARD_EVENT).toBe("insert-copy-card");
    // The take's phase: a not-confirmed paste still ends in `done` (live.ts reduceTakeStage).
    const stages: TakeStageName[] = ["decoding", "pasting", "paste_not_confirmed", "done"];
    const phase = stages.reduce<LivePhase>((p, st) => reduceTakeStage(p, st, st === "done" ? 9 : undefined), "thinking");
    expect(phase).toBe("done");
    // What YappyPill does on that path, take after take, for every voice and size.
    for (const voice of ALL) {
      let mem: TakeLineMemory = { recent: [], strongDay: null };
      let copyCardShown = true;                       // left over from an earlier take
      let claimsWhenLanded = 0;
      for (let i = 0; i < 200; i++) {
        const w = [3, 21, 61, 420][i % 4];
        const cardTake = i % 3 !== 2;
        const signals: TakePasteSignal[] = cardTake
          ? ["listening", "insert-copy-card", "paste_not_confirmed"]
          : ["listening", "paste_confirmed"];
        copyCardShown = signals.reduce(noteTakeSignal, copyCardShown);
        expect(copyCardShown).toBe(cardTake);
        const out = takeDoneLine(w, voice, mem, { nonce: i, today: TODAY, pasted: !copyCardShown });
        if (cardTake) expect(claimsPaste(out.picked.text)).toBe(false);
        else if (claimsPaste(out.picked.text)) claimsWhenLanded++;
        expect(out.log).toContain(`pasted=${!cardTake}`);
        mem = out.memory;
      }
      // Positive control: voices with paste claims still say them on takes that landed.
      if (voice === "professional" || voice === "bestfriend") expect(claimsWhenLanded).toBeGreaterThan(0);
    }
  });

  it("pill voice_line log carries pasted=<bool> and no transcript text", () => {
    const mem: TakeLineMemory = { recent: [], strongDay: null };
    for (const pasted of [true, false]) {
      const out = takeDoneLine(61, "professional", mem, { nonce: 4, today: TODAY, pasted });
      expect(out.log).toMatch(/^pill voice_line voice=[a-z]+ band=[a-z]+ source=[a-z]+ curse=(true|false) recent=\d+ pasted=(true|false)$/);
      expect(out.log.endsWith(` pasted=${pasted}`)).toBe(true);
      expect(out.log).not.toContain(out.picked.text);
      expect(out.memory.recent).toEqual([out.picked.key]);
    }
    expect(takeDoneLine(3, "bestfriend", mem).log).toContain("pasted=true");   // default
    expect(takeDoneLine(3, "badass", mem, { curse: true, pasted: false }).log).toContain(" curse=true recent=1 pasted=false");
  });

  it("YappyPill wires it: tracks the Copy card per take and passes pasted into the done line", () => {
    const src = readFileSync(new URL("./YappyPill.tsx", import.meta.url), "utf8") as string;
    expect(src).toContain("listen<unknown>(INSERT_COPY_CARD_EVENT");
    expect(src).toContain("listen<TakeStageNotice>(TAKE_STAGE_EVENT");
    expect(src).toContain('copyCardShown = noteTakeSignal(copyCardShown, "listening")');
    expect(src).toContain("pasted: !copyCardShown");
    expect(src).toContain("console.info(out.log)");
    expect(src).not.toMatch(/pickTakeLine\(/);
  });
});
