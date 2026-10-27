import { levelNeedsPolishModel, selectedSpeed } from "../../formatting";
import WritingStyle from "../../WritingStyle";
import { useAppCtx } from "../../appShell";
import { useState } from "react";
import VoicePreferencesPanel from "../../VoicePreferencesPanel";
import {
  STRUCTURE_CHOICES,
  type StructureChoice,
  structureEnabled,
  structureOverrideRows,
  withStructureOverride,
} from "../../structureSettings";

export default function Dictation() {
  const {
    formatting, polishSetup, saveSettings, setSignatureDraft, settings, signatureDraft,
  } = useAppCtx();
  // yap-format-structure: the app id being typed into the "add an app" row.
  const [newStructureApp, setNewStructureApp] = useState("");
  const [newStructureChoice, setNewStructureChoice] =
    useState<StructureChoice>("plain");
  if (!settings) return null;
  return (
                <section className="settings-panel" data-settings-pane="dictation">
                  <div className="panel" data-row>
                    <h3>Dictation mode</h3>
                    <div className="profile-row">
                      {(
                        [
                          ["auto", "Auto", "Match the app I’m typing into"],
                          ["plain", "Plain", "Exactly what I said, no changes"],
                          ["list", "List", "Turn spoken lists into bullets"],
                          ["email", "Email", "Tidy formatting for messages"],
                          ["code", "Code", "Leave code and names untouched"],
                          ["notes", "Notes", "Formatting for quick notes"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.dictationMode ?? "auto") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, dictationMode: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                    <p className="settings-hint">
                      Auto fits the app you’re typing into; a fixed mode always
                      formats the same way.
                    </p>
                  </div>
                  {/* Y4-H — controls first, prose last. The question is "how
                      much should Yap clean up?", not "what is cleanup_level?",
                      and every description below is built backend-side from the
                      SAME `CleanupLevel::runs_*` predicates `run_cleanup`
                      branches on. That is why the stale "the AI polish pass
                      isn't wired up yet" line is gone and cannot come back: no
                      string here describes a stage, so none can go out of date.
                      The stored KEY (`cleanupLevel`: none|light|medium|high) is
                      untouched — only the label a person reads changed. */}
                  <div className="panel" data-row>
                    <h3>How much should Yap clean up?</h3>
                    {/* Y4-A — ONE quiet line, and only for a user whose stored
                        level was migrated. `formattingNoticePending` is set by
                        the v1 → v2 settings migration and by nothing else, so a
                        fresh install never sees this: there is no change to
                        explain. It says what changed and where to undo it,
                        because a silent behaviour change on somebody's existing
                        install is the thing being avoided, not shipped. */}
                    {settings.formattingNoticePending && (
                      <p className="notice" role="status">
                        Formatting is on now — spoken punctuation, lists and
                        email shape. Change it in the picker below.{" "}
                        <button
                          type="button"
                          className="linklike"
                          onClick={() =>
                            saveSettings({
                              ...settings,
                              formattingNoticePending: false,
                            })
                          }
                        >
                          Got it
                        </button>
                      </p>
                    )}
                    <div className="profile-row">
                      {formatting.cleanupLevels.map((level) => (
                        <button
                          key={level.id}
                          type="button"
                          className={
                            (settings.cleanupLevel ?? "medium") === level.id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({
                              ...settings,
                              cleanupLevel: level.id,
                            })
                          }
                        >
                          <strong>{level.label}</strong>
                          <span>{level.description}</span>
                          {level.needsPolishModel && (
                            <span className="muted tiny">
                              {polishSetup.active
                                ? "Uses the local model you installed"
                                : "Needs the local model — install it below"}
                            </span>
                          )}
                        </button>
                      ))}
                    </div>
                    {levelNeedsPolishModel(
                      settings.cleanupLevel ?? "medium",
                      formatting.cleanupLevels,
                    ) &&
                      !polishSetup.active && (
                        <p className="muted">
                          No local model is installed, so this setting behaves
                          like the one before it until you install one under “AI
                          polish model” below.
                        </p>
                      )}
                    {/* Y4-H — `polish_deadline_ms` was invisible: a bounded
                        number (100..5000) no screen showed. It is a named
                        choice now, and only where it can change anything. */}
                    {levelNeedsPolishModel(
                      settings.cleanupLevel ?? "medium",
                      formatting.cleanupLevels,
                    ) &&
                      polishSetup.active && (
                        <>
                          <h4>How long may the model think?</h4>
                          <div className="profile-row">
                            {formatting.polishSpeeds.map((speed) => (
                              <button
                                key={speed.id}
                                type="button"
                                className={
                                  selectedSpeed(
                                    settings.polishDeadlineMs,
                                    formatting.polishSpeeds,
                                  ) === speed.id
                                    ? "profile active"
                                    : "profile"
                                }
                                onClick={() =>
                                  saveSettings({
                                    ...settings,
                                    polishDeadlineMs: speed.deadlineMs,
                                  })
                                }
                              >
                                <strong>{speed.label}</strong>
                                <span>{speed.description}</span>
                              </button>
                            ))}
                          </div>
                        </>
                      )}
                    <p className="settings-hint">
                      Yap keeps the raw take too — undo with ⌃⌘Z, “Undo AI Edit”
                      in the menu bar, or “Paste raw” in History.
                    </p>
                  </div>
                  {/* YV62 (R14) — the tone dial, per surface. It reaches the
                      rules, not just the local model: Formal keeps the full
                      stop, Very Casual drops it everywhere. */}
                  <div className="panel" data-row>
                    <h3>How should Yap sound in each app?</h3>
                    <div className="tone-dial">
                      {(
                        [
                          ["email", "Email"],
                          ["chat", "Chat and messaging"],
                          ["notes", "Notes"],
                          ["document", "Documents"],
                          ["list", "Lists"],
                          ["plain", "Plain"],
                        ] as const
                      ).map(([mode, label]) => (
                        <label className="field" key={mode}>
                          <span>{label}</span>
                          <select
                            value={settings.polishStyles?.[mode] ?? "default"}
                            onChange={(e) =>
                              saveSettings({
                                ...settings,
                                polishStyles: {
                                  ...(settings.polishStyles ?? {}),
                                  [mode]: e.target.value,
                                },
                              })
                            }
                          >
                            <option value="very casual">Very casual</option>
                            <option value="casual">Casual</option>
                            <option value="default">Default</option>
                            <option value="formal">Formal</option>
                          </select>
                        </label>
                      ))}
                    </div>
                    <p className="settings-hint">
                      Only capitalisation and punctuation change, never your words.
                    </p>
                  </div>
                  {/* YV62 (R13) — the sign-off block. Opt-in, and pasted byte
                      for byte after every other stage so nothing rewrites it. */}
                  <div className="panel" data-row>
                    <h3>Should Yap sign off for you?</h3>
                    <div className="profile-row">
                      {(
                        [
                          ["off", "Off", "Never add a signature"],
                          ["cue", "On cue", "Only when I say “sign it”"],
                          [
                            "auto",
                            "Automatic",
                            "On “sign it”, and on emails I end with “thanks”",
                          ],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.signatureMode ?? "off") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, signatureMode: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                    <label className="field">
                      <span>Signature</span>
                      <textarea
                        rows={2}
                        placeholder="Wilson — drivia.consulting"
                        aria-label="Signature block"
                        value={signatureDraft ?? settings.signature ?? ""}
                        onChange={(e) => setSignatureDraft(e.target.value)}
                        onBlur={() => {
                          if (
                            signatureDraft !== null &&
                            signatureDraft !== (settings.signature ?? "")
                          ) {
                            saveSettings({
                              ...settings,
                              signature: signatureDraft,
                            });
                          }
                          setSignatureDraft(null);
                        }}
                      />
                    </label>
                    <p className="settings-hint">
                      Pasted exactly as typed, after every other step, so nothing
                      rewrites it.
                    </p>
                  </div>
                  <div className="panel" data-row>
                    <h3>Lists</h3>
                    <label className="toggle">
                      <input
                        type="checkbox"
                        aria-label="Turn spoken lists into bullets"
                        checked={structureEnabled(settings)}
                        onChange={(e) =>
                          saveSettings({
                            ...settings,
                            structureEnabled: e.target.checked,
                          })
                        }
                      />
                      <span>Turn spoken lists into bullets</span>
                    </label>
                    <p className="settings-hint">
                      Spoken lists become a list in the app you’re typing into;
                      your words stay as you said them.
                    </p>
                    {structureOverrideRows(settings.structureAppOverrides).map(
                      ([appId, choice]) => (
                        <div className="field" key={appId}>
                          <span>{appId}</span>
                          <select
                            aria-label={`List style in ${appId}`}
                            value={choice}
                            onChange={(e) =>
                              saveSettings({
                                ...settings,
                                structureAppOverrides: withStructureOverride(
                                  settings.structureAppOverrides,
                                  appId,
                                  e.target.value as StructureChoice,
                                ),
                              })
                            }
                          >
                            {STRUCTURE_CHOICES.map(([id, label]) => (
                              <option key={id} value={id}>
                                {label}
                              </option>
                            ))}
                          </select>
                          <button
                            type="button"
                            onClick={() =>
                              saveSettings({
                                ...settings,
                                structureAppOverrides: withStructureOverride(
                                  settings.structureAppOverrides,
                                  appId,
                                  null,
                                ),
                              })
                            }
                          >
                            Remove
                          </button>
                        </div>
                      ),
                    )}
                    <div className="field">
                      <span>A different list style for one app</span>
                      <input
                        type="text"
                        placeholder="App ID, e.g. com.apple.mail"
                        aria-label="App ID"
                        value={newStructureApp}
                        onChange={(e) => setNewStructureApp(e.target.value)}
                      />
                      <select
                        aria-label="List style for that app"
                        value={newStructureChoice}
                        onChange={(e) =>
                          setNewStructureChoice(e.target.value as StructureChoice)
                        }
                      >
                        {STRUCTURE_CHOICES.map(([id, label]) => (
                          <option key={id} value={id}>
                            {label}
                          </option>
                        ))}
                      </select>
                      <button
                        type="button"
                        disabled={!newStructureApp.trim()}
                        onClick={() => {
                          saveSettings({
                            ...settings,
                            structureAppOverrides: withStructureOverride(
                              settings.structureAppOverrides,
                              newStructureApp,
                              newStructureChoice,
                            ),
                          });
                          setNewStructureApp("");
                        }}
                      >
                        Add
                      </button>
                    </div>
                  </div>
                  <div className="panel" data-row>
                  <h3>Paste and language</h3>
                  <label className="toggle">
                    <input
                      type="checkbox"
                      checked={settings.autoPaste}
                      onChange={(e) =>
                        saveSettings({
                          ...settings,
                          autoPaste: e.target.checked,
                        })
                      }
                    />
                    <span>Paste my words automatically when I finish talking</span>
                  </label>
                  <label className="field">
                    <span>Language I speak</span>
                    <select
                      value={settings.language}
                      onChange={(e) =>
                        saveSettings({ ...settings, language: e.target.value })
                      }
                    >
                      <option value="en">English</option>
                      <option value="es">Spanish</option>
                      <option value="fr">French</option>
                      <option value="ht">Haitian Creole</option>
                    </select>
                  </label>
                  <p className="settings-hint">
                    Auto-paste needs the Accessibility permission.
                  </p>
                  </div>
                  {/* Y9-C — spoken preference rules, with an explicit Apply step. */}
                  <VoicePreferencesPanel />
                  {/* Y9-B — writing samples → the local style profile. */}
                  <WritingStyle />
                </section>
  );
}
