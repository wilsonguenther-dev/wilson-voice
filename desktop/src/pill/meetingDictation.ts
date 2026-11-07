// yap-fx-06 — meeting + dictation at the same time, made legible.
//
// Three moments, one card (MeetingDictationCard.tsx), one pure model (here):
//
//   cancelled   Esc (or the pill's Cancel) ended a dictation while a meeting
//               keeps recording. The backend's `take_cancelled` carries
//               `meetingStillRecording` + `meetingNotice`; the card says the
//               sentence and offers Stop meeting (also ⌃⌘M) / Keep recording.
//   ask         Hands-free dictation started during a meeting. Asked ONCE per
//               meeting by the backend (`hands_free_meeting_ask`): Both /
//               Just the meeting (which cancels only the dictation).
//   stop-failed Stop meeting failed. The card stays until answered: Try again
//               or Quit Yap — quitting finalizes the recording, so force quit
//               is never the only way out.
//
// Nothing here talks to Tauri; the card does. Lines never carry transcript
// text, file paths or vendor names.

/** The Rust `MEETING_STILL_RECORDING_NOTICE`, word for word. */
export const MEETING_CANCEL_NOTICE = "Dictation cancelled — meeting still recording. Stop meeting?";
/** The Rust `HANDS_FREE_MEETING_ASK_EVENT`. */
export const HANDS_FREE_MEETING_ASK_EVENT = "hands_free_meeting_ask";
/** The Rust `TAKE_CANCELLED_EVENT`. */
export const TAKE_CANCELLED_EVENT = "take_cancelled";
/** The command the ask's answer goes to (`both: bool`). */
export const HANDS_FREE_MEETING_RESPOND = "hands_free_meeting_respond";
/** The last-resort quit (same teardown as Cmd-Q; it saves the meeting). */
export const PILL_QUIT_YAP = "pill_quit_yap";
/**
 * A window (DOM) event the meeting badge's Stop dispatches when `stop_meeting`
 * fails, so the failure lands on the same card instead of a silent catch.
 */
export const MEETING_STOP_FAILED_EVENT = "yap:meeting-stop-failed";
/** The meeting toggle chord, as the shortcut table spells it (shortcuts.rs). */
export const STOP_MEETING_CHORD = "⌃⌘M";

export const MEETING_NOTICE_TTL_MS = { cancelled: 8000, ask: 15000 } as const;

export type MeetingNotice =
  | { kind: "cancelled"; line: string }
  | { kind: "ask"; line: string; meetingId: string | null }
  | { kind: "stop-failed"; line: string };

export type MeetingNoticeAction = "stop" | "keep" | "both" | "meetingOnly" | "retry" | "quit" | "dismiss";

export interface NoticeButton {
  action: MeetingNoticeAction;
  label: string;
  /** The one filled (primary) button per card. */
  primary?: boolean;
  /** The keyboard hint shown beside the label, e.g. ⌃⌘M. */
  chord?: string;
}

/** `take_cancelled` → the "meeting still recording" notice, or null (no meeting). */
export function noticeFromCancel(payload: unknown): MeetingNotice | null {
  if (!payload || typeof payload !== "object") return null;
  const p = payload as { meetingStillRecording?: unknown; meetingNotice?: unknown };
  if (p.meetingStillRecording !== true) return null;
  const line = typeof p.meetingNotice === "string" && p.meetingNotice.trim() ? p.meetingNotice : MEETING_CANCEL_NOTICE;
  return { kind: "cancelled", line };
}

/** `hands_free_meeting_ask` → the one-time question. */
export function noticeFromAsk(payload: unknown): MeetingNotice {
  const p = (payload && typeof payload === "object" ? payload : {}) as { meetingId?: unknown; elapsedLabel?: unknown };
  const at = typeof p.elapsedLabel === "string" && p.elapsedLabel ? ` (${p.elapsedLabel})` : "";
  return {
    kind: "ask",
    line: `A meeting is recording${at}. Dictate too, and keep both going?`,
    meetingId: typeof p.meetingId === "string" ? p.meetingId : null,
  };
}

/** A failed Stop meeting → the card that never leaves force quit as the only exit. */
export function noticeFromStopFailure(): MeetingNotice {
  return {
    kind: "stop-failed",
    line: "Couldn't stop the meeting. Try again, or quit Yap — quitting saves the recording.",
  };
}

/** The buttons for a notice, in order. Exactly one primary each. */
export function noticeButtons(n: MeetingNotice): NoticeButton[] {
  switch (n.kind) {
    case "cancelled":
      return [
        { action: "stop", label: "Stop meeting", primary: true, chord: STOP_MEETING_CHORD },
        { action: "keep", label: "Keep recording" },
      ];
    case "ask":
      return [
        { action: "both", label: "Both", primary: true },
        { action: "meetingOnly", label: "Just the meeting" },
      ];
    case "stop-failed":
      return [
        { action: "retry", label: "Try again", primary: true },
        { action: "quit", label: "Quit Yap" },
      ];
  }
}

/** How long a notice stays unanswered; `null` = until answered (a failure). */
export function noticeTtlMs(n: MeetingNotice): number | null {
  return n.kind === "stop-failed" ? null : MEETING_NOTICE_TTL_MS[n.kind];
}

/**
 * A meeting status arrived: a notice ABOUT a running meeting is moot once the
 * meeting has stopped (from the tray, ⌃⌘M, the badge). A stop failure stays:
 * it is cleared only by a stop that worked, which also ends the recording.
 */
export function noticeAfterMeetingStatus(n: MeetingNotice | null, recording: boolean): MeetingNotice | null {
  if (!n || recording) return n;
  return null;
}

/** The forced matrix scene (`?meetingNotice=cancelled|ask|stop-failed`), dev/matrix builds only. */
export function forcedMeetingNotice(search: string): MeetingNotice | null {
  switch (new URLSearchParams(search).get("meetingNotice")) {
    case "cancelled":
      return { kind: "cancelled", line: MEETING_CANCEL_NOTICE };
    case "ask":
      return noticeFromAsk({ meetingId: "matrix", elapsedLabel: "00:12:04" });
    case "stop-failed":
      return noticeFromStopFailure();
    default:
      return null;
  }
}
