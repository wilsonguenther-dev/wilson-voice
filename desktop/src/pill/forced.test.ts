import { describe, expect, it } from "vitest";
import { ALL_LIVE_PHASES, PHASE_ID } from "./live";
import {
  forcedPill,
  LICENSE_SCENES,
  matrixLicenseScenes,
  matrixPhases,
  resolvePhase,
  SAMPLE_PROGRESS,
  sceneLicense,
} from "./forced";
import { pillLicense } from "./license";

describe("forcedPill (yap24-UI3, dev-only matrix handle)", () => {
  it("is null without a phase, so a plain float.html is untouched", () => {
    expect(forcedPill("")).toBeNull();
    expect(forcedPill("?pill=yappy&dock=left")).toBeNull();
  });

  it("is null for an unknown phase rather than guessing one", () => {
    expect(forcedPill("?phase=listenin")).toBeNull();
    expect(resolvePhase("IDLE")).toBeNull();
  });

  it("accepts every phase by variant name AND by product id", () => {
    for (const p of ALL_LIVE_PHASES) {
      expect(forcedPill(`?phase=${p}`)?.phase).toBe(p);
      expect(forcedPill(`?phase=${PHASE_ID[p]}`)?.phase).toBe(p);
    }
    expect(resolvePhase("model_loading")).toBe("model-loading");
  });

  it("carries pill and dock only when they are real values", () => {
    expect(forcedPill("?phase=done&pill=yappy&dock=right")).toMatchObject({ style: "yappy", dock: "right" });
    expect(forcedPill("?phase=done&pill=fancy&dock=top")).toMatchObject({ style: null, dock: null });
  });

  it("seeds progress for transcribing only — the phase is never entered without it", () => {
    expect(forcedPill("?phase=transcribing")?.progress).toEqual(SAMPLE_PROGRESS);
    expect(forcedPill("?phase=thinking")?.progress).toBeNull();
  });
});

describe("matrixPhases", () => {
  it("is every declared phase, once, in ALL_LIVE_PHASES order, with its id and treatment", () => {
    const rows = matrixPhases();
    expect(rows.map((r) => r.phase)).toEqual([...ALL_LIVE_PHASES]);
    for (const r of rows) {
      expect(r.id).toBe(PHASE_ID[r.phase]);
      expect(r.label.length).toBeGreaterThan(0);
    }
  });
});

describe("yap24-PILL1 — license scenes", () => {
  it("forcedPill carries a known scene and ignores an unknown one", () => {
    expect(forcedPill("?phase=idle&license=trial-quiet")?.license?.state).toBe("trial");
    expect(forcedPill("?phase=idle&license=bogus")?.license).toBeNull();
    expect(forcedPill("?phase=idle")?.license).toBeNull();
  });

  it("every scene is a payload whose pill tone IS the scene name, checkout closed", () => {
    for (const scene of Object.keys(LICENSE_SCENES)) {
      const s = sceneLicense(scene)!;
      expect(s.purchase_open).toBe(false);
      const p = pillLicense(s);
      expect(p.show).toBe(true);
      expect(p.tone).toBe(scene);
      expect(p.action).not.toBe("purchase");
    }
    expect(sceneLicense("ended")!.state).toBe("license_required");
    expect(sceneLicense(null)).toBeNull();
    expect(sceneLicense("toString")).toBeNull();
  });

  it("matrix rows ride the idle phase, one per scene, named by the scene", () => {
    const rows = matrixLicenseScenes();
    expect(rows.map((r) => r.id)).toEqual(Object.keys(LICENSE_SCENES));
    for (const r of rows) {
      expect(r.phase).toBe("idle");
      expect(r.phaseId).toBe("idle");
      expect(r.license).toBe(r.id);
      expect(r.label.length).toBeGreaterThan(0);
    }
    expect(rows.some((r) => r.id === "trial-quiet")).toBe(true);
  });
});
