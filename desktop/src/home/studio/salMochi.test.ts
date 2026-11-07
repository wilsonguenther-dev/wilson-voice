/**
 * yap-1106-sal-and-mochi — Sal the salamander and Mochi the koala, room only.
 * Acceptance: Sal sits at the back-room furnace and lights it while the engine
 * loads; Mochi hangs on the door during Focus, screen share and Do Not
 * Disturb. Neither ever appears in the pill.
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync, readdirSync } from "node:fs";
import { Raster } from "../../sprites/raster";
import { skyPhase } from "../habitat/habitat";
import {
  EMPTY_STUDIO_INPUTS,
  FLOOR,
  FULL_FLAME,
  LIGHTING_CYCLE_S,
  MOCHI_ON_DOOR,
  MOCHI_ON_FRAME,
  PAD,
  STUDIO_H,
  STUDIO_W,
  drawLights,
  drawMid,
  furnaceFlame,
  furnaceState,
  mochiOnDoor,
  mochiQuietReason,
  mochiSpot,
  redrawPlan,
  salFrame,
  studioAriaLabel,
  studioBackRoomLabel,
  type StudioState,
} from "./studio";
import { EMPTY_LEDGER, EVENT_OWNER, FRIEND_EVENTS, ROOM_ONLY, decideVisit, forcedFriendVisit, type VisitContext } from "../../pill/friendVisits";
import { PILL_CHARACTERS } from "../../pill/characterFit";

const NOON = skyPhase(new Date(2026, 10, 7, 12, 0));
const DAY_SKY = { top: "#7ec8ff", bot: "#cdeeff", amb: "#fff2d8", ambA: 0 };

function st(over: Partial<StudioState> = {}): StudioState {
  return { ...EMPTY_STUDIO_INPUTS, phase: NOON, sky: DAY_SKY, meetingRecording: false, yappy: "rest", modelReady: true, ...over };
}
function paint(fn: (r: Raster) => void): Raster {
  const r = new Raster(STUDIO_W + PAD * 2, STUDIO_H);
  r.translate(PAD, 0);
  fn(r);
  return r;
}
/** Pixels that differ inside a box (art px, before PAD). */
function differsIn(a: Raster, b: Raster, x0 = -PAD, y0 = 0, x1 = STUDIO_W + PAD, y1 = STUDIO_H): number {
  let n = 0;
  for (let y = Math.max(0, y0); y < Math.min(STUDIO_H, y1); y++)
    for (let x = x0 + PAD; x < x1 + PAD; x++) {
      const p = a.px(x, y), q = b.px(x, y);
      if (p.r !== q.r || p.g !== q.g || p.b !== q.b || p.a !== q.a) n++;
    }
  return n;
}
const BACK_ROOM = [270, 0, STUDIO_W, FLOOR + 4] as const;

describe("Sal sits at the back-room furnace and lights it while the engine loads", () => {
  it("sal_lights_the_furnace_while_the_engine_loads", () => {
    expect(furnaceState({ engineLoading: true, modelReady: true })).toBe("lighting");
    expect(furnaceState({ engineLoading: true, modelReady: false })).toBe("lighting");
    expect(furnaceState({ engineLoading: false, modelReady: true })).toBe("lit");
    expect(furnaceState({ engineLoading: false, modelReady: null })).toBe("lit");
    expect(furnaceState({ engineLoading: false, modelReady: false })).toBe("cold");
    // Home's empty inputs (nothing known yet) never claim a cold or loading engine.
    expect(furnaceState(EMPTY_STUDIO_INPUTS)).toBe("lit");
  });

  it("the_flame_grows_from_a_spark_to_the_full_fire_and_never_outgrows_it", () => {
    expect(furnaceFlame("lit", 3)).toEqual({ rx: FULL_FLAME.rx, ry: FULL_FLAME.ry });
    expect(furnaceFlame("cold", 3)).toBeNull();
    expect(furnaceFlame("lighting", 0)).toEqual({ rx: 2, ry: 1 });
    expect(furnaceFlame("lighting", Number.NaN)).toEqual({ rx: 2, ry: 1 });
    expect(furnaceFlame("lighting", -4)).toEqual({ rx: 2, ry: 1 });
    let prev = 0;
    for (let t = 0; t < LIGHTING_CYCLE_S; t += 0.05) {
      const f = furnaceFlame("lighting", t)!;
      expect(f.rx).toBeGreaterThanOrEqual(prev);
      expect(f.rx).toBeLessThanOrEqual(FULL_FLAME.rx);
      expect(f.ry).toBeLessThanOrEqual(FULL_FLAME.ry);
      prev = f.rx;
    }
    expect(furnaceFlame("lighting", LIGHTING_CYCLE_S * 0.99)!.rx).toBe(FULL_FLAME.rx);
  });

  it("sal_is_busy_while_lighting_and_dozes_by_a_cold_furnace", () => {
    expect(salFrame("lighting", 0)).toBe(0);
    expect(salFrame("lighting", 0.3)).toBe(1);
    expect(salFrame("cold", 0)).toBe(1);
    expect(salFrame("lit", 0)).toBe(0);
    expect(salFrame("lit", 0.6)).toBe(1);
    expect(salFrame("lit", Number.NaN)).toBe(0);
  });

  it("the_back_room_furnace_draws_lit_lighting_and_cold_differently", () => {
    const lit = paint((r) => drawLights(r, st(), 0, 0, 0));
    const lighting = paint((r) => drawLights(r, st({ engineLoading: true }), 0, 0, 0));
    const cold = paint((r) => drawLights(r, st({ modelReady: false }), 0, 0, 0));
    expect(differsIn(lit, lighting, ...BACK_ROOM)).toBeGreaterThan(0);
    expect(differsIn(lit, cold, ...BACK_ROOM)).toBeGreaterThan(0);
    // Only the back room changes: the rest of the lights are untouched.
    expect(differsIn(lit, lighting, -PAD, 0, 270, STUDIO_H)).toBe(0);
    // The cold furnace shows ash in the station layer; Sal is still on it.
    const midLit = paint((r) => drawMid(r, st(), 0));
    const midCold = paint((r) => drawMid(r, st({ modelReady: false }), 0));
    expect(differsIn(midLit, midCold, ...BACK_ROOM)).toBeGreaterThan(0);
    expect(() => paint((r) => drawLights(r, st({ engineLoading: true }), Number.NaN, 0, 0))).not.toThrow();
  });

  it("the_flame_moves_while_lighting_so_the_room_runs_live", () => {
    const base = { reduceMotion: false, appState: "idle" as const, reacting: false, parallaxSettling: false, meetingRecording: false };
    expect(redrawPlan({ ...base, furnaceLighting: true }).mode).toBe("live");
    expect(redrawPlan({ ...base, furnaceLighting: false }).mode).toBe("idle");
    expect(redrawPlan(base).mode).toBe("idle");
    expect(redrawPlan({ ...base, reduceMotion: true, furnaceLighting: true }).mode).toBe("still");
  });
});

describe("Mochi hangs on the door during Focus, screen share and Do Not Disturb", () => {
  it("mochi_hangs_on_the_door_during_focus_screen_share_and_dnd", () => {
    expect(mochiOnDoor({ screenSharing: false, doNotDisturb: false })).toBe(false);
    expect(mochiOnDoor({ screenSharing: true, doNotDisturb: false })).toBe(true);
    // A Focus is reported as doNotDisturb by pill_quiet.rs (Assertions.json).
    expect(mochiOnDoor({ screenSharing: false, doNotDisturb: true })).toBe(true);
    expect(mochiQuietReason({ screenSharing: true, doNotDisturb: true })).toBe("screen-sharing");
    expect(mochiQuietReason({ screenSharing: false, doNotDisturb: true })).toBe("do-not-disturb");
    expect(mochiQuietReason({ screenSharing: false, doNotDisturb: false })).toBeNull();
    expect(mochiSpot({ screenSharing: false, doNotDisturb: false })).toEqual(MOCHI_ON_FRAME);
    expect(mochiSpot({ screenSharing: false, doNotDisturb: true })).toEqual(MOCHI_ON_DOOR);
    // On the door = lower than the frame, still over the back door (x 282..316).
    expect(MOCHI_ON_DOOR.y).toBeGreaterThan(MOCHI_ON_FRAME.y);
    expect(MOCHI_ON_DOOR.x + 20).toBeGreaterThan(282);
    expect(MOCHI_ON_DOOR.x + 20).toBeLessThan(316);
  });

  it("the_door_picture_changes_only_in_the_back_room", () => {
    const calm = paint((r) => drawMid(r, st(), 0));
    const dnd = paint((r) => drawMid(r, st({ doNotDisturb: true }), 0));
    const share = paint((r) => drawMid(r, st({ screenSharing: true }), 0));
    expect(differsIn(calm, dnd, ...BACK_ROOM)).toBeGreaterThan(0);
    expect(differsIn(calm, dnd, -PAD, 0, 270, STUDIO_H)).toBe(0);
    expect(differsIn(dnd, share)).toBe(0);
  });

  it("the_label_says_what_sal_and_mochi_are_doing", () => {
    const q = { screenSharing: false, doNotDisturb: false };
    expect(studioBackRoomLabel({ engineLoading: true, modelReady: true, ...q })).toContain("Sal is lighting the furnace");
    expect(studioBackRoomLabel({ engineLoading: false, modelReady: false, ...q })).toContain("cold furnace");
    expect(studioBackRoomLabel({ engineLoading: false, modelReady: true, ...q })).toContain("Sal keeps the furnace warm");
    expect(studioBackRoomLabel({ engineLoading: false, modelReady: true, ...q })).toContain("Mochi hangs over the door.");
    expect(studioBackRoomLabel({ engineLoading: false, modelReady: true, screenSharing: true, doNotDisturb: false })).toContain("while your screen is shared");
    expect(studioBackRoomLabel({ engineLoading: false, modelReady: true, screenSharing: false, doNotDisturb: true })).toContain("during Do Not Disturb");
    expect(studioAriaLabel(st({ engineLoading: true, doNotDisturb: true }))).toContain("Mochi hangs on the door");
  });
});

describe("neither Sal nor Mochi ever appears in the pill", () => {
  const ctx: VisitContext = { meetingRecording: false, screenSharing: false, doNotDisturb: false, takesSoFar: 0, occupant: "yappy", cloudConnected: true };

  it("sal_and_mochi_never_appear_in_the_pill", () => {
    for (const who of ["sal", "mochi"] as const) {
      expect(ROOM_ONLY).toContain(who);
      expect(PILL_CHARACTERS).not.toContain(who);
      expect(Object.values(EVENT_OWNER)).not.toContain(who);
      expect(forcedFriendVisit(`?friend=${who}`)).toBeNull();
      for (const e of FRIEND_EVENTS) {
        const d = decideVisit(e, ctx, EMPTY_LEDGER, Date.UTC(2026, 10, 7, 12), null, who);
        expect(d.show).toBe(false);
        // Even an unrequested visit is always someone else.
        const free = decideVisit(e, ctx, EMPTY_LEDGER, Date.UTC(2026, 10, 7, 12));
        if (free.show) expect(free.visit.friend).not.toBe(who);
      }
    }
  });

  it("no_pill_source_draws_sal_or_mochi", () => {
    const dir = new URL("../../pill/", import.meta.url);
    const files = (readdirSync(dir) as string[]).filter((f) => /\.tsx?$/.test(f) && !/\.test\./.test(f));
    expect(files.length).toBeGreaterThan(0);
    const sources = [...files.map((f) => ({ f, src: readFileSync(new URL(f, dir), "utf8") as string })), { f: "float-main.tsx", src: readFileSync(new URL("../../float-main.tsx", import.meta.url), "utf8") as string }];
    for (const { f, src } of sources) {
      for (const who of ["sal", "mochi"]) {
        // The only pill mention allowed is the ROOM_ONLY list that keeps them out.
        const hits = src.split("\n").filter((line) => line.includes(`"${who}"`) || line.includes(`'${who}'`) || line.includes(`FRIENDS.${who}`));
        const allowed = hits.filter((line) => /ROOM_ONLY/.test(line));
        expect({ f, who, hits: hits.length - allowed.length }).toEqual({ f, who, hits: 0 });
      }
    }
  });
});
