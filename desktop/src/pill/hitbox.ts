/**
 * Y5-E — hover hysteresis and alpha hit-testing for the docked pill.
 *
 * THE BUG THIS EXISTS TO KILL. An edge-docked pill that expands on hover and
 * collapses on leave oscillates forever when the cursor dwells on the screen
 * edge: the cursor sits one pixel inside the COLLAPSED capsule, the capsule
 * expands, the layout shifts under the cursor, the cursor is now outside the
 * thing it was hovering, the capsule collapses — and the next mouse sample
 * starts the loop again. Wispr shipped a source comment about hitting exactly
 * this (parity note §4.4, "Hit-testing and hover (the part that breaks naive
 * implementations)").
 *
 * THE FIX IS GEOMETRY, NOT DELAY. Expand on the first sample inside the
 * COLLAPSED rect; collapse only once the cursor has been outside the EXPANDED
 * rect — capsule plus its invisible `inset: -12px` margin — for a short
 * debounce. Because the expanded rect strictly contains the collapsed one, a
 * cursor that just triggered an expand is still inside the region that keeps it
 * expanded, so the loop cannot close. A long timeout would also "fix" the
 * oscillation and would make the pill feel dead; that is explicitly not what
 * this does (`collapseDebounceMs` is a small settle, not the mechanism).
 *
 * THE MARGIN IS CONDITIONAL, AND THE CONDITIONALITY IS THE FIX. Painted always,
 * a 12 px hot ring around an idle 16 px seed is a dead zone that swallows
 * clicks meant for whatever is under it. Painted never, the boundary
 * oscillates. So float.css paints `.pill::before { inset: -12px }` ONLY under
 * the expanded selectors, and `publishedHitbox()` below reports exactly the
 * same two rects to the backend, so the NSPanel's ignore-mouse-events region
 * and the painted box are the same box in both states. A margin CSS believes in
 * and the panel does not is worse than no margin: the cursor would leave the
 * OS-level hot region, the webview would stop receiving pointermove, and the
 * machine would never see the samples that keep it expanded.
 *
 * This module is PURE (no DOM, no Tauri). `drag.ts` reads the live element and
 * calls in; `hitbox.test.ts` drives the same reducer over synthetic paths.
 */

export type Dock = "top" | "left" | "right";

/** A rect in the float window's logical points — the units `pill_set_hitbox` takes. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** One cursor sample: position plus the millisecond timestamp it arrived at. */
export interface CursorSample {
  x: number;
  y: number;
  t: number;
}

export type HoverPhase = "collapsed" | "expanded";

/**
 * The hysteresis table. Y5-D owns `motion.ts` and its constants table; that
 * file is not on main yet (PR open), so these live here rather than in a
 * whole-file conflict with it — see the PR body. When Y5-D lands, fold this
 * object into `motion.ts`'s table and re-export it from here; nothing else
 * needs to move, because every consumer reads it through this name.
 */
export const HOVER_MOTION = {
  /**
   * The invisible alpha margin painted around the capsule while expanded, in
   * px. Parity note P0 #3 specifies 12. It is uniform on all four sides,
   * exactly as `inset: -12px` paints it, so the published rect and the painted
   * box cannot drift apart per dock edge.
   */
  expandedMarginPx: 12,
  /**
   * Background alpha of the hot area. Non-zero so macOS/the webview treat the
   * box as a real hit target; low enough to be invisible over any wallpaper.
   */
  hotAreaAlpha: 0.004,
  /**
   * How long the cursor must be continuously outside the EXPANDED rect before
   * the capsule collapses. A settle for jitter, not the anti-oscillation
   * mechanism — the geometry above is that.
   */
  collapseDebounceMs: 120,
  /** Expand fires on the first sample inside the collapsed rect. No lag. */
  expandDelayMs: 0,
} as const;

/**
 * The classes float.css treats as "the capsule is at full size" AND paints the
 * alpha margin under. Kept here so the CSS selector list and the rect the
 * backend is told about are derived from one list rather than two that drift —
 * if you add a selector to `.pill::before` in float.css, add its token here in
 * the same commit or the panel stops matching the paint.
 *
 * The meeting capsule is deliberately absent: its expansion is state-driven and
 * lasts as long as the meeting, so it cannot oscillate, and naming its class
 * here would re-type the `meeting` event literal that
 * `src-tauri/tests/meeting_event_contract.rs` forbids outside
 * `meetings/consent.ts`. Hovering a meeting capsule still expands it through
 * `data-hover`, which is on this list by construction.
 */
export const EXPANDED_CLASSES = ["live", "busy", "done"] as const;

/** The attribute ClassicPill stamps when the hover machine says "expanded". */
export const HOVER_ATTR = "data-hover";

/** Grow a rect uniformly. Negative amounts shrink it. */
export function inflate(r: Rect, by: number): Rect {
  return { x: r.x - by, y: r.y - by, w: r.w + by * 2, h: r.h + by * 2 };
}

/** The capsule plus the invisible alpha margin — what `inset: -12px` paints. */
export function expandedRect(collapsed: Rect, margin: number = HOVER_MOTION.expandedMarginPx): Rect {
  return inflate(collapsed, margin);
}

/**
 * The rect the backend must be told about for a given phase. Collapsed: the
 * bare capsule, so there is no dead zone around an idle seed. Expanded: capsule
 * + margin, so the OS hot region matches the painted `::before`.
 */
export function publishedHitbox(
  collapsed: Rect,
  phase: HoverPhase,
  margin: number = HOVER_MOTION.expandedMarginPx,
): Rect {
  return phase === "expanded" ? expandedRect(collapsed, margin) : { ...collapsed };
}

/** Half-open containment, so a cursor exactly on the right/bottom edge is out. */
export function contains(r: Rect, p: { x: number; y: number }): boolean {
  return p.x >= r.x && p.x < r.x + r.w && p.y >= r.y && p.y < r.y + r.h;
}

/** The rect that currently decides "is the cursor on the pill" — the hysteresis. */
export function hotRect(collapsed: Rect, phase: HoverPhase, margin: number = HOVER_MOTION.expandedMarginPx): Rect {
  return publishedHitbox(collapsed, phase, margin);
}

export interface HoverMachineState {
  phase: HoverPhase;
  /** Wall-clock ms at which a pending collapse fires, or null if none is armed. */
  collapseAt: number | null;
  /** How many times the phase actually changed. The acceptance metric. */
  transitions: number;
}

export const INITIAL_HOVER: HoverMachineState = { phase: "collapsed", collapseAt: null, transitions: 0 };

export interface HoverOptions {
  margin?: number;
  collapseDebounceMs?: number;
}

/**
 * One step of the machine. Pure: same state + same sample => same next state.
 *
 * - collapsed + inside the COLLAPSED rect  -> expanded (immediately).
 * - expanded  + inside the EXPANDED  rect  -> stay expanded, disarm any pending collapse.
 * - expanded  + outside the EXPANDED rect  -> arm a collapse; fire it once the
 *   cursor has been continuously outside for `collapseDebounceMs`.
 *
 * Pass a sample with an unchanged position and a later `t` to let a pending
 * collapse mature when the cursor has simply stopped moving (ClassicPill does
 * this from a timer; the tests do it explicitly).
 */
export function hoverStep(
  state: HoverMachineState,
  collapsed: Rect,
  sample: CursorSample,
  opts: HoverOptions = {},
): HoverMachineState {
  const margin = opts.margin ?? HOVER_MOTION.expandedMarginPx;
  const debounce = opts.collapseDebounceMs ?? HOVER_MOTION.collapseDebounceMs;
  const inside = contains(hotRect(collapsed, state.phase, margin), sample);

  if (state.phase === "collapsed") {
    if (!inside) return state.collapseAt === null ? state : { ...state, collapseAt: null };
    return { phase: "expanded", collapseAt: null, transitions: state.transitions + 1 };
  }

  if (inside) return state.collapseAt === null ? state : { ...state, collapseAt: null };

  const due = state.collapseAt ?? sample.t + debounce;
  if (sample.t >= due) {
    return { phase: "collapsed", collapseAt: null, transitions: state.transitions + 1 };
  }
  return { ...state, collapseAt: due };
}

/**
 * Fold a whole cursor path through the machine. `rect` is the COLLAPSED capsule
 * rect; the expanded rect is derived, never passed in, so a caller cannot feed
 * the two inconsistently.
 */
export function hoverState(
  rect: Rect,
  cursorPath: readonly CursorSample[],
  opts: HoverOptions = {},
): HoverMachineState {
  let s = INITIAL_HOVER;
  for (const sample of cursorPath) s = hoverStep(s, rect, sample, opts);
  return s;
}

/** The phase sequence a path produces — for asserting on the shape of the run. */
export function hoverPhases(
  rect: Rect,
  cursorPath: readonly CursorSample[],
  opts: HoverOptions = {},
): HoverPhase[] {
  const out: HoverPhase[] = [];
  let s = INITIAL_HOVER;
  for (const sample of cursorPath) {
    s = hoverStep(s, rect, sample, opts);
    out.push(s.phase);
  }
  return out;
}

/**
 * A stateful wrapper for the running app. ClassicPill owns one of these and
 * feeds it `pointermove`; `collapseAt` tells it when to schedule the one timer
 * that lets a pending collapse mature after the cursor stops moving.
 */
export function createHoverMachine(opts: HoverOptions = {}) {
  let state = INITIAL_HOVER;
  return {
    get phase(): HoverPhase {
      return state.phase;
    },
    get collapseAt(): number | null {
      return state.collapseAt;
    },
    get transitions(): number {
      return state.transitions;
    },
    /** Returns true when the PHASE changed, so the caller only touches the DOM then. */
    push(rect: Rect, sample: CursorSample): boolean {
      const next = hoverStep(state, rect, sample, opts);
      const changed = next.phase !== state.phase;
      state = next;
      return changed;
    },
    /** The cursor left the window entirely — collapse without waiting. */
    reset(): boolean {
      const changed = state.phase !== "collapsed";
      state = { phase: "collapsed", collapseAt: null, transitions: state.transitions + (changed ? 1 : 0) };
      return changed;
    },
  };
}
