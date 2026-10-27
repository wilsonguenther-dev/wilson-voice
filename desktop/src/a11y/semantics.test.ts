/**
 * Y5-J — the semantic floor, as a sweep over the real sources.
 *
 * Read through Vite (`import.meta.glob(..., ?raw)`), the same way
 * pill/motion.test.ts reads float.css, so the test sees exactly the files the
 * two HTML entries ship.
 */
import { describe, expect, it } from "vitest";

const TSX = import.meta.glob("../**/*.tsx", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const CSS = import.meta.glob("../**/*.css", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const ENTRY = import.meta.glob(["../main.tsx", "../float-main.tsx"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;

const lineOf = (src: string, index: number) => src.slice(0, index).split("\n").length;

describe("a11y — semantics (Y5-J)", () => {
  it("the sweep actually sees the app", () => {
    expect(Object.keys(TSX)).toContain("../App.tsx");
    expect(Object.keys(TSX)).toContain("../pill/ClassicPill.tsx");
    expect(Object.keys(CSS)).toContain("../App.css");
    expect(Object.keys(CSS)).toContain("../float.css");
    expect(Object.keys(CSS)).toContain("./focus.css");
  });

  it("nothing clickable is a div: an action is a <button>", () => {
    // Any non-interactive element with a pointer handler. The pattern is
    // proven to match the shape it hunts (see the self-check below).
    const CLICKABLE = /<(div|span|li|tr|td|section|article|p|img|label|header|footer|aside|nav|ul|i|svg|h[1-6])\b[^<>]*?\bon(Click|MouseDown|PointerDown|MouseUp|PointerUp)=/gs;
    expect('<div className="banner warn" onClick={() => go()}>'.match(CLICKABLE)).not.toBeNull();
    const offenders: string[] = [];
    for (const [file, src] of Object.entries(TSX)) {
      for (const m of src.matchAll(CLICKABLE)) {
        const tag = m[0];
        // A modal backdrop: click-outside to dismiss, with Escape as its
        // keyboard equivalent and a real close button inside.
        if (/role="dialog"/.test(tag) && /"Escape"/.test(src)) continue;
        // ClassicPill's capsule: it CONTAINS buttons (meeting badge), so it
        // cannot itself be a <button>; it is a focusable role=button with
        // Enter/Space handled.
        if (/role="button"/.test(tag) && /tabIndex=\{0\}/.test(tag) && /onKeyDown=/.test(src)) continue;
        offenders.push(`${file}:${lineOf(src, m.index ?? 0)} <${m[1]} on${m[2]}>`);
      }
    }
    expect(offenders).toEqual([]);
  });

  it("no banner is hand-built: src/Banner.tsx is the only way to draw one", () => {
    const handBuilt = Object.entries(TSX)
      .filter(([f]) => f !== "../Banner.tsx")
      .flatMap(([f, src]) => [...src.matchAll(/className="banner (warn|blocked|update)"[^>]*onClick/g)].map((m) => `${f}:${lineOf(src, m.index ?? 0)}`));
    expect(handBuilt).toEqual([]);
    expect(TSX["../App.tsx"]).not.toContain('className="banner warn"');
  });

  it("focus_visible_is_styled_once_globally", () => {
    const declaring = Object.entries(CSS)
      .filter(([, css]) => /:focus-visible\s*[,{]/.test(css.replace(/\/\*[\s\S]*?\*\//g, "")))
      .map(([f]) => f);
    expect(declaring).toEqual(["./focus.css"]);
    const ring = CSS["./focus.css"];
    for (const sel of ["button", "a[href]", "input", "select", "textarea", '[role="button"]']) {
      expect(ring, sel).toContain(`${sel}:focus-visible`);
    }
    expect(ring).toContain("outline: var(--focus-ring-width) solid var(--focus-ring)");
    // Both HTML entries load it.
    expect(ENTRY["../main.tsx"]).toContain('import "./a11y/focus.css"');
    expect(ENTRY["../float-main.tsx"]).toContain('import "./a11y/focus.css"');
    // The pill world re-points the ring at its own always-dark value.
    expect(CSS["../float.css"]).toContain("--focus-ring: var(--focus-ring-pill)");
  });

  it("no rule zeroes the outline", () => {
    // Not just `:focus` rules: Vite loads the shared focus.css BEFORE each
    // entry's own sheet, so a plain `.field input { outline: none }` at equal
    // specificity would beat `input:focus-visible` in the built app.
    const offenders: string[] = [];
    for (const [file, raw] of Object.entries(CSS)) {
      const css = raw.replace(/\/\*[\s\S]*?\*\//g, "");
      for (const m of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
        if (/(^|[;\s])outline\s*:\s*(none|0)\s*(;|$)/m.test(m[2])) offenders.push(`${file}: ${m[1].trim()}`);
      }
    }
    expect(offenders).toEqual([]);
  });

  it("no inline style zeroes the outline either", () => {
    // The CSS sweep above cannot see a React `style={{ ... }}` object, and an
    // inline declaration beats `input:focus-visible` outright (fix round 1:
    // views/Dictionary.tsx's search field shipped `outline: "none"` inline,
    // so a keyboard user tabbing into it got no ring at all).
    const ZEROED = [
      /\boutline\s*:\s*(["'`])\s*(none|0(px)?|transparent)\b/g,
      /\boutline\s*:\s*0\b/g,
      /\boutlineStyle\s*:\s*(["'`])\s*(none|hidden)\b/g,
      /\boutlineWidth\s*:\s*((["'`])\s*0(px)?\b|0\b)/g,
      /\boutlineColor\s*:\s*(["'`])\s*transparent\b/g,
      /style=\{?["'`][^"'`]*outline\s*:\s*(none|0)/g,
    ];
    // Each pattern is proven to match the shape it hunts.
    const shapes = [
      'outline: "none"', "outline: 0", 'outlineStyle: "none"', "outlineWidth: 0", 'outlineColor: "transparent"', 'style="outline:none"',
    ];
    ZEROED.forEach((re, i) => expect(shapes[i].match(re), shapes[i]).not.toBeNull());
    // ...and does not trip on a colour that merely lives under an `outline` key.
    expect(ZEROED.some((re) => 'outline: "#6b3a2e"'.match(re))).toBe(false);
    const offenders: string[] = [];
    for (const [file, raw] of Object.entries(TSX)) {
      // Comments are blanked, not removed, so the reported line is the real one.
      const blank = (c: string) => c.replace(/[^\n]/g, " ");
      const src = raw.replace(/\/\*[\s\S]*?\*\//g, blank).replace(/^\s*\/\/.*$/gm, blank);
      for (const re of ZEROED) {
        for (const m of src.matchAll(re)) offenders.push(`${file}:${lineOf(src, m.index ?? 0)} ${m[0]}`);
      }
    }
    expect(offenders).toEqual([]);
  });
});
