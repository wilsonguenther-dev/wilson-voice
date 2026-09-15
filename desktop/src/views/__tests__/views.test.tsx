import { describe, expect, it } from "vitest";

/**
 * Y5-G — the sweep that keeps the split honest.
 *
 * App.tsx used to be 4,975 lines holding all seven screens and all eight
 * settings panels. It is now the shell only. Two things have to stay true or
 * the split quietly rots back into a monolith:
 *
 *   1. every view module is a real default-exported component, so the shell can
 *      render it by name and a later item can edit one screen without touching
 *      the others;
 *   2. NO view module registers a Tauri event listener. The shell subscribes
 *      once, in `appShell.ts`. Seven views each calling `listen("take_status")`
 *      is seven subscriptions and a leak every time the user changes screens.
 *
 * Rule 2 is checked against the source text rather than at runtime, because a
 * listener registered inside a component body only runs when that component
 * mounts — a runtime assertion would pass on an unmounted view and prove
 * nothing. The sources are pulled in through `import.meta.glob` so the test
 * needs no filesystem access and therefore no `@types/node`.
 */

const VIEWS = [
  "Home",
  "Permissions",
  "Meetings",
  "Insights",
  "Dictionary",
  "Scratchpad",
  "Settings",
];
const SETTINGS_TABS = [
  "Companion",
  "Dictation",
  "Snippets",
  "Audio",
  "Shortcut",
  "Advanced",
  "Privacy",
  "License",
];

const viewMods = import.meta.glob("../*.tsx", { eager: true }) as Record<
  string,
  { default?: unknown }
>;
const settingsMods = import.meta.glob("../settings/*.tsx", { eager: true }) as Record<
  string,
  { default?: unknown }
>;
const viewSrc = import.meta.glob("../*.tsx", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;
const settingsSrc = import.meta.glob("../settings/*.tsx", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;
const shellSrc = import.meta.glob("../../appShell.ts", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;
const appSrc = import.meta.glob("../../App.tsx", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const baseName = (p: string) => p.slice(p.lastIndexOf("/") + 1).replace(/\.tsx$/, "");
const allSrc = { ...viewSrc, ...settingsSrc };
const allMods = { ...viewMods, ...settingsMods };

describe("view modules", () => {
  it("has exactly one module per Nav value and per SettingsTab", () => {
    expect(Object.keys(viewMods).map(baseName).sort()).toEqual([...VIEWS].sort());
    expect(Object.keys(settingsMods).map(baseName).sort()).toEqual([...SETTINGS_TABS].sort());
  });

  it.each(Object.keys(allMods).map((p) => ({ name: baseName(p), path: p })))(
    "$name exports a default component",
    ({ path }) => {
      expect(typeof allMods[path].default).toBe("function");
    },
  );

  /**
   * no_view_module_registers_a_listener — the listener discipline. The shell in
   * appShell.ts owns every `listen()`; a view that grows one leaks a
   * subscription per navigation, so this fails the moment one appears.
   */
  it("no_view_module_registers_a_listener", () => {
    const offenders = Object.entries(allSrc)
      // Catch the call and the import that enables it, so aliasing the binding
      // (`listen as subscribe`) cannot slip past the check.
      .filter(([, src]) => /\blisten\s*\(/.test(src) || /@tauri-apps\/api\/event/.test(src))
      .map(([p]) => baseName(p));
    expect(offenders).toEqual([]);
  });

  it("the shell is the one place that registers listeners", () => {
    const shell = Object.values(shellSrc)[0];
    expect(shell).toMatch(/@tauri-apps\/api\/event/);
    expect(/\blisten\s*\(/.test(shell)).toBe(true);
  });

  it("App.tsx is the shell only, not a screen", () => {
    const app = Object.values(appSrc)[0];
    // The settings sub-tabs moved out wholesale; if this reappears in App.tsx
    // the panels have started leaking back into the shell.
    expect(app.includes("settingsTab === ")).toBe(false);
    expect(app.split("\n").length).toBeLessThanOrEqual(900);
  });
});
