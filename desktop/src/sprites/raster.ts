/**
 * FB-09 — a tiny software 2D context: exact, deterministic pixels with no
 * browser, so the sprite snapshot test can run in CI (`npm test`, node).
 *
 * It implements only what the pixel art and the crisp blit use: fillStyle
 * (hex colours), globalAlpha, fillRect, clearRect, save/restore, translate,
 * scale, setTransform and a NEAREST-NEIGHBOUR drawImage. Anything a real
 * canvas would anti-alias — a rectangle edge or a blit that does not land on
 * whole device pixels — is counted in `subpixel`, because on screen that is
 * exactly what reads as "blurred". The snapshot test requires it to be zero.
 */

export interface Rgba { r: number; g: number; b: number; a: number }

/** "#rgb" or "#rrggbb" only: the approved art uses nothing else. */
export function parseHex(col: string): Rgba {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(col.trim());
  if (!m) throw new Error(`raster: unsupported colour ${JSON.stringify(col)}`);
  const h = m[1].length === 3 ? m[1].split("").map((c) => c + c).join("") : m[1];
  return { r: parseInt(h.slice(0, 2), 16), g: parseInt(h.slice(2, 4), 16), b: parseInt(h.slice(4, 6), 16), a: 255 };
}

interface Xf { a: number; d: number; e: number; f: number }

const isWhole = (n: number) => Math.abs(n - Math.round(n)) < 1e-9;

export class Raster {
  readonly data: Uint8ClampedArray;
  fillStyle: string | CanvasGradient | CanvasPattern = "#000000";
  globalAlpha = 1;
  imageSmoothingEnabled = true;
  /** Draws that a real canvas would anti-alias (edges off the device grid). */
  subpixel = 0;
  /** drawImage calls made while imageSmoothingEnabled was true. */
  smoothedBlits = 0;
  private xf: Xf = { a: 1, d: 1, e: 0, f: 0 };
  private stack: Xf[] = [];

  constructor(public width: number, public height: number) {
    this.data = new Uint8ClampedArray(width * height * 4);
  }

  save(): void { this.stack.push({ ...this.xf }); }
  restore(): void { const x = this.stack.pop(); if (x) this.xf = x; }
  translate(x: number, y: number): void { this.xf.e += x * this.xf.a; this.xf.f += y * this.xf.d; }
  scale(x: number, y: number): void { this.xf.a *= x; this.xf.d *= y; }
  setTransform(a: number, _b: number, _c: number, d: number, e: number, f: number): void { this.xf = { a, d, e, f }; }

  /** Device-space rectangle for a user-space one, counting off-grid edges. */
  private toDevice(x: number, y: number, w: number, h: number): [number, number, number, number] {
    const x0 = this.xf.e + x * this.xf.a, y0 = this.xf.f + y * this.xf.d;
    const x1 = x0 + w * this.xf.a, y1 = y0 + h * this.xf.d;
    if (![x0, y0, x1, y1].every(isWhole)) this.subpixel++;
    return [Math.round(Math.min(x0, x1)), Math.round(Math.min(y0, y1)), Math.round(Math.max(x0, x1)), Math.round(Math.max(y0, y1))];
  }

  private blend(i: number, c: Rgba, alpha: number): void {
    const sa = (c.a / 255) * alpha;
    if (sa <= 0) return;
    const d = this.data, da = d[i + 3] / 255;
    const oa = sa + da * (1 - sa);
    d[i] = Math.round((c.r * sa + d[i] * da * (1 - sa)) / oa);
    d[i + 1] = Math.round((c.g * sa + d[i + 1] * da * (1 - sa)) / oa);
    d[i + 2] = Math.round((c.b * sa + d[i + 2] * da * (1 - sa)) / oa);
    d[i + 3] = Math.round(oa * 255);
  }

  fillRect(x: number, y: number, w: number, h: number): void {
    if (typeof this.fillStyle !== "string") throw new Error("raster: only hex fillStyle");
    const c = parseHex(this.fillStyle);
    const [x0, y0, x1, y1] = this.toDevice(x, y, w, h);
    for (let py = Math.max(0, y0); py < Math.min(this.height, y1); py++)
      for (let px = Math.max(0, x0); px < Math.min(this.width, x1); px++) this.blend((py * this.width + px) * 4, c, this.globalAlpha);
  }

  clearRect(x: number, y: number, w: number, h: number): void {
    const [x0, y0, x1, y1] = this.toDevice(x, y, w, h);
    for (let py = Math.max(0, y0); py < Math.min(this.height, y1); py++)
      this.data.fill(0, (py * this.width + Math.max(0, x0)) * 4, (py * this.width + Math.min(this.width, x1)) * 4);
  }

  /** Nearest-neighbour blit of a source rectangle. A destination that is not
   * a WHOLE multiple of the source (or lands off the device grid) counts as
   * subpixel: a real canvas resamples it, which is the blur FB-09 forbids. */
  drawImage(src: Raster, sx: number, sy: number, sw: number, sh: number, dx: number, dy: number, dw: number, dh: number): void {
    if (this.imageSmoothingEnabled) this.smoothedBlits++;
    const [x0, y0, x1, y1] = this.toDevice(dx, dy, dw, dh);
    const kx = (x1 - x0) / sw, ky = (y1 - y0) / sh;
    if (!isWhole(kx) || !isWhole(ky) || ![sx, sy, sw, sh].every(isWhole)) this.subpixel++;
    for (let py = Math.max(0, y0); py < Math.min(this.height, y1); py++) {
      const iy = Math.floor(sy + (py - y0) / ky);
      for (let px = Math.max(0, x0); px < Math.min(this.width, x1); px++) {
        const ix = Math.floor(sx + (px - x0) / kx);
        if (ix < 0 || iy < 0 || ix >= src.width || iy >= src.height) continue;
        const j = (iy * src.width + ix) * 4, s = src.data;
        this.blend((py * this.width + px) * 4, { r: s[j], g: s[j + 1], b: s[j + 2], a: s[j + 3] }, this.globalAlpha);
      }
    }
  }

  /** RGBA at (x, y), or transparent black off the edge. */
  px(x: number, y: number): Rgba {
    if (x < 0 || y < 0 || x >= this.width || y >= this.height) return { r: 0, g: 0, b: 0, a: 0 };
    const i = (y * this.width + x) * 4, d = this.data;
    return { r: d[i], g: d[i + 1], b: d[i + 2], a: d[i + 3] };
  }
}

export interface SnapDiff {
  /** The offset (each of -1, 0, 1) at which the two images matched best. */
  dx: number; dy: number;
  /** Pixels that differ at that offset (any channel, zero tolerance). */
  mismatched: number;
  /** Opaque-ish pixels in the approved image (the denominator a reader wants). */
  painted: number;
}

/**
 * Compare `got` to `want` with ZERO colour tolerance and up to `slack` pixels
 * of POSITION tolerance: `got` may be shifted by (dx, dy) in [-slack, slack]
 * as a whole, and must then equal `want` exactly. A blurred sprite has colours
 * the approved one never had; a sprite drifted by 2 px has no matching shift.
 */
export function snapDiff(got: Raster, want: Raster, slack = 1): SnapDiff {
  let best: SnapDiff | null = null;
  let painted = 0;
  for (let y = 0; y < want.height; y++) for (let x = 0; x < want.width; x++) if (want.px(x, y).a > 0) painted++;
  for (let dy = -slack; dy <= slack; dy++) for (let dx = -slack; dx <= slack; dx++) {
    let mismatched = 0;
    const W = Math.max(want.width, got.width), Hh = Math.max(want.height, got.height);
    for (let y = -slack; y < Hh + slack; y++) for (let x = -slack; x < W + slack; x++) {
      const a = want.px(x, y), b = got.px(x + dx, y + dy);
      if (a.r !== b.r || a.g !== b.g || a.b !== b.b || a.a !== b.a) {
        // fully transparent pixels compare equal whatever their RGB
        if (!(a.a === 0 && b.a === 0)) mismatched++;
      }
    }
    if (!best || mismatched < best.mismatched || (mismatched === best.mismatched && Math.abs(dx) + Math.abs(dy) < Math.abs(best.dx) + Math.abs(best.dy)))
      best = { dx, dy, mismatched, painted };
  }
  return best!;
}
