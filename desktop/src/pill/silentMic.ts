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

/* ── fitting the row ─────────────────────────────────────────────────────────
 * loop3 bounce fix: the row used to be ONE 18 px line capped at HINT_W
 * (192 px) with the button beside the text, which left the text ~105 px —
 * "I can't hear MacBook Pro Microphone" (200 px) was cut to "I can't hear
 * Mac…". The row is now TWO rows inside the hint slot: the line on top with
 * the whole width, the button under it. Every line the row can show is fitted
 * here so it never overflows that width, measured with the same pessimistic
 * 6.0 px/char estimate the phase strip uses (live.ts STRIP_CHAR_PX); the full
 * warning stays in the tooltip and the accessible name.
 */

/** The row's width: the hint slot (pill/state.ts HINT_W). */
export const SILENT_MIC_ROW_W = 192;
/** `.silent-mic-row { padding: 0 8px }`, both sides. */
export const SILENT_MIC_ROW_PAD_X = 16;
/** The drawable width of the line, px. */
export const SILENT_MIC_LINE_PX = SILENT_MIC_ROW_W - SILENT_MIC_ROW_PAD_X;
/** Pessimistic advance width at 11 px system sans (live.ts STRIP_CHAR_PX). */
export const SILENT_MIC_CHAR_PX = 6.0;
/** The row's height: 3 px pad + 14 px line + 2 px gap + 14 px button + 3 px pad. */
export const SILENT_MIC_ROW_H = 36;
/** Hint lines (pill/state.ts HINT_LINE_H = 14) the layout reserves while the row is up. */
export const SILENT_MIC_HINT_LINES = 3;

const WARNING_PREFIX = "I can't hear ";
const SWITCHED_PREFIX = "Next take uses ";

/** Estimated drawn width of a line, px. */
export const silentMicLineWidthPx = (text: string): number => text.length * SILENT_MIC_CHAR_PX;

/** Does a line fit the row on one line? */
export const fitsSilentMicRow = (text: string): boolean => silentMicLineWidthPx(text) <= SILENT_MIC_LINE_PX;

/**
 * `prefix + name`, fitted to the row: as is when it fits; else without a
 * generic trailing "Microphone"/"Mic" ("MacBook Pro Microphone" is
 * "MacBook Pro"); else the name cut to the room left, with an ellipsis.
 */
export function fitMicName(prefix: string, name: string): string {
  const n = name.replace(/\s+/g, " ").trim();
  const full = prefix + n;
  if (fitsSilentMicRow(full)) return full;
  const short = n.replace(/\s+(microphone|mic)$/i, "").trim();
  const base = short || n;
  if (fitsSilentMicRow(prefix + base)) return prefix + base;
  const room = Math.max(1, Math.floor(SILENT_MIC_LINE_PX / SILENT_MIC_CHAR_PX) - prefix.length - 1);
  return prefix + base.slice(0, room).trimEnd() + "\u2026";
}

/** The warning as the row draws it: "I can't hear <mic>", fitted to the row. */
export function silentMicLine(warning: string): string {
  const one = warning.replace(/\s+/g, " ").trim();
  if (fitsSilentMicRow(one)) return one;
  if (one.startsWith(WARNING_PREFIX)) return fitMicName(WARNING_PREFIX, one.slice(WARNING_PREFIX.length));
  const room = Math.floor(SILENT_MIC_LINE_PX / SILENT_MIC_CHAR_PX) - 1;
  return one.slice(0, room).trimEnd() + "\u2026";
}

/** The row's line after a switch: the pick applies from the next take. */
export function switchedLine(name: string): string {
  const n = name.trim();
  return n ? fitMicName(SWITCHED_PREFIX, n) : "Next take uses the new mic";
}

/** No other microphone is plugged in: say so, with the one thing to do. */
export const NO_OTHER_MIC_LINE = "No other mic \u2014 check mute";

/** The switch failed (the backend logged why): one sentence. */
export const SWITCH_FAILED_LINE = "Couldn't switch. Try Settings";

/** The accessible name of the row: the warning plus what the button does. */
export function silentMicAccessibleName(warning: string): string {
  return `${warning}. ${SWITCH_MIC_LABEL} to use another microphone; still recording.`;
}

/** Matrix/dev only: `float.html?phase=listening&silentMic=1`. */
export const SAMPLE_SILENT_MIC_WARNING = "I can't hear MacBook Pro Microphone";

export function forcedSilentMic(search: string): string | null {
  return new URLSearchParams(search).get("silentMic") === "1" ? SAMPLE_SILENT_MIC_WARNING : null;
}
