import { invoke } from "@tauri-apps/api/core";
import { type ScratchNote, formatTime } from "../appTypes";
import { useAppCtx } from "../appShell";

export default function Scratchpad() {
  const {
    activeNoteId, copyText, noteBody, noteTitle, pasteText, scratch,
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
  return (
            <div className="scratch">
              <div className="scratch-list">
                <button className="primary" onClick={newNote}>
                  New note
                </button>
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
            </div>
  );
}
