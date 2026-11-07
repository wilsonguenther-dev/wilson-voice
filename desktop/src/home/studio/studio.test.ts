/**
 * yap-1106-studio-room (+ FB-08) — the layered studio's station rules, its
 * redraw budget, and that every station really draws from the app state it
 * names. Pixel checks run on the test raster (sprites/raster.ts), so no webview.
 */
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { Raster } from "../../sprites/raster";
import type { TranscriptEntry } from "../../appTypes";
import { skyPhase } from "../habitat/habitat";
import { ROOM_H, ROOM_W } from "../homeLayout";
import {
  FIREFLY_SPOTS,
  FLOOR,
  IDLE_REDRAW_MS,
  JAR_CARDS,
  LAYERS,
  MAX_FIREFLIES,
  MAX_TINT,
  MEETING_REDRAW_MS,
  PAD,
  PARALLAX,
  STILL_T,
  STUDIO_H,
  STUDIO_W,
  batteryLevel,
  drawBack,
  drawFrame,
  drawLights,
  drawMid,
  drawTint,
  fireflyCount,
  fixesToday,
  jarCards,
  onAirLit,
  parallaxOffsets,
  parallaxTarget,
  parseTakeTime,
  redrawPlan,
  roomTintAlpha,
  shellyPluggedIn,
  skyBand,
  studioAriaLabel,
  tidyCounter,
  yappyPose,
  type RedrawInputs,
  type StudioState,
} from "./studio";

const read = (rel: string): string => readFileSync(new URL(rel, import.meta.url), "utf8") as string;

const DAY_SKY = { top: "#7ec8ff", bot: "#cdeeff", amb: "#fff2d8", ambA: 0 };
const NIGHT_SKY = { top: "#171a3d", bot: "#2c3060", amb: "#1a2050", ambA: 0.42 };
const NOON = skyPhase(new Date(2026, 10, 7, 12, 0));
const MIDNIGHT = skyPhase(new Date(2026, 10, 7, 0, 0));

function st(over: Partial<StudioState> = {}): StudioState {
  return {
    phase: NOON,
    sky: DAY_SKY,
    meetingRecording: false,
    fixesToday: null,
    dictionarySize: null,
    battery: null,
    streakDays: null,
    newsReady: false,
    yappy: "rest",
    ...over,
  };
}

/** Paint one painter into a fresh raster the size of a layer (PAD each side). */
function paint(fn: (r: Raster) => void, w = STUDIO_W + PAD * 2): Raster {
  const r = new Raster(w, STUDIO_H);
  r.translate(PAD, 0);
  fn(r);
  return r;
}
function differs(a: Raster, b: Raster): number {
  let n = 0;
  for (let y = 0; y < a.height; y++)
    for (let x = 0; x < a.width; x++) {
      const p = a.px(x, y), q = b.px(x, y);
      if (p.r !== q.r || p.g !== q.g || p.b !== q.b || p.a !== q.a) n++;
    }
  return n;
}
const hexOf = (r: Raster, x: number, y: number) => {
  const p = r.px(x, y);
  return "#" + [p.r, p.g, p.b].map((v) => v.toString(16).padStart(2, "0")).join("");
};

describe("the studio is the room: 320 x 160, whole-pixel layers 1 / 3 / 6", () => {
  it("homeLayout's room grid is the studio buffer", () => {
    expect([ROOM_W, ROOM_H]).toEqual([STUDIO_W, STUDIO_H]);
    expect([...PARALLAX]).toEqual([1, 3, 6]);
    expect(LAYERS).toHaveLength(3);
  });

  it("parallax offsets are whole pixels, bounded per layer, zero at rest", () => {
    expect(parallaxOffsets(0)).toEqual([0, 0, 0]);
    for (let p = -1; p <= 1; p += 0.05) {
      const o = parallaxOffsets(p);
      o.forEach((v, i) => {
        expect(Number.isInteger(v)).toBe(true);
        expect(Math.abs(v)).toBeLessThanOrEqual(Math.ceil(PARALLAX[i] * 0.6));
      });
    }
    expect(parallaxOffsets(Number.NaN)).toEqual([0, 0, 0]);
    expect(parallaxTarget(150, 100, 100)).toBe(0);
    expect(parallaxTarget(100, 100, 100)).toBeCloseTo(0.6);
    expect(parallaxTarget(200, 100, 100)).toBeCloseTo(-0.6);
    expect(parallaxTarget(10, 0, 0)).toBe(0);
  });
});

describe("the budget: idle at most once a minute, reduce motion one still frame", () => {
  const base: RedrawInputs = { reduceMotion: false, appState: "idle", reacting: false, parallaxSettling: false, meetingRecording: false };
  it("idle_redraws_at_most_once_a_minute", () => {
    expect(IDLE_REDRAW_MS).toBeGreaterThanOrEqual(60_000);
    expect(redrawPlan(base)).toEqual({ mode: "idle", delayMs: IDLE_REDRAW_MS });
  });
  it("reduce_motion_is_one_still_frame_whatever_happens", () => {
    for (const appState of ["idle", "listening", "thinking", "done"] as const)
      expect(redrawPlan({ ...base, reduceMotion: true, appState, reacting: true, parallaxSettling: true, meetingRecording: true })).toEqual({ mode: "still", delayMs: null });
    expect(STILL_T % 3.4).toBeGreaterThanOrEqual(0.15); // Yappy is not mid-blink in the still frame
  });
  it("moves only while something moves; a meeting follows its 1 Hz clock", () => {
    expect(redrawPlan({ ...base, appState: "listening" }).mode).toBe("live");
    expect(redrawPlan({ ...base, reacting: true }).mode).toBe("live");
    expect(redrawPlan({ ...base, parallaxSettling: true }).mode).toBe("live");
    expect(redrawPlan({ ...base, meetingRecording: true })).toEqual({ mode: "tick", delayMs: MEETING_REDRAW_MS });
  });
  it("YappyHouse follows the plan: no loop in reduce motion, a timer when idle", () => {
    const house = read("../YappyHouse.tsx");
    expect(house).toMatch(/redrawPlan\(\{/);
    expect(house).toMatch(/if \(p\.mode === "still"\) return;/);
    expect(house).toMatch(/window\.setTimeout\(/);
    expect(house).not.toMatch(/IDLE_FRAME_MS/);
    expect(house).toContain("STILL_T");
  });
});

describe("every station reads real app state", () => {
  it("hoots_on_air_is_lit_only_while_a_meeting_records", () => {
    expect(onAirLit({ meetingRecording: true })).toBe(true);
    expect(onAirLit({ meetingRecording: false })).toBe(false);
    const lit = paint((r) => drawLights(r, st({ meetingRecording: true }), 0, 0, 0));
    const dark = paint((r) => drawLights(r, st({ meetingRecording: false }), 0, 0, 0));
    expect(hexOf(lit, PAD + 77, 14)).toBe("#e5484d");
    expect(hexOf(dark, PAD + 77, 14)).toBe("#4a2c2a");
  });

  it("tidys_counter_shows_todays_fixes", () => {
    expect(tidyCounter(null)).toBe("");
    expect(tidyCounter(0)).toBe("0");
    expect(tidyCounter(12)).toBe("12");
    expect(tidyCounter(4000)).toBe("999");
    const now = new Date(2026, 10, 7, 15, 0);
    const take = (createdAt: string, text: string, rawText: string | null) => ({ id: createdAt, text, rawText, createdAt }) as unknown as TranscriptEntry;
    const today = new Date(2026, 10, 7, 9, 30).toISOString();
    const yesterday = new Date(2026, 10, 6, 9, 30).toISOString();
    const history = [
      take(today, "Hello there.", "um hello there"), // a fix
      take(today, "Same text", "Same text"), // nothing fixed
      take(today, "No raw", null), // no raw kept
      take(yesterday, "Fixed yesterday.", "uh fixed yesterday"), // not today
    ];
    expect(fixesToday(history, now)).toBe(1);
    expect(fixesToday(null, now)).toBe(0);
    expect(parseTakeTime("2026-11-07 09:30:00")?.toISOString()).toBe("2026-11-07T09:30:00.000Z");
    expect(parseTakeTime("not a date")).toBeNull();
    const none = paint((r) => drawBack(r, st({ fixesToday: null }), 0));
    const some = paint((r) => drawBack(r, st({ fixesToday: 12 }), 0));
    expect(differs(none, some)).toBeGreaterThan(0);
    expect(hexOf(some, PAD + 246, 77)).toBe("#7fdc7a"); // the "1" of 12 starts at x 245: its top row is 010
  });

  it("gulps_jar_fill_tracks_dictionary_size", () => {
    expect(jarCards(null)).toBe(0);
    expect(jarCards(0)).toBe(0);
    expect(jarCards(1)).toBe(1);
    expect(jarCards(701)).toBe(6);
    expect(jarCards(1000)).toBe(JAR_CARDS);
    expect(jarCards(50_000)).toBe(JAR_CARDS);
    for (let n = 0; n < 2000; n += 37) expect(jarCards(n + 37)).toBeGreaterThanOrEqual(jarCards(n));
    const empty = paint((r) => drawMid(r, st({ dictionarySize: 0 }), 0));
    const unknown = paint((r) => drawMid(r, st({ dictionarySize: null }), 0));
    const one = paint((r) => drawMid(r, st({ dictionarySize: 10 }), 0));
    const full = paint((r) => drawMid(r, st({ dictionarySize: 1000 }), 0));
    expect(differs(empty, unknown)).toBe(0);
    expect(differs(empty, one)).toBeGreaterThan(0);
    expect(differs(one, full)).toBeGreaterThan(0);
  });

  it("shellys_nook_shows_the_real_battery_and_charging_state", () => {
    expect(batteryLevel(null)).toBeNull();
    expect(batteryLevel({ percent: null, charging: true })).toBeNull();
    expect(batteryLevel({ percent: 80.4, charging: false })).toBe(80);
    expect(batteryLevel({ percent: 140, charging: true })).toBe(100);
    expect(shellyPluggedIn({ percent: 50, charging: true })).toBe(true);
    expect(shellyPluggedIn({ percent: 50, charging: false })).toBe(false);
    expect(shellyPluggedIn(null)).toBe(false);
    const high = paint((r) => drawMid(r, st({ battery: { percent: 90, charging: false } }), 0));
    const low = paint((r) => drawMid(r, st({ battery: { percent: 10, charging: false } }), 0));
    const charging = paint((r) => drawMid(r, st({ battery: { percent: 90, charging: true } }), 0));
    expect(differs(high, low)).toBeGreaterThan(0);
    expect(differs(high, charging)).toBeGreaterThan(0);
    // the meter's first cell: green when high, red when low
    expect(hexOf(high, PAD + 41, FLOOR - 43)).toBe("#5fb35a");
    expect(hexOf(low, PAD + 41, FLOOR - 43)).toBe("#e5484d");
  });

  it("embers_fireflies_are_streak_days_at_night_only_max_seven", () => {
    expect(fireflyCount(3, "night")).toBe(3);
    expect(fireflyCount(30, "night")).toBe(MAX_FIREFLIES);
    expect(MAX_FIREFLIES).toBe(7);
    expect(FIREFLY_SPOTS.length).toBeGreaterThanOrEqual(MAX_FIREFLIES);
    for (const band of ["dawn", "day", "dusk"] as const) expect(fireflyCount(5, band)).toBe(0);
    expect(fireflyCount(null, "night")).toBe(0);
    expect(fireflyCount(-2, "night")).toBe(0);
    const night = (streakDays: number) => paint((r) => drawLights(r, st({ phase: MIDNIGHT, sky: NIGHT_SKY, streakDays }), 0, 0, 0));
    const day = (streakDays: number) => paint((r) => drawLights(r, st({ phase: NOON, streakDays }), 0, 0, 0));
    expect(differs(night(0), night(3))).toBeGreaterThan(0);
    expect(differs(night(7), night(20))).toBe(0);
    expect(differs(day(0), day(5))).toBe(0);
  });

  it("the_real_clock_sky_is_kept", () => {
    const band = (h: number) => skyBand(skyPhase(new Date(2026, 10, 7, h, 0)));
    expect(band(6)).toBe("dawn");
    expect(band(12)).toBe("day");
    expect(band(18)).toBe("dusk");
    expect(band(23)).toBe("night");
    expect(band(3)).toBe("night");
    expect(roomTintAlpha({ ambA: 0 })).toBe(0);
    expect(roomTintAlpha({ ambA: 0.42 })).toBeLessThanOrEqual(MAX_TINT);
    // the window shows the sky it is given
    const w = paint((r) => drawLights(r, st({ sky: NIGHT_SKY, phase: MIDNIGHT }), 0, 0, 0));
    expect(hexOf(w, PAD + 15, 21)).toBe("#171a3d");
    const tinted = paint((r) => drawTint(r, st({ sky: NIGHT_SKY })), STUDIO_W);
    expect(tinted.px(10, 10).a).toBeGreaterThan(0);
  });

  it("yappy_works_at_his_desk_from_real_dictation_events", () => {
    expect(yappyPose({ appState: "listening", activity: "sleep", reaction: null })).toBe("work");
    expect(yappyPose({ appState: "thinking", activity: "idle", reaction: null })).toBe("think");
    expect(yappyPose({ appState: "idle", activity: "idle", reaction: "cheer" })).toBe("cheer");
    expect(yappyPose({ appState: "idle", activity: "sleep", reaction: null })).toBe("sleep");
    expect(yappyPose({ appState: "idle", activity: "water", reaction: null })).toBe("rest");
  });

  it("the_back_room_and_cinder_are_staged_in_the_same_room", () => {
    const quiet = paint((r) => drawMid(r, st({ newsReady: false }), 0));
    const news = paint((r) => drawMid(r, st({ newsReady: true }), 0));
    expect(differs(quiet, news)).toBeGreaterThan(0);
    const src = read("./studio.ts");
    for (const who of ['"sal"', '"mochi"', '"cinder"', '"hoot"', '"tidy"', '"gulp"', '"shelly"', '"ember"', '"yappy"']) expect(src).toContain(`stamp(o, ${who}`);
  });

  it("the frame and every layer paint whole pixels without throwing", () => {
    const s = st({ meetingRecording: true, fixesToday: 3, dictionarySize: 701, battery: { percent: 64, charging: true }, streakDays: 4, newsReady: true, yappy: "work" });
    for (const fn of LAYERS) expect(() => paint((r) => fn(r, s, 2.3))).not.toThrow();
    expect(() => paint((r) => drawFrame(r), STUDIO_W)).not.toThrow();
    const label = studioAriaLabel(s);
    expect(label).toContain("ON AIR light is on");
    expect(label).toContain("Tidy's counter shows 3 fixes today");
    expect(label).toContain("701 Dictionary words");
    expect(label).toContain("64% battery, charging");
    expect(label).toContain("Cinder is at the back door");
  });
});
