/**
 * Y4-H — what the formatting controls MEAN, fetched from the backend instead of
 * retyped here.
 *
 * The settings screen used to carry its own prose for each `cleanup_level`. It
 * drifted, exactly the way hand-copied copy always does: long after
 * `polish::polish_llm` shipped, the High button still read "the AI polish pass
 * isn't wired up yet". The backend builds these strings from the same
 * `CleanupLevel::runs_*` predicates `run_cleanup` branches on
 * (`src-tauri/src/formatting_options.rs`), so the screen cannot describe a stage
 * the pipeline does not run.
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/** One row of the "how much should Yap clean up?" picker. */
export type CleanupLevelOption = {
  /** The stored `cleanup_level` value — the KEY, which never changes. */
  id: string;
  label: string;
  /** One line, derived from the pipeline's predicates. */
  description: string;
  /** The stage tags this level runs, in pipeline order. */
  stages: string[];
  /** True only where the level asks for the local model. */
  needsPolishModel: boolean;
};

/** One row of the "how long may the model think?" picker. */
export type PolishSpeedOption = {
  id: string;
  label: string;
  description: string;
  /** Written to `polish_deadline_ms`; always inside the backend's bounds. */
  deadlineMs: number;
};

export type FormattingOptions = {
  cleanupLevels: CleanupLevelOption[];
  polishSpeeds: PolishSpeedOption[];
};

/** Nothing to render until the backend answers — never invented copy. */
export const NO_FORMATTING_OPTIONS: FormattingOptions = {
  cleanupLevels: [],
  polishSpeeds: [],
};

/**
 * Which speed button is selected for a stored deadline: the NEAREST one, so a
 * value written by an older build (or hand-edited into settings.json) still
 * lights a button instead of leaving all three dark.
 */
export function selectedSpeed(
  deadlineMs: number | undefined,
  speeds: PolishSpeedOption[],
): string | undefined {
  if (speeds.length === 0) return undefined;
  const ms = typeof deadlineMs === "number" ? deadlineMs : NaN;
  if (!Number.isFinite(ms)) {
    return speeds.find((s) => s.id === "balanced")?.id ?? speeds[0].id;
  }
  let best = speeds[0];
  for (const s of speeds) {
    if (Math.abs(s.deadlineMs - ms) < Math.abs(best.deadlineMs - ms)) best = s;
  }
  return best.id;
}

/** Does the chosen level ask for the local model? */
export function levelNeedsPolishModel(
  levelId: string | undefined,
  levels: CleanupLevelOption[],
): boolean {
  return levels.find((l) => l.id === levelId)?.needsPolishModel ?? false;
}

/** Fetch the options once per mount. A failure leaves the lists empty. */
export function useFormattingOptions(): FormattingOptions {
  const [options, setOptions] = useState<FormattingOptions>(
    NO_FORMATTING_OPTIONS,
  );
  useEffect(() => {
    let alive = true;
    invoke<FormattingOptions>("formatting_options")
      .then((o) => {
        if (alive) setOptions(o);
      })
      .catch(() => {
        // Leave the lists empty rather than show copy we cannot back up.
      });
    return () => {
      alive = false;
    };
  }, []);
  return options;
}
