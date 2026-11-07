/**
 * yap-wf-01-click-the-pill-to — WF-01 acceptance: a pointer-down and pointer-up
 * under 4 px of movement and under 300 ms toggles the take; anything longer is
 * a drag.
 */
import { describe, expect, it } from "vitest";
import { beginPress, CLICK_MAX_MS, CLICK_SLOP, classifyPress, endPress, trackPress } from "./press";

describe("WF-01 gesture classifier", () => {
  it("pins the thresholds the acceptance names", () => {
    expect(CLICK_SLOP).toBe(4);
    expect(CLICK_MAX_MS).toBe(300);
  });

  it("a still, quick press is a click", () => {
    const p = beginPress(100, 100, 1_000);
    expect(endPress(p, 100, 100, 1_120)).toBe("click");
  });

  it("under 4 px of travel and under 300 ms is still a click", () => {
    const p = beginPress(100, 100, 0);
    trackPress(p, 102, 101);
    expect(endPress(p, 102.5, 102.5, 299)).toBe("click"); // 3.54 px, 299 ms
  });

  it("4 px of travel or more is a drag, however quick", () => {
    expect(classifyPress(4, 50)).toBe("drag");
    expect(endPress(beginPress(0, 0, 0), 4, 0, 40)).toBe("drag");
    expect(endPress(beginPress(0, 0, 0), 3, 3, 40)).toBe("drag"); // 4.24 px diagonal
  });

  it("300 ms or longer is a drag, however still", () => {
    expect(classifyPress(0, 300)).toBe("drag");
    expect(endPress(beginPress(10, 10, 0), 10, 10, 900)).toBe("drag");
  });

  it("a wobble out past the slop and back is a drag, not a click", () => {
    const p = beginPress(50, 50, 0);
    expect(trackPress(p, 58, 50)).toBe(8);
    expect(trackPress(p, 50, 50)).toBe(0);
    expect(p.far).toBe(8);
    expect(endPress(p, 50, 50, 100)).toBe("drag");
  });

  it("a clock that ran backwards never reads as a click", () => {
    expect(classifyPress(0, -1)).toBe("drag");
  });
});
