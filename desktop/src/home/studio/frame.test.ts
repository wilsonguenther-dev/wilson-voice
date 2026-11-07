/**
 * yap-1106-fb-24 — the Home room hangs in a wooden pixel frame (home.html
 * section 5). Samples the real room buffer: the frame is the artifact's,
 * byte-exact; it is drawn once into an offscreen cache; it never moves with
 * the parallax while the layers do; and the room is shown at an integer cell.
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";
import { Raster } from "../../sprites/raster";
import { pixelGrid } from "../../pill/pixelGrid";
import { skyPhase } from "../habitat/habitat";
import { ROOM_H, ROOM_SCALES, ROOM_W, roomBox } from "../homeLayout";
import {
  FRAME_F,
  FRAME_PAL,
  FRAME_PEGS,
  LAYERS,
  PAD,
  PLATE_W,
  PLATE_X,
  STUDIO_H,
  STUDIO_W,
  composeStudio,
  createFrameCache,
  drawFrame,
  isFramePixel,
  parallaxOffsets,
  type FrameCache,
  type StudioState,
} from "./studio";

const read = (rel: string): string => readFileSync(new URL(rel, import.meta.url), "utf8") as string;

const S: StudioState = {
  phase: skyPhase(new Date(2026, 10, 7, 12, 0)),
  sky: { top: "#7ec8ff", bot: "#cdeeff", amb: "#fff2d8", ambA: 0 },
  meetingRecording: false,
  fixesToday: 2,
  dictionarySize: 300,
  battery: { percent: 80, charging: false },
  streakDays: 3,
  newsReady: true,
  yappy: "rest",
  engineLoading: false,
  modelReady: true,
  screenSharing: false,
  doNotDisturb: false,
};

const hexOf = (r: Raster, x: number, y: number) => {
  const p = r.px(x, y);
  return "#" + [p.r, p.g, p.b].map((v) => v.toString(16).padStart(2, "0")).join("");
};
const same = (r: Raster, q: Raster, x: number, y: number) => {
  const a = r.px(x, y), b = q.px(x, y);
  return a.r === b.r && a.g === b.g && a.b === b.b && a.a === b.a;
};

/** The three layers painted exactly as YappyHouse paints them (PAD each side). */
function paintLayers(): Raster[] {
  return LAYERS.map((fn) => {
    const c = new Raster(STUDIO_W + PAD * 2, STUDIO_H);
    c.translate(PAD, 0);
    fn(c, S, 1);
    return c;
  });
}
function rasterFrameCache(): FrameCache<Raster> {
  return createFrameCache(() => {
    const r = new Raster(STUDIO_W, STUDIO_H);
    return { surface: r, ctx: r };
  });
}
/** One room buffer at a pointer position (px = the parallax value). */
function room(cache: FrameCache<Raster>, layers: Raster[], px: number): Raster {
  const out = new Raster(STUDIO_W, STUDIO_H);
  composeStudio(out, layers, cache.get(), parallaxOffsets(px), S, 1);
  return out;
}

describe("the wooden pixel frame (home.html section 5)", () => {
  it("frame_matches_the_artifact_pegs_plate_and_rails", () => {
    const out = room(rasterFrameCache(), paintLayers(), 0);
    // Corner pegs: a 2 x 2 brass peg at (2, 2) and each corner. The artifact
    // lights the peg's top-left pixel #fff2b0, so (2, 2) itself is the shine and
    // the other three peg pixels are the #e8c86a brass.
    for (const [x, y] of FRAME_PEGS) {
      expect(hexOf(out, x, y)).toBe(FRAME_PAL.pegShine);
      expect(hexOf(out, x + 1, y)).toBe(FRAME_PAL.peg);
      expect(hexOf(out, x, y + 1)).toBe(FRAME_PAL.peg);
      expect(hexOf(out, x + 1, y + 1)).toBe(FRAME_PAL.peg);
    }
    expect(FRAME_PEGS[0]).toEqual([2, 2]);
    expect(hexOf(out, 3, 3)).toBe("#e8c86a");
    // Dark edge on all four sides, lit top/left, shaded bottom/right.
    expect(hexOf(out, 40, 0)).toBe(FRAME_PAL.edgeLit);
    expect(hexOf(out, 0, 40)).toBe(FRAME_PAL.edgeLit);
    expect(hexOf(out, 40, STUDIO_H - 1)).toBe(FRAME_PAL.edgeShade);
    expect(hexOf(out, STUDIO_W - 1, 40)).toBe(FRAME_PAL.edgeShade);
    expect(hexOf(out, 40, 1)).toBe(FRAME_PAL.litTop);
    expect(hexOf(out, 1, 40)).toBe(FRAME_PAL.litLeft);
    expect(hexOf(out, 40, STUDIO_H - 2)).toBe(FRAME_PAL.shade);
    expect(hexOf(out, STUDIO_W - 2, 40)).toBe(FRAME_PAL.shade);
    for (let x = 0; x < STUDIO_W; x++) for (const y of [0, STUDIO_H - 1]) expect(out.px(x, y).a).toBe(255);
    for (let y = 0; y < STUDIO_H; y++) for (const x of [0, STUDIO_W - 1]) expect(out.px(x, y).a).toBe(255);
  });

  it("plate_spans_28_buffer_px_centred_on_the_bottom_rail", () => {
    const out = room(rasterFrameCache(), paintLayers(), 0);
    const y = STUDIO_H - FRAME_F; // the plate's lit line
    const lit: number[] = [];
    for (let x = 0; x < STUDIO_W; x++) if (hexOf(out, x, y) === FRAME_PAL.plateLit) lit.push(x);
    expect(lit.length).toBe(28);
    expect(PLATE_W).toBe(28);
    expect(lit[0]).toBe(PLATE_X);
    expect(lit[lit.length - 1]).toBe(PLATE_X + 27);
    // centred: equal margins left and right
    expect(lit[0]).toBe(STUDIO_W - 1 - lit[lit.length - 1]);
    expect((lit[0] + lit[lit.length - 1] + 1) / 2).toBe(STUDIO_W / 2);
    expect(hexOf(out, PLATE_X + 2, STUDIO_H - 1)).toBe(FRAME_PAL.plateShade);
    // six engraved ticks, no text
    let ticks = 0;
    for (let x = PLATE_X; x < PLATE_X + PLATE_W; x++) if (hexOf(out, x, STUDIO_H - 3) === FRAME_PAL.plateTick) ticks++;
    expect(ticks).toBe(12); // six 2 px ticks
  });

  it("frame_is_byte_identical_across_a_pointer_move_while_the_layers_shift", () => {
    const cache = rasterFrameCache();
    const layers = paintLayers();
    const rest = room(cache, layers, 0);
    const moved = room(cache, layers, 0.6); // the pointer at the canvas edge
    expect(parallaxOffsets(0.6)).not.toEqual([0, 0, 0]);
    let frameDiff = 0, frameCount = 0, roomDiff = 0;
    for (let y = 0; y < STUDIO_H; y++)
      for (let x = 0; x < STUDIO_W; x++) {
        if (isFramePixel(x, y)) {
          frameCount++;
          if (!same(rest, moved, x, y)) frameDiff++;
        } else if (!same(rest, moved, x, y)) roomDiff++;
      }
    expect(frameCount).toBeGreaterThan(0);
    expect(frameDiff).toBe(0);
    expect(roomDiff).toBeGreaterThan(0);
  });

  it("frame_is_rendered_once_and_every_later_redraw_is_a_cache_hit", () => {
    const cache = rasterFrameCache();
    const layers = paintLayers();
    const first = cache.get();
    for (let i = 0; i < 9; i++) room(cache, layers, (i % 3) * 0.3);
    expect(cache.stats.renders).toBe(1);
    expect(cache.stats.hits).toBe(9);
    expect(cache.get()).toBe(first);
    // the cached surface IS the artifact frame, nothing else
    const ref = new Raster(STUDIO_W, STUDIO_H);
    drawFrame(ref);
    for (let y = 0; y < STUDIO_H; y++) for (let x = 0; x < STUDIO_W; x++) expect(same(first, ref, x, y)).toBe(true);
    // the component paints through the cache, never drawFrame per redraw
    const src = read("../YappyHouse.tsx");
    expect(src).toContain("createFrameCache(");
    expect(src).toContain("frameCache.get()");
    expect(src).not.toMatch(/drawFrame\(/);
  });

  it("the_room_and_its_frame_are_shown_at_an_integer_scale_with_no_margin", () => {
    expect(ROOM_W).toBe(STUDIO_W);
    expect(ROOM_H).toBe(STUDIO_H);
    for (const col of [640, 900, 960, 1100, 1280, 1400]) {
      const b = roomBox(col);
      expect(Number.isInteger(b.scale)).toBe(true);
      expect(ROOM_SCALES).toContain(b.scale);
      for (const dpr of [1, 2]) {
        const g = pixelGrid({ w: b.width, h: b.height }, { w: STUDIO_W, h: STUDIO_H }, dpr);
        expect(Number.isInteger(g.cell)).toBe(true);
        expect(g.cell).toBe(b.scale * dpr);
        // edge to edge: the framed buffer fills the stage box exactly, nothing cropped
        expect(g.drawW).toBe(g.backingW);
        expect(g.drawH).toBe(g.backingH);
        expect(g.offsetX).toBe(0);
        expect(g.offsetY).toBe(0);
      }
    }
  });
});
