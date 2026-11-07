/**
 * yap-1106-home-full-width-room (FB-07 + home-contained-scroll-footer) — the
 * pure half of the round-two Home, approved by Wilson on the Home design page
 * ("What to approve", 2026-11-06 17:08Z). Everything here is a plain function
 * so the vitest can pin it without a webview:
 *
 *   - the room fills the main column at a WHOLE-NUMBER scale (4x, 3x or 2x of
 *     the 320 x 160 studio, never a fraction — option A; yap-1106-studio-room
 *     moved the room to the approved layered studio, shown at 3x on the design
 *     page's 960 px column);
 *   - the three stat tiles become one quiet line under the room;
 *   - Home shows the last 5 takes, then a "See all" row (no list grows as you
 *     scroll); History pages 50 at a time;
 *   - failed and cancelled takes sit in "Needs attention", hidden when empty;
 *   - during a meeting the room label reads "Recording meeting · mm:ss".
 */

/** The Home room's art grid (YappyHouse draws the studio at exactly this size;
 * equal to studio/studio.ts STUDIO_W x STUDIO_H, pinned by the test). */
export const ROOM_W = 320;
export const ROOM_H = 160;

/** The only scales the room is ever drawn at, largest first.
 * yap-1107-parity-home: the approved Home page (section 5, 2026-11-06) draws the
 * layered studio at EXACTLY 3x (960 x 480) edge to edge in the main column, so
 * 3x is the ceiling — a wide window shows more wood beside the room, never a
 * 4x room. A column narrower than 960 px steps down to 2x (whole numbers only). */
export const ROOM_SCALES = [3, 2] as const;
export type RoomScale = (typeof ROOM_SCALES)[number];

/**
 * The largest whole-number scale whose room still fits `columnWidth` CSS px.
 * A column narrower than 2x (640 px) still draws at 2x and the frame crops the
 * sides: shrinking to 1x would make the room a stamp, and a fraction is never
 * allowed. Missing / non-finite widths (a detached node, a test) read as 2x.
 */
export function roomScale(columnWidth: number | null | undefined): RoomScale {
  const w = typeof columnWidth === "number" && Number.isFinite(columnWidth) ? columnWidth : 0;
  for (const s of ROOM_SCALES) if (ROOM_W * s <= w) return s;
  return ROOM_SCALES[ROOM_SCALES.length - 1];
}

/** The stage box for a column: full column width, the room's height at `scale`. */
export function roomBox(columnWidth: number | null | undefined): { scale: RoomScale; width: number; height: number } {
  const scale = roomScale(columnWidth);
  return { scale, width: ROOM_W * scale, height: ROOM_H * scale };
}

/** Home's definite end: this many takes, then the "See all" row. */
export const HOME_RECENT = 5;
/** History's page size (Prev / Next, never infinite). */
export const HISTORY_PAGE = 50;

/** The takes Home lists under the latest one. Never more than HOME_RECENT. */
export function recentTakes<T>(history: readonly T[]): T[] {
  return history.slice(0, HOME_RECENT);
}

/** The "Needs attention" box shows only when something needs attention. */
export function showNeedsAttention(failed: readonly unknown[] | null | undefined): boolean {
  return Array.isArray(failed) && failed.length > 0;
}

/** A cancelled take is parked with this kind of reason; anything else failed. */
export function attentionTitle(error: string | null | undefined): string {
  return /cancel/i.test(error ?? "") ? "Cancelled take" : "Could not transcribe";
}

const fmt = (n: number) => Math.max(0, Math.round(n)).toLocaleString("en-US");

/**
 * The quiet stats line under the room. Only REAL numbers: a value that has not
 * loaded is left out, never shown as 0.
 */
export function statsLine(o: { wordsToday?: number | null; streakDays?: number | null; totalTakes?: number | null }): string[] {
  const parts: string[] = [];
  if (typeof o.wordsToday === "number") parts.push(`${fmt(o.wordsToday)} words today`);
  if (typeof o.streakDays === "number") parts.push(`${fmt(o.streakDays)} day streak`);
  if (typeof o.totalTakes === "number") parts.push(`${fmt(o.totalTakes)} ${o.totalTakes === 1 ? "take" : "takes"}`);
  return parts;
}

/** One figure of the stats line: the number (bold, mono 20 px on the approved
 * page) and its label ("words today"). */
export interface StatPart {
  key: "words" | "streak" | "takes";
  value: string;
  label: string;
}

/**
 * yap-1107-parity-home — the stats line as the approved page draws it:
 * `<b>2,632</b>words today  <b>17</b>day streak  <b>288</b>takes`. Same rule as
 * statsLine: a value that has not loaded is left out, never shown as 0.
 */
export function statsParts(o: { wordsToday?: number | null; streakDays?: number | null; totalTakes?: number | null }): StatPart[] {
  const parts: StatPart[] = [];
  if (typeof o.wordsToday === "number") parts.push({ key: "words", value: fmt(o.wordsToday), label: "words today" });
  if (typeof o.streakDays === "number") parts.push({ key: "streak", value: fmt(o.streakDays), label: "day streak" });
  if (typeof o.totalTakes === "number")
    parts.push({ key: "takes", value: fmt(o.totalTakes), label: o.totalTakes === 1 ? "take" : "takes" });
  return parts;
}

/** mm:ss under an hour, h:mm:ss after — the meeting clock in the room label. */
export function meetingClock(elapsedSeconds: number | null | undefined): string {
  const s = Math.max(0, Math.floor(typeof elapsedSeconds === "number" && Number.isFinite(elapsedSeconds) ? elapsedSeconds : 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  const p = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${p(m)}:${p(r)}` : `${p(m)}:${p(r)}`;
}

/** The room label while a meeting records; null when no meeting is running. */
export function meetingRoomLabel(m: { recording?: boolean; elapsedSeconds?: number } | null | undefined): string | null {
  if (!m || !m.recording) return null;
  return `Recording meeting · ${meetingClock(m.elapsedSeconds)}`;
}

/** The row that ends Home's list. */
export function seeAllLabel(total: number | null | undefined, query: string): string {
  if (query) return "See every match in History";
  return typeof total === "number" && total > 0 ? `See all ${fmt(total)} →` : "See all →";
}

/** "Takes 51–100" for a History page (1-based, inclusive). */
export function pageRangeLabel(pageIndex: number, rowsOnPage: number): string {
  if (rowsOnPage <= 0) return "No takes";
  const first = pageIndex * HISTORY_PAGE + 1;
  return `Takes ${first.toLocaleString("en-US")}–${(first + rowsOnPage - 1).toLocaleString("en-US")}`;
}

/** The one-line preview of a take in Home's list. */
export function takePreview(text: string, max = 90): string {
  const t = text.replace(/\s+/g, " ").trim();
  return t.length > max ? `${t.slice(0, max - 1).trimEnd()}…` : t;
}

// ---- Open → History handoff ---------------------------------------------
// "Open" on Home's latest take (or a recent row) shows that take in History.
// The id rides a one-shot slot rather than shell state: History reads it once
// on mount and clears it, so a later visit to History opens on page one.
let focusId: string | null = null;
export function requestHistoryFocus(id: string | null): void {
  focusId = id;
}
export function takeHistoryFocus(): string | null {
  const id = focusId;
  focusId = null;
  return id;
}
