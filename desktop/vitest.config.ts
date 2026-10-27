/// <reference types="vitest/config" />
import { readdirSync, readFileSync } from "node:fs";
import { dirname, isAbsolute, relative } from "node:path";
import { fileURLToPath } from "node:url";
import type { Plugin } from "vite";
import { defineConfig, mergeConfig } from "vitest/config";
import viteConfig from "./vite.config";

/**
 * Y7-D — coverage for the PURE frontend modules, and only for them.
 *
 * A pure module is a `.ts` file under `src/` that holds a state machine, a
 * parser or a formatter, imports nothing from `@tauri-apps/*` (directly or
 * through another module) and is tested by a sibling `*.test.ts` without a
 * window. `src/purity.test.ts` enforces all three, and it reads THIS object —
 * so the list below is the single source of truth for both the coverage
 * include and the purity check.
 *
 * Every entry carries its own floor, set from the coverage it had when it was
 * added (rounded down). There is deliberately NO global threshold: a
 * repo-wide number is either trivially met or permanently red, because
 * App.tsx and the views cannot be unit-tested without a DOM. A per-module
 * floor cannot be bought by a well-covered neighbour.
 *
 * Adding a pure module: add it here with its measured floor and a sibling
 * test. Lowering a floor is a reviewed diff, never a side effect.
 */
export interface CoverageFloor {
  lines: number;
  branches: number;
  functions: number;
  statements: number;
}

export const PURE_MODULES: Record<string, CoverageFloor> = {
  "appTypes.ts": { lines: 90, branches: 100, functions: 100, statements: 92 },
  "buildChannel.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "diff.ts": { lines: 96, branches: 100, functions: 100, statements: 96 },
  "errors.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "hotkeyRecorder.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "hotkeyStatus.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "languagePicker.ts": { lines: 96, branches: 80, functions: 93, statements: 97 },
  "license/status.ts": { lines: 100, branches: 95, functions: 100, statements: 100 },
  "meetings/consent.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "meetings/notes.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "meetings/progress.ts": { lines: 89, branches: 88, functions: 100, statements: 90 },
  "meetings/sampleMeeting.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "meetings/systemAudio.ts": { lines: 100, branches: 86, functions: 100, statements: 94 },
  "meetings/transcript.ts": { lines: 100, branches: 95, functions: 100, statements: 100 },
  "micStatus.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "onboarding.ts": { lines: 100, branches: 97, functions: 100, statements: 100 },
  "permission.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "pill/copyCardModel.ts": { lines: 100, branches: 97, functions: 100, statements: 100 },
  "pill/dock.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "pill/forced.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "pill/hitbox.ts": { lines: 92, branches: 85, functions: 87, statements: 90 },
  "pill/license.ts": { lines: 96, branches: 95, functions: 100, statements: 97 },
  "pill/live.ts": { lines: 98, branches: 89, functions: 90, statements: 95 },
  "pill/meeting.ts": { lines: 100, branches: 92, functions: 100, statements: 100 },
  "pill/meetingOffer.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "pill/motion.ts": { lines: 88, branches: 84, functions: 84, statements: 89 },
  "pill/mouth.ts": { lines: 100, branches: 90, functions: 100, statements: 100 },
  "pill/nudge.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "pill/pixelGrid.ts": { lines: 100, branches: 73, functions: 100, statements: 91 },
  "pill/tone.ts": { lines: 100, branches: 94, functions: 100, statements: 96 },
  "polishOffer.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "recoveryToast.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "scratchpad/notesModel.ts": { lines: 100, branches: 96, functions: 100, statements: 96 },
  "structureSettings.ts": { lines: 100, branches: 71, functions: 100, statements: 100 },
  "support/bundle.ts": { lines: 94, branches: 92, functions: 100, statements: 92 },
  "toast.ts": { lines: 84, branches: 82, functions: 69, statements: 81 },
  "viewState.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
  "voicePreferences.ts": { lines: 92, branches: 90, functions: 100, statements: 94 },
  "writingSamples.ts": { lines: 100, branches: 100, functions: 100, statements: 100 },
};

/**
 * Keeps a `?raw` copy of a source file from erasing that file's coverage.
 *
 * `pill/motion.test` and `permission.test` sweep the sources as TEXT through
 * `import.meta.glob(..., { query: "?raw" })`. Vite caches that string module
 * under `src/pill/forced.ts?raw`, and @vitest/coverage-v8 3.2 strips the query
 * when it pairs V8 counters with transform results (`normalizeTransformResults`
 * -> `cleanUrl`): whichever of `forced.ts` / `forced.ts?raw` is cached first is
 * the source the REAL module's counters get mapped onto. Measured on this tree:
 * forced.ts reported 0/0/0/0 in the full suite and 100/100/100/100 alone.
 *
 * Serving `.ts`/`.tsx` raw reads from a virtual id gives the text copy its own
 * cache key, so it can no longer stand in for the module. The glob is still
 * expanded by Vite, so the sweeps keep their "same resolution universe"
 * property; only the loader changes. Test-only: `vite build` never loads this.
 */
function rawSourceIsolation(): Plugin {
  const PREFIX = "\0yap-raw-source:";
  return {
    name: "yap:raw-source-isolation",
    enforce: "pre",
    async resolveId(source, importer) {
      const m = /^(.+\.tsx?)\?raw$/.exec(source);
      if (!m) return null;
      const resolved = await this.resolve(m[1], importer, { skipSelf: true });
      return resolved ? PREFIX + resolved.id : null;
    },
    load(id) {
      if (!id.startsWith(PREFIX)) return null;
      return `export default ${JSON.stringify(readFileSync(id.slice(PREFIX.length), "utf8"))};`;
    },
  };
}

/** `src/`-relative module path -> the root-relative path coverage reports. */
export const coveragePath = (mod: string): string => `src/${mod}`;

const ROOT = dirname(fileURLToPath(import.meta.url));

/** Every test file under `src/`, root-relative with forward slashes (`src/x.test.ts`). */
export function listTestFiles(root: string = ROOT): string[] {
  return readdirSync(`${root}/src`, { recursive: true, encoding: "utf8" })
    .map((p) => `src/${p.split("\\").join("/")}`)
    .filter((p) => /\.test\.tsx?$/.test(p) && !p.includes("/node_modules/"))
    .sort();
}

/**
 * The test files a filtered run (`npm test -- viewState`) selects, mirroring
 * vitest 3's `filterFiles`: a filter selects a file when it is a
 * case-insensitive substring of the file's root-relative path. `./x` and an
 * absolute path are made root-relative first, as vitest does.
 */
export function selectTests(testFiles: readonly string[], filters: readonly string[], root: string = ROOT): string[] {
  if (filters.length === 0) return [...testFiles];
  const needles = filters.map((f) => (isAbsolute(f) ? relative(root, f) : f.replace(/^\.\//, "")).toLowerCase());
  return testFiles.filter((t) => needles.some((n) => t.toLowerCase().includes(n)));
}

/** purity.test.ts's sibling rule: `src/x.test.ts(x)` or an aspect file `src/x.<aspect>.test.ts(x)` owns `x.ts`. */
export function isSiblingTest(mod: string, testFile: string): boolean {
  const base = `src/${mod.replace(/\.tsx?$/, "")}.`;
  return testFile.startsWith(base) && /^(?:[\w-]+\.)?test\.tsx?$/.test(testFile.slice(base.length));
}

/**
 * The PURE_MODULES whose floors a run enforces.
 *
 * The full suite (no filters — what CI and the gate run) enforces every
 * floor, exactly as before. A FILTERED run enforces the floor of each module
 * whose own sibling test it selected, and no other: a module whose test did
 * not run reports 0% by construction, so enforcing its floor made every
 * filtered run exit 1 whatever the code did (`npm test -- viewState` failed on
 * `pill/nudge.ts`). The floor of a module that DID get its test is still a
 * hard floor.
 */
export function enforcedModules(filters: readonly string[], testFiles: readonly string[], root: string = ROOT): string[] {
  if (filters.length === 0) return Object.keys(PURE_MODULES);
  const selected = selectTests(testFiles, filters, root);
  return Object.keys(PURE_MODULES).filter((m) => selected.some((t) => isSiblingTest(m, t)));
}

/**
 * The file filters on THIS process's vitest command line, parsed by vitest's
 * own CLI parser so value-taking flags (`--reporter verbose`) are never read
 * as filters. Any process that is not the vitest CLI (a test worker importing
 * this config, `vite build`) has none. `vitest related <files>` reports none
 * either, so it keeps every floor.
 */
export async function cliFilters(argv: readonly string[] = process.argv): Promise<string[]> {
  if (!/(?:^|[\\/])vitest(?:\.mjs)?$/.test(argv[1] ?? "")) return [];
  const { parseCLI } = await import("vitest/node");
  return parseCLI(["vitest", ...argv.slice(2)]).filter;
}

export default defineConfig(async (env) => {
  const filters = await cliFilters();
  const enforced = enforcedModules(filters, filters.length ? listTestFiles() : []);
  if (filters.length) {
    process.stderr.write(
      `vitest.config coverage_scope filtered=1 filters=${filters.join(",")} floors_enforced=${enforced.length} floors_total=${Object.keys(PURE_MODULES).length}\n`,
    );
  }
  return mergeConfig(await viteConfig(env), {
    plugins: [rawSourceIsolation()],
    test: {
      // Y5-D — vitest stubs every CSS module to the empty string by default,
      // which silently turns a stylesheet assertion into a test of "".
      // `pill/motion.test` sweeps float.css for reduced-motion rules that would
      // hide information, and that sweep has to see the real file.
      css: true,
      coverage: {
        provider: "v8",
        // AST-aware remapping (vitest 3.2): the default v8-to-istanbul remap
        // reported forced.ts at 0% statements while every function in it ran.
        experimentalAstAwareRemapping: true,
        // An explicit include list: a file that no test loads still reports
        // (at 0%) instead of quietly vanishing from the denominator.
        include: Object.keys(PURE_MODULES).map(coveragePath),
        reporter: ["text"],
        thresholds: Object.fromEntries(enforced.map((mod) => [coveragePath(mod), PURE_MODULES[mod]])),
      },
    },
  });
});
