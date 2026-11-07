/**
 * stuckScene — the pill keeps its whole scene while the engine is stuck
 * (yap-1106-stuck-pill-keeps-its-scene).
 *
 * Observed 2026-11-06: during a take stuck on "working on it…" the pill fell
 * to bare level bars on a dark blob — the sky, Yappy and the shell were gone.
 * The engine was saturating the GPU, and the pill's canvases (the cached sky
 * surface, the scene buffer, the visible canvas) lost what they had painted
 * while the 60 fps loop kept competing for the same GPU.
 *
 * The rule this module encodes: a health state NEVER changes what the pill is
 * made of. `scene` is always "full" (shell + sky + Yappy). While the engine is
 * stuck or restarting the pill
 *   1. says so, in plain words (`label`, the engine-owner's chip words),
 *   2. draws at a calm ~10 fps instead of 60, so it stops fighting the engine
 *      for the GPU (`sceneFramePlan`),
 *   3. repaints the WHOLE scene from scratch — sky cache included — on entry,
 *      on recovery, when a lost canvas comes back, when the pill is shown
 *      again, and every `STUCK_REPAINT_MS` while it lasts (`repaint` bumps).
 *
 * Pure: no DOM except `watchCanvasLoss`, which only wires listeners.
 */
import { engineHealthLabel, type EngineHealth } from "./engineHealth";
import type { FramePlan } from "./live";

/** The frame interval while the engine is strained: ~10 fps, never 60. */
export const STUCK_FRAME_MS = 100;
/** While strained, the whole scene is rebuilt at least this often. */
export const STUCK_REPAINT_MS = 2000;

/** Why the whole scene was repainted from scratch (the log line's `reason`). */
export type RepaintReason = "strained" | "recovered" | "context_restored" | "shown" | "refresh";

export interface SceneState {
  health: EngineHealth;
  /** Always "full": a health state never drops the shell, the sky or Yappy. */
  scene: "full";
  /** The engine is stuck or restarting. */
  strained: boolean;
  /** The plain-words state the pill shows while strained; null otherwise. */
  label: string | null;
  /** A canvas reported its backing store lost and has not come back yet. */
  contextLost: boolean;
  /** Bumped every time the whole scene must be repainted from scratch. */
  repaint: number;
  /** Why `repaint` last moved; null before the first repaint. */
  reason: RepaintReason | null;
  /** When the last full repaint was asked for (performance.now() ms). */
  lastRepaintAt: number;
}

export type SceneEvent =
  | { type: "health"; health: EngineHealth; at: number }
  | { type: "context_lost"; at: number }
  | { type: "context_restored"; at: number }
  | { type: "shown"; at: number }
  | { type: "tick"; at: number };

export const SCENE_IDLE: SceneState = {
  health: "idle",
  scene: "full",
  strained: false,
  label: null,
  contextLost: false,
  repaint: 0,
  reason: null,
  lastRepaintAt: 0,
};

/** Stuck and restarting are the two states the pill must ride out. */
export function isStrained(health: EngineHealth): boolean {
  return health === "stuck" || health === "restarting";
}

function repaintNow(s: SceneState, reason: RepaintReason, at: number): SceneState {
  return { ...s, repaint: s.repaint + 1, reason, lastRepaintAt: at };
}

/**
 * The one reducer. Returns the SAME object when nothing changed, so a caller
 * can compare `repaint` (or identity) to know whether to act.
 */
export function reduceStuckScene(s: SceneState, e: SceneEvent): SceneState {
  switch (e.type) {
    case "health": {
      if (e.health === s.health) return s;
      const strained = isStrained(e.health);
      const next: SceneState = {
        ...s,
        health: e.health,
        scene: "full",
        strained,
        label: strained ? engineHealthLabel(e.health) : null,
      };
      if (strained && !s.strained) return repaintNow(next, "strained", e.at);
      if (!strained && s.strained) return repaintNow(next, "recovered", e.at);
      // stuck <-> restarting: the words change, the scene is rebuilt again.
      if (strained) return repaintNow(next, "strained", e.at);
      return next;
    }
    case "context_lost":
      return s.contextLost ? s : { ...s, contextLost: true };
    case "context_restored":
      return repaintNow({ ...s, contextLost: false }, "context_restored", e.at);
    case "shown":
      // A plain show repaints nothing new; only a strained or lost scene is rebuilt.
      return s.strained || s.contextLost ? repaintNow({ ...s, contextLost: false }, "shown", e.at) : s;
    case "tick":
      return s.strained && e.at - s.lastRepaintAt >= STUCK_REPAINT_MS ? repaintNow(s, "refresh", e.at) : s;
  }
}

/**
 * The frame plan while strained: a live rAF is throttled to STUCK_FRAME_MS
 * (never faster), and every other mode is left exactly as `framePlan` chose.
 */
export function sceneFramePlan(plan: FramePlan, strained: boolean): FramePlan {
  if (!strained || plan.mode !== "raf") return plan;
  return { ...plan, intervalMs: Math.max(plan.intervalMs, STUCK_FRAME_MS) };
}

/**
 * Wire `contextlost` / `contextrestored` on every canvas the scene paints
 * through. Returns the unsubscribe.
 */
export function watchCanvasLoss(
  canvases: readonly EventTarget[],
  onLost: () => void,
  onRestored: () => void,
): () => void {
  for (const c of canvases) {
    c.addEventListener("contextlost", onLost);
    c.addEventListener("contextrestored", onRestored);
  }
  return () => {
    for (const c of canvases) {
      c.removeEventListener("contextlost", onLost);
      c.removeEventListener("contextrestored", onRestored);
    }
  };
}
