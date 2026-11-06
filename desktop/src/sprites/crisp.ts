/**
 * FB-09 — put a sprite on screen without blurring it, on 1x and 2x displays.
 *
 * The rule: one art pixel is a WHOLE number of DEVICE pixels. The canvas
 * backing store is the CSS box in whole device pixels at the real backing
 * scale (pixelGrid's `backingScale`, never clamped), the CSS box is whole CSS
 * pixels, so the compositor maps the store 1:1 and never resamples; the art
 * is blitted from a 1-unit-per-pixel buffer with smoothing OFF at an integer
 * scale. `image-rendering: pixelated` is set as well, as a safety net only.
 */
import { backingScale, toDevice } from "../pill/pixelGrid";
import { FRIENDS, H, type FriendSprite, type PixelCtx, type SpriteName, type SpriteOpt } from "./art";

export interface CrispPlan {
  /** The canvas's CSS box, whole CSS px. */
  cssW: number; cssH: number;
  /** The backing scale used (device px per CSS px). */
  dpr: number;
  /** The canvas backing store, whole device px. */
  backingW: number; backingH: number;
  /** Device px per art px — always a whole number >= 1. */
  scale: number;
  /** Top-left of the art in device px (whole numbers, centred). */
  dx: number; dy: number;
}

/** The plan for an `artW x artH` sprite in a `boxW x boxH` CSS box. */
export function crispPlan(boxW: number, boxH: number, artW: number, artH: number, dprRaw: number | undefined | null): CrispPlan {
  const dpr = backingScale(dprRaw);
  const cssW = Math.max(1, Math.floor(boxW)), cssH = Math.max(1, Math.floor(boxH));
  const backingW = toDevice(cssW, dpr), backingH = toDevice(cssH, dpr);
  const scale = Math.max(1, Math.floor(Math.min(backingW / artW, backingH / artH)));
  return {
    cssW, cssH, dpr, backingW, backingH, scale,
    dx: Math.floor((backingW - artW * scale) / 2), dy: Math.floor((backingH - artH * scale) / 2),
  };
}

/** The blit side of a 2D context (a real one, or the test raster). */
export interface BlitCtx<I> {
  imageSmoothingEnabled: boolean;
  setTransform(a: number, b: number, c: number, d: number, e: number, f: number): void;
  clearRect(x: number, y: number, w: number, h: number): void;
  drawImage(image: I, sx: number, sy: number, sw: number, sh: number, dx: number, dy: number, dw: number, dh: number): void;
}

/** What `paintCrisp` needs of a canvas element. */
export interface CrispSurface<I> {
  width: number; height: number;
  style: { width: string; height: string; imageRendering: string };
  getContext(kind: "2d"): BlitCtx<I> | null;
}

/** A 1-unit-per-art-pixel buffer the sprite is drawn into first. */
export interface ArtBuffer<I> { image: I; ctx: PixelCtx }

/**
 * Size `surface` per the plan and draw `sprite` frame `f` into it, crisp.
 * Returns the plan, or null (one log line, no throw) when the canvas has no
 * 2D context — a sprite that cannot draw never takes its screen down.
 */
export function paintCrisp<I>(
  surface: CrispSurface<I>, sprite: FriendSprite, f: number, opt: SpriteOpt | undefined,
  box: { w: number; h: number }, dprRaw: number | undefined | null,
  makeBuffer: (w: number, h: number) => ArtBuffer<I>, label = "sprite",
): CrispPlan | null {
  const plan = crispPlan(box.w, box.h, sprite.w, sprite.h, dprRaw);
  try {
    if (surface.width !== plan.backingW) surface.width = plan.backingW;
    if (surface.height !== plan.backingH) surface.height = plan.backingH;
    surface.style.width = `${plan.cssW}px`;
    surface.style.height = `${plan.cssH}px`;
    surface.style.imageRendering = "pixelated";
    const ctx = surface.getContext("2d");
    if (!ctx) { console.warn(`sprites paint-failed name=${label} reason=no-2d-context`); return null; }
    const buf = makeBuffer(sprite.w, sprite.h);
    sprite.draw(buf.ctx, H(buf.ctx), f, opt);
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, plan.backingW, plan.backingH);
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(buf.image, 0, 0, sprite.w, sprite.h, plan.dx, plan.dy, sprite.w * plan.scale, sprite.h * plan.scale);
    return plan;
  } catch (e) {
    console.warn(`sprites paint-failed name=${label} reason=${e instanceof Error ? e.name : "unknown"}`);
    return null;
  }
}

/** A DOM buffer for `paintCrisp` (the app path). */
export function domBuffer(w: number, h: number): ArtBuffer<HTMLCanvasElement> {
  const image = document.createElement("canvas");
  image.width = w; image.height = h;
  const ctx = image.getContext("2d");
  if (!ctx) throw new Error("no-2d-context");
  ctx.imageSmoothingEnabled = false;
  return { image, ctx };
}

/** Paint a named approved sprite into an on-screen canvas at its CSS box. */
export function paintSprite(canvas: HTMLCanvasElement, name: SpriteName, f = 0, opt?: SpriteOpt): CrispPlan | null {
  const r = canvas.getBoundingClientRect();
  return paintCrisp<HTMLCanvasElement>(canvas, FRIENDS[name], f, opt, { w: r.width, h: r.height }, window.devicePixelRatio, domBuffer, name);
}
