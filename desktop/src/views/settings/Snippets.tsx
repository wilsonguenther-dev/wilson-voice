import { invoke } from "@tauri-apps/api/core";
import { type Snippet } from "../../appTypes";
import { useAppCtx } from "../../appShell";

export default function Snippets() {
  const {
    editExpansion, editTrigger, editingSnippetId, newExpansion, newTrigger, saveSettings,
    setEditExpansion, setEditTrigger, setEditingSnippetId, setNewExpansion, setNewTrigger, setSnippets,
    settings, snippets, toast, toastError,
  } = useAppCtx();
  if (!settings) return null;
  async function addSnippet() {
    if (!newTrigger.trim() || !newExpansion.trim()) return;
    try {
      await invoke("add_snippet", {
        trigger: newTrigger.trim(),
        expansion: newExpansion,
      });
      setNewTrigger("");
      setNewExpansion("");
      setSnippets(await invoke("list_snippets"));
      toast("Snippet saved");
    } catch (e) {
      toastError(e);
    }
  }
  function startEditSnippet(s: Snippet) {
    setEditingSnippetId(s.id);
    setEditTrigger(s.trigger);
    setEditExpansion(s.expansion);
  }
  async function saveEditSnippet() {
    if (!editingSnippetId || !editTrigger.trim() || !editExpansion.trim()) return;
    try {
      await invoke("update_snippet", {
        id: editingSnippetId,
        trigger: editTrigger.trim(),
        expansion: editExpansion,
      });
      setEditingSnippetId(null);
      setSnippets(await invoke("list_snippets"));
      toast("Snippet updated");
    } catch (e) {
      toastError(e);
    }
  }
  async function toggleSnippet(s: Snippet) {
    try {
      await invoke("set_snippet_enabled", { id: s.id, enabled: !s.enabled });
      setSnippets(await invoke("list_snippets"));
    } catch (e) {
      toastError(e);
    }
  }
  async function removeSnippet(id: string) {
    try {
      await invoke("delete_snippet", { id });
      setSnippets((all) => all.filter((s) => s.id !== id));
    } catch (e) {
      toastError(e);
    }
  }
  return (
                <section className="settings-panel" data-settings-pane="snippets">
                  <div className="panel" data-row>
                    <h3>New snippet</h3>
                    <div className="snip-add">
                      <input
                        placeholder="Trigger phrase (e.g. my email)"
                        value={newTrigger}
                        onChange={(e) => setNewTrigger(e.target.value)}
                        aria-label="Trigger phrase"
                      />
                      <textarea
                        placeholder="Expands to… (multiple lines are fine)"
                        value={newExpansion}
                        onChange={(e) => setNewExpansion(e.target.value)}
                        rows={3}
                        aria-label="Expansion text"
                      />
                      <button
                        className="primary"
                        onClick={addSnippet}
                        disabled={!newTrigger.trim() || !newExpansion.trim()}
                      >
                        Add snippet
                      </button>
                    </div>
                    <p className="settings-hint">
                      Say the trigger while dictating and Yap pastes the expansion
                      — whole words only, any capitalisation.
                    </p>
                  </div>
                  <div className="panel" data-row>
                    <h3>Where triggers fire</h3>
                    <div className="profile-row">
                      {(
                        [
                          [
                            "inline",
                            "Inline",
                            "Anywhere in what I said",
                          ],
                          [
                            "utterance",
                            "Whole utterance",
                            "Only when I say just the trigger",
                          ],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.snippetScope ?? "inline") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, snippetScope: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                    <p className="settings-hint">
                      Whole utterance only expands when the trigger is all you said.
                    </p>
                  </div>
                  {snippets.length === 0 ? (
                    <div className="panel" data-row>
                      <p className="tiny">
                        No snippets yet. Add one above and it takes effect on
                        your next dictation.
                      </p>
                    </div>
                  ) : (
                    <ul className="snip-list">
                      {snippets.map((s) => (
                        <li key={s.id}>
                          {editingSnippetId === s.id ? (
                            <div className="snip-edit">
                              <input
                                value={editTrigger}
                                onChange={(e) => setEditTrigger(e.target.value)}
                                aria-label="Trigger phrase"
                                autoFocus
                              />
                              <textarea
                                value={editExpansion}
                                onChange={(e) =>
                                  setEditExpansion(e.target.value)
                                }
                                rows={3}
                                aria-label="Expansion text"
                              />
                              <div className="snip-actions">
                                <button
                                  className="primary"
                                  onClick={saveEditSnippet}
                                >
                                  Save
                                </button>
                                <button
                                  className="ghost"
                                  onClick={() => setEditingSnippetId(null)}
                                >
                                  Cancel
                                </button>
                              </div>
                            </div>
                          ) : (
                            <>
                              <div className="snip-body">
                                <strong>{s.trigger}</strong>
                                <pre className="snip-expansion">
                                  {s.expansion}
                                </pre>
                                {!s.enabled && (
                                  <div className="tiny">
                                    Off — this one never expands
                                  </div>
                                )}
                              </div>
                              <div className="snip-actions">
                                <label className="toggle inline">
                                  <input
                                    type="checkbox"
                                    checked={s.enabled}
                                    onChange={() => toggleSnippet(s)}
                                    aria-label={`Enable ${s.trigger}`}
                                  />
                                  <span>On</span>
                                </label>
                                <button
                                  className="ghost"
                                  onClick={() => startEditSnippet(s)}
                                >
                                  Edit
                                </button>
                                <button
                                  className="ghost danger"
                                  onClick={() => removeSnippet(s.id)}
                                >
                                  Remove
                                </button>
                              </div>
                            </>
                          )}
                        </li>
                      ))}
                    </ul>
                  )}
                </section>
  );
}
