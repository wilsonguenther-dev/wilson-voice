/**
 * yap-wf-02-the-pill-shrinks-to — WF-02: the pill shrinks to a blob while you
 * drag it, Esc mid-drag restores the old dock, and Reduce Motion swaps the
 * animation for an instant change. The reducer is pure, so the whole gesture
 * is pinned here without a DOM, a window or a backend.
 */
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import {
  applyDragVisual,
  BLOB_PT,
  currentDock,
  DRAG_IDLE,
  dragReduce,
  EDGE_SNAP_PT,
  forcedDragScene,
  settleVisual,
  snapTarget,
  type DragEffect,
  type DragEvent,
  type DragState,
  type PillDock,
} from "./drag";
import { matrixDragBlobScene } from "./forced";

/** A 1440-point-wide primary monitor at the global origin. */
const FRAME = { frameX: 0, frameW: 1440 };

function run(events: DragEvent[], from: DragState = DRAG_IDLE): { state: DragState; effects: DragEffect[] } {
  let state = from;
  const effects: DragEffect[] = [];
  for (const e of events) {
    const r = dragReduce(state, e);
    state = r.state;
    effects.push(...r.effects);
  }
  return { state, effects };
}

const down = (dock: PillDock, x = 720, y = 860): DragEvent => ({ type: "down", x, y, dock });
const move = (x: number, y = 860): DragEvent => ({ type: "move", x, y, ...FRAME });

describe("WF-02 drag reducer — blob on drag", () => {
  it("a press under the slop stays a press: no blob, no move", () => {
    const { state, effects } = run([down("bottom"), move(722)]);
    expect(state.phase).toBe("pressed");
    expect(effects).toEqual([{ kind: "start" }]);
  });

  it("passing the slop turns the pill into the blob exactly once", () => {
    const { state, effects } = run([down("bottom"), move(760), move(800)]);
    expect(state.phase).toBe("blob");
    expect(effects.filter((f) => f.kind === "blob")).toHaveLength(1);
    expect(effects.filter((f) => f.kind === "move")).toEqual([
      { kind: "move", dx: 40, dy: 0 },
      { kind: "move", dx: 80, dy: 0 },
    ]);
  });

  it("the lit drop zone follows the pointer across all three docks", () => {
    expect(run([down("bottom"), move(40)]).state.target).toBe("left");
    expect(run([down("bottom"), move(1400)]).state.target).toBe("right");
    expect(run([down("left", 40), move(700)]).state.target).toBe("bottom");
  });

  it("a release snaps to the lit zone and settles there", () => {
    const { state, effects } = run([down("bottom"), move(30), { type: "up", x: 30, y: 860 }]);
    expect(state.phase).toBe("idle");
    expect(effects.slice(-2)).toEqual([
      { kind: "end", x: 30, y: 860, snap: true },
      { kind: "settle", dock: "left" },
    ]);
  });

  it("the blob is Yappy's face only: 28 points", () => {
    expect(BLOB_PT).toBe(28);
  });
});

describe("WF-02 drag reducer — Esc mid-drag restores the old dock", () => {
  for (const from of ["bottom", "left", "right"] as PillDock[]) {
    it(`Esc while dragging away from ${from} puts the pill back on ${from}`, () => {
      const { state, effects } = run([down(from, 720), move(30), move(1420), { type: "escape" }]);
      expect(state.phase).toBe("cancelled");
      expect(state.target).toBe(from);
      expect(state.cancelled).toBe(true);
      // The backend re-parks on the dock it already has (snap: false) — it
      // never sees the release point the pill was dragged to.
      expect(effects.slice(-2)).toEqual([
        { kind: "end", x: 720, y: 860, snap: false },
        { kind: "restore", dock: from },
      ]);
    });
  }

  it("after Esc, further moves and the release are inert — no second dock, no snap", () => {
    const { state, effects } = run([
      down("right", 1400),
      move(30),
      { type: "escape" },
      move(20),
      { type: "up", x: 20, y: 860 },
    ]);
    const tail = effects.slice(effects.findIndex((f) => f.kind === "restore") + 1);
    expect(tail).toEqual([]);
    expect(state.phase).toBe("idle");
    expect(effects.filter((f) => f.kind === "end")).toHaveLength(1);
    expect(effects.some((f) => f.kind === "end" && f.snap)).toBe(false);
  });

  it("Esc before the slop spends the press without a restore", () => {
    const { state, effects } = run([down("bottom"), { type: "escape" }, { type: "up", x: 720, y: 860 }]);
    expect(state.cancelled).toBe(true);
    expect(effects).toEqual([{ kind: "start" }, { kind: "end", x: 720, y: 860, snap: false }]);
  });

  it("Esc at rest does nothing", () => {
    expect(dragReduce(DRAG_IDLE, { type: "escape" })).toEqual({ state: DRAG_IDLE, effects: [] });
  });

  it("a system pointer-cancel mid-drag also goes home, never to the drag point", () => {
    const { effects } = run([down("left", 40), move(1420), { type: "cancel" }]);
    expect(effects.slice(-2)).toEqual([
      { kind: "end", x: 40, y: 860, snap: false },
      { kind: "restore", dock: "left" },
    ]);
  });

  it("the next press starts clean after a cancelled one", () => {
    const after = run([down("left", 40), move(1420), { type: "escape" }, { type: "up", x: 1420, y: 860 }]).state;
    const { state } = run([down("left", 40)], after);
    expect(state).toMatchObject({ phase: "pressed", moved: false, cancelled: false, from: "left" });
  });
});

describe("snapTarget mirrors float_pill.rs snap_position", () => {
  it("edges within EDGE_SNAP_PT are side docks, the rest is the bottom island", () => {
    expect(snapTarget(EDGE_SNAP_PT, 0, 1440)).toBe("left");
    expect(snapTarget(EDGE_SNAP_PT + 1, 0, 1440)).toBe("bottom");
    expect(snapTarget(1440 - EDGE_SNAP_PT, 0, 1440)).toBe("right");
    expect(snapTarget(1300, 1440, 1440)).toBe("left"); // a frame that does not start at 0
  });
  it("an unknown frame lands on the bottom island", () => {
    expect(snapTarget(10, 0, 0)).toBe("bottom");
  });
});

/** A stand-in for <html>: only `dataset` is touched. */
function fakeRoot(dock?: string): HTMLElement {
  const dataset: Record<string, string> = {};
  if (dock) dataset.dock = dock;
  return { dataset } as unknown as HTMLElement;
}

describe("WF-02 Reduce Motion swaps the animation for an instant change", () => {
  it("under Reduce Motion the drop never squashes: straight back to rest", () => {
    expect(settleVisual(true)).toBe("rest");
    expect(settleVisual(false)).toBe("squash");
  });

  it("the blob is published with motion=instant under Reduce Motion, animate otherwise", () => {
    const r = fakeRoot("bottom");
    applyDragVisual(r, "blob", "left", true);
    expect(r.dataset).toMatchObject({ pillDrag: "blob", pillDragTarget: "left", pillDragMotion: "instant" });
    applyDragVisual(r, "blob", "right", false);
    expect(r.dataset).toMatchObject({ pillDrag: "blob", pillDragTarget: "right", pillDragMotion: "animate" });
  });

  it("rest clears the blob and its drop zone", () => {
    const r = fakeRoot();
    applyDragVisual(r, "blob", "left", false);
    applyDragVisual(r, "rest", null, false);
    expect(r.dataset.pillDrag).toBeUndefined();
    expect(r.dataset.pillDragTarget).toBeUndefined();
  });
});

describe("dock + matrix plumbing", () => {
  it("currentDock reads <html data-dock>, defaulting to bottom", () => {
    expect(currentDock(fakeRoot("left"))).toBe("left");
    expect(currentDock(fakeRoot("right"))).toBe("right");
    expect(currentDock(fakeRoot("nonsense"))).toBe("bottom");
    expect(currentDock(null)).toBe("bottom");
  });
  it("the pill matrix photographs the blob scene (every dock x character x meeting)", () => {
    const row = matrixDragBlobScene();
    expect(row).toMatchObject({ id: "drag-blob", phase: "idle", query: "drag=blob", drag: "blob" });
    expect(forcedDragScene("?phase=idle&" + row.query)).toBe("blob");
  });
  it("forcedDragScene pins the blob only for drag=blob", () => {
    expect(forcedDragScene("?phase=idle&drag=blob")).toBe("blob");
    expect(forcedDragScene("?phase=idle")).toBeNull();
  });
});
