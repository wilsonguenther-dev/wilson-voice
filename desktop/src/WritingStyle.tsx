import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppCtx } from "./appShell";
import {
  countWords,
  rulesBlocker,
  sampleBlocker,
  type UserContextView,
} from "./writingSamples";

// Y9-B — Settings → Dictation → Writing style, rendered inside the Dictation
// pane (views/settings/Dictation.tsx). Samples the user pastes become
// few-shot context for the LOCAL polish model, so its rewrite sounds like
// them. Stored in the `user_context` table; never sent anywhere else.
export default function WritingStyle() {
  const { toast } = useAppCtx();
  const [view, setView] = useState<UserContextView | null>(null);
  const [draft, setDraft] = useState("");
  const [rulesDraft, setRulesDraft] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    invoke<UserContextView>("get_user_context")
      .then((v) => {
        if (live && v && Array.isArray(v.writingSamples)) setView(v);
      })
      .catch((e) => toast(String(e)));
    return () => {
      live = false;
    };
    // Load once on mount: `toast` is not a stable identity, and re-running this
    // on every render would refetch in a loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (!view) return null;
  const blocker = sampleBlocker(draft, view);
  const rules = rulesDraft ?? view.customRules;
  const rulesProblem = rulesBlocker(rules, view);

  async function addSample() {
    if (blocker) return;
    try {
      setView(await invoke<UserContextView>("add_writing_sample", { body: draft }));
      setDraft("");
      toast("Writing sample saved");
    } catch (e) {
      toast(String(e));
    }
  }
  async function removeSample(id: string) {
    try {
      setView(await invoke<UserContextView>("delete_writing_sample", { id }));
    } catch (e) {
      toast(String(e));
    }
  }
  async function saveRules() {
    if (rulesProblem) return;
    try {
      setView(await invoke<UserContextView>("set_custom_rules", { rules }));
      setRulesDraft(null);
      toast("Custom rules saved");
    } catch (e) {
      toast(String(e));
    }
  }

  return (
                <>
                  <div className="panel" data-row data-writing-style>
                    <h3>Writing style</h3>
                    <p className="settings-hint">
                      Paste an email or message you wrote. The polish model on
                      your Mac reads it as an example of how you write, so its
                      cleanup sounds like you. Samples stay on this Mac — they
                      are never uploaded, logged or put in a diagnostics file.
                    </p>
                    <div className="snip-add">
                      <textarea
                        placeholder="Paste an email you've written…"
                        value={draft}
                        onChange={(e) => setDraft(e.target.value)}
                        rows={5}
                        aria-label="Writing sample"
                        aria-describedby="writing-sample-status"
                      />
                      <button
                        className="primary"
                        onClick={addSample}
                        disabled={blocker !== null}
                      >
                        Add sample
                      </button>
                    </div>
                    <p className="tiny" id="writing-sample-status" aria-live="polite">
                      {draft.trim() === "" ? (
                        `${view.writingSamples.length} of ${view.maxSamples} samples · at least ${view.minSampleWords} words each`
                      ) : (
                        blocker ?? `${countWords(draft)} words — ready to add`
                      )}
                    </p>
                    <p className="settings-hint">
                      Used only when an AI polish model is installed.
                    </p>
                  </div>
                  {view.writingSamples.length > 0 && (
                    <ul className="snip-list" aria-label="Writing samples">
                      {view.writingSamples.map((s) => (
                        <li key={s.id}>
                          <div>
                            <span className="snip-expansion">{s.body}</span>
                            <span className="tiny">
                              {s.words} words
                              {s.inUse ? "" : " · not used: over the word budget"}
                            </span>
                          </div>
                          <button
                            type="button"
                            className="ghost"
                            onClick={() => removeSample(s.id)}
                            aria-label={`Remove writing sample (${s.words} words)`}
                          >
                            Remove
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                  <div className="panel" data-row>
                    <h3>Custom rules</h3>
                    <textarea
                      placeholder="e.g. Sign off with just my first name. Never use exclamation marks."
                      value={rules}
                      onChange={(e) => setRulesDraft(e.target.value)}
                      rows={3}
                      aria-label="Custom rules"
                    />
                    <div className="profile-row">
                      <button
                        className="primary"
                        onClick={saveRules}
                        disabled={rulesDraft === null || rulesProblem !== null}
                      >
                        Save rules
                      </button>
                    </div>
                    <p className="tiny" aria-live="polite">
                      {rulesProblem ??
                        `${countWords(rules)} of ${view.maxRulesWords} words · the formatting rules always win`}
                    </p>
                  </div>
                </>
  );
}
