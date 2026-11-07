/**
 * yap-1106-main-character-switch — the batch-10 bounce fix: a friend in the
 * pill sits in the ONE pill stage (YappyPill), painted at Yappy's spot, never
 * in a second (legacy 300 x 140) capsule.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(() => Promise.resolve()),
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import { Raster } from "../sprites/raster";
import { drawFriendFrame, friendManifest, type FriendManifest } from "../sprites/friends";
import { ALL_LIVE_PHASES } from "./live";
import { PILL_SCENE, chickBox } from "./characterFit";
import { PILL_FRIENDS } from "./characters/friends";
import { OCCUPANT_BOTTOM, drawOccupant, isPillFriend, occupantFrame, occupantPlacement, resetOccupantWarnings } from "./occupant";

const read = (rel: string): string => readFileSync(new URL(rel, import.meta.url), "utf8") as string;
const manifest = (id: string): FriendManifest => {
  const m = friendManifest(id);
  if (!m) throw new Error(`no manifest for ${id}`);
  return m;
};
const alphaAt = (r: Raster, x: number, y: number): number => r.data[(y * r.width + x) * 4 + 3];
const rgbaAt = (r: Raster, x: number, y: number): number[] => Array.from(r.data.slice((y * r.width + x) * 4, (y * r.width + x) * 4 + 4));

afterEach(() => {
  vi.restoreAllMocks();
  resetOccupantWarnings();
});

describe("who is painted in the one pill (yap-1106-main-character-switch)", () => {
  it("occupant_only_the_four_pill_friends_are_friends", () => {
    for (const id of PILL_FRIENDS) expect(isPillFriend(id), id).toBe(true);
    for (const id of ["yappy", "classic", "ember", "sal", "mochi", "cinder", "", null, undefined]) expect(isPillFriend(id as string | null | undefined), String(id)).toBe(false);
  });

  it("occupant_yappy_or_unknown_paints_nothing_so_the_stage_draws_the_chick", () => {
    const r = new Raster(PILL_SCENE.w, PILL_SCENE.h);
    for (const id of ["yappy", null, undefined, "unicorn"]) {
      expect(drawOccupant(r, id, "listening", { hop: 0, blink: 0 }, 0, false), String(id)).toBe(false);
    }
    expect(r.data.some((v) => v !== 0)).toBe(false);
  });

  it("occupant_placement_stays_inside_yappys_box_for_every_friend_and_hop", () => {
    const b = chickBox();
    for (const id of PILL_FRIENDS) {
      const m = manifest(id);
      for (const hop of [0, 1, 2.6, 5, 40, -3, Number.NaN]) {
        const at = occupantPlacement(m, hop);
        expect(Number.isInteger(at.x) && Number.isInteger(at.y), `${id} hop ${hop}`).toBe(true);
        expect(at.x >= b.x && at.y >= b.y, `${id} hop ${hop}`).toBe(true);
        expect(at.x + at.w <= b.x + b.w && at.y + at.h <= b.y + b.h, `${id} hop ${hop}`).toBe(true);
        expect(at.y + at.h).toBeLessThanOrEqual(OCCUPANT_BOTTOM);
      }
      // Standing still, its last row sits on the shadow band.
      expect(occupantPlacement(m, 0).y + m.h).toBe(OCCUPANT_BOTTOM);
    }
  });

  it("occupant_every_friend_paints_crisp_1to1_in_every_one_of_the_17_states", () => {
    expect(ALL_LIVE_PHASES.length).toBe(17);
    for (const id of PILL_FRIENDS) {
      const m = manifest(id);
      for (const phase of ALL_LIVE_PHASES) {
        const scene = new Raster(PILL_SCENE.w, PILL_SCENE.h);
        expect(drawOccupant(scene, id, phase, { hop: 0, blink: 0 }, 0, false), `${id}/${phase}`).toBe(true);
        const f = occupantFrame(m, phase, 0, 0, false);
        const direct = new Raster(m.w, m.h);
        expect(drawFriendFrame(direct, id, f.kind, f.index)).toBe(true);
        const at = occupantPlacement(m, 0);
        let painted = 0;
        for (let y = 0; y < m.h; y++) {
          for (let x = 0; x < m.w; x++) {
            if (alphaAt(direct, x, y) > 0) painted++;
            // Whole-pixel placement: the scene crop IS the sprite, pixel for pixel.
            expect(rgbaAt(scene, at.x + x, at.y + y), `${id}/${phase} @${x},${y}`).toEqual(rgbaAt(direct, x, y));
          }
        }
        expect(painted, `${id}/${phase}`).toBeGreaterThan(50);
      }
    }
  });

  it("occupant_work_alternates_action_frames_but_reduce_motion_holds_one", () => {
    const m = manifest("gulp");
    const beat = m.action.frameMs;
    expect(occupantFrame(m, "transcribing", 0, 0, false)).toEqual({ kind: "action", index: 0 });
    expect(occupantFrame(m, "transcribing", 0, beat + 1, false)).toEqual({ kind: "action", index: 1 });
    expect(occupantFrame(m, "transcribing", 0, beat + 1, true)).toEqual({ kind: "action", index: 0 });
    // `done` is a held proud pose, never a loop.
    expect(occupantFrame(m, "done", 0, beat + 1, false)).toEqual({ kind: "action", index: 0 });
    // A resting pose blinks with the stage; reduce motion never blinks.
    expect(occupantFrame(m, "idle", 1, 0, false)).toEqual({ kind: "blink", index: 0 });
    expect(occupantFrame(m, "idle", 1, 0, true)).toEqual({ kind: "idle", index: 0 });
    expect(occupantFrame(m, "idle", 0, 0, false)).toEqual({ kind: "idle", index: 0 });
  });

  it("occupant_the_shell_hands_the_one_stage_its_occupant_and_the_stage_paints_it", () => {
    const shell = read("../float-main.tsx");
    expect(shell).toMatch(/<Stage \{\.\.\.pillProps\}[^>]*occupant=\{character\.id\}/);
    const stage = read("./YappyPill.tsx");
    expect(stage).toMatch(/drawOccupant\(octx, occupantRef\.current, gateRef\.current/);
    expect(stage).toMatch(/data-occupant=\{occupant \?\? "yappy"\}/);
    expect(stage).toMatch(/<MeetingBadge [^>]*occupant=\{occupant \?\? "yappy"\}/);
    // The matrix asks every character for the one-pill stage, never a legacy capsule.
    const matrix = read("../../../scripts/pill-matrix.mjs");
    expect(matrix).toMatch(/const CHARACTERS = \["yappy", "hoot", "tidy", "gulp", "shelly"\]/);
    expect(matrix).toMatch(/capsule\.dataset\.occupant !== want\.pill/);
  });
});
