// yap-perm-truth — the banner text names the failing OS call, the bundle path
// it checked, a stale grant and any duplicate Yap. Pure: no Tauri, no React.
import { describe, expect, it } from "vitest";
import {
  permissionHealth,
  permissionTruthNotes,
  type FailingCondition,
  type PermissionGrantRow,
} from "./permission";

const AX_OFF: FailingCondition = {
  grant: "accessibility",
  label: "Accessibility",
  api: "AXIsProcessTrusted",
  observed: "false",
  text: "Accessibility false (AXIsProcessTrusted)",
};

const IM_OFF: FailingCondition = {
  grant: "input_monitoring",
  label: "Input Monitoring",
  api: "IOHIDCheckAccess listen",
  observed: "denied",
  text: "Input Monitoring false (IOHIDCheckAccess listen = denied)",
};

const grants = (axStatus: "authorized" | "denied"): PermissionGrantRow[] => [
  { key: "microphone", label: "Microphone", status: "authorized", pane: "Microphone", detail: "ok" },
  {
    key: "accessibility",
    label: "Accessibility",
    status: axStatus,
    pane: "Accessibility",
    detail: "Accessibility is off for Yap, so the transcript is copied but never pasted.",
  },
  { key: "input_monitoring", label: "Input Monitoring", status: "unknown", pane: "InputMonitoring", detail: "u" },
  { key: "audio_capture", label: "System audio recording", status: "unknown", pane: "SystemAudio", detail: "u" },
];

const STALE =
  "macOS still lists Yap as allowed in Accessibility, but that permission was given to Yap 0.8.5, signed differently. In System Settings → Privacy & Security → Accessibility, select Yap, click the minus button, then click plus and choose /Applications/Yap.app.";

describe("permission banner truth (yap-perm-truth)", () => {
  it("names each failing condition and the bundle path it checked", () => {
    const health = permissionHealth({
      grants: grants("denied"),
      checkedBundlePath: "/Applications/Yap.app",
      failingConditions: [AX_OFF, IM_OFF],
    });
    expect(health.visible).toBe(true);
    expect(health.notes).toEqual([
      "Accessibility is off for /Applications/Yap.app (AXIsProcessTrusted = false).",
      "Input Monitoring is off for /Applications/Yap.app (IOHIDCheckAccess listen = denied).",
    ]);
  });

  it("adds the stale-grant sentence and every duplicate Yap, in that order", () => {
    const health = permissionHealth({
      grants: grants("denied"),
      checkedBundlePath: "/Applications/Yap.app",
      failingConditions: [AX_OFF],
      staleGrants: [
        { grant: "accessibility", label: "Accessibility", previousVersion: "0.8.5", message: STALE },
      ],
      duplicateBundles: ["/Applications/Old builds/Yap.app", "/Users/w/Applications/Yap.app"],
    });
    expect(health.notes).toEqual([
      "Accessibility is off for /Applications/Yap.app (AXIsProcessTrusted = false).",
      STALE,
      "Another copy of Yap at /Applications/Old builds/Yap.app may hold the permission.",
      "Another copy of Yap at /Users/w/Applications/Yap.app may hold the permission.",
    ]);
  });

  it("never raises a banner on its own: no denial means no notes, even with a duplicate on disk", () => {
    const health = permissionHealth({
      grants: grants("authorized"),
      checkedBundlePath: "/Applications/Yap.app",
      duplicateBundles: ["/Applications/Yap copy.app"],
    });
    expect(health.visible).toBe(false);
    expect(health.notes).toEqual([]);
  });

  it("falls back to the bare condition text when the path is missing (older payload)", () => {
    expect(permissionTruthNotes({ failingConditions: [AX_OFF] })).toEqual([
      "Accessibility false (AXIsProcessTrusted).",
    ]);
    expect(permissionTruthNotes(null)).toEqual([]);
  });
});
