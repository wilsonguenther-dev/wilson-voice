/**
 * yap-1107-parity-pill — the approved pill's RIGHT SEGMENT, inside the shell.
 *
 * design-2026-11-06/pill.html builds every pill state as ONE dark shell
 * (.ypill: padding 4, gap 4) holding the sky window (.win) and, for every state
 * that says something, a `.seg.r` (gap 6, padding 0 6, 500 12px) with the
 * words, the keycap, the stop square or the light call-to-action button. The
 * bounce on this item: the built pill drew that text as a separate strip
 * OUTSIDE the shell. This module is the pure model of that segment and of the
 * shell sizes the page uses when a segment is present; YappyPill renders it
 * inside .yappy-capsule-box and sizes the painted shell around both.
 *
 * Numbers are the page's own (pill.html lines 381-476, pill-kit.js `pill`):
 *   status states  h 32, window 44 x 24 r 12   (Warming up, No words heard, ...)
 *   open + segment h 52, window 196 x 44        (hands-free, writing it up, ...)
 *   open + desk    h 52, window 208 x 44        (Long take: "an ESSAY, live?!")
 *   open, no seg   h 52, window 220 x 44        (Holding fn: OPEN_CAPSULE)
 * Copy is the page's own, word for word. Pure: no DOM, no React.
 */

/** pill.html .ypill padding and gap (characterFit SHELL_PAD, tokens --pill-shell-pad/-gap). */
export const SEG_SHELL_PAD = 4;
export const SEG_SHELL_GAP = 4;
/** pill.html .ypill .seg: gap 6, padding 0 6 (both sides). */
export const SEG_GAP = 6;
export const SEG_PAD_X = 6;
/** pill.html .ypill font: 500 12px; .stack .t: 500 11px mono. */
export const SEG_FONT_PX = 12;
export const SEG_MONO_PX = 11;
/** .sbtn / .cbtn are 24 tall; .sbtn is 24 square; icons are 14. */
export const SEG_BUTTON_PX = 24;
export const SEG_ICON_PX = 14;
/** .bar: 44 x 3. */
export const SEG_BAR = { w: 44, h: 3 } as const;

/** The capsule (shell minus the segment) the page draws for each class. */
export const STATUS_CAPSULE = { w: 44 + 2 * SEG_SHELL_PAD, h: 24 + 2 * SEG_SHELL_PAD } as const; // 52 x 32
export const OPEN_SEG_CAPSULE = { w: 196 + 2 * SEG_SHELL_PAD, h: 44 + 2 * SEG_SHELL_PAD } as const; // 204 x 52
export const OPEN_DESK_SEG_CAPSULE = { w: 208 + 2 * SEG_SHELL_PAD, h: 44 + 2 * SEG_SHELL_PAD } as const; // 216 x 52

export type SegIcon = "lock" | "paste" | "check" | "mute" | "batt";

/** One piece of the right segment, in reading order. */
export type SegItem =
  | { kind: "icon"; icon: SegIcon }
  | { kind: "text"; text: string; sub?: string }
  | { kind: "kbd"; text: string }
  | { kind: "stop" }
  | { kind: "cta"; text: string }
  | { kind: "bar"; fraction: number }
  /** .stack: a mono line over a bar (Writing it up "3 of 9"). */
  | { kind: "stack"; top: string; fraction: number };

export interface SegInput {
  /** The float's phase (YappyPill's `gate`): LivePhase plus listening-locked / blind. */
  phase: string;
  /**
   * yap-1107-parity-companions — Yappy visiting for a milestone (companions.html
   * s09, "Yappy visits for your 100th take"): the finished take's segment reads
   * "✓ 100th take" instead of the word count (pill/visitorInWindow.ts
   * visitorSegment). Only the done state carries it.
   */
  milestone?: string | null;
  /** Words in the take that just pasted (done). */
  words?: number;
  /** Decode progress (transcribing): chunk of `of`. */
  progress?: { chunk: number; of: number } | null;
  /** The engine warm-up fraction 0..1 (model-loading). */
  warm?: number | null;
  /** The user's stop key, shown as the keycap (pill.html shows "fn"). */
  stopKey?: string;
  /** The desk prop is out (Long take): the page's wider 208 window. */
  desk?: boolean;
  /**
   * pill.html row 20 "Saving battery": below 20 percent or Low Power Mode.
   * A resting pill says it with the battery icon; `lowPowerLabel` false =
   * the label already showed its 3 seconds ("then only the leaf stays").
   */
  lowPower?: boolean;
  lowPowerLabel?: boolean;
  /**
   * pill.html #wispr "Fix-up notices": the done state carries Backtrack
   * ("Fixed 2 words" + Show), never a separate toast. Words the take fixed.
   */
  fixedWords?: number;
}

/** pill.html row 20 copy, and how long the label holds before only the icon stays. */
export const LOW_POWER_LABEL = "Saving battery";
export const LOW_POWER_LABEL_MS = 3000;
/** pill.html #wispr Fix-up notices button. */
export const FIXED_SHOW_CTA = "Show";

/** "Fixed 2 words" / "Fixed 1 word" — the page's own wording. */
export function fixedWordsLine(n: number): string {
  const k = Math.max(0, Math.round(n));
  return `Fixed ${k} ${k === 1 ? "word" : "words"}`;
}

/** Which capsule class a phase draws at once it carries a segment. */
export type SegClass = "none" | "status" | "open";

const clamp01 = (n: number) => (Number.isFinite(n) ? Math.max(0, Math.min(1, n)) : 0);

/**
 * The page's right segment for a phase. Empty = the shell is just the window
 * (Resting, Asleep, Holding fn, Thinking). The kbd text defaults to the page's
 * "fn"; a bound chord shows instead, never an empty keycap.
 */
export function segmentFor(i: SegInput): SegItem[] {
  const key = (i.stopKey ?? "").trim() || "fn";
  switch (i.phase) {
    case "listening-locked":
      return [{ kind: "icon", icon: "lock" }, { kind: "kbd", text: key }, { kind: "stop" }];
    case "model-loading":
      return [{ kind: "text", text: "Warming up" }, { kind: "bar", fraction: clamp01(i.warm ?? 0.4) }];
    case "transcribing": {
      const p = i.progress;
      if (p && p.of > 0) return [{ kind: "stack", top: `${Math.min(p.chunk, p.of)} of ${p.of}`, fraction: clamp01(p.chunk / p.of) }];
      return [{ kind: "text", text: "Writing it up" }];
    }
    case "polishing":
      return [{ kind: "text", text: "Polishing" }];
    case "pasting":
      return [{ kind: "icon", icon: "paste" }, { kind: "text", text: "Pasting" }];
    case "idle":
    case "sleepy":
      if (!i.lowPower) return [];
      return i.lowPowerLabel === false ? [{ kind: "icon", icon: "batt" }] : [{ kind: "icon", icon: "batt" }, { kind: "text", text: LOW_POWER_LABEL }];
    case "done": {
      const milestone = (i.milestone ?? "").trim();
      if (milestone) return [{ kind: "icon", icon: "check" }, { kind: "text", text: milestone }];
      const fixed = Math.max(0, Math.round(i.fixedWords ?? 0));
      if (fixed > 0) return [{ kind: "icon", icon: "check" }, { kind: "text", text: fixedWordsLine(fixed) }, { kind: "cta", text: FIXED_SHOW_CTA }];
      const n = Math.max(0, Math.round(i.words ?? 0));
      return n > 0 ? [{ kind: "icon", icon: "check" }, { kind: "text", text: String(n), sub: n === 1 ? "word" : "words" }] : [];
    }
    case "empty":
      return [{ kind: "text", text: "No words heard" }];
    case "cancelled":
      return [{ kind: "text", text: "Cancelled" }];
    case "error":
      return [{ kind: "text", text: "Didn’t go through" }, { kind: "cta", text: "Retry" }];
    case "blocked":
      return [{ kind: "icon", icon: "mute" }, { kind: "cta", text: "Allow mic" }];
    case "waiting":
      return [{ kind: "text", text: "Answer the macOS prompt" }];
    case "gated":
      return [{ kind: "text", text: "Trial ended" }, { kind: "cta", text: "License" }];
    case "blind":
      return [{ kind: "text", text: "fn is off here", sub: "Click to dictate" }];
    default:
      return [];
  }
}

/** Phases the page draws OPEN (52 tall) with a segment; the rest are the 32-tall status pill. */
const OPEN_SEG_PHASES = new Set(["listening-locked", "transcribing", "polishing", "pasting", "done"]);

export function segClassFor(phase: string, items: readonly SegItem[]): SegClass {
  if (!items.length) return "none";
  return OPEN_SEG_PHASES.has(phase) ? "open" : "status";
}

/** The capsule (window + shell padding, no segment) the page draws for a class. */
export function segCapsule(cls: SegClass, desk = false): { w: number; h: number } | null {
  if (cls === "status") return { ...STATUS_CAPSULE };
  if (cls === "open") return desk ? { ...OPEN_DESK_SEG_CAPSULE } : { ...OPEN_SEG_CAPSULE };
  return null;
}

/**
 * The whole painted shell: the capsule plus the gap and the measured segment,
 * centred on the anchor and kept inside the panel (a `margin` from each edge).
 * Returns the shell box and where the capsule (the window's shell) starts.
 */
export function shellBounds(
  anchorX: number,
  capsule: { w: number; h: number },
  segW: number,
  panelW: number,
  margin = 2,
): { x: number; w: number; capX: number } {
  const seg = Number.isFinite(segW) && segW > 0 ? segW : 0;
  // pad + window + gap + segment + pad = capsule.w (window + 2 pad) + gap + segment
  const w = capsule.w + (seg ? SEG_SHELL_GAP + seg : 0);
  let x = anchorX - w / 2;
  if (panelW > 0) x = Math.max(margin, Math.min(x, panelW - margin - w));
  return { x, w, capX: x };
}

/** One telemetry line when the shell's segment class changes (ctx3 rules telemetry). */
export function segEvent(phase: string, cls: SegClass, items: readonly SegItem[], shellW: number): string {
  return `pill event=shell-seg phase=${phase} class=${cls} items=${items.map((s) => s.kind).join("+") || "-"} shell_w=${Math.round(shellW)}`;
}
