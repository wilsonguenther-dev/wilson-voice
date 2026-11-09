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
import { VDOCK_OPEN, VDOCK_TAB } from "./vdock";
import { BUBBLE_MAX_H_PX, BUBBLE_MAX_WIDTH_PX } from "./bubble";

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
  /**
   * yap-1107-one-pill-during-meeting — the ONE meeting shell: the union of the
   * character and the clock, drawn as one dark device shell behind both, so a
   * meeting is never a second capsule beside Yappy. Null without a meeting.
   */
  shell: Box | null;
  /**
   * yap-1107-one-pill-during-meeting — a meeting is recording and no take is
   * live: the meeting scene (MeetingWorld: sun clock, Hoot at the tape, Yappy in
   * headphones) IS the pill. `char` and `clock` are the same box and Yappy's
   * own sky capsule is not drawn, so there is one Yappy and one surface.
   */
  sceneCharacter: boolean;
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
/**
 * Yappy's widest open capsule, rounded up: the panel envelope. yap-1107-pill-matches-artifact-size:
 * OPEN_CAPSULE is now 228 x 53 (the artifact's 228 x 52; yap-1107-parity-pill), and a prop tier only
 * widens it (openCapsuleFor: 265 x 53 at the desk tier x 1.16); the envelope keeps its old
 * 236 x 60 x 1.16 size so the window, hit targets and dock math do not move.
 */
export const CHARACTER_OPEN = { w: 274, h: 70 } as const;
/**
 * The speech bubble's reserved row. yap-1106-yappy-states-and-motion: the
 * bubble clamps to TWO lines (bubble.ts), so the row holds two — a 2-line
 * bubble grows upward inside its own row and never over the hint above it.
 */
export const BUBBLE_W = BUBBLE_MAX_WIDTH_PX;
export const BUBBLE_H = BUBBLE_MAX_H_PX;
/** The bubble's tail + air between it and the capsule (YappyPill: y0 - 10). */
export const BUBBLE_LIFT = 10;
/** `.phase-strip` max-width (SIDE_DOCK_TEXT_PX) and its 11px/1.25 line. */
export const HINT_W = 192;
export const HINT_LINE_H = 14;
/**
 * The clock row. yap-1106-pill-rec-headphones-hoot-tape: no red dot any more —
 * the meeting scene (MeetingWorld, SCENE_W x SCENE_H = 140 x 36: sun, time,
 * Hoot at the tape, Yappy in headphones) + call-audio mark + stop.
 * yap-1107-parity-pill: pill.html mtgPill / #timer is a 202 x 44 shell (4 px
 * padding, the 140 x 36 window, then the 50 px rec segment: 6 + 8 + 6 + 24 + 6)
 * — was 208 x 40, 9 percent short. "after" keeps the scene plus the word
 * "Saved" (140 + 72, as 112 + 72 before).
 */
export const CLOCK_W: Record<Exclude<MeetingMode, "none">, number> = { recording: 202, after: 212 };
export const CLOCK_H = 44;
/**
 * yap-1107-one-pill-during-meeting — dictating mid-meeting, the clock leaves
 * the scene and "shrinks into a small stack next to stop" (Pill artifact,
 * "Meeting + dictating"): the elapsed time over the call chip, plus stop.
 */
export const CLOCK_STACK_W = 96;

/** The clock slot's width for a meeting mode, at rest (the scene) or mid-take (the stack). */
export function clockWidth(meeting: Exclude<MeetingMode, "none">, resting: boolean): number {
  if (meeting === "recording" && !resting) return CLOCK_STACK_W;
  return CLOCK_W[meeting];
}
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
// WF-05's "Polished — Reveal" toast/card (`.polish-reveal-wrap`) is a card too:
// the opened diff card is taller than the resting panel, so the layout must
// reserve its row or Use original / Keep fall outside the window.
export const CARD_CLASSES = [
  "copy-card",
  "meeting-offer-wrap",
  "language-offer-wrap",
  "nudge-row",
  "polish-reveal-wrap",
  // yap-fx-06 — "Dictation cancelled — meeting still recording" and friends.
  "meeting-dictation-wrap",
  // yap-1106-fb-22a — "Who's talking right now?": float.css already places it
  // in the card row; without it here it was never measured, so the panel did
  // not grow and the card was drawn over Yappy, cut off by the window.
  "who-card-wrap",
  // integrate(yap-1106-fb-02-pill-first-click) — WF-06's "Backtrack caught"
  // toast: off this list it stayed pinned at the panel's top edge, and at a
  // side dock it was drawn over the meeting Stop, so the first press on Stop
  // landed on the toast's headline (pill-matrix --clicks).
  "backtrack-caught-wrap",
] as const;
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

/**
 * fix-l3b1-1-meeting-stack-matches-layout — the ONE test for "a recording
 * meeting's clock is the small stack (time + stop), not the scene". The layout
 * (`clockWidth` through pillLayout and sideLayout) reserves CLOCK_STACK_W for
 * exactly these phases, and both pill shells (YappyPill, PillShell) hand the
 * same answer to MeetingBadge as `stacked`. It was `isListening(gate)` in the
 * shells, so transcribing / polishing / pasting / done / error / cancelled /
 * blind drew the 112 px scene (a second Yappy) into the 96 px box, clipped,
 * with the Stop button cut off.
 */
export function meetingClockStacked(phase: LivePhase): boolean {
  return !RESTING_PHASES.has(phase);
}

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
  // WF-13: an explicit hint reservation (the silent-mic row) is part of the
  // geometric identity, or the memoised layout would never grow for it.
  const hintKey = input.hintLines !== undefined ? `h${Math.max(1, Math.round(input.hintLines))}` : "";
  const key = `${character}|${dock}|${meeting}|${RESTING_PHASES.has(phase) ? "rest" : "open"}|${phase === "blind" ? "b" : ""}|${phase === PROGRESS_PHASE ? "p" : ""}|${input.card ? `c${Math.ceil(input.card.h)}` : ""}${hintKey ? `|${hintKey}` : ""}`;
  if (character === "legacy") {
    const w = LEGACY_PANEL.w, h = LEGACY_PANEL.h;
    const char = { x: 0, y: 0, w, h };
    return {
      key, phase, meeting, dock, character, legacy: true, panel: { w, h }, char,
      bubble: char, hint: char, clock: null, shell: null, sceneCharacter: false, progress: null, card: null, anchor: { x: w / 2, y: h / 2 },
    };
  }
  // yap-1106-pill-vertical-dock-world — a side dock is a TALL shell (64 x 216
  // open, a 30 x 40 tab at rest), flush to the screen edge; every slot (the
  // clock, the progress row, the bubble, the hint, a card) opens INWARD beside
  // it, so nothing ever reaches across the screen edge side.
  const side = dock === "left" || dock === "right";
  const resting = RESTING_PHASES.has(phase);
  // yap-1107-one-pill-during-meeting — recording with no take live: the scene is
  // the character's box (one Yappy, the one in headphones), not a slot beside it.
  // A side dock keeps its <= 64 px tab/room as the character; its clock is a
  // flush segment of the one shell instead (sideLayout).
  const sceneCharacter = !side && meeting === "recording" && resting;
  const env = side ? (resting ? VDOCK_TAB : VDOCK_OPEN) : sceneCharacter ? { w: CLOCK_W.recording, h: CLOCK_H } : resting ? CHARACTER_REST : CHARACTER_OPEN;
  if (side) return sideLayout(key, input, character, env);
  const lines = Math.max(1, Math.round(input.hintLines ?? (phase === "blind" ? 3 : 1)));
  const hintH = lines * HINT_LINE_H;
  // 1 — everything placed relative to the character's top-left at (0, 0).
  const char0: Box = { x: 0, y: 0, w: env.w, h: env.h };
  // A box of width `w` aligned to the character's dock edge (centred at the bottom dock).
  // (side docks returned above through sideLayout, so only the bottom dock reaches here.)
  const alignX = (w: number) => (env.w - w) / 2;
  // The next free row under the character.
  const below = env.h + SLOT_GAP;
  let clock0: Box | null = null;
  if (sceneCharacter) {
    clock0 = { ...char0 };
  } else if (meeting !== "none") {
    const cw = clockWidth(meeting, !meetingClockStacked(phase));
    // yap-1107-one-pill-during-meeting — flush to Yappy (no gap): the clock is a
    // segment of the one shell, never a second capsule floating beside him.
    // The pill's own trailing segment: beside Yappy, vertically centred on him.
    // (Side docks returned above through sideLayout.)
    clock0 = { x: env.w, y: (env.h - CLOCK_H) / 2, w: cw, h: CLOCK_H };
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
  const clock = clock0 ? shift(clock0, dx, dy) : null;
  return {
    key, phase, meeting, dock, character, legacy: false,
    panel: { w: Math.ceil(maxX - minX + 2 * PANEL_MARGIN), h: Math.ceil(maxY - minY + 2 * PANEL_MARGIN) },
    char,
    bubble: shift(bubble0, dx, dy),
    hint: shift(hint0, dx, dy),
    clock,
    shell: clock ? meetingShellBox(char, clock, false) : null,
    sceneCharacter,
    progress: progress0 ? shift(progress0, dx, dy) : null,
    card: card0 ? shift(card0, dx, dy) : null,
    anchor: { x: char.x + char.w / 2, y: char.y + char.h / 2 },
  };
}

/**
 * yap-1106-pill-vertical-dock-world — the side-dock layout. The shell is the
 * character box, flush to the dock edge (no PANEL_MARGIN on that side, so the
 * window parks it touching the screen edge); the slots stack in one column on
 * the inner side, SLOT_GAP from the shell: clock first (so it always sits level
 * with him), then progress, a card, the bubble and the hint. The column is centred on the
 * shell when it is shorter, top-aligned when it is taller.
 */
function sideLayout(key: string, input: PillInput, character: LayoutCharacter, env: Readonly<{ w: number; h: number }>): PillLayout {
  const { phase, meeting, dock } = input;
  const left = dock === "left";
  const lines = Math.max(1, Math.round(input.hintLines ?? (phase === "blind" ? 3 : 1)));
  const char0: Box = { x: 0, y: 0, w: env.w, h: env.h };
  const inX = (w: number) => (left ? env.w + SLOT_GAP : -SLOT_GAP - w);
  const rows: Array<{ name: "clock" | "progress" | "bubble" | "hint" | "card"; w: number; h: number }> = [];
  // yap-1107-one-pill-during-meeting — the clock is a segment of the one shell:
  // flush to the tab/room (no SLOT_GAP), the stack mid-take, the scene at rest.
  if (meeting !== "none") rows.push({ name: "clock", w: clockWidth(meeting, !meetingClockStacked(phase)), h: CLOCK_H });
  if (phase === PROGRESS_PHASE) rows.push({ name: "progress", w: PROGRESS_W, h: PROGRESS_H });
  const cardH = input.card ? Math.max(1, Math.ceil(input.card.h)) : 0;
  if (cardH) rows.push({ name: "card", w: CARD_W, h: cardH });
  rows.push({ name: "bubble", w: BUBBLE_W, h: BUBBLE_H });
  rows.push({ name: "hint", w: HINT_W, h: lines * HINT_LINE_H });
  const colH = rows.reduce((a, r) => a + r.h, 0) + SLOT_GAP * (rows.length - 1);
  let y = colH < env.h ? (env.h - colH) / 2 : 0;
  const placed: Partial<Record<(typeof rows)[number]["name"], Box>> = {};
  for (const r of rows) {
    const x = r.name === "clock" ? (left ? env.w : -r.w) : inX(r.w);
    placed[r.name] = { x, y, w: r.w, h: r.h };
    y += r.h + SLOT_GAP;
  }
  const all = [char0, ...Object.values(placed) as Box[]];
  const minX = Math.min(...all.map((b) => b.x));
  const minY = Math.min(...all.map((b) => b.y));
  const maxX = Math.max(...all.map(right));
  const maxY = Math.max(...all.map(bottom));
  // flush: the edge side carries no margin, the inner side PANEL_MARGIN
  const dx = (left ? 0 : PANEL_MARGIN) - minX, dy = PANEL_MARGIN - minY;
  const char = shift(char0, dx, dy);
  const at = (b: Box | undefined) => (b ? shift(b, dx, dy) : null);
  return {
    key, phase, meeting, dock, character, legacy: false,
    panel: { w: Math.ceil(maxX - minX + PANEL_MARGIN), h: Math.ceil(maxY - minY + 2 * PANEL_MARGIN) },
    char,
    bubble: at(placed.bubble)!,
    hint: at(placed.hint)!,
    clock: at(placed.clock),
    shell: placed.clock ? meetingShellBox(char, at(placed.clock)!, true) : null,
    sceneCharacter: false,
    progress: at(placed.progress),
    card: at(placed.card),
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
      // The scene IS the character in a resting meeting: one box, by design.
      if (l.sceneCharacter && named[i][0] === "character" && named[j][0] === "clock") continue;
      if (boxesOverlap(named[i][1], named[j][1])) out.push(`overlap ${named[i][0]}/${named[j][0]}`);
    }
  }
  if (l.clock && boxGap(l.clock, l.char) > CLOCK_REACH_PT) out.push(`clock ${boxGap(l.clock, l.char).toFixed(1)}pt from the character`);
  out.push(...meetingSurfaceProblems(l));
  const u = unionBox(named.map((n) => n[1]));
  // a side dock is flush to the screen edge: PANEL_MARGIN on the inner side only
  const marginsX = l.dock === "left" || l.dock === "right" ? 1 : 2;
  const wantW = Math.ceil(u.w + marginsX * PANEL_MARGIN), wantH = Math.ceil(u.h + 2 * PANEL_MARGIN);
  if (l.panel.w !== wantW || l.panel.h !== wantH) out.push(`panel ${l.panel.w}x${l.panel.h} != union+margin ${wantW}x${wantH}`);
  for (const [n, b] of named) {
    if (b.x < 0 || b.y < 0 || right(b) > l.panel.w + 0.5 || bottom(b) > l.panel.h + 0.5) out.push(`${n} outside the panel`);
  }
  return out;
}

/**
 * yap-1107-one-pill-during-meeting — the ONE meeting shell around the character
 * and the clock. At the bottom dock it is their union. On a side dock the
 * character is the tall room (or the 30 x 40 tab), so the shell is the clock
 * segment joined to the strip of the room level with it: the room is drawn over
 * that strip, so what shows is one surface flush to the edge, never a dark box
 * behind the bubble and hint column.
 */
export function meetingShellBox(char: Box, clock: Box, side: boolean): Box {
  if (!side) return unionBox([char, clock]);
  const y0 = Math.max(char.y, clock.y), y1 = Math.min(char.y + char.h, clock.y + clock.h);
  const strip: Box = y1 > y0 ? { x: char.x, y: y0, w: char.w, h: y1 - y0 } : char;
  return unionBox([strip, clock]);
}

/**
 * yap-1107-one-pill-during-meeting — Wilson 2026-11-07: "why is the recording
 * and the actual pill next to each other?" A meeting is ONE surface: the clock
 * touches the character (a segment of the shell, never a capsule a gap away),
 * the shell is exactly their union, and a resting meeting draws the scene as
 * the character. Shared by the tests and the pill matrix.
 */
export function meetingSurfaceProblems(l: PillLayout): string[] {
  if (l.legacy) return [];
  const out: string[] = [];
  if (l.meeting === "none") {
    if (l.clock || l.shell) out.push("a meeting surface with no meeting");
    if (l.sceneCharacter) out.push("the scene drawn as the character with no meeting");
    return out;
  }
  if (!l.clock || !l.shell) {
    out.push("a meeting without its one shell");
    return out;
  }
  const gap = boxGap(l.clock, l.char);
  if (gap > 0.5) out.push(`second surface: the clock is ${gap.toFixed(1)}pt off the character`);
  const u = meetingShellBox(l.char, l.clock, l.dock === "left" || l.dock === "right");
  if (Math.abs(u.x - l.shell.x) > 0.5 || Math.abs(u.y - l.shell.y) > 0.5 || Math.abs(u.w - l.shell.w) > 0.5 || Math.abs(u.h - l.shell.h) > 0.5) {
    out.push("the shell is not the one box around the character and the clock");
  }
  const side = l.dock === "left" || l.dock === "right";
  const restingRecording = !side && l.meeting === "recording" && RESTING_PHASES.has(l.phase);
  if (restingRecording !== l.sceneCharacter) out.push(`sceneCharacter=${l.sceneCharacter} for ${l.meeting}/${l.phase}`);
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
  const sh = l.shell ?? none;
  const p = l.progress ?? none;
  const k = l.card ?? none;
  return {
    "--panel-w": px(l.panel.w), "--panel-h": px(l.panel.h),
    "--char-x": px(l.char.x), "--char-y": px(l.char.y), "--char-w": px(l.char.w), "--char-h": px(l.char.h),
    "--bubble-x": px(l.bubble.x), "--bubble-y": px(l.bubble.y), "--bubble-w": px(l.bubble.w), "--bubble-h": px(l.bubble.h),
    "--hint-x": px(l.hint.x), "--hint-y": px(l.hint.y), "--hint-w": px(l.hint.w), "--hint-h": px(l.hint.h),
    "--clock-x": px(c.x), "--clock-y": px(c.y), "--clock-w": px(c.w), "--clock-h": px(c.h),
    "--shell-x": px(sh.x), "--shell-y": px(sh.y), "--shell-w": px(sh.w), "--shell-h": px(sh.h),
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
