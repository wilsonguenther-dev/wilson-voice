/**
 * yap-1106-fb-02-pill-first-click — every pill button works on the first click.
 * Pins the webview half: the click-through region is the LIST of rects the
 * controls draw in (each at least 24 x 24 pt), the centre and four inner
 * corners of every control are inside it in every dock, a press holds the panel,
 * and every click logs `pill.click {control, handled}`. The browser half (real
 * clicks on real elements) is scripts/pill-matrix.mjs --clicks; the AppKit half
 * is float_pill.rs `every_control_centre_and_inner_corner_is_a_hit`.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Rect4 } from "./drag";

const invoke = vi.fn((..._args: unknown[]) => Promise.resolve());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

type Drag = typeof import("./drag");
type Hit = typeof import("./hitTargets");
let drag: Drag;
let hit: Hit;

function lastList(): Rect4[] {
  const calls = invoke.mock.calls.filter((c) => c[0] === "pill_set_hitboxes");
  if (!calls.length) return [];
  const { rects } = calls[calls.length - 1][1] as { rects: { x: number; y: number; w: number; h: number }[] };
  return rects.map((r) => [r.x, r.y, r.w, r.h] as Rect4);
}

beforeEach(async () => {
  vi.resetModules();
  invoke.mockClear();
  drag = await import("./drag");
  hit = await import("./hitTargets");
});
afterEach(() => vi.unstubAllGlobals());

// One geometry per dock: the capsule, the meeting stop in the clock slot, the
// Trial-ended chip and a card's button — where yap-1106-one-pill lays them out.
const DOCKS: Record<string, { capsule: Rect4; controls: Record<string, Rect4> }> = {
  bottom: {
    capsule: [114, 67, 72, 22],
    controls: { "meeting-stop": [196, 68, 20, 20], "trial-ended": [100, 40, 100, 26], "copy-card-close": [262, 8, 18, 18] },
  },
  left: {
    capsule: [10, 59, 72, 22],
    controls: { "meeting-stop": [30, 90, 20, 20], "trial-ended": [6, 20, 96, 26], "copy-card-close": [80, 8, 18, 18] },
  },
  right: {
    capsule: [218, 59, 72, 22],
    controls: { "meeting-stop": [250, 90, 20, 20], "trial-ended": [198, 20, 96, 26], "copy-card-close": [270, 8, 18, 18] },
  },
};

describe("padToMin / probePoints", () => {
  it("grows a control under 24 x 24 pt about its centre and leaves a big one alone", () => {
    expect(hit.padToMin([196, 68, 20, 20])).toEqual([194, 66, 24, 24]);
    expect(hit.padToMin([0, 0, 100, 26])).toEqual([0, 0, 100, 26]);
    expect(hit.MIN_HIT_PT).toBe(24);
  });
  it("is the centre plus the four inner corners, every one inside the control", () => {
    const pts = hit.probePoints([10, 20, 30, 24]);
    expect(pts).toEqual([
      [25, 32],
      [11, 21],
      [39, 21],
      [11, 43],
      [39, 43],
    ]);
    for (const [x, y] of pts) expect(hit.pointInRects(x, y, [[10, 20, 30, 24]])).toBe(true);
  });
});

describe("the click-through region is the controls' own rects", () => {
  for (const [dock, g] of Object.entries(DOCKS)) {
    it(`(${dock} dock) centre + 4 inner corners of every control are hits; empty glass is not`, () => {
      drag.reportPillHitbox(...g.capsule);
      const padded = Object.values(g.controls).map((r) => hit.padToMin(r));
      drag.extendPillHitboxRects(padded, drag.CONTROLS_OWNER);
      const list = lastList();
      expect(list).toHaveLength(1 + padded.length);
      for (const r of list.slice(1)) {
        expect(r[2]).toBeGreaterThanOrEqual(24);
        expect(r[3]).toBeGreaterThanOrEqual(24);
      }
      for (const [name, r] of Object.entries(g.controls)) {
        for (const [x, y] of hit.probePoints(r)) {
          expect(hit.pointInRects(x, y, list), `${dock} ${name} @${x},${y}`).toBe(true);
        }
      }
      for (const [x, y] of hit.probePoints(g.capsule)) expect(hit.pointInRects(x, y, list)).toBe(true);
      // A corner of the panel no control covers stays click-through glass.
      expect(hit.pointInRects(1, 139, list)).toBe(false);
    });
  }

  it("keeps the controls reachable while a card holds the hitbox, and drops them on null", () => {
    drag.reportPillHitbox(114, 67, 72, 22);
    drag.extendPillHitboxRects([[194, 66, 24, 24]], drag.CONTROLS_OWNER);
    drag.holdPillHitbox({ x: 10, y: 6, w: 280, h: 60 }, "copy-card");
    let list = lastList();
    expect(list).toContainEqual([10, 6, 280, 60]);
    expect(list).toContainEqual([194, 66, 24, 24]);
    expect(list).not.toContainEqual([114, 67, 72, 22]);
    drag.holdPillHitbox(null, "copy-card");
    drag.extendPillHitboxRects(null, drag.CONTROLS_OWNER);
    list = lastList();
    expect(list).toEqual([[114, 67, 72, 22]]);
  });

  it("still sends the legacy bounding union for older listeners", () => {
    drag.reportPillHitbox(114, 67, 72, 22);
    drag.extendPillHitboxRects([[194, 66, 24, 24]], drag.CONTROLS_OWNER);
    const legacy = invoke.mock.calls.filter((c) => c[0] === "pill_set_hitbox");
    expect(legacy[legacy.length - 1][1]).toEqual({ x: 114, y: 66, w: 104, h: 24 });
  });
});

describe("measureControls", () => {
  it("measures every visible control, padded, and skips hidden ones", () => {
    vi.stubGlobal("getComputedStyle", (el: { hidden?: boolean }) => ({
      visibility: el.hidden ? "hidden" : "visible",
      pointerEvents: "auto",
    }));
    const el = (r: Rect4, hidden = false) => ({
      hidden,
      getClientRects: () => [1],
      getBoundingClientRect: () => ({ left: r[0], top: r[1], width: r[2], height: r[3] }),
    });
    const root = {
      querySelectorAll: (sel: string) => (sel === hit.CONTROL_SELECTOR ? [el([196, 68, 20, 20]), el([0, 0, 50, 30], true), el([5, 5, 0, 0])] : []),
    } as unknown as ParentNode;
    expect(hit.measureControls(root)).toEqual([[194, 66, 24, 24]]);
  });
});

describe("press hold + pill.click telemetry", () => {
  class FakeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  function fakeDoc(control: { name: string; disabled: boolean }) {
    const handlers: Record<string, ((e: unknown) => void)[]> = {};
    const on = (t: string, fn: (e: unknown) => void) => (handlers[t] ||= []).push(fn);
    const target = {
      closest: (sel: string) =>
        sel === hit.CONTROL_SELECTOR
          ? { disabled: control.disabled, getAttribute: (a: string) => (a === hit.CONTROL_ATTR ? control.name : null) }
          : null,
    };
    const win = { addEventListener: on, removeEventListener() {} };
    const doc = {
      defaultView: win,
      documentElement: {},
      querySelectorAll: () => [],
      addEventListener: on,
      removeEventListener() {},
    };
    const fire = (t: string, e: Record<string, unknown>) => (handlers[t] || []).forEach((fn) => fn({ target, button: 0, ...e }));
    return { doc: doc as unknown as Document, fire };
  }

  it("holds the panel from pointer-down to pointer-up and logs one pill.click per click", () => {
    vi.stubGlobal("ResizeObserver", FakeObserver);
    vi.stubGlobal("MutationObserver", FakeObserver);
    const { doc, fire } = fakeDoc({ name: "meeting-stop", disabled: false });
    const stop = hit.startPillControlTargets(doc);
    fire("pointerdown", {});
    fire("pointerup", {});
    fire("click", {});
    const calls = invoke.mock.calls.filter((c) => c[0] !== "pill_set_hitbox" && c[0] !== "pill_set_hitboxes");
    expect(calls).toEqual([
      ["pill_press_hold", { held: true }],
      ["pill_press_hold", { held: false }],
      ["pill_click_log", { control: "meeting-stop", handled: true }],
    ]);
    stop();
  });

  it("logs handled=false for a disabled control", () => {
    vi.stubGlobal("ResizeObserver", FakeObserver);
    vi.stubGlobal("MutationObserver", FakeObserver);
    const { doc, fire } = fakeDoc({ name: "language-offer-add", disabled: true });
    const stop = hit.startPillControlTargets(doc);
    fire("click", {});
    expect(invoke.mock.calls.filter((c) => c[0] === "pill_click_log")).toEqual([
      ["pill_click_log", { control: "language-offer-add", handled: false }],
    ]);
    stop();
  });
});
