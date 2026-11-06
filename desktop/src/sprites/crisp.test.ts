/**
 * FB-09 — every sprite canvas draws at an INTEGER scale, smoothing off,
 * `image-rendering: pixelated`, with a CSS box that maps 1:1 onto the backing
 * store, on 1x and 2x displays — and what lands on screen is the approved art,
 * each art pixel a solid block of exactly its approved colour (no blur).
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync, readdirSync, statSync } from "node:fs";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { join } from "node:path";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { fileURLToPath } from "node:url";
import { FRIENDS, H, SPRITE_NAMES, type FriendSprite } from "./art";
import { crispPlan, paintCrisp, type ArtBuffer, type CrispSurface } from "./crisp";
import { Raster } from "./raster";

class FakeSurface implements CrispSurface<Raster> {
  width = 300;
  height = 150;
  style = { width: "", height: "", imageRendering: "" };
  r: Raster | null = null;
  noContext = false;
  getContext(): Raster | null {
    if (this.noContext) return null;
    let r = this.r;
    if (!r || r.width !== this.width || r.height !== this.height) { r = new Raster(this.width, this.height); this.r = r; }
    return r;
  }
}
const rasterBuffer = (w: number, h: number): ArtBuffer<Raster> => { const image = new Raster(w, h); image.imageSmoothingEnabled = false; return { image, ctx: image }; };

const DPRS = [1, 2] as const;
const BOXES = [{ w: 96, h: 96 }, { w: 97.6, h: 61.3 }, { w: 40, h: 40 }, { w: 200, h: 120 }];

describe("crispPlan — whole device px per art px", () => {
  for (const dpr of DPRS) for (const box of BOXES) for (const n of SPRITE_NAMES) {
    const s = FRIENDS[n];
    it(`${n} in ${box.w}x${box.h} @${dpr}x`, () => {
      const p = crispPlan(box.w, box.h, s.w, s.h, dpr);
      expect(Number.isInteger(p.scale) && p.scale >= 1).toBe(true);
      expect(Number.isInteger(p.cssW) && Number.isInteger(p.cssH)).toBe(true);
      expect([p.backingW, p.backingH], "backing store = CSS box x dpr exactly (no compositor resample)").toEqual([p.cssW * dpr, p.cssH * dpr]);
      expect(Number.isInteger(p.dx) && Number.isInteger(p.dy)).toBe(true);
      if (p.backingW >= s.w && p.backingH >= s.h) {
        expect(p.dx).toBeGreaterThanOrEqual(0);
        expect(p.dx + s.w * p.scale).toBeLessThanOrEqual(p.backingW);
        expect(p.dy + s.h * p.scale).toBeLessThanOrEqual(p.backingH);
      }
    });
  }

  it("a 2x display gets twice the device px per art px of a 1x one", () => {
    const s = FRIENDS.hoot;
    expect(crispPlan(120, 120, s.w, s.h, 2).scale).toBe(2 * crispPlan(120, 120, s.w, s.h, 1).scale);
  });

  it("a missing or broken devicePixelRatio reads as 1x", () => {
    for (const bad of [undefined, null, 0, -1, Number.NaN]) expect(crispPlan(80, 80, 40, 40, bad).dpr).toBe(1);
  });
});

function approvedCells(s: FriendSprite, f: number): Raster { const r = new Raster(s.w, s.h); s.draw(r, H(r), f); return r; }

describe("paintCrisp — on screen the sprite is the approved art, block for block", () => {
  for (const dpr of DPRS) for (const n of SPRITE_NAMES) {
    it(`sprite_paints_crisp ${n} @${dpr}x`, () => {
      const s = FRIENDS[n], surf = new FakeSurface();
      const plan = paintCrisp(surf, s, 0, undefined, { w: 97.6, h: 88.2 }, dpr, rasterBuffer, n)!;
      expect(plan).not.toBeNull();
      expect(surf.style.imageRendering).toBe("pixelated");
      expect(surf.style.width).toBe(`${plan.cssW}px`);
      expect(surf.style.height).toBe(`${plan.cssH}px`);
      expect([surf.width, surf.height]).toEqual([plan.cssW * dpr, plan.cssH * dpr]);
      const out = surf.r!;
      expect(out.imageSmoothingEnabled, "smoothing is off at the blit").toBe(false);
      expect(out.smoothedBlits).toBe(0);
      expect(out.subpixel, "no fractional blit or edge").toBe(0);
      const art = approvedCells(s, 0);
      let bad = 0;
      for (let y = 0; y < s.h; y++) for (let x = 0; x < s.w; x++) {
        const want = art.px(x, y);
        for (let j = 0; j < plan.scale; j++) for (let i = 0; i < plan.scale; i++) {
          const got = out.px(plan.dx + x * plan.scale + i, plan.dy + y * plan.scale + j);
          if (got.r !== want.r || got.g !== want.g || got.b !== want.b || got.a !== want.a) bad++;
        }
      }
      expect(bad, `${bad} device px off their art cell's colour`).toBe(0);
    });
  }

  it("no 2D context: returns null and logs one line, never throws", () => {
    const surf = new FakeSurface(); surf.noContext = true;
    const warn = console.warn; const lines: string[] = [];
    console.warn = (m: string) => { lines.push(m); };
    try {
      expect(paintCrisp(surf, FRIENDS.sal, 0, undefined, { w: 50, h: 50 }, 1, rasterBuffer, "sal")).toBeNull();
    } finally { console.warn = warn; }
    expect(lines).toEqual(["sprites paint-failed name=sal reason=no-2d-context"]);
  });

  it("a draw that throws: returns null and logs the error class, never the message", () => {
    const boom: FriendSprite = { w: 4, h: 4, draw() { throw new TypeError("secret detail"); } };
    const warn = console.warn; const lines: string[] = [];
    console.warn = (m: string) => { lines.push(m); };
    try {
      expect(paintCrisp(new FakeSurface(), boom, 0, undefined, { w: 8, h: 8 }, 2, rasterBuffer)).toBeNull();
    } finally { console.warn = warn; }
    expect(lines).toEqual(["sprites paint-failed name=sprite reason=TypeError"]);
  });
});

/* ── every character and room canvas in the app: pixelated, never CSS-scaled ── */

const SRC = fileURLToPath(new URL("..", import.meta.url));
function walk(dir: string): string[] {
  return (readdirSync(dir) as string[]).flatMap((f: string) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return f === "node_modules" || f === "approved" ? [] : walk(p);
    return [p];
  });
}
const FILES = walk(SRC);
// comments stripped, so a comment that NAMES a class never attributes the next rule to it
const CSS = FILES.filter((f) => f.endsWith(".css")).map((f) => readFileSync(f, "utf8")).join("\n").replace(/\/\*[\s\S]*?\*\//g, "");

/** Every declaration block whose selector names `.cls`. */
function rulesFor(cls: string): string[] {
  const out: string[] = [];
  const re = /([^{}]+)\{([^{}]*)\}/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(CSS))) if (new RegExp(`\\.${cls}(?![\\w-])`).test(m[1])) out.push(m[2]);
  return out;
}

describe("canvas_css_is_pixelated_and_unscaled", () => {
  const canvases = FILES.filter((f) => f.endsWith(".tsx") && !/\.test\.tsx$/.test(f)).flatMap((f) => {
    const src = readFileSync(f, "utf8");
    return [...src.matchAll(/<canvas\b[^>]*?className="([\w-]+)"/g)].map((m) => ({ file: f.slice(SRC.length), cls: m[1], src }));
  });

  it("found the app's character and room canvases", () => {
    expect(canvases.map((c) => c.cls)).toEqual(expect.arrayContaining(["kami-canvas", "yh-stage"]));
  });

  for (const c of canvases) {
    it(`${c.cls} (${c.file})`, () => {
      const rules = rulesFor(c.cls);
      const inline = new RegExp(`className="${c.cls}"[\\s\\S]{0,400}?imageRendering: "pixelated"`).test(c.src);
      const decls = rules.join(";");
      // the LAST image-rendering that applies must be pixelated (a later crisp-edges/auto wins in WebKit)
      const ir = [...decls.matchAll(/image-rendering:\s*([\w-]+)/g)].map((m) => m[1]);
      expect(inline || ir[ir.length - 1] === "pixelated", `${c.cls}: image-rendering ${JSON.stringify(ir)}`).toBe(true);
      expect(decls, `${c.cls}: no CSS scale transform / zoom on a pixel canvas`).not.toMatch(/transform:[^;]*scale\(|(^|[;\s])zoom:/);
      expect(decls, `${c.cls}: smoothing hints`).not.toMatch(/image-rendering:\s*(auto|smooth|high-quality)/);
    });
  }

  it("no source turns canvas smoothing back on", () => {
    const on = FILES.filter((f) => /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f)).filter((f) => /imageSmoothingEnabled\s*=\s*true/.test(readFileSync(f, "utf8")));
    expect(on).toEqual([]);
  });
});
