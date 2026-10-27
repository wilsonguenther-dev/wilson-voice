/**
 * Y5-A — the token layer is the ONLY place a colour literal may live.
 *
 * The acceptance gate greps App.css and float.css for hex literals; this test
 * holds the same line inside `npm test`, so a later item that pastes a `#fff`
 * into either sheet fails here on the PR instead of at the next audit. It
 * also closes the hole a migration opens: replacing a literal with a
 * `var(--name)` nobody declares compiles, builds and paints NOTHING (an unset
 * custom property falls back to the property's initial value), so every
 * fallback-less `var()` the two sheets call must resolve to a declaration.
 */
// No @types/node in this project (see pill/live.test.ts for the same shape).
// @ts-expect-error -- node builtin, untyped here; the test runs under Node.
import { readFileSync, readdirSync } from "node:fs";
import { describe, expect, it } from "vitest";

const SRC = new URL("./", import.meta.url);
const read = (rel: string): string => readFileSync(new URL(rel, SRC), "utf8") as string;
const SHEETS = ["App.css", "float.css"] as const;

/** Same pattern as the Y5-A gate: `grep -cE '#[0-9a-fA-F]{3,8}'`. */
const HEX = /#[0-9a-fA-F]{3,8}/;

/** Every custom property declared anywhere in src/ — in a sheet, or set from TS
 *  (style.setProperty("--x") / a style object key "--x"). */
function declaredProperties(): Set<string> {
  const names = new Set<string>();
  const files = (readdirSync(SRC, { recursive: true }) as string[]).filter((f) =>
    /\.(css|ts|tsx)$/.test(f) && !/\.test\.tsx?$/.test(f),
  );
  for (const f of files) {
    const text = read(f);
    const decl = f.endsWith(".css") ? /(--[\w-]+)\s*:/g : /["'`](--[\w-]+)["'`]/g;
    for (const m of text.matchAll(decl)) names.add(m[1]);
  }
  return names;
}

describe("Y5-A token layer", () => {
  it.each(SHEETS)("%s carries zero hex colour literals (comments included, as the gate greps)", (sheet) => {
    const offenders = read(sheet)
      .split("\n")
      .map((line, i) => ({ line: i + 1, text: line.trim() }))
      .filter((l) => HEX.test(l.text));
    expect(offenders).toEqual([]);
  });

  it("the HEX pattern matches the shapes it must catch (absence proof is not vacuous)", () => {
    for (const shape of ["color: #fff;", "background: #4f7cff;", "#e5484dcc", "/* #250 */"]) {
      expect(HEX.test(shape)).toBe(true);
    }
    expect(HEX.test("var(--overlay-action)")).toBe(false);
  });

  it.each(SHEETS)("every fallback-less var() in %s resolves to a declared custom property", (sheet) => {
    const declared = declaredProperties();
    const css = read(sheet).replace(/\/\*[\s\S]*?\*\//g, "");
    const missing = new Set<string>();
    for (const m of css.matchAll(/var\(\s*(--[\w-]+)\s*\)/g)) {
      if (!declared.has(m[1])) missing.add(m[1]);
    }
    expect([...missing].sort()).toEqual([]);
  });
});
