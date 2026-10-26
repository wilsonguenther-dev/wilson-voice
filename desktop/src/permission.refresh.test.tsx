// fix-ax-probe-refresh — Wilson 2026-10-19 (Yap Test): Accessibility was
// switched on at 16:08:20Z and the banner still read "Accessibility is off for
// Yap. Waiting for you to switch Yap on…"; every Repair reset only PostEvent.
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { PermissionRepair } from "./PermissionRepairRow";
import {
  nameProduct,
  reduceRepair,
  repairRows,
  serviceCopy,
  IDLE,
  type ProbeReport,
  type ProbeState,
  type ServiceProbe,
} from "./permissionRepair";
import { permissionHealth } from "./permission";

const sp = (service: ServiceProbe["service"], label: string, api: string, state: ProbeState): ServiceProbe => ({
  service,
  state,
  raw: state === "granted" ? "granted" : "denied",
  label,
  api,
  note: null,
});

const probe = (ax: ProbeState, post: ProbeState): ProbeReport => ({
  accessibility: sp("accessibility", "Accessibility", "AXIsProcessTrusted", ax),
  listenEvent: sp("listen_event", "Input Monitoring", "CGPreflightListenEventAccess", "granted"),
  postEvent: sp("post_event", "Event posting (⌘V)", "CGPreflightPostEventAccess", post),
  microphone: sp("microphone", "Microphone", "AVCaptureDevice", "granted"),
  listenEventCg: true,
  listenEventHid: "authorized",
  listenEventDisagree: false,
  signatureChanged: false,
});

describe("the banner follows the live probe", () => {
  it("flipping Accessibility to granted clears the Accessibility row (re-render with the new report)", () => {
    const before = renderToStaticMarkup(
      <PermissionRepair probe={probe("denied", "denied")} productName="Yap Test" />,
    );
    expect(before).toContain("Accessibility is off for Yap Test.");
    expect(before).toContain('data-repair-service="accessibility"');
    expect(before).not.toContain('data-repair-service="post_event"');
    // Both granted: nothing to repair.
    const after = renderToStaticMarkup(
      <PermissionRepair probe={probe("granted", "granted")} productName="Yap Test" />,
    );
    expect(after).toBe("");
  });

  it("the 16:08:20Z state keeps the PostEvent row, never titled Accessibility", () => {
    // AX granted, CGPreflightPostEventAccess false: an element that refuses the
    // AX insert falls back to ⌘V, so the denial is real and stays repairable.
    const html = renderToStaticMarkup(
      <PermissionRepair probe={probe("granted", "denied")} productName="Yap Test" />,
    );
    expect(html).toContain('data-repair-service="post_event"');
    expect(html).not.toContain('data-repair-service="accessibility"');
    expect(html).not.toContain("Accessibility is off");
    expect(repairRows(probe("granted", "denied")).map((r) => r.service)).toEqual(["post_event"]);
    expect(repairRows(probe("granted", "stale_identity")).map((r) => r.service)).toEqual(["post_event"]);
    // Accessibility refused: ONE row, the Accessibility one, whose Repair resets both.
    expect(repairRows(probe("denied", "denied")).map((r) => r.service)).toEqual(["accessibility"]);
  });

  it("a PostEvent row, when it shows, is never titled Accessibility", () => {
    const p = probe("unknown", "denied");
    const rows = repairRows(p);
    expect(rows.map((r) => r.service)).toEqual(["post_event"]);
    expect(serviceCopy(rows[0], "Yap Test").title).toBe("Event posting (⌘V) is off for Yap Test.");
  });
});

describe("Repair says what it reset, for which product", () => {
  it("shows the reset summary and the product in the waiting line", () => {
    const started = reduceRepair(IDLE, { kind: "started", product: "Yap Test" });
    expect(started.line).toBe("Resetting Yap Test's entry…");
    const v = reduceRepair(started, {
      kind: "outcome",
      outcome: {
        service: "accessibility",
        commands: [
          "/usr/bin/tccutil reset Accessibility com.wilsonguenther.wilson-voice.test",
          "/usr/bin/tccutil reset PostEvent com.wilsonguenther.wilson-voice.test",
        ],
        ok: true,
        stderr: "",
        manualSteps: "",
        paneUrl: "x",
        needsRelaunch: false,
        reset: ["kTCCServiceAccessibility", "kTCCServicePostEvent"],
        product: "Yap Test",
        summary: "Reset kTCCServiceAccessibility and kTCCServicePostEvent for Yap Test.",
      },
    });
    expect(v).toEqual({
      phase: "waiting",
      line: "Waiting for you to switch Yap Test on…",
      detail: "Reset kTCCServiceAccessibility and kTCCServicePostEvent for Yap Test.",
    });
  });

  it("nameProduct swaps the word, never doubles it, and leaves production alone", () => {
    expect(nameProduct("switch Yap on", "Yap Test")).toBe("switch Yap Test on");
    expect(nameProduct("switch Yap Test on", "Yap Test")).toBe("switch Yap Test on");
    expect(nameProduct("switch Yap on", "Yap")).toBe("switch Yap on");
    expect(nameProduct("switch Yap on", undefined)).toBe("switch Yap on");
  });

  it("the health banner names the product too", () => {
    const row = (key: string, label: string) => ({ key, label, status: "denied", detail: "d", pane: "p" });
    const h = permissionHealth({
      grants: [row("accessibility", "Accessibility"), row("microphone", "Microphone")] as never,
      productName: "Yap Test",
    });
    expect(h.line.startsWith("Accessibility and Microphone are off for Yap Test.")).toBe(true);
  });
});
