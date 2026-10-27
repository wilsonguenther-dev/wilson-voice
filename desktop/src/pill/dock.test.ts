/**
 * Y5-I — the golden geometry test for the docked bar.
 *
 * The table (`dock-geometry.json`) is the SHELL's: the window content box for
 * every phase x dock, with no style dimension in it. These tests hold three
 * things to it:
 *   1. the numbers are the parity research's golden table (section 4.2);
 *   2. the orientation rules Wispr learned the hard way — no state is thicker
 *      than the 30 px strip on a side dock, banners stay horizontal in every
 *      dock, the waveform inverts its axis on a side dock;
 *   3. float.css DRAWS exactly that box: a small cascade over the real
 *      stylesheet (`?raw`, vitest css: true) resolves the bar's width / height /
 *      direction from the shell's own root props, for every phase x dock.
 *
 * What 3 is NOT: a layout engine. vitest runs without a DOM, so the box is
 * resolved from the declarations float.css makes, not measured off a painted
 * element. The painted measurement is `scripts/pill-matrix.mjs` in headless
 * Chrome; this test is the part that runs on every `npm test`.
 */
import { describe, expect, it } from "vitest";
import FLOAT_CSS from "../float.css?raw";
import TOKENS_CSS from "../tokens.css?raw";
import GEOMETRY from "./dock-geometry.json";
import { ALL_LIVE_PHASES } from "./live";
import {
  BAR_DOCKS, BAR_PHASES, STRIP_PX, barAxis, barBox, barDock, barOrientation, barRootProps,
  barTokens, shellBarPhase, waveAxis, type BarDock, type BarPhase,
} from "./dock";

const SIDE: readonly BarDock[] = ["left", "right"];

/* ── a deliberately small cascade over the real float.css ─────────────────── */

interface Rule { selectors: string[]; decls: Record<string, string>; order: number }

/** Top-level style rules only; @media / @keyframes / @property blocks are skipped. */
function topLevelRules(css: string): Rule[] {
  const src = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const rules: Rule[] = [];
  let i = 0;
  let order = 0;
  while (i < src.length) {
    const open = src.indexOf("{", i);
    if (open < 0) break;
    const head = src.slice(i, open).trim();
    let depth = 1;
    let j = open + 1;
    while (j < src.length && depth > 0) {
      if (src[j] === "{") depth += 1;
      else if (src[j] === "}") depth -= 1;
      j += 1;
    }
    const body = src.slice(open + 1, j - 1);
    if (!head.startsWith("@")) {
      const decls: Record<string, string> = {};
      for (const d of body.split(";")) {
        const k = d.indexOf(":");
        if (k > 0) decls[d.slice(0, k).trim()] = d.slice(k + 1).trim();
      }
      rules.push({ selectors: head.split(",").map((x) => x.trim().replace(/\s+/g, " ")), decls, order: order++ });
    }
    i = j;
  }
  return rules;
}

const RULES = topLevelRules(FLOAT_CSS);

/** The `.bar` root's attributes, exactly as the shell renders them. */
function rootAttrs(phase: BarPhase, dock: BarDock): Record<string, string> {
  const p = barRootProps(phase, dock) as unknown as Record<string, string>;
  return Object.fromEntries(Object.entries(p).filter(([k]) => k.startsWith("data-")));
}

/**
 * Does `sel` match element `el` inside the shell's root? Only the two shapes the
 * bar uses are understood: `el` alone, or `[data-…="v"] el`. Anything else
 * (state classes, :not(), siblings) is outside this contract and never matches.
 */
function matches(sel: string, el: string, attrs: Record<string, string>): number {
  if (sel === el) return 10;
  const m = /^\[(data-[a-z-]+)="([^"]+)"\] (\S+)$/.exec(sel);
  if (m && m[3] === el && attrs[m[1]] === m[2]) return 20;
  return 0;
}

/** Cascaded declarations for `el` under the root: specificity, then source order. */
function cascade(el: string, attrs: Record<string, string>): Record<string, string> {
  const hits: Array<{ spec: number; order: number; decls: Record<string, string> }> = [];
  for (const r of RULES) {
    const spec = Math.max(0, ...r.selectors.map((s) => matches(s, el, attrs)));
    if (spec > 0) hits.push({ spec, order: r.order, decls: r.decls });
  }
  hits.sort((a, b) => a.spec - b.spec || a.order - b.order);
  return Object.assign({}, ...hits.map((h) => h.decls));
}

/** Resolve `var(--x)` against the shell's inline tokens, then tokens.css. */
function resolveVar(value: string, inline: Record<string, string>): string {
  return value.replace(/var\((--[a-z-]+)\)/g, (_, name: string) => {
    if (inline[name] !== undefined) return inline[name];
    const m = new RegExp(`\\n\\s*${name}:\\s*([^;]+);`).exec(TOKENS_CSS);
    if (!m) throw new Error(`unresolved token ${name}`);
    return m[1].trim();
  });
}

/** The box float.css draws for the `.pill` element at one phase x dock. */
function drawnBox(phase: BarPhase, dock: BarDock) {
  const props = barRootProps(phase, dock);
  const inline = props.style as Record<string, string>;
  const d = cascade(".pill", rootAttrs(phase, dock));
  const px = (v: string) => {
    const r = resolveVar(v, inline);
    const m = /^(\d+(?:\.\d+)?)px$/.exec(r);
    if (!m) throw new Error(`not a px length: ${v} -> ${r}`);
    return Number(m[1]);
  };
  return { width: px(d.width), height: px(d.height), direction: d["flex-direction"] };
}

/* ── the golden table, verbatim from the parity research section 4.2 ─────── */
const GOLDEN: Array<[BarPhase, { bottom: [number, number]; side: [number, number] }]> = [
  ["idle", { bottom: [40, 8], side: [8, 40] }], // resting 8x40
  ["model-loading", { bottom: [50, 30], side: [30, 50] }], // ready 30x50
  ["listening", { bottom: [102.5, 30], side: [30, 102.5] }], // activePopo 30x102.5
  ["thinking", { bottom: [98, 30], side: [98, 30] }], // processing, stays horizontal
  ["pasting", { bottom: [98, 30], side: [98, 30] }], // processing, stays horizontal
  ["polishing", { bottom: [136, 30], side: [30, 136] }], // polishProcessing 136x30, column on a side dock
  ["done", { bottom: [152, 30], side: [152, 30] }], // polishCompleted, side dock 152x30
  ["error", { bottom: [91, 30], side: [91, 30] }], // error 30x91, stays horizontal
];

describe("Y5-I dock geometry — the shell's table", () => {
  it("the_table_covers_every_phase_and_carries_no_style_dimension", () => {
    expect([...BAR_PHASES].sort()).toEqual([...ALL_LIVE_PHASES, "meeting-on-screen"].sort());
    expect(BAR_DOCKS).toEqual(["bottom", "left", "right"]);
    expect(Object.keys(GEOMETRY).sort()).toEqual(["about", "docks", "phases", "strip_px"]);
    for (const p of BAR_PHASES) {
      const row = (GEOMETRY.phases as Record<string, Record<string, unknown>>)[p];
      // The only dimensions are the docks. A character key here would fork the
      // table per creature, which the owner decision forbids.
      for (const k of Object.keys(row)) {
        expect(["wispr", "orientation", "yap_only", "why", "bottom", "left", "right"], `${p}.${k}`).toContain(k);
      }
      expect(["strip", "banner"]).toContain(barOrientation(p));
      expect(String(row.why).length, `${p} documents its orientation`).toBeGreaterThan(20);
    }
    expect(JSON.stringify(GEOMETRY)).not.toMatch(/pill_?style|pillStyle|classic|yappy/i);
  });

  it("wispr_rows_match_the_parity_golden_table", () => {
    for (const [p, g] of GOLDEN) {
      expect(barBox(p, "bottom"), `${p}@bottom`).toEqual({ width: g.bottom[0], height: g.bottom[1] });
      for (const d of SIDE) expect(barBox(p, d), `${p}@${d}`).toEqual({ width: g.side[0], height: g.side[1] });
    }
  });

  it("no_state_exceeds_the_thirty_pixel_strip_on_a_side_dock", () => {
    expect(STRIP_PX).toBe(30);
    expect(/--flow-bar-strip:\s*30px;/.test(TOKENS_CSS)).toBe(true);
    for (const p of BAR_PHASES) {
      for (const d of BAR_DOCKS) {
        // Thickness is the dimension ACROSS the bar; it never exceeds the strip
        // in any dock, at any phase.
        expect(barTokens(p, d).thickness, `${p}@${d} thickness`).toBeLessThanOrEqual(STRIP_PX);
      }
      for (const d of SIDE) {
        const b = barBox(p, d);
        if (barOrientation(p) === "strip") {
          // A phase in the strip is a column no wider than the strip itself.
          expect(b.width, `${p}@${d} width`).toBeLessThanOrEqual(STRIP_PX);
          expect(b.height, `${p}@${d} is a column`).toBeGreaterThan(b.width);
        } else {
          // A banner never enters the strip: it is a row no taller than it.
          expect(b.height, `${p}@${d} height`).toBeLessThanOrEqual(STRIP_PX);
        }
        // And what float.css draws obeys the same bound.
        const drawn = drawnBox(p, d);
        expect(Math.min(drawn.width, drawn.height), `${p}@${d} drawn`).toBeLessThanOrEqual(STRIP_PX);
      }
    }
  });

  it("banner_states_stay_horizontal_in_every_dock", () => {
    // The research names them: processing, completion and error. Yap's own
    // labelled phases join them by the 30 px crush rule.
    for (const p of ["thinking", "pasting", "transcribing", "done", "error", "empty", "cancelled",
      "gated", "waiting", "blocked", "meeting-on-screen"] as BarPhase[]) {
      expect(barOrientation(p), p).toBe("banner");
    }
    for (const p of BAR_PHASES.filter((x) => barOrientation(x) === "banner")) {
      for (const d of BAR_DOCKS) {
        expect(barAxis(p, d), `${p}@${d}`).toBe("row");
        expect(barBox(p, d), `${p}@${d} same box as bottom`).toEqual(barBox(p, "bottom"));
        const drawn = drawnBox(p, d);
        expect(drawn.direction, `${p}@${d} drawn direction`).toBe("row");
        expect(drawn.width, `${p}@${d} drawn wider than tall`).toBeGreaterThan(drawn.height);
      }
    }
  });

  it("strip_states_rotate_into_a_column_on_a_side_dock", () => {
    for (const p of ["idle", "sleepy", "model-loading", "listening", "polishing"] as BarPhase[]) {
      expect(barOrientation(p), p).toBe("strip");
      expect(barAxis(p, "bottom")).toBe("row");
      const bottom = barBox(p, "bottom");
      for (const d of SIDE) {
        expect(barAxis(p, d), `${p}@${d}`).toBe("column");
        // Same tokens, other axis: length and thickness are orientation-neutral.
        expect(barTokens(p, d)).toEqual(barTokens(p, "bottom"));
        expect(barBox(p, d)).toEqual({ width: bottom.height, height: bottom.width });
        expect(drawnBox(p, d).direction, `${p}@${d} drawn`).toBe("column");
      }
    }
  });

  it("every_phase_x_dock_box_is_what_float_css_draws", () => {
    for (const p of BAR_PHASES) {
      for (const d of BAR_DOCKS) {
        const { width, height } = drawnBox(p, d);
        expect({ width, height }, `${p}@${d}`).toEqual(barBox(p, d));
      }
    }
  });

  it("waveform_axis_inverts_on_a_side_dock", () => {
    // The axis is a token: --flow-wave-on-x is 1 in the column base (2 px bars
    // growing along X) and the row override flips it to 0.
    expect(/--flow-wave-on-x:\s*1;/.test(TOKENS_CSS)).toBe(true);
    expect(/--flow-wave-bar:\s*2px;/.test(TOKENS_CSS)).toBe(true);
    const bar = cascade(".wave i", {});
    expect(bar.width).toContain("var(--flow-wave-on-x) * var(--wave-extent)");
    expect(bar.height).toContain("(1 - var(--flow-wave-on-x)) * var(--wave-extent)");
    for (const p of BAR_PHASES) {
      for (const d of BAR_DOCKS) {
        const wave = cascade(".wave", rootAttrs(p, d));
        const onX = wave["--flow-wave-on-x"] ?? "1"; // unset = the tokens.css base
        const axis = onX === "1" ? "x" : "y";
        expect(axis, `${p}@${d} css`).toBe(waveAxis(p, d));
        expect(wave["flex-direction"] ?? "", `${p}@${d}`).toBe(axis === "x" ? "column" : "row");
      }
    }
    expect(waveAxis("listening", "left")).toBe("x");
    expect(waveAxis("listening", "right")).toBe("x");
    expect(waveAxis("listening", "bottom")).toBe("y");
  });

  it("the_capsule_is_never_rotated_and_no_other_rule_sizes_the_box", () => {
    expect(FLOAT_CSS).not.toMatch(/transform:\s*rotate/);
    for (const r of RULES) {
      for (const s of r.selectors) {
        if (!/\.pill\b/.test(s) || /\.pill\b\S*\s+\S/.test(s)) continue; // descendants are content, not the box
        const own = ["width", "height", "min-width", "max-width", "min-height", "max-height"]
          .filter((k) => r.decls[k] !== undefined);
        if (own.length === 0) continue;
        // Only the base and the row override may size the bar, and only with the tokens.
        expect([".pill", '[data-bar-position="bottom"] .pill', '[data-bar-orientation="banner"] .pill'], s).toContain(s);
        for (const k of own) expect(r.decls[k], `${s} ${k}`).toMatch(/^var\(--flow-bar-(length|thickness)\)$/);
      }
    }
    // Declared ONCE, in tokens.css; float.css only consumes them.
    expect(/--flow-bar-length:/.test(TOKENS_CSS) && /--flow-bar-thickness:/.test(TOKENS_CSS)).toBe(true);
    expect(FLOAT_CSS).not.toMatch(/--flow-bar-(length|thickness)\s*:/);
  });

  it("a_meeting_takes_the_bar_only_when_no_take_is_on_screen", () => {
    expect(shellBarPhase("idle", true)).toBe("meeting-on-screen");
    expect(shellBarPhase("sleepy", true)).toBe("meeting-on-screen");
    expect(shellBarPhase("listening", true)).toBe("listening");
    expect(shellBarPhase("transcribing", true)).toBe("transcribing");
    expect(shellBarPhase("idle", false)).toBe("idle");
    expect(barDock("left")).toBe("left");
    expect(barDock("top")).toBe("bottom");
    expect(barDock(undefined)).toBe("bottom");
  });
});
