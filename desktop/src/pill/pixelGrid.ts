/**
 * yap24-UI1 — the integer pixel grid. Every sprite pixel lands on WHOLE device
 * pixels, at every dock size and every backing scale.
 *
 * The bug this file exists to keep dead: the pixel-art canvases were sized from
 * the CSS box and scaled by whatever ratio fell out (YappyPill clamped the DPR
 * and let the camera zoom to any real number; YappyHouse painted a fixed 960x640
 * backing store that CSS then shrank to fit). Whenever one art pixel came out as
 * 2.69 or 5.38 device pixels, a 1-art-pixel line straddled two device pixels and
 * read soft. `image-rendering: pixelated` only picks nearest-neighbour; it cannot
 * make a non-integer scale integer.
 *
 * The rule, in one sentence: pick the LARGEST WHOLE number of device pixels per
 * art pixel that still fits the art inside the box, centre it on a whole device
 * pixel, and fill whatever is left over with scene — never stretch.
 *
 * Pure: no DOM, no canvas. The components measure, call in, and draw what comes
 * back. Terms used below:
 *   - CSS px     the layout unit the DOM reports (getBoundingClientRect).
 *   - device px  a physical pixel on the panel: CSS px x devicePixelRatio.
 *   - art px     one pixel of the low-res offscreen scene buffer.
 *   - cell       how many device px one art px covers, per axis. Always an integer.
 */

/** A size in some unit (CSS px unless the name says otherwise). */
export interface Size {
  w: number;
  h: number;
}

/** A rectangle in CSS px (x, y may be fractional — layout is). */
export interface CssRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A rectangle in whole units (device px or art px — the field says which). */
export interface IntRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface PixelGrid {
  /** The backing scale actually used (sanitised devicePixelRatio, never clamped). */
  dpr: number;
  /** canvas.width / canvas.height — whole device px covering the CSS box. */
  backingW: number;
  backingH: number;
  /** Device px per art px. An integer >= 1. */
  cell: number;
  /** The art grid drawn at `cell`: gridW * cell, gridH * cell (device px). */
  drawW: number;
  drawH: number;
  /** Where the art grid's top-left lands, in whole device px, centred in the backing store.
   *  Negative only when even cell = 1 is larger than the box (it is then centred and cropped). */
  offsetX: number;
  offsetY: number;
}

/**
 * The backing scale for a reported devicePixelRatio. NOT clamped: a backing store
 * smaller than the real device grid is upscaled by the compositor by a
 * non-integer ratio, which is exactly the soft edge this module removes.
 * Missing, zero, negative or non-finite (a detached window, a test) reads as 1.
 */
export function backingScale(dpr: number | undefined | null): number {
  return typeof dpr === "number" && Number.isFinite(dpr) && dpr > 0 ? dpr : 1;
}

/** CSS length -> whole device px. At least 1, so a canvas is never 0x0. */
export function toDevice(css: number, dpr: number): number {
  return Math.max(1, Math.round(css * backingScale(dpr)));
}

/**
 * The SMALLEST integer cell at which the grid covers the device box (>= 1) — the
 * integer form of `object-fit: cover`. Used where a margin must never show: the
 * pill's world behind the capsule, and the Home room under a cover rule.
 */
export function coverCell(devW: number, devH: number, gridW: number, gridH: number): number {
  const gw = Math.max(1, Math.round(gridW));
  const gh = Math.max(1, Math.round(gridH));
  return Math.max(1, Math.ceil(devW / gw), Math.ceil(devH / gh));
}

/** How the art is fitted to the box — the CSS `object-fit` the element carries. */
export type ArtFit = "contain" | "cover";
export interface FitOptions {
  /** contain (default): whole art, largest integer cell, margin left for the caller to paint.
   *  cover: art fills the box, smallest integer cell that covers it, the overflow cropped. */
  fit?: ArtFit;
  /** Where the art sits in the box, as fractions 0..1 per axis (CSS `object-position`). Default centre. */
  pos?: { x: number; y: number };
}

/**
 * A computed `object-position` ("50% 70%", "left top", "center") -> fractions 0..1.
 * Anything it cannot read as a percentage or keyword (a length) reads as centre.
 */
export function parseObjectPosition(css: string | undefined | null): { x: number; y: number } {
  const parts = String(css ?? "").trim().split(/\s+/).filter(Boolean);
  const one = (tok: string | undefined): number | null => {
    if (tok === undefined) return null;
    if (tok === "left" || tok === "top") return 0;
    if (tok === "center") return 0.5;
    if (tok === "right" || tok === "bottom") return 1;
    const m = /^(-?\d+(?:\.\d+)?)%$/.exec(tok);
    return m ? Math.min(1, Math.max(0, Number(m[1]) / 100)) : null;
  };
  // a lone vertical keyword ("top") names y, not x
  if (parts.length === 1 && (parts[0] === "top" || parts[0] === "bottom")) return { x: 0.5, y: one(parts[0])! };
  return { x: one(parts[0]) ?? 0.5, y: one(parts[1]) ?? 0.5 };
}

/** The largest integer cell such that the grid still fits the device box (>= 1). */
export function fitCell(devW: number, devH: number, gridW: number, gridH: number): number {
  const gw = Math.max(1, Math.round(gridW));
  const gh = Math.max(1, Math.round(gridH));
  return Math.max(1, Math.min(Math.floor(devW / gw), Math.floor(devH / gh)));
}

function fitInDevice(devW: number, devH: number, gridW: number, gridH: number, dpr: number, opts: FitOptions = {}): PixelGrid {
  const gw = Math.max(1, Math.round(gridW));
  const gh = Math.max(1, Math.round(gridH));
  const cell = opts.fit === "cover" ? coverCell(devW, devH, gw, gh) : fitCell(devW, devH, gw, gh);
  const px = opts.pos?.x ?? 0.5, py = opts.pos?.y ?? 0.5;
  const drawW = gw * cell;
  const drawH = gh * cell;
  return {
    dpr,
    backingW: devW,
    backingH: devH,
    cell,
    drawW,
    drawH,
    offsetX: Math.floor((devW - drawW) * px),
    offsetY: Math.floor((devH - drawH) * py),
  };
}

/**
 * THE function. Given a CSS box, an art grid (w x h art px) and the DPR, return
 * the largest integer device-pixel cell, the canvas backing size and the centring
 * offset in whole device pixels.
 */
export function pixelGrid(box: Size, grid: Size, dprRaw: number | undefined | null, opts: FitOptions = {}): PixelGrid {
  const dpr = backingScale(dprRaw);
  return fitInDevice(toDevice(box.w, dpr), toDevice(box.h, dpr), grid.w, grid.h, dpr, opts);
}

/** Snap a CSS rect to the device grid: whole device px, edges rounded independently
 *  so two rects that share a CSS edge still share a device edge. */
export function snapRect(r: CssRect, dprRaw: number): IntRect {
  const dpr = backingScale(dprRaw);
  const x = Math.round(r.x * dpr);
  const y = Math.round(r.y * dpr);
  return { x, y, w: Math.max(1, Math.round((r.x + r.w) * dpr) - x), h: Math.max(1, Math.round((r.y + r.h) * dpr) - y) };
}

/** Whole art px needed on each side of the grid to cover the leftover margin. */
export function fillSpan(g: PixelGrid): { left: number; top: number; right: number; bottom: number } {
  const span = (margin: number) => Math.max(0, Math.ceil(margin / g.cell));
  return {
    left: span(g.offsetX),
    top: span(g.offsetY),
    right: span(g.backingW - g.offsetX - g.drawW),
    bottom: span(g.backingH - g.offsetY - g.drawH),
  };
}

export interface SceneBlit {
  /** Device px per art px — integer. */
  cell: number;
  /** The camera view the caller asked for, rounded to whole art px (what the cell is fitted to,
   *  unless the buffer is too small to cover the window at that cell — then the view is cropped). */
  gridW: number;
  gridH: number;
  /** The clip window (the capsule interior) on the device grid. */
  clip: IntRect;
  /** Source rect in the scene buffer, whole art px, ALWAYS inside [0, buffer] — the view plus
   *  the scene that fills the margin. */
  src: IntRect;
  /** Destination rect, whole device px. Always exactly src * cell: never stretched. */
  dst: IntRect;
}

/**
 * A camera into a scene buffer that is LARGER than what it shows (YappyPill's
 * world), drawn into a clip window (the capsule interior) on the device grid.
 *
 * The cell is the largest integer that fits the requested view in the window —
 * unless the buffer at that cell is too narrow or too short to cover the window,
 * in which case it is the smallest integer that DOES cover it (the camera zooms
 * in and crops the view; the window is never letterboxed). The camera centre
 * lands on the window centre, panned back inside the buffer when it would show
 * past an edge. `src` therefore only ever reads real scene px — a drawImage source
 * rect past the buffer does not throw, it just leaves those device px unpainted —
 * and `dst` = src * cell covers the whole clip window.
 *
 * The caller clips to `clip` (or a shape inside it) and draws
 * `drawImage(buf, src.x, src.y, src.w, src.h, dst.x, dst.y, dst.w, dst.h)`
 * under an identity transform.
 */
export function sceneBlit(o: {
  /** The window the scene shows through, in CSS px. */
  rect: CssRect;
  /** The camera view, in art px. */
  view: Size;
  /** The art px the camera is centred on. */
  centre: { x: number; y: number };
  /** The scene buffer's size, in art px — the only pixels `src` may read. */
  buffer: Size;
  dpr: number;
}): SceneBlit {
  const dpr = backingScale(o.dpr);
  const clip = snapRect(o.rect, dpr);
  const gridW = Math.max(1, Math.round(o.view.w));
  const gridH = Math.max(1, Math.round(o.view.h));
  const bufW = Math.max(1, Math.round(o.buffer.w));
  const bufH = Math.max(1, Math.round(o.buffer.h));
  const cell = Math.max(fitCell(clip.w, clip.h, gridW, gridH), coverCell(clip.w, clip.h, bufW, bufH));
  // One axis: the art coordinate at the window's leading edge (centred on the
  // camera, clamped into the buffer — `cell` guarantees bufLen*cell >= clipLen),
  // split into a whole art px `s` and a whole-device-px lead-in before the window.
  const axis = (clipPos: number, clipLen: number, centre: number, bufLen: number) => {
    const span = clipLen / cell;
    const a0 = Math.min(Math.max(centre - span / 2, 0), Math.max(0, bufLen - span));
    const s = Math.max(0, Math.floor(a0));
    const lead = Math.min(cell - 1, Math.max(0, Math.floor((a0 - s) * cell)));
    const len = Math.min(bufLen - s, Math.ceil((clipLen + lead) / cell));
    return { s, len, d: clipPos - lead };
  };
  const ax = axis(clip.x, clip.w, o.centre.x, bufW);
  const ay = axis(clip.y, clip.h, o.centre.y, bufH);
  const src = { x: ax.s, y: ay.s, w: ax.len, h: ay.len };
  const dst = { x: ax.d, y: ay.d, w: src.w * cell, h: src.h * cell };
  return { cell, gridW, gridH, clip, src, dst };
}

/** The three docks the float window can sit on (`<html data-dock>`). */
export type Dock = "bottom" | "left" | "right";
export const DOCKS: readonly Dock[] = ["bottom", "left", "right"];

/**
 * YV53 — on a side dock the window is parked flush to the screen edge, so the
 * capsule is drawn this many CSS px in from it: enough for its glow/shadow, not
 * so much that it stops reading as docked. Mirrors the .stage inset in float.css.
 */
export const DOCK_PAD = 10;

/** The capsule's centre x, in CSS px, for a dock. Unknown docks read as bottom. */
export function dockCentreX(dock: string | undefined, canvasW: number, capW: number): number {
  return dock === "left" ? DOCK_PAD + capW / 2
    : dock === "right" ? canvasW - DOCK_PAD - capW / 2
      : canvasW / 2;
}
