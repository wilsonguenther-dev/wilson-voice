import { describe, expect, it } from "vitest";
import type { HotkeysStatus } from "./appTypes";
import { carbonRegistered, dictateLabel, fnDelivering, repairPane } from "./hotkeyStatus";

const status = (hotkeys?: HotkeysStatus) => ({ hotkeys });

describe("hotkeyStatus", () => {
  it("carbonRegistered is true only for a register call that returned Ok", () => {
    expect(carbonRegistered(status({ carbon: { state: "registered" }, fnTap: { state: "notAttempted" } }))).toBe(true);
    expect(carbonRegistered(status({ carbon: { state: "failed", reason: "-9878" }, fnTap: { state: "notAttempted" } }))).toBe(false);
    expect(carbonRegistered(status({ carbon: { state: "notAttempted" }, fnTap: { state: "notAttempted" } }))).toBe(false);
    expect(carbonRegistered(status())).toBe(false);
  });

  it("fnDelivering is true only after the tap delivered a real key event", () => {
    expect(fnDelivering(status({ carbon: { state: "registered" }, fnTap: { state: "delivering" } }))).toBe(true);
    expect(fnDelivering(status({ carbon: { state: "registered" }, fnTap: { state: "armedNoDelivery" } }))).toBe(false);
    expect(fnDelivering(status())).toBe(false);
  });

  it("dictateLabel falls back to the default chord for a blank or missing rebind", () => {
    expect(dictateLabel({ dictateToggleLabel: "⌃⌥Space" })).toBe("⌃⌥Space");
    expect(dictateLabel({ dictateToggleLabel: "" })).toBe("⌃⌥⌘Space");
    expect(dictateLabel({})).toBe("⌃⌥⌘Space");
  });

  it("repairPane opens Input Monitoring except when there is nothing a grant can fix", () => {
    const pane = (fnTap: HotkeysStatus["fnTap"]) =>
      repairPane(status({ carbon: { state: "registered" }, fnTap }));
    expect(pane({ state: "delivering" })).toBeNull();
    expect(pane({ state: "secureInput", reason: "1Password" })).toBeNull();
    expect(pane({ state: "blocked", reason: "tap disabled" })).toBe("InputMonitoring");
    expect(pane({ state: "armedNoDelivery" })).toBe("InputMonitoring");
    expect(repairPane(status())).toBe("InputMonitoring");
  });
});
