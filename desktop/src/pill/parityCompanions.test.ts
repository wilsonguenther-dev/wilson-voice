/**
 * yap-1107-parity-companions — the approved Companions page (companions.html
 * s04, "Hoot in the pill") is the source of truth for what Hoot wears and how
 * big he is drawn:
 *   - Listening: `hootWorld({ bare: true })` — Hoot WITHOUT headphones.
 *   - Meeting:   `hootWorld({})` — Hoot records it himself with his OWN
 *     headphones, his whole 40 x 40 sprite at 1:1 where Yappy stands.
 * These tests pin both, plus the whole-number scale and the no-crop rule.
 */
import { describe, expect, it } from "vitest";
import { Raster } from "../sprites/raster";
import { drawFriendFrame, friendManifest } from "../sprites/friends";
import { PILL_SCENE } from "./characterFit";
import { drawOccupant, occupantSpriteOpt } from "./occupant";
import { SCENE_H, SCENE_LAYOUT, SCENE_W, meetingScene } from "./meetingScene";

/** Hoot's headphone ear-cup red (friends-hd.js: used by the headphones only). */
const CUP_RED: readonly number[] = [0xc8, 0x44, 0x3a];

function countRgb(r: Raster, rgb: readonly number[]): number {
  let n = 0;
  for (let i = 0; i < r.data.length; i += 4) {
    if (r.data[i + 3] > 0 && r.data[i] === rgb[0] && r.data[i + 1] === rgb[1] && r.data[i + 2] === rgb[2]) n++;
  }
  return n;
}

const restPose = { hop: 0, blink: 0 };

describe("parity companions: Hoot in the pill (companions.html s04)", () => {
  it("parity_hoot_listens_bare_and_wears_headphones_only_for_a_meeting", () => {
    expect(occupantSpriteOpt("hoot")).toEqual({ bare: true });
    expect(occupantSpriteOpt("hoot", false)).toEqual({ bare: true });
    expect(occupantSpriteOpt("hoot", true)).toBeUndefined();
    for (const id of ["yappy", "tidy", "gulp", "shelly", null, undefined]) {
      expect(occupantSpriteOpt(id), String(id)).toBeUndefined();
      expect(occupantSpriteOpt(id, true), String(id)).toBeUndefined();
    }
  });

  it("parity_draw_friend_frame_extra_bare_drops_hoots_headphones", () => {
    const m = friendManifest("hoot");
    expect(m).not.toBeNull();
    const withPhones = new Raster(m!.w, m!.h);
    expect(drawFriendFrame(withPhones, "hoot", "idle")).toBe(true);
    const bare = new Raster(m!.w, m!.h);
    expect(drawFriendFrame(bare, "hoot", "idle", 0, { bare: true })).toBe(true);
    expect(countRgb(withPhones, CUP_RED)).toBeGreaterThan(0);
    expect(countRgb(bare, CUP_RED)).toBe(0);
    // The action frames keep their own layers and still honour `extra`.
    const act = new Raster(m!.w, m!.h);
    expect(drawFriendFrame(act, "hoot", "action", 1, { bare: true })).toBe(true);
    expect(countRgb(act, CUP_RED)).toBe(0);
  });

  it("parity_hoot_occupant_is_bare_while_listening_and_phones_on_in_a_meeting", () => {
    const listening = new Raster(PILL_SCENE.w, PILL_SCENE.h);
    expect(drawOccupant(listening, "hoot", "listening", restPose, 0, true)).toBe(true);
    expect(countRgb(listening, CUP_RED)).toBe(0);
    const meeting = new Raster(PILL_SCENE.w, PILL_SCENE.h);
    expect(drawOccupant(meeting, "hoot", "listening", restPose, 0, true, true)).toBe(true);
    expect(countRgb(meeting, CUP_RED)).toBeGreaterThan(0);
    // Other friends draw exactly as approved, meeting or not.
    const a = new Raster(PILL_SCENE.w, PILL_SCENE.h);
    const b = new Raster(PILL_SCENE.w, PILL_SCENE.h);
    drawOccupant(a, "tidy", "listening", restPose, 0, true, false);
    drawOccupant(b, "tidy", "listening", restPose, 0, true, true);
    expect(Array.from(a.data)).toEqual(Array.from(b.data));
  });

  it("parity_hoot_records_the_meeting_at_1x_whole_inside_the_scene", () => {
    const m = friendManifest("hoot")!;
    const hm = SCENE_LAYOUT.hootMain;
    // 1:1 — the whole sprite, never the fractional 24 px squeeze.
    expect(hm.size).toBe(m.w);
    expect(m.w).toBe(40);
    expect(m.h).toBe(40);
    // Fully inside the 112 x 40 buffer (no cropped character).
    expect(hm.x).toBeGreaterThanOrEqual(0);
    expect(hm.y).toBeGreaterThanOrEqual(0);
    expect(hm.x + m.w).toBeLessThanOrEqual(SCENE_W);
    expect(hm.y + m.h).toBeLessThanOrEqual(SCENE_H);
    // Centred where Yappy stands (cx 74).
    expect(hm.x + m.w / 2).toBe(SCENE_LAYOUT.yappy.cx);
    // Clear of the tape machine on his left.
    const t = SCENE_LAYOUT.tape;
    expect(hm.x).toBeGreaterThanOrEqual(t.x + t.w);
  });

  it("parity_hoot_meeting_recording_light_is_his_own_right_ear_cup", () => {
    const s = meetingScene({ elapsedS: 600, occupant: "hoot" });
    expect(s.hoot.visiting).toBe(false);
    expect(s.cup).toEqual({ ...SCENE_LAYOUT.hootCup });
    // The cup sits on Hoot's own sprite cup: sprite (cx + 10, cy - 8), cx 20, cy 23.
    const hm = SCENE_LAYOUT.hootMain;
    expect(s.cup.x).toBe(hm.x + 20 + 10);
    expect(s.cup.y).toBe(hm.y + 23 - 8);
    // And the painted cup red is exactly there on the 1:1 sprite.
    const r = new Raster(40, 40);
    drawFriendFrame(r, "hoot", "idle");
    const at = (x: number, y: number) => Array.from(r.data.slice((y * 40 + x) * 4, (y * 40 + x) * 4 + 3));
    expect(at(s.cup.x - hm.x, s.cup.y - hm.y + 3)).toEqual([...CUP_RED]);
    // Anyone else in the pill keeps the generic friend cup.
    expect(meetingScene({ elapsedS: 600, occupant: "tidy" }).cup).toEqual({ ...SCENE_LAYOUT.mainCup });
  });
});
