/**
 * yap-1106-yappy-states-and-motion — Yappy's speech bubble: its geometry, its
 * fixed lines, its placement and its one-at-a-time queue.
 *
 * Approved sheet (design-2026-11-06/pill.html §03): "In: fades up 4 px from the
 * pill and grows from 96 to 100 percent. Out: fades only. A new line waits for
 * the old one to leave. Never two bubbles." The bubble is at most
 * `BUBBLE_MAX_WIDTH_PX` wide and clamps to `BUBBLE_MAX_LINES` lines (float.css
 * `.kami-bubble-text`), and on a side dock it keeps `BUBBLE_DOCK_INSET_PX` off
 * the screen edge so it never overlaps the dock.
 *
 * Timings come from `DURATION` (motion.ts); no time literal lives here.
 */
import type { ChatTone, DockSide } from "./live";
import { DURATION } from "./motion";

// ── geometry (mirrors float.css `.kami-bubble` / `.kami-bubble-text`) ───────

/** The widest a bubble may be, border included (float.css `max-width`). */
export const BUBBLE_MAX_WIDTH_PX = 200;
/** The 2-line clamp. */
export const BUBBLE_MAX_LINES = 2;
/** 12 px bold on a 1.25 line. */
export const BUBBLE_LINE_H_PX = 15;
/** 5 px top + bottom padding and a 1 px border, each side. */
export const BUBBLE_CHROME_V_PX = 12;
/** 10 px left + right padding and a 1 px border, each side. */
export const BUBBLE_CHROME_H_PX = 22;
/** The tallest bubble: two lines and its chrome. state.ts reserves this row. */
export const BUBBLE_MAX_H_PX = BUBBLE_MAX_LINES * BUBBLE_LINE_H_PX + BUBBLE_CHROME_V_PX;
/** Clearance kept between the bubble and the screen edge a side dock hugs. */
export const BUBBLE_DOCK_INSET_PX = 8;
/** How far the bubble rises as it enters (and drifts back as it leaves). */
export const BUBBLE_LIFT_PX = 4;
/** The scale it grows from on enter. */
export const BUBBLE_ENTER_SCALE = 0.96;
/** Estimated advance width of the 12 px bold bubble face (system sans averages ≈ 6.0). */
export const BUBBLE_CHAR_PX = 6.2;

/** Characters one bubble line holds (28 at 200 px). */
export const BUBBLE_LINE_CHARS = Math.floor((BUBBLE_MAX_WIDTH_PX - BUBBLE_CHROME_H_PX) / BUBBLE_CHAR_PX);

/**
 * Does `text` fit the 2-line clamp without an ellipsis? Greedy word wrap at
 * `BUBBLE_LINE_CHARS`, the same estimate style as live.ts `fitsSideDockStrip`.
 */
export function fitsBubble(text: string): boolean {
  const words = text.trim().split(/\s+/).filter(Boolean);
  let lines = words.length ? 1 : 0, used = 0;
  for (const w of words) {
    const len = [...w].length;
    if (len > BUBBLE_LINE_CHARS) return false;
    const need = used === 0 ? len : used + 1 + len;
    if (need <= BUBBLE_LINE_CHARS) used = need;
    else { lines++; used = len; }
  }
  return lines <= BUBBLE_MAX_LINES;
}

// ── the copy table ─────────────────────────────────────────────────────────

/** Every FIXED line the bubble says (phase chatter is the voices table). */
export type BubbleLineId = "handsFree" | "handsFreeShort";

/**
 * THE bubble copy table. `{key}` is the live stop binding (`chord_label`) —
 * never hardcoded, the chords are rebindable. Total over every tone.
 */
export const BUBBLE_COPY: Record<BubbleLineId, Record<ChatTone, string>> = {
  handsFree: {
    friendly: "Hands-free: I keep listening until you press {key}",
    rude: "Hands-free: I'm listening till you hit {key}",
    rose: "Hands-free: I keep listening until you press {key} 🌹",
  },
  handsFreeShort: {
    friendly: "Hands-free: press {key} to stop",
    rude: "Hands-free: {key} stops me",
    rose: "Hands-free: {key} to stop, love",
  },
};

/** One line from the table, the key filled in. */
export function bubbleCopy(id: BubbleLineId, tone: ChatTone, key: string): string {
  return BUBBLE_COPY[id][tone].split("{key}").join(key);
}

/**
 * The hands-free explanation, falling back to the short line when a long
 * rebind (⌃⌥⇧⌘Backspace) would break the 2-line clamp.
 */
export function handsFreeExplainLine(tone: ChatTone, key: string): string {
  const full = bubbleCopy("handsFree", tone, key);
  return fitsBubble(full) ? full : bubbleCopy("handsFreeShort", tone, key);
}

// ── placement ──────────────────────────────────────────────────────────────

export interface BubbleBox { x: number; w: number }

/**
 * The bubble's CENTRE x (it is drawn `translate(-50%)`): as close to the
 * capsule centre `cx` as the box allows, never closer than
 * `BUBBLE_DOCK_INSET_PX` to the edge a side dock hugs. On a side dock that
 * pushes it inward — the side that faces the middle of the screen.
 */
export function bubbleCentreX(cx: number, bubbleW: number, box: BubbleBox, dock: DockSide | string | undefined): number {
  if (!(bubbleW > 0)) return cx;
  const lo = box.x + bubbleW / 2 + (dock === "left" ? BUBBLE_DOCK_INSET_PX : 0);
  const hi = box.x + box.w - bubbleW / 2 - (dock === "right" ? BUBBLE_DOCK_INSET_PX : 0);
  if (hi < lo) return (lo + hi) / 2;
  return Math.max(lo, Math.min(hi, cx));
}

// ── the queue: never two bubbles ───────────────────────────────────────────

export interface BubbleView {
  setText(text: string): void;
  setShown(shown: boolean): void;
}
export interface BubbleScheduler {
  after(ms: number, fn: () => void): number;
  cancel(id: number): void;
}
export interface BubbleQueue {
  /** Say `text` ("" = hide). A new line waits for the old one to leave. */
  say(text: string): void;
  /** What is on screen now ("" while hidden or leaving). */
  current(): string;
  dispose(): void;
}

/** How long the queue waits for a bubble to leave. */
export const bubbleExitMs = (reduceMotion: boolean): number =>
  reduceMotion ? DURATION.reducedCrossfade : DURATION.bubbleOut;

export function createBubbleQueue(view: BubbleView, sched: BubbleScheduler, opts: { reduceMotion?: boolean } = {}): BubbleQueue {
  let shown = "";          // the line on screen, "" when hidden
  let leaving = false;     // an exit is in flight
  let pending: string | null = null;
  let timer = 0;
  const exitMs = bubbleExitMs(!!opts.reduceMotion);

  const enter = (t: string) => { view.setText(t); view.setShown(true); shown = t; };
  const leave = () => {
    view.setShown(false);
    leaving = true;
    timer = sched.after(exitMs, () => {
      leaving = false; timer = 0; shown = "";
      const next = pending; pending = null;
      if (next) enter(next); else view.setText("");
    });
  };

  return {
    say(text: string) {
      if (leaving) { pending = text || null; return; }
      if (text === shown) return;
      if (!shown) { if (text) enter(text); return; }
      pending = text || null;
      leave();
    },
    current: () => (leaving ? "" : shown),
    dispose() { if (timer) sched.cancel(timer); timer = 0; pending = null; },
  };
}
