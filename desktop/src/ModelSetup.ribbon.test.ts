// yap-uiux-2026-10-16-O1 — the first-run walk (FR/onboarding-1-980x700.png)
// showed "Preparing your speech engine…" with no size, no percentage and no
// error even though `download_model` had been rejected: the catalog refresh
// that follows a download cleared the download's error. These pin the line.
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { ribbonLine, type ModelEntry, type RibbonInput } from "./ModelSetup";

const small: ModelEntry = {
  id: "small",
  name: "Small",
  description: "",
  architecture: "whisper",
  filename: "small.bin",
  quant: "q5",
  sizeBytes: 190_000_000,
  downloaded: false,
  selected: false,
  recommended: true,
  recommendedRank: 1,
};

const base: RibbonInput = {
  loaded: true,
  ready: false,
  target: small,
  downloading: null,
  progress: null,
  downloadError: null,
  catalogError: null,
};

describe("ribbonLine — the speech-model ribbon says size, progress and failure", () => {
  it("is silent before the catalog is read and once the engine is ready", () => {
    expect(ribbonLine({ ...base, loaded: false })).toBeNull();
    expect(ribbonLine({ ...base, ready: true })).toBeNull();
  });

  it("names the download size before the first byte arrives", () => {
    expect(ribbonLine({ ...base, downloading: "small", progress: { downloaded: 0, total: 0 } })).toBe(
      "Downloading speech model · 190 MB · starting…",
    );
  });

  it("shows bytes of total and a percentage while downloading", () => {
    expect(
      ribbonLine({ ...base, downloading: "small", progress: { downloaded: 80_000_000, total: 190_000_000 } }),
    ).toBe("Downloading speech model · 80 MB of 190 MB · 42%");
  });

  it("a failed download is a failure line, even after a clean catalog refresh", () => {
    // The state the walk hit: download_model rejected, then list_models
    // succeeded. Before O1 one shared error slot was cleared by that refresh.
    const line = ribbonLine({ ...base, downloadError: "network unreachable", catalogError: null });
    expect(line).toBe("Speech model download failed (190 MB) — network unreachable");
    expect(line).not.toMatch(/Preparing/);
  });

  it("a failed catalog read says so", () => {
    expect(ribbonLine({ ...base, target: null, catalogError: "boom" })).toBe(
      "Couldn't read the speech model list — boom",
    );
  });

  it("never falls back to the old size-less 'Preparing your speech engine…'", () => {
    for (const s of [
      base,
      { ...base, downloading: "small" },
      { ...base, downloading: "small", progress: { downloaded: 5, total: 10 } },
    ]) {
      expect(ribbonLine(s)).toMatch(/190 MB|of 10 MB|%/);
    }
  });
});
