/**
 * yap-1106-one-pill — ONE pill, ONE layout.
 *
 * Wilson's 2026-11-06 screenshots: the meeting clock hung UNDER Yappy as a
 * second capsule (bottom dock), floated alone 150 pt away from him (side
 * docks), and "Hands-free — fn to stop" was drawn over his face. The cause was
 * not two windows (ARCHITECTURE-PILL-2026-11-06.md): the pill, the clock, the
 * hint line and the bubble all live in the one `float` NSPanel, but each was
 * placed by its own CSS rule against a fixed 300x140 window, so nothing knew
 * where Yappy actually was.
 *
 * This module is the one reducer that replaces those six independent rules:
 *
 *     {dictation phase} x {meeting: none | recording | after} x {dock}  →  one PillLayout
 *
 * Every box is placed RELATIVE TO THE CHARACTER first (the clock in Yappy's
 * trailing slot, the bubble and the hint stacked above him, aligned to his dock
 * edge), then the whole group is translated so its union sits PANEL_MARGIN in
 * from the panel's edges, and the panel is sized to exactly that union plus the
 * margin. So by construction:
 *   - the panel frame equals the union of its content boxes + margin;
 *   - nothing is ever outside the panel;
 *   - the clock touches (SLOT_GAP <= CLOCK_REACH_PT) the character box;
 *   - the hint, bubble, clock and character boxes never overlap.
 * `layoutProblems()` checks exactly those four, and the pill matrix runs it
 * against the painted page (scripts/pill-matrix.mjs).
 *
 * `anchor` is the character's centre inside the panel. float_pill.rs parks the
 * panel by that point (bottom dock: centred on the screen; side docks: centred
 * vertically), so Yappy stays put on screen when the clock appears and the
 * panel grows around him.
 *
 * Units are CSS px, which are logical points in the WKWebView.
 */
import type { BarDock } from "./dock";
import type { LivePhase } from "./live";
import type { MeetingAfterView, MeetingStatus } from "./meeting";

/** `after` = stopping, processing, notes ready, failed: the post-stop badge. */
export type MeetingMode = "none" | "recording" | "after";
/**
 * `yappy` lays out from this module. `legacy` is any character that still owns
 * a full-window stage of its own (the Classic pill, being retired by
 * yap-1106-retire-classic-pill): it keeps the historical fixed panel.
 */
export type LayoutCharacter = "yappy" | "legacy";

export interface Box { x: number; y: number; w: number; h: number }

export interface PillInput {
  phase: LivePhase;
  meeting: MeetingMode;
  dock: BarDock;
  character?: LayoutCharacter;
  /** Lines the hint reserves; defaults to 3 for `blind` (it wraps), else 1. */
  hintLines?: number;
}

export interface PillLayout {
  /** Stable identity of the inputs that shaped this layout. */
  key: string;
  phase: LivePhase;
  meeting: MeetingMode;
  dock: BarDock;
  character: LayoutCharacter;
  legacy: boolean;
  panel: { w: number; h: number };
  /** The character's box: the capsule's envelope for this phase class. */
  char: Box;
  bubble: Box;
  hint: Box;
  /** The meeting clock / post-stop badge, in Yappy's trailing slot. */
  clock: Box | null;
  /** The character's centre inside the panel (float_pill.rs parks by it). */
  anchor: { x: number; y: number };
}

/** The gap kept between the content union and the panel edge (= pixelGrid DOCK_PAD). */
export const PANEL_MARGIN = 10;
/** The gap between two slots of the pill. */
export const SLOT_GAP = 6;
/** Acceptance: the clock touches or sits within this many points of the character. */
export const CLOCK_REACH_PT = 12;
/** Yappy's resting capsule (YappyPill: lerp(72, …), lerp(22, …) at openV = 0). */
export const CHARACTER_REST = { w: 72, h: 22 } as const;
/** Yappy's widest open capsule: 236 x 60 at the desk tier (x 1.16), rounded up. */
export const CHARACTER_OPEN = { w: 274, h: 70 } as const;
/** The speech bubble's reserved row (12 px bold + 5 px padding + border). */
export const BUBBLE_W = 200;
export const BUBBLE_H = 26;
/** The bubble's tail + air between it and the capsule (YappyPill: y0 - 10). */
export const BUBBLE_LIFT = 10;
/** `.phase-strip` max-width (SIDE_DOCK_TEXT_PX) and its 11px/1.25 line. */
export const HINT_W = 192;
export const HINT_LINE_H = 14;
/** The clock row: dot + "00:03:37" + call-audio mark + stop; the after badge is wider. */
export const CLOCK_W: Record<Exclude<MeetingMode, "none">, number> = { recording: 132, after: 176 };
export const CLOCK_H = 26;
/** The historical fixed panel (float_pill.rs PILL_W x PILL_H) a legacy character keeps. */
export const LEGACY_PANEL = { w: 300, h: 140 } as const;
/** Phases Yappy draws as the small resting capsule; every other phase opens him. */
export const RESTING_PHASES: ReadonlySet<LivePhase> = new Set<LivePhase>(["idle", "sleepy"]);

/** The meeting slot from the meeting pipeline's two views (float-main owns both). */
export function meetingMode(status: Pick<MeetingStatus, "recording"> | null | undefined, after: MeetingAfterView | null | undefined): MeetingMode {
  if (after) return "after";
  return status?.recording ? "recording" : "none";
}

const right = (b: Box) => b.x + b.w;
const bottom = (b: Box) => b.y + b.h;
const shift = (b: Box, dx: number, dy: number): Box => ({ x: b.x + dx, y: b.y + dy, w: b.w, h: b.h });

/** The one reducer: phase x meeting x dock → the pill's layout. Pure. */
export function pillLayout(input: PillInput): PillLayout {
  const { phase, meeting, dock } = input;
  const character: LayoutCharacter = input.character ?? "yappy";
  const key = `${character}|${dock}|${meeting}|${RESTING_PHASES.has(phase) ? "rest" : "open"}|${phase === "blind" ? "b" : ""}`;
  if (character === "legacy") {
    const w = LEGACY_PANEL.w, h = LEGACY_PANEL.h;
    const char = { x: 0, y: 0, w, h };
    return {
      key, phase, meeting, dock, character, legacy: true, panel: { w, h }, char,
      bubble: char, hint: char, clock: null, anchor: { x: w / 2, y: h / 2 },
    };
  }
  const env = RESTING_PHASES.has(phase) ? CHARACTER_REST : CHARACTER_OPEN;
  const lines = Math.max(1, Math.round(input.hintLines ?? (phase === "blind" ? 3 : 1)));
  const hintH = lines * HINT_LINE_H;
  // 1 — everything placed relative to the character's top-left at (0, 0).
  const char0: Box = { x: 0, y: 0, w: env.w, h: env.h };
  // A box of width `w` aligned to the character's dock edge (centred at the bottom dock).
  const alignX = (w: number) => (dock === "left" ? 0 : dock === "right" ? env.w - w : (env.w - w) / 2);
  let clock0: Box | null = null;
  if (meeting !== "none") {
    const cw = CLOCK_W[meeting];
    clock0 = dock === "bottom"
      // The pill's own trailing slot: beside Yappy, vertically centred on him.
      ? { x: env.w + SLOT_GAP, y: (env.h - CLOCK_H) / 2, w: cw, h: CLOCK_H }
      // A side dock is a column at the screen edge: the slot sits under him,
      // flush to the same edge, so the clock is never stranded mid-screen.
      : { x: alignX(cw), y: env.h + SLOT_GAP, w: cw, h: CLOCK_H };
  }
  const bubble0: Box = { x: alignX(BUBBLE_W), y: -BUBBLE_LIFT - BUBBLE_H, w: BUBBLE_W, h: BUBBLE_H };
  const hint0: Box = { x: alignX(HINT_W), y: bubble0.y - SLOT_GAP - hintH, w: HINT_W, h: hintH };
  // 2 — the union, then translate it PANEL_MARGIN in from the panel's top-left.
  const all = [char0, bubble0, hint0, ...(clock0 ? [clock0] : [])];
  const minX = Math.min(...all.map((b) => b.x));
  const minY = Math.min(...all.map((b) => b.y));
  const maxX = Math.max(...all.map(right));
  const maxY = Math.max(...all.map(bottom));
  const dx = PANEL_MARGIN - minX, dy = PANEL_MARGIN - minY;
  const char = shift(char0, dx, dy);
  return {
    key, phase, meeting, dock, character, legacy: false,
    panel: { w: Math.ceil(maxX - minX + 2 * PANEL_MARGIN), h: Math.ceil(maxY - minY + 2 * PANEL_MARGIN) },
    char,
    bubble: shift(bubble0, dx, dy),
    hint: shift(hint0, dx, dy),
    clock: clock0 ? shift(clock0, dx, dy) : null,
    anchor: { x: char.x + char.w / 2, y: char.y + char.h / 2 },
  };
}

/** Where Yappy's capsule (of width `capW`) is centred inside his box at this dock. */
export function capsuleCentre(layout: PillLayout, capW: number): { x: number; y: number } {
  const c = layout.char;
  const x = layout.dock === "left" ? c.x + capW / 2 : layout.dock === "right" ? right(c) - capW / 2 : c.x + c.w / 2;
  return { x, y: c.y + c.h / 2 };
}

/** True when two boxes share any area (touching edges is not an overlap). */
export function boxesOverlap(a: Box, b: Box, tol = 0.5): boolean {
  return a.x < right(b) - tol && b.x < right(a) - tol && a.y < bottom(b) - tol && b.y < bottom(a) - tol;
}

/** The gap between two boxes in points (0 when they touch or overlap). */
export function boxGap(a: Box, b: Box): number {
  const gx = Math.max(0, b.x - right(a), a.x - right(b));
  const gy = Math.max(0, b.y - bottom(a), a.y - bottom(b));
  return Math.hypot(gx, gy);
}

/** The union of boxes. */
export function unionBox(boxes: readonly Box[]): Box {
  const x = Math.min(...boxes.map((b) => b.x));
  const y = Math.min(...boxes.map((b) => b.y));
  return { x, y, w: Math.max(...boxes.map(right)) - x, h: Math.max(...boxes.map(bottom)) - y };
}

/**
 * The four acceptance checks, as findings (empty = clean). Shared by the
 * reducer tests and, serialized, by the pill matrix against painted boxes.
 */
export function layoutProblems(l: PillLayout): string[] {
  if (l.legacy) return [];
  const out: string[] = [];
  const named: Array<[string, Box]> = [["character", l.char], ["bubble", l.bubble], ["hint", l.hint]];
  if (l.clock) named.push(["clock", l.clock]);
  for (let i = 0; i < named.length; i++) {
    for (let j = i + 1; j < named.length; j++) {
      if (boxesOverlap(named[i][1], named[j][1])) out.push(`overlap ${named[i][0]}/${named[j][0]}`);
    }
  }
  if (l.clock && boxGap(l.clock, l.char) > CLOCK_REACH_PT) out.push(`clock ${boxGap(l.clock, l.char).toFixed(1)}pt from the character`);
  const u = unionBox(named.map((n) => n[1]));
  const wantW = Math.ceil(u.w + 2 * PANEL_MARGIN), wantH = Math.ceil(u.h + 2 * PANEL_MARGIN);
  if (l.panel.w !== wantW || l.panel.h !== wantH) out.push(`panel ${l.panel.w}x${l.panel.h} != union+margin ${wantW}x${wantH}`);
  for (const [n, b] of named) {
    if (b.x < 0 || b.y < 0 || right(b) > l.panel.w + 0.5 || bottom(b) > l.panel.h + 0.5) out.push(`${n} outside the panel`);
  }
  return out;
}

/**
 * The CSS custom properties float.css places the DOM slots with. Written on
 * `<html>` by float-main; nothing else in float.css positions a slot in
 * one-pill mode.
 */
export function layoutVars(l: PillLayout): Record<string, string> {
  const px = (n: number) => `${Math.round(n * 100) / 100}px`;
  const c = l.clock ?? { x: 0, y: 0, w: 0, h: 0 };
  return {
    "--panel-w": px(l.panel.w), "--panel-h": px(l.panel.h),
    "--char-x": px(l.char.x), "--char-y": px(l.char.y), "--char-w": px(l.char.w), "--char-h": px(l.char.h),
    "--bubble-x": px(l.bubble.x), "--bubble-y": px(l.bubble.y), "--bubble-w": px(l.bubble.w), "--bubble-h": px(l.bubble.h),
    "--hint-x": px(l.hint.x), "--hint-y": px(l.hint.y), "--hint-w": px(l.hint.w), "--hint-h": px(l.hint.h),
    "--clock-x": px(c.x), "--clock-y": px(c.y), "--clock-w": px(c.w), "--clock-h": px(c.h),
  };
}

/* ── the published layout ────────────────────────────────────────────────── */
/*
 * Yappy's draw loop is a mount-once effect that reads the dock live off
 * <html>; it reads the layout the same way, through this getter, so no prop
 * has to thread into the loop. float-main is the only writer.
 */
let published: PillLayout | null = null;
export function publishPillLayout(l: PillLayout | null): void {
  published = l;
  if (typeof window !== "undefined") (window as unknown as { __YAP_PILL_LAYOUT__?: PillLayout | null }).__YAP_PILL_LAYOUT__ = l;
}
export function currentPillLayout(): PillLayout | null {
  return published;
}

/**
 * Grow now, shrink late: a panel that shrank the instant a take ended would
 * clip Yappy's closing animation, so a smaller layout waits SHRINK_HOLD_MS.
 * Pure decision; float-main owns the timer.
 */
export const SHRINK_HOLD_MS = 700;
export function shrinkDelay(prev: PillLayout | null, next: PillLayout): number {
  if (!prev || prev.legacy !== next.legacy || prev.dock !== next.dock) return 0;
  return next.panel.w < prev.panel.w || next.panel.h < prev.panel.h ? SHRINK_HOLD_MS : 0;
}
