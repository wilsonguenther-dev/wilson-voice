/**
 * yap24-UI3 — the pill matrix's handle on the float window. DEV-ONLY.
 *
 * `scripts/pill-matrix.mjs` renders float.html once per phase x dock x
 * character in headless Chrome. A real phase only happens when the backend
 * emits the right events in the right order, which a browser cannot do, so
 * the matrix forces one through the query string instead:
 *
 *     float.html?phase=transcribing&pill=yappy&dock=left
 *
 * `phase` takes the TypeScript variant (`model-loading`) or the product id
 * (`model_loading`, PHASE_ID). `pill` and `dock` are optional and fall back to
 * whatever settings say.
 *
 * WHY THIS NEVER SHIPS: float-main only calls into this module behind
 * `import.meta.env.DEV || __YAP_PILL_MATRIX__`. Both are compile-time
 * constants that are `false` in `npm run build` (vite.config.ts defines
 * `__YAP_PILL_MATRIX__` from `YAP_PILL_MATRIX=1`, which only the matrix sets),
 * so rollup drops the branch and this module with it — a released float
 * window has no query parameter that can move its phase.
 *
 * Pure: no DOM, no React, no timers, so the parse is unit-tested on its own.
 */
import {
  ALL_LIVE_PHASES,
  PHASE_ID,
  SAMPLE_BLIND,
  phaseVisual,
  type BlindState,
  type LivePhase,
  type PhaseVisual,
  type TranscribeProgress,
} from "./live";

export const PILL_STYLES = ["classic", "yappy"] as const;
export type PillStyle = (typeof PILL_STYLES)[number];
export const PILL_DOCKS = ["bottom", "left", "right"] as const;
export type PillDock = (typeof PILL_DOCKS)[number];

/**
 * `transcribing` is the one phase whose capsule draws data the phase alone
 * does not carry: a determinate fill and its numeral (Y3-C). Without a sample
 * the matrix would photograph a transcribing pill with no progress — a state
 * the app never shows, since that phase is only entered on a progress event.
 */
export const SAMPLE_PROGRESS: TranscribeProgress = { chunk: 3, of: 7, wordsSoFar: 58 };

export interface ForcedPill {
  phase: LivePhase;
  style: PillStyle | null;
  dock: PillDock | null;
  progress: TranscribeProgress | null;
  /**
   * yap24-PILL3 — `blind` is the other phase whose line needs data the phase
   * does not carry (the holder and the chord). Without a sample the matrix
   * would photograph the tone fallback, a line the app only says when the
   * status names neither.
   */
  blind: BlindState | null;
}

/** A phase by its variant name or its product id; null for anything else. */
export function resolvePhase(raw: string | null | undefined): LivePhase | null {
  if (!raw) return null;
  const want = raw.trim();
  return ALL_LIVE_PHASES.find((p) => p === want || PHASE_ID[p] === want) ?? null;
}

/**
 * The forced pill named by a query string, or null when it names none.
 * An UNKNOWN phase is null too, never a guess: the matrix asserts that
 * `<html data-phase>` carries the id it asked for, so a typo surfaces there as
 * "phase not applied" instead of a convincing photo of `idle`.
 */
export function forcedPill(search: string): ForcedPill | null {
  const q = new URLSearchParams(search);
  const phase = resolvePhase(q.get("phase"));
  if (!phase) return null;
  const pill = q.get("pill");
  const dock = q.get("dock");
  return {
    phase,
    style: (PILL_STYLES as readonly string[]).includes(pill ?? "") ? (pill as PillStyle) : null,
    dock: (PILL_DOCKS as readonly string[]).includes(dock ?? "") ? (dock as PillDock) : null,
    progress: phase === "transcribing" ? SAMPLE_PROGRESS : null,
    blind: phase === "blind" ? SAMPLE_BLIND : null,
  };
}

/** One row of the matrix's phase list. */
export interface MatrixPhase {
  phase: LivePhase;
  id: string;
  tone: PhaseVisual["tone"];
  label: string;
}

/**
 * The phase list the matrix walks, read off the pill's OWN table: every
 * declared phase, through `phaseVisual`. The harness never keeps a copy, so a
 * phase a later item adds to the union (and therefore to `phaseVisual`, which
 * is exhaustive) is photographed without anyone editing the script.
 */
export function matrixPhases(): MatrixPhase[] {
  return ALL_LIVE_PHASES.map((phase) => {
    const v = phaseVisual(phase);
    return { phase, id: PHASE_ID[phase], tone: v.tone, label: v.label };
  });
}
