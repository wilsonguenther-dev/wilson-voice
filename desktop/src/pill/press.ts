/**
 * yap-wf-01-click-the-pill-to — WF-01: the gesture classifier for a press on
 * the pill. Pure (no `@tauri-apps/*`), so the click-vs-drag rule is a unit test.
 *
 * A press is a CLICK only when the pointer never travelled 4 logical points
 * from where it went down AND it came back up inside 300 ms. Anything else —
 * it moved, or it was held — is a DRAG, and a drag never toggles a take.
 * Travel is the FURTHEST the pointer got during the press, not where it was
 * released: a wobble out and back is still a drag.
 */

/** Travel (logical points) at or past which a press is a DRAG, not a click. */
export const CLICK_SLOP = 4;
/** Hold (ms) at or past which a press is a DRAG, not a click. */
export const CLICK_MAX_MS = 300;

export type PressKind = "click" | "drag";

/** One press in flight, in screen points and a monotonic ms clock. */
export interface PressTrack {
  x: number;
  y: number;
  t: number;
  /** The furthest travel from the down point seen so far. */
  far: number;
}

export function beginPress(x: number, y: number, t: number): PressTrack {
  return { x, y, t, far: 0 };
}

/** Record a move; returns the travel from the down point at this move. */
export function trackPress(p: PressTrack, x: number, y: number): number {
  const d = Math.hypot(x - p.x, y - p.y);
  if (d > p.far) p.far = d;
  return d;
}

/** The rule itself: under 4 points of travel and under 300 ms is a click. */
export function classifyPress(travel: number, ms: number): PressKind {
  return travel < CLICK_SLOP && ms >= 0 && ms < CLICK_MAX_MS ? "click" : "drag";
}

/** Close a press at its pointer-up point and time. */
export function endPress(p: PressTrack, x: number, y: number, t: number): PressKind {
  trackPress(p, x, y);
  return classifyPress(p.far, t - p.t);
}
