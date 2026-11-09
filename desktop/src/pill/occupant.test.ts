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
import { MAX_LIFT, OPEN_CAPSULE, PILL_SCENE, REST_CAPSULE, chickBox, pillCamera } from "./characterFit";
import { PILL_FRIENDS } from "./characters/friends";
import { OCCUPANT_BOTTOM, drawOccupant, isPillFriend, occupantBoxFor, occupantEnvelope, occupantFrame, occupantPlacement, occupantSpriteOpt, resetOccupantWarnings } from "./occupant";

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
        // yap-1107-parity-companions — Hoot listens bare (companions.html s04).
        expect(drawFriendFrame(direct, id, f.kind, f.index, occupantSpriteOpt(id))).toBe(true);
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

  it("occupant_camera_frames_a_taller_friend_whole_at_the_artifact_size", () => {
    // Loop three: Hoot is 40 art px tall, taller than the chick's clamped box at
    // the artifact's scale, so the camera frames the friend's own box (a smaller
    // whole cell when it has to), never Yappy's box with the friend's head cut.
    expect(occupantEnvelope("yappy")).toBeUndefined();
    expect(occupantEnvelope(null)).toBeUndefined();
    expect(occupantEnvelope("unicorn")).toBeUndefined();
    for (const id of PILL_FRIENDS) {
      const m = manifest(id);
      const env = occupantEnvelope(id);
      expect(env, id).toBeDefined();
      for (const lift of [0, 1, 3, MAX_LIFT]) {
        const box = occupantBoxFor(m, lift);
        for (const hop of [0, 1, 2, 5, 40]) {
          const at = occupantPlacement(m, hop, lift);
          expect(at.y >= box.y && at.y + at.h <= box.y + box.h && at.x >= box.x && at.x + at.w <= box.x + box.w, `${id} lift ${lift} hop ${hop}`).toBe(true);
          expect(at.y + at.h).toBeLessThanOrEqual(OCCUPANT_BOTTOM);
        }
      }
      const shells = { rest: { w: REST_CAPSULE.w, h: REST_CAPSULE.h }, open: { w: OPEN_CAPSULE.w, h: OPEN_CAPSULE.h } };
      for (const phase of ["rest", "open"] as const) for (const dpr of [1, 2]) {
        const cam = pillCamera({ x: 0, y: 0, ...shells[phase] }, dpr, env);
        expect(cam.fits, `${id} ${phase} dpr=${dpr}`).toBe(true);
        expect(Number.isInteger(cam.blit.cell), `${id} ${phase} dpr=${dpr}`).toBe(true);
        // The framed box is the friend's whole box at the lift the camera chose.
        const box = occupantBoxFor(m, cam.lift);
        expect(cam.char.h, `${id} ${phase} dpr=${dpr}`).toBe(box.h * cam.blit.cell);
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
