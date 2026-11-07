/**
 * yap-1106-meetings-redesign — every decision the two-pane Meetings view
 * makes, as pure functions (YAP-UI-SPEC-2026-11 §3.3).
 *
 * The view (`views/Meetings.tsx`) is JSX around these: which date group a
 * meeting sits in, which people chips a row shows, what the filters keep,
 * which tab a detail opens on, what a key press does, when a deleted meeting
 * is really deleted (the 10 s Undo), and the one log line per user action.
 * None of it needs a window, so `meetingsLayout.test.ts` walks it directly.
 *
 * Honesty rule: a speaker label is a suggestion with a reason. Yap cannot
 * tell voices apart by name yet (that is the "Who's talking?" work), so the
 * People here are what the tracks prove — "You" is the microphone of a call
 * meeting, "Call" is the call's audio, "Speaker" is a voice nobody has named.
 */
import type { Meeting } from "../appTypes";
import {
  diarizationTarget,
  orderedTranscript,
  speakerLabel,
  MIC_SPEAKER_LABEL,
  MIC_TRACK,
  SYSTEM_SPEAKER_LABEL,
  SYSTEM_TRACK,
  UNCLUSTERED_SPEAKER_LABEL,
  type TranscriptSegment,
} from "./transcript";

// ─── Date groups ─────────────────────────────────────────────────────────────

export type DateGroupId = "today" | "yesterday" | "week" | "earlier";

export const DATE_GROUP_LABELS: Record<DateGroupId, string> = {
  today: "Today",
  yesterday: "Yesterday",
  week: "This week",
  earlier: "Earlier",
};

const DATE_GROUP_ORDER: DateGroupId[] = ["today", "yesterday", "week", "earlier"];

function startOfLocalDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/**
 * Which group a meeting started in, by LOCAL calendar day: today, yesterday,
 * the five days before that ("This week" = the last 7 days), or earlier. An
 * unreadable timestamp is "earlier" — never "today", which would be a lie at
 * the top of the list. A start in the future (clock skew) is "today".
 */
export function dateGroupOf(startedAt: string, now: Date): DateGroupId {
  const t = Date.parse(startedAt);
  if (!Number.isFinite(t)) return "earlier";
  const days = Math.round((startOfLocalDay(now) - startOfLocalDay(new Date(t))) / 86_400_000);
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  if (days < 7) return "week";
  return "earlier";
}

export interface DateGroup<M> {
  id: DateGroupId;
  label: string;
  meetings: M[];
}

/** The list's groups, in Today → Earlier order, empty groups left out. Rows keep their input order. */
export function groupMeetings<M extends Pick<Meeting, "startedAt">>(
  meetings: readonly M[],
  now: Date,
): DateGroup<M>[] {
  const buckets = new Map<DateGroupId, M[]>();
  for (const m of meetings) {
    const id = dateGroupOf(m.startedAt, now);
    const list = buckets.get(id) ?? [];
    list.push(m);
    buckets.set(id, list);
  }
  return DATE_GROUP_ORDER.filter((id) => buckets.has(id)).map((id) => ({
    id,
    label: DATE_GROUP_LABELS[id],
    meetings: buckets.get(id) as M[],
  }));
}

// ─── People ──────────────────────────────────────────────────────────────────

/**
 * The people chips a LIST row can honestly show, from the row alone: the
 * tracks it recorded and the kind it was started under. Mirrors the speaker
 * labels the transcript itself uses (`speakerLabel`).
 */
export function listPeople(
  meeting: Pick<Meeting, "kind" | "state"> & { sysWavPath?: string | null; people?: string[] | null },
): string[] {
  if (meeting.state === "recording") return [];
  // yap-1107-meetings-row-chips-baseline — names the backend already knows win;
  // blanks and repeats never become chips.
  if (Array.isArray(meeting.people)) {
    const named = Array.from(new Set(meeting.people.map((p) => p.trim()).filter((p) => p.length > 0)));
    if (named.length > 0) return named;
  }
  const twoTrack = Boolean(meeting.sysWavPath);
  const target = diarizationTarget(meeting.kind, twoTrack);
  const people = [speakerLabel(MIC_TRACK, target)];
  if (twoTrack) people.push(speakerLabel(SYSTEM_TRACK, target));
  return people;
}

/**
 * yap-1107-meetings-row-chips-baseline — how many people chips fit on a list
 * row's one chip line before the rest collapse into a "+N" chip. `widths` are
 * the chips' rendered widths in order, `gap` the line's column gap, `avail`
 * the width the chips may use (the state chip already taken out), and
 * `moreWidth` the "+N" chip's width. All chips fit → all of them; otherwise
 * the longest prefix that still leaves room for the "+N" chip (possibly 0,
 * so a very narrow row shows "+N" alone rather than wrapping or clipping).
 */
export function fitChipCount(widths: readonly number[], gap: number, avail: number, moreWidth: number): number {
  const n = widths.length;
  if (n === 0) return 0;
  const g = Number.isFinite(gap) && gap > 0 ? gap : 0;
  const room = Number.isFinite(avail) ? avail + 0.5 : Infinity; // sub-pixel layout slack
  const w = widths.map((x) => (Number.isFinite(x) && x > 0 ? x : 0));
  const all = w.reduce((a, b) => a + b, 0) + g * (n - 1);
  if (all <= room) return n;
  const more = Number.isFinite(moreWidth) && moreWidth > 0 ? moreWidth : 0;
  let used = more;
  let k = 0;
  while (k < n - 1 && used + w[k] + g <= room) {
    used += w[k] + g;
    k += 1;
  }
  return k;
}

export interface PersonRow {
  name: string;
  /** Lines this person spoke in the shown transcript. */
  lines: number;
  /** Seconds of speech, from segment end − start where the segment has an end. */
  seconds: number;
  /** The first line's offset, `mm:ss`. */
  firstOffset: string;
  /** false = a voice nobody has named yet ("Speaker"). */
  named: boolean;
  /** Why Yap labelled them this way — every guessed label carries one. */
  reason: string;
}

export const PERSON_REASON: Record<string, string> = {
  [MIC_SPEAKER_LABEL]: "Your microphone, on a call meeting",
  [SYSTEM_SPEAKER_LABEL]: "The call's audio from your Mac",
  [UNCLUSTERED_SPEAKER_LABEL]: "Voices on the microphone that Yap hasn't told apart yet",
};

/** The People tab: one row per speaker label in the shown transcript, most lines first. */
export function detailPeople<S extends TranscriptSegment & { endSeconds?: number | null }>(
  segments: readonly S[],
  kind: string | null | undefined,
): PersonRow[] {
  const rows = new Map<string, PersonRow>();
  for (const line of orderedTranscript(segments, kind)) {
    const row =
      rows.get(line.speaker) ??
      {
        name: line.speaker,
        lines: 0,
        seconds: 0,
        firstOffset: line.offset,
        named: line.speaker !== UNCLUSTERED_SPEAKER_LABEL,
        reason: PERSON_REASON[line.speaker] ?? "Named from this meeting",
      };
    row.lines += 1;
    const end = line.segment.endSeconds;
    if (typeof end === "number" && Number.isFinite(end) && end > line.startSeconds) {
      row.seconds += end - line.startSeconds;
    }
    rows.set(line.speaker, row);
  }
  return [...rows.values()].sort((a, b) => b.lines - a.lines || a.name.localeCompare(b.name));
}

/** The voices the "Who's talking?" strip asks about: the unnamed ones. */
export function unnamedVoices(people: readonly PersonRow[]): PersonRow[] {
  return people.filter((p) => !p.named);
}

/** `754` → `12 min`; under a minute → `under a minute`. */
export function talkTimeLabel(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 60) return "under a minute";
  return `${Math.round(seconds / 60)} min`;
}

// ─── Find: filters ───────────────────────────────────────────────────────────

export type NotesFilter = "all" | "with" | "without";

export interface MeetingFilter {
  /** "all" or a kind (`virtual | in_person | unknown`). */
  kind: string;
  notes: NotesFilter;
  /** A people-chip name, or null for anyone. */
  person: string | null;
}

export const NO_FILTER: MeetingFilter = { kind: "all", notes: "all", person: null };

export function filterActive(f: MeetingFilter): boolean {
  return f.kind !== "all" || f.notes !== "all" || f.person !== null;
}

function kindOf(kind: string | null | undefined): string {
  return kind === "virtual" || kind === "in_person" ? kind : "unknown";
}

type FilterRow = Pick<Meeting, "kind" | "state" | "summary"> & { sysWavPath?: string | null };

export function filterMeetings<M extends FilterRow>(meetings: readonly M[], f: MeetingFilter): M[] {
  return meetings.filter((m) => {
    if (f.kind !== "all" && kindOf(m.kind) !== f.kind) return false;
    const hasNotes = Boolean(m.summary && m.summary.trim());
    if (f.notes === "with" && !hasNotes) return false;
    if (f.notes === "without" && hasNotes) return false;
    if (f.person !== null && !listPeople(m).includes(f.person)) return false;
    return true;
  });
}

/** Every person a filter chip can offer, in first-seen order. */
export function peopleOptions(meetings: readonly FilterRow[]): string[] {
  const seen: string[] = [];
  for (const m of meetings) for (const p of listPeople(m)) if (!seen.includes(p)) seen.push(p);
  return seen;
}

// ─── Detail tabs ─────────────────────────────────────────────────────────────

export type DetailTab = "notes" | "transcript" | "people";

export const DETAIL_TABS: { id: DetailTab; label: string; keys: string }[] = [
  { id: "notes", label: "Notes", keys: "⌥⌘1" },
  { id: "transcript", label: "Transcript", keys: "⌥⌘2" },
  { id: "people", label: "People", keys: "⌥⌘3" },
];

/** Notes when there are notes to read, otherwise the transcript. */
export function defaultTab(detail: { meeting: Meeting }): DetailTab {
  return detail.meeting.summary && detail.meeting.summary.trim() ? "notes" : "transcript";
}

// ─── Keyboard ────────────────────────────────────────────────────────────────

export interface KeyLike {
  key: string;
  code?: string;
  metaKey?: boolean;
  altKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
}

export type MeetingsKeyAction =
  | { kind: "move"; by: 1 | -1 }
  | { kind: "open" }
  | { kind: "delete" }
  | { kind: "search" }
  | { kind: "close" }
  | { kind: "tab"; tab: DetailTab };

/**
 * What a key press does in the Meetings view (§3.3 Keyboard). ⌘1/2/3 belong
 * to the sidebar, so the tabs are ⌥⌘1/2/3 — read off `code`, because Option
 * changes `key` to "¡", "™", "£" on a US layout. `inField` = the key landed in
 * a text input, where arrows, Return and Delete are the field's own.
 */
export function meetingsKeyAction(e: KeyLike, inField: boolean): MeetingsKeyAction | null {
  if (e.metaKey && e.altKey && !e.ctrlKey) {
    const digit = e.code?.startsWith("Digit") ? e.code.slice(5) : e.key;
    const tab = DETAIL_TABS[Number(digit) - 1];
    return tab ? { kind: "tab", tab: tab.id } : null;
  }
  if (e.metaKey && !e.altKey && !e.ctrlKey && e.key.toLowerCase() === "f") return { kind: "search" };
  if (e.key === "Escape") return { kind: "close" };
  if (inField || e.metaKey || e.altKey || e.ctrlKey) return null;
  if (e.key === "ArrowDown") return { kind: "move", by: 1 };
  if (e.key === "ArrowUp") return { kind: "move", by: -1 };
  if (e.key === "Enter") return { kind: "open" };
  if (e.key === "Backspace" || e.key === "Delete") return { kind: "delete" };
  return null;
}

/** The next selected id, clamped to the list; nothing selected starts at the first (down) or last (up). */
export function moveSelection(ids: readonly string[], current: string | null, by: 1 | -1): string | null {
  if (ids.length === 0) return null;
  const at = current === null ? -1 : ids.indexOf(current);
  if (at < 0) return by === 1 ? ids[0] : ids[ids.length - 1];
  return ids[Math.min(ids.length - 1, Math.max(0, at + by))];
}

// ─── Delete + restore: the 10 s Undo ─────────────────────────────────────────

/** How long a deleted meeting can be put back. Its files go only after this. */
export const UNDO_DELETE_MS = 10_000;

export interface PendingDelete {
  id: string;
  title: string;
  /** ms since epoch when the person pressed Delete. */
  at: number;
}

export function schedulePendingDelete(
  pending: readonly PendingDelete[],
  id: string,
  title: string,
  now: number,
): PendingDelete[] {
  return [...pending.filter((p) => p.id !== id), { id, title, at: now }];
}

export function undoPendingDelete(pending: readonly PendingDelete[], id: string): PendingDelete[] {
  return pending.filter((p) => p.id !== id);
}

/** Split into the deletes whose Undo has run out (`due`) and those still undoable (`waiting`). */
export function duePendingDeletes(
  pending: readonly PendingDelete[],
  now: number,
): { due: PendingDelete[]; waiting: PendingDelete[] } {
  const due = pending.filter((p) => now - p.at >= UNDO_DELETE_MS);
  return { due, waiting: pending.filter((p) => now - p.at < UNDO_DELETE_MS) };
}

/** Whole seconds left to undo, never below 0. */
export function undoSecondsLeft(p: PendingDelete, now: number): number {
  return Math.max(0, Math.ceil((UNDO_DELETE_MS - (now - p.at)) / 1000));
}

/** The list without the meetings waiting out their Undo. */
export function withoutPending<M extends { id: string }>(meetings: readonly M[], pending: readonly PendingDelete[]): M[] {
  if (pending.length === 0) return [...meetings];
  const gone = new Set(pending.map((p) => p.id));
  return meetings.filter((m) => !gone.has(m.id));
}

// ─── Layout + recording hero ─────────────────────────────────────────────────

/** Under this width the detail covers the list and carries a Back button (§2 ways out). */
export const TWO_PANE_MIN_WIDTH = 900;

export function isNarrow(width: number): boolean {
  return Number.isFinite(width) && width > 0 && width < TWO_PANE_MIN_WIDTH;
}

/** Stop asks first only for a meeting under 10 s (a mis-click) or over 3 h (a forgotten one). */
export function stopNeedsConfirm(elapsedSeconds: number): boolean {
  return elapsedSeconds < 10 || elapsedSeconds > 3 * 3600;
}

/** The sun-clock (DECIDED, meeting-clock B): 0 at the start, 1 after an hour, held there. */
export function sunProgress(elapsedSeconds: number): number {
  if (!Number.isFinite(elapsedSeconds) || elapsedSeconds <= 0) return 0;
  return Math.min(1, elapsedSeconds / 3600);
}

// ─── Telemetry ───────────────────────────────────────────────────────────────

/** The first 8 characters of an id: enough to find the row, never a title or a path. */
export function shortId(id: string): string {
  return id.slice(0, 8);
}

/**
 * The ONE structured line per Meetings action (§1 telemetry): `ui meeting_<event> k=v`,
 * e.g. `ui meeting_delete id=1a2b3c4d undone=true`. Values are squashed to one
 * token; callers pass ids through `shortId` and never pass a title or transcript text.
 */
export function meetingsLogLine(event: string, fields: Record<string, string | number | boolean> = {}): string {
  const kv = Object.entries(fields).map(([k, v]) => `${k}=${String(v).replace(/\s+/g, "_")}`);
  return ["ui", `meeting_${event}`, ...kv].join(" ");
}
