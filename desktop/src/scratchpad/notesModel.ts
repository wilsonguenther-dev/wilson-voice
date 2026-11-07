/**
 * DB-D — the Scratchpad window's pure state: tabs, caret insertion of a
 * dictated take, and "does this note need a finalize". No React, no IPC, so
 * every rule here is a vitest.
 */

/** One note as the backend returns it (`db::Note`, camelCase). */
export interface Note {
  id: string;
  title: string;
  content: string;
  preview: string;
  pinned: boolean;
  created: string;
  modified: string;
}

/** One finalized state (`db::NoteVersion`), newest first from the backend. */
export interface NoteVersion {
  id: number;
  noteId: string;
  title: string;
  content: string;
  created: string;
}

/** `scratchpad-dictation` payload (`scratchpad::DictationPayload`). */
export interface DictationPayload {
  noteId: string;
  text: string;
}

export const UNTITLED = "Untitled";

/**
 * Insert a dictated take at the caret (replacing a selection), with one space
 * between it and a word it would otherwise run into. Returns the new content
 * and where the caret goes (just after the inserted text).
 */
export function insertAtCaret(
  content: string,
  selStart: number,
  selEnd: number,
  text: string,
): { content: string; caret: number } {
  const start = Math.max(0, Math.min(selStart, content.length));
  const end = Math.max(start, Math.min(selEnd, content.length));
  const before = content.slice(0, start);
  const after = content.slice(end);
  let insert = text.trim();
  if (insert === "") return { content, caret: end };
  if (before !== "" && !/\s$/.test(before)) insert = " " + insert;
  if (after !== "" && !/^\s/.test(after)) insert = insert + " ";
  return { content: before + insert + after, caret: before.length + insert.length };
}

/** Open `id` as a tab (no duplicates) and make it active. */
export function openTab(tabs: string[], id: string): string[] {
  return tabs.includes(id) ? tabs : [...tabs, id];
}

/**
 * Close `id`. The next active tab is the one that took its place (or the new
 * last tab); `null` when none are left.
 */
export function closeTab(
  tabs: string[],
  id: string,
  active: string | null,
): { tabs: string[]; active: string | null } {
  const i = tabs.indexOf(id);
  if (i < 0) return { tabs, active };
  const next = tabs.filter((t) => t !== id);
  if (active !== id) return { tabs: next, active };
  return { tabs: next, active: next[Math.min(i, next.length - 1)] ?? null };
}

/** Drop tabs whose note no longer exists (deleted from the main window). */
export function pruneTabs(tabs: string[], notes: Pick<Note, "id">[]): string[] {
  const ids = new Set(notes.map((n) => n.id));
  return tabs.filter((t) => ids.has(t));
}

/**
 * fix-b5-1 — which tabs are open and which note is active after a re-read of
 * the list. `openId` is the note the window was opened FOR (the pill's
 * timestamped meeting note): when it is in the list it gets a tab and becomes
 * active, even over an active or pinned note. Without one, the active note is
 * kept (or the list's first note when it was deleted). `list` is non-empty.
 */
export function landOn(
  tabs: string[],
  active: string | null,
  list: Pick<Note, "id">[],
  openId: string | null | undefined,
): { tabs: string[]; active: string | null } {
  if (list.length === 0) return { tabs: [], active: null };
  const kept = pruneTabs(tabs, list);
  if (openId && list.some((n) => n.id === openId)) {
    return { tabs: openTab(kept, openId), active: openId };
  }
  const still = active && list.some((n) => n.id === active) ? active : list[0].id;
  return { tabs: kept.length ? kept : [list[0].id], active: still };
}

/**
 * fix-b5-1 — where the caret goes once the note the window was opened FOR is
 * on screen: the END of its content (just after the "[12:34] " seed), so the
 * next keystroke continues the timestamp line. `null` while that note is not
 * the active one yet (nothing to place).
 */
export function openedCaret(
  pending: string | null,
  active: string | null,
  content: string | undefined,
): number | null {
  if (!pending || pending !== active || content === undefined) return null;
  return content.length;
}

/**
 * A finalize writes a version row, so it only runs when the note differs from
 * what was last finalized — blur / tab switch / close on an untouched note
 * write nothing.
 */
export function needsFinalize(
  draft: { title: string; content: string },
  lastFinalized: { title: string; content: string } | undefined,
): boolean {
  if (!lastFinalized) return draft.content !== "" || (draft.title !== "" && draft.title !== UNTITLED);
  return draft.title !== lastFinalized.title || draft.content !== lastFinalized.content;
}
