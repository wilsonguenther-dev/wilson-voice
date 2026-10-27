/**
 * Y5-E — hover hysteresis and the alpha hot area, pure over the
 * `hoverState(rect, cursorPath)` reducer (hitbox.ts).
 *
 * Wispr parity P0 #3: "simulated pointer dwell at the dock edge produces <=1
 * state transition". The geometry below is the float window as it ships
 * (float_pill.rs PILL_W x PILL_H = 300 x 140) and the Classic capsule as
 * float.css lays it out: a 56 x 16 seed at rest, a 200 x 44 radio pill
 * expanded, vertically centred, 10 px off the screen edge on the side docks
 * (`:root[data-dock] .stage { padding: 10px }`), and growing AWAY from that
 * edge — which is exactly why the edge-side boundary is shared by both states
 * and why a naive hover flaps on it.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
// No @types/node in this project (see live.test.ts).
// @ts-expect-error -- node builtin, untyped here; the test runs under Node.
import { readFileSync } from "node:fs";
import {
  contains, hotRect, hoverState, inflate, clip,
  type HoverConfig, type HoverInput, type PillGeometry, type Rect,
} from "./hitbox";
import { HOVER } from "./motion";

const invoke = vi.fn((..._args: unknown[]) => Promise.resolve());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const FLOAT: Rect = { x: 0, y: 0, w: 300, h: 140 };
const PAD = 10;
const SEED = { w: 56, h: 16 };
const OPEN = { w: 200, h: 44 };
const cy = (h: number) => (FLOAT.h - h) / 2;

type Dock = "bottom" | "left" | "right";
const DOCKS: Dock[] = ["bottom", "left", "right"];

function geometry(dock: Dock): PillGeometry {
  const at = (sz: { w: number; h: number }): Rect => {
    const x = dock === "left" ? PAD : dock === "right" ? FLOAT.w - PAD - sz.w : (FLOAT.w - sz.w) / 2;
    return { x, y: cy(sz.h), w: sz.w, h: sz.h };
  };
  return { collapsed: at(SEED), expanded: at(OPEN), viewport: FLOAT };
}

/**
 * The boundary of the COLLAPSED capsule that faces the dock edge, as an
 * (inside, outside) pixel pair one pixel apart. Half-open rects: x === right
 * edge is outside.
 */
function dockEdgePair(dock: Dock): { inside: [number, number]; outside: [number, number] } {
  const c = geometry(dock).collapsed;
  const midY = c.y + c.h / 2;
  if (dock === "left") return { inside: [c.x, midY], outside: [c.x - 1, midY] };
  if (dock === "right") return { inside: [c.x + c.w - 1, midY], outside: [c.x + c.w, midY] };
  const midX = c.x + c.w / 2;
  return { inside: [midX, c.y + c.h - 1], outside: [midX, c.y + c.h] };
}

/** A 60 Hz dwell that crosses `pair` by one pixel every frame for `ms`. */
function jitter(pair: { inside: [number, number]; outside: [number, number] }, ms: number, t0 = 0): HoverInput[] {
  const path: HoverInput[] = [];
  for (let i = 0, t = t0; t <= t0 + ms; i++, t += 16) {
    const [x, y] = i % 2 === 0 ? pair.inside : pair.outside;
    path.push({ at: t, x, y });
  }
  return path;
}

/** What main had before Y5-E: hovered = cursor on the capsule, no margin, no debounce. */
const NAIVE: HoverConfig = { marginPx: 0, collapseAfterMs: 0 };

describe("Y5-E hover hysteresis", () => {
  for (const dock of DOCKS) {
    it(`dwell_at_the_dock_edge_produces_at_most_one_transition (${dock} dock)`, () => {
      // 2 s of one-pixel jitter across the collapsed boundary — far longer than
      // the collapse debounce, so the margin (geometry), not the delay, holds it.
      const path = jitter(dockEdgePair(dock), 2000);
      expect(path.length).toBeGreaterThan(100);
      const s = hoverState(geometry(dock), path);
      expect(s.transitions).toBeLessThanOrEqual(1);
      expect(s.transitions).toBe(1); // it DID expand: the cursor was on the pill
      expect(s.phase).toBe("expanded");
    });
  }

  it("the same dwell flaps a naive hover on the side docks (the test can fail)", () => {
    // The side docks grow away from the screen edge, so the edge-side boundary
    // is shared by both states: without the margin every pixel is a flip.
    for (const dock of ["left", "right"] as const) {
      const s = hoverState(geometry(dock), jitter(dockEdgePair(dock), 2000), NAIVE);
      expect(s.transitions).toBeGreaterThan(100);
    }
  });

  for (const dock of DOCKS) {
    it(`jitter across the EXPANDED hot boundary never collapses it; leaving for good collapses once (${dock} dock)`, () => {
      const g = geometry(dock);
      const hot = hotRect(g.expanded, "expanded", g.viewport);
      const midX = hot.x + hot.w / 2;
      const enter: HoverInput = { at: 0, x: g.collapsed.x + 1, y: g.collapsed.y + 1 };
      // Top edge of the hot rect (it is inside the window on every dock).
      const flap = jitter({ inside: [midX, hot.y], outside: [midX, hot.y - 1] }, 1000, 16);
      const s = hoverState(g, [enter, ...flap]);
      expect(s.transitions).toBe(1);
      expect(s.phase).toBe("expanded");

      const gone: HoverInput[] = [{ at: 2000, gone: true }, { at: 2000 + HOVER.collapseAfterMs - 1 }];
      expect(hoverState(g, [enter, ...flap, ...gone]).phase).toBe("expanded");
      const done = hoverState(g, [enter, ...flap, ...gone, { at: 2000 + HOVER.collapseAfterMs }]);
      expect(done.phase).toBe("collapsed");
      expect(done.transitions).toBe(2);
    });
  }

  it("expands on the FIRST sample on the capsule — hysteresis is geometry, not an expand delay", () => {
    const g = geometry("bottom");
    const s = hoverState(g, [{ at: 0, x: g.collapsed.x + 2, y: g.collapsed.y + 2 }]);
    expect(s.phase).toBe("expanded");
  });

  for (const dock of DOCKS) {
    it(`collapsed_pill_has_no_margin_dead_zone (${dock} dock)`, () => {
      const g = geometry(dock);
      const c = g.collapsed;
      // The collapsed hot rect IS the capsule: not one pixel of margin.
      expect(hotRect(c, "collapsed", g.viewport)).toEqual(c);
      // One pixel outside every side of the idle capsule never takes the pill.
      const ring: Array<[number, number]> = [
        [c.x - 1, c.y + c.h / 2], [c.x + c.w, c.y + c.h / 2],
        [c.x + c.w / 2, c.y - 1], [c.x + c.w / 2, c.y + c.h],
      ];
      for (const [x, y] of ring) {
        const s = hoverState(g, [{ at: 0, x, y }, { at: 500, x, y }]);
        expect(s.phase, `(${x},${y})`).toBe("collapsed");
        expect(s.transitions).toBe(0);
      }
      // ...and a point that WAS in the expanded margin stops counting once the
      // pill has collapsed: the margin does not outlive the state that paints it.
      const marginPt: [number, number] = [g.expanded.x + g.expanded.w / 2, g.expanded.y - HOVER.marginPx / 2];
      const s = hoverState(g, [
        { at: 0, x: c.x + 1, y: c.y + 1 },
        { at: 10, gone: true },
        { at: 10 + HOVER.collapseAfterMs },
        { at: 1000, x: marginPt[0], y: marginPt[1] },
      ]);
      expect(s.phase).toBe("collapsed");
      expect(s.transitions).toBe(2);
    });
  }

  it("collapsed_pill_has_no_margin_dead_zone — float.css paints the margin ONLY in the expanded state", () => {
    const css = stripComments(readFileSync(new URL("../float.css", import.meta.url), "utf8") as string);
    const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map((m) => ({ sel: m[1].trim(), body: m[2] }));
    const margin = rules.filter((r) => /inset:\s*-12px/.test(r.body));
    expect(margin.length).toBe(1);
    expect(margin[0].sel).toBe('.pill-hot[data-hover="expanded"]::before');
    // No unconditional hot-area pseudo-element anywhere.
    expect(rules.some((r) => /^\.pill-hot(::before|::after)$/.test(r.sel))).toBe(false);
  });
});

function stripComments(src: string): string {
  return src.replace(/\/\*[\s\S]*?\*\//g, "");
}

/** The painted hot area, read from float.css rather than assumed. */
function paintedRect(capsule: Rect, expanded: boolean, viewport: Rect): Rect {
  if (!expanded) return capsule;
  const css = stripComments(readFileSync(new URL("../float.css", import.meta.url), "utf8") as string);
  const m = css.match(/\.pill-hot\[data-hover="expanded"\]::before\s*\{([^}]*)\}/);
  expect(m).not.toBeNull();
  const body = m![1];
  expect(body).toMatch(/background:\s*rgba\(0,\s*0,\s*0,\s*0?\.004\)/);
  const inset = Number(/inset:\s*(-?\d+(?:\.\d+)?)px/.exec(body)![1]);
  // A negative inset grows the box; the window clips whatever it cannot show.
  return clip(inflate(capsule, -inset), viewport);
}

describe("published_hitbox_matches_the_painted_rect_in_both_states", () => {
  let observers: Array<() => void> = [];
  beforeEach(() => {
    vi.resetModules();
    invoke.mockClear();
    observers = [];
    class FakeObserver {
      cb: () => void;
      constructor(cb: () => void) {
        this.cb = cb;
        observers.push(cb);
      }
      observe() {}
      disconnect() {}
    }
    vi.stubGlobal("ResizeObserver", FakeObserver);
    vi.stubGlobal("MutationObserver", FakeObserver);
    vi.stubGlobal("window", { addEventListener() {}, removeEventListener() {} });
    vi.stubGlobal("document", { documentElement: {} });
  });
  afterEach(() => vi.unstubAllGlobals());

  const lastHitbox = (): Rect => {
    const calls = invoke.mock.calls.filter((c) => c[0] === "pill_set_hitbox");
    return calls[calls.length - 1][1] as Rect;
  };

  for (const dock of DOCKS) {
    it(`(${dock} dock)`, async () => {
      const drag = await import("./drag");
      const g = geometry(dock);
      let phase: "collapsed" | "expanded" = "collapsed";
      let cap = g.collapsed;
      const el = {
        getBoundingClientRect: () => ({ left: cap.x, top: cap.y, width: cap.w, height: cap.h }),
      } as unknown as HTMLElement;
      // Exactly the wiring ClassicPill uses.
      const watch = drag.watchPillHitbox(el, (r) => hotRect(r, phase, g.viewport));

      // Collapsed: the panel takes the cursor over the capsule and nowhere else.
      expect(lastHitbox()).toEqual(paintedRect(g.collapsed, false, FLOAT));
      expect(lastHitbox()).toEqual(g.collapsed);

      // Expanded: capsule + the CSS margin, the same rect float.css paints.
      phase = "expanded";
      watch.refresh();
      expect(lastHitbox()).toEqual(paintedRect(g.collapsed, true, FLOAT));
      cap = g.expanded; // the capsule grows; the ResizeObserver re-publishes
      observers.forEach((cb) => cb());
      expect(lastHitbox()).toEqual(paintedRect(g.expanded, true, FLOAT));
      expect(lastHitbox()).toEqual(hotRect(g.expanded, "expanded", g.viewport));

      // ...and back: collapsing removes the margin from the panel too.
      phase = "collapsed";
      cap = g.collapsed;
      watch.refresh();
      expect(lastHitbox()).toEqual(paintedRect(g.collapsed, false, FLOAT));
      watch.stop();
    });
  }

  it("the CSS margin and the motion table agree", () => {
    const painted = paintedRect({ x: 100, y: 50, w: 10, h: 10 }, true, { x: -1e4, y: -1e4, w: 2e4, h: 2e4 });
    expect(painted).toEqual(inflate({ x: 100, y: 50, w: 10, h: 10 }, HOVER.marginPx));
    expect(HOVER.marginPx).toBe(12);
  });

  it("the dock-edge dwell stays inside the published expanded hitbox", () => {
    for (const dock of DOCKS) {
      const g = geometry(dock);
      const hot = hotRect(g.expanded, "expanded", g.viewport);
      const { inside, outside } = dockEdgePair(dock);
      expect(contains(hot, ...inside)).toBe(true);
      expect(contains(hot, ...outside)).toBe(true);
      expect(contains(g.collapsed, ...outside)).toBe(false);
    }
  });
});
