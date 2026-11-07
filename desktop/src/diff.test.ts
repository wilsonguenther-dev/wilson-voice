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
  diffPhrases,
  readableDiff,
  changeRuns,
  groupHunks,
  diffLogLine,
  MIN_EQUAL_WORDS,
  BLOCKS_CHANGED_SHARE,
  type DiffToken,
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

// ── yap-1107-polish-diff-readable ──────────────────────────────────────────
// Wilson's exact take from Yap Test's History, 2026-11-07T17:21:58Z (row
// 4313a844…, stages dictionary,backtrack,rules,polish), copied verbatim from a
// read-only copy of the test install's database.
const FIXTURE_RAW =
  "Go ahead and add the live onboarding No live training workshop to my calendar and then add the Wednesday onboarding book camp to my calendar.";
const FIXTURE_TEXT =
  "Add the live onboarding No live training workshop to your calendar. Add the Wednesday onboarding book camp to your calendar.";
/** What the "What polish changed" card showed him, character for character. */
const FIXTURE_SOUP =
  "GoAdd ahead and add the live onboarding No live training workshop to myyour calendarcalendar. andAdd then add the Wednesday onboarding book camp to myyour calendar.";

/** Every (before, after) pair this file diffs, plus Wilson's fixture. */
const words = (n: number, seed: string) =>
  Array.from({ length: n }, (_, i) => `${seed}${i}`).join(" ");
const CORPUS: Array<[string, string]> = [
  ["ship the recovery item", "ship the recovery item"],
  ["", ""],
  ["um the report is done", "the report is done"],
  ["ship it", "ship it now"],
  ["one two three four", "one two\n\nthree four"],
  [
    "um so i think we should uh ship the thing on tuesday wait no friday",
    "So I think we should ship the thing on Friday.",
  ],
  ["hello world", "hello world."],
  ["a b c", "a\n\nb c d"],
  ["so basically the report is kind of done now", "The report is now complete."],
  ["ship it to my team", "ship it to your team"],
  [words(2000, "w"), words(2000, "w").replace("w1000 ", "")],
  [words(300, "alpha"), words(300, "omega")],
  [FIXTURE_RAW, FIXTURE_TEXT],
];

const wordsIn = (t: DiffToken) => (t.text.match(/\S+/g) ?? []).length;

describe("phrase-level runs (Wilson's 17:22Z take)", () => {
  it("the fixture reproduces his word soup under the old word-level renderer", () => {
    // DiffView used to render diffWords() tokens as adjacent spans.
    const old = diffWords(FIXTURE_RAW, FIXTURE_TEXT)
      .map((t) => t.text)
      .join("");
    expect(old).toBe(FIXTURE_SOUP);
  });

  it("renders the fixture as at most 3 change runs", () => {
    const runs = diffPhrases(FIXTURE_RAW, FIXTURE_TEXT);
    expect(changeRuns(runs)).toBeLessThanOrEqual(3);
    expect(runs).toEqual([
      { op: "removed", text: "Go ahead and add " },
      { op: "added", text: "Add " },
      { op: "equal", text: "the live onboarding No live training workshop to " },
      { op: "removed", text: "my calendar and then add " },
      { op: "added", text: "your calendar. Add " },
      { op: "equal", text: "the Wednesday onboarding book camp to " },
      { op: "removed", text: "my" },
      { op: "added", text: "your" },
      { op: "equal", text: " calendar." },
    ]);
    expect(rebuild(runs)).toEqual({ before: FIXTURE_RAW, after: FIXTURE_TEXT });
  });

  it("keeps the fixture inline (the toggle offers Before / After)", () => {
    const r = readableDiff(FIXTURE_RAW, FIXTURE_TEXT);
    expect(r.hunks).toBe(3);
    expect(r.changedShare).toBeLessThanOrEqual(BLOCKS_CHANGED_SHARE);
    expect(r.layout).toBe("inline");
    // The summary still counts the exact word-level edits.
    expect(r.exact).toEqual(diffWords(FIXTURE_RAW, FIXTURE_TEXT));
  });
});

describe("phrase-level runs — property over the corpus", () => {
  it("never alternates: each hunk is one removed run then one added run", () => {
    for (const [before, after] of CORPUS) {
      const runs = diffPhrases(before, after);
      for (let i = 1; i < runs.length; i++) {
        const prev = runs[i - 1].op;
        const cur = runs[i].op;
        // No two runs with the same verdict side by side (coalesced) …
        expect(cur === prev, `${JSON.stringify(before)}: ${prev} then ${cur}`).toBe(false);
        // … and an added run is never followed straight by a removed run.
        expect(prev === "added" && cur === "removed", JSON.stringify(before)).toBe(false);
      }
    }
  });

  it("puts at least MIN_EQUAL_WORDS unchanged words between two hunks", () => {
    for (const [before, after] of CORPUS) {
      const runs = diffPhrases(before, after);
      runs.forEach((t, i) => {
        const between = i > 0 && i < runs.length - 1 && t.op === "equal";
        if (between) expect(wordsIn(t), JSON.stringify(before)).toBeGreaterThanOrEqual(MIN_EQUAL_WORDS);
      });
    }
  });

  it("still rebuilds raw and formatted exactly", () => {
    for (const [before, after] of CORPUS) {
      expect(rebuild(diffPhrases(before, after))).toEqual({ before, after });
    }
  });

  it("keeps diffSummary and isUnchanged meaning the same", () => {
    for (const [before, after] of CORPUS) {
      const r = readableDiff(before, after);
      expect(diffSummary(r.exact)).toEqual(diffSummary(diffWords(before, after)));
      expect(isUnchanged(r.runs)).toBe(isUnchanged(r.exact));
    }
  });
});

describe("phrase-level runs — layout verdict", () => {
  it("folds a short unchanged stretch into both sides of one hunk", () => {
    const script: DiffToken[] = [
      { op: "equal", text: "to " },
      { op: "removed", text: "my" },
      { op: "added", text: "your" },
      { op: "equal", text: " " },
      { op: "removed", text: "calendar" },
      { op: "added", text: "calendar." },
      { op: "equal", text: " today" },
    ];
    expect(groupHunks(script)).toEqual([
      { op: "equal", text: "to " },
      { op: "removed", text: "my calendar" },
      { op: "added", text: "your calendar." },
      { op: "equal", text: " today" },
    ]);
  });

  it("shows a one-word change inline", () => {
    const r = readableDiff("ship it to my team", "ship it to your team");
    expect(r.layout).toBe("inline");
    expect(r.hunks).toBe(1);
  });

  it("switches a dense rewrite to Before / After blocks", () => {
    const r = readableDiff("so basically the report is kind of done now", "The report is now complete.");
    expect(r.changedShare).toBeGreaterThan(BLOCKS_CHANGED_SHARE);
    expect(r.layout).toBe("blocks");
  });

  it("never asks for blocks on an unchanged take", () => {
    const r = readableDiff("ship the recovery item", "ship the recovery item");
    expect(r.hunks).toBe(0);
    expect(r.layout).toBe("inline");
    expect(readableDiff("", "").layout).toBe("inline");
  });

  it("logs counts and layout, never text", () => {
    expect(diffLogLine("layout_toggle", { to: "blocks", hunks: 3 })).toBe(
      "diff_view layout_toggle to=blocks hunks=3",
    );
  });
});
