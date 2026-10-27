/**
 * Y5-D — a filtered run (`npm test -- pill/motion`) is held to the coverage
 * floors of the modules it selects, not to 38 modules it never loaded.
 *
 * Before this, `vitest run --coverage pill/motion` passed all 13 motion tests
 * and still exited 1, because every other PURE_MODULES floor measured 0%.
 * CI's unfiltered `npm test` must keep the full table, and no filter may ever
 * switch the gate off.
 */
import { describe, expect, it } from "vitest";

interface Floor {
  lines: number;
  branches: number;
  functions: number;
  statements: number;
}
interface ConfigModule {
  PURE_MODULES: Record<string, Floor>;
  cliFilters: (argv: readonly string[]) => string[];
  scopeFloors: (filters: readonly string[], floors?: Record<string, Floor>) => Record<string, Floor>;
}

async function loadConfig(): Promise<ConfigModule> {
  // A variable specifier keeps tsc out of the config (it is not in tsconfig's
  // `include`), exactly as purity.test does.
  const spec = "../vitest.config.ts";
  return (await import(/* @vite-ignore */ spec)) as ConfigModule;
}

const BIN = ["/usr/local/bin/node", "/x/node_modules/vitest/vitest.mjs"];

describe("cliFilters", () => {
  it("reads the positional filters npm appends after the script", async () => {
    const { cliFilters } = await loadConfig();
    expect(cliFilters([...BIN, "run", "--coverage", "pill/motion"])).toEqual(["pill/motion"]);
    expect(cliFilters([...BIN, "run", "--coverage", "./src/pill/motion.test.ts"])).toEqual([
      "src/pill/motion.test.ts",
    ]);
  });

  it("sees no filter on CI's bare npm test", async () => {
    const { cliFilters } = await loadConfig();
    expect(cliFilters([...BIN, "run", "--coverage"])).toEqual([]);
    expect(cliFilters([...BIN, "run", "--coverage", "--reporter=verbose"])).toEqual([]);
  });
});

describe("scopeFloors", () => {
  it("an_unfiltered_run_keeps_every_floor", async () => {
    const { PURE_MODULES, scopeFloors } = await loadConfig();
    expect(scopeFloors([])).toEqual(PURE_MODULES);
  });

  it("a_filtered_run_is_held_only_to_the_modules_it_selects", async () => {
    const { PURE_MODULES, scopeFloors } = await loadConfig();
    expect(Object.keys(scopeFloors(["pill/motion"]))).toEqual(["pill/motion.ts"]);
    expect(Object.keys(scopeFloors(["motion"]))).toEqual(["pill/motion.ts"]);
    expect(Object.keys(scopeFloors(["src/pill/motion.test.ts"]))).toEqual(["pill/motion.ts"]);
    // The selected module keeps its OWN floor, unlowered.
    expect(scopeFloors(["pill/motion"])["pill/motion.ts"]).toEqual(PURE_MODULES["pill/motion.ts"]);
    const pill = Object.keys(scopeFloors(["pill/"]));
    expect(pill.length).toBeGreaterThan(1);
    expect(pill.every((m) => m.startsWith("pill/"))).toBe(true);
  });

  it("a_filter_that_selects_no_pure_module_never_switches_the_gate_off", async () => {
    const { PURE_MODULES, scopeFloors } = await loadConfig();
    // A stray option value (`--reporter verbose`) or a non-pure test file.
    expect(scopeFloors(["verbose"])).toEqual(PURE_MODULES);
    expect(scopeFloors(["App"])).toEqual(PURE_MODULES);
    // A stray word next to a real filter cannot widen or empty the scope.
    expect(Object.keys(scopeFloors(["verbose", "pill/motion"]))).toEqual(["pill/motion.ts"]);
  });
});
