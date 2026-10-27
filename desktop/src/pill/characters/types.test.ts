/** Y5-K — the seam's runtime half: the phase/tone sweep axes and the box arithmetic. */
import { describe, expect, it } from "vitest";
import { ALL_LIVE_PHASES } from "../live";
import { CHARACTER_PHASES, CHARACTER_TONES, fitSquare, fitsBox } from "./types";

describe("character types", () => {
  it("sweeps every LivePhase and every ChatTone", () => {
    expect([...CHARACTER_PHASES].sort()).toEqual([...ALL_LIVE_PHASES].sort());
    expect([...CHARACTER_TONES].sort()).toEqual(["friendly", "rose", "rude"]);
  });

  it("fitsBox: inside or equal fits; over, negative or non-finite never does", () => {
    expect(fitsBox({ w: 10, h: 10 }, { w: 10, h: 10 })).toBe(true);
    expect(fitsBox({ w: 0, h: 0 }, { w: 0, h: 0 })).toBe(true);
    expect(fitsBox({ w: 11, h: 10 }, { w: 10, h: 10 })).toBe(false);
    expect(fitsBox({ w: 10, h: 11 }, { w: 10, h: 10 })).toBe(false);
    expect(fitsBox({ w: -1, h: 1 }, { w: 10, h: 10 })).toBe(false);
    expect(fitsBox({ w: 1, h: NaN }, { w: 10, h: 10 })).toBe(false);
  });

  it("fitSquare: the largest whole-px square inside the box, capped", () => {
    expect(fitSquare({ w: 200, h: 40 })).toEqual({ w: 40, h: 40 });
    expect(fitSquare({ w: 200, h: 40 }, 24)).toEqual({ w: 24, h: 24 });
    expect(fitSquare({ w: 9.7, h: 40 })).toEqual({ w: 9, h: 9 });
    expect(fitSquare({ w: -5, h: 4 })).toEqual({ w: 0, h: 0 });
  });
});
