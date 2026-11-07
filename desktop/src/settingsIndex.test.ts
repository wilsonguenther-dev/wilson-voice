/**
 * yap-1106-settings-redesign (§14) — the Settings IA table: five list sections
 * plus Account & help, every block in exactly one section, every old tab id
 * landing somewhere, and a search that covers labels and help text.
 */
import { describe, expect, it } from "vitest";
import { SETTINGS_TABS } from "./appTypes";
import {
  LIST_SECTIONS,
  SECTION_HOME_TAB,
  SETTINGS_INDEX,
  SETTINGS_SECTIONS,
  TAB_SECTION,
  type SettingsBlock,
  searchSettings,
  sectionForTab,
  sectionOfBlock,
  stepSection,
} from "./settingsIndex";

describe("settings IA", () => {
  it("the left list is the five approved sections, in order; Account & help is not a list row", () => {
    expect(LIST_SECTIONS.map((s) => s.label)).toEqual([
      "General",
      "Dictation",
      "Meetings",
      "Yappy & pill",
      "Models & privacy",
    ]);
    expect(SETTINGS_SECTIONS.map((s) => s.id)).toContain("account");
    expect(LIST_SECTIONS.map((s) => s.id)).not.toContain("account");
  });

  it("each block (so each control) appears in exactly one section", () => {
    const all = SETTINGS_SECTIONS.flatMap((s) => s.blocks);
    expect(new Set(all).size).toBe(all.length);
  });

  it("General holds shortcut, microphone, language and start at login", () => {
    expect(SETTINGS_SECTIONS.find((s) => s.id === "general")?.blocks).toEqual([
      "shortcut",
      "microphone",
      "language",
      "startup",
    ]);
    expect(sectionOfBlock("startup")).toBe("general");
    expect(sectionOfBlock("language")).toBe("general");
  });

  it("Dictation links to the Dictionary; Models & privacy holds the engines and privacy", () => {
    expect(sectionOfBlock("dictionary-link")).toBe("dictation");
    expect(sectionOfBlock("snippets")).toBe("dictation");
    expect(sectionOfBlock("audio-cleanup")).toBe("dictation");
    expect(sectionOfBlock("speech-models")).toBe("models");
    expect(sectionOfBlock("providers")).toBe("models");
    expect(sectionOfBlock("privacy")).toBe("models");
    expect(sectionOfBlock("license")).toBe("account");
    expect(sectionOfBlock("diagnostics")).toBe("account");
  });

  it("every old tab id (tray / deep links) lands on a section, and each section's home tab round-trips", () => {
    for (const t of SETTINGS_TABS) expect(TAB_SECTION[t.id]).toBeDefined();
    for (const s of SETTINGS_SECTIONS) expect(TAB_SECTION[SECTION_HOME_TAB[s.id]]).toBe(s.id);
    expect(sectionForTab("shortcut")).toBe("general");
    expect(sectionForTab("license")).toBe("account");
    expect(sectionForTab(null)).toBe("dictation");
  });

  it("the shell's engine refresh still sees Models & privacy (home tab is privacy)", () => {
    expect(SECTION_HOME_TAB.models).toBe("privacy");
  });
});

describe("settings search index", () => {
  it("Meetings holds call audio, kinds, retention, Speakers / voiceprints, destinations — in that order", () => {
    expect(SETTINGS_SECTIONS.find((s) => s.id === "meetings")?.blocks).toEqual([
      "meetings",
      "meeting-kinds",
      "meeting-retention",
      "speakers",
      "meeting-exports",
    ]);
    for (const q of ["call audio", "meeting kinds", "retention", "speakers", "voiceprints", "destinations"]) {
      const hits = searchSettings(q);
      expect([q, hits.length > 0]).toEqual([q, true]);
      expect([q, hits[0].section]).toEqual([q, "meetings"]);
    }
  });

  it("every block is indexed and every label is unique", () => {
    const blocks = new Set<SettingsBlock>(SETTINGS_SECTIONS.flatMap((s) => s.blocks));
    for (const b of blocks) expect(SETTINGS_INDEX.some((e) => e.block === b)).toBe(true);
    const labels = SETTINGS_INDEX.map((e) => e.label.toLowerCase());
    expect(new Set(labels).size).toBe(labels.length);
  });

  it("never names an AI vendor or model on a user-visible surface", () => {
    const text = [
      ...SETTINGS_SECTIONS.flatMap((s) => [s.label, s.hint]),
      ...SETTINGS_INDEX.flatMap((e) => [e.label, e.help]),
    ].join(" ");
    expect(text).not.toMatch(/openai|gemini|whisper|parakeet|anthropic|claude|gpt|llama|qwen|mistral|nvidia/i);
  });
});

describe("searchSettings", () => {
  it("a blank query has no hits", () => {
    expect(searchSettings("")).toEqual([]);
    expect(searchSettings("   ")).toEqual([]);
  });

  it("matches labels, case-insensitively, with the section it lives in", () => {
    const [hit] = searchSettings("DENOISE");
    expect(hit.block).toBe("audio-cleanup");
    expect(hit.section).toBe("dictation");
  });

  it("matches help text across sections", () => {
    const hits = searchSettings("login");
    expect(hits[0]).toMatchObject({ label: "Launch Yap at login", section: "general" });
  });

  it("every word must match; label matches rank first", () => {
    const hits = searchSettings("check updates");
    expect(hits[0].label).toBe("Check for updates");
    expect(searchSettings("voice")[0].label).toBe("Yappy's voice");
  });

  it("no match is an empty list (the empty state shows Clear search)", () => {
    expect(searchSettings("zzzz-no-such-setting")).toEqual([]);
  });

  it("honours the limit", () => {
    expect(searchSettings("e", 3).length).toBeLessThanOrEqual(3);
  });
});

describe("stepSection (↑/↓ in the section list)", () => {
  it("moves through the five list sections and wraps", () => {
    expect(stepSection("general", 1)).toBe("dictation");
    expect(stepSection("models", 1)).toBe("general");
    expect(stepSection("general", -1)).toBe("models");
    expect(stepSection("account", 1)).toBe("general");
    expect(stepSection("account", -1)).toBe("models");
  });
});
