/**
 * Y5-E — the docked pill stops oscillating on the screen edge.
 *
 * Every test here drives the PURE reducer in `hitbox.ts` over a synthetic
 * cursor path. The acceptance the parity note specifies is literally a count:
 * "simulated pointer dwell at the dock edge produces <=1 state transition".
 */
import { describe, expect, it } from "vitest";
import {
  contains,
  createHoverMachine,
  expandedRect,
  hoverPhases,
  hoverState,
  HOVER_MOTION,
  publishedHitbox,
  type CursorSample,
  type Dock,
  type Rect,
} from "./hitbox";

/** The float window, in logical points. Wide enough that a dock is a real edge. */
const WIN = { w: 1440, h: 900 };
/** The capsule at rest: float.css gives `.pill` min-width 56px, height 16px. */
const SEED = { w: 56, h: 16 };
/** `.stage` insets the capsule from the screen edge by 10px on a side dock. */
const EDGE_INSET = 10;

/** Where the collapsed capsule sits for each dock, matching float.css's `.stage` rules. */
function collapsedRectFor(dock: Dock): Rect {
  if (dock === "left") return { x: EDGE_INSET, y: (WIN.h - SEED.h) / 2, ...SEED };
  if (dock === "right") return { x: WIN.w - EDGE_INSET - SEED.w, y: (WIN.h - SEED.h) / 2, ...SEED };
  return { x: (WIN.w - SEED.w) / 2, y: 6, ...SEED };
}

/**
 * The boundary the cursor dwells on: the capsule edge that faces the screen
 * edge. That is the one a docked pill oscillates across, because the user has
 * slammed the cursor into the screen edge and it is resting there.
 */
function dwellAxis(dock: Dock, r: Rect): { inside: CursorSample; outside: CursorSample } {
  const cy = r.y + r.h / 2;
  const cx = r.x + r.w / 2;
  if (dock === "left") {
    return { inside: { x: r.x + 1, y: cy, t: 0 }, outside: { x: r.x - 1, y: cy, t: 0 } };
  }
  if (dock === "right") {
    return { inside: { x: r.x + r.w - 1, y: cy, t: 0 }, outside: { x: r.x + r.w + 1, y: cy, t: 0 } };
  }
  return { inside: { x: cx, y: r.y + 1, t: 0 }, outside: { x: cx, y: r.y - 1, t: 0 } };
}

/**
 * A pointer resting on the dock edge, jittering one pixel either side of the
 * COLLAPSED boundary — the exact motion a hand makes while the cursor is parked
 * against the screen edge. 8ms apart, i.e. faster than the collapse debounce,
 * which is what a real 120Hz trackpad produces.
 */
function dwellPath(dock: Dock, r: Rect, samples = 40): CursorSample[] {
  const { inside, outside } = dwellAxis(dock, r);
  return Array.from({ length: samples }, (_, i) => ({
    ...(i % 2 === 0 ? inside : outside),
    t: i * 8,
  }));
}

const DOCKS: Dock[] = ["top", "left", "right"];

describe("Y5-E hover hysteresis", () => {
  it("dwell_at_the_dock_edge_produces_at_most_one_transition", () => {
    for (const dock of DOCKS) {
      const r = collapsedRectFor(dock);
      const end = hoverState(r, dwellPath(dock, r));
      expect(end.transitions, `dock=${dock}`).toBeLessThanOrEqual(1);
      // and it is 1, not 0: the pill DID expand. A machine that never expands
      // also never oscillates, and would be a worse bug.
      expect(end.transitions, `dock=${dock}`).toBe(1);
      expect(end.phase, `dock=${dock}`).toBe("expanded");
    }
  });

  it("the same dwell path oscillates without the expanded-rect margin", () => {
    // The control. With margin 0 the hot rect is the same in both phases, so
    // every jitter sample flips the phase — the bug Wispr shipped a comment
    // about. This is what the test above is proving the fix against.
    for (const dock of DOCKS) {
      const r = collapsedRectFor(dock);
      const naive = hoverState(r, dwellPath(dock, r), { margin: 0, collapseDebounceMs: 0 });
      expect(naive.transitions, `dock=${dock}`).toBeGreaterThan(1);
    }
  });

  it("collapsed_pill_has_no_margin_dead_zone", () => {
    for (const dock of DOCKS) {
      const r = collapsedRectFor(dock);
      // A point in the ring between the capsule and where the margin WOULD be:
      // 6px outside the capsule, i.e. inside `inset: -12px` but outside the seed.
      const ring: CursorSample = dock === "top"
        ? { x: r.x + r.w / 2, y: r.y - 6, t: 0 }
        : dock === "left"
        ? { x: r.x - 6, y: r.y + r.h / 2, t: 0 }
        : { x: r.x + r.w + 6, y: r.y + r.h / 2, t: 0 };

      // the margin box would swallow it...
      expect(contains(expandedRect(r), ring), `dock=${dock} margin box`).toBe(true);
      // ...but while collapsed nothing is painted there, so it is not a hit,
      // the pill does not expand, and the click belongs to whatever is beneath.
      expect(contains(r, ring), `dock=${dock} capsule`).toBe(false);
      const end = hoverState(r, [ring, { ...ring, t: 500 }]);
      expect(end.phase, `dock=${dock}`).toBe("collapsed");
      expect(end.transitions, `dock=${dock}`).toBe(0);
      // and the rect handed to the backend while collapsed is the bare capsule,
      // so the NSPanel has no dead zone around an idle seed either.
      expect(publishedHitbox(r, "collapsed")).toEqual(r);
    }
  });

  it("published_hitbox_matches_the_painted_rect_in_both_states", () => {
    const m = HOVER_MOTION.expandedMarginPx;
    expect(m).toBe(12); // float.css paints `inset: -12px`
    for (const dock of DOCKS) {
      const r = collapsedRectFor(dock);
      // collapsed: no ::before is painted, so the published box is the capsule.
      expect(publishedHitbox(r, "collapsed"), `dock=${dock}`).toEqual(r);
      // expanded: `inset: -12px` on all four sides, uniformly, and the panel is
      // told exactly that box — not the capsule, and not some per-dock variant.
      expect(publishedHitbox(r, "expanded"), `dock=${dock}`).toEqual({
        x: r.x - m,
        y: r.y - m,
        w: r.w + 2 * m,
        h: r.h + 2 * m,
      });
      expect(publishedHitbox(r, "expanded"), `dock=${dock}`).toEqual(expandedRect(r));
    }
  });

  it("expands on the first sample inside the capsule — no lag", () => {
    const r = collapsedRectFor("top");
    const phases = hoverPhases(r, [{ x: r.x + 4, y: r.y + 4, t: 0 }]);
    expect(phases).toEqual(["expanded"]);
  });

  it("collapses once the cursor is outside the EXPANDED rect for the debounce", () => {
    const r = collapsedRectFor("left");
    const far = { x: r.x + 400, y: r.y + 300 };
    const path: CursorSample[] = [
      { x: r.x + 2, y: r.y + 8, t: 0 }, // expand
      { ...far, t: 10 }, // outside the margin box — arms the collapse
      { ...far, t: 60 }, // still inside the debounce window
      { ...far, t: 10 + HOVER_MOTION.collapseDebounceMs }, // matures
    ];
    expect(hoverPhases(r, path)).toEqual(["expanded", "expanded", "expanded", "collapsed"]);
    expect(hoverState(r, path).transitions).toBe(2);
  });

  it("re-entering the margin box disarms a pending collapse", () => {
    const r = collapsedRectFor("right");
    const inMargin = { x: r.x - 6, y: r.y + r.h / 2 }; // inside `inset:-12`, outside the capsule
    const path: CursorSample[] = [
      { x: r.x + 4, y: r.y + 8, t: 0 },
      { x: r.x + 900, y: r.y, t: 10 }, // arm
      { ...inMargin, t: 40 }, // back in the hot ring — disarm
      { ...inMargin, t: 5000 }, // dwell forever; never collapses
    ];
    const end = hoverState(r, path);
    expect(end.phase).toBe("expanded");
    expect(end.collapseAt).toBeNull();
    expect(end.transitions).toBe(1);
  });

  it("createHoverMachine reports only real phase changes", () => {
    const r = collapsedRectFor("top");
    const m = createHoverMachine();
    expect(m.push(r, { x: r.x + 4, y: r.y + 4, t: 0 })).toBe(true);
    expect(m.push(r, { x: r.x + 5, y: r.y + 5, t: 8 })).toBe(false);
    expect(m.phase).toBe("expanded");
    expect(m.reset()).toBe(true);
    expect(m.phase).toBe("collapsed");
    expect(m.reset()).toBe(false);
  });

  it("the hot area alpha is non-zero but invisible", () => {
    expect(HOVER_MOTION.hotAreaAlpha).toBeGreaterThan(0);
    expect(HOVER_MOTION.hotAreaAlpha).toBeLessThan(0.01);
  });
});
