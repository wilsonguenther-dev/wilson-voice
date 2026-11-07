import { describe, expect, it } from "vitest";
import {
  DOOR_SCROLL,
  FRIEND_HOUSES,
  HOLD_PCT,
  LAYER_SPEEDS,
  READY_CATCHUP_MS,
  READY_SHOW_MS,
  STALL_MS,
  WALK_DIST,
  WARM_HOP_MS,
  houseX,
  launchWalkModel,
  parseLoadProgress,
  scrollForPct,
  stillFrameKey,
  type EngineLoadProgress,
  type LoadStage,
  type WalkInput,
} from "./launchWalk";

const prog = (pct: number, stage: LoadStage = "reading"): EngineLoadProgress => ({ pct, stage, modelId: "m", elapsedMs: 0 });
const base = (o: Partial<WalkInput> = {}): WalkInput => ({
  progress: null, sinceChangeMs: 0, sinceReadyMs: null, readyFromPct: HOLD_PCT, warmHopMs: null,
  recording: false, reduceMotion: false, tMs: 0, ...o,
});

/**
 * A slowed fake loader: it reports a few percentages, then sits at the hold
 * for a long native load, then (only then) says ready. Replayed on a fake
 * clock at 60 fps, exactly as the pill's frame loop would consume it.
 */
function slowedFakeLoader(): { atMs: number; p: EngineLoadProgress }[] {
  const out: { atMs: number; p: EngineLoadProgress }[] = [{ atMs: 0, p: prog(0, "starting") }];
  for (let i = 1; i <= 20; i++) out.push({ atMs: i * 400, p: prog(Math.min(94, i * 5)) });
  out.push({ atMs: 8_400, p: prog(HOLD_PCT, "waiting") });
  out.push({ atMs: 20_000, p: prog(100, "ready") }); // a 12-second native load
  return out;
}

describe("launch walk — bound to engine load, never a timer", () => {
  it("a slowed fake loader never shows the ready pose before ready", () => {
    const events = slowedFakeLoader();
    const readyAt = events[events.length - 1].atMs;
    let last: EngineLoadProgress | null = null;
    let changedAt = 0;
    let readyFrom = 0;
    let frames = 0;
    let next = 0;
    for (let t = 0; t < readyAt + READY_SHOW_MS + 100; t += 16) {
      while (next < events.length && events[next].atMs <= t) {
        const e = events[next++];
        if (e.p.stage === "ready") readyFrom = last?.pct ?? 0;
        if (!last || last.pct !== e.p.pct || last.stage !== e.p.stage) changedAt = e.atMs;
        last = e.p;
      }
      for (const reduceMotion of [false, true]) {
        const m = launchWalkModel(base({
          progress: last, sinceChangeMs: t - changedAt, recording: t > 3_000 && t < 5_000,
          sinceReadyMs: last?.stage === "ready" ? t - readyAt : null, readyFromPct: readyFrom, reduceMotion, tMs: t,
        }));
        frames++;
        if (t < readyAt) {
          expect(m.mode).not.toBe("ready");
          expect(m.officeOpen).toBe(false);
          expect(m.pose.mood).toBeLessThan(1);
          expect(m.scroll).toBeLessThanOrEqual(DOOR_SCROLL);
          expect(m.pct).toBeLessThanOrEqual(HOLD_PCT);
        }
      }
    }
    expect(frames).toBeGreaterThan(1000);
  });

  it("position comes from the percentage alone: the clock never moves the ground", () => {
    for (const pct of [0, 10, 40, 80]) {
      const a = launchWalkModel(base({ progress: prog(pct), tMs: 0 }));
      const b = launchWalkModel(base({ progress: prog(pct), tMs: 9_999 }));
      expect(a.scroll).toBe(b.scroll);
      expect(a.scroll).toBeCloseTo(scrollForPct(pct));
    }
    expect(scrollForPct(50)).toBeGreaterThan(scrollForPct(10));
    expect(scrollForPct(200)).toBe(DOOR_SCROLL);
    expect(scrollForPct(Number.NaN)).toBe(0);
  });

  it("a stalled load stops the ground; his legs keep going (walks on the spot)", () => {
    const moving = launchWalkModel(base({ progress: prog(30), sinceChangeMs: 100 }));
    const stalled0 = launchWalkModel(base({ progress: prog(30), sinceChangeMs: STALL_MS, tMs: 0 }));
    const stalled1 = launchWalkModel(base({ progress: prog(30), sinceChangeMs: STALL_MS + 5_000, tMs: 125 }));
    expect(moving.stalled).toBe(false);
    expect(stalled0.stalled).toBe(true);
    expect(stalled1.scroll).toBe(stalled0.scroll);
    expect(stalled0.pose.hop).not.toBe(stalled1.pose.hop);
  });

  it("waits at the office door from 95 % until ready", () => {
    const w = launchWalkModel(base({ progress: prog(HOLD_PCT, "waiting"), sinceChangeMs: 60_000 }));
    expect(w.mode).toBe("wait");
    expect(w.scroll).toBe(DOOR_SCROLL);
    expect(w.officeOpen).toBe(false);
    expect(w.stalled).toBe(false);
  });

  it("ready: door opens after at most a 300 ms catch-up, then the walk hides", () => {
    const early = launchWalkModel(base({ progress: prog(100, "ready"), sinceReadyMs: 0, readyFromPct: 40 }));
    expect(early.mode).toBe("ready");
    expect(early.running).toBe(true);
    expect(early.scroll).toBeCloseTo(scrollForPct(40));
    const caught = launchWalkModel(base({ progress: prog(100, "ready"), sinceReadyMs: READY_CATCHUP_MS, readyFromPct: 40 }));
    expect(caught.scroll).toBe(WALK_DIST);
    expect(caught.officeOpen).toBe(true);
    expect(launchWalkModel(base({ progress: prog(100, "ready"), sinceReadyMs: READY_SHOW_MS })).mode).toBe("hidden");
  });

  it("a warm engine skips the walk: one 600 ms hop", () => {
    const hop = launchWalkModel(base({ warmHopMs: WARM_HOP_MS / 2 }));
    expect(hop.mode).toBe("hop");
    expect(hop.pose.hop).toBeGreaterThan(0);
    expect(launchWalkModel(base({ warmHopMs: WARM_HOP_MS })).mode).toBe("hidden");
    expect(launchWalkModel(base()).mode).toBe("hidden");
  });

  it("a failed load stops him and shows Retry", () => {
    const f = launchWalkModel(base({ progress: prog(60, "failed"), tMs: 500 }));
    const g = launchWalkModel(base({ progress: prog(60, "failed"), tMs: 9_000 }));
    expect(f.mode).toBe("failed");
    expect(f.showRetry).toBe(true);
    expect(f.officeOpen).toBe(false);
    expect(f.scroll).toBe(g.scroll);
    expect(f.pose).toEqual(g.pose);
  });

  it("recording during the walk turns it into a run (and never blocks it)", () => {
    const walk = launchWalkModel(base({ progress: prog(30), tMs: 100 }));
    const run = launchWalkModel(base({ progress: prog(30), recording: true, tMs: 100 }));
    expect(walk.running).toBe(false);
    expect(run.running).toBe(true);
    expect(run.mode).toBe("walk");
    expect(run.scroll).toBe(walk.scroll);
  });

  it("reduce motion: one still frame at the office door with a progress line", () => {
    const a = launchWalkModel(base({ progress: prog(20), reduceMotion: true, tMs: 0 }));
    const b = launchWalkModel(base({ progress: prog(70), reduceMotion: true, tMs: 777 }));
    expect(a.scroll).toBe(DOOR_SCROLL);
    expect(b.scroll).toBe(DOOR_SCROLL);
    expect(a.progressLine).toBe(20);
    expect(b.progressLine).toBe(70);
    expect(a.pose).toEqual(b.pose);
    expect(a.waving).toEqual([]);
    expect(stillFrameKey(a)).not.toBe(stillFrameKey(b));
    expect(launchWalkModel(base({ progress: prog(20) })).progressLine).toBeNull();
  });

  it("at least two parallax layers, the far ones slower than the ground", () => {
    const speeds = new Set(Object.values(LAYER_SPEEDS));
    expect(speeds.size).toBeGreaterThanOrEqual(2);
    expect(LAYER_SPEEDS.hills).toBeLessThan(LAYER_SPEEDS.trees);
    expect(LAYER_SPEEDS.trees).toBeLessThan(LAYER_SPEEDS.ground);
  });

  it("each friend waves as Yappy passes their house", () => {
    expect(FRIEND_HOUSES).toHaveLength(8);
    for (const h of FRIEND_HOUSES) {
      const pct = (h.at / 100) * HOLD_PCT * (WALK_DIST / DOOR_SCROLL);
      const m = launchWalkModel(base({ progress: prog(Math.round(pct)) }));
      expect(Math.abs(houseX(h, m.scroll) - 88)).toBeLessThan(30);
      expect(m.waving).toContain(h.id);
    }
  });

  it("parses the backend payload and refuses a 100 that is not ready", () => {
    expect(parseLoadProgress({ pct: 100, stage: "reading", modelId: "m", elapsedMs: 3 })?.pct).toBe(HOLD_PCT);
    expect(parseLoadProgress({ pct: 100, stage: "ready", modelId: "m", elapsedMs: 3 })?.pct).toBe(100);
    expect(parseLoadProgress({ pct: 40, stage: "nope" })).toBeNull();
    expect(parseLoadProgress(null)).toBeNull();
    expect(parseLoadProgress({ pct: Number.NaN, stage: "reading" })).toBeNull();
    expect(parseLoadProgress({ pct: 12.4, stage: "reading" })).toEqual({ pct: 12, stage: "reading", modelId: "", elapsedMs: 0 });
  });
});
