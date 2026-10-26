import { useEffect, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { type ScratchNote, formatTime } from "../appTypes";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { EmptyState, ErrorState, LoadingState, PageGlyph } from "../ViewStates";

/**
 * yap-uiux-2026-10-16-S1 — the note the editor should open on its own, or null.
 *
 * Walk row S1 (daily): one note was listed, but the editor opened on a blank
 * "Scratchpad" title. When nothing is open and the user has not asked for a
 * blank editor (New note, Home's send-to-scratchpad, a draft in progress), the
 * editor opens the most recently updated note. Ties keep list order.
 */
export function noteToOpen(
  scratch: ScratchNote[],
  activeNoteId: string | null,
  composing: boolean,
): ScratchNote | null {
  if (composing || activeNoteId != null || scratch.length === 0) return null;
  let latest = scratch[0];
  for (const n of scratch) if (Date.parse(n.updatedAt) > Date.parse(latest.updatedAt)) latest = n;
  return latest;
}

/* yap-uiux-2026-10-16-SCRATCH — the editor surface is CONTAINED: the view fills
   the content region and never grows past it, the note list scrolls inside its
   own column, and Save / Copy / Paste / Delete sit in a footer pinned under the
   text. These are inline on purpose: the item is scoped to this file, and none
   of them touches grid-template-columns, so App.css's narrow-window rule
   (single column below the breakpoint) still applies. */
const FILL: CSSProperties = { height: "100%", minHeight: 0 };
const LIST: CSSProperties = { minHeight: 0 };
const NOTES: CSSProperties = { display: "flex", flexDirection: "column", gap: 6, flex: 1, minHeight: 0, overflowY: "auto" };
const EDITOR: CSSProperties = { minHeight: 0 };
const TEXT: CSSProperties = { minHeight: 120, resize: "none" };
const FOOTER: CSSProperties = { flexShrink: 0, paddingTop: 10, borderTop: "1px solid var(--border)" };

export default function Scratchpad() {
  const {
    activeNoteId, bootError, bootErrors, booting, retryBoot, copyText, noteBody, noteTitle, pasteText, refreshAll, scratch,
    scratchComposing, setActiveNoteId, setNoteBody, setNoteTitle, setScratch, setScratchComposing, settings, toast,
  } = useAppCtx();
  async function saveNote() {
    try {
      const note = await invoke<ScratchNote>("save_scratch", {
        id: activeNoteId,
        title: noteTitle || "Note",
        body: noteBody,
      });
      setActiveNoteId(note.id);
      setScratch(await invoke("list_scratch"));
      toast("Scratchpad saved");
    } catch (e) {
      toast(String(e));
    }
  }
  function openNote(n: ScratchNote) {
    setScratchComposing(true);
    setActiveNoteId(n.id);
    setNoteTitle(n.title);
    setNoteBody(n.body);
  }
  async function newNote() {
    // Y5-B review fix — without this flag the state below is exactly the
    // empty-state condition, so "New note" re-rendered the empty state.
    setScratchComposing(true);
    setActiveNoteId(null);
    setNoteTitle("New note");
    setNoteBody("");
  }
  async function deleteNote(id: string) {
    try {
      await invoke("delete_scratch", { id });
      if (activeNoteId === id) {
        // S1 — deleting the open note hands the editor back to the
        // most-recent-note rule below (or to the empty state when this was the
        // last note), instead of stranding a blank "New note" over a list.
        setScratchComposing(false);
        setActiveNoteId(null);
        setNoteTitle("");
        setNoteBody("");
      }
      setScratch(await invoke("list_scratch"));
    } catch (e) {
      toast(String(e));
    }
  }
  // Y5-B — Scratchpad's SHAPE is provisional: DB-D gives it a real second
  // window and versions, which will change what "empty" and "loading" mean
  // here. These three states are deliberately thin so DB-D replaces them
  // rather than fighting them.
  // Y5-B fix round (#192 BLOCKING) - bootError is shared by refreshAll's 11
  // calls plus loadHistory, so it only replaces THIS view when this view's own
  // data never landed. A later refresh that fails (model-ready edge,
  // silent_capture) keeps already-loaded data on screen; the bridge message
  // is in status, the same as on main.
  // This list starts as [] so it cannot say "never loaded"; settings lands in
  // the same refreshAll batch (all setters run together), so null settings is
  // that signal.
  const scratchError = bootErrors?.list_scratch ?? (settings == null ? bootError : null);
  const state = viewState(scratch, booting, scratchError);
  // S1 — open the most recent note when one exists and nothing else is open.
  const autoOpen = state === "ready" ? noteToOpen(scratch, activeNoteId, scratchComposing) : null;
  useEffect(() => {
    if (autoOpen) openNote(autoOpen);
    // openNote only calls stable state setters, so autoOpen is the only input.
  }, [autoOpen]);
  if (state === "error")
    return (
      <ErrorState
        data-error-state="scratchpad"
        view="scratchpad"
        error={scratchError}
        actionLabel="Open your notes again"
        onAction={() => void (settings == null ? refreshAll() : retryBoot(["list_scratch"]))}
      />
    );
  if (state === "loading")
    return <LoadingState data-loading-state="scratchpad" noun="notes" expected={scratch.length || null} onRetry={() => void refreshAll()} />;
  // S2 — first run: ONE "New note" (the empty state's), no placeholder copy.
  if (scratch.length === 0 && !scratchComposing && !noteBody && !activeNoteId)
    return (
      <EmptyState
        data-empty-state="scratchpad"
        glyph={<PageGlyph />}
        title="Nothing parked here yet"
        body="Somewhere to hold text between apps: a draft prompt, a paragraph you are not ready to send, a transcript you want to edit before it goes anywhere."
        actionLabel="New note"
        onAction={newNote}
      />
    );
  return (
    <div className="scratch" style={FILL}>
      <div className="scratch-list" style={LIST}>
        <button className="primary" onClick={newNote}>
          New note
        </button>
        {scratch.length > 0 && (
          <div className="scratch-notes" style={NOTES} aria-label="Saved notes">
            {scratch.map((n) => (
              <button
                key={n.id}
                data-row="scratch-note"
                aria-current={activeNoteId === n.id ? "true" : undefined}
                className={activeNoteId === n.id ? "note-item active" : "note-item"}
                onClick={() => openNote(n)}
              >
                <strong>{n.title}</strong>
                <span className="tiny">{formatTime(n.updatedAt)}</span>
              </button>
            ))}
          </div>
        )}
      </div>
      <div className="scratch-editor" style={EDITOR}>
        <input
          className="note-title"
          aria-label="Note title"
          value={noteTitle}
          placeholder="Untitled note"
          onChange={(e) => setNoteTitle(e.target.value)}
        />
        <textarea
          style={TEXT}
          aria-label="Note text"
          value={noteBody}
          onChange={(e) => setNoteBody(e.target.value)}
          placeholder="Park text, draft prompts…"
        />
        <div className="actions" data-scratch-footer="" style={FOOTER}>
          <button className="primary" onClick={saveNote}>
            Save
          </button>
          <button onClick={() => copyText(noteBody)} disabled={!noteBody}>
            Copy
          </button>
          <button onClick={() => pasteText(noteBody)} disabled={!noteBody}>
            Paste
          </button>
          {activeNoteId && (
            <button className="ghost danger" onClick={() => deleteNote(activeNoteId)}>
              Delete
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
