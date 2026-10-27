import { describe, expect, it } from "vitest";
import { bucketFor, DEFAULT_TONE, reactiveLine, type Tone } from "./tone";

const CURSES = /\b(damn|hell|bro)\b/i;

describe("bucketFor (pill/tone)", () => {
  it("cuts the word-count bands at 5 / 20 / 60 / 150", () => {
    expect([0, 5, 6, 20, 21, 60, 61, 150, 151].map(bucketFor)).toEqual([
      "tiny", "tiny", "short", "short", "medium", "medium", "long", "long", "epic",
    ]);
  });
});

describe("reactiveLine (pill/tone)", () => {
  it("is silent when the tone is off", () => {
    expect(reactiveLine(40, { tone: "off", curseFilter: false })).toBe("");
  });

  it("is deterministic for the same inputs and rotates with the nonce", () => {
    const a = reactiveLine(3, DEFAULT_TONE, 0);
    expect(reactiveLine(3, DEFAULT_TONE, 0)).toBe(a);
    expect(reactiveLine(3, DEFAULT_TONE, 1)).not.toBe(a);
  });

  it("never indexes out of range, whatever the nonce", () => {
    for (const nonce of [-7, -1, 0, 1, 2, 3, 1_000_003]) {
      for (const tone of ["rude", "friendly", "rose"] as Tone[]) {
        expect(reactiveLine(80, { tone, curseFilter: true }, nonce)).not.toBe("");
      }
    }
  });

  it("clamps negative and fractional counts into the tiny/short bands", () => {
    expect(reactiveLine(-10, DEFAULT_TONE)).toBe(reactiveLine(0, DEFAULT_TONE));
    expect(reactiveLine(5.9, DEFAULT_TONE)).toBe(reactiveLine(5, DEFAULT_TONE));
  });

  it("curse filter ON keeps Rude clean in every band; OFF lets it curse", () => {
    const clean: string[] = [];
    const raw: string[] = [];
    for (const words of [1, 10, 40, 100, 400]) {
      for (let nonce = 0; nonce < 4; nonce++) {
        clean.push(reactiveLine(words, { tone: "rude", curseFilter: true }, nonce));
        raw.push(reactiveLine(words, { tone: "rude", curseFilter: false }, nonce));
      }
    }
    expect(clean.filter((l) => CURSES.test(l))).toEqual([]);
    expect(raw.some((l) => CURSES.test(l))).toBe(true);
  });

  it("the curse filter does not touch the other tones", () => {
    for (const tone of ["friendly", "rose"] as Tone[]) {
      expect(reactiveLine(100, { tone, curseFilter: false }, 1)).toBe(
        reactiveLine(100, { tone, curseFilter: true }, 1),
      );
    }
  });
});
