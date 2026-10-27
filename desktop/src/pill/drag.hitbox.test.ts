/**
 * yap24-NT7 fix — Yappy's meeting badge sits outside the canvas capsule, so the
 * hitbox the float panel uses for click-through must cover BOTH while the badge
 * is up. These pin the drag.ts contract the pill relies on.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn((..._args: unknown[]) => Promise.resolve());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

type Drag = typeof import("./drag");
let drag: Drag;

function lastHitbox() {
  const calls = invoke.mock.calls.filter((c) => c[0] === "pill_set_hitbox");
  return calls.length ? calls[calls.length - 1][1] : null;
}

beforeEach(async () => {
  vi.resetModules();
  invoke.mockClear();
  drag = await import("./drag");
});

describe("unionHitbox", () => {
  it("is the bounding box of both rects", () => {
    expect(drag.unionHitbox([114, 67, 72, 22], [90, 113, 120, 21])).toEqual([90, 67, 120, 67]);
  });
  it("ignores a missing or empty side", () => {
    expect(drag.unionHitbox([1, 2, 3, 4], null)).toEqual([1, 2, 3, 4]);
    expect(drag.unionHitbox(null, [1, 2, 3, 4])).toEqual([1, 2, 3, 4]);
    expect(drag.unionHitbox([1, 2, 3, 4], [50, 50, 0, 0])).toEqual([1, 2, 3, 4]);
  });
});

describe("extendPillHitbox", () => {
  it("adds the badge rect to the capsule's hitbox, so a click on the badge is not passed through", () => {
    drag.reportPillHitbox(114, 67, 72, 22); // Yappy idle capsule
    expect(lastHitbox()).toEqual({ x: 114, y: 67, w: 72, h: 22 });
    drag.extendPillHitbox({ x: 90, y: 113, w: 120, h: 21 }); // the badge, bottom: 6px
    expect(lastHitbox()).toEqual({ x: 90, y: 67, w: 120, h: 67 });
  });
  it("keeps the union as the capsule animates, and drops it when the badge goes", () => {
    drag.reportPillHitbox(114, 67, 72, 22);
    drag.extendPillHitbox({ x: 90, y: 113, w: 120, h: 21 });
    drag.reportPillHitbox(110, 66, 80, 24);
    expect(lastHitbox()).toEqual({ x: 90, y: 66, w: 120, h: 68 });
    drag.extendPillHitbox(null);
    expect(lastHitbox()).toEqual({ x: 110, y: 66, w: 80, h: 24 });
  });
  it("a held rect (Copy card) still wins, and release restores capsule + badge", () => {
    drag.reportPillHitbox(114, 67, 72, 22);
    drag.extendPillHitbox({ x: 90, y: 113, w: 120, h: 21 });
    drag.holdPillHitbox({ x: 10, y: 10, w: 280, h: 120 });
    expect(lastHitbox()).toEqual({ x: 10, y: 10, w: 280, h: 120 });
    drag.extendPillHitbox({ x: 90, y: 112, w: 130, h: 22 });
    expect(lastHitbox()).toEqual({ x: 10, y: 10, w: 280, h: 120 });
    drag.holdPillHitbox(null);
    expect(lastHitbox()).toEqual({ x: 90, y: 67, w: 130, h: 67 });
  });
});
