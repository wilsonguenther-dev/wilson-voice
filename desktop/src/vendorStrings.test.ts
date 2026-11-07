/**
 * yap-1106-byom-cloud-fast-path — the vendor-string sweep (HARD rule: never
 * name an AI vendor or model on a customer-facing surface; Diagnostics only).
 *
 * Every string literal and every piece of JSX text under `desktop/src`
 * (tests, this file and the Diagnostics view excluded) must be free of
 * vendor and model names. Identifiers and comments are not user-visible and
 * are not swept; a kind id like `open_ai_compatible` is an identifier.
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here; the test runs under Node.
import { readFileSync } from "node:fs";

const SRC = new URL("./", import.meta.url);
const read = (rel: string): string => readFileSync(new URL(rel, SRC), "utf8") as string;
const VENDORS = /\b(anthropic|claude|openai|open ai|chatgpt|gpt-?\d|gpt|sonnet|opus|haiku|gemini|mistral|llama|ollama|openrouter|groq|deepseek|qwen)\b/i;

/** Files whose strings may name a vendor: Diagnostics (by design) and tests. */
const EXEMPT = [/\.test\.tsx?$/, /__tests__\//, /views\/settings\/Diagnostics\.tsx$/, /vendorStrings\.test\.ts$/];

/** Every `.ts` / `.tsx` under src/, `src/`-relative. */
function walk(): string[] {
  return Object.keys(import.meta.glob(["./**/*.{ts,tsx}", "!./**/*.d.ts"]))
    .map((k) => k.replace(/^\.\//, ""))
    .sort();
}

/** Strip comments; return string literals + JSX text as `(line, text)`. */
export function userVisibleStrings(src: string): Array<{ line: number; text: string }> {
  const out: Array<{ line: number; text: string }> = [];
  const noComments = src
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "))
    .replace(/(^|[^:\\"'`])\/\/[^\n]*/g, (m, lead) => lead + " ".repeat(m.length - lead.length));
  const lineOf = (idx: number) => noComments.slice(0, idx).split("\n").length;
  const lit = /"((?:[^"\\\n]|\\.)*)"|'((?:[^'\\\n]|\\.)*)'|`((?:[^`\\]|\\.)*)`/g;
  let m: RegExpExecArray | null;
  while ((m = lit.exec(noComments))) {
    const text = m[1] ?? m[2] ?? m[3] ?? "";
    if (text.trim()) out.push({ line: lineOf(m.index), text });
  }
  const jsx = />([^<>{}]+)</g;
  while ((m = jsx.exec(noComments))) {
    const text = m[1];
    if (text.trim()) out.push({ line: lineOf(m.index), text });
  }
  return out;
}

describe("customer-facing strings name no AI vendor or model", () => {
  const files = walk().filter((f) => !EXEMPT.some((re) => re.test(f)));

  it("sweeps a real tree", () => {
    expect(files.length).toBeGreaterThan(50);
  });

  it("finds nothing", () => {
    const hits: string[] = [];
    for (const f of files) {
      for (const { line, text } of userVisibleStrings(read(f))) {
        if (VENDORS.test(text)) hits.push(`${f}:${line}: ${JSON.stringify(text)}`);
      }
    }
    expect(hits, hits.join("\n")).toEqual([]);
  });

  it("would catch a vendor name (the sweep is not vacuous)", () => {
    const sample = userVisibleStrings('const a = "Powered by Anthropic"; const b = <p>Opus is fast</p>;');
    expect(sample.filter((s) => VENDORS.test(s.text)).length).toBe(2);
    // Identifiers and comments are not swept.
    const clean = userVisibleStrings('// claude\nconst open_ai_compatible = 1; /* gpt */');
    expect(clean.filter((s) => VENDORS.test(s.text)).length).toBe(0);
  });
});
