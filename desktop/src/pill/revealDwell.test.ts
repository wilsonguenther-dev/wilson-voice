// yap-1107-reveal-stays-until-read — the reveal's dwell rule and timer, driven
// with fake timers (vitest fakes Date.now with them, so `now` follows).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { REVEAL_EDIT_TTL_MS } from "./polishResult";
import {
  REVEAL_CLOSE_REASONS,
  REVEAL_MIN_DWELL_MS,
  REVEAL_POINTER_GRACE_MS,
  createRevealDwell,
  isRevealCloseReason,
  mayAutoClose,
  revealCloseAt,
  type RevealCloseReason,
  type RevealDwell,
} from "./revealDwell";
import { backendCloseReason } from "./PolishRevealCard";

type Case = "no-pointer" | "pointer-moving" | "keep-open";
const CASES: readonly Case[] = ["no-pointer", "pointer-moving", "keep-open"];

function harness(keepOpen = false) {
  const closes: Array<{ reason: RevealCloseReason; dwellMs: number }> = [];
  const dwell = createRevealDwell(keepOpen, { onClose: (reason, dwellMs) => closes.push({ reason, dwellMs }) });
  return { dwell, closes };
}

/** Advance to absolute time `t` ms after the reveal was shown at 0. */
let t0 = 0;
const at = (t: number) => vi.advanceTimersByTime(t - (Date.now() - t0));

/** Pointer enters at 7 900 ms and moves every 100 ms until 12 000 ms, then leaves. */
function pointerMovesUntil12s(dwell: RevealDwell) {
  at(7_900);
  dwell.pointerEnter();
  for (let t = 8_000; t <= 12_000; t += 100) {
    at(t);
    dwell.pointerMove();
  }
  dwell.pointerLeave();
}

/** Put the reveal into each case at a time where the timer has NOT closed it. */
function setUp(c: Case): ReturnType<typeof harness> {
  const h = harness(c === "keep-open");
  t0 = Date.now();
  h.dwell.show("toast");
  if (c === "pointer-moving") pointerMovesUntil12s(h.dwell);
  else at(c === "keep-open" ? 60_000 : 7_000);
  expect(h.dwell.isOpen()).toBe(true);
  return h;
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-11-07T15:00:00Z"));
});
afterEach(() => {
  vi.useRealTimers();
});

describe("reveal dwell — the rule", () => {
  it("is at least 8 s and the TTL constant follows it", () => {
    expect(REVEAL_EDIT_TTL_MS).toBeGreaterThanOrEqual(8_000);
    expect(REVEAL_MIN_DWELL_MS).toBe(REVEAL_EDIT_TTL_MS);
    expect(REVEAL_POINTER_GRACE_MS).toBe(2_000);
  });

  it("never before 8 s, never under the pointer, never within 2 s of a move, never with keep-open or on the card", () => {
    const base = { shownAt: 0, face: "toast" as const, pointerInside: false, lastMoveInsideAt: null, keepOpen: false };
    expect(mayAutoClose(base, 7_999)).toBe(false);
    expect(mayAutoClose(base, 8_000)).toBe(true);
    expect(mayAutoClose({ ...base, pointerInside: true, lastMoveInsideAt: 1_000 }, 30_000)).toBe(false);
    expect(revealCloseAt({ ...base, lastMoveInsideAt: 12_000 })).toBe(14_000);
    expect(mayAutoClose({ ...base, lastMoveInsideAt: 12_000 }, 13_999)).toBe(false);
    expect(mayAutoClose({ ...base, lastMoveInsideAt: 1_000 }, 8_000)).toBe(true);
    expect(mayAutoClose({ ...base, keepOpen: true }, 600_000)).toBe(false);
    expect(mayAutoClose({ ...base, face: "card" }, 600_000)).toBe(false);
  });
});

describe("reveal dwell — timing (fake timers)", () => {
  it("pointer never enters: still open at 7 999 ms, closed by 8 000 ms", () => {
    const { dwell, closes } = harness();
    t0 = Date.now();
    dwell.show("toast");
    at(7_999);
    expect(dwell.isOpen()).toBe(true);
    at(8_000);
    expect(dwell.isOpen()).toBe(false);
    expect(closes).toEqual([{ reason: "timer", dwellMs: 8_000 }]);
  });

  it("pointer inside at 7.9 s and moving until 12 s: open at 13 999 ms, closed by 14 000 ms after it leaves", () => {
    const { dwell, closes } = harness();
    t0 = Date.now();
    dwell.show("toast");
    at(7_900);
    dwell.pointerEnter();
    at(8_000);
    expect(dwell.isOpen()).toBe(true); // 8 s passed, but the pointer is inside
    for (let t = 8_100; t <= 12_000; t += 100) {
      at(t);
      dwell.pointerMove();
    }
    dwell.pointerLeave();
    at(13_999);
    expect(dwell.isOpen()).toBe(true);
    at(14_000);
    expect(dwell.isOpen()).toBe(false);
    expect(closes).toEqual([{ reason: "timer", dwellMs: 14_000 }]);
  });

  it("the pointer resting inside holds it open indefinitely", () => {
    const { dwell } = harness();
    t0 = Date.now();
    dwell.show("toast");
    at(1_000);
    dwell.pointerEnter();
    at(120_000);
    expect(dwell.isOpen()).toBe(true);
  });

  it("keep-open on: still open at 60 000 ms", () => {
    const { dwell, closes } = harness(true);
    t0 = Date.now();
    dwell.show("toast");
    at(60_000);
    expect(dwell.isOpen()).toBe(true);
    expect(closes).toEqual([]);
  });

  it("switching keep-open off later lets the timer close it", () => {
    const { dwell } = harness(true);
    t0 = Date.now();
    dwell.show("toast");
    at(20_000);
    dwell.setKeepOpen(false);
    vi.advanceTimersByTime(1);
    expect(dwell.isOpen()).toBe(false);
  });

  it("the opened card waits for an answer", () => {
    const { dwell } = harness();
    t0 = Date.now();
    dwell.show("toast");
    at(3_000);
    dwell.setFace("card");
    at(600_000);
    expect(dwell.isOpen()).toBe(true);
  });

  it("a newer reveal restarts the 8 s clock", () => {
    const { dwell } = harness();
    t0 = Date.now();
    dwell.show("toast");
    at(6_000);
    t0 = Date.now();
    dwell.show("toast");
    at(7_999);
    expect(dwell.isOpen()).toBe(true);
    at(8_000);
    expect(dwell.isOpen()).toBe(false);
  });
});

describe("reveal dwell — close paths are immediate in all three cases", () => {
  const paths: ReadonlyArray<[string, RevealCloseReason]> = [
    ["Esc (backend event)", "esc"],
    ["click outside (backend event)", "click_outside"],
    ["the close button", "close_button"],
    ["a new recording", "new_recording"],
  ];
  for (const c of CASES) {
    for (const [name, reason] of paths) {
      it(`${name} closes it at once — ${c}`, () => {
        const { dwell, closes } = setUp(c);
        dwell.close(reason);
        expect(dwell.isOpen()).toBe(false);
        expect(closes).toHaveLength(1);
        expect(closes[0].reason).toBe(reason);
        // Closed once: a later timer or a second close does nothing.
        dwell.close("timer");
        vi.advanceTimersByTime(120_000);
        expect(closes).toHaveLength(1);
      });
    }
  }

  it("the backend close event carries only esc / click_outside", () => {
    expect(backendCloseReason("esc")).toBe("esc");
    expect(backendCloseReason("click_outside")).toBe("click_outside");
    expect(backendCloseReason("timer")).toBeNull();
    expect(backendCloseReason(null)).toBeNull();
    expect(backendCloseReason({ reason: "esc" })).toBeNull();
  });

  it("every close reason is a known tag (mirrors float_pill.rs REVEAL_REASONS)", () => {
    for (const r of REVEAL_CLOSE_REASONS) expect(isRevealCloseReason(r)).toBe(true);
    expect(isRevealCloseReason("other")).toBe(false);
  });
});
