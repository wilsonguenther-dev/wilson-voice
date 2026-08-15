/**
 * meeting — the pill's half of YV95's manual start/stop surface.
 *
 * The pure part lives here (and is unit-tested); the React bits live in
 * MeetingBadge.tsx. Split for the same reason live.ts is split from the pills:
 * the formatting rule is the thing that can be wrong, and it should not need a
 * canvas to check.
 *
 * ## Energy (OS-12 fix 1)
 *
 * The elapsed clock is NOT a `setInterval` in the webview and NOT a rAF frame:
 * the backend emits one `meeting` event per second with the label ALREADY
 * rendered (`meetings::format_offset` in Rust), and this module only stores it.
 * A three-hour meeting therefore wakes the JS thread ~10,800 times, once per
 * visible change, instead of 648,000 times to redraw a clock that changed once a
 * second. The recording pulse is a CSS animation on opacity/transform — it runs
 * on the compositor and does not wake JS at all.
 *
 * Those two numbers only describe THIS module. The other half of fix (1) is that
 * the Yappy pill's canvas loop parks for the meeting (`meetingRecording` in
 * live.ts `framePlan`) — otherwise its settled-idle ambient tick redraws the
 * scene every 100 ms for the whole session, 108,000 frames over three hours,
 * which dwarfs the clock this module was written to make cheap.
 */

/**
 * The Tauri event the backend broadcasts status on, re-exported from the ONE
 * place the frontend declares it (`meetings/consent.ts`).
 *
 * Three surfaces listen to this string — the main window's banner, the pill's
 * badge, and YV96's one-time capture notice — and they were written on three
 * different branches. `src-tauri/tests/meeting_event_contract.rs` reads the
 * declaration out of `consent.ts` and asserts it against Rust's
 * `meetings::MEETING_EVENT`, and `no_surface_retypes_the_meeting_event_name`
 * asserts that nothing re-types the literal, so a rename cannot leave one of the
 * three listening to a name nobody emits.
 */
export { MEETING_EVENT } from "../meetings/consent";

/** Mirrors `meeting_control::MeetingStatus` (serde camelCase). */
export interface MeetingStatus {
  recording: boolean;
  id?: string | null;
  title?: string | null;
  elapsedSeconds: number;
  elapsedLabel?: string;
  captureAvailable: boolean;
  unavailableReason?: string | null;
  /**
   * YV110 — the running meeting's honest system-audio sentence, or absent when
   * both tracks are recording and there is nothing to say.
   *
   * Two sentences reach this field, and they are matrix rows 1 and 2: why this
   * meeting is recording the microphone only (macOS too old, the setup step
   * never run, permission looks denied), and — for a meeting that DID attach
   * the call's audio and then lost it — that the track is gone and the mic is
   * still recording. Rendered rather than logged, because a recording that
   * quietly captured half of what the user expected is the failure this whole
   * phase is about.
   */
  systemAudio?: string | null;
}

export const IDLE_MEETING: MeetingStatus = {
  recording: false,
  id: null,
  title: null,
  elapsedSeconds: 0,
  elapsedLabel: "00:00:00",
  captureAvailable: false,
};

/** `3725` → `01:02:05`. The TS mirror of `meetings::format_offset`. */
export function formatElapsed(seconds: number): string {
  const total = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(h)}:${pad(m)}:${pad(s)}`;
}

/**
 * The clock to render. Prefers the label the backend already rendered so the
 * pill and the main window can never disagree by a second at a boundary, and
 * falls back to formatting the count locally — a backend that predates the
 * field must show a time, never `undefined`.
 */
export function elapsedLabel(status: MeetingStatus | null | undefined): string {
  if (!status) return "00:00:00";
  if (status.elapsedLabel && status.elapsedLabel.length > 0) return status.elapsedLabel;
  return formatElapsed(status.elapsedSeconds);
}

/**
 * What the Record control says right now. One function so the tray item, the
 * pill's button and the Meetings empty state cannot describe the same action
 * three different ways.
 */
export function recordLabel(status: MeetingStatus): string {
  if (status.recording) return "Stop meeting";
  return "Record this meeting";
}

/**
 * The system-audio sentence to show beside a running meeting, or `null`.
 *
 * A function rather than a raw field read for the same reason `elapsedLabel` is
 * one: it is rendered by three surfaces (the pill badge, the main window's
 * recording bar, and any future meeting detail), and "show it only while
 * recording" is a rule those three must not each decide for themselves. A
 * stale badge on an idle app would describe a meeting that is over.
 */
export function systemAudioBadge(status: MeetingStatus | null | undefined): string | null {
  if (!status || !status.recording) return null;
  const note = status.systemAudio;
  return note && note.trim().length > 0 ? note : null;
}

/** Why the control is disabled, or `null` when it is not. */
export function disabledReason(status: MeetingStatus): string | null {
  if (status.recording || status.captureAvailable) return null;
  return status.unavailableReason || "Meeting recording is unavailable in this build.";
}
