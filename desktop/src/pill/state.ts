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
import { REST_CAPSULE } from "./characterFit";

/** `after` = stopping, processing, notes ready, failed: the post-stop badge. */
export type MeetingMode = "none" | "recording" | "after";
/**
 * `yappy` lays out from this module. `legacy` is any character that still owns
 * a full-window stage of its own (the shell-body path; the Classic pill that
 * used it was retired by yap-1106-retire-classic-pill): it keeps the
 * historical fixed panel.
 */
export type LayoutCharacter = "yappy" | "legacy";

/**
 * Which layout a mounted character gets, from whether it declares a `stage`.
 * Lives here so the mount point (float-main.tsx) never names a creature
 * (characters.test.ts a_new_creature_needs_no_shell_change).
 */
export function layoutCharacterFor(hasStage: boolean): LayoutCharacter {
  return hasStage ? "yappy" : "legacy";
}

export interface Box { x: number; y: number; w: number; h: number }

export interface PillInput {
  phase: LivePhase;
  meeting: MeetingMode;
  dock: BarDock;
  character?: LayoutCharacter;
  /** Lines the hint reserves; defaults to 3 for `blind` (it wraps), else 1. */
  hintLines?: number;
  /**
   * A card on screen (the copy card, "Record this meeting?", WF-12's
   * "Sounded like Spanish — Add Spanish?", the coaching nudge), with its
   * measured height — the whole stack's, when more than one is up. It gets
   * the top row of the panel.
   */
  card?: { h: number } | null;
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
  /** The decode progress row under Yappy (transcribing only). */
  progress: Box | null;
  /** A card's row above everything else, when one is on screen. */
  card: Box | null;
  /** The character's centre inside the panel (float_pill.rs parks by it). */
  anchor: { x: number; y: number };
}

/** The gap kept between the content union and the panel edge (= pixelGrid DOCK_PAD). */
export const PANEL_MARGIN = 10;
/** The gap between two slots of the pill. */
export const SLOT_GAP = 6;
/** Acceptance: the clock touches or sits within this many points of the character. */
export const CLOCK_REACH_PT = 12;
/**
 * Yappy's resting capsule (YappyPill: lerp(REST_CAPSULE.w, …) at openV = 0).
 * yap-1106-fb-10 — sized by characterFit to hold every pill character WHOLE
 * (was 72 x 22, which only fitted a face close-up: "the heads cropped").
 */
export const CHARACTER_REST: Readonly<{ w: number; h: number }> = REST_CAPSULE;
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
/**
 * The clock row. yap-1106-pill-rec-headphones-hoot-tape: no red dot any more —
 * the meeting scene (MeetingWorld, SCENE_W x SCENE_H = 112 x 40: sun, time,
 * Hoot at the tape, Yappy in headphones) + call-audio mark + stop. 40 tall is
 * the scene at a whole-number scale, and still clears the bubble row above
 * (vertically centred on the 22 pt capsule: -9 vs the bubble's -10).
 */
export const CLOCK_W: Record<Exclude<MeetingMode, "none">, number> = { recording: 208, after: 184 };
export const CLOCK_H = 40;
/** Y3-C's decode fill + numeral (`.kami-progress`): a row under Yappy while transcribing. */
export const PROGRESS_W = 100;
export const PROGRESS_H = 14;
/** The phase that carries the determinate progress row. */
export const PROGRESS_PHASE: LivePhase = "transcribing";
/** A card's width: the old 300 pt panel less its 8 pt insets. */
export const CARD_W = 284;
/**
 * fix-b1-2 — every card that takes the panel's top row while it is on
 * screen: the copy card, "Record this meeting?", WF-12's language offer and
 * the coaching nudge. float-main measures EVERY match (not the first), stacks
 * them in DOM order with `stackCards`, and hands the stack's height to
 * `pillLayout` as `card`; float.css places each one in that row. A card left
 * off this list is neither measured nor placed, so it is drawn over Yappy.
 */
export const CARD_CLASSES = ["copy-card", "meeting-offer-wrap", "language-offer-wrap", "nudge-row"] as const;
export const CARD_SELECTOR: string = CARD_CLASSES.map((c) => `.${c}`).join(", ");
/** The CSS custom property float.css adds to `--card-y` for a card's place in the stack. */
export const CARD_STACK_VAR = "--card-stack-y";

/**
 * fix-b1-2 — two cards can be up at once (a nudge and a language offer), so
 * the card row is a STACK: each card's top inside the row, SLOT_GAP apart,
 * and the row's total height. A card with no height (not laid out yet) takes
 * no room and no gap.
 */
export function stackCards(heights: readonly number[]): { offsets: number[]; total: number } {
  const offsets: number[] = [];
  let y = 0;
  for (const raw of heights) {
    const h = Math.max(0, Math.ceil(raw));
    if (h === 0) {
      offsets.push(y);
      continue;
    }
    if (y > 0) y += SLOT_GAP;
    offsets.push(y);
    y += h;
  }
  return { offsets, total: y };
}
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
  const key = `${character}|${dock}|${meeting}|${RESTING_PHASES.has(phase) ? "rest" : "open"}|${phase === "blind" ? "b" : ""}|${phase === PROGRESS_PHASE ? "p" : ""}|${input.card ? `c${Math.ceil(input.card.h)}` : ""}`;
  if (character === "legacy") {
    const w = LEGACY_PANEL.w, h = LEGACY_PANEL.h;
    const char = { x: 0, y: 0, w, h };
    return {
      key, phase, meeting, dock, character, legacy: true, panel: { w, h }, char,
      bubble: char, hint: char, clock: null, progress: null, card: null, anchor: { x: w / 2, y: h / 2 },
    };
  }
  const env = RESTING_PHASES.has(phase) ? CHARACTER_REST : CHARACTER_OPEN;
  const lines = Math.max(1, Math.round(input.hintLines ?? (phase === "blind" ? 3 : 1)));
  const hintH = lines * HINT_LINE_H;
  // 1 — everything placed relative to the character's top-left at (0, 0).
  const char0: Box = { x: 0, y: 0, w: env.w, h: env.h };
  // A box of width `w` aligned to the character's dock edge (centred at the bottom dock).
  const alignX = (w: number) => (dock === "left" ? 0 : dock === "right" ? env.w - w : (env.w - w) / 2);
  // The next free row under the character.
  let below = env.h + SLOT_GAP;
  let clock0: Box | null = null;
  if (meeting !== "none") {
    const cw = CLOCK_W[meeting];
    if (dock === "bottom") {
      // The pill's own trailing slot: beside Yappy, vertically centred on him.
      clock0 = { x: env.w + SLOT_GAP, y: (env.h - CLOCK_H) / 2, w: cw, h: CLOCK_H };
    } else {
      // A side dock is a column at the screen edge: the slot sits under him,
      // flush to the same edge, so the clock is never stranded mid-screen.
      clock0 = { x: alignX(cw), y: below, w: cw, h: CLOCK_H };
      below += CLOCK_H + SLOT_GAP;
    }
  }
  const progress0: Box | null =
    phase === PROGRESS_PHASE ? { x: alignX(PROGRESS_W), y: below, w: PROGRESS_W, h: PROGRESS_H } : null;
  const bubble0: Box = { x: alignX(BUBBLE_W), y: -BUBBLE_LIFT - BUBBLE_H, w: BUBBLE_W, h: BUBBLE_H };
  const hint0: Box = { x: alignX(HINT_W), y: bubble0.y - SLOT_GAP - hintH, w: HINT_W, h: hintH };
  const cardH = input.card ? Math.max(1, Math.ceil(input.card.h)) : 0;
  const card0: Box | null = cardH ? { x: alignX(CARD_W), y: hint0.y - SLOT_GAP - cardH, w: CARD_W, h: cardH } : null;
  // 2 — the union, then translate it PANEL_MARGIN in from the panel's top-left.
  const all = [char0, bubble0, hint0, ...(clock0 ? [clock0] : []), ...(progress0 ? [progress0] : []), ...(card0 ? [card0] : [])];
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
    progress: progress0 ? shift(progress0, dx, dy) : null,
    card: card0 ? shift(card0, dx, dy) : null,
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
  if (l.progress) named.push(["progress", l.progress]);
  if (l.card) named.push(["card", l.card]);
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
  const none = { x: 0, y: 0, w: 0, h: 0 };
  const c = l.clock ?? none;
  const p = l.progress ?? none;
  const k = l.card ?? none;
  return {
    "--panel-w": px(l.panel.w), "--panel-h": px(l.panel.h),
    "--char-x": px(l.char.x), "--char-y": px(l.char.y), "--char-w": px(l.char.w), "--char-h": px(l.char.h),
    "--bubble-x": px(l.bubble.x), "--bubble-y": px(l.bubble.y), "--bubble-w": px(l.bubble.w), "--bubble-h": px(l.bubble.h),
    "--hint-x": px(l.hint.x), "--hint-y": px(l.hint.y), "--hint-w": px(l.hint.w), "--hint-h": px(l.hint.h),
    "--clock-x": px(c.x), "--clock-y": px(c.y), "--clock-w": px(c.w), "--clock-h": px(c.h),
    "--progress-x": px(p.x), "--progress-y": px(p.y), "--progress-w": px(p.w), "--progress-h": px(p.h),
    // With no card yet, a card that mounts is laid out at its real width (so
    // the height it is measured at is the height it will have), then moved
    // into the row the next layout reserves for it.
    "--card-x": px(k.x), "--card-y": px(k.y), "--card-w": px(l.card ? k.w : CARD_W),
  };
}

/* ── the published layout ────────────────────────────────────────────────── */
/*
 * Yappy's draw loop is a mount-once effect that reads the dock live off
 * <html>; it reads the layout the same way, through this getter, so no prop
 * has to thread into the loop. float-main is the only writer.
 */
let published: PillLayout | null = null;
/**
 * fix-b1-2 — fired on `window` each time a layout is applied, AFTER the slot
 * positions are written: a card that holds the window's hitbox re-takes it
 * from its real box here, since the layout MOVES the card (into the row it
 * reserved) without resizing it, and a ResizeObserver never sees a move.
 */
export const PILL_LAYOUT_EVENT = "yap-pill-layout";
export function publishPillLayout(l: PillLayout | null): void {
  published = l;
  if (typeof window !== "undefined") {
    (window as unknown as { __YAP_PILL_LAYOUT__?: PillLayout | null }).__YAP_PILL_LAYOUT__ = l;
    window.dispatchEvent(new Event(PILL_LAYOUT_EVENT));
  }
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
