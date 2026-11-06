/**
 * WF-08 — the pill's Meeting popover, as data.
 *
 * The rail's Meeting button (WF-03) opens a small popover: today's next three
 * meetings, each a Record row; "Record now"; and, when the calendar is not in
 * use, "No upcoming meetings" with "Connect calendar", which opens the existing
 * setup card. WHAT it lists is decided in Rust (`calendar::meeting_popover`,
 * through the `meeting_popover` command) from the SAME snapshot the menu bar's
 * "Today's Meetings" reads (WF-07), so the two can never disagree. This module
 * only validates that answer and turns it into the popover's rows, in order.
 *
 * PURE: no `@tauri-apps/*`. slots.tsx renders the rows as a native popup menu
 * (it escapes the pill window, and arrow keys / Return / Escape come with it).
 */

/** One Record row (`calendar::PopoverMeeting`, serde camelCase). */
export interface PopoverMeeting {
  key: string;
  label: string;
  clock: string;
  hasLink: boolean;
}

/** Where "Connect calendar" goes: the main window's `navigate` and `settings-tab` payloads. */
export interface PopoverConnect {
  nav: string;
  tab: string | null;
}

export type PopoverState = "meetings" | "empty" | "not_connected";

/** `calendar::MeetingPopover`. */
export interface MeetingPopover {
  state: PopoverState;
  meetings: PopoverMeeting[];
  emptyText: string | null;
  connectText: string | null;
  connect: PopoverConnect | null;
  recordNowText: string;
}

/** Mirrors `meeting_control::KindChoice` (serde camelCase). */
export interface KindChoice {
  kind: string;
  label: string;
}

/** One row of the popover, top to bottom. */
export type PopoverEntry =
  | { type: "record"; key: string; text: string }
  | { type: "note"; text: string }
  | { type: "connect"; text: string; nav: string; tab: string | null }
  | { type: "separator" }
  | { type: "record-now"; text: string; kinds: KindChoice[] };

/** The words the Rust side sends; used only when its answer is unusable. */
export const RECORD_NOW_TEXT = "Record now";
export const MAX_POPOVER_MEETINGS = 3;

const STATES: readonly PopoverState[] = ["meetings", "empty", "not_connected"];
const str = (v: unknown): v is string => typeof v === "string" && v.trim().length > 0;
const strOrNull = (v: unknown): string | null => (str(v) ? v : null);

/**
 * The `meeting_popover` answer, checked. Anything that is not the shape this
 * build expects is `null`, and the popover falls back to Record now alone — a
 * broken answer never shows a half-drawn list.
 */
export function parseMeetingPopover(raw: unknown): MeetingPopover | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  if (!STATES.includes(o.state as PopoverState) || !Array.isArray(o.meetings)) return null;
  const meetings: PopoverMeeting[] = o.meetings
    .filter((m): m is Record<string, unknown> => !!m && typeof m === "object")
    .filter((m) => str(m.key) && str(m.label))
    .slice(0, MAX_POPOVER_MEETINGS)
    .map((m) => ({
      key: m.key as string,
      label: (m.label as string).trim(),
      clock: str(m.clock) ? m.clock.trim() : "",
      hasLink: m.hasLink === true,
    }));
  const c = o.connect as Record<string, unknown> | null | undefined;
  const connect: PopoverConnect | null =
    c && typeof c === "object" && str(c.nav) ? { nav: c.nav, tab: strOrNull(c.tab) } : null;
  return {
    state: o.state as PopoverState,
    meetings,
    emptyText: strOrNull(o.emptyText),
    connectText: strOrNull(o.connectText),
    connect,
    recordNowText: strOrNull(o.recordNowText) ?? RECORD_NOW_TEXT,
  };
}

/** "Record Standup · 2:00 PM" — the row says what a click does. */
export function recordRowText(m: PopoverMeeting): string {
  return m.clock ? `Record ${m.label} · ${m.clock}` : `Record ${m.label}`;
}

/**
 * The popover's rows, top to bottom, for each of the three states:
 *
 *  - meetings:       up to three Record rows, separator, Record now
 *  - empty:          "No upcoming meetings" (a disabled line), separator, Record now
 *  - not_connected:  "No upcoming meetings", Connect calendar, separator, Record now
 *
 * `model === null` (the command failed or this backend predates WF-08) is just
 * Record now, so the button still does what it did before. Record now carries
 * the kind picker's choices (the same list the tray offers); with none it
 * starts an Unknown-kind meeting directly.
 */
export function popoverEntries(model: MeetingPopover | null, kinds: readonly KindChoice[] | null | undefined): PopoverEntry[] {
  const kindList = (kinds ?? []).filter((k) => k && str(k.kind) && str(k.label));
  const recordNow: PopoverEntry = { type: "record-now", text: model?.recordNowText ?? RECORD_NOW_TEXT, kinds: kindList };
  if (!model) return [recordNow];
  const out: PopoverEntry[] = [];
  if (model.state === "meetings" && model.meetings.length > 0) {
    for (const m of model.meetings) out.push({ type: "record", key: m.key, text: recordRowText(m) });
  } else {
    out.push({ type: "note", text: model.emptyText ?? "No upcoming meetings" });
    if (model.state === "not_connected" && model.connect) {
      out.push({ type: "connect", text: model.connectText ?? "Connect calendar", nav: model.connect.nav, tab: model.connect.tab });
    }
  }
  out.push({ type: "separator" }, recordNow);
  return out;
}
