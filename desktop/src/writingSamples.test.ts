import { describe, expect, it } from "vitest";
import { countWords, rulesBlocker, sampleBlocker, type UserContextView } from "./writingSamples";

const view = (words: number[] = []): UserContextView => ({
  writingSamples: words.map((w, i) => ({
    id: String(i),
    body: "x",
    words: w,
    createdAt: "2026-10-27T00:00:00Z",
    inUse: true,
  })),
  customRules: "",
  maxSamples: 3,
  minSampleWords: 20,
  maxSampleWordsTotal: 320,
  maxRulesWords: 80,
});
const draft = (n: number) => Array.from({ length: n }, (_, i) => `w${i}`).join(" ");

describe("writing style guards (Y9-B)", () => {
  it("counts words the way the backend does", () => {
    expect(countWords("")).toBe(0);
    expect(countWords("  hi \n there  ")).toBe(2);
  });
  it("refuses a two-word sample as noise", () => {
    expect(sampleBlocker("thanks, bye", view())).toMatch(/at least 20 words/);
    expect(sampleBlocker(draft(20), view())).toBeNull();
  });
  it("refuses past the count cap and the shared word budget", () => {
    expect(sampleBlocker(draft(20), view([20, 20, 20]))).toMatch(/remove one/);
    expect(sampleBlocker(draft(30), view([300]))).toMatch(/only 20 more fit/);
  });
  it("caps the custom rules", () => {
    expect(rulesBlocker(draft(80), view())).toBeNull();
    expect(rulesBlocker(draft(81), view())).toMatch(/under 80/);
  });
});
