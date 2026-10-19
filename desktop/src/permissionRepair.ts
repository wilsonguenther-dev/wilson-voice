// yap-p0-perm-repair — the Repair flow, as data.
//
// 0.9.0 changed signing identity. System Settings kept showing Yap ON, every
// live API said no, and the switch Wilson could see belonged to an older Yap.
// `perm_probe.rs` classifies a denial as `stale_identity` only when it has
// evidence of an identity change (a ledger entry under another authority /
// team, or a last-run file written by an earlier build of THIS code), so the
// 0.9.0 change itself reads as plain `denied`. Both are repairable. This
// module decides what a row says about each, and what Repair does. Copy and phase transitions
// are pure; the only side effects are the two invokes at the bottom.

import { invoke } from "@tauri-apps/api/core";

/** `perm_probe::ProbeState`, verbatim off the wire. */
export type ProbeState = "granted" | "denied" | "not_determined" | "stale_identity" | "unknown";

/** `perm_probe::Service` — `repair_permission`'s argument. */
export type RepairService = "accessibility" | "listen_event" | "post_event" | "microphone";

export interface ServiceProbe {
  service: RepairService;
  state: ProbeState;
  raw: string;
  label: string;
  api: string;
  note?: string | null;
}

/** `PermissionReport.probe` — ONE live probe the banner and onboarding share. */
export interface ProbeReport {
  accessibility: ServiceProbe;
  listenEvent: ServiceProbe;
  postEvent: ServiceProbe;
  microphone: ServiceProbe;
  listenEventCg: boolean;
  listenEventHid: string;
  listenEventDisagree: boolean;
  signatureChanged: boolean;
}

/**
 * fix-ax-probe-refresh — permission copy names the running product ("Yap Test"
 * on the test channel), because that is the name System Settings lists. Pure.
 */
export function nameProduct(text: string, product?: string | null): string {
  const p = product?.trim();
  return p && p !== "Yap" ? text.replace(/\bYap\b(?! Test)/g, p) : text;
}

/** What one Repair click did (`perm_probe::RepairOutcome`). */
export interface RepairOutcome {
  service: RepairService;
  commands: string[];
  ok: boolean;
  stderr: string;
  manualSteps: string;
  paneUrl: string;
  needsRelaunch: boolean;
  /** The `kTCCService*` names this Repair reset, row's own service first. */
  reset?: string[];
  /** "Yap" / "Yap Test". */
  product?: string;
  /** "Reset kTCCServiceAccessibility and kTCCServicePostEvent for Yap Test." */
  summary?: string;
}

/** The `permission_repair` event payload. */
export interface RepairEvent {
  service: RepairService;
  phase: "waiting" | "fixed" | "relaunch" | "timed_out";
  message: string;
}

export const REPAIR_EVENT = "permission_repair";

export const STALE_IDENTITY_LINE =
  "macOS lists Yap as allowed, but that permission belongs to an older Yap";
export const WAITING_LINE = "Waiting for you to switch Yap on…";
export const FIXED_LINE = "Fixed";
export const RELAUNCH_LINE = "Granted — quit and reopen Yap to finish";

/** Only a refusal is repairable. Not-asked and unreadable are not problems. */
export function canRepair(state: ProbeState): boolean {
  return state === "denied" || state === "stale_identity";
}

export interface RepairCopy {
  title: string;
  body: string;
}

/** The System Settings list a service's grant lives in. */
export function settingsList(service: RepairService): string {
  switch (service) {
    case "accessibility":
    case "post_event":
      return "Accessibility";
    case "listen_event":
      return "Input Monitoring";
    case "microphone":
      return "Microphone";
  }
}

export function serviceCopy(probe: ServiceProbe, product?: string | null): RepairCopy {
  const list = settingsList(probe.service);
  if (probe.state === "stale_identity") {
    return {
      title: nameProduct(`${probe.label}: ${STALE_IDENTITY_LINE}.`, product),
      body: nameProduct(
        `The switch you see in System Settings was given to a differently signed Yap, so it does nothing for this one. Repair clears Yap's own entry and opens ${list} — switch Yap on there.`,
        product,
      ),
    };
  }
  return {
    title: nameProduct(`${probe.label} is off for Yap.`, product),
    body: nameProduct(
      `${probe.api} answered no. Repair clears Yap's own entry and opens ${list} — switch Yap on there.`,
      product,
    ),
  };
}

/**
 * The services a Repair row is shown for, in reading order. PostEvent rides on
 * the Accessibility repair (which resets both), so it only gets its own row
 * when Accessibility itself is fine.
 */
export function repairRows(probe: ProbeReport | null | undefined): ServiceProbe[] {
  if (!probe) return [];
  const rows = [probe.accessibility, probe.listenEvent, probe.microphone].filter((p) =>
    canRepair(p.state),
  );
  // fix-ax-probe-refresh: never under a live Accessibility grant — text goes in
  // through AX then, and a PostEvent "no" breaks nothing (paste.rs
  // choose_insert_path). The Rust report already reconciles this; belt and braces.
  if (
    canRepair(probe.postEvent.state) &&
    !canRepair(probe.accessibility.state) &&
    probe.accessibility.state !== "granted"
  ) {
    rows.splice(1, 0, probe.postEvent);
  }
  return rows;
}

export type RepairPhase =
  | "idle"
  | "running"
  | "waiting"
  | "fixed"
  | "relaunch"
  | "timed_out"
  | "failed";

export interface RepairView {
  phase: RepairPhase;
  line: string;
  /** tccutil's stderr and the manual steps, when Repair could not run. */
  detail?: string;
}

export const IDLE: RepairView = { phase: "idle", line: "" };

export type RepairInput =
  | { kind: "started"; product?: string | null }
  | { kind: "outcome"; outcome: RepairOutcome }
  | { kind: "event"; event: RepairEvent }
  | { kind: "error"; message: string };

/** The phase machine, pure. Terminal phases ignore a late "waiting". */
export function reduceRepair(prev: RepairView, input: RepairInput): RepairView {
  switch (input.kind) {
    case "started":
      return { phase: "running", line: nameProduct("Resetting Yap's entry…", input.product) };
    case "error":
      return { phase: "failed", line: "Repair could not run.", detail: input.message };
    case "outcome":
      if (!input.outcome.ok) {
        return {
          phase: "failed",
          line: "Repair could not reset the permission.",
          detail: [input.outcome.stderr, input.outcome.manualSteps].filter(Boolean).join(" "),
        };
      }
      // The re-probe's first event may already have arrived.
      return prev.phase === "running" || prev.phase === "idle"
        ? {
            phase: "waiting",
            line: nameProduct(WAITING_LINE, input.outcome.product),
            // Say which services were reset, so a row can never silently
            // reset somebody else's grant again.
            ...(input.outcome.summary ? { detail: input.outcome.summary } : {}),
          }
        : prev;
    case "event":
      switch (input.event.phase) {
        case "waiting":
          return prev.phase === "fixed" || prev.phase === "relaunch" || prev.phase === "waiting"
            ? prev
            : { phase: "waiting", line: input.event.message || WAITING_LINE };
        case "fixed":
          return { phase: "fixed", line: FIXED_LINE };
        case "relaunch":
          return { phase: "relaunch", line: input.event.message || RELAUNCH_LINE };
        case "timed_out":
          return {
            phase: "timed_out",
            line:
              input.event.service === "listen_event"
                ? `${input.event.message} If you already switched it on, quit and reopen Yap to finish.`
                : input.event.message,
          };
      }
  }
  return prev;
}

/** Does this view offer the Relaunch button? Input Monitoring only. */
export function offersRelaunch(service: RepairService, view: RepairView): boolean {
  return (
    service === "listen_event" && (view.phase === "relaunch" || view.phase === "timed_out")
  );
}

/** Repair one service. The Rust side owns the argv, the bundle id and the pane. */
export function requestRepair(service: RepairService): Promise<RepairOutcome> {
  return invoke("repair_permission", { service });
}

/** What a failed Repair tells the user: tccutil's stderr and the manual steps. */
export function repairFailureMessage(outcome: RepairOutcome): string {
  const detail = [outcome.stderr, outcome.manualSteps].filter(Boolean).join(" ");
  return detail ? `Repair could not reset the permission. ${detail}` : "Repair could not reset the permission.";
}

export interface PillRepairDeps {
  toast: (message: string) => void;
  /** The pre-repair behaviour: open the privacy pane. */
  openPane: () => Promise<unknown>;
  refresh: () => unknown;
}

/**
 * The degraded-pill Repair. The Rust side returns BEFORE opening the pane on
 * every failure path, so an ok=false outcome is shown (stderr + manual steps)
 * and the pane is opened here — the button never fails silently. A rejected
 * invoke also falls back to the pane and is rethrown for the caller's toast.
 */
export async function repairFromPill(
  service: RepairService,
  deps: PillRepairDeps,
): Promise<RepairOutcome> {
  let outcome: RepairOutcome;
  try {
    outcome = await requestRepair(service);
  } catch (e) {
    await deps.openPane().catch(() => undefined);
    throw e;
  }
  if (!outcome.ok) {
    deps.toast(repairFailureMessage(outcome));
    await deps.openPane().catch((e) => deps.toast(String(e)));
  }
  deps.refresh();
  return outcome;
}

/** Quit and reopen (Input Monitoring only takes effect after a relaunch). */
export function relaunchApp(): Promise<void> {
  return invoke("relaunch_app");
}
