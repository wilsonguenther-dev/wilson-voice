import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";
import {
  forcedPartial, governorAllowsPartials, nextRenderDelay, PARTIAL_EMPTY, PARTIAL_MAX_CHARS, PARTIAL_RENDER_MIN_MS,
  parsePartial, partialShows, partialTail, partialTakeLogLine, partialText, reducePartial, SAMPLE_PARTIAL_TEXT,
  TRANSCRIPT_PARTIAL_EVENT, type PartialState,
} from "./partialTranscript";
import { GOVERNOR_FULL, type GovernorView } from "./governor";
import { SIDE_DOCK_TEXT_PX, STRIP_CHAR_PX } from "./live";

const here = (p: string): string => readFileSync(new URL(p, import.meta.url), "utf8") as string;

const partial = (take: number, committed: string, tentative = "") => ({
  type: "partial" as const,
  payload: { take, committed, tentative },
});

describe("yap-1106-partial-transcript-stream — event contract", () => {
  it("listens on the same event name the backend emits", () => {
    const rust = here("../../src-tauri/src/stream_decode.rs");
    expect(rust).toContain(`pub const TRANSCRIPT_PARTIAL_EVENT: &str = "${TRANSCRIPT_PARTIAL_EVENT}";`);
  });

  it("parses the backend payload and ignores malformed ones", () => {
    expect(parsePartial({ take: 3, committed: "hello there", tentative: "" })).toEqual({
      take: 3, committed: "hello there", tentative: "",
    });
    expect(parsePartial({ take: 3, committed: "hi" })).toEqual({ take: 3, committed: "hi", tentative: "" });
    expect(parsePartial(null)).toBeNull();
    expect(parsePartial("text")).toBeNull();
    expect(parsePartial({ take: "3", committed: "x" })).toBeNull();
    expect(parsePartial({ take: -1, committed: "x" })).toBeNull();
    expect(parsePartial({ take: 1, committed: 7 })).toBeNull();
  });

  it("joins committed and tentative text", () => {
    expect(partialText({ take: 1, committed: " so the plan ", tentative: "" })).toBe("so the plan");
    expect(partialText({ take: 1, committed: "so the plan", tentative: " is" })).toBe("so the plan is");
    expect(partialText({ take: 1, committed: "", tentative: "is" })).toBe("is");
  });
});

describe("yap-1106-partial-transcript-stream — take lifecycle", () => {
  it("shows the newest partial of the take and counts updates", () => {
    let s: PartialState = reducePartial(PARTIAL_EMPTY, { type: "recording", recording: true });
    s = reducePartial(s, partial(1, "so the plan"));
    s = reducePartial(s, partial(1, "so the plan for Monday"));
    expect(s.take).toBe(1);
    expect(s.text).toBe("so the plan for Monday");
    expect(s.updates).toBe(2);
  });

  it("clears on the final transcript, an error or a cancel, and drops that take's stragglers", () => {
    let s = reducePartial(PARTIAL_EMPTY, partial(4, "first take words"));
    s = reducePartial(s, { type: "end" });
    expect(s.text).toBe("");
    expect(s.take).toBeNull();
    // The release tail's partial can land after the final transcript.
    const late = reducePartial(s, partial(4, "first take words and the tail"));
    expect(late).toBe(s);
    expect(late.text).toBe("");
  });

  it("a new take starts empty and ignores the previous take", () => {
    let s = reducePartial(PARTIAL_EMPTY, partial(7, "old words"));
    s = reducePartial(s, { type: "recording", recording: true });
    expect(s.text).toBe("");
    expect(reducePartial(s, partial(7, "old words more")).text).toBe("");
    expect(reducePartial(s, partial(8, "new words")).text).toBe("new words");
  });

  it("key-up keeps the words until the take ends (hidden by phase meanwhile)", () => {
    const s = reducePartial(PARTIAL_EMPTY, partial(2, "still here"));
    expect(reducePartial(s, { type: "recording", recording: false })).toBe(s);
  });

  it("a malformed payload changes nothing", () => {
    const s = reducePartial(PARTIAL_EMPTY, partial(2, "words"));
    expect(reducePartial(s, { type: "partial", payload: { nope: true } })).toBe(s);
  });
});

describe("yap-1106-partial-transcript-stream — where and when it shows", () => {
  const light: GovernorView = { ...GOVERNOR_FULL, level: "light", reason: "battery" };
  const minimal: GovernorView = { ...GOVERNOR_FULL, level: "minimal", reason: "thermal" };

  it("only while the take is listening", () => {
    expect(partialShows("words", "listening", GOVERNOR_FULL)).toBe(true);
    expect(partialShows("words", "listening-locked", GOVERNOR_FULL)).toBe(true);
    expect(partialShows("words", "transcribing", GOVERNOR_FULL)).toBe(false);
    expect(partialShows("words", "idle", GOVERNOR_FULL)).toBe(false);
    expect(partialShows("   ", "listening", GOVERNOR_FULL)).toBe(false);
  });

  it("is off whenever the resource governor is not at full", () => {
    expect(governorAllowsPartials(GOVERNOR_FULL)).toBe(true);
    expect(governorAllowsPartials(null)).toBe(true);
    expect(governorAllowsPartials(light)).toBe(false);
    expect(governorAllowsPartials(minimal)).toBe(false);
    expect(partialShows("words", "listening", light)).toBe(false);
    expect(partialShows("words", "listening", minimal)).toBe(false);
  });

  it("draws the newest words that fit the strip's one line, ellipsis first", () => {
    expect(PARTIAL_MAX_CHARS).toBe(Math.floor(SIDE_DOCK_TEXT_PX / STRIP_CHAR_PX));
    expect(partialTail("short line")).toBe("short line");
    const tail = partialTail(SAMPLE_PARTIAL_TEXT);
    expect(tail.startsWith("…")).toBe(true);
    expect(tail.length).toBeLessThanOrEqual(PARTIAL_MAX_CHARS);
    expect(SAMPLE_PARTIAL_TEXT.endsWith(tail.slice(1))).toBe(true);
    // Cut at a word: the shown tail never starts mid-word.
    const shown = tail.slice(1);
    const before = SAMPLE_PARTIAL_TEXT.charAt(SAMPLE_PARTIAL_TEXT.length - shown.length - 1);
    expect(before).toBe(" ");
    // One word longer than the line keeps its end.
    expect(partialTail("supercalifragilisticexpialidocious", 10)).toBe("…lidocious");
    expect(partialTail("a   b\n c")).toBe("a b c");
  });

  it("redraws at most every 500 ms, trailing edge", () => {
    expect(PARTIAL_RENDER_MIN_MS).toBe(500);
    expect(nextRenderDelay(null, 1000)).toBe(0);
    expect(nextRenderDelay(1000, 1200)).toBe(300);
    expect(nextRenderDelay(1000, 1500)).toBe(0);
    expect(nextRenderDelay(1000, 4000)).toBe(0);
  });

  it("the pill matrix can pin the sample words", () => {
    expect(forcedPartial("?phase=listening&partial=1")).toBe(SAMPLE_PARTIAL_TEXT);
    expect(forcedPartial("?phase=listening")).toBeNull();
  });
});

describe("yap-1106-partial-transcript-stream — never history, telemetry without text", () => {
  it("the hook and the model never invoke a command or store the words", () => {
    for (const src of [here("./usePartialTranscript.ts"), here("./partialTranscript.ts")]) {
      expect(src).not.toMatch(/\binvoke\s*[<(]/);
      expect(src).not.toMatch(/@tauri-apps\/api\/core/);
      expect(src).not.toMatch(/localStorage|sessionStorage|indexedDB/);
    }
  });

  it("the take_end line carries counts and timings, never transcript text", () => {
    const line = partialTakeLogLine({ take: 9, updates: 3, firstMs: 21_034.6, maxGapMs: 9_800.2, hiddenByGovernor: 0 });
    expect(line).toBe("pill partial take_end take=9 updates=3 first_ms=21035 max_gap_ms=9800 hidden_governor=0");
    expect(partialTakeLogLine({ take: 1, updates: 1, firstMs: null, maxGapMs: 0, hiddenByGovernor: 1 })).toContain(
      "first_ms=none",
    );
  });

  it("float-main draws the words only through partialShows, in the strip, behind every line that owns it", () => {
    const main = here("../float-main.tsx");
    expect(main).toContain("usePartialTranscript(");
    expect(main).toMatch(/const partialUp = partialShows\(/);
    expect(main).toContain("!railError && !stillRecording && !fnHint");
    expect(main).toContain('className="partial-words"');
    expect(main).toContain("dataset.partial = partialUp");
    const css = here("../float.css");
    expect(css).toContain('html[data-partial="true"] .phase-strip { opacity: 1; }');
  });
});
