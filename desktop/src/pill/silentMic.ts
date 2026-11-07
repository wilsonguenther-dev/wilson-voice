/**
 * WF-13 — "I can't hear <mic>": the pill's half of the silent-mic warning.
 *
 * The backend (record.rs `SilentMicCheck`) judges the first 1.5 s of every
 * take; when it is below the silence floor, `build_status` carries
 * `silentMicWarning` while the take keeps recording. The pill shows that line
 * in the hint row with a Switch mic button (WF-09's picker, one click: the
 * next microphone in the list). A pick is saved for the NEXT take — the take in
 * flight keeps the device it opened on — so after a switch the row says so
 * instead of pretending the live take moved.
 *
 * Pure, so the rules are unit tests; `SilentMicRow.tsx` is the thin view.
 */
import type { LivePhase } from "./live";

/** Mirrors `mic_select::MicDevice` (camelCase on the wire). */
export interface SilentMicDevice {
  id: string;
  name: string;
  isSystemDefault?: boolean;
  isVirtual?: boolean;
}

/** The slice of `mic_select::MicList` the switch needs. */
export interface SilentMicList {
  devices: SilentMicDevice[];
  chosenId: string | null;
  inUseId: string | null;
}

export const LIST_MICROPHONES_COMMAND = "list_microphones";
export const SET_MICROPHONE_COMMAND = "set_microphone";
export const SWITCH_MIC_LABEL = "Switch mic";

/** The warning as the status event carries it, trimmed; `null` when absent. */
export function parseSilentMicWarning(raw: unknown): string | null {
  if (typeof raw !== "string") return null;
  const one = raw.replace(/\s+/g, " ").trim();
  return one.length > 0 ? one : null;
}

/** Shown only during a live take — the two listening phases. */
export function silentMicShows(warning: string | null, phase: LivePhase): boolean {
  return warning !== null && (phase === "listening" || phase === "listening-locked");
}

/**
 * The mic one click of Switch mic moves to: the next REAL microphone after the
 * one in use (wrapping), skipping virtual/loopback inputs the way WF-10's
 * fallback does — a virtual input is the most likely silent one. Falls back to
 * a virtual input only when nothing physical is left. `null` when there is no
 * other microphone at all (the button then says so instead of doing nothing).
 */
export function nextMicrophone(list: SilentMicList | null | undefined): SilentMicDevice | null {
  const devices = list?.devices ?? [];
  if (devices.length === 0) return null;
  const current = list?.inUseId ?? list?.chosenId ?? null;
  const at = devices.findIndex((d) => d.id === current);
  const rotated = at < 0 ? devices : [...devices.slice(at + 1), ...devices.slice(0, at)];
  const others = rotated.filter((d) => d.id !== current);
  return others.find((d) => !d.isVirtual) ?? others[0] ?? null;
}

/** The row's line after a switch: the pick applies from the next take. */
export function switchedLine(name: string): string {
  const n = name.trim();
  return n ? `Next take uses ${n}` : "Next take uses the new microphone";
}

/** No other microphone is plugged in: say so, with the one thing to do. */
export const NO_OTHER_MIC_LINE = "No other microphone — check the mute switch";

/** The switch failed (the backend logged why): one sentence. */
export const SWITCH_FAILED_LINE = "Couldn't switch mics — try Settings → Audio";

/** The accessible name of the row: the warning plus what the button does. */
export function silentMicAccessibleName(warning: string): string {
  return `${warning}. ${SWITCH_MIC_LABEL} to use another microphone; still recording.`;
}

/** Matrix/dev only: `float.html?phase=listening&silentMic=1`. */
export const SAMPLE_SILENT_MIC_WARNING = "I can't hear MacBook Pro Microphone";

export function forcedSilentMic(search: string): string | null {
  return new URLSearchParams(search).get("silentMic") === "1" ? SAMPLE_SILENT_MIC_WARNING : null;
}
