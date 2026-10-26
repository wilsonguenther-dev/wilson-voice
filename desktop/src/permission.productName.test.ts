// yap-test-channel-product-name — in "Yap Test.app" every permission sentence
// the frontend builds names "Yap Test" (the row System Settings lists), never a
// bare "Yap". One mechanism: `nameProduct` + `PermissionReport.productName`.
import { describe, expect, it } from "vitest";
import {
  duplicateBundleSentence,
  MIC_PERMISSION_STATUSES,
  nameProduct,
  permissionCopy,
  permissionHealth,
  permissionTruthNotes,
  systemAudioDeniedSentence,
} from "./permission";
import { nameProduct as reExported } from "./permissionRepair";
import { setupState } from "./meetings/systemAudio";

const T = "Yap Test";
const bareYap = (s: string) => /\bYap\b/.test(s.replace(/Yap Test/g, ""));

describe("permission copy names the running product", () => {
  it("there is one nameProduct: permissionRepair re-exports permission's", () => {
    expect(reExported).toBe(nameProduct);
  });

  it("mic copy, every status", () => {
    for (const s of MIC_PERMISSION_STATUSES) {
      const c = permissionCopy(s, T);
      expect(bareYap(c.title + " " + c.body), `${s}: ${c.body}`).toBe(false);
      expect(permissionCopy(s, "Yap")).toEqual(permissionCopy(s));
    }
    expect(permissionCopy("denied", T).body).toContain("Turn Yap Test on");
  });

  it("duplicate bundle, stale grant and system-audio lines", () => {
    expect(duplicateBundleSentence("/tmp/Yap.app", T)).toBe(
      "Another copy of Yap Test at /tmp/Yap.app may hold the permission.",
    );
    expect(systemAudioDeniedSentence(null, T)).toContain("reached Yap Test");
    const notes = permissionTruthNotes({
      productName: T,
      duplicateBundles: ["/Users/x/Applications/Yap Test.app"],
    });
    expect(notes).toEqual([
      "Another copy of Yap Test at /Users/x/Applications/Yap Test.app may hold the permission.",
    ]);
  });

  it("multi-denied banner line", () => {
    const h = permissionHealth({
      productName: T,
      grants: [
        { key: "microphone", label: "Microphone", status: "denied", detail: "Microphone is off for Yap Test." },
        { key: "accessibility", label: "Accessibility", status: "denied", detail: "x" },
      ],
    } as Parameters<typeof permissionHealth>[0]);
    expect(h.line.startsWith("Microphone and Accessibility are off for Yap Test.")).toBe(true);
  });

  it("System Audio Recording status lines, every verdict", () => {
    const verdicts = ["granted", "looks_denied", "ran", "failed", "unavailable"] as const;
    const states = [
      setupState(null, true, "macOS 14.4", T),
      ...verdicts.map((v) =>
        setupState(
          { hasRun: true, verdict: v, lastRunAt: "2026-10-01T00:00:00Z" } as Parameters<typeof setupState>[0],
          true,
          "macOS 14.4",
          T,
        ),
      ),
    ];
    for (const s of states) expect(bareYap(s.label), s.label).toBe(false);
    expect(states[0].label).toContain("Yap Test has not asked");
  });
});
