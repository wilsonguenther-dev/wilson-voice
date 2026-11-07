/**
 * yap-1106-pill-props-detail — the notepad, desk and typewriter of the pill
 * scene, with the detail Wilson approved on the pill design page (2026-11-06,
 * section 11 "The props, with more detail"): same pixel size, same place in the
 * scene. The notepad gets a clip, a margin line and handwriting that grows; the
 * desk gets wood grain and an edge highlight; the typewriter gets a carriage,
 * return levers, round keys that light as he types and a page that bounces; a
 * mug and pencils sit on the left.
 *
 * The draw bodies are a line-for-line port of the `A.hd` branches of the
 * approved yappy.js drawScene (./approved/yappy.js). props.snapshot.test.ts
 * renders both through the same raster and requires them to be identical, so
 * do not "tidy" them: new art goes through a new approved artifact first.
 *
 * The only addition is `ink` on the notepad: the approved frame is the FULL
 * page of handwriting (ink = 1); a smaller ink writes the same strokes in
 * reading order, so the handwriting grows while he notes.
 */
import type { PixelCtx } from "./art";

/** Ink pixels on a full notepad page: four lines of 5, 6, 5 and 3, and the full stop. */
export const PAD_INK_LINES: readonly number[] = [5, 6, 5, 3];
export const PAD_INK_PX = PAD_INK_LINES.reduce((a, b) => a + b, 0) + 1;
/** Seconds of noting until the page is full (the handwriting then stays). */
export const PAD_INK_FILL_SEC = 6;
/** The typewriter has six round keys; one lights at a time, left to right. */
export const TYPEWRITER_KEYS = 6;

/** Whole ink pixels written for an ink amount in [0, 1]. */
export function padInkPixels(ink: number): number {
  const k = Number.isFinite(ink) ? Math.max(0, Math.min(1, ink)) : 1;
  return Math.round(k * PAD_INK_PX);
}

/** Ink after `dt` more seconds of noting (never past a full page). */
export function growPadInk(ink: number, dt: number): number {
  const k = Number.isFinite(ink) ? ink : 0;
  return Math.max(0, Math.min(1, k + Math.max(0, dt) / PAD_INK_FILL_SEC));
}

/** Which key is lit and whether the page is lifted, `elapsed` seconds in. */
export function typewriterBeat(elapsed: number): { key: number; lift: 0 | 1 } {
  const e = Number.isFinite(elapsed) ? Math.max(0, elapsed) : 0;
  return { key: Math.floor(e * 9) % TYPEWRITER_KEYS, lift: Math.floor(e * 6) % 3 === 0 ? 1 : 0 };
}

function rcter(o: PixelCtx) {
  return (x: number, y: number, w: number, h: number, col: string) => { o.fillStyle = col; o.fillRect(Math.round(x), Math.round(y), w, h); };
}

/**
 * The clipboard notepad that slides in from the right while he notes.
 * `cx` is the chick's centre x, `groundY` the top row of the grass.
 */
export function drawNotepadHD(o: PixelCtx, cx: number, groundY: number, propIn: number, paper: string, ink = 1): void {
  if (!(propIn > .05)) return;
  const rct = rcter(o);
  const qx = cx + 12 + (1 - propIn) * 18, qy = groundY - 14;
  o.globalAlpha = propIn;
  rct(qx + 1, qy + 15, 12, 1, "rgba(58,36,24,.25)");
  rct(qx, qy + 1, 12, 14, "#8a7a5a"); rct(qx + 1, qy + 2, 10, 12, paper); rct(qx + 1, qy + 13, 10, 1, "#eee6cf"); rct(qx + 10, qy + 2, 1, 12, "#eee6cf");
  rct(qx + 2, qy, 8, 3, "#9aa3ad"); rct(qx + 3, qy, 6, 1, "#c8d0d8"); rct(qx + 5, qy - 1, 2, 1, "#9aa3ad");   // the clip
  rct(qx + 2, qy + 4, 1, 9, "#f2b8b0");                                                                         // the margin line
  let left = padInkPixels(ink);
  for (let j = 0; j < PAD_INK_LINES.length; j++) {
    rct(qx + 3, qy + 5 + j * 2, 7, 1, "#c9d6e6");                                                               // the ruled line
    const n = Math.min(left, PAD_INK_LINES[j]); left -= n;
    if (n > 0) rct(qx + 3, qy + 5 + j * 2, n, 1, "#4a4f5c");                                                     // handwriting
  }
  if (left > 0) rct(qx + 7, qy + 11, 1, 1, "#4a4f5c");                                                           // the full stop
  o.globalAlpha = 1;
}

/**
 * The receptionist desk rising from the bottom, the typewriter on it and the
 * mug + pencils on the left. Drawn AFTER the chick (the desk is in front of him).
 * `sceneW` is the scene buffer width; `elapsed` drives the keys and the page.
 */
export function drawDeskHD(o: PixelCtx, cx: number, groundY: number, sceneW: number, propIn: number, elapsed: number, paper: string): void {
  if (!(propIn > .05)) return;
  const rct = rcter(o), PW = sceneW;
  const ey = groundY - 2 + (1 - propIn) * 16;
  o.globalAlpha = Math.min(1, propIn + .2);
  rct(4, ey, PW - 8, 9, "#5a3420"); rct(5, ey, PW - 10, 8, "#b8763f"); rct(5, ey, PW - 10, 1, "#d8955a"); rct(5, ey + 2, PW - 10, 1, "#9a5c30");   // top + edge highlight
  for (let g = 12; g < PW - 10; g += 23) { rct(g, ey + 4, 9, 1, "#a8683a"); rct(g + 5, ey + 6, 7, 1, "#a8683a"); }                            // wood grain
  rct(5, ey + 7, PW - 10, 1, "#7a4626");
  const ux = cx + 13, uy = ey - 9;
  rct(ux - 1, uy + 9, 22, 1, "rgba(40,20,10,.35)");
  rct(ux, uy + 3, 20, 6, "#2a2d35"); rct(ux + 1, uy + 3, 18, 5, "#4a5060"); rct(ux + 1, uy + 3, 18, 1, "#6a7284");                              // body
  rct(ux + 2, uy, 16, 4, "#2a2d35"); rct(ux + 3, uy + 1, 14, 2, "#4a5060");                                                                    // carriage
  rct(ux - 1, uy + 1, 3, 2, "#2a2d35"); rct(ux + 18, uy + 1, 3, 2, "#2a2d35"); rct(ux - 1, uy + 1, 1, 1, "#c8d0d8"); rct(ux + 20, uy + 1, 1, 1, "#c8d0d8");   // return levers
  const beat = typewriterBeat(elapsed), lift = beat.lift;
  rct(ux + 4, uy - 6 - lift, 12, 7, "#c9c3ad"); rct(ux + 5, uy - 6 - lift, 10, 6, paper); rct(ux + 6, uy - 4 - lift, 7, 1, "#4a4f5c"); rct(ux + 6, uy - 2 - lift, 5, 1, "#4a4f5c");   // the bouncing page
  for (let q = 0; q < TYPEWRITER_KEYS; q++) { rct(ux + 2 + q * 3, uy + 5, 2, 2, "#1e2025"); rct(ux + 2 + q * 3, uy + 5, 2, 1, q === beat.key ? "#ffd23f" : "#e8e6e1"); }   // keys, lit in sequence
  rct(ux + 3, uy + 8, 14, 1, "#1e2025");
  const mx = cx - 30, my = ey - 6;
  rct(mx, my, 6, 6, "#5a3420"); rct(mx + 1, my, 4, 5, "#e8e6e1"); rct(mx + 1, my + 1, 4, 1, "#7a4626"); rct(mx + 6, my + 1, 2, 3, "#5a3420"); rct(mx + 2, my - 2, 1, 1, "rgba(255,255,255,.7)"); rct(mx + 3, my - 3, 1, 1, "rgba(255,255,255,.5)");   // the mug + steam
  rct(mx + 9, my + 1, 1, 5, "#ffcf3a"); rct(mx + 9, my, 1, 1, "#ff8fb0"); rct(mx + 11, my + 2, 1, 4, "#5a9fd6");                              // pencils
  o.globalAlpha = 1;
}
