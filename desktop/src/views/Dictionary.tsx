import { invoke } from "@tauri-apps/api/core";
import { type DictEntry, type DictCandidate } from "../appTypes";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { EmptyState, ErrorState, LoadingState, WordGlyph } from "../ViewStates";

export default function Dictionary() {
  const {
    bootError, booting, candidates, dictionary, editPreferred, editTerm, editingTermId, newPreferred,
    refreshAll,
    newTerm, setCandidates, setDictionary, setEditPreferred, setEditTerm, setEditingTermId,
    setNewPreferred, setNewTerm, toast,
  } = useAppCtx();
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
  const state = viewState(dictionary, booting, bootError);
  if (state === "error")
    return (
      <ErrorState
        data-error-state="dictionary"
        view="dictionary"
        error={bootError}
        actionLabel="Open the dictionary again"
        onAction={() => void refreshAll()}
      />
    );
  if (state === "loading")
    return <LoadingState data-loading-state="dictionary" noun="dictionary" onRetry={() => void refreshAll()} />;
  return (
            <div className="dict">
              <div className="panel intro">
                <h3>Spell the way you do</h3>
                <p>
                  After ASR, matching tokens are rewritten to your preferred
                  form. Starred terms are always fed to the recognizer before it
                  decodes, so it expects them.
                </p>
                <div className="dict-add">
                  <input
                    placeholder="Term (e.g. Drivia)"
                    value={newTerm}
                    onChange={(e) => setNewTerm(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && addTerm()}
                  />
                  <input
                    placeholder="Preferred (optional)"
                    value={newPreferred}
                    onChange={(e) => setNewPreferred(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && addTerm()}
                  />
                  <button className="primary" onClick={addTerm}>
                    Add
                  </button>
                </div>
              </div>

              {candidates.length > 0 && (
                <div className="panel dict-suggest">
                  <h3>
                    Yap noticed:{" "}
                    {candidates
                      .slice(0, 3)
                      .map((c) => c.term)
                      .join(", ")}
                  </h3>
                  <p className="tiny">
                    Words you fixed by hand. Add them and Yap stops getting them
                    wrong.
                  </p>
                  <ul className="cand-list">
                    {candidates.map((c) => (
                      <li key={c.id}>
                        <div>
                          <strong>{c.term}</strong>
                          {c.wrong && (
                            <span className="muted"> ← heard “{c.wrong}”</span>
                          )}
                          <div className="tiny">
                            fixed {c.useCount}×
                          </div>
                        </div>
                        <div className="cand-actions">
                          <button
                            className="primary"
                            onClick={() => acceptCandidate(c)}
                          >
                            Add
                          </button>
                          <button
                            className="ghost"
                            onClick={() => dismissCandidate(c)}
                          >
                            Dismiss
                          </button>
                        </div>
                      </li>
                    ))}
                  </ul>
                </div>
              )}

              {dictionary.length === 0 && candidates.length === 0 && (
                <EmptyState
                  data-empty-state="dictionary"
                  glyph={<WordGlyph />}
                  title="No words taught yet"
                  body="Names, jargon and spellings Yap keeps getting wrong go here. Star one and the recognizer expects it before it even decodes."
                  actionLabel="Add a word"
                  onAction={() =>
                    document
                      .querySelector<HTMLInputElement>(".dict-add input")
                      ?.focus()
                  }
                />
              )}

              <ul className="dict-list">
                {dictionary.map((d) => (
                  <li key={d.id}>
                    {editingTermId === d.id ? (
                      <div className="dict-edit">
                        <input
                          value={editTerm}
                          onChange={(e) => setEditTerm(e.target.value)}
                          onKeyDown={(e) =>
                            e.key === "Enter" && saveEditTerm()
                          }
                          aria-label="Term"
                          autoFocus
                        />
                        <input
                          value={editPreferred}
                          placeholder="Preferred (optional)"
                          onChange={(e) => setEditPreferred(e.target.value)}
                          onKeyDown={(e) =>
                            e.key === "Enter" && saveEditTerm()
                          }
                          aria-label="Preferred form"
                        />
                        <button className="primary" onClick={saveEditTerm}>
                          Save
                        </button>
                        <button
                          className="ghost"
                          onClick={() => setEditingTermId(null)}
                        >
                          Cancel
                        </button>
                      </div>
                    ) : (
                      <>
                        <div>
                          <strong>{d.term}</strong>
                          {d.preferred && (
                            <span className="muted"> → {d.preferred}</span>
                          )}
                          <div className="tiny">
                            {d.hits} hits
                            {d.starred ? " · always biased" : ""}
                          </div>
                        </div>
                        <div className="dict-actions">
                          <button
                            className={d.starred ? "star on" : "star"}
                            onClick={() => toggleStar(d)}
                            aria-pressed={!!d.starred}
                            title={
                              d.starred
                                ? "Starred — always biases the recognizer"
                                : "Star to always bias the recognizer"
                            }
                          >
                            {d.starred ? "★" : "☆"}
                          </button>
                          <button
                            className="ghost"
                            onClick={() => startEditTerm(d)}
                          >
                            Edit
                          </button>
                          <button
                            className="ghost danger"
                            onClick={() => removeTerm(d.id)}
                          >
                            Remove
                          </button>
                        </div>
                      </>
                    )}
                  </li>
                ))}
              </ul>
            </div>
  );
}
