import { invoke } from "@tauri-apps/api/core";
import { type ScratchNote, formatTime } from "../appTypes";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { EmptyState, ErrorState, LoadingState, PageGlyph } from "../ViewStates";

export default function Scratchpad() {
  const {
    activeNoteId, bootError, booting, copyText, noteBody, noteTitle, pasteText, refreshAll, scratch,
    setActiveNoteId, setNoteBody, setNoteTitle, setScratch, toast,
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
    setActiveNoteId(n.id);
    setNoteTitle(n.title);
    setNoteBody(n.body);
  }
  async function newNote() {
    setActiveNoteId(null);
    setNoteTitle("New note");
    setNoteBody("");
  }
  async function deleteNote(id: string) {
    try {
      await invoke("delete_scratch", { id });
      if (activeNoteId === id) newNote();
      setScratch(await invoke("list_scratch"));
    } catch (e) {
      toast(String(e));
    }
  }
  // Y5-B — Scratchpad's SHAPE is provisional: DB-D gives it a real second
  // window and versions, which will change what "empty" and "loading" mean
  // here. These three states are deliberately thin so DB-D replaces them
  // rather than fighting them.
  const state = viewState(scratch, booting, bootError);
  if (state === "error")
    return (
      <ErrorState
        data-error-state="scratchpad"
        view="scratchpad"
        error={bootError}
        actionLabel="Open your notes again"
        onAction={() => void refreshAll()}
      />
    );
  if (state === "loading")
    return <LoadingState data-loading-state="scratchpad" noun="notes" expected={scratch.length || null} onRetry={() => void refreshAll()} />;
  return (
            <div className="scratch">
              <div className="scratch-list">
                <button className="primary" onClick={newNote}>
                  New note
                </button>
                {scratch.length === 0 && (
                  <p className="tiny muted">
                    Saved notes list here.
                  </p>
                )}
                {scratch.map((n) => (
                  <button
                    key={n.id}
                    className={
                      activeNoteId === n.id ? "note-item active" : "note-item"
                    }
                    onClick={() => openNote(n)}
                  >
                    <strong>{n.title}</strong>
                    <span className="tiny">{formatTime(n.updatedAt)}</span>
                  </button>
                ))}
              </div>
              {scratch.length === 0 && !noteBody && !activeNoteId ? (
                <div className="scratch-editor">
                  <EmptyState
                    data-empty-state="scratchpad"
                    glyph={<PageGlyph />}
                    title="Nothing parked here yet"
                    body="Somewhere to hold text between apps: a draft prompt, a paragraph you are not ready to send, a transcript you want to edit before it goes anywhere."
                    actionLabel="New note"
                    onAction={newNote}
                  />
                </div>
              ) : (
              <div className="scratch-editor">
                <input
                  className="note-title"
                  value={noteTitle}
                  onChange={(e) => setNoteTitle(e.target.value)}
                />
                <textarea
                  value={noteBody}
                  onChange={(e) => setNoteBody(e.target.value)}
                  placeholder="Park text, draft prompts…"
                />
                <div className="actions">
                  <button className="primary" onClick={saveNote}>
                    Save
                  </button>
                  <button
                    onClick={() => copyText(noteBody)}
                    disabled={!noteBody}
                  >
                    Copy
                  </button>
                  <button
                    onClick={() => pasteText(noteBody)}
                    disabled={!noteBody}
                  >
                    Paste
                  </button>
                  {activeNoteId && (
                    <button
                      className="ghost danger"
                      onClick={() => deleteNote(activeNoteId)}
                    >
                      Delete
                    </button>
                  )}
                </div>
              </div>
              )}
            </div>
  );
}
