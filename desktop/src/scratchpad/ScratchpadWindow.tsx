/**
 * DB-D — the Scratchpad window: a notes rail with search, tabs, and a plain
 * text (markdown-as-typed) editor. It lives in a NON-ACTIVATING panel
 * (scratchpad.rs), so it takes your keystrokes while the app you were in stays
 * active.
 *
 * Two contracts with the backend:
 *  - FOCUS: whenever the panel gains/loses key or the active note changes, it
 *    reports `scratchpad_focus {focused, noteId}`. That is the dictation
 *    target: a take that stops while a note is focused comes back here as
 *    `scratchpad-dictation` instead of being pasted into the app behind.
 *  - VERSIONS: typing autosaves (`save_note_draft`, no version row); a
 *    FINALIZE (`finalize_note`, one version row) happens on blur, tab switch,
 *    close, ⌘S, Esc and every dictated take — only when the note changed.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  closeTab,
  insertAtCaret,
  needsFinalize,
  openTab,
  pruneTabs,
  UNTITLED,
  type DictationPayload,
  type Note,
  type NoteVersion,
} from "./notesModel";

type Draft = { title: string; content: string };

function upsert(notes: Note[], n: Note): Note[] {
  const rest = notes.filter((x) => x.id !== n.id);
  return [n, ...rest].sort(
    (a, b) => Number(b.pinned) - Number(a.pinned) || b.modified.localeCompare(a.modified),
  );
}

export default function ScratchpadWindow() {
  const [notes, setNotes] = useState<Note[]>([]);
  const [results, setResults] = useState<Note[] | null>(null);
  const [query, setQuery] = useState("");
  const [tabs, setTabs] = useState<string[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  /** The open note's version history, `null` while the panel is closed. */
  const [versions, setVersions] = useState<NoteVersion[] | null>(null);
  const finalized = useRef<Record<string, Draft>>({});
  const draftsRef = useRef(drafts);
  draftsRef.current = drafts;
  const activeRef = useRef(active);
  activeRef.current = active;
  const textRef = useRef<HTMLTextAreaElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const saveTimer = useRef<number | undefined>(undefined);

  const setDraft = useCallback((id: string, d: Draft) => {
    const next = { ...draftsRef.current, [id]: d };
    draftsRef.current = next;
    setDrafts(next);
  }, []);

  /** One version row, only when the note changed since its last finalize. */
  const finalize = useCallback(async (id: string | null) => {
    if (!id) return;
    const d = draftsRef.current[id];
    if (!d || !needsFinalize(d, finalized.current[id])) return;
    window.clearTimeout(saveTimer.current);
    finalized.current[id] = { ...d };
    try {
      const n = await invoke<Note>("finalize_note", { id, title: d.title || UNTITLED, content: d.content });
      setNotes((ns) => upsert(ns, n));
    } catch (e) {
      delete finalized.current[id];
      setError(String(e));
    }
  }, []);

  const newNote = useCallback(async () => {
    await finalize(activeRef.current);
    try {
      const n = await invoke<Note>("save_note_draft", { id: null, title: UNTITLED, content: "" });
      setDraft(n.id, { title: n.title, content: n.content });
      setNotes((ns) => upsert(ns, n));
      setTabs((t) => openTab(t, n.id));
      setActive(n.id);
      requestAnimationFrame(() => textRef.current?.focus());
    } catch (e) {
      setError(String(e));
    }
  }, [finalize, setDraft]);

  const refresh = useCallback(async () => {
    try {
      const list = await invoke<Note[]>("list_notes");
      setNotes(list);
      const next = { ...draftsRef.current };
      for (const n of list) {
        const local = next[n.id];
        const last = finalized.current[n.id];
        // Never clobber unsaved local typing; otherwise take the stored copy
        // (the main window may have edited it).
        if (!local || !last || !needsFinalize(local, last)) {
          next[n.id] = { title: n.title, content: n.content };
          finalized.current[n.id] = { title: n.title, content: n.content };
        }
      }
      draftsRef.current = next;
      setDrafts(next);
      setError(null);
      if (list.length === 0) {
        // An open panel always has a note to dictate into.
        await newNote();
        return;
      }
      setTabs((t) => {
        const kept = pruneTabs(t, list);
        return kept.length ? kept : [list[0].id];
      });
      setActive((a) => (a && list.some((n) => n.id === a) ? a : list[0].id));
    } catch (e) {
      setError(String(e));
    }
  }, [newNote]);

  // Load, and re-read every time the panel is shown.
  useEffect(() => {
    void refresh();
    const un = listen("scratchpad-shown", () => {
      void refresh();
      requestAnimationFrame(() => textRef.current?.focus());
    });
    return () => {
      void un.then((f) => f());
    };
  }, [refresh]);

  // FOCUS contract — the dictation target.
  useEffect(() => {
    const report = () =>
      void invoke("scratchpad_focus", {
        focused: document.hasFocus() && !document.hidden,
        noteId: activeRef.current,
      }).catch(() => {});
    const onBlur = () => {
      report();
      void finalize(activeRef.current);
    };
    window.addEventListener("focus", report);
    window.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", report);
    report();
    return () => {
      window.removeEventListener("focus", report);
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", report);
    };
  }, [finalize]);
  useEffect(() => {
    void invoke("scratchpad_focus", { focused: document.hasFocus(), noteId: active }).catch(() => {});
  }, [active]);

  // A dictated take for one of our notes: in at the caret, then finalize.
  useEffect(() => {
    const un = listen<DictationPayload>("scratchpad-dictation", (e) => {
      const { noteId, text } = e.payload;
      const cur = draftsRef.current[noteId];
      if (!cur) return;
      const ta = textRef.current;
      const here = activeRef.current === noteId && ta != null;
      const [s, t] = here ? [ta.selectionStart, ta.selectionEnd] : [cur.content.length, cur.content.length];
      const r = insertAtCaret(cur.content, s, t, text);
      setDraft(noteId, { ...cur, content: r.content });
      void finalize(noteId);
      if (here) {
        requestAnimationFrame(() => {
          ta.focus();
          ta.setSelectionRange(r.caret, r.caret);
        });
      }
    });
    return () => {
      void un.then((f) => f());
    };
  }, [finalize, setDraft]);

  // Search over notes_fts.
  useEffect(() => {
    if (query.trim() === "") {
      setResults(null);
      return;
    }
    const h = window.setTimeout(() => {
      invoke<Note[]>("search_notes", { query })
        .then(setResults)
        .catch((e) => setError(String(e)));
    }, 150);
    return () => window.clearTimeout(h);
  }, [query, notes]);

  function edit(id: string, patch: Partial<Draft>) {
    const cur = draftsRef.current[id] ?? { title: UNTITLED, content: "" };
    setDraft(id, { ...cur, ...patch });
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      const d = draftsRef.current[id];
      if (!d) return;
      invoke<Note>("save_note_draft", { id, title: d.title || UNTITLED, content: d.content })
        .then((n) => setNotes((ns) => upsert(ns, n)))
        .catch((e) => setError(String(e)));
    }, 600);
  }

  async function toggleHistory(id: string) {
    if (versions) {
      setVersions(null);
      return;
    }
    await finalize(id);
    try {
      setVersions(await invoke<NoteVersion[]>("list_note_versions", { id }));
    } catch (e) {
      setError(String(e));
    }
  }

  /** Restoring is an edit: it becomes the next version on the next finalize. */
  function restore(id: string, v: NoteVersion) {
    edit(id, { title: v.title, content: v.content });
    setVersions(null);
    requestAnimationFrame(() => textRef.current?.focus());
  }

  async function select(id: string) {
    if (id !== activeRef.current) await finalize(activeRef.current);
    setVersions(null);
    setConfirmDelete(false);
    setTabs((t) => openTab(t, id));
    setActive(id);
  }

  async function close(id: string) {
    await finalize(id);
    const r = closeTab(tabs, id, active);
    setTabs(r.tabs);
    setActive(r.active);
  }

  async function hide() {
    await finalize(activeRef.current);
    await invoke("hide_scratchpad").catch(() => {});
  }

  async function togglePin(n: Note) {
    try {
      await invoke("set_note_pinned", { id: n.id, pinned: !n.pinned });
      setNotes((ns) => upsert(ns, { ...n, pinned: !n.pinned }));
    } catch (e) {
      setError(String(e));
    }
  }

  async function remove(id: string) {
    if (!confirmDelete) {
      setConfirmDelete(true);
      return;
    }
    setConfirmDelete(false);
    try {
      await invoke("delete_note", { id });
      delete finalized.current[id];
      const { [id]: _gone, ...rest } = draftsRef.current;
      draftsRef.current = rest;
      setDrafts(rest);
      const r = closeTab(tabs, id, active);
      setTabs(r.tabs);
      setActive(r.active);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  function onKeyDown(e: React.KeyboardEvent) {
    const mod = e.metaKey || e.ctrlKey;
    if (e.key === "Escape") {
      e.preventDefault();
      void hide();
    } else if (mod && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void finalize(activeRef.current);
    } else if (mod && e.key.toLowerCase() === "n") {
      e.preventDefault();
      void newNote();
    } else if (mod && e.key.toLowerCase() === "w" && activeRef.current) {
      e.preventDefault();
      void close(activeRef.current);
    } else if (mod && e.key.toLowerCase() === "f") {
      e.preventDefault();
      searchRef.current?.focus();
    }
  }

  const byId = useMemo(() => new Map(notes.map((n) => [n.id, n])), [notes]);
  const rail = results ?? notes;
  const activeNote = active ? byId.get(active) : undefined;
  const draft = active ? drafts[active] : undefined;

  return (
    <div className="sp" onKeyDown={onKeyDown}>
      <aside className="sp-rail" aria-label="Notes">
        <input
          ref={searchRef}
          className="sp-search"
          type="search"
          placeholder="Search notes"
          aria-label="Search notes"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button type="button" className="sp-new" onClick={() => void newNote()}>
          New note
        </button>
        <ul className="sp-list">
          {rail.map((n) => (
            <li key={n.id}>
              <button
                type="button"
                className={n.id === active ? "sp-item on" : "sp-item"}
                aria-current={n.id === active ? "true" : undefined}
                onClick={() => void select(n.id)}
              >
                <span className="sp-item-title">
                  {n.pinned && <span className="sp-pin" aria-label="Pinned">●</span>}
                  {(drafts[n.id]?.title ?? n.title) || UNTITLED}
                </span>
                <span className="sp-item-preview">{n.preview || "Empty note"}</span>
              </button>
            </li>
          ))}
        </ul>
        {rail.length === 0 && (
          <p className="sp-hint">{query ? `No notes match “${query}”.` : "No notes yet."}</p>
        )}
      </aside>
      <main className="sp-main">
        <div className="sp-tabs" role="tablist" aria-label="Open notes">
          {tabs.map((id) => (
            <div key={id} className={id === active ? "sp-tab on" : "sp-tab"} role="tab" aria-selected={id === active}>
              <button type="button" className="sp-tab-name" onClick={() => void select(id)}>
                {(drafts[id]?.title ?? byId.get(id)?.title) || UNTITLED}
              </button>
              <button type="button" className="sp-tab-close" aria-label="Close tab" onClick={() => void close(id)}>
                ×
              </button>
            </div>
          ))}
        </div>
        {active && draft ? (
          <>
            <div className="sp-head">
              <input
                className="sp-title"
                aria-label="Note title"
                value={draft.title}
                onChange={(e) => edit(active, { title: e.target.value })}
              />
              {activeNote && (
                <button type="button" className="sp-btn" onClick={() => void togglePin(activeNote)}>
                  {activeNote.pinned ? "Unpin" : "Pin"}
                </button>
              )}
              <button
                type="button"
                className="sp-btn"
                aria-expanded={versions != null}
                onClick={() => void toggleHistory(active)}
              >
                History
              </button>
              <button
                type="button"
                className={confirmDelete ? "sp-btn danger" : "sp-btn"}
                onClick={() => void remove(active)}
                onBlur={() => setConfirmDelete(false)}
              >
                {confirmDelete ? "Confirm delete" : "Delete"}
              </button>
            </div>
            {versions && (
              <ol className="sp-versions" aria-label="Versions, newest first">
                {versions.length === 0 && <li className="sp-hint">No versions yet.</li>}
                {versions.map((v) => (
                  <li key={v.id}>
                    <time dateTime={v.created}>{new Date(v.created).toLocaleString()}</time>
                    <span className="sp-item-preview">{v.content.split("\n")[0] || "Empty"}</span>
                    <button type="button" className="sp-btn" onClick={() => restore(active, v)}>
                      Restore
                    </button>
                  </li>
                ))}
              </ol>
            )}
            <textarea
              ref={textRef}
              className="sp-text"
              aria-label="Note"
              value={draft.content}
              placeholder="Type, or hold your dictation key — the words land here, not in the app behind."
              onChange={(e) => edit(active, { content: e.target.value })}
            />
          </>
        ) : (
          <div className="sp-blank">
            <button type="button" className="sp-new" onClick={() => void newNote()}>
              New note
            </button>
          </div>
        )}
        <footer className="sp-foot">
          Plain text and markdown · Esc hides · ⌘N new · ⌘W close tab · ⌘F search
        </footer>
        {error && (
          <p className="sp-error" role="alert">
            {error}
          </p>
        )}
      </main>
    </div>
  );
}
