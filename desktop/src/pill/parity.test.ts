/**
 * yap-1107-parity-pill — the approved Pill page's colours and bubble, pinned.
 *
 * Wilson, 2026-11-07: the built pill must match design-2026-11-06/pill.html
 * EXACTLY ("no degrading, never degrading"); every colour an exact hex match.
 * The measured truth lives in docs/loop/evidence/parity-pill.manifest.json
 * (scripts/parity-crops.mjs on the approved page). These tests pin the token
 * layer and the CSS that consumes it, so a later edit cannot drift a colour
 * back without a red test; the pixel side is scripts/parity-sheet.mjs --check.
 */
import { describe, expect, it } from "vitest";
import FLOAT_CSS from "../float.css?raw";
import TOKENS_CSS from "../tokens.css?raw";
import YAPPY_PILL_SRC from "./YappyPill.tsx?raw";
import MANIFEST_SRC from "../../../docs/loop/evidence/parity-pill.manifest.json?raw";
import { matrixBacktrackCaughtScene, matrixDragBlobScene, matrixPhases, matrixRailScene } from "./forced";

interface ManifestState {
  id: string;
  section: string;
  artifact: { selector: string };
  app: { selector: string; query: string; capture: string };
}
interface NonVisual { id: string; section: string; reason: string }
const MANIFEST = JSON.parse(MANIFEST_SRC) as { states: ManifestState[]; nonVisual?: NonVisual[] };

/** The value of a custom property in tokens.css (first declaration, the light/base block). */
function token(name: string): string {
  const m = TOKENS_CSS.match(new RegExp(`${name.replace(/[-]/g, "\\-")}:\\s*([^;]+);`));
  return m ? m[1].trim().toLowerCase() : "";
}

/** One CSS rule's body from float.css (the first rule whose selector is exactly `sel`). */
function rule(sel: string): string {
  const esc = sel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const m = FLOAT_CSS.match(new RegExp(`(^|\\n)${esc}\\s*\\{([^}]*)\\}`));
  return m ? m[2] : "";
}

// pill.html (review.css :root and the .ypill / .ybub / .bar rules), measured.
const APPROVED = {
  "--pill-shell": "#1e2025",
  "--pill-shell-top": "#1e2025",
  "--pill-shell-bottom": "#1e2025",
  "--pill-shell-2": "#2b2e35",
  "--pill-text": "#eceae4",
  "--pill-on-shell": "#eceae4",
  "--pill-on-shell-2": "#a9a59c",
  "--pill-bubble-surface": "#fffdf7",
  "--pill-bubble-text": "#1d1c1a",
  "--pill-bar-fill": "#f2c14e",
  "--pill-warmup-fill": "#f2c14e",
  "--pill-gate-cta-surface": "#efece4",
  "--pill-gate-cta-text": "#1d1c1a",
} as const;

describe("yap-1107-parity-pill — the approved pill's colours", () => {
  it("pill_tokens_are_the_approved_hex_exactly", () => {
    for (const [name, hex] of Object.entries(APPROVED)) expect(token(name), name).toBe(hex);
    expect(token("--pill-shell-line").replace(/\s+/g, "")).toBe("rgba(255,255,255,0.09)");
    expect(token("--pill-shell-shadow").replace(/\s+/g, " ")).toBe("0 6px 16px rgba(0, 0, 0, 0.28)");
  });

  it("bubble_matches_the_approved_ybub", () => {
    const b = rule(".kami-bubble");
    expect(b).toMatch(/background:\s*var\(--pill-bubble-surface\)/);
    expect(b).toMatch(/color:\s*var\(--pill-bubble-text\)/);
    expect(b).toMatch(/padding:\s*6px 10px/);
    expect(b).toMatch(/font-size:\s*12px/);
    expect(b).toMatch(/font-weight:\s*500/);
    expect(b).toMatch(/line-height:\s*1\.2/);
    expect(b).toMatch(/border-radius:\s*var\(--pill-bubble-radius\)/);
    expect(token("--pill-bubble-radius")).toBe("9px");
    expect(b).toMatch(/border:\s*0/);
    // the tail is the bubble's own colour, no rim (pill.html .ybub::before)
    expect(rule(".kami-bubble::after")).not.toMatch(/border-(right|bottom)/);
  });

  it("fill_bars_and_meeting_shell_paint_the_approved_tokens", () => {
    expect(rule(".kami-progress-track > i")).toMatch(/background:\s*var\(--pill-bar-fill\)/);
    expect(rule(".warmup-bar > i")).toMatch(/background:\s*var\(--pill-warmup-fill\)/);
    const shell = rule('html[data-one-pill="true"][data-meeting-shell="true"] .kami-stage .meeting-shell');
    expect(shell).toMatch(/background:\s*var\(--pill-shell\)/);
    expect(shell).toMatch(/inset 0 0 0 1px var\(--pill-shell-line\)/);
  });

  it("canvas_shell_reads_the_token_not_a_stale_literal", () => {
    // the side-dock shell and the capsule's 1 px border paint --pill-shell
    expect(YAPPY_PILL_SRC).toMatch(/getPropertyValue\("--pill-shell"\)/);
    expect(YAPPY_PILL_SRC).not.toMatch(/fillStyle = "#1d1c1a"/);
    // the capsule's DOM twin the parity sheet measures
    expect(YAPPY_PILL_SRC).toMatch(/className="yappy-capsule-box"/);
    expect(YAPPY_PILL_SRC).toMatch(/syncCapsuleBox\(x0, y0, capW, capH, "capsule"\)/);
    expect(rule(".yappy-capsule-box")).toMatch(/pointer-events:\s*none/);
  });
});

describe("yap-1107-parity-pill — the manifest lists every approved state", () => {
  const ids = MANIFEST.states;
  const nonVisual = MANIFEST.nonVisual ?? [];

  it("manifest_has_all_23_state_rows_and_every_section", () => {
    expect(ids.filter((s) => s.section === "#states")).toHaveLength(23);
    for (const sec of ["#timer", "#hoot", "#warm", "#props", "#vdock", "#trial", "#heavy", "#wispr", "#chatshots"]) {
      const listed = ids.some((s) => s.section === sec) || nonVisual.some((n) => n.section === sec);
      expect(listed, sec).toBe(true);
    }
    for (const s of ids) expect(s.app.selector, s.id).toBe(s.id === "dragging" ? ".kami-blob" : ".yappy-capsule-box");
    expect(new Set(ids.map((s) => s.id)).size).toBe(ids.length);
  });

  // batch-4 bounce: section selectors (#clocks, #heavy...) measured 1176 px page
  // sections against a pill. Every measured row now names ONE drawn pill.
  it("every_artifact_selector_is_one_pill_not_a_page_section", () => {
    for (const s of ids) expect(s.artifact.selector, s.id).toMatch(/\.(ypill|blob)$/);
  });

  // #heavy is a behaviour table and #chatshots are pre-redesign defect
  // screenshots: listed with a reason, never measured as a pill.
  it("non_visual_sections_carry_a_reason", () => {
    expect(nonVisual.map((n) => n.id).sort()).toEqual(["chatshots", "heavy"]);
    for (const n of nonVisual) expect(n.reason.length, n.id).toBeGreaterThan(40);
  });

  // batch-4 bounce: capture names that pill-matrix never writes. The matrix
  // writes <captures>/app/<row id>__yappy__<dock>__<meeting>.png for docks
  // bottom|left|right and meetings none|recording|saved, navigating with
  // ?phase=<row phase>; the manifest must name exactly those files.
  it("every_capture_is_a_file_pill_matrix_writes", () => {
    const rows = [...matrixPhases(), matrixRailScene(), matrixDragBlobScene(), matrixBacktrackCaughtScene()];
    const byId = new Map<string, { phase: string; query?: string }>(
      rows.map((r): [string, { phase: string; query?: string }] => [r.id, r]),
    );
    for (const s of ids) {
      const m = s.app.capture.match(/^app\/(.+)__yappy__(bottom|left|right)__(none|recording|saved)\.png$/);
      expect(m, s.id + " " + s.app.capture).not.toBeNull();
      if (!m) continue;
      const row = byId.get(m[1]);
      expect(row, s.id + ": no matrix row " + m[1]).toBeDefined();
      if (!row) continue;
      const q = new URLSearchParams(s.app.query);
      expect(q.get("phase"), s.id).toBe(row.phase);
      expect(q.get("dock"), s.id).toBe(m[2]);
      expect(q.get("meeting"), s.id).toBe(m[3]);
      expect(q.get("pill"), s.id).toBe("yappy");
      if (row.query) for (const [k, v] of new URLSearchParams(row.query)) expect(q.get(k), s.id + " " + k).toBe(v);
    }
  });
});

describe("yap-1107-parity-pill — the drag blob is the approved .blob", () => {
  it("blob_is_a_real_36px_disc_with_the_approved_rim_and_shadow", () => {
    expect(YAPPY_PILL_SRC).toMatch(/<span className="kami-blob" aria-hidden="true" \/>/);
    const b = rule('html[data-pill-drag="blob"] .kami-blob');
    expect(b).toMatch(/width:\s*calc\(var\(--blob-d\) \+ 2 \* var\(--blob-rim\)\)/);
    expect(b).toMatch(/height:\s*calc\(var\(--blob-d\) \+ 2 \* var\(--blob-rim\)\)/);
    expect(b).toMatch(/background:\s*var\(--pill-shell-top\)/);
    expect(b).toMatch(/inset 0 0 0 1px var\(--pill-shell-line\)/);
    expect(b).toMatch(/var\(--pill-blob-shadow\)/);
    // 28 px face + 2 x 4 px rim = pill.html .blob 36 x 36
    expect(FLOAT_CSS).toMatch(/--blob-d:\s*28px/);
    expect(FLOAT_CSS).toMatch(/--blob-rim:\s*4px/);
    expect(token("--pill-blob-shadow").replace(/\s+/g, " ")).toBe("0 10px 22px rgba(0, 0, 0, 0.38)");
    expect(rule(".kami-blob")).toMatch(/display:\s*none/);
  });
});
