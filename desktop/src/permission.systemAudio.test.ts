// yap-meetings-fix — the perm-truth banner covers System Audio Recording with
// the same truth rules as #211: it names the bundle path that was checked and
// the real System Settings path, and an unknown grant stays silent.
import { describe, expect, it } from "vitest";
import {
  permissionHealth,
  systemAudioDeniedSentence,
  SYSTEM_AUDIO_SETTINGS_PATH,
  type PermissionGrantRow,
} from "./permission";

const row = (status: "authorized" | "denied" | "unknown"): PermissionGrantRow => ({
  key: "audio_capture",
  label: "System Audio Recording",
  status,
  pane: "SystemAudio",
  detail: "rust detail",
});

describe("System Audio Recording banner row", () => {
  it("is silent when the grant is unknown (a quiet Mac is not a denial)", () => {
    expect(permissionHealth({ grants: [row("unknown")] }).visible).toBe(false);
    expect(permissionHealth({ grants: [row("authorized")] }).visible).toBe(false);
  });
  it("names the bundle path and the real System Settings path when this binary saw a denial", () => {
    const h = permissionHealth({ grants: [row("denied")], checkedBundlePath: "/Applications/Yap.app" });
    expect(h.visible).toBe(true);
    expect(h.line).toContain("System Audio Recording");
    expect(h.line).toContain("/Applications/Yap.app");
    expect(h.line).toContain(SYSTEM_AUDIO_SETTINGS_PATH);
    expect(h.action).toEqual({ kind: "settings", label: "Open System Audio Recording settings", pane: "SystemAudio" });
  });
  it("copy names Screen & System Audio Recording › System Audio Recording Only", () => {
    expect(SYSTEM_AUDIO_SETTINGS_PATH).toBe(
      "Privacy & Security › Screen & System Audio Recording › System Audio Recording Only",
    );
    expect(systemAudioDeniedSentence(null)).toContain("System Audio Recording is not granted for Yap");
  });
});
