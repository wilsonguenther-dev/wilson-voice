/**
 * yap-1106-settings-redesign (§14, spec §1.4) — Settings › Meetings › Meeting
 * kinds: the kind a NEW meeting starts as. The start-of-meeting picker on the
 * Meetings screen still changes it for the next meeting; this is only where the
 * shell's `meetingKind` begins at launch.
 *
 * `localStorage` is the right home: the kind is a diarization hint, never a
 * gate (YV125), so losing it costs one "Not sure" start, which is not worth a
 * settings migration. Anything unreadable falls back to `unknown`, the skip
 * answer the record button has always used.
 */
export const MEETING_KIND_DEFAULT_KEY = "yap.meetingKindDefault";

/** The kinds `meeting_control::KIND_PICKER` publishes. */
export const KNOWN_MEETING_KINDS = ["in_person", "virtual", "unknown"] as const;

/** `meeting_control::KindChoice.kind` values are serde snake_case. */
export function isKnownMeetingKind(kind: unknown): kind is string {
  return typeof kind === "string" && (KNOWN_MEETING_KINDS as readonly string[]).includes(kind);
}

export function readMeetingKindDefault(): string {
  try {
    const raw = window.localStorage.getItem(MEETING_KIND_DEFAULT_KEY);
    return isKnownMeetingKind(raw) ? raw : "unknown";
  } catch {
    return "unknown"; // private mode / storage disabled — the skip answer
  }
}

/** Returns false when storage refused the write (the caller says so once). */
export function writeMeetingKindDefault(kind: string): boolean {
  try {
    window.localStorage.setItem(MEETING_KIND_DEFAULT_KEY, isKnownMeetingKind(kind) ? kind : "unknown");
    return true;
  } catch {
    return false;
  }
}
