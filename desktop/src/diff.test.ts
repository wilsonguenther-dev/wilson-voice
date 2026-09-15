/**
 * Y4-G — the diff is a TRUST mechanism, so its failure modes are: claiming a
 * change that did not happen, hiding one that did, and hanging the History pane
 * on a long take. One test each.
 */
import { describe, it, expect } from "vitest";
import {
  diffWords,
  diffSummary,
  isUnchanged,
  tokenize,
  DEFAULT_MAX_D,
} from "./diff";

/** Re-assembling the two sides of a diff must reproduce the two inputs. */
function rebuild(tokens: ReturnType<typeof diffWords>) {
  const before = tokens
    .filter((t) => t.op !== "added")
    .map((t) => t.text)
    .join("");
  const after = tokens
    .filter((t) => t.op !== "removed")
    .map((t) => t.text)
    .join("");
  return { before, after };
}

describe("tokenize", () => {
  it("keeps whitespace as its own token so paragraph breaks are visible", () => {
    expect(tokenize("a\n\nb")).toEqual(["a", "\n\n", "b"]);
  });

  it("returns an empty list for an empty string", () => {
    expect(tokenize("")).toEqual([]);
  });
});

describe("diffWords", () => {
  it("reports identical strings as all equal", () => {
    const t = diffWords("ship the recovery item", "ship the recovery item");
    expect(t).toEqual([{ op: "equal", text: "ship the recovery item" }]);
    expect(isUnchanged(t)).toBe(true);
    expect(diffSummary(t)).toEqual({ added: 0, removed: 0, unchanged: 4 });
  });

  it("reports two empty strings as no tokens at all", () => {
    expect(diffWords("", "")).toEqual([]);
    expect(isUnchanged(diffWords("", ""))).toBe(true);
  });

  it("shows a removed filler word and nothing else", () => {
    const t = diffWords("um the report is done", "the report is done");
    expect(isUnchanged(t)).toBe(false);
    expect(t.filter((x) => x.op === "removed").map((x) => x.text.trim())).toEqual(
      ["um"],
    );
    expect(t.some((x) => x.op === "added")).toBe(false);
    expect(rebuild(t)).toEqual({
      before: "um the report is done",
      after: "the report is done",
    });
  });

  it("shows an added word", () => {
    const t = diffWords("ship it", "ship it now");
    expect(t.filter((x) => x.op === "added").map((x) => x.text.trim())).toEqual([
      "now",
    ]);
    expect(rebuild(t)).toEqual({ before: "ship it", after: "ship it now" });
  });

  it("shows a paragraph break the rules stage inserted", () => {
    const t = diffWords("one two three four", "one two\n\nthree four");
    expect(isUnchanged(t)).toBe(false);
    expect(rebuild(t)).toEqual({
      before: "one two three four",
      after: "one two\n\nthree four",
    });
  });

  it("round-trips a realistic cleanup rewrite exactly", () => {
    const raw =
      "um so i think we should uh ship the thing on tuesday wait no friday";
    const formatted = "So I think we should ship the thing on Friday.";
    const t = diffWords(raw, formatted);
    expect(rebuild(t)).toEqual({ before: raw, after: formatted });
  });

  it("never invents an add/remove pair for a suffix-only change", () => {
    const t = diffWords("hello world", "hello world.");
    expect(rebuild(t)).toEqual({ before: "hello world", after: "hello world." });
    expect(t[0]).toEqual({ op: "equal", text: "hello " });
  });
});

describe("performance", () => {
  const words = (n: number, seed: string) =>
    Array.from({ length: n }, (_, i) => `${seed}${i}`).join(" ");

  it("diffs a 2,000-word take with a small edit in well under a second", () => {
    const raw = words(2000, "w");
    const formatted = raw.replace("w1000 ", "");
    const started = Date.now();
    const t = diffWords(raw, formatted);
    const elapsed = Date.now() - started;
    expect(rebuild(t)).toEqual({ before: raw, after: formatted });
    expect(elapsed).toBeLessThan(1000);
  });

  it("does not blow up quadratically on a pathological 2,000-word pair", () => {
    // Two 2,000-word texts with NO token in common: the edit distance is 4,000,
    // which is the case a naive LCS table turns into 4,000,000 cells. The maxD
    // ceiling must catch it and degrade to the coarse answer instead.
    const raw = words(2000, "alpha");
    const formatted = words(2000, "omega");
    const started = Date.now();
    const t = diffWords(raw, formatted);
    const elapsed = Date.now() - started;
    expect(elapsed).toBeLessThan(2000);
    // Still TRUE, even when coarse: both sides reconstruct exactly.
    expect(rebuild(t)).toEqual({ before: raw, after: formatted });
    expect(isUnchanged(t)).toBe(false);
  });

  it("honours an explicit maxD ceiling by degrading, never by hanging", () => {
    const raw = words(400, "alpha");
    const formatted = words(400, "omega");
    const t = diffWords(raw, formatted, { maxD: 1 });
    expect(t).toEqual([
      { op: "removed", text: raw },
      { op: "added", text: formatted },
    ]);
    expect(DEFAULT_MAX_D).toBeGreaterThan(1);
  });
});

describe("diffSummary", () => {
  it("counts words, not whitespace runs", () => {
    const t = diffWords("a b c", "a\n\nb c d");
    const s = diffSummary(t);
    expect(s.added).toBe(1);
    expect(s.removed).toBe(0);
  });
});
