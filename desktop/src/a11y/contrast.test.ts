/**
 * Y5-J — the contrast floor, measured on the TOKEN layer.
 *
 * tokens.css is the one place a colour is allowed to be declared (Y5-A), so it
 * is the one place contrast can be guaranteed: a rule-level colour that does
 * not come from a token is already a Y5-A defect. This test reads the real
 * file (not a copy of its values), resolves every `var()` chain, composites
 * every translucent surface over the ground it actually sits on, and holds
 * each declared pair to its WCAG 2.x floor:
 *
 *   4.5:1  body text                          (SC 1.4.3)
 *   3:1    large text and UI boundaries       (SC 1.4.3 large / SC 1.4.11)
 *
 * Both themes are checked: the dark block in tokens.css overrides the light
 * map, exactly as the cascade does. A failing pair is a failed item, not a
 * TODO — the fix is the token, never a lower floor in this table.
 */
// No @types/node in this project (see pill/live.test.ts for the same shape).
// @ts-expect-error -- node builtin, untyped here; the test runs under Node.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const TOKENS = readFileSync(new URL("../tokens.css", import.meta.url), "utf8") as string;

type RGBA = { r: number; g: number; b: number; a: number };
type Theme = "light" | "dark";

/** Split tokens.css into its light `:root` and its dark `@media` override. */
function themeMaps(css: string): Record<Theme, Map<string, string>> {
  const src = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const cut = src.indexOf("@media (prefers-color-scheme: dark)");
  if (cut < 0) throw new Error("tokens.css has no dark block — the parser is reading the wrong file");
  const decls = (s: string) => {
    const m = new Map<string, string>();
    for (const d of s.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) m.set(d[1], d[2].trim());
    return m;
  };
  const light = decls(src.slice(0, cut));
  const dark = new Map(light);
  for (const [k, v] of decls(src.slice(cut))) dark.set(k, v);
  return { light, dark };
}

function resolve(map: Map<string, string>, name: string, seen: string[] = []): string {
  if (seen.includes(name)) throw new Error(`var() cycle: ${[...seen, name].join(" -> ")}`);
  const v = map.get(name);
  if (v === undefined) throw new Error(`token ${name} is not declared in tokens.css`);
  const ref = /^var\((--[\w-]+)\)$/.exec(v);
  return ref ? resolve(map, ref[1], [...seen, name]) : v;
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return [f(0) * 255, f(8) * 255, f(4) * 255];
}

function parseColor(v: string, alpha?: number): RGBA {
  let m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(v);
  if (m) {
    const h = m[1].length === 3 ? [...m[1]].map((c) => c + c).join("") : m[1];
    return { r: parseInt(h.slice(0, 2), 16), g: parseInt(h.slice(2, 4), 16), b: parseInt(h.slice(4, 6), 16), a: alpha ?? 1 };
  }
  m = /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)$/.exec(v);
  if (m) return { r: +m[1], g: +m[2], b: +m[3], a: m[4] === undefined ? (alpha ?? 1) : +m[4] };
  // A bare HSL triplet — the pill-world roles float.css wraps in hsl(… / a).
  m = /^([\d.]+)\s+([\d.]+)%\s+([\d.]+)%$/.exec(v);
  if (m) {
    const [r, g, b] = hslToRgb(+m[1], +m[2] / 100, +m[3] / 100);
    return { r, g, b, a: alpha ?? 1 };
  }
  throw new Error(`unparseable colour "${v}"`);
}

const over = (top: RGBA, under: RGBA): RGBA => ({
  r: top.r * top.a + under.r * (1 - top.a),
  g: top.g * top.a + under.g * (1 - top.a),
  b: top.b * top.a + under.b * (1 - top.a),
  a: 1,
});

const lum = ({ r, g, b }: RGBA) => {
  const ch = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b);
};

export function contrastRatio(a: RGBA, b: RGBA): number {
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const BODY = 4.5;
const UI = 3;

interface Pair {
  fg: string;
  bg: string;
  /** What a translucent `bg` is laid over. Defaults to the page itself. */
  ground?: string;
  /** For bare HSL triplet tokens: the alpha the stylesheet wraps them in. */
  bgAlpha?: number;
  floor: number;
  where: string;
  themes?: Theme[];
}

const PAGE_SURFACES = ["--surface", "--surface-card", "--surface-sidebar", "--surface-raised"];

/**
 * Every pair a rule actually draws. Grouped by role so a reader can audit the
 * table against App.css / float.css in one pass.
 */
const PAIRS: Pair[] = [
  // ── body text on every page surface ──────────────────────────────────────
  ...PAGE_SURFACES.flatMap((bg) => [
    { fg: "--text-primary", bg, floor: BODY, where: "prose, headings, card titles" },
    { fg: "--text-muted", bg, floor: BODY, where: "ledes, stat labels, card meta" },
  ]),
  // ── semantic inks: text that wears a hue ─────────────────────────────────
  { fg: "--accent-action-ink", bg: "--surface", floor: BODY, where: "accent text, active profile title" },
  { fg: "--accent-action-ink", bg: "--accent-action-soft", floor: BODY, where: "accent chip text inside its own soft chip" },
  { fg: "--accent-data-ink", bg: "--surface", floor: BODY, where: "data / info text" },
  { fg: "--accent-data-ink", bg: "--accent-data-soft", floor: BODY, where: "info chip text inside its own soft chip" },
  { fg: "--accent-urgent-ink", bg: "--surface", floor: BODY, where: "error text, blocked banner" },
  { fg: "--accent-caution-ink", bg: "--surface", floor: BODY, where: "warning banner text" },
  // ── text on saturated fills ──────────────────────────────────────────────
  { fg: "--text-on-accent", bg: "--accent-action", floor: BODY, where: "button.primary, dictate button, the ✓ badge" },
  { fg: "--text-on-fill", bg: "--accent-urgent", floor: BODY, where: "onboarding record button (live)" },
  { fg: "--text-on-fill", bg: "--status-live-fill", floor: BODY, where: "dictate-side.live" },
  { fg: "--text-on-inverse", bg: "--surface-inverse", floor: BODY, where: "toast" },
  { fg: "--accent-on-inverse", bg: "--surface-inverse", floor: BODY, where: "toast action" },
  { fg: "--retro-paper", bg: "--retro-wood-deep", floor: BODY, where: "house tag lettering" },
  // ── the pill's always-dark world ─────────────────────────────────────────
  { fg: "--pill-text", bg: "--pill-shell-top", floor: BODY, where: "pill capsule text" },
  { fg: "--pill-text", bg: "--pill-shell-bottom", floor: BODY, where: "pill capsule text" },
  { fg: "--pill-bubble-text", bg: "--pill-bubble-surface", floor: BODY, where: "Yappy's speech bubble" },
  { fg: "--text-on-fill", bg: "--pill-shell-highlight", floor: BODY, where: "pill .ctrl glyph" },
  {
    fg: "--text-on-fill", bg: "--accent-meeting", bgAlpha: 0.34, ground: "--pill-shell-top",
    floor: UI, where: "meeting .rec-stop glyph (a 12px icon: a UI graphic, SC 1.4.11)",
  },
  // ── UI boundaries: the focus ring against everything it can sit on ───────
  ...PAGE_SURFACES.map((bg) => ({ fg: "--focus-ring", bg, floor: UI, where: "global :focus-visible ring (main window)" })),
  ...["--pill-shell-top", "--pill-shell-bottom", "--pill-shell-highlight", "--pill-bubble-surface"].map((bg) => ({
    fg: "--focus-ring-pill", bg, floor: UI, where: "global :focus-visible ring (float pill)",
  })),
];

/** Text-bearing tokens: each must be held to a floor by at least one pair. */
const TEXT_ROLE = /^--(text-[\w-]+|pill-text|pill-bubble-text|accent-[a-z]+-ink|accent-on-inverse|focus-ring[\w-]*)$/;

function measure(map: Map<string, string>, p: Pair): number {
  const ground = parseColor(resolve(map, p.ground ?? "--surface"));
  const bg = over(parseColor(resolve(map, p.bg), p.bgAlpha), ground);
  const fg = over(parseColor(resolve(map, p.fg)), bg);
  return contrastRatio(fg, bg);
}

describe("a11y — contrast floors on the token layer (Y5-J)", () => {
  const maps = themeMaps(TOKENS);

  it("the ratio maths matches the WCAG reference points", () => {
    const black = parseColor("#000");
    const white = parseColor("#fff");
    expect(contrastRatio(black, white)).toBeCloseTo(21, 5);
    expect(contrastRatio(parseColor("#767676"), white)).toBeGreaterThan(4.5); // the canonical AA grey
    expect(contrastRatio(parseColor("#777777"), white)).toBeLessThan(4.5);
  });

  it("every_token_pair_meets_its_contrast_floor", () => {
    const failures: string[] = [];
    for (const theme of ["light", "dark"] as Theme[]) {
      for (const p of PAIRS) {
        if (p.themes && !p.themes.includes(theme)) continue;
        const ratio = measure(maps[theme], p);
        if (ratio < p.floor) {
          failures.push(
            `${theme}: ${p.fg} on ${p.bg}${p.bgAlpha ? ` @${p.bgAlpha}` : ""} = ${ratio.toFixed(2)}:1 < ${p.floor}:1 (${p.where})`,
          );
        }
      }
    }
    expect(failures).toEqual([]);
  });

  it("every text-role token is held to a floor by at least one pair", () => {
    const covered = new Set(PAIRS.map((p) => p.fg));
    // A type-scale step (--text-xs: 0.72rem) shares the prefix; only a token
    // that resolves to a COLOUR is a text role.
    const isColour = (k: string) => {
      try {
        parseColor(resolve(maps.light, k));
        return true;
      } catch {
        return false;
      }
    };
    const orphans = [...maps.light.keys()].filter((k) => TEXT_ROLE.test(k) && isColour(k) && !covered.has(k));
    expect(orphans).toEqual([]);
  });
});
