/**
 * Y7-D — the discipline that keeps the pure-module coverage number honest.
 *
 * `vitest.config.ts` holds PURE_MODULES: the `.ts` files whose coverage is
 * gated, each with its own floor. A list like that rots in two directions — a
 * new module that is never added to it, and a listed module that quietly grows
 * an `invoke()` and stops being drivable without a window (`live.ts` is pure,
 * which is the only reason its 300-second takes can be unit-tested). This file
 * fails on both, and on a list the coverage config does not actually use.
 *
 * Sources are read as TEXT with node:fs, never through a `?raw` import:
 * see `rawSourceIsolation` in vitest.config.ts for what a raw copy does to
 * the coverage of the module it copies. The glob below is LAZY, so Vite
 * enumerates the tree without loading a single module.
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here; the test runs under Node.
import { readFileSync } from "node:fs";

const SRC = new URL("./", import.meta.url);
const read = (rel: string): string => readFileSync(new URL(rel, SRC), "utf8") as string;

/** Every file under src/ that a module or test could be, `src/`-relative ("pill/live.ts"). */
const FILES: string[] = Object.keys(
  import.meta.glob(["./**/*.{ts,tsx}", "./pill/**/*", "!./**/*.d.ts"]),
)
  .map((k) => k.replace(/^\.\//, ""))
  .sort();

const isTest = (f: string) => /\.test\.tsx?$/.test(f);
const isModule = (f: string) => /\.ts$/.test(f) && !isTest(f) && !f.endsWith(".d.ts");

/**
 * `.ts` modules that are NOT pure, and why. Each one reaches `@tauri-apps/*`,
 * which the purity check below verifies: an adapter that stops invoking is
 * pure and has to move onto PURE_MODULES with a floor.
 */
const ADAPTERS: Record<string, string> = {
  "appShell.ts": "the main window's bridge: every invoke() and listen() the views use",
  "formatting.ts": "useFormattingOptions loads and saves the formatting settings over invoke()",
  "meetings/consentWatch.ts": "subscribes to the backend's meeting-consent event",
  "meetings/notetakerSetup.ts": "drives the notetaker setup commands",
  "permissionRepair.ts": "runs the per-service permission probe and repair commands",
  "pill/drag.ts": "moves the float window through invoke(); its hit-box math is tested on its own",
  "updater.ts": "the check_for_update / install_update commands",
};

/**
 * React components under src/pill/. They render, so they are not on the
 * coverage list (no DOM library, by decision), but a new FILE under src/pill/
 * must be registered here or carry a test — a pill piece nobody looked at is
 * how the pill stops being drivable.
 */
const PILL_COMPONENTS = ["ClassicPill.tsx", "CopyCard.tsx", "MeetingBadge.tsx", "MeetingOfferCard.tsx", "NudgeRow.tsx", "PhaseAnnouncer.tsx", "YappyPill.tsx"];
/** Docs and data under src/pill/ (dock-geometry.json is the Y5-I table dock.ts reads and dock.test.ts pins). */
const PILL_DOCS = ["README.md", "dock-geometry.json"];

async function loadConfig(): Promise<{
  PURE_MODULES: Record<string, Record<string, number>>;
  coveragePath: (m: string) => string;
  resolved: any;
}> {
  // A variable specifier keeps tsc out of the config (it is not in tsconfig's
  // `include`); Vite resolves it at run time like any other module.
  const spec = "../vitest.config.ts";
  const mod = await import(/* @vite-ignore */ spec);
  const resolved = await mod.default({ command: "serve", mode: "test" });
  return { PURE_MODULES: mod.PURE_MODULES, coveragePath: mod.coveragePath, resolved };
}

/** The tests that own a module: `x.test.ts(x)` or an aspect file `x.<aspect>.test.ts(x)`. */
function siblingTests(mod: string): string[] {
  const base = mod.replace(/\.tsx?$/, "");
  return FILES.filter((f) => f.startsWith(`${base}.`) && /^(?:[\w-]+\.)?test\.tsx?$/.test(f.slice(base.length + 1)));
}

/* ── the import scanner ──────────────────────────────────────────────────── */

export interface ImportRef {
  spec: string;
  typeOnly: boolean;
}

/**
 * Runtime and type-only imports of a TS source. Anchored at line start for
 * static forms (so prose in a doc comment does not count) and unanchored for
 * `import("...")`, which errs toward flagging. The clause allows only
 * identifier / brace / comma / `*` characters, so a later `from "..."` can
 * never be glued onto an unrelated `export const`.
 */
export function scanImports(source: string): ImportRef[] {
  const out: ImportRef[] = [];
  const stmt = /^[ \t]*(import|export)\s+(type\s+)?([\w$*{},\s]*?)\s*from\s*["']([^"']+)["']/gm;
  for (const m of source.matchAll(stmt)) {
    const clause = m[3].trim();
    const named = /^\{([\s\S]*)\}$/.exec(clause);
    const allTypeNamed =
      !!named &&
      named[1].split(",").map((s) => s.trim()).filter(Boolean).every((s) => s.startsWith("type "));
    out.push({ spec: m[4], typeOnly: !!m[2] || allTypeNamed });
  }
  for (const m of source.matchAll(/^[ \t]*import\s*["']([^"']+)["']/gm)) out.push({ spec: m[1], typeOnly: false });
  for (const m of source.matchAll(/\bimport\s*\(\s*["']([^"']+)["']\s*\)/g)) out.push({ spec: m[1], typeOnly: false });
  return out;
}

const TAURI = /^@tauri-apps\//;

/** `from` ("pill/meeting.ts") + "../meetings/consent" -> "meetings/consent.ts", or null for a non-source. */
function resolveRel(from: string, spec: string): string | null {
  const parts = from.split("/").slice(0, -1);
  for (const seg of spec.split("/")) {
    if (seg === "." || seg === "") continue;
    if (seg === "..") parts.pop();
    else parts.push(seg);
  }
  const base = parts.join("/");
  for (const cand of [base, `${base}.ts`, `${base}.tsx`, `${base}/index.ts`, `${base}/index.tsx`]) {
    if (/\.tsx?$/.test(cand) && FILES.includes(cand)) return cand;
  }
  return null; // JSON, CSS, a Rust file read as text: data, not code
}

/** The runtime import chain from `mod` to a `@tauri-apps/*` package, or null. */
function tauriChain(mod: string, seen = new Set<string>()): string[] | null {
  if (seen.has(mod)) return null;
  seen.add(mod);
  for (const ref of scanImports(read(mod))) {
    if (ref.typeOnly) continue; // erased at compile time: costs a test nothing
    if (TAURI.test(ref.spec)) return [mod, ref.spec];
    if (!ref.spec.startsWith(".")) continue;
    const next = resolveRel(mod, ref.spec);
    if (!next) continue;
    const chain = tauriChain(next, seen);
    if (chain) return [mod, ...chain];
  }
  return null;
}

/* ── the checks ──────────────────────────────────────────────────────────── */

describe("the import scanner can see every shape a Tauri import takes", () => {
  it("flags static, default, namespace, side-effect, re-export, multi-line and dynamic forms", () => {
    const shapes = [
      `import { invoke } from "@tauri-apps/api/core";`,
      `import core from '@tauri-apps/api/core';`,
      `import * as ev from "@tauri-apps/api/event";`,
      `import "@tauri-apps/api/window";`,
      `export { listen } from "@tauri-apps/api/event";`,
      `export * from "@tauri-apps/plugin-updater";`,
      `import {\n  invoke,\n  type InvokeArgs,\n} from "@tauri-apps/api/core";`,
      `const m = await import("@tauri-apps/api/core");`,
      `import x, { y } from "@tauri-apps/api/core";`,
    ];
    for (const s of shapes) {
      expect(scanImports(s).filter((r) => !r.typeOnly && TAURI.test(r.spec)), s).toHaveLength(1);
    }
  });

  it("treats type-only imports as erased, and ignores prose and unrelated exports", () => {
    expect(scanImports(`import type { Event } from "@tauri-apps/api/event";`)[0].typeOnly).toBe(true);
    expect(scanImports(`import { type A, type B } from "./x";`)[0].typeOnly).toBe(true);
    expect(scanImports(`import { type A, b } from "./x";`)[0].typeOnly).toBe(false);
    expect(scanImports(` * Call it like: import { invoke } from "@tauri-apps/api/core"`.replace(/^/, "/**\n"))).toEqual([]);
    expect(scanImports(`export const A = 1;\nconst s = "from 'x'";`)).toEqual([]);
  });
});

describe("no_pure_module_imports_the_tauri_api", () => {
  it("no module on PURE_MODULES reaches @tauri-apps/*, directly or through another module", async () => {
    const { PURE_MODULES } = await loadConfig();
    const offenders = Object.keys(PURE_MODULES)
      .map((m) => tauriChain(m))
      .filter((c): c is string[] => c !== null)
      .map((c) => c.join(" -> "));
    expect(offenders, "a pure module that invokes cannot be driven without a window").toEqual([]);
  });

  it("every adapter really does reach @tauri-apps/* (else it is pure: give it a floor)", () => {
    const pureAfterAll = Object.keys(ADAPTERS).filter((m) => tauriChain(m) === null);
    expect(pureAfterAll).toEqual([]);
  });
});

describe("every_pure_module_has_a_sibling_test", () => {
  it("every PURE_MODULES entry exists and has a sibling *.test.ts", async () => {
    const { PURE_MODULES } = await loadConfig();
    const missing = Object.keys(PURE_MODULES).filter((m) => !FILES.includes(m));
    const untested = Object.keys(PURE_MODULES).filter((m) => siblingTests(m).length === 0);
    expect(missing, "listed in vitest.config.ts but not on disk").toEqual([]);
    expect(untested, "a pure module with no sibling *.test.ts").toEqual([]);
  });

  it("every .ts module under src/ is on PURE_MODULES or a named adapter — never neither, never both", async () => {
    const { PURE_MODULES } = await loadConfig();
    const modules = FILES.filter(isModule);
    expect(modules.length, "the glob found the tree").toBeGreaterThan(20);
    const unclassified = modules.filter((m) => !(m in PURE_MODULES) && !(m in ADAPTERS));
    const both = modules.filter((m) => m in PURE_MODULES && m in ADAPTERS);
    const staleAdapters = Object.keys(ADAPTERS).filter((m) => !FILES.includes(m));
    expect(unclassified, "new module: add it to PURE_MODULES (with a test and a floor) or ADAPTERS (with a reason)").toEqual([]);
    expect(both).toEqual([]);
    expect(staleAdapters).toEqual([]);
  });

  it("every file under src/pill/ is tested, or a registered component", () => {
    const pill = FILES.filter((f) => f.startsWith("pill/")).map((f) => f.slice("pill/".length));
    expect(pill).toContain("live.ts");
    const unaccounted = pill.filter((f) => {
      if (isTest(`pill/${f}`)) return false;
      if (PILL_DOCS.includes(f)) return false;
      if (siblingTests(`pill/${f}`).length > 0) return false;
      return !PILL_COMPONENTS.includes(f);
    });
    expect(unaccounted, "a new pill file needs a sibling *.test.ts (or, for a component, a line in PILL_COMPONENTS)").toEqual([]);
    expect(PILL_COMPONENTS.filter((c) => !pill.includes(c)), "stale PILL_COMPONENTS entry").toEqual([]);
  });
});

describe("coverage is scoped to the pure modules", () => {
  it("the include list IS PURE_MODULES, every module has its own floor, and there is no global threshold", async () => {
    const { PURE_MODULES, coveragePath, resolved } = await loadConfig();
    const cov = resolved.test.coverage;
    const want = Object.keys(PURE_MODULES).map(coveragePath);
    expect(cov.include).toEqual(want);
    expect(Object.keys(cov.thresholds).sort()).toEqual([...want].sort());
    for (const key of ["lines", "branches", "functions", "statements", "perFile", "100"]) {
      expect(cov.thresholds, `global threshold "${key}"`).not.toHaveProperty(key);
    }
    for (const [m, floor] of Object.entries(PURE_MODULES)) {
      expect(Object.keys(floor).sort(), m).toEqual(["branches", "functions", "lines", "statements"]);
    }
  });
});
