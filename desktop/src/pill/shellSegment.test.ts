/**
 * yap-1107-parity-pill — the right segment lives INSIDE the shell, with the
 * approved page's words, sizes and order (design-2026-11-06/pill.html).
 */
import { describe, expect, it } from "vitest";
import FLOAT_CSS from "../float.css?raw";
import YAPPY_PILL_SRC from "./YappyPill.tsx?raw";
import MANIFEST_SRC from "../../../docs/loop/evidence/parity-pill.manifest.json?raw";
import {
  LOW_POWER_LABEL_MS, OPEN_DESK_SEG_CAPSULE, OPEN_SEG_CAPSULE, SEG_GAP, SEG_PAD_X, SEG_SHELL_GAP, STATUS_CAPSULE, fixedWordsLine,
  segCapsule, segClassFor, segEvent, segmentFor, shellBounds, type SegItem,
} from "./shellSegment";
import { forcedPill, matrixBatteryScene } from "./forced";
import { SAMPLE_BACKTRACK_CAUGHT, backtrackFixedWords, forcedFixedWords } from "./backtrackCaught";

interface ManifestState { id: string; expect: { w: number; h: number; sprite?: { w: number; h: number }[] } }
const MANIFEST = JSON.parse(MANIFEST_SRC) as { states: ManifestState[] };
const row = (id: string) => MANIFEST.states.find((s) => s.id === id)!;
const kinds = (items: SegItem[]) => items.map((s) => s.kind).join("+");
const words = (items: SegItem[]) => items.map((s) => ("text" in s ? s.text + ("sub" in s && s.sub ? " " + s.sub : "") : "")).filter(Boolean).join(" | ");

describe("yap-1107-parity-pill — shell segment copy is the page's, word for word", () => {
  it("segment_items_match_pill_html_per_state", () => {
    expect(kinds(segmentFor({ phase: "listening-locked" }))).toBe("icon+kbd+stop");
    expect(segmentFor({ phase: "listening-locked" })[1]).toEqual({ kind: "kbd", text: "fn" });
    expect(words(segmentFor({ phase: "model-loading" }))).toBe("Warming up");
    expect(kinds(segmentFor({ phase: "model-loading" }))).toBe("text+bar");
    expect(segmentFor({ phase: "transcribing", progress: { chunk: 3, of: 9 } })).toEqual([{ kind: "stack", top: "3 of 9", fraction: 3 / 9 }]);
    expect(words(segmentFor({ phase: "polishing" }))).toBe("Polishing");
    expect(kinds(segmentFor({ phase: "pasting" }))).toBe("icon+text");
    expect(words(segmentFor({ phase: "done", words: 212 }))).toBe("212 words");
    expect(words(segmentFor({ phase: "empty" }))).toBe("No words heard");
    expect(words(segmentFor({ phase: "cancelled" }))).toBe("Cancelled");
    expect(segmentFor({ phase: "error" })).toEqual([{ kind: "text", text: "Didn’t go through" }, { kind: "cta", text: "Retry" }]);
    expect(segmentFor({ phase: "blocked" })).toEqual([{ kind: "icon", icon: "mute" }, { kind: "cta", text: "Allow mic" }]);
    expect(words(segmentFor({ phase: "waiting" }))).toBe("Answer the macOS prompt");
    expect(segmentFor({ phase: "gated" })).toEqual([{ kind: "text", text: "Trial ended" }, { kind: "cta", text: "License" }]);
    expect(words(segmentFor({ phase: "blind" }))).toBe("fn is off here Click to dictate");
  });

  it("window_only_states_have_no_segment", () => {
    for (const phase of ["idle", "sleepy", "listening", "thinking"]) expect(segmentFor({ phase })).toEqual([]);
    expect(segmentFor({ phase: "done", words: 0 })).toEqual([]);
  });

  it("a_bound_chord_replaces_fn_and_never_leaves_an_empty_keycap", () => {
    expect(segmentFor({ phase: "listening-locked", stopKey: "⌥Space" })[1]).toEqual({ kind: "kbd", text: "⌥Space" });
    expect(segmentFor({ phase: "listening-locked", stopKey: "  " })[1]).toEqual({ kind: "kbd", text: "fn" });
  });
});

describe("yap-1107-parity-pill — shell sizes with a segment match the manifest", () => {
  it("status_and_open_capsules_hold_the_measured_windows", () => {
    expect(STATUS_CAPSULE).toEqual({ w: 52, h: 32 });
    expect(OPEN_SEG_CAPSULE).toEqual({ w: 204, h: 52 });
    expect(OPEN_DESK_SEG_CAPSULE).toEqual({ w: 216, h: 52 });
    expect(row("warming-up").expect.h).toBe(STATUS_CAPSULE.h);
    expect(row("warming-up").expect.sprite![0]).toEqual({ w: STATUS_CAPSULE.w - 8, h: STATUS_CAPSULE.h - 8 });
    expect(row("hands-free").expect.h).toBe(OPEN_SEG_CAPSULE.h);
    expect(row("hands-free").expect.sprite![0]).toEqual({ w: OPEN_SEG_CAPSULE.w - 8, h: OPEN_SEG_CAPSULE.h - 8 });
    expect(row("long-take").expect.sprite![0]).toEqual({ w: OPEN_DESK_SEG_CAPSULE.w - 8, h: 44 });
  });

  it("seg_class_open_for_take_phases_status_for_the_rest", () => {
    expect(segClassFor("listening-locked", segmentFor({ phase: "listening-locked" }))).toBe("open");
    expect(segClassFor("done", segmentFor({ phase: "done", words: 3 }))).toBe("open");
    expect(segClassFor("error", segmentFor({ phase: "error" }))).toBe("status");
    expect(segClassFor("idle", [])).toBe("none");
    expect(segCapsule("none")).toBeNull();
    expect(segCapsule("open", true)).toEqual({ w: 216, h: 52 });
  });

  it("shell_width_is_capsule_plus_gap_plus_segment_and_stays_in_the_panel", () => {
    // hands-free: 204 + 4 + 87.2 = 295.2, the manifest's measured shell
    const b = shellBounds(150, OPEN_SEG_CAPSULE, row("hands-free").expect.w - OPEN_SEG_CAPSULE.w - SEG_SHELL_GAP, 400);
    expect(Math.abs(b.w - row("hands-free").expect.w) / row("hands-free").expect.w).toBeLessThan(0.03);
    expect(b.x).toBeCloseTo(150 - b.w / 2, 5);
    const edge = shellBounds(20, OPEN_SEG_CAPSULE, 40, 300, 2);
    expect(edge.x).toBe(2);
    const right = shellBounds(290, OPEN_SEG_CAPSULE, 40, 300, 2);
    expect(right.x + right.w).toBe(298);
    expect(shellBounds(100, STATUS_CAPSULE, 0, 300).w).toBe(52);
  });

  it("seg_event_is_one_greppable_line", () => {
    expect(segEvent("error", "status", segmentFor({ phase: "error" }), 216.4)).toBe("pill event=shell-seg phase=error class=status items=text+cta shell_w=216");
  });
});

describe("yap-1107-parity-pill — the segment is rendered inside the painted shell", () => {
  it("yappy_pill_renders_yappy_seg_inside_the_capsule_box", () => {
    const box = YAPPY_PILL_SRC.indexOf('className="yappy-capsule-box"');
    const seg = YAPPY_PILL_SRC.indexOf('className="yappy-seg"');
    expect(box).toBeGreaterThan(0);
    expect(seg).toBeGreaterThan(box);
    expect(YAPPY_PILL_SRC).toMatch(/shellBounds\(/);
  });

  it("float_css_seg_uses_the_page_gap_padding_and_tokens", () => {
    const rule = FLOAT_CSS.slice(FLOAT_CSS.indexOf(".yappy-seg {"), FLOAT_CSS.indexOf("}", FLOAT_CSS.indexOf(".yappy-seg {")));
    expect(rule).toContain(`gap: ${SEG_GAP}px`);
    expect(rule).toContain(`padding: 0 ${SEG_PAD_X}px`);
    expect(rule).toContain("var(--pill-on-shell)");
    expect(FLOAT_CSS).toMatch(/\.yappy-seg-stop\s*\{[^}]*var\(--pill-shell-2\)/);
    expect(FLOAT_CSS).toMatch(/\.yappy-seg-cta\s*\{[^}]*var\(--pill-cta\)/);
    expect(FLOAT_CSS).toMatch(/html\[data-pill-seg="1"\] \.phase-strip/);
  });
});

describe("yap-1107-parity-pill: saving battery and fix-up notices (pill.html row 20, #wispr W7)", () => {
  it("saving_battery_is_the_batt_icon_and_the_page_words_on_a_resting_pill", () => {
    const items = segmentFor({ phase: "idle", lowPower: true });
    expect(items).toEqual([{ kind: "icon", icon: "batt" }, { kind: "text", text: "Saving battery" }]);
    expect(segClassFor("idle", items)).toBe("status");
    expect(segCapsule(segClassFor("idle", items))).toEqual({ w: 52, h: 32 });
  });
  it("after_the_3_second_label_only_the_icon_stays", () => {
    expect(LOW_POWER_LABEL_MS).toBe(3000);
    expect(segmentFor({ phase: "idle", lowPower: true, lowPowerLabel: false })).toEqual([{ kind: "icon", icon: "batt" }]);
  });
  it("no_low_power_means_the_resting_pill_says_nothing", () => {
    expect(segmentFor({ phase: "idle" })).toEqual([]);
    expect(segmentFor({ phase: "sleepy", lowPower: false })).toEqual([]);
  });
  it("a_done_take_that_fixed_words_says_so_with_show_never_a_separate_toast", () => {
    const items = segmentFor({ phase: "done", words: 212, fixedWords: 2 });
    expect(items).toEqual([{ kind: "icon", icon: "check" }, { kind: "text", text: "Fixed 2 words" }, { kind: "cta", text: "Show" }]);
    expect(segClassFor("done", items)).toBe("open");
    expect(fixedWordsLine(1)).toBe("Fixed 1 word");
    expect(segmentFor({ phase: "done", words: 212, fixedWords: 0 })[1]).toEqual({ kind: "text", text: "212", sub: "words" });
  });
  it("the_battery_scene_is_forced_by_query_and_listed_for_the_matrix", () => {
    expect(forcedPill("?phase=idle&battery=low")?.battery).toBe(true);
    expect(forcedPill("?phase=idle")?.battery).toBe(false);
    expect(matrixBatteryScene()).toMatchObject({ phase: "idle", id: "battery", query: "battery=low" });
  });
});

// yap-1107-parity-pill — W7 "Fix-up notices" is fed by the real Backtrack data:
// float-main turns each backtrack_caught payload into the done pill's count.
describe("parity: the fix-up segment reads the Backtrack payload", () => {
  it("parity_fixup_count_comes_from_backtrack_caught", () => {
    expect(backtrackFixedWords(SAMPLE_BACKTRACK_CAUGHT)).toBe(1); // kept "3"
    expect(backtrackFixedWords({ ...SAMPLE_BACKTRACK_CAUGHT, kept: "  Thursday at noon " })).toBe(3);
    expect(backtrackFixedWords({ ...SAMPLE_BACKTRACK_CAUGHT, kept: "x" })).toBe(1);
    expect(forcedFixedWords("?phase=done&fixed=2")).toBe(2);
    expect(forcedFixedWords("?phase=done")).toBeNull();
    expect(forcedFixedWords("?phase=done&fixed=-1")).toBeNull();
    expect(forcedFixedWords("?phase=done&fixed=abc")).toBeNull();
  });
  it("parity_done_pill_passes_fixed_words_into_the_segment", () => {
    // YappyPill hands its fixedWords prop to segmentFor; float-main feeds it.
    expect(YAPPY_PILL_SRC).toMatch(/segmentFor\(\{[\s\S]*?fixedWords,[\s\S]*?\}\)/);
    const wispr = (JSON.parse(MANIFEST_SRC) as { states: Array<{ id: string; app: { query: string } }> }).states.find((x) => x.id === "wispr")!;
    expect(wispr.app.query).toMatch(/&fixed=2$/);
  });
});
