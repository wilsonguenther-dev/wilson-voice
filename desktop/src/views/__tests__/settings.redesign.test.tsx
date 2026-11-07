/**
 * yap-1106-settings-redesign (§14) — the Settings screen: a slim left list of
 * five sections with search, one main column, and a pinned footer holding
 * Account & help and Done. Panes are stubbed: this pins the SHELL of the
 * screen (which blocks render where, once each), not the panes themselves.
 */
import { describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined), emit: vi.fn(async () => undefined) }));

const stub = vi.hoisted(
  () => (name: string) => async () => {
    const { createElement } = await import("react");
    return {
      default: (p: { part?: string } = {}) =>
        createElement("div", { "data-stub": name, "data-part": p.part ?? "all" }),
    };
  },
);
vi.mock("../settings/Companion", stub("companion"));
vi.mock("../settings/Notifications", stub("notifications"));
vi.mock("../settings/News", stub("news"));
vi.mock("../settings/Dictation", stub("dictation"));
vi.mock("../settings/Snippets", stub("snippets"));
vi.mock("../settings/Audio", stub("audio"));
vi.mock("../settings/Shortcut", stub("shortcut"));
vi.mock("../settings/Advanced", stub("advanced"));
vi.mock("../settings/Privacy", stub("privacy"));
vi.mock("../settings/Meetings", stub("meetings"));
vi.mock("../settings/MeetingPrefs", stub("meetingprefs"));
vi.mock("../settings/License", stub("license"));
vi.mock("../settings/Diagnostics", stub("diagnostics"));
vi.mock("../../settings/ModelProviders", stub("providers"));
vi.mock("../../settings/CloudRoute", stub("cloudroute"));
vi.mock("../../LanguageSettings", stub("language"));
vi.mock("../../BootNote", stub("bootnote"));

import { AppContext, type AppCtx } from "../../appShell";
import type { SettingsTab } from "../../appTypes";
import Settings, { HIT_HIGHLIGHT_MS, SearchResults } from "../Settings";
import { searchSettings } from "../../settingsIndex";
import settingsSrc from "../Settings.tsx?raw";
import appShellSrc from "../../appShell.ts?raw";
import appCss from "../../App.css?raw";

type State = Record<string, unknown>;
const makeCtx = (state: State): AppCtx =>
  new Proxy(state, {
    get: (t, k: string) => (k in t ? t[k] : () => undefined),
  }) as unknown as AppCtx;
const render = (node: ReactElement, state: State) =>
  renderToStaticMarkup(<AppContext.Provider value={makeCtx(state)}>{node}</AppContext.Provider>);
const at = (tab: SettingsTab) =>
  render(<Settings />, { settings: {}, booting: false, bootError: null, settingsTab: tab });
const stubs = (html: string) =>
  [...html.matchAll(/data-stub="([a-z]+)" data-part="([a-z]+)"/g)].map((m) => `${m[1]}:${m[2]}`);

describe("Settings layout", () => {
  it("shows the five sections in the left list, search above it, Account & help and Done in the footer", () => {
    const html = at("dictation");
    const list = html.slice(html.indexOf('class="settings-list"'), html.indexOf("</nav>"));
    const labels = [...list.matchAll(/data-section="[a-z]+"[^>]*>([^<]+)</g)].map((m) => m[1]);
    expect(labels).toEqual(["General", "Dictation", "Meetings", "Yappy &amp; pill", "Models &amp; privacy"]);
    expect(html.indexOf('aria-label="Search settings"')).toBeLessThan(html.indexOf('class="settings-list"'));
    const foot = html.slice(html.indexOf('class="settings-done-bar"'));
    expect(foot).toContain("Account &amp; help");
    expect(foot).toMatch(/>Done<\/button>/);
  });

  it("General: shortcut, microphone, language, start at login", () => {
    expect(stubs(at("shortcut"))).toEqual(["shortcut:all", "audio:input", "language:all", "advanced:startup"]);
  });

  it("Dictation: formatting, audio cleanup, snippets, and a Dictionary link", () => {
    const html = at("dictation");
    expect(stubs(html)).toEqual(["dictation:all", "audio:cleanup", "bootnote:all", "snippets:all"]);
    expect(html).toContain("Open Dictionary");
    expect(html).toMatch(/<h2>Dictation<\/h2>/);
  });

  it("Meetings, Yappy & pill, Models & privacy, Account & help hold their panes", () => {
    expect(stubs(at("meetings"))).toEqual([
      "meetings:call",
      "meetingprefs:kinds",
      "privacy:retention",
      "meetingprefs:speakers",
      "meetings:exports",
    ]);
    expect(stubs(at("companion"))).toEqual(["companion:all", "notifications:all", "news:all"]);
    expect(stubs(at("notifications"))).toEqual(["companion:all", "notifications:all", "news:all"]);
    expect(stubs(at("news"))).toEqual(["companion:all", "notifications:all", "news:all"]);
    expect(stubs(at("advanced"))).toEqual(["advanced:models", "providers:all", "cloudroute:all", "privacy:main"]);
    expect(stubs(at("privacy"))).toEqual(["advanced:models", "providers:all", "cloudroute:all", "privacy:main"]);
    expect(stubs(at("license"))).toEqual(["license:all", "diagnostics:all"]);
  });

  it("each control appears once across all sections", () => {
    const seen = new Map<string, number>();
    for (const tab of ["shortcut", "dictation", "meetings", "companion", "privacy", "license"] as const)
      for (const s of stubs(at(tab))) seen.set(s, (seen.get(s) ?? 0) + 1);
    for (const [s, n] of seen) expect([s, n]).toEqual([s, 1]);
  });

  it("the active section carries aria-current; Account & help lights up in the footer", () => {
    expect(at("shortcut")).toMatch(/data-section="general"[^>]*aria-current="page"/);
    const html = at("license");
    expect(html).toMatch(/settings-account active" aria-current="page"/);
    expect(html).not.toMatch(/data-section="[a-z]+"[^>]*aria-current="page"/);
  });

  it("loading and error states are unchanged", () => {
    expect(render(<Settings />, { settings: null, booting: true, bootError: null, settingsTab: "dictation" })).toContain(
      'data-loading-state="settings"',
    );
  });
});

describe("Settings search", () => {
  it("lists hits with the section each lives in", () => {
    const html = renderToStaticMarkup(
      <SearchResults query="denoise" hits={searchSettings("denoise")} onJump={() => undefined} onClear={() => undefined} />,
    );
    expect(html).toContain('data-settings-hit="audio-cleanup"');
    expect(html).toContain("<strong>Denoise</strong>");
    expect(html).toContain("Dictation");
  });

  it("the empty state names the query and offers Clear search", () => {
    const html = renderToStaticMarkup(
      <SearchResults query="  zzz " hits={[]} onJump={() => undefined} onClear={() => undefined} />,
    );
    expect(html).toContain("No setting matches &#x27;zzz&#x27;.");
    expect(html).toContain("Clear search");
  });

  it("a jump highlights the control for 2 s", () => {
    expect(HIT_HIGHLIGHT_MS).toBe(2000);
    expect(settingsSrc).toContain('classList.add("settings-hit")');
    expect(appCss).toMatch(/\.settings-hit \{/);
  });
});

describe("Settings keyboard, Done and telemetry", () => {
  it("⌘F focuses search, Escape clears then leaves, ↑/↓ walk the list", () => {
    expect(settingsSrc).toMatch(/e\.key\.toLowerCase\(\) === "f"/);
    expect(settingsSrc).toContain('e.key !== "Escape"');
    expect(settingsSrc).toContain("leaveSettings()");
    expect(settingsSrc).toMatch(/ArrowDown[\s\S]*stepSection/);
  });

  it("Done returns to the screen you came from (the shell remembers it)", () => {
    expect(appShellSrc).toMatch(/if \(nav !== "settings"\) settingsReturnRef\.current = nav;/);
    expect(appShellSrc).toMatch(/const leaveSettings = useCallback/);
  });

  it("logs one structured line per action, never the query text", () => {
    expect(settingsSrc).toContain("ui settings_search hits=${hits.length}");
    expect(settingsSrc).toContain("ui settings_jump section=${hit.section} block=${hit.block}");
    expect(settingsSrc).toContain("ui settings_section to=${id}");
    expect(settingsSrc).not.toMatch(/console\.info\([^)]*query/);
  });
});

describe("app rail", () => {
  it("the sidebar is a slim icon + label rail that collapses to icons under 900px", () => {
    expect(appCss).toMatch(/\.shell \{[\s\S]*?grid-template-columns: 184px minmax\(0, 1fr\);/);
    const rail = appCss.slice(appCss.indexOf("@media (max-width: 900px) {\n  .shell {"));
    expect(rail).toMatch(/grid-template-columns: 64px minmax\(0, 1fr\);/);
    expect(rail.slice(0, rail.indexOf("\n}\n"))).toMatch(/\.nav-label,\n  \.dictate-label \{/);
  });
});
