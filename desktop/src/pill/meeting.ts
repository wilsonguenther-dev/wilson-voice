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

/** Mirrors `meeting_control::MeetingStatus` (serde camelCase). */
export interface MeetingStatus {
  recording: boolean;
  id?: string | null;
  title?: string | null;
  elapsedSeconds: number;
  elapsedLabel?: string;
  captureAvailable: boolean;
  unavailableReason?: string | null;
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

/** Why the control is disabled, or `null` when it is not. */
export function disabledReason(status: MeetingStatus): string | null {
  if (status.recording || status.captureAvailable) return null;
  return status.unavailableReason || "Meeting recording is unavailable in this build.";
}
