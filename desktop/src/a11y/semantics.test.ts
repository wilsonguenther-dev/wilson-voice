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
});
