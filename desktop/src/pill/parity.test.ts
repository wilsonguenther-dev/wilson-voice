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

interface ManifestState { id: string; section: string; app: { selector: string } }
const MANIFEST = JSON.parse(MANIFEST_SRC) as { states: ManifestState[] };

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

  it("manifest_has_all_23_state_rows_and_every_section", () => {
    expect(ids.filter((s) => s.section === "#states")).toHaveLength(23);
    for (const sec of ["#timer", "#hoot", "#warm", "#props", "#vdock", "#trial", "#heavy", "#wispr", "#chatshots"]) {
      expect(ids.some((s) => s.section === sec), sec).toBe(true);
    }
    for (const s of ids) expect(s.app.selector, s.id).toBe(".yappy-capsule-box");
    expect(new Set(ids.map((s) => s.id)).size).toBe(ids.length);
  });
});
