// PERM-A — the frontend half of the microphone authorization contract.
//
// `request_microphone` no longer blocks: macOS answers the TCC dialog on its own
// schedule, and blocking the AppKit main thread while a human decides freezes the
// whole app. So the command returns the status at the moment of asking and the UI
// has to find out the rest by reading the authoritative status back.
//
// The old UI guessed — `setTimeout(refreshPerms, 900)` — which is both too long
// when the answer is already final (Denied never shows a dialog) and far too
// short when it is not (a human takes seconds).

export type MicStatus =
  | "not_determined"
  | "denied"
  | "restricted"
  | "authorized";

/** Anything we do not recognise is NOT a grant. */
export function normalizeMicStatus(raw: unknown): MicStatus {
  switch (raw) {
    case "authorized":
    case "denied":
    case "restricted":
    case "not_determined":
      return raw;
    default:
      return "restricted";
  }
}

/** A final answer needs no polling — no dialog is coming. */
export function isFinalMicStatus(s: MicStatus): boolean {
  return s !== "not_determined";
}

export const MIC_POLL_MS = 400;
/** 60 s of 400 ms ticks — long enough for a human, bounded so it cannot leak. */
export const MIC_POLL_MAX = 150;

export interface MicDecisionDeps {
  /** invoke("request_microphone") */
  request: () => Promise<unknown>;
  /** invoke("microphone_status") */
  read: () => Promise<unknown>;
  sleep: (ms: number) => Promise<void>;
  maxPolls?: number;
  pollMs?: number;
}

/**
 * Ask, then read until macOS has decided. Resolves with the final status, or
 * with `"not_determined"` if the user simply never answered.
 */
export async function awaitMicDecision(
  deps: MicDecisionDeps,
): Promise<MicStatus> {
  const maxPolls = deps.maxPolls ?? MIC_POLL_MAX;
  const pollMs = deps.pollMs ?? MIC_POLL_MS;
  const first = normalizeMicStatus(await deps.request());
  if (isFinalMicStatus(first)) return first;
  for (let i = 0; i < maxPolls; i++) {
    await deps.sleep(pollMs);
    const s = normalizeMicStatus(await deps.read());
    if (isFinalMicStatus(s)) return s;
  }
  return "not_determined";
}
