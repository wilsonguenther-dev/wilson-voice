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

// yap-1107-pill-click-always-registers — acceptance 2: the pressed state is
// instant (pill.html: the shell scales to 96 percent while pressed) and its
// latency from pointer-down is measured against a 50 ms budget.
import {
  isPressed,
  markPressed,
  PRESS_VISUAL_BUDGET_MS,
  PRESSED_ATTR,
  PRESSED_DATASET_KEY,
  PRESSED_SCALE,
  pressLogLine,
  pressVisualLatency,
} from "./press";

describe("pressed state is instant and measured", () => {
  const fakeRoot = () => ({ dataset: {} as Record<string, string | undefined> }) as unknown as HTMLElement;

  it("pins the approved numbers", () => {
    expect(PRESSED_SCALE).toBe(0.96);
    expect(PRESS_VISUAL_BUDGET_MS).toBe(50);
    expect(PRESSED_ATTR).toBe("data-pill-pressed");
    expect(PRESSED_DATASET_KEY).toBe("pillPressed");
  });

  it("markPressed is synchronous and idempotent", () => {
    const r = fakeRoot();
    expect(isPressed(r)).toBe(false);
    markPressed(r, true);
    expect(isPressed(r)).toBe(true); // visible with no await, no IPC
    markPressed(r, true);
    expect(isPressed(r)).toBe(true);
    markPressed(r, false);
    expect(isPressed(r)).toBe(false);
    markPressed(r, false);
    expect(isPressed(r)).toBe(false);
  });

  it("latency is paint minus pointer-down, rounded to 0.1 ms", () => {
    expect(pressVisualLatency(1_000, 1_016.66)).toBe(16.7);
    expect(pressVisualLatency(1_000, 1_000)).toBe(0);
  });

  it("a missing or backwards clock is unknown, never a fake number", () => {
    expect(pressVisualLatency(0, 10)).toBeNull();
    expect(pressVisualLatency(Number.NaN, 10)).toBeNull();
    expect(pressVisualLatency(10, Number.POSITIVE_INFINITY)).toBeNull();
    expect(pressVisualLatency(20, 10)).toBeNull();
  });

  it("the log line names the budget and flags an overrun", () => {
    expect(pressLogLine(16.7)).toBe("pill press event=pressed_visual latency_ms=16.7 budget_ms=50 over_budget=false");
    expect(pressLogLine(51)).toContain("over_budget=true");
    expect(pressLogLine(null)).toContain("latency_ms=unknown");
    expect(pressLogLine(null)).toContain("over_budget=false");
  });
});
