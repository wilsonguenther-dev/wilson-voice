// yap-test-channel-product-name — the main-window banner, rendered from a
// PermissionReport whose productName is "Yap Test" (the row System Settings
// lists for /Applications/Yap Test.app), names "Yap Test" and never a bare
// "Yap". The grant details are the words permissions.rs builds with the
// product; the banner's own words (multi-denied line, truth notes, Repair rows)
// are the frontend's and must take productName too.
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { PermissionHealthRow } from "./PermissionHealthRow";
import { PermissionRepair } from "./PermissionRepairRow";
import type { PermissionGrantRow } from "./permission";
import type { ProbeReport, ProbeState, ServiceProbe } from "./permissionRepair";

const T = "Yap Test";
const bareYap = (s: string) => /\bYap\b/.test(s.replace(/Yap Test/g, ""));
const text = (html: string) => html.replace(/<[^>]+>/g, " ").replace(/&#x27;/g, "'");

// Exactly what `permissions::build_report_with_grants(.., "Yap Test")` emits.
const ax: PermissionGrantRow = {
  key: "accessibility",
  label: "Accessibility",
  status: "denied",
  pane: "Accessibility",
  detail:
    "Accessibility is off for Yap Test, so the transcript is copied but never pasted. Turn Yap Test on in System Settings → Privacy & Security → Accessibility.",
} as PermissionGrantRow;
const im: PermissionGrantRow = {
  key: "input_monitoring",
  label: "Input Monitoring",
  status: "denied",
  pane: "InputMonitoring",
  detail:
    "Input Monitoring is off for Yap Test, so holding fn does nothing. Turn Yap Test on in System Settings → Privacy & Security → Input Monitoring.",
} as PermissionGrantRow;

const sp = (service: ServiceProbe["service"], label: string, api: string, state: ProbeState): ServiceProbe => ({
  service,
  state,
  raw: state === "granted" ? "granted" : "denied",
  label,
  api,
  note: null,
});

describe("permission banner names the running product", () => {
  it("one denied grant: the row's own line, naming Yap Test", () => {
    const out = text(renderToStaticMarkup(<PermissionHealthRow report={{ grants: [ax], productName: T }} />));
    expect(out).toContain("Accessibility is off for Yap Test");
    expect(bareYap(out), out).toBe(false);
  });

  it("several denied: the banner's own joining line takes productName", () => {
    const out = text(
      renderToStaticMarkup(
        <PermissionHealthRow
          report={{ grants: [ax, im], productName: T, duplicateBundles: ["/tmp/Yap.app"] }}
        />,
      ),
    );
    expect(out).toContain("Accessibility and Input Monitoring are off for Yap Test.");
    expect(out).toContain("Another copy of Yap Test at /tmp/Yap.app");
    expect(bareYap(out.replace("/tmp/Yap.app", "")), out).toBe(false);
  });

  it("Repair rows name Yap Test", () => {
    const probe = {
      accessibility: sp("accessibility", "Accessibility", "AXIsProcessTrusted", "denied"),
      postEvent: sp("post_event", "Event posting (⌘V)", "CGPreflightPostEventAccess", "denied"),
      listenEvent: sp("listen_event", "Input Monitoring", "CGPreflightListenEventAccess", "stale_identity"),
      microphone: sp("microphone", "Microphone", "AVCaptureDevice", "granted"),
    } as unknown as ProbeReport;
    const out = text(renderToStaticMarkup(<PermissionRepair probe={probe} productName={T} />));
    expect(out).toContain("Accessibility is off for Yap Test.");
    expect(bareYap(out), out).toBe(false);
  });

  it("the shipped build still says Yap", () => {
    const yapAx = { ...ax, detail: ax.detail.replace(/Yap Test/g, "Yap") };
    const out = text(renderToStaticMarkup(<PermissionHealthRow report={{ grants: [yapAx, im], productName: "Yap" }} />));
    expect(out).toContain("are off for Yap.");
  });
});
