/**
 * Y5-K — the habitat director: routines on the REAL clock with intent pathing,
 * and reactions pushed by the take / paste / model events. Pure, so the clock
 * is injected and a whole day runs in a millisecond.
 */
import { describe, expect, it, vi } from "vitest";
// @ts-expect-error -- node builtin, untyped here; the test runs under Node.
import { readFileSync } from "node:fs";
import {
  CHEER_MS, DONE_HOLD_MS, RUSH, ROUTINES, ROUTINE_STEP_MIN, STATIONS, WALK_PX_PER_S,
  createDirector, eventForStatus, intentAt, modelReadyFrom, moodOf, reactTo, routineAt, skyPhase, stepDirector,
  type DirectorState,
} from "./habitat";

const read = (rel: string): string => readFileSync(new URL(rel, import.meta.url), "utf8") as string;
const at = (h: number, m = 0, s = 0) => new Date(2026, 9, 27, h, m, s);
/** Run the director for `secs` of wall time from `start`, in 1/60 s steps. */
function run(st: DirectorState, start: Date, secs: number): number[] {
  const xs: number[] = [];
  const dt = 1 / 60;
  for (let i = 0; i < secs * 60; i++) {
    stepDirector(st, dt, new Date(start.getTime() + i * dt * 1000));
    xs.push(st.x);
  }
  return xs;
}
const monotone = (xs: number[], dir: 1 | -1) => xs.every((x, i) => i === 0 || (x - xs[i - 1]) * dir >= 0);

describe("the habitat director (Y5-K)", () => {
  it("the_director_runs_on_the_real_clock_not_a_random_walk", () => {
    const rnd = vi.spyOn(Math, "random");
    // The day is a table: it starts at midnight, in hour order, every station real.
    expect(ROUTINES[0].from).toBe(0);
    ROUTINES.forEach((r, i) => i && expect(r.from).toBeGreaterThan(ROUTINES[i - 1].from));
    for (const r of ROUTINES) for (const s of r.stations) expect(STATIONS[s]).toBeDefined();
    // The clock decides: 3am is the nest, mid-morning is the desk, and the same
    // moment means the same place every time it is asked.
    expect(routineAt(at(3)).id).toBe("sleep");
    expect(intentAt(at(3, 30))).toBe("nest");
    expect(intentAt(at(9, 0))).toBe("desk");
    expect(intentAt(at(9, ROUTINE_STEP_MIN))).toBe("coffee");
    expect(intentAt(at(23, 59))).toBe("nest");
    for (let h = 0; h < 24; h++) expect(intentAt(at(h, 17))).toBe(intentAt(at(h, 17)));
    // The sky follows the same clock: 6am dawn, noon day, 6pm dusk.
    expect(skyPhase(at(6))).toBeCloseTo(0);
    expect(skyPhase(at(12))).toBeCloseTo(0.25);
    expect(skyPhase(at(18))).toBeCloseTo(0.5);
    // INTENT PATHING: from the nest at 10:00 she walks straight to the desk —
    // never backwards, never past it — and then works there.
    const st = createDirector();
    const xs = run(st, at(10, 0), (STATIONS.desk.x - STATIONS.nest.x) / WALK_PX_PER_S + 1);
    expect(monotone(xs, 1)).toBe(true);
    expect(Math.max(...xs)).toBe(STATIONS.desk.x);
    expect(st).toMatchObject({ station: "desk", mode: "act", activity: "work", x: STATIONS.desk.x });
    // ...and at 3am, from the desk, straight back to bed, facing the way she walks.
    const back = run(st, at(3, 0), (STATIONS.desk.x - STATIONS.nest.x) / WALK_PX_PER_S + 1);
    expect(monotone(back, -1)).toBe(true);
    expect(st).toMatchObject({ station: "nest", activity: "sleep", x: STATIONS.nest.x });
    // A whole simulated day, sampled every routine step: she always ends up
    // where the clock says, so the house is the same house at the same time.
    for (let m = 0; m < 24 * 60; m += ROUTINE_STEP_MIN) {
      const now = at(0, m);
      run(st, now, 8);
      expect(st.station, `${now.getHours()}:${now.getMinutes()}`).toBe(intentAt(now));
      expect(st.x).toBe(STATIONS[intentAt(now)].x);
    }
    expect(rnd).not.toHaveBeenCalled();
    rnd.mockRestore();
    // And by construction: the director has no dice in it, and the house runs it.
    expect(read("./habitat.ts")).not.toMatch(/Math\.random/);
    const house = read("../YappyHouse.tsx");
    expect(house).toContain("stepDirector(state, dt, new Date())");
    expect(house).not.toMatch(/function pickNext|const STATIONS/);
  });

  it("the_habitat_reacts_to_take_paste_and_model_events", () => {
    const t0 = at(3, 0).getTime(); // asleep in the nest
    const st = createDirector();
    // A take STARTING: she rushes to the desk (faster than a stroll) and works.
    reactTo(st, eventForStatus({ recording: true }), t0);
    expect(st.appState).toBe("listening");
    const rush = run(st, at(3, 0), (STATIONS.desk.x - STATIONS.nest.x) / (WALK_PX_PER_S * RUSH) + 0.5);
    expect(monotone(rush, 1)).toBe(true);
    expect(st).toMatchObject({ station: "desk", activity: "work", mode: "act" });
    expect(moodOf(st)).toBe("listening");
    reactTo(st, eventForStatus({ busy: true }), t0);
    expect(moodOf(st)).toBe("thinking");
    // A PASTE landing: the happy reaction, held, and a status=idle inside the
    // hold does not cut it short.
    reactTo(st, { kind: "paste" }, t0);
    stepDirector(st, 1 / 60, new Date(t0));
    expect(st).toMatchObject({ appState: "done", activity: "happy" });
    reactTo(st, eventForStatus(null), t0 + DONE_HOLD_MS - 1);
    expect(st.appState).toBe("done");
    stepDirector(st, 1 / 60, new Date(t0 + DONE_HOLD_MS + 1));
    expect(st.appState).toBe("idle");
    reactTo(st, eventForStatus({}), t0 + DONE_HOLD_MS + 2);
    expect(st.appState).toBe("idle");
    // A MODEL finishing its download: one cheer, in place, then back to the routine.
    const seen = new Set<string>();
    expect(modelReadyFrom(null, seen)).toBeNull();
    expect(modelReadyFrom({ model_id: "base", downloaded: 1, total: 0 }, seen)).toBeNull();
    expect(modelReadyFrom({ model_id: "base", downloaded: 5, total: 10 }, seen)).toBeNull();
    const ready = modelReadyFrom({ model_id: "base", downloaded: 10, total: 10 }, seen);
    expect(ready).toEqual({ kind: "model-ready" });
    expect(modelReadyFrom({ model_id: "base", downloaded: 10, total: 10 }, seen)).toBeNull();
    // A payload without an id still counts once (keyed by the empty id).
    expect(modelReadyFrom({ downloaded: 3, total: 3 }, new Set())).toEqual({ kind: "model-ready" });
    const x = st.x;
    reactTo(st, ready!, t0 + 5000);
    stepDirector(st, 1 / 60, new Date(t0 + 5000));
    expect(st).toMatchObject({ reaction: "cheer", activity: "happy", x });
    expect(moodOf(st)).toBe("done");
    stepDirector(st, 1 / 60, new Date(t0 + 5000 + CHEER_MS + 1));
    expect(st.reaction).toBeNull();
    // The live level lights the window beam, clamped.
    reactTo(st, { kind: "level", value: 4 }, t0);
    expect(st.energy).toBe(1);
    reactTo(st, { kind: "level", value: Number.NaN }, t0);
    expect(st.energy).toBe(0);
    // Mood words come from the real state.
    expect(moodOf({ ...createDirector(), activity: "sleep" })).toBe("nap");
    expect(moodOf({ ...createDirector(), activity: "look" })).toBe("idle");
    expect(moodOf({ ...createDirector(), activity: "work" })).toBe("work");
    // The feed SUBSCRIBES to the same events the pill does, and never polls.
    const feed = read("./events.ts");
    for (const ev of ["status", "recording", "audio_level", "transcript", "model_download_progress"]) {
      expect(feed, ev).toMatch(new RegExp(`listen(?:<[^>]+>)?\\("${ev}"`));
    }
    expect(feed).not.toMatch(/setInterval|setTimeout|requestAnimationFrame/);
    expect(read("../YappyHouse.tsx")).toContain("subscribeHabitat(");
  });
});
