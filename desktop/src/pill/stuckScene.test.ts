/**
 * stuckScene.test — yap-1106-stuck-pill-keeps-its-scene: a stuck or
 * restarting engine never strips the pill to bare level bars; it keeps the
 * full scene, says the engine's state, calms its frame rate and rebuilds the
 * scene from scratch.
 */
import { describe, expect, it, vi } from "vitest";
import { ENGINE_HEALTH_LABEL, ENGINE_HEALTH_STATES, engineHealthTakesStrip } from "./engineHealth";
import { framePlan } from "./live";
import {
  SCENE_IDLE,
  STUCK_FRAME_MS,
  STUCK_REPAINT_MS,
  isStrained,
  reduceStuckScene,
  sceneFramePlan,
  watchCanvasLoss,
  type SceneEvent,
  type SceneState,
} from "./stuckScene";

const run = (events: SceneEvent[], from: SceneState = SCENE_IDLE): SceneState =>
  events.reduce(reduceStuckScene, from);

describe("reduceStuckScene", () => {
  it("keeps the full scene in every engine health state", () => {
    for (const health of ENGINE_HEALTH_STATES) {
      const s = run([{ type: "health", health, at: 10 }]);
      expect(s.scene).toBe("full");
      expect(s.health).toBe(health);
    }
  });

  it("stuck and restarting carry the explicit state label; quiet states carry none", () => {
    expect(run([{ type: "health", health: "stuck", at: 1 }]).label).toBe(ENGINE_HEALTH_LABEL.stuck);
    expect(run([{ type: "health", health: "restarting", at: 1 }]).label).toBe(ENGINE_HEALTH_LABEL.restarting);
    for (const health of ["idle", "loading", "ready", "busy"] as const) {
      const s = run([{ type: "health", health, at: 1 }]);
      expect(s.label).toBeNull();
      expect(s.strained).toBe(false);
    }
    expect(isStrained("stuck")).toBe(true);
    expect(isStrained("restarting")).toBe(true);
    expect(isStrained("busy")).toBe(false);
  });

  it("repaints the whole scene on entering stuck, on stuck -> restarting, and on recovery", () => {
    const stuck = run([{ type: "health", health: "busy", at: 0 }, { type: "health", health: "stuck", at: 5 }]);
    expect(stuck.repaint).toBe(1);
    expect(stuck.reason).toBe("strained");
    const restarting = reduceStuckScene(stuck, { type: "health", health: "restarting", at: 6 });
    expect(restarting.repaint).toBe(2);
    const ready = reduceStuckScene(restarting, { type: "health", health: "ready", at: 7 });
    expect(ready.repaint).toBe(3);
    expect(ready.reason).toBe("recovered");
    expect(ready.strained).toBe(false);
    expect(ready.label).toBeNull();
    expect(ready.scene).toBe("full");
  });

  it("returns the same object for a repeated health state (no repaint storm)", () => {
    const s = run([{ type: "health", health: "stuck", at: 1 }]);
    expect(reduceStuckScene(s, { type: "health", health: "stuck", at: 2 })).toBe(s);
    const quiet = run([{ type: "health", health: "busy", at: 1 }]);
    expect(quiet.repaint).toBe(0);
  });

  it("refreshes the scene every STUCK_REPAINT_MS while strained, never while healthy", () => {
    const s = run([{ type: "health", health: "stuck", at: 1000 }]);
    expect(reduceStuckScene(s, { type: "tick", at: 1000 + STUCK_REPAINT_MS - 1 })).toBe(s);
    const r = reduceStuckScene(s, { type: "tick", at: 1000 + STUCK_REPAINT_MS });
    expect(r.repaint).toBe(s.repaint + 1);
    expect(r.reason).toBe("refresh");
    const healthy = run([{ type: "health", health: "busy", at: 0 }]);
    expect(reduceStuckScene(healthy, { type: "tick", at: 999_999 })).toBe(healthy);
  });

  it("a lost canvas is rebuilt when it comes back, and a show rebuilds only a strained or lost scene", () => {
    const lost = reduceStuckScene(SCENE_IDLE, { type: "context_lost", at: 1 });
    expect(lost.contextLost).toBe(true);
    expect(reduceStuckScene(lost, { type: "context_lost", at: 2 })).toBe(lost);
    const back = reduceStuckScene(lost, { type: "context_restored", at: 3 });
    expect(back.contextLost).toBe(false);
    expect(back.repaint).toBe(1);
    expect(back.reason).toBe("context_restored");
    expect(reduceStuckScene(SCENE_IDLE, { type: "shown", at: 4 })).toBe(SCENE_IDLE);
    const shownLost = reduceStuckScene(lost, { type: "shown", at: 4 });
    expect(shownLost.repaint).toBe(1);
    expect(shownLost.reason).toBe("shown");
    const shownStuck = reduceStuckScene(run([{ type: "health", health: "stuck", at: 0 }]), { type: "shown", at: 9 });
    expect(shownStuck.repaint).toBe(2);
  });
});

describe("sceneFramePlan", () => {
  it("calms a live 60 fps loop to ~10 fps while strained and leaves everything else alone", () => {
    const live = framePlan("thinking", { level: 0, busyVisuals: false, reduceMotion: false, settled: false, hidden: false });
    expect(live.mode).toBe("raf");
    expect(sceneFramePlan(live, true)).toEqual({ mode: "raf", intervalMs: Math.max(live.intervalMs, STUCK_FRAME_MS) });
    expect(sceneFramePlan(live, false)).toBe(live);
    const parked = { mode: "parked" as const, intervalMs: Infinity };
    expect(sceneFramePlan(parked, true)).toBe(parked);
  });
});

describe("the strip names the stuck engine during a stuck take", () => {
  it("shows the engine label in the phases a stuck take sits in", () => {
    for (const phaseId of ["thinking", "transcribing", "model_loading"]) {
      expect(engineHealthTakesStrip("stuck", phaseId)).toBe(true);
      expect(engineHealthTakesStrip("restarting", phaseId)).toBe(true);
    }
    expect(engineHealthTakesStrip("stuck", "listening")).toBe(false);
  });
});

describe("watchCanvasLoss", () => {
  it("wires and unwires lost/restored on every canvas", () => {
    const t = new EventTarget();
    const lost = vi.fn();
    const restored = vi.fn();
    const off = watchCanvasLoss([t], lost, restored);
    t.dispatchEvent(new Event("contextlost"));
    t.dispatchEvent(new Event("contextrestored"));
    expect(lost).toHaveBeenCalledTimes(1);
    expect(restored).toHaveBeenCalledTimes(1);
    off();
    t.dispatchEvent(new Event("contextlost"));
    expect(lost).toHaveBeenCalledTimes(1);
  });
});
