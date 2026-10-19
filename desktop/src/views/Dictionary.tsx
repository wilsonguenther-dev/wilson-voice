import { useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { type DictEntry, type DictCandidate } from "../appTypes";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { ErrorState, LoadingState, WordGlyph } from "../ViewStates";

/*
 * yap-uiux-2026-10-16-DICT — walk rows D1 + D2.
 *
 * Layout, top to bottom: the controls (add a word, then search), the
 * suggestions Yap noticed, the word list in its OWN scroll region, and a
 * footer pinned under it. The view fills the content pane and never grows the
 * page: a long dictionary scrolls inside the list, the add form and the footer
 * stay put. Prose comes last and is one plain line; how stars work lives on the
 * star's tooltip, not above the form (D1). The empty dictionary has no second
 * add button: the form above it is the only way to add a word (D2).
 */
export const DICT_HELP = "Yap will spell these your way.";
export const STAR_ON_TIP = "Starred: Yap listens for this word before you say it. Click to unstar.";
export const STAR_OFF_TIP = "Star this word and Yap listens for it before you say it.";

/** Case-insensitive match on the word or its preferred spelling. */
export function filterDictionary(list: DictEntry[], query: string): DictEntry[] {
  const q = query.trim().toLowerCase();
  if (!q) return list;
  return list.filter(
    (d) => d.term.toLowerCase().includes(q) || (d.preferred ?? "").toLowerCase().includes(q),
  );
}

/** The footer's one line: how many words, how many starred, how many shown. */
export function dictionarySummary(list: DictEntry[], shown: number, query: string): string {
  const n = list.length;
  const stars = list.filter((d) => d.starred).length;
  const words = `${n} ${n === 1 ? "word" : "words"}`;
  const base = stars ? `${words} · ${stars} starred` : words;
  return query.trim() ? `Showing ${shown} of ${base}` : base;
}

const fill: CSSProperties = { display: "flex", flexDirection: "column", gap: 12, height: "100%", minHeight: 0 };
const listRegion: CSSProperties = { flex: "1 1 auto", minHeight: 96, overflowY: "auto", overscrollBehavior: "contain" };
const footer: CSSProperties = {
  flexShrink: 0, display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12,
  borderTop: "1px solid var(--border)", paddingTop: 10,
};
const search: CSSProperties = {
  width: "100%", marginTop: 10, background: "var(--bg-card)", border: "1px solid var(--border)",
  borderRadius: 10, color: "var(--text)", padding: "0.6rem 0.75rem", outline: "none", boxSizing: "border-box",
};

export default function Dictionary() {
  const {
    bootError, bootErrors, booting, candidates, retryBoot, dictionary, editPreferred, editTerm, editingTermId, newPreferred,
    refreshAll,
    newTerm, setCandidates, setDictionary, setEditPreferred, setEditTerm, setEditingTermId,
    setNewPreferred, setNewTerm, settings, toast,
  } = useAppCtx();
  const [dictQuery, setDictQuery] = useState("");
  async function addTerm() {
    if (!newTerm.trim()) return;
    try {
      await invoke("add_dictionary_term", {
        term: newTerm.trim(),
        preferred: newPreferred.trim() || null,
      });
      setNewTerm("");
      setNewPreferred("");
      setDictionary(await invoke("list_dictionary"));
      toast("Dictionary term added");
    } catch (e) {
      toast(String(e));
    }
  }
  async function removeTerm(id: string) {
    try {
      await invoke("delete_dictionary_term", { id });
      setDictionary((d) => d.filter((x) => x.id !== id));
    } catch (e) {
      toast(String(e));
    }
  }
  async function toggleStar(d: DictEntry) {
    try {
      await invoke("set_dictionary_starred", {
        id: d.id,
        starred: !d.starred,
      });
      setDictionary(await invoke("list_dictionary"));
    } catch (e) {
      toast(String(e));
    }
  }
  function startEditTerm(d: DictEntry) {
    setEditingTermId(d.id);
    setEditTerm(d.term);
    setEditPreferred(d.preferred ?? "");
  }
  async function saveEditTerm() {
    if (!editingTermId || !editTerm.trim()) return;
    try {
      await invoke("update_dictionary_term", {
        id: editingTermId,
        term: editTerm.trim(),
        preferred: editPreferred.trim() || null,
      });
      setEditingTermId(null);
      setDictionary(await invoke("list_dictionary"));
      toast("Dictionary term updated");
    } catch (e) {
      toast(String(e));
    }
  }
  async function acceptCandidate(c: DictCandidate) {
    try {
      await invoke("promote_dict_candidate", { id: c.id });
      const [dict, cands] = await Promise.all([
        invoke<DictEntry[]>("list_dictionary"),
        invoke<DictCandidate[]>("list_dict_candidates"),
      ]);
      setDictionary(dict);
      setCandidates(cands);
      toast(`Added ${c.term} to your dictionary`);
    } catch (e) {
      toast(String(e));
    }
  }
  async function dismissCandidate(c: DictCandidate) {
    try {
      await invoke("dismiss_dict_candidate", { id: c.id });
      setCandidates((cs) => cs.filter((x) => x.id !== c.id));
    } catch (e) {
      toast(String(e));
    }
  }
  // Y5-B fix round (#192 BLOCKING) - bootError is shared by refreshAll's 11
  // calls plus loadHistory, so it only replaces THIS view when this view's own
  // data never landed. A later refresh that fails (model-ready edge,
  // silent_capture) keeps already-loaded data on screen; the bridge message
  // is in status, the same as on main.
  // This list starts as [] so it cannot say "never loaded"; settings lands in
  // the same refreshAll batch (all setters run together), so null settings is
  // that signal.
  // DO 3 — this view's own commands first; settings-null is the old signal.
  const dictError =
    bootErrors?.list_dictionary ?? bootErrors?.list_dict_candidates ?? (settings == null ? bootError : null);
  const state = viewState(dictionary, booting, dictError);
  if (state === "error")
    return (
      <ErrorState
        data-error-state="dictionary"
        view="dictionary"
        error={dictError}
        actionLabel="Open the dictionary again"
        onAction={() => void (settings == null ? refreshAll() : retryBoot(["list_dictionary", "list_dict_candidates"]))}
      />
    );
  if (state === "loading")
    return <LoadingState data-loading-state="dictionary" noun="dictionary" onRetry={() => void refreshAll()} />;
  const shown = filterDictionary(dictionary, dictQuery);
  return (
    <div className="dict" style={fill}>
      <div className="panel intro" style={{ flexShrink: 0 }}>
        <h3>Your words</h3>
        <div className="dict-add">
          <input
            placeholder="Word (e.g. Drivia)"
            aria-label="Word"
            value={newTerm}
            onChange={(e) => setNewTerm(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && addTerm()}
          />
          <input
            placeholder="Spell it as (optional)"
            aria-label="Spell it as"
            value={newPreferred}
            onChange={(e) => setNewPreferred(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && addTerm()}
          />
          <button className="primary" onClick={addTerm}>
            Add
          </button>
        </div>
        {dictionary.length > 0 && (
          <input
            type="search"
            data-dict-search
            placeholder="Search your words"
            aria-label="Search your words"
            value={dictQuery}
            onChange={(e) => setDictQuery(e.target.value)}
            style={search}
          />
        )}
        <p className="tiny muted" style={{ margin: "10px 0 0" }}>{DICT_HELP}</p>
      </div>

      {candidates.length > 0 && (
        <div className="panel dict-suggest" style={{ flexShrink: 0 }}>
          <h3>
            Yap noticed:{" "}
            {candidates
              .slice(0, 3)
              .map((c) => c.term)
              .join(", ")}
          </h3>
          <p className="tiny">Words you fixed by hand. Add them and Yap stops getting them wrong.</p>
          <ul className="cand-list" style={{ maxHeight: 180, overflowY: "auto" }}>
            {candidates.map((c) => (
              <li key={c.id}>
                <div>
                  <strong>{c.term}</strong>
                  {c.wrong && <span className="muted"> ← heard “{c.wrong}”</span>}
                  <div className="tiny">fixed {c.useCount}×</div>
                </div>
                <div className="cand-actions">
                  <button className="primary" onClick={() => acceptCandidate(c)}>
                    Add
                  </button>
                  <button className="ghost" onClick={() => dismissCandidate(c)}>
                    Dismiss
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </div>
      )}

      {dictionary.length === 0 && candidates.length === 0 && (
        <div className="view-state view-empty" data-empty-state="dictionary">
          <div className="view-state-glyph">
            <WordGlyph />
          </div>
          <h3>No words yet</h3>
          <p>Names, jargon and spellings Yap keeps getting wrong. Type one above and press Add.</p>
        </div>
      )}

      {dictionary.length > 0 && (
        <>
          <div className="dict-scroll" data-dict-list style={listRegion}>
            {shown.length === 0 ? (
              <p className="muted" data-empty-state="dictionary-search">
                No words match “{dictQuery.trim()}”.
              </p>
            ) : (
              <ul className="dict-list">
                {shown.map((d) => (
                  <li key={d.id}>
                    {editingTermId === d.id ? (
                      <div className="dict-edit">
                        <input
                          value={editTerm}
                          onChange={(e) => setEditTerm(e.target.value)}
                          onKeyDown={(e) => e.key === "Enter" && saveEditTerm()}
                          aria-label="Word"
                          autoFocus
                        />
                        <input
                          value={editPreferred}
                          placeholder="Spell it as (optional)"
                          onChange={(e) => setEditPreferred(e.target.value)}
                          onKeyDown={(e) => e.key === "Enter" && saveEditTerm()}
                          aria-label="Spell it as"
                        />
                        <button className="primary" onClick={saveEditTerm}>
                          Save
                        </button>
                        <button className="ghost" onClick={() => setEditingTermId(null)}>
                          Cancel
                        </button>
                      </div>
                    ) : (
                      <>
                        <div>
                          <strong>{d.term}</strong>
                          {d.preferred && <span className="muted"> → {d.preferred}</span>}
                          <div className="tiny">
                            used {d.hits}×{d.starred ? " · starred" : ""}
                          </div>
                        </div>
                        <div className="dict-actions">
                          <button
                            className={d.starred ? "star on" : "star"}
                            onClick={() => toggleStar(d)}
                            aria-pressed={!!d.starred}
                            aria-label={d.starred ? `Unstar ${d.term}` : `Star ${d.term}`}
                            title={d.starred ? STAR_ON_TIP : STAR_OFF_TIP}
                          >
                            {d.starred ? "★" : "☆"}
                          </button>
                          <button className="ghost" onClick={() => startEditTerm(d)}>
                            Edit
                          </button>
                          <button className="ghost danger" onClick={() => removeTerm(d.id)}>
                            Remove
                          </button>
                        </div>
                      </>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>
          <div className="dict-footer" data-dict-footer style={footer}>
            <span className="tiny muted">{dictionarySummary(dictionary, shown.length, dictQuery)}</span>
            {dictQuery.trim() && (
              <button className="ghost" onClick={() => setDictQuery("")}>
                Clear search
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
}
