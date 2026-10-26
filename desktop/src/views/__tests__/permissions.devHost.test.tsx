/**
 * yap-dev-perm-probe-attribution — walk D5 step 2c: a dev build launched from a
 * terminal showed Accessibility and Input Monitoring as Yap's green rows; they
 * were the terminal's grants. The Permissions screen now says so in ONE line,
 * with no green rows and no Repair button.
 */
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined), emit: vi.fn(async () => undefined) }));

import { AppContext, type AppCtx } from "../../appShell";
import Permissions from "../Permissions";
import { PermissionRepair } from "../../PermissionRepairRow";
import {
  DEV_HOST_LINE,
  isDevHostAttributed,
  repairRows,
  type ProbeReport,
  type ProbeState,
  type ServiceProbe,
} from "../../permissionRepair";

const sp = (service: ServiceProbe["service"], label: string, state: ProbeState, raw = "granted"): ServiceProbe => ({
  service, state, raw, label, api: "x", note: null,
});

const probe = (state: ProbeState, devHostAttributed: boolean): ProbeReport => ({
  accessibility: sp("accessibility", "Accessibility", state),
  listenEvent: sp("listen_event", "Input Monitoring", state),
  postEvent: sp("post_event", "Event posting (⌘V)", state, "denied"),
  microphone: sp("microphone", "Microphone", state),
  listenEventCg: true,
  listenEventHid: "authorized",
  listenEventDisagree: false,
  signatureChanged: false,
  devHostAttributed,
});

function makeCtx(state: Record<string, unknown>): AppCtx {
  return new Proxy(state, {
    get(target, key: string) {
      if (key in target) return target[key];
      return () => undefined;
    },
  }) as unknown as AppCtx;
}

const perms = (p: ProbeReport) => ({
  accessibility: true, microphone: true, microphoneStatus: "authorized", grants: [],
  asrOk: true, asrDetail: "ok", summary: "All critical permissions look good for Yap.",
  allCriticalOk: true, productName: "Yap", probe: p,
});

function render(p: ProbeReport): string {
  const ctx = makeCtx({
    booting: false, permsError: null, perms: perms(p), modelSetup: { ready: true },
    status: { modelReady: true, recording: false, busy: false, message: "", lastError: null },
    sysAudio: null, sysAudioBusy: false, sysAudioGate: { available: true, message: null },
  });
  // renderToStaticMarkup escapes the apostrophe; read the text a human sees.
  return renderToStaticMarkup(
    <AppContext.Provider value={ctx}>
      <Permissions />
    </AppContext.Provider>,
  ).replace(/&#x27;/g, "'");
}

describe("Permissions screen on a dev build (DevHostAttributed)", () => {
  it("shows ONE line, no green rows, no Repair", () => {
    const html = render(probe("dev_host_attributed", true));
    expect(html).toContain(DEV_HOST_LINE);
    expect(DEV_HOST_LINE).toBe("dev build: permissions are the host terminal's");
    expect(html).toContain('data-dev-host-attributed="permissions"');
    // no rows at all, so no green row
    expect(html).not.toContain("perm-list");
    expect(html).not.toContain('class="ok"');
    expect(html).not.toContain("dot-ok");
    expect(html).not.toContain("Repair");
    expect(html).not.toContain("<li");
  });

  it("the installed build is unchanged: real rows, no dev line", () => {
    const html = render(probe("granted", false));
    expect(html).not.toContain(DEV_HOST_LINE);
    expect(html).toContain("perm-list");
    expect(html).toContain("<strong>Accessibility</strong>");
  });

  it("no Repair row is ever offered for a host-attributed probe, even with a raw denial", () => {
    const p = probe("dev_host_attributed", true);
    expect(repairRows(p)).toEqual([]);
    expect(renderToStaticMarkup(<PermissionRepair probe={p} productName="Yap" />)).toBe("");
  });

  it("detects host attribution from the flag or from every state", () => {
    expect(isDevHostAttributed(probe("dev_host_attributed", true))).toBe(true);
    expect(isDevHostAttributed(probe("dev_host_attributed", false))).toBe(true);
    expect(isDevHostAttributed(probe("granted", false))).toBe(false);
    expect(isDevHostAttributed(null)).toBe(false);
  });
});
