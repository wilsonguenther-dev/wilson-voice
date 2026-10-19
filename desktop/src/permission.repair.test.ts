// yap-p0-perm-repair — the Repair flow's copy, its invoke, and its phases.
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

import {
  IDLE,
  RELAUNCH_LINE,
  STALE_IDENTITY_LINE,
  WAITING_LINE,
  offersRelaunch,
  reduceRepair,
  repairRows,
  requestRepair,
  serviceCopy,
  type ProbeReport,
  type ProbeState,
  type RepairOutcome,
  type ServiceProbe,
} from "./permissionRepair";

const sp = (service: ServiceProbe["service"], label: string, state: ProbeState): ServiceProbe => ({
  service,
  state,
  raw: state === "granted" ? "granted" : "denied",
  label,
  api: "AXIsProcessTrusted",
  note: null,
});

const probe = (ax: ProbeState, im: ProbeState, mic: ProbeState = "granted"): ProbeReport => ({
  accessibility: sp("accessibility", "Accessibility", ax),
  listenEvent: sp("listen_event", "Input Monitoring", im),
  postEvent: sp("post_event", "Accessibility", ax),
  microphone: sp("microphone", "Microphone", mic),
  listenEventCg: false,
  listenEventHid: "denied",
  listenEventDisagree: false,
  signatureChanged: true,
});

const OK: RepairOutcome = {
  service: "accessibility",
  commands: ["/usr/bin/tccutil reset Accessibility com.wilsonguenther.wilson-voice"],
  ok: true,
  stderr: "",
  manualSteps: "",
  paneUrl: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
  needsRelaunch: false,
};

describe("permission repair", () => {
  beforeEach(() => invoke.mockReset());

  it("StaleIdentity copy says the switch belongs to an older Yap", () => {
    const rows = repairRows(probe("stale_identity", "stale_identity"));
    expect(rows.map((r) => r.service)).toEqual(["accessibility", "listen_event"]);
    const ax = serviceCopy(rows[0]);
    expect(ax.title).toBe(`Accessibility: ${STALE_IDENTITY_LINE}.`);
    expect(STALE_IDENTITY_LINE).toBe(
      "macOS lists Yap as allowed, but that permission belongs to an older Yap",
    );
    expect(serviceCopy(rows[1]).title).toContain("Input Monitoring");
    // Granted / not-asked / unreadable rows never offer a Repair.
    expect(repairRows(probe("granted", "unknown", "not_determined"))).toEqual([]);
  });

  it("Repair for Denied and StaleIdentity calls invoke(\"repair_permission\")", async () => {
    invoke.mockResolvedValue(OK);
    const rows = repairRows(probe("denied", "stale_identity", "denied"));
    expect(rows.map((r) => r.state)).toEqual(["denied", "stale_identity", "denied"]);
    for (const r of rows) await requestRepair(r.service);
    expect(invoke).toHaveBeenCalledTimes(3);
    expect(invoke).toHaveBeenNthCalledWith(1, "repair_permission", { service: "accessibility" });
    expect(invoke).toHaveBeenNthCalledWith(2, "repair_permission", { service: "listen_event" });
    expect(invoke).toHaveBeenNthCalledWith(3, "repair_permission", { service: "microphone" });
  });

  it("shows Fixed after the probe flips, and Relaunch for Input Monitoring", () => {
    let v = reduceRepair(IDLE, { kind: "started" });
    v = reduceRepair(v, { kind: "outcome", outcome: OK });
    expect(v).toEqual({ phase: "waiting", line: WAITING_LINE });
    v = reduceRepair(v, {
      kind: "event",
      event: { service: "accessibility", phase: "fixed", message: "Fixed" },
    });
    expect(v.phase).toBe("fixed");
    expect(v.line).toBe("Fixed");
    // A late "waiting" never un-fixes it.
    expect(
      reduceRepair(v, {
        kind: "event",
        event: { service: "accessibility", phase: "waiting", message: "" },
      }).phase,
    ).toBe("fixed");

    const im = reduceRepair(
      { phase: "waiting", line: WAITING_LINE },
      { kind: "event", event: { service: "listen_event", phase: "relaunch", message: "" } },
    );
    expect(im.line).toBe(RELAUNCH_LINE);
    expect(RELAUNCH_LINE).toBe("Granted — quit and reopen Yap to finish");
    expect(offersRelaunch("listen_event", im)).toBe(true);
    expect(offersRelaunch("accessibility", v)).toBe(false);
  });

  it("a failed tccutil shows its stderr and the manual steps", () => {
    const v = reduceRepair(
      { phase: "running", line: "" },
      {
        kind: "outcome",
        outcome: { ...OK, ok: false, stderr: "tccutil exited 1: No such bundle", manualSteps: "Open System Settings…" },
      },
    );
    expect(v.phase).toBe("failed");
    expect(v.detail).toContain("No such bundle");
    expect(v.detail).toContain("Open System Settings");
  });
});
