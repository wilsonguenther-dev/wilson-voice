/**
 * WF-20 — the per-app style chip: the pure half.
 *
 * While a take is listening the pill shows which of Yap's six app-aware modes
 * the backend picked for the frontmost app ("Slack · casual"). A click asks the
 * backend (`pill_cycle_style`, style_chip.rs) to move THAT app to the next mode
 * for 15 minutes; the backend owns the clock and the per-app scoping, so this
 * module only decides whether the chip is on screen and what it says.
 *
 * The chip is hidden when the user has app-aware modes off (a fixed mode in
 * Formatting): the backend sends `visible: false`, and `chipShows` honours it.
 */
import type { LivePhase } from "./live";

/** The Tauri event carrying a `StyleChip` (style_chip.rs `STYLE_CHIP_EVENT`). */
export const STYLE_CHIP_EVENT = "style_chip";
/** The command a click invokes (lib.rs `pill_cycle_style`). */
export const CYCLE_STYLE_COMMAND = "pill_cycle_style";
/** style_chip.rs `OVERRIDE_TTL_MS` — how long a pick holds for its app. */
export const OVERRIDE_TTL_MS = 15 * 60 * 1000;

/** style_chip.rs `StyleChip`, camelCase over the wire. */
export interface StyleChip {
  visible: boolean;
  app: string;
  mode: string;
  label: string;
  overridden: boolean;
  expiresInMs: number | null;
}

export const HIDDEN_CHIP: StyleChip = {
  visible: false,
  app: "",
  mode: "",
  label: "",
  overridden: false,
  expiresInMs: null,
};

/** Is the chip on screen? Only while listening, and only when the backend says
 *  app-aware modes are on and an app was read. A hands-free (locked) take keeps
 *  the hint row for its way-out line, so the chip yields it there. */
export function chipShows(chip: StyleChip | null, phase: LivePhase): boolean {
  return phase === "listening" && chip !== null && chip.visible && chip.label.length > 0;
}

/** Whole minutes left on a pick, rounded up so "1 min" never reads "0 min". */
export function minutesLeft(ms: number | null): number | null {
  if (ms === null || ms <= 0) return null;
  return Math.ceil(ms / 60000);
}

/** The chip's accessible name and tooltip: what it is, and what a click does. */
export function chipAccessibleName(chip: StyleChip): string {
  const left = chip.overridden ? minutesLeft(chip.expiresInMs) : null;
  const now = `Style for ${chip.app}: ${chip.mode}`;
  const tail = left === null
    ? "Click to change it for this app for 15 minutes."
    : `Changed for this app, ${left} min left. Click to change again.`;
  return `${now}. ${tail}`;
}

/** The payload is untrusted shape-wise (an older backend, a forced matrix):
 *  anything that is not a well-formed chip reads as hidden. */
export function parseChip(raw: unknown): StyleChip {
  if (!raw || typeof raw !== "object") return HIDDEN_CHIP;
  const r = raw as Record<string, unknown>;
  if (typeof r.visible !== "boolean" || typeof r.label !== "string") return HIDDEN_CHIP;
  return {
    visible: r.visible,
    app: typeof r.app === "string" ? r.app : "",
    mode: typeof r.mode === "string" ? r.mode : "",
    label: r.label,
    overridden: r.overridden === true,
    expiresInMs: typeof r.expiresInMs === "number" ? r.expiresInMs : null,
  };
}

/** The sample chip the pill matrix draws (`float.html?phase=listening&styleChip=1`). */
export const SAMPLE_STYLE_CHIP: StyleChip = {
  visible: true,
  app: "Slack",
  mode: "casual",
  label: "Slack · casual",
  overridden: false,
  expiresInMs: null,
};

/** The forced chip for a matrix scene, or null when the URL asks for none. */
export function forcedStyleChip(search: string): StyleChip | null {
  return new URLSearchParams(search).get("styleChip") === "1" ? SAMPLE_STYLE_CHIP : null;
}
