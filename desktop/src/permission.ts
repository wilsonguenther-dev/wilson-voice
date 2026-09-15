// PERM-B — the microphone permission state machine, as data.
//
// macOS TCC has FOUR answers to "may Yap use the microphone", and three of them
// were rendered here as the same not-a-check-mark: one boolean, one button whose
// label flipped between "Request Microphone" and "Granted ✓". That collapse is
// the bug. A user who has never been asked and a user macOS has permanently
// refused saw the identical screen, pressed the identical button, and for the
// denied user that button did NOTHING — TCC never re-prompts after a denial, so
// the call returns false without drawing a dialog, forever.
//
// This module is PURE on purpose: no `invoke`, no React, no `window`. The copy
// and the action for a status are decidable from the status alone, so they are
// testable without a Tauri host, and `Onboarding.tsx` stays the thing that
// merely renders the answer.
//
// The System Settings URL is deliberately NOT here. It is a compile-time
// constant in `permissions.rs` (`open_privacy_pane`), the same discipline
// `license.rs` applies to `PAYMENT_LINK_URL`: a deep link that has to be
// verified against a real OS build belongs on the side of the wire that can be
// verified, not in a string the frontend can drift.

import type { MicStatus } from "./micStatus";

/**
 * The four answers `AVCaptureDevice.authorizationStatus` can give.
 *
 * ALIASED to PERM-A's `MicStatus` rather than re-declared: two structurally
 * identical unions in one codebase drift the moment one of them gains a case,
 * and the compiler cannot tell you which screen went missing.
 */
export type MicPermissionStatus = MicStatus;

export const MIC_PERMISSION_STATUSES: readonly MicPermissionStatus[] = [
  "not_determined",
  "authorized",
  "denied",
  "restricted",
] as const;

/**
 * What the row says. `tone` drives styling only — never branch behaviour on it,
 * branch on the status.
 */
export interface PermissionCopy {
  /** Sentence-case heading for the row. */
  title: string;
  /** One or two sentences: what happens next, not what went wrong. */
  body: string;
  tone: "pending" | "settled" | "blocked";
}

/**
 * What the row's single button does, if it has one.
 *
 * - `request`  — invoke `request_microphone`; macOS draws its dialog.
 * - `settings` — invoke `open_privacy_settings` with `pane`; the Rust side owns
 *   the URL. `pane` is the argument `open_privacy_pane` matches on.
 * - `none`     — render no button. Either there is nothing to do, or nothing
 *   the user CAN do, and a button that cannot work is worse than no button.
 */
export type PermissionAction =
  | { kind: "request"; label: string }
  | { kind: "settings"; label: string; pane: "Microphone" }
  | { kind: "none" };

export function permissionCopy(status: MicPermissionStatus): PermissionCopy {
  switch (status) {
    case "not_determined":
      return {
        title: "Microphone",
        body: "Yap needs your microphone to hear you. Allowing it opens the macOS prompt, and this row updates as soon as you answer.",
        tone: "pending",
      };
    case "authorized":
      return {
        title: "Microphone",
        body: "Yap can hear you. Nothing else to do here.",
        tone: "settled",
      };
    case "denied":
      return {
        title: "macOS is blocking the microphone",
        body: "Yap was refused microphone access, and macOS will not ask again — the prompt only ever appears once. Turn Yap on under Privacy & Security, then Microphone, in System Settings. Come back to this window and the row updates on its own.",
        tone: "blocked",
      };
    case "restricted":
      return {
        title: "Microphone is managed for you",
        body: "A configuration profile on this Mac decides microphone access, so neither you nor Yap can change it here. Whoever manages this Mac can allow Yap under Privacy & Security, then Microphone.",
        tone: "blocked",
      };
  }
}

export function permissionAction(status: MicPermissionStatus): PermissionAction {
  switch (status) {
    case "not_determined":
      return { kind: "request", label: "Allow microphone access" };
    case "authorized":
      // A settled row. No button, no spinner.
      return { kind: "none" };
    case "denied":
      return {
        kind: "settings",
        label: "Open System Settings",
        pane: "Microphone",
      };
    case "restricted":
      // Deliberately NOT a settings action. The pane opens, the toggle is
      // greyed out by the MDM profile, and the user has been sent on an errand
      // that was never going to work.
      return { kind: "none" };
  }
}

/** True when dictation can actually run. Only one status qualifies. */
export function micIsUsable(status: MicPermissionStatus): boolean {
  return status === "authorized";
}

/**
 * Narrow whatever the backend said into one of the four. Unknown strings fall
 * back to `fallback` rather than silently becoming `denied` — showing the
 * denied screen to someone who was never asked is the one mistake this item
 * exists to prevent.
 */
export function parseMicPermissionStatus(
  value: unknown,
  fallback: MicPermissionStatus = "not_determined",
): MicPermissionStatus {
  return MIC_PERMISSION_STATUSES.includes(value as MicPermissionStatus)
    ? (value as MicPermissionStatus)
    : fallback;
}

/**
 * Today `get_permissions` answers with a BOOLEAN (`permissions.rs` reports a
 * usable default input device), and PERM-A is replacing it with the real TCC
 * status under a `microphoneStatus` field. This reads the real field when it is
 * there and derives the best available answer when it is not, so PERM-B's four
 * screens are live now and get MORE accurate — never differently shaped — the
 * moment PERM-A lands.
 *
 * `asked` is the frontend's own memory of having invoked `request_microphone`
 * in this session. Granted is `authorized` outright. Refused after an ask is a
 * denial. Not-granted-and-never-asked is `not_determined`, never `denied`.
 *
 * `restricted` is NOT derivable from a boolean by construction: an MDM block and
 * a plain refusal look identical from here. It arrives with `microphoneStatus`.
 */
export function deriveMicStatus(input: {
  microphoneStatus?: unknown;
  microphone: boolean;
  asked: boolean;
}): MicPermissionStatus {
  if (input.microphoneStatus !== undefined && input.microphoneStatus !== null) {
    return parseMicPermissionStatus(
      input.microphoneStatus,
      input.microphone ? "authorized" : "not_determined",
    );
  }
  if (input.microphone) return "authorized";
  return input.asked ? "denied" : "not_determined";
}

// ── PERM-E — the one permission health surface ─────────────────────────────
//
// Yap needs four grants and used to report them in three unrelated places: the
// onboarding checklist, `PermissionReport.summary`, and the meeting setup flow.
// A user whose Accessibility grant an OS update revoked found out when paste
// silently failed. This is the single source: `permissions.rs` builds the four
// rows, this function decides whether a human should be told anything at all,
// and `PermissionHealthRow.tsx` renders the answer.
//
// It is PURE, like everything else in this module — no `invoke`, no React.

/** The tri-state `permissions.rs::GrantState` serialises to. */
export type GrantStatus = "authorized" | "denied" | "unknown";

/** One row of `PermissionReport.grants`, verbatim off the wire. */
export interface PermissionGrantRow {
  /** `"microphone" | "accessibility" | "input_monitoring" | "audio_capture"` */
  key: string;
  label: string;
  status: GrantStatus;
  /** The `open_privacy_settings` pane argument. Rust owns the URL. */
  pane: string;
  detail: string;
}

/** The health row's single button, or none at all. */
export type PermissionHealthAction =
  | { kind: "settings"; label: string; pane: string }
  | { kind: "none" };

export interface PermissionHealth {
  /**
   * FALSE whenever nothing is denied — which includes the all-authorized case
   * AND every `unknown`. A permanent banner for a grant nobody can read is how
   * a good app becomes nagware, so `unknown` is silent by construction.
   */
  visible: boolean;
  /** One calm line. Empty when `visible` is false. */
  line: string;
  action: PermissionHealthAction;
}

const INVISIBLE: PermissionHealth = {
  visible: false,
  line: "",
  action: { kind: "none" },
};

/**
 * Is this a grant the user should be told about? Only a denial.
 *
 * `unknown` is deliberately NOT a problem: system audio capture has no readable
 * status at all (there is no `authorizationStatus` for it), and Input Monitoring
 * reports `kIOHIDAccessTypeUnknown` in ordinary situations where the fn hold is
 * working fine under an Accessibility grant.
 */
export function grantNags(status: GrantStatus): boolean {
  return status === "denied";
}

/**
 * The whole health decision, from the report.
 *
 * All four fine (or merely unreadable) → `{ visible: false }`, and NOTHING
 * renders. One denied → that row's own sentence and the one button that fixes
 * it. Several denied → still one line and still ONE button, pointing at the
 * first, because four persistent cards is the dashboard this item exists to not
 * build.
 */
export function permissionHealth(report: {
  grants?: PermissionGrantRow[] | null;
} | null | undefined): PermissionHealth {
  const grants = report?.grants ?? [];
  const denied = grants.filter((g) => grantNags(g.status));
  if (denied.length === 0) return INVISIBLE;

  const first = denied[0];
  const line =
    denied.length === 1
      ? first.detail
      : `${denied.map((g) => g.label).join(" and ")} are off for Yap. ${first.detail}`;

  return {
    visible: true,
    line,
    action: {
      kind: "settings",
      label: `Open ${first.label} settings`,
      pane: first.pane,
    },
  };
}
