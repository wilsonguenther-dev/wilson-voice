/**
 * yap-1106-resource-governor — the pill's "going easy" chip, pure half.
 *
 * Mirrors `resource_governor::GovernorView`. The backend decides the level and
 * writes the sentences; this file only decides WHETHER the pill shows a chip
 * and what its one action is, so the rule is testable without a window.
 */

/** The Tauri event a changed view arrives on. Mirrors `GOVERNOR_EVENT`. */
export const GOVERNOR_EVENT = "governor";

export type GovernorLevel = "full" | "light" | "minimal";

export interface GovernorView {
  level: GovernorLevel;
  reason: string | null;
  batteryPercent: number | null;
  onAc: boolean;
  lowPower: boolean;
  thermal: string;
  memory: string;
  adapterWatts: number | null;
  pauseBelowPercent: number;
  /** Settings: "Yap is going easy: 9% battery". */
  sentence: string | null;
  /** Pill: "Saving battery: notes after you plug in". */
  pillLabel: string | null;
  heavyWorkDeferred: boolean;
  waitingJobs: string[];
}

export const GOVERNOR_FULL: GovernorView = {
  level: "full",
  reason: null,
  batteryPercent: null,
  onAc: true,
  lowPower: false,
  thermal: "unknown",
  memory: "unknown",
  adapterWatts: null,
  pauseBelowPercent: 20,
  sentence: null,
  pillLabel: null,
  heavyWorkDeferred: false,
  waitingJobs: [],
};

export interface GovernorChip {
  label: string;
  /** "minimal" draws the stronger dot. */
  tone: "light" | "minimal";
  /** Offer "Run anyway" — only when something is actually waiting. */
  runAnyway: boolean;
}

/** `null` at full (no chip), otherwise the chip the pill draws. */
export function governorChip(view: GovernorView | null | undefined): GovernorChip | null {
  if (!view || view.level === "full" || !view.pillLabel) return null;
  return {
    label: view.pillLabel,
    tone: view.level === "minimal" ? "minimal" : "light",
    runAnyway: view.heavyWorkDeferred && view.waitingJobs.length > 0,
  };
}

/** Settings › Advanced: the sentence, or the quiet default. */
export function governorSentence(view: GovernorView | null | undefined): string {
  return view?.sentence ?? "Yap is running at full speed.";
}

/** 1..=99, whole; anything else keeps `current`. Mirrors `clamp_pause_below`. */
export function pauseBelowFromInput(raw: string, current: number): number {
  const n = Number.parseInt(raw, 10);
  if (!Number.isFinite(n)) return current;
  return Math.min(99, Math.max(1, n));
}
