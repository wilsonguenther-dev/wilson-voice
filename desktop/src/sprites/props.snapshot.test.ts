/**
 * yap-1106-pill-props-detail — the pill's notepad, desk and typewriter must
 * equal the approved detail pass (design page pill.html section 11, the
 * `A.hd` branches of ./approved/yappy.js drawScene) pixel for pixel: ZERO
 * colour tolerance, ZERO position tolerance — same pixel size, same place.
 *
 * The approved script is read byte for byte (art.snapshot.test.ts pins its
 * sha256); only the in-memory copy run here also exports its drawScene.
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";
import type { PixelCtx } from "./art";
import { PAD_INK_FILL_SEC, PAD_INK_PX, TYPEWRITER_KEYS, drawDeskHD, drawNotepadHD, growPadInk, padInkPixels, typewriterBeat } from "./props";
import { Raster, snapDiff } from "./raster";

/** The approved scene buffer (yappy.js PW/PH/GY/FX) and the desk's chick shift. */
const PW = 176, PH = 44, GY = 30, FX = 88, DESK_SHIFT = 6, PAPER = "#fffdf5";

interface SceneA { world: number; prop: "none" | "pad" | "desk"; propIn: number; hd: boolean; hideChick: boolean }
type DrawScene = (o: PixelCtx, A: SceneA, elapsed: number) => void;

function loadApprovedScene(): DrawScene {
  const src = readFileSync(new URL("./approved/yappy.js", import.meta.url), "utf8") as string;
  const hook = "window.Yappy = { C: C,";
  if (!src.includes(hook)) throw new Error("approved yappy.js no longer has its window.Yappy export line");
  const exposed = src.replace(hook, "window.Yappy = { drawScene: drawScene, C: C,");
  const canvas = () => {
    const c = { width: 300, height: 150, r: null as Raster | null, dataset: {}, style: {}, getAttribute: () => "0",
      getContext(): Raster { if (!c.r || c.r.width !== c.width || c.r.height !== c.height) c.r = new Raster(c.width, c.height); return c.r; } };
    return c;
  };
  const win: Record<string, unknown> = { devicePixelRatio: 1, matchMedia: () => ({ matches: true }), addEventListener: () => undefined };
  new Function("window", "document", "performance", "requestAnimationFrame", exposed)(win, { createElement: canvas }, { now: () => 0 }, () => 0);
  return (win.Yappy as { drawScene: DrawScene }).drawScene;
}
const approvedScene = loadApprovedScene();

/** A PixelCtx over the test raster that also takes the art's `rgba(r,g,b,a)`
 * shadows (the raster itself is hex-only): the alpha folds into globalAlpha. */
class RgbaCtx implements PixelCtx {
  fillStyle: string | CanvasGradient | CanvasPattern = "#000000";
  constructor(readonly r: Raster) {}
  get globalAlpha(): number { return this.r.globalAlpha; }
  set globalAlpha(v: number) { this.r.globalAlpha = v; }
  fillRect(x: number, y: number, w: number, h: number): void {
    const s = String(this.fillStyle), m = /^rgba\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*([\d.]+)\s*\)$/.exec(s);
    if (!m) { this.r.fillStyle = s; this.r.fillRect(x, y, w, h); return; }
    const hex = "#" + [m[1], m[2], m[3]].map((v) => Number(v).toString(16).padStart(2, "0")).join("");
    const ga = this.r.globalAlpha;
    this.r.fillStyle = hex; this.r.globalAlpha = ga * Number(m[4]); this.r.fillRect(x, y, w, h); this.r.globalAlpha = ga;
  }
  clearRect(x: number, y: number, w: number, h: number): void { this.r.clearRect(x, y, w, h); }
}

function renderApproved(prop: "pad" | "desk", propIn: number, elapsed: number): Raster {
  const r = new Raster(PW, PH);
  approvedScene(new RgbaCtx(r), { world: 1, prop, propIn, hd: true, hideChick: true }, elapsed);
  return r;
}
/** The approved backdrop with NO prop, then the app's prop code on top. */
function renderApp(prop: "pad" | "desk", propIn: number, elapsed: number, ink = 1): Raster {
  const r = new Raster(PW, PH), o = new RgbaCtx(r);
  approvedScene(o, { world: 1, prop: "none", propIn: 0, hd: true, hideChick: true }, elapsed);
  if (prop === "pad") drawNotepadHD(o, FX, GY, propIn, PAPER, ink);
  else drawDeskHD(o, FX - DESK_SHIFT, GY, PW, propIn, elapsed, PAPER);
  return r;
}
const countColour = (r: Raster, hex: string): number => {
  const v = parseInt(hex.slice(1), 16), R = v >> 16, G = (v >> 8) & 255, B = v & 255;
  let n = 0;
  for (let y = 0; y < r.height; y++) for (let x = 0; x < r.width; x++) { const p = r.px(x, y); if (p.r === R && p.g === G && p.b === B && p.a === 255) n++; }
  return n;
};

describe("pill props equal the approved detail pass", () => {
  it("props_notepad_matches_approved_artifact", () => {
    for (const propIn of [1, 0.6, 0.2]) {
      const d = snapDiff(renderApp("pad", propIn, 1.2), renderApproved("pad", propIn, 1.2), 0);
      expect(d.mismatched, `notepad propIn=${propIn}`).toBe(0);
    }
  });

  it("props_desk_typewriter_mug_match_approved_artifact_every_beat", () => {
    for (const propIn of [1, 0.5]) for (const t of [0, 0.12, 0.25, 0.4, 0.55, 1.2, 3.7]) {
      const d = snapDiff(renderApp("desk", propIn, t), renderApproved("desk", propIn, t), 0);
      expect(d.mismatched, `desk propIn=${propIn} t=${t}`).toBe(0);
    }
  });

  it("props_stay_at_todays_place_and_scale", () => {
    // the desk spans x 4..PW-4 with its top on the row GY-2, as the shipped desk did
    const r = renderApp("desk", 1, 0);
    expect(r.px(4, GY - 2)).toMatchObject({ r: 0x5a, g: 0x34, b: 0x20, a: 255 });
    expect(r.px(PW - 5, GY - 2)).toMatchObject({ r: 0x5a, g: 0x34, b: 0x20, a: 255 });
    expect(r.px(PW - 4, GY - 2)).not.toMatchObject({ r: 0x5a, g: 0x34, b: 0x20 });
    // the notepad sits to the right of the chick (cx + 12) on the grass line
    const p = renderApp("pad", 1, 0);
    expect(p.px(FX + 12, GY - 13)).toMatchObject({ r: 0x8a, g: 0x7a, b: 0x5a, a: 255 });
  });

  it("props_notepad_has_clip_and_margin_line", () => {
    const p = renderApp("pad", 1, 0);
    expect(countColour(p, "#9aa3ad"), "clip").toBeGreaterThan(0);
    expect(countColour(p, "#f2b8b0"), "margin line").toBe(9);
  });

  it("props_desk_has_grain_edge_highlight_levers_mug_and_pencils", () => {
    const r = renderApp("desk", 1, 0);
    expect(countColour(r, "#a8683a"), "wood grain").toBeGreaterThan(0);
    // the typewriter's 22 px contact shadow falls on the highlight row
    expect(countColour(r, "#d8955a"), "edge highlight").toBe(PW - 10 - 22);
    expect(countColour(r, "#c8d0d8"), "return lever caps").toBe(2);
    expect(countColour(r, "#7a4626"), "coffee in the mug + desk underside").toBeGreaterThan(PW - 10);
    expect(countColour(r, "#5a9fd6"), "blue pencil").toBe(4);
    expect(countColour(r, "#ffcf3a"), "yellow pencil").toBe(5);
  });
});

describe("pill props motion", () => {
  it("props_handwriting_grows_in_reading_order", () => {
    const ink = (k: number) => countColour(renderApp("pad", 1, 0, k), "#4a4f5c");
    expect(ink(0)).toBe(0);
    let prev = -1;
    for (let k = 0; k <= 1.0001; k += 0.05) { const n = ink(k); expect(n).toBeGreaterThanOrEqual(prev); prev = n; }
    expect(ink(1)).toBe(PAD_INK_PX);
    expect(ink(0.5)).toBe(padInkPixels(0.5));
    expect(padInkPixels(Number.NaN)).toBe(PAD_INK_PX);
  });

  it("props_handwriting_fills_while_noting_then_stays", () => {
    let k = 0;
    for (let i = 0; i < Math.round(PAD_INK_FILL_SEC * 20); i++) k = growPadInk(k, 0.05);
    expect(k).toBeCloseTo(1, 6);
    expect(growPadInk(k, 10)).toBe(1);
    expect(growPadInk(0.3, -1)).toBe(0.3);
    expect(growPadInk(0.25, PAD_INK_FILL_SEC / 2)).toBeCloseTo(0.75, 6);
  });

  it("props_hidden_until_they_start_to_arrive_and_bad_clocks_are_safe", () => {
    const empty = () => { const r = new Raster(PW, PH); approvedScene(new RgbaCtx(r), { world: 1, prop: "none", propIn: 0, hd: true, hideChick: true }, 0); return r; };
    for (const propIn of [0, 0.05, Number.NaN]) {
      expect(snapDiff(renderApp("pad", propIn, 0), empty(), 0).mismatched, `pad propIn=${propIn}`).toBe(0);
      expect(snapDiff(renderApp("desk", propIn, 0), empty(), 0).mismatched, `desk propIn=${propIn}`).toBe(0);
    }
    expect(typewriterBeat(Number.NaN)).toEqual({ key: 0, lift: 1 });
    expect(typewriterBeat(-3)).toEqual({ key: 0, lift: 1 });
    expect(growPadInk(Number.NaN, 0.6)).toBeCloseTo(0.6 / PAD_INK_FILL_SEC, 6);
    expect(padInkPixels(2)).toBe(PAD_INK_PX);
    expect(padInkPixels(-1)).toBe(0);
  });

  it("props_typewriter_keys_light_in_sequence_and_page_bounces", () => {
    const keys: number[] = [];
    for (let i = 0; i < TYPEWRITER_KEYS * 2; i++) keys.push(typewriterBeat(i / 9 + 0.01).key);
    expect(keys).toEqual([0, 1, 2, 3, 4, 5, 0, 1, 2, 3, 4, 5]);
    const lifts = [0, 1, 2, 3, 4, 5].map((i) => typewriterBeat(i / 6 + 0.01).lift);
    expect(lifts).toEqual([1, 0, 0, 1, 0, 0]);
    // exactly ONE lit key per frame, and it moves
    const lit = (t: number) => countColour(renderApp("desk", 1, t), "#ffd23f");
    expect(lit(0.01)).toBe(2);
    expect(snapDiff(renderApp("desk", 1, 0.01), renderApp("desk", 1, 0.12), 0).mismatched).toBeGreaterThan(0);
  });
});
