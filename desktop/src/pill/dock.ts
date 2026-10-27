/**
 * Y5-I — the docked bar, with VERTICAL as the base.
 *
 * Wispr's vertical bar is not a rotated horizontal capsule: the column is the
 * CSS base and horizontal is the override, sized by orientation-neutral tokens
 * (`--flow-bar-length` along the bar, `--flow-bar-thickness` across it). Their
 * first attempt rotated the capsule and recorded a "30x6-always-horizontal
 * collapse bug"; Yap's YV53/65 approach (slide the horizontal capsule to the
 * screen edge) was the same dead end, and this module replaces it.
 *
 * This is the SHELL's contract, not a character's. `dock-geometry.json` gives
 * the window content box for every phase x dock, and every character renders
 * inside the box it is handed. There is no style dimension anywhere in it.
 *
 * Two attributes on the shell's `.bar` root carry the override, and float.css
 * reads only them:
 *   data-bar-position  bottom | left | right   (bottom supplies the row)
 *   data-bar-orientation  strip | banner       (a banner is a row in EVERY dock:
 *                                               fixed-width labels crush in a
 *                                               30 px column)
 * and the two tokens carry the size. Nothing is ever rotated with a transform.
 */
import GEOMETRY from "./dock-geometry.json";
import type { LivePhase } from "./live";

export type BarDock = "bottom" | "left" | "right";
/** A take phase, or a meeting on screen while no dictation is. */
export type BarPhase = LivePhase | "meeting-on-screen";
/** `strip` rotates into the side-dock column; `banner` stays horizontal. */
export type BarOrientation = "strip" | "banner";
export interface BarBox {
  width: number;
  height: number;
}
interface BarRow {
  wispr: string | null;
  orientation: BarOrientation;
  yap_only?: boolean;
  why: string;
  bottom: BarBox;
  left: BarBox;
  right: BarBox;
}

const ROWS = GEOMETRY.phases as Record<BarPhase, BarRow>;

/** The side-dock column's width: no strip phase may be thicker than this. */
export const STRIP_PX: number = GEOMETRY.strip_px;
export const BAR_DOCKS: readonly BarDock[] = GEOMETRY.docks as BarDock[];
/** Every row the table carries, in its committed order. */
export const BAR_PHASES: readonly BarPhase[] = Object.keys(ROWS) as BarPhase[];

/** Unknown or missing values read as the default bottom dock. */
export function barDock(d: string | null | undefined): BarDock {
  return d === "left" || d === "right" ? d : "bottom";
}

export const barOrientation = (phase: BarPhase): BarOrientation => ROWS[phase].orientation;

/** The box the shell hands the character for `phase` at `dock`, in CSS px. */
export function barBox(phase: BarPhase, dock: BarDock): BarBox {
  return ROWS[phase][dock];
}

/**
 * The bar's main axis. A column only where BOTH agree: the dock is a side dock
 * and the phase is one that rotates into the strip. Everything else is a row.
 */
export function barAxis(phase: BarPhase, dock: BarDock): "column" | "row" {
  return dock !== "bottom" && barOrientation(phase) === "strip" ? "column" : "row";
}

/**
 * The axis the waveform's bars animate along. In a column the bars are 2 px
 * tall and grow along X; in a row they grow along Y. The waveform inverts with
 * the bar, never independently of it.
 */
export function waveAxis(phase: BarPhase, dock: BarDock): "x" | "y" {
  return barAxis(phase, dock) === "column" ? "x" : "y";
}

/**
 * The orientation-neutral tokens for one box: length runs along the bar's main
 * axis, thickness across it. These are the ONLY numbers float.css sizes the bar
 * with, so the table cannot drift from what is drawn.
 */
export function barTokens(phase: BarPhase, dock: BarDock): { length: number; thickness: number } {
  const b = barBox(phase, dock);
  return barAxis(phase, dock) === "column"
    ? { length: b.height, thickness: b.width }
    : { length: b.width, thickness: b.height };
}

/**
 * The shell's own ranking: a meeting badge takes the bar only while no take is
 * on screen (idle / sleepy). Any take phase outranks it, exactly as
 * `meetingAfterView` already ranks the post-stop badge below dictation.
 */
export function shellBarPhase(phase: LivePhase, meetingOnScreen: boolean): BarPhase {
  return meetingOnScreen && (phase === "idle" || phase === "sleepy") ? "meeting-on-screen" : phase;
}

/** The attributes and style the shell's `.bar` root carries for one box. */
export function barRootProps(phase: BarPhase, dock: BarDock) {
  const t = barTokens(phase, dock);
  return {
    "data-bar-position": dock,
    "data-bar-orientation": barOrientation(phase),
    "data-bar-phase": phase,
    "data-wave-axis": waveAxis(phase, dock),
    style: {
      ["--flow-bar-length" as string]: `${t.length}px`,
      ["--flow-bar-thickness" as string]: `${t.thickness}px`,
    },
  };
}
