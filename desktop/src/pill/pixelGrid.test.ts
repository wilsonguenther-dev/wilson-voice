/**
 * pixelGrid.test — yap24-UI1. Every sprite pixel lands on whole device pixels
 * at every dock size and backing scale.
 *
 * The invariants, checked over DPR 1 and 2, all three docks (bottom / left /
 * right, the capsule at rest and fully open), the 30 px side dock and the Home
 * room:
 *   - the cell is a whole number of device px, and the LARGEST one that fits;
 *   - the backing store and every offset / destination edge are whole device px;
 *   - the destination is exactly source x cell (never stretched);
 *   - the margin is covered by scene drawn at the same cell (never left blank,
 *     never stretched).
 */
import { describe, expect, it } from "vitest";
// No @types/node in this project (see live.test.ts).
// @ts-expect-error -- node builtin, untyped here; the test runs under Node.
import { readFileSync } from "node:fs";
import {
  backingScale, dockCentreX, fillSpan, fitCell, pixelGrid, sceneBlit, snapRect, toDevice,
  DOCKS, DOCK_PAD, type PixelGrid, type SceneBlit,
} from "./pixelGrid";

const readSibling = (rel: string): string =>
  readFileSync(new URL(rel, import.meta.url), "utf8") as string;

const DPRS = [1, 2] as const;

/** The float window (float_pill.rs PILL_W x PILL_H, logical points). */
const FLOAT = { w: 300, h: 140 };
/** YappyPill's capsule at rest and fully open (no prop tier), and its camera view. */
const CAPSULE = {
  rest: { capW: 72, capH: 22, viewW: 26, centre: { x: 88, y: 18 } },
  open: { capW: 236, capH: 60, viewW: 176 * 0.8, centre: { x: 88, y: 22 } },
} as const;

const isInt = (n: number) => Number.isInteger(n);

function expectWholeGrid(g: PixelGrid, gridW: number, gridH: number) {
  for (const n of [g.backingW, g.backingH, g.cell, g.drawW, g.drawH, g.offsetX, g.offsetY]) expect(isInt(n)).toBe(true);
  expect(g.cell).toBeGreaterThanOrEqual(1);
  expect(g.drawW).toBe(gridW * g.cell);
  expect(g.drawH).toBe(gridH * g.cell);
  // centred: the two margins differ by at most one device px
  const l = g.offsetX, r = g.backingW - g.offsetX - g.drawW;
  const t = g.offsetY, b = g.backingH - g.offsetY - g.drawH;
  expect(Math.abs(l - r)).toBeLessThanOrEqual(1);
  expect(Math.abs(t - b)).toBeLessThanOrEqual(1);
}

function expectLargestFit(g: PixelGrid, gridW: number, gridH: number) {
  if (gridW <= g.backingW && gridH <= g.backingH) {
    expect(g.drawW).toBeLessThanOrEqual(g.backingW);
    expect(g.drawH).toBeLessThanOrEqual(g.backingH);
    // one more device px per art px would no longer fit
    expect((g.cell + 1) * gridW > g.backingW || (g.cell + 1) * gridH > g.backingH).toBe(true);
  } else {
    expect(g.cell).toBe(1);   // even 1:1 overflows: centred and cropped, never shrunk below a device px
  }
}

function expectWholeBlit(b: SceneBlit) {
  for (const r of [b.clip, b.src, b.dst]) for (const n of [r.x, r.y, r.w, r.h]) expect(isInt(n)).toBe(true);
  expect(isInt(b.cell)).toBe(true);
  expect(b.dst.w).toBe(b.src.w * b.cell);   // never stretched
  expect(b.dst.h).toBe(b.src.h * b.cell);
  // the scene covers the whole clip window — no unfilled margin
  expect(b.dst.x).toBeLessThanOrEqual(b.clip.x);
  expect(b.dst.y).toBeLessThanOrEqual(b.clip.y);
  expect(b.dst.x + b.dst.w).toBeGreaterThanOrEqual(b.clip.x + b.clip.w);
  expect(b.dst.y + b.dst.h).toBeGreaterThanOrEqual(b.clip.y + b.clip.h);
  // ...and by no more than one cell per side (the fill is the margin, not a stretch)
  expect(b.clip.x - b.dst.x).toBeLessThan(b.cell);
  expect(b.clip.y - b.dst.y).toBeLessThan(b.cell);
  expect(b.dst.x + b.dst.w - (b.clip.x + b.clip.w)).toBeLessThan(b.cell);
  expect(b.dst.y + b.dst.h - (b.clip.y + b.clip.h)).toBeLessThan(b.cell);
  // the requested view fits at that cell, and is the largest cell that does
  expect(b.gridW * b.cell).toBeLessThanOrEqual(b.clip.w);
  expect(b.gridH * b.cell).toBeLessThanOrEqual(b.clip.h);
  expect((b.cell + 1) * b.gridW > b.clip.w || (b.cell + 1) * b.gridH > b.clip.h).toBe(true);
}

/** The blit YappyPill makes for a dock, a capsule state and a DPR — the same arithmetic as the draw loop. */
function pillBlit(dock: string, state: keyof typeof CAPSULE, dpr: number, canvas = FLOAT): SceneBlit {
  const c = CAPSULE[state];
  const cx = dockCentreX(dock, canvas.w, c.capW), cy = canvas.h * 0.56;
  const x0 = cx - c.capW / 2, y0 = cy - c.capH / 2, iw = c.capW - 2, ih = c.capH - 2;
  return sceneBlit({ rect: { x: x0 + 1, y: y0 + 1, w: iw, h: ih }, view: { w: c.viewW, h: c.viewW * (ih / iw) }, centre: c.centre, dpr });
}

describe("pixelGrid — the pure fit", () => {
  it("backing scale is the real DPR, never clamped; junk reads as 1", () => {
    expect(backingScale(2)).toBe(2);
    expect(backingScale(3)).toBe(3);   // the old Math.min(dpr, 2) clamp left a 1.5x CSS upscale here
    expect(backingScale(1.5)).toBe(1.5);
    for (const junk of [0, -1, NaN, Infinity, undefined, null]) expect(backingScale(junk)).toBe(1);
  });

  it("the backing store is whole device px, at least 1", () => {
    expect(toDevice(300, 2)).toBe(600);
    expect(toDevice(150.3, 2)).toBe(301);
    expect(toDevice(0, 2)).toBe(1);
  });

  it("the cell is the LARGEST integer that fits — the 2.69x / 5.38x rest pill is 2x / 5x", () => {
    expect(fitCell(70, 20, 26, 7)).toBe(2);    // DPR 1: 70/26 = 2.69 used to straddle device px
    expect(fitCell(140, 40, 26, 7)).toBe(5);   // DPR 2: 140/26 = 5.38
    expect(fitCell(960, 640, 192, 128)).toBe(5);
    expect(fitCell(959, 640, 192, 128)).toBe(4);
    expect(fitCell(10, 10, 26, 7)).toBe(1);    // overflow never shrinks below 1
  });

  for (const dpr of DPRS) {
    it(`DPR ${dpr}: the Home room (192x128) on every stage width the CSS can produce`, () => {
      for (const cssW of [240, 330, 450, 451, 517, 600, 812]) {
        const cssH = Math.min(300, cssW * (2 / 3));   // App.css: aspect-ratio 3/2, max-height 300
        const g = pixelGrid({ w: cssW, h: cssH }, { w: 192, h: 128 }, dpr);
        expect(g.backingW).toBe(Math.round(cssW * dpr));
        expect(g.backingH).toBe(Math.round(cssH * dpr));
        expectWholeGrid(g, 192, 128);
        expectLargestFit(g, 192, 128);
      }
    });

    it(`DPR ${dpr}: the 30 px side dock — a 30 px-wide box still gets a whole-px grid`, () => {
      for (const [grid, box] of [
        [{ w: 26, h: 7 }, { w: 30, h: 140 }],   // Yappy's resting face view in a 30 px strip
        [{ w: 26, h: 26 }, { w: 30, h: 140 }],  // a square sprite in the strip
        [{ w: 44, h: 44 }, { w: 30, h: 30 }],   // art larger than the strip at 1:1 (DPR 1)
      ] as const) {
        const g = pixelGrid(box, grid, dpr);
        expect(g.backingW).toBe(30 * dpr);
        expectWholeGrid(g, grid.w, grid.h);
        expectLargestFit(g, grid.w, grid.h);
      }
      // concrete numbers, so a regression names itself
      expect(pixelGrid({ w: 30, h: 140 }, { w: 26, h: 7 }, dpr).cell).toBe(dpr === 1 ? 1 : 2);
      const crop = pixelGrid({ w: 30, h: 30 }, { w: 44, h: 44 }, dpr);
      expect(crop.cell).toBe(1);
      expect(crop.offsetX).toBe(dpr === 1 ? -7 : 8);
    });

    for (const dock of DOCKS) {
      for (const state of ["rest", "open"] as const) {
        it(`DPR ${dpr}: ${dock} dock, capsule ${state} — the camera blit is whole device px, scene-filled`, () => {
          expectWholeBlit(pillBlit(dock, state, dpr));
        });
      }
    }

    it(`DPR ${dpr}: every frame of the open/close camera tween stays on whole device px`, () => {
      for (const dock of DOCKS) {
        for (let i = 0; i <= 40; i++) {
          const t = i / 40;
          const capW = 72 + (236 - 72) * t, capH = 22 + (60 - 22) * t;
          const cx = dockCentreX(dock, FLOAT.w, capW), cy = FLOAT.h * 0.56;
          const iw = capW - 2, ih = capH - 2, viewW = 26 + (176 * 0.8 - 26) * t;
          expectWholeBlit(sceneBlit({
            rect: { x: cx - capW / 2 + 1, y: cy - capH / 2 + 1, w: iw, h: ih },
            view: { w: viewW, h: viewW * (ih / iw) }, centre: { x: 88, y: 18 + (22 - 18) * t }, dpr,
          }));
        }
      }
    });
  }

  it("the three docks anchor where float.css parks the capsule", () => {
    expect(dockCentreX("bottom", 300, 72)).toBe(150);
    expect(dockCentreX("left", 300, 72)).toBe(DOCK_PAD + 36);
    expect(dockCentreX("right", 300, 72)).toBe(300 - DOCK_PAD - 36);
    expect(dockCentreX(undefined, 300, 72)).toBe(150);
    expect(readSibling("../float.css")).toMatch(/data-dock="left"\] \.stage \{\s*justify-content: flex-start;\s*padding-left: 10px;/);
  });

  it("concrete pill numbers: the resting face is 2x at DPR 1 and 5x at DPR 2, never 2.69x / 5.38x", () => {
    expect(pillBlit("bottom", "rest", 1).cell).toBe(2);
    expect(pillBlit("bottom", "rest", 2).cell).toBe(5);
    expect(pillBlit("left", "open", 2).cell).toBe(3);
  });

  it("snapRect: shared CSS edges stay shared device edges", () => {
    const a = snapRect({ x: 10.3, y: 0, w: 20.4, h: 5 }, 2);
    const b = snapRect({ x: 30.7, y: 0, w: 5, h: 5 }, 2);
    expect(a.x + a.w).toBe(b.x);
  });

  it("fillSpan: margins are covered by whole art px at the same cell", () => {
    const g = pixelGrid({ w: 451, h: 300 }, { w: 192, h: 128 }, 1);
    const f = fillSpan(g);
    expect(f.left * g.cell).toBeGreaterThanOrEqual(g.offsetX);
    expect(f.right * g.cell).toBeGreaterThanOrEqual(g.backingW - g.offsetX - g.drawW);
  });
});

describe("pixelGrid — reachability: the canvases that draw pixel art use it", () => {
  const pill = readSibling("./YappyPill.tsx");
  const house = readSibling("../home/YappyHouse.tsx");

  it("YappyPill draws the camera through sceneBlit on an unclamped backing scale", () => {
    expect(pill).toMatch(/from "\.\/pixelGrid"/);
    expect(pill).toMatch(/sceneBlit\(/);
    expect(pill).toMatch(/toDevice\(W, DPR\)/);
    expect(pill).not.toMatch(/Math\.min\(devicePixelRatio/);
    // the old fractional camera blit: drawImage(os, vcx - viewW / 2, …, x0 + 1, y0 + 1, iw, ih)
    expect(pill).not.toMatch(/drawImage\(os, vcx/);
    // the dock anchor has ONE definition
    expect(pill).toMatch(/dockCentreX\(/);
    expect(pill).not.toMatch(/const DOCK_PAD/);
  });

  it("YappyHouse sizes its backing store from the grid, not a fixed 5x", () => {
    expect(house).toMatch(/from "\.\.\/pill\/pixelGrid"/);
    expect(house).toMatch(/pixelGrid\(\{ w: r\.width, h: r\.height \}, \{ w: W, h: H \}, devicePixelRatio\)/);
    expect(house).not.toMatch(/W \* S, H \* S/);
    expect(house).toMatch(/grid\.offsetX, grid\.offsetY, grid\.drawW, grid\.drawH/);
  });

  it("ClassicPill draws no pixel art: it is a DOM capsule with no canvas", () => {
    const classic = readSibling("./ClassicPill.tsx");
    expect(classic).not.toMatch(/getContext\(|<canvas/);
  });
});
