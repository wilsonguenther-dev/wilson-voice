/**
 * yap-1106-yappy-states-and-motion — hands-free gets a DISTINCT pose (wings
 * folded across the belly, no pencil), and leaving the flag off draws exactly
 * the pixels it always did.
 */
import { describe, expect, it } from "vitest";
import { CHICK_BOX, drawChick, type ChickPose, type PixelCtx } from "./art";

function paint(pose: ChickPose): string[] {
  const ops: string[] = [];
  const o: PixelCtx = {
    fillStyle: "#000",
    globalAlpha: 1,
    fillRect(x, y, w, h) { ops.push(`${String(this.fillStyle)}@${this.globalAlpha}:${x},${y},${w},${h}`); },
  };
  drawChick(o, CHICK_BOX.cx, CHICK_BOX.feetY, pose);
  return ops;
}

const BASE: ChickPose = {
  hop: 0, sx: 0, sy: 0, blink: 0, mood: 0, beakF: 0, sway: 0, flap: 0,
  look: "fwd", glasses: false, pencil: true, droop: false,
};

describe("hands-free pose", () => {
  it("hands_free_pose_is_distinct_from_listening", () => {
    const listening = paint(BASE);
    const handsFree = paint({ ...BASE, handsFree: true });
    expect(handsFree).not.toEqual(listening);
  });

  it("hands_free_off_is_pixel_identical_to_the_old_pose", () => {
    expect(paint({ ...BASE, handsFree: false })).toEqual(paint(BASE));
  });
});
