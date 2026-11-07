import { describe, expect, it } from "vitest";
import { type EngineLoadProgress, type LoadStage, parseLoadProgress } from "./launchWalk";
import {
  BELL_SHOW_MS,
  ENGINE_ART_STAGE,
  SWEAT_MS,
  WARM_LABELS,
  WARM_WIN_H,
  WARM_WIN_W,
  downloadPct,
  forcedWarmup,
  loadBar,
  loadStep,
  warmupAriaLabel,
  warmupModel,
  warmupStillKey,
  type WarmBeat,
  type WarmInput,
} from "./warmup";

const STEP: Record<LoadStage, number> = { starting: 0, reading: 1, waiting: 2, testing: 3, ready: 4, failed: 2 };
const prog = (pct: number, stage: LoadStage, step: number | undefined = STEP[stage]): EngineLoadProgress =>
  ({ pct, stage, modelId: "m", elapsedMs: 0, ...(step === undefined ? {} : { step }) });
const base = (o: Partial<WarmInput> = {}): WarmInput => ({
  download: null, load: null, stepAgeMs: 0, sinceReadyMs: null, recording: false, reduceMotion: false, tMs: 0, ...o,
});

/**
 * A slowed fake loader as a timeline: [ms since start, progress]. The native
 * load sits at "waiting" for 20 s and the test pass for 9 s, far longer than
 * any animation or the bell's own show time.
 */
function slowedLoader(): Array<[number, EngineLoadProgress]> {
  const t: Array<[number, EngineLoadProgress]> = [[0, prog(0, "starting")]];
  for (let pct = 1, at = 400; pct <= 94; pct += 3, at += 700) t.push([at, prog(pct, "reading")]);
  const afterRead = 400 + 32 * 700;
  t.push([afterRead, prog(95, "waiting")]);
  t.push([afterRead + 20_000, prog(95, "testing")]);
  t.push([afterRead + 29_000, prog(100, "ready")]);
  return t;
}

/** Drive the pill model over the fake loader's timeline, like the component does. */
function run(timeline: Array<[number, EngineLoadProgress]>, recording = false) {
  const readyAt = timeline.find(([, p]) => p.stage === "ready")![0];
  const end = readyAt + BELL_SHOW_MS + 500;
  const out: Array<{ t: number; beat: WarmBeat; mode: string; bar: number; label: string; sweat: boolean }> = [];
  let idx = 0;
  let stepStart = 0;
  let lastStep = -1;
  let seenNonReady = false;
  let readyTime: number | null = null;
  for (let t = 0; t <= end; t += 50) {
    while (idx + 1 < timeline.length && timeline[idx + 1][0] <= t) idx++;
    const p = timeline[idx][1];
    const step = loadStep(p);
    if (step !== lastStep) { stepStart = t; lastStep = step; }
    if (p.stage !== "ready") seenNonReady = true;
    if (p.stage === "ready" && readyTime === null && seenNonReady) readyTime = t;
    const m = warmupModel(base({
      load: p, stepAgeMs: t - stepStart, sinceReadyMs: readyTime === null ? null : t - readyTime, recording, tMs: t,
    }));
    out.push({ t, beat: m.beat, mode: m.mode, bar: m.bar, label: m.label, sweat: m.sweat });
  }
  return { out, readyAt };
}

describe("yap-1106-pill-warmup-engine — beats from the load signal", () => {
  it("a slowed fake loader never shows the bell before ready", () => {
    const { out, readyAt } = run(slowedLoader());
    for (const f of out) {
      if (f.t < readyAt) {
        expect(f.beat).not.toBe("bell");
        expect(f.mode).not.toBe("ready");
        expect(f.bar).toBeLessThan(100);
      }
    }
    // ...and it does ring once ready arrives, then the pill folds back.
    expect(out.some((f) => f.t >= readyAt && f.beat === "bell" && f.mode === "ready")).toBe(true);
    expect(out[out.length - 1].mode).toBe("hidden");
  });

  it("a slowed fake loader that records throughout still never rings early", () => {
    const { out, readyAt } = run(slowedLoader(), true);
    expect(out.filter((f) => f.t < readyAt).every((f) => f.beat !== "bell")).toBe(true);
  });

  it("walks the five beats in order: asleep, crank, typing code, steam, bell", () => {
    const { out } = run(slowedLoader());
    const order: WarmBeat[] = [];
    for (const f of out) if (f.mode !== "hidden" && order[order.length - 1] !== f.beat) order.push(f.beat);
    expect(order).toEqual(["asleep", "crank", "typing", "steam", "bell"]);
  });

  it("the beat comes from the signal, never the clock", () => {
    for (const stage of ["starting", "reading", "waiting", "testing"] as LoadStage[]) {
      const beats = new Set<WarmBeat>();
      for (const tMs of [0, 333, 5000, 61_000, 9_999_999]) beats.add(warmupModel(base({ load: prog(40, stage), tMs })).beat);
      expect(beats.size).toBe(1);
    }
    expect(warmupModel(base({ load: prog(0, "starting") })).beat).toBe("asleep");
    expect(warmupModel(base({ load: prog(30, "reading") })).beat).toBe("crank");
    expect(warmupModel(base({ load: prog(95, "waiting") })).beat).toBe("typing");
    expect(warmupModel(base({ load: prog(95, "testing") })).beat).toBe("steam");
    expect(warmupModel(base({ load: prog(100, "ready"), sinceReadyMs: 10 })).beat).toBe("bell");
  });

  it("each step is one third of the bar; only ready fills it", () => {
    expect(loadBar(prog(0, "starting"))).toBe(0);
    expect(loadBar(prog(1, "reading"))).toBe(0);
    expect(loadBar(prog(48, "reading"))).toBeGreaterThan(15);
    expect(loadBar(prog(48, "reading"))).toBeLessThan(17);
    expect(loadBar(prog(94, "reading"))).toBeCloseTo(33.3, 1);
    expect(loadBar(prog(95, "waiting"))).toBeCloseTo(33.3, 1);
    expect(loadBar(prog(95, "testing"))).toBeCloseTo(66.7, 1);
    expect(loadBar(prog(100, "ready"))).toBe(100);
    // An older backend without `step` derives the same steps from the stage.
    expect(loadStep(prog(95, "testing", undefined))).toBe(3);
    // A stray step 4 on a non-ready stage is never trusted as ready.
    expect(loadStep(prog(95, "waiting", 4))).toBe(3);
  });

  it("parses the backend's step field and the new testing stage", () => {
    expect(parseLoadProgress({ pct: 95, stage: "testing", modelId: "m", elapsedMs: 4, step: 3 }))
      .toEqual({ pct: 95, stage: "testing", modelId: "m", elapsedMs: 4, step: 3 });
    expect(parseLoadProgress({ pct: 95, stage: "testing", step: 9 })?.step).toBeUndefined();
  });

  it("a step over 8 s shows the sweat pose and 'First start takes longer'", () => {
    const under = warmupModel(base({ load: prog(95, "waiting"), stepAgeMs: SWEAT_MS - 1 }));
    expect(under.sweat).toBe(false);
    expect(under.label).toBe(WARM_LABELS.working);
    const over = warmupModel(base({ load: prog(95, "waiting"), stepAgeMs: SWEAT_MS + 1 }));
    expect(over.sweat).toBe(true);
    expect(over.label).toBe("First start takes longer");
    expect(over.beat).toBe("typing");
    // The slowed loader's 20 s native load sweats; the bell never does.
    const { out } = run(slowedLoader());
    expect(out.some((f) => f.sweat && f.beat === "typing")).toBe(true);
    expect(out.some((f) => f.sweat && f.beat === "bell")).toBe(false);
  });

  it("fn during warm-up records anyway: notepad out, crank dropped, beat unchanged", () => {
    const m = warmupModel(base({ load: prog(30, "reading"), recording: true }));
    expect(m.notepad).toBe(true);
    expect(m.beat).toBe("crank");
    expect(m.pose.pencil).toBe(true);
    expect(m.engineStage).not.toBe(ENGINE_ART_STAGE.crank);
    expect(warmupAriaLabel(m)).toContain("Recording");
  });

  it("a load failure shows smoke and Retry", () => {
    const m = warmupModel(base({ load: prog(95, "failed", 2) }));
    expect(m.mode).toBe("failed");
    expect(m.beat).toBe("smoke");
    expect(m.showRetry).toBe(true);
    expect(m.bar).toBeLessThan(100);
    expect(warmupAriaLabel(m)).toContain("Retry");
  });

  it("a warm engine skips all of it", () => {
    // Opened onto a ready engine: no non-ready seen, so no bell.
    expect(warmupModel(base({ load: prog(100, "ready"), sinceReadyMs: null })).mode).toBe("hidden");
    expect(warmupModel(base()).mode).toBe("hidden");
    expect(warmupModel(base({ load: prog(100, "ready"), sinceReadyMs: BELL_SHOW_MS })).mode).toBe("hidden");
  });

  it("first launch fills the bar from model_download_progress", () => {
    expect(downloadPct({ model_id: "m", downloaded: 50, total: 200 })).toBe(25);
    expect(downloadPct({ downloaded: 1, total: 0 })).toBeNull();
    expect(downloadPct(null)).toBeNull();
    const m = warmupModel(base({ download: 25 }));
    expect(m.mode).toBe("download");
    expect(m.bar).toBe(25);
    expect(m.beat).toBe("crank");
    expect(warmupModel(base({ download: 0 })).beat).toBe("asleep");
    expect(warmupModel(base({ download: 100 })).mode).toBe("hidden");
    // A running load outranks a stale download number.
    expect(warmupModel(base({ download: 40, load: prog(95, "testing") })).mode).toBe("load");
  });

  it("reduce motion: still poses, one key per visible change", () => {
    const a = warmupModel(base({ load: prog(30, "reading"), reduceMotion: true, tMs: 0 }));
    const b = warmupModel(base({ load: prog(30, "reading"), reduceMotion: true, tMs: 777 }));
    expect(a.pose).toEqual(b.pose);
    expect(warmupStillKey(a)).toBe(warmupStillKey(b));
    const c = warmupModel(base({ load: prog(95, "waiting"), reduceMotion: true }));
    expect(warmupStillKey(c)).not.toBe(warmupStillKey(a));
  });

  it("the window is the approved 72 x 24", () => {
    expect([WARM_WIN_W, WARM_WIN_H]).toEqual([72, 24]);
  });

  it("forced captures parse, junk is null", () => {
    expect(forcedWarmup("?warm=dl:40")).toEqual({ download: 40, load: null, sweat: false });
    expect(forcedWarmup("?warm=95:testing:sweat")?.sweat).toBe(true);
    expect(forcedWarmup("?warm=100:ready")?.load?.pct).toBe(100);
    expect(forcedWarmup("?warm=50:nope")).toBeNull();
    expect(forcedWarmup("?walk=40")).toBeNull();
  });
});
