/**
 * yap-1106-pill-sky — the pill's sky is the Home room's time of day (option C,
 * companions.html section 9). What this file holds the sky to:
 *
 *  - the colours ARE the room's (a drift guard against home/YappyHouse.tsx),
 *  - the room's clock picks the time of day,
 *  - the draw budget: every redraw is ONE drawImage, the backdrop is repainted
 *    only when the time of day changes, the clock is read at most every 30 s,
 *    and the idle redraw schedule is exactly what it was (framePlan untouched),
 *  - the 22-px nub stays legible under all four skies (pixel snapshot on the
 *    same software raster the sprite snapshot uses).
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";
import {
  ROOM_SKY, SKY_BANDS, SKY_CHECK_MS, SKY_SCENE, SKY_TIMES, SkyBackdrop, blitSky, mixHex, paintSky, skyForced,
  skyLook, skyTimeAt, skyTimeFromPhase, type SkyBlitCtx, type SkyPaintCtx, type SkyTime,
} from "./sky";
import { AMBIENT_FRAME_MS, framePlan } from "./live";
import { drawChick, type ChickPose } from "../sprites/art";
import { Raster, type Rgba } from "../sprites/raster";

const readSibling = (rel: string): string => readFileSync(new URL(rel, import.meta.url), "utf8") as string;

/* ── WCAG contrast, for the nub check ─────────────────────────────────── */
const lin = (v: number) => { const c = v / 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
const lumRgb = (c: { r: number; g: number; b: number }) => 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b);
const lumHex = (h: string) => lumRgb({ r: parseInt(h.slice(1, 3), 16), g: parseInt(h.slice(3, 5), 16), b: parseInt(h.slice(5, 7), 16) });
const contrast = (a: number, b: number) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
const toHex = (p: Rgba) => "#" + [p.r, p.g, p.b].map((v) => v.toString(16).padStart(2, "0")).join("");

/** A backdrop painted into the test raster, pinned to one time of day. */
function rasterBackdrop(time: SkyTime): SkyBackdrop<Raster> {
  const r = new Raster(SKY_SCENE.w, SKY_SCENE.h);
  return new SkyBackdrop<Raster>({ image: r, ctx: r }, SKY_SCENE.w, SKY_SCENE.h, SKY_SCENE.gy, () => new Date(0), time);
}

describe("yap-1106-pill-sky — the Home room's time of day", () => {
  it("the_keyframes_are_the_home_rooms_own_colours", () => {
    const room = readSibling("../home/YappyHouse.tsx");
    for (const t of SKY_TIMES) {
      const k = ROOM_SKY[t];
      // the room's SKY table line for this keyframe carries the same pair
      expect(room).toMatch(new RegExp(`top: "${k.top}", bot: "${k.bot}"`));
    }
    expect(SKY_TIMES.map((t) => ROOM_SKY[t].at)).toEqual([0, 0.25, 0.5, 0.75]);
  });

  it("the_room_clock_picks_the_time_of_day", () => {
    const at = (h: number, m = 0) => skyTimeAt(new Date(2026, 10, 6, h, m));
    expect(at(6)).toBe("dawn");
    expect(at(12)).toBe("day");
    expect(at(18)).toBe("dusk");
    expect(at(0)).toBe("night");
    // snaps to the NEAREST keyframe: changes at 9am, 3pm, 9pm, 3am
    expect(at(8, 59)).toBe("dawn");
    expect(at(9, 0)).toBe("day");
    expect(at(14, 59)).toBe("day");
    expect(at(15, 0)).toBe("dusk");
    expect(at(20, 59)).toBe("dusk");
    expect(at(21, 0)).toBe("night");
    expect(at(2, 59)).toBe("night");
    expect(at(3, 0)).toBe("dawn");
    expect(skyTimeFromPhase(1.25)).toBe("day");
    expect(skyTimeFromPhase(-0.25)).toBe("night");
  });

  it("each_time_of_day_draws_six_room_bands_snapshot", () => {
    expect(Object.fromEntries(SKY_TIMES.map((t) => [t, skyLook(t).bands]))).toEqual({
      dawn: ["#f6a6a0", "#f8b0a0", "#fabaa0", "#fbc5a0", "#fdcfa0", "#ffd9a0"],
      day: ["#7ec8ff", "#8ed0ff", "#9ed7ff", "#addfff", "#bde6ff", "#cdeeff"],
      dusk: ["#ff9d6b", "#ffa86e", "#ffb271", "#ffbd74", "#ffc777", "#ffd27a"],
      night: ["#171a3d", "#1b1e44", "#1f234b", "#242752", "#282c59", "#2c3060"],
    });
    for (const t of SKY_TIMES) {
      const l = skyLook(t);
      expect(l.bands).toHaveLength(SKY_BANDS);
      expect(l.bands[0]).toBe(ROOM_SKY[t].top);
      expect(l.bands[SKY_BANDS - 1]).toBe(ROOM_SKY[t].bot);
      // only the night has a moon and stars
      expect(l.disc.moon).toBe(t === "night");
      expect(l.stars.length > 0).toBe(t === "night");
    }
    expect(mixHex("#000000", "#ffffff", 0.5)).toBe("#808080");
  });

  it("the_matrix_pin_accepts_only_the_four_times", () => {
    expect(skyForced("night")).toBe("night");
    expect(skyForced("dawn")).toBe("dawn");
    expect(skyForced("noon")).toBeNull();
    expect(skyForced("")).toBeNull();
    expect(skyForced(null)).toBeNull();
    expect(skyForced(undefined)).toBeNull();
  });
});

describe("yap-1106-pill-sky — the draw budget", () => {
  /** A scene context that only counts what a redraw costs. */
  function countingScene<S>() {
    const c = { globalAlpha: 1, drawImage: 0, fillRect: 0 };
    const ctx: SkyBlitCtx<S> & { fillRect(): void } = {
      get globalAlpha() { return c.globalAlpha; },
      set globalAlpha(v: number) { c.globalAlpha = v; },
      drawImage: () => { c.drawImage++; },
      fillRect: () => { c.fillRect++; },
    };
    return { c, ctx };
  }
  /** A surface context that counts the fills a (re)paint costs. */
  function countingSurface() {
    const c = { fills: 0, clears: 0 };
    const ctx: SkyPaintCtx = { fillStyle: "#000000", globalAlpha: 1, fillRect: () => { c.fills++; }, clearRect: () => { c.clears++; } };
    return { c, ctx };
  }

  it("each_redraw_is_one_drawImage_and_no_fills", () => {
    const surf = countingSurface();
    const bg = new SkyBackdrop<object>({ image: {}, ctx: surf.ctx }, SKY_SCENE.w, SKY_SCENE.h, SKY_SCENE.gy, () => new Date(2026, 10, 6, 12, 0));
    const scene = countingScene<object>();
    for (let i = 0; i < 50; i++) blitSky(scene.ctx, bg, i * AMBIENT_FRAME_MS, 0.8);
    expect(scene.c.drawImage).toBe(50);
    expect(scene.c.fillRect).toBe(0);
    expect(scene.c.globalAlpha).toBe(1);            // left as it found it
    expect(bg.builds).toBe(1);                        // painted once, then cached
    expect(surf.c.clears).toBe(1);
    expect(surf.c.fills).toBeGreaterThan(0);
    expect(surf.c.fills).toBeLessThan(120);           // the whole backdrop, once
  });

  it("three_hours_of_ambient_ticks_repaint_only_on_a_time_of_day_change", () => {
    // 07:30 -> 10:30 crosses exactly one change (dawn -> day at 9:00).
    const start = new Date(2026, 10, 6, 7, 30).getTime();
    let reads = 0;
    const surf = countingSurface();
    const bg = new SkyBackdrop<object>({ image: {}, ctx: surf.ctx }, SKY_SCENE.w, SKY_SCENE.h, SKY_SCENE.gy, (() => { reads++; return new Date(start + now); }));
    const scene = countingScene<object>();
    let now = 0;
    const frames = (3 * 3600 * 1000) / AMBIENT_FRAME_MS;
    expect(frames).toBe(108_000);
    for (let i = 0; i < frames; i++) { now = i * AMBIENT_FRAME_MS; blitSky(scene.ctx, bg, now, 1); }
    expect(scene.c.drawImage).toBe(frames);
    expect(scene.c.fillRect).toBe(0);
    expect(bg.builds).toBe(2);
    expect(bg.look?.time).toBe("day");
    // the clock is read every SKY_CHECK_MS, never per frame
    expect(reads).toBe(Math.ceil((3 * 3600 * 1000) / SKY_CHECK_MS));
  });

  it("a_whole_day_repaints_the_backdrop_once_per_time_of_day", () => {
    const start = new Date(2026, 10, 6, 7, 30).getTime();
    let now = 0;
    const surf = countingSurface();
    const seen: SkyTime[] = [];
    const bg = new SkyBackdrop<object>({ image: {}, ctx: surf.ctx }, SKY_SCENE.w, SKY_SCENE.h, SKY_SCENE.gy, () => new Date(start + now), null, (l) => { seen.push(l.time); });
    for (now = 0; now <= 24 * 3600 * 1000; now += SKY_CHECK_MS) bg.current(now);
    expect(seen).toEqual(["dawn", "day", "dusk", "night", "dawn"]);
    expect(bg.builds).toBe(5);
  });

  it("a_pinned_sky_never_reads_the_clock", () => {
    let reads = 0;
    const surf = countingSurface();
    const bg = new SkyBackdrop<object>({ image: {}, ctx: surf.ctx }, SKY_SCENE.w, SKY_SCENE.h, SKY_SCENE.gy, () => { reads++; return new Date(); }, "dusk");
    for (let i = 0; i < 20; i++) bg.current(i * SKY_CHECK_MS);
    expect(reads).toBe(0);
    expect(bg.builds).toBe(1);
    expect(bg.look?.time).toBe("dusk");
  });

  it("idle_redraw_count_is_unchanged", () => {
    // The sky has nothing that moves, so it asks the frame policy for nothing:
    // the settled idle pill keeps the same 10fps ambient tick, a meeting and a
    // hidden pill still park it, reduced motion is still one calm frame.
    const quiet = { level: 0, busyVisuals: false, reduceMotion: false, settled: true, hidden: false, meetingRecording: false };
    expect(framePlan("idle", quiet)).toEqual({ mode: "ambient", intervalMs: AMBIENT_FRAME_MS });
    expect(AMBIENT_FRAME_MS).toBe(100);
    expect(framePlan("idle", { ...quiet, meetingRecording: true }).mode).toBe("parked");
    expect(framePlan("idle", { ...quiet, hidden: true }).mode).toBe("parked");
    expect(framePlan("idle", { ...quiet, reduceMotion: true }).mode).toBe("parked");
    // …and the pill draws the sky through the cached blit, with no drifting clouds left.
    const pill = readSibling("./YappyPill.tsx");
    expect(pill).toMatch(/blitSky<HTMLCanvasElement>\(octx, sky, performance\.now\(\), Math\.max\(0\.8, w\)\)/);
    expect(pill).not.toMatch(/C\.cloud|C\.sky1|C\.sky2/);
  });
});

describe("yap-1106-pill-sky — the nub stays legible", () => {
  // The resting capsule is 72 x 22 pt; inside its 1-pt rim the camera shows a
  // 26-art-px-wide view centred on Yappy's face (FX 88, FY 18), at the capsule
  // aspect (70 x 20) — YappyPill's `viewW = lerp(26, …)` at openV 0.
  const FX = 88, FY = 18, VIEW_W = 26, VIEW_H = VIEW_W * (20 / 70);
  const X0 = Math.floor(FX - VIEW_W / 2), X1 = Math.ceil(FX + VIEW_W / 2);
  const Y0 = Math.floor(FY - VIEW_H / 2), Y1 = Math.ceil(FY + VIEW_H / 2);
  const REST: ChickPose = { hop: 0, sx: 0, sy: 0, blink: 0, mood: 0, beakF: 0, sway: 0, flap: 0, look: "fwd", glasses: false, pencil: false, droop: false };
  const OUTLINE = "#6b3a2e", BODY = "#ffd24a"; // sprites/art.ts CHICK_COLORS

  function nub(time: SkyTime) {
    const bg = rasterBackdrop(time);
    const scene = new Raster(SKY_SCENE.w, SKY_SCENE.h);
    scene.imageSmoothingEnabled = false;
    blitSky(scene, bg, 0, 1);
    const back = bg.current(0);
    drawChick(scene, FX, SKY_SCENE.gy, REST);
    const mask: string[] = [], sky: string[] = [];
    for (let y = Y0; y < Y1; y++) for (let x = X0; x < X1; x++) {
      const a = scene.px(x, y), b = back.px(x, y);
      if (a.r !== b.r || a.g !== b.g || a.b !== b.b || a.a !== b.a) mask.push(`${x},${y}`);
      else sky.push(toHex(b));
    }
    return { mask, sky, look: bg.look! };
  }

  it("the_nub_view_is_plain_room_sky_behind_a_full_chick_at_every_time", () => {
    const day = nub("day");
    expect(day.mask.length).toBeGreaterThan(40);   // Yappy's face fills the nub
    for (const t of SKY_TIMES) {
      const n = nub(t);
      // the same silhouette under every sky: the sky never eats an edge
      expect(n.mask).toEqual(day.mask);
      // nothing but the room's bands behind him — no sun, moon or star in the nub
      for (const c of n.sky) expect(n.look.bands).toContain(c);
    }
  });

  it("yappy_clears_5_to_1_against_every_sky_pixel_in_the_nub", () => {
    const lo = lumHex(OUTLINE), lb = lumHex(BODY);
    for (const t of SKY_TIMES) {
      const n = nub(t);
      expect(n.sky.length).toBeGreaterThan(0);
      for (const c of new Set(n.sky)) {
        const l = lumHex(c);
        expect(Math.max(contrast(lo, l), contrast(lb, l)), `${t} ${c}`).toBeGreaterThanOrEqual(5);
      }
    }
  });

  it("the_backdrop_is_whole_pixels_only", () => {
    for (const t of SKY_TIMES) {
      const r = new Raster(SKY_SCENE.w, SKY_SCENE.h);
      paintSky(r, skyLook(t), SKY_SCENE.w, SKY_SCENE.h, SKY_SCENE.gy);
      expect(r.subpixel, t).toBe(0);
      // the grass starts at GY under every sky
      expect(toHex(r.px(10, SKY_SCENE.gy))).toBe(skyLook(t).grassD);
      expect(toHex(r.px(10, SKY_SCENE.gy + 5))).toBe(skyLook(t).grass);
    }
  });
});

describe("yap-1106-stuck-pill-keeps-its-scene SkyBackdrop.invalidate (integrator, loop3 batch 3)", () => {
  it("invalidate makes the next current repaint the cached surface from scratch", () => {
    const ctx: SkyPaintCtx = { fillStyle: "#000000", globalAlpha: 1, fillRect: () => {}, clearRect: () => {} };
    const bg = new SkyBackdrop<object>({ image: {}, ctx }, SKY_SCENE.w, SKY_SCENE.h, SKY_SCENE.gy, () => new Date(2026, 10, 6, 12, 0));
    bg.current(0);
    bg.current(1);
    expect(bg.builds).toBe(1);
    bg.invalidate();
    bg.current(2);
    expect(bg.builds).toBe(2);
  });
});
