/**
 * yap-uiux-2026-10-16-SETTINGS — the three "| Settings |" rows of
 * docs/loop/UIUX-WALK-2026-10-16.md, pinned so they cannot drift back.
 *
 * T1  Settings opens on Dictation, and no pane repeats its tab name as a header.
 * T2  Every settings card carries `data-row`, so the structural smoke can tell
 *     a populated form from an empty list (it read rows=0 on a full form).
 * T3  Permissions leads with one row per grant; "Choose a speech model" is gone
 *     from it; the bundle-id prose is a footnote after the rows.
 * Plus the item's own shape: control first, its one-line hint after, and the
 * pane's save/reset actions in the pinned `.settings-footer`.
 */
import { describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined), emit: vi.fn(async () => undefined) }));

import { AppContext, type AppCtx } from "../../appShell";
import { SETTINGS_TABS } from "../../appTypes";
import Permissions from "../Permissions";
import Companion from "../settings/Companion";
import Audio from "../settings/Audio";
import appShellSrc from "../../appShell.ts?raw";

type State = Record<string, unknown>;
const makeCtx = (state: State): AppCtx =>
  new Proxy(state, {
    get: (t, k: string) => (k in t ? t[k] : () => undefined),
  }) as unknown as AppCtx;
const render = (node: ReactElement, state: State) =>
  renderToStaticMarkup(<AppContext.Provider value={makeCtx(state)}>{node}</AppContext.Provider>);

const paneSrc = import.meta.glob("../settings/*.tsx", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;
// License renders license/LicensePanel, which is outside this item's files.
const panes = Object.entries(paneSrc).filter(([f]) => !f.endsWith("/License.tsx"));

describe("walk row T1 — Settings opens on Dictation, no header repeats the tab", () => {
  it("Dictation is the first tab and the shell's initial tab", () => {
    expect(SETTINGS_TABS[0].id).toBe("dictation");
    expect(appShellSrc).toContain('useState<SettingsTab>("dictation")');
  });
  it.each(panes)("%s has no settings-section header", (_f, src) => {
    expect(src).not.toContain('className="settings-section"');
    expect(src).toMatch(/<section className="settings-panel" data-settings-pane="[a-z]+">/);
  });
});

describe("walk row T2 — every settings card is a data-row", () => {
  it.each(panes)("%s marks every .panel with data-row", (_f, src) => {
    expect(src).toContain('className="panel" data-row');
    expect(src).not.toMatch(/className="panel"(?! data-row)/);
  });
  it("a rendered pane exposes rows to the smoke detector", () => {
    const html = render(<Audio />, { settings: {} });
    expect(html).toContain('data-row="true"');
    expect(html).not.toContain("settings-section");
  });
});

describe("control first, one-line hint after", () => {
  it("Companion: each picker precedes its hint", () => {
    const html = render(<Companion />, { settings: {}, saveSettings: () => undefined });
    const firstPicker = html.indexOf('class="profile-row"');
    const firstHint = html.indexOf('class="settings-hint"');
    expect(firstPicker).toBeGreaterThan(-1);
    expect(firstHint).toBeGreaterThan(firstPicker);
    expect((html.match(/class="settings-hint"/g) ?? []).length).toBe(4);
  });
  it("save / reset actions live in the pinned footer", () => {
    const privacy = paneSrc["../settings/Privacy.tsx"];
    const shortcut = paneSrc["../settings/Shortcut.tsx"];
    expect(privacy.indexOf('className="settings-footer"')).toBeGreaterThan(-1);
    expect(shortcut.indexOf('className="settings-footer"')).toBeGreaterThan(-1);
    expect(privacy.indexOf("Save settings")).toBeGreaterThan(privacy.indexOf('className="settings-footer"'));
    expect(shortcut.indexOf("Reset to ⌃⌥⌘Space")).toBeGreaterThan(shortcut.indexOf('className="settings-footer"'));
  });
});

describe("walk row T3 — Permissions leads with the grants", () => {
  const perms = {
    accessibility: false, microphone: false, microphoneStatus: "not_determined", inputMonitoring: "unknown",
    audioCapture: "unknown", grants: [], asrOk: false, asrDetail: "No speech model yet", allCriticalOk: false,
    summary: "Microphone not granted",
  };
  const state = {
    booting: false, permsError: null, perms, modelSetup: { ready: false, ribbon: null }, status: { modelReady: false },
    sysAudio: null, sysAudioBusy: false, sysAudioGate: { available: true, message: null },
  };
  it("no model picker, rows before the bundle-id footnote, one button per grant", () => {
    const html = render(<Permissions />, state);
    expect(html).not.toContain("Choose a speech model");
    expect(html).not.toContain("Manage speech model");
    expect(html.indexOf('class="perm-list"')).toBeLessThan(html.indexOf("Bundle id"));
    const rows = html.split("<li ").slice(1);
    expect(rows.length).toBeGreaterThanOrEqual(4);
    for (const row of rows) {
      expect(row).toContain("data-row");
      expect((row.match(/<button/g) ?? []).length).toBeLessThanOrEqual(1);
    }
    expect(html).toContain("Request Microphone");
    expect(html).toContain("Prompt Accessibility");
  });
  it("a denied microphone gets the deep link instead of a dead request", () => {
    const html = render(<Permissions />, { ...state, perms: { ...perms, microphoneStatus: "denied" } });
    expect(html).toContain("Open Microphone settings");
    expect(html).not.toContain("Request Microphone");
  });
});
