/**
 * Y5-E — hover hysteresis and the alpha hot area (Wispr parity P0 #3, §4.4
 * "Hit-testing and hover — the part that breaks naive implementations").
 *
 * The bug this exists to prevent: an edge-docked pill whose expanded and
 * collapsed capsules SHARE the boundary facing the dock edge (the left/right
 * docks grow away from the screen edge, so the edge-side boundary never moves).
 * A cursor dwelling on that boundary is inside the capsule one frame and one
 * pixel outside it the next; a naive "hovered = cursor in capsule" pill expands
 * and collapses on every pixel of jitter.
 *
 * The fix is GEOMETRY, not delay:
 *   - ENTER is tested against the capsule as painted (collapsed: no margin, so
 *     an idle pill has no dead zone around it), and expands immediately.
 *   - STAY is tested against the EXPANDED hot rect: the capsule plus a
 *     `HOVER.marginPx` alpha margin, which float.css paints ONLY in this state
 *     and which is the exact rect the panel's hitbox is fed. Jitter across the
 *     collapsed boundary is 12 px deep inside it.
 *   - COLLAPSE happens only after the cursor has been outside that hot rect for
 *     `HOVER.collapseAfterMs` — so jitter across the hot rect's OWN boundary
 *     cannot flap it either.
 *
 * Everything here is pure: the reducer takes the capsule rect the pill is
 * painting right now and a timestamped cursor sample, and returns the next
 * state. ClassicPill feeds it real pointer samples and the backend's
 * "the cursor left the hitbox" event; hitbox.test.ts feeds it synthetic paths.
 */
import { HOVER } from "./motion";

/** A rect in window-logical points (the space `pill_set_hitbox` takes). */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type HoverPhase = "collapsed" | "expanded";

/**
 * Emitted by the backend (`float_pill::PILL_HOVER_OUT_EVENT`) the moment it
 * hands the panel back to click-through because the cursor left the published
 * hitbox. From then on the webview gets no pointer events, so this is the only
 * "the cursor is gone" signal the hover reducer can rely on.
 */
export const PILL_HOVER_OUT_EVENT = "pill-hover-out";

export interface HoverState {
  phase: HoverPhase;
  /** When the cursor was first seen outside the hot rect; null while inside. */
  outsideSince: number | null;
  /** Expand/collapse flips so far — what the dwell test counts. */
  transitions: number;
}

/**
 * One input to the reducer. A point is a cursor sample in window coordinates;
 * `gone` means the cursor is no longer over the panel at all (the backend
 * turned click-through back on, or the pointer left the window); a bare `{at}`
 * is the passage of time — the collapse debounce's timer firing.
 */
export type HoverInput =
  | { at: number; x: number; y: number }
  | { at: number; gone: true }
  | { at: number };

export interface HoverConfig {
  marginPx: number;
  collapseAfterMs: number;
}

export const HOVER_CONFIG: HoverConfig = {
  marginPx: HOVER.marginPx,
  collapseAfterMs: HOVER.collapseAfterMs,
};

export const initialHover = (): HoverState => ({ phase: "collapsed", outsideSince: null, transitions: 0 });

export function inflate(r: Rect, by: number): Rect {
  return { x: r.x - by, y: r.y - by, w: r.w + 2 * by, h: r.h + 2 * by };
}

/** Intersection of two rects; an empty overlap is a zero-size rect. */
export function clip(r: Rect, to: Rect): Rect {
  const x = Math.max(r.x, to.x);
  const y = Math.max(r.y, to.y);
  const right = Math.min(r.x + r.w, to.x + to.w);
  const bottom = Math.min(r.y + r.h, to.y + to.h);
  return { x, y, w: Math.max(0, right - x), h: Math.max(0, bottom - y) };
}

/** Half-open containment, so two rects that share an edge never both claim it. */
export function contains(r: Rect, x: number, y: number): boolean {
  return x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;
}

/**
 * THE hot rect: what float.css paints (`::before`, alpha 0.004) and what the
 * panel's hitbox is fed — one function, so the two cannot drift. Collapsed it
 * is the capsule alone (no dead zone); expanded it is the capsule plus the
 * margin. Clipped to the window, because nothing can be painted (or hovered)
 * outside it.
 */
export function hotRect(capsule: Rect, phase: HoverPhase, viewport: Rect | null = null, cfg: HoverConfig = HOVER_CONFIG): Rect {
  const r = phase === "expanded" ? inflate(capsule, cfg.marginPx) : capsule;
  return viewport ? clip(r, viewport) : r;
}

/**
 * The reducer. `capsule` is the capsule rect being painted at the moment of the
 * sample (the pill measures it live; it grows while expanding), `viewport` the
 * window. Returns the SAME object when nothing changed, so a caller can skip a
 * render on `next === prev`.
 */
export function stepHover(
  s: HoverState,
  input: HoverInput,
  capsule: Rect,
  viewport: Rect | null = null,
  cfg: HoverConfig = HOVER_CONFIG,
): HoverState {
  const hot = hotRect(capsule, s.phase, viewport, cfg);
  const inside = "x" in input ? contains(hot, input.x, input.y) : "gone" in input ? false : null;

  if (s.phase === "collapsed") {
    if (inside) return { phase: "expanded", outsideSince: null, transitions: s.transitions + 1 };
    return s;
  }
  // expanded
  if (inside === true) return s.outsideSince === null ? s : { ...s, outsideSince: null };
  const since = inside === false ? (s.outsideSince ?? input.at) : s.outsideSince;
  if (since !== null && input.at - since >= cfg.collapseAfterMs) {
    return { phase: "collapsed", outsideSince: null, transitions: s.transitions + 1 };
  }
  return since === s.outsideSince ? s : { ...s, outsideSince: since };
}

/** Milliseconds until a pending collapse is due, or null when none is pending. */
export function collapseDueIn(s: HoverState, now: number, cfg: HoverConfig = HOVER_CONFIG): number | null {
  if (s.phase !== "expanded" || s.outsideSince === null) return null;
  return Math.max(0, s.outsideSince + cfg.collapseAfterMs - now);
}

/** The capsule rects a pill paints in each hover phase, and its window. */
export interface PillGeometry {
  collapsed: Rect;
  expanded: Rect;
  viewport: Rect | null;
}

/**
 * `hoverState(rect, cursorPath)` — fold a whole cursor path through the
 * reducer over a pill whose capsule is `rect.collapsed` while collapsed and
 * `rect.expanded` while expanded.
 */
export function hoverState(rect: PillGeometry, cursorPath: readonly HoverInput[], cfg: HoverConfig = HOVER_CONFIG): HoverState {
  let s = initialHover();
  for (const input of cursorPath) {
    s = stepHover(s, input, s.phase === "expanded" ? rect.expanded : rect.collapsed, rect.viewport, cfg);
  }
  return s;
}
