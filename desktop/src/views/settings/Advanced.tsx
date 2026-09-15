import { invoke } from "@tauri-apps/api/core";
import { ModelPicker, PolishModelPicker } from "../../ModelSetup";
import { checkForUpdate } from "../../updater";
import { type AppSettings } from "../../appTypes";
import { useAppCtx } from "../../appShell";

export default function Advanced() {
  const {
    modelSetup, perms, polishSetup, saveSettings,
    setSettings, setUpdate, settings, toast,
  } = useAppCtx();
  if (!settings) return null;
  async function checkForUpdateNow() {
    try {
      // Asking explicitly means "show me anything" — retire an earlier skip so
      // the version they dismissed can be offered again on request.
      if (settings?.skippedUpdateVersion) {
        const next: AppSettings = { ...settings, skippedUpdateVersion: null };
        await invoke("save_settings", { settings: next });
        setSettings(next);
      }
      const found = await checkForUpdate();
      setUpdate(found);
      toast(
        found
          ? `Yap ${found.version} is available`
          : "You're on the latest Yap",
      );
    } catch (e) {
      toast(`Couldn't check for updates: ${e}`);
    }
  }
  return (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Advanced
                    <span className="sub">
                      Your speech model and how Yap starts up. Leave this as-is
                      unless dictation feels slow or misses words.
                    </span>
                  </h2>
                  <div className="panel">
                    <h3>Speech model</h3>
                    <p className="muted">
                      Yap transcribes inside the app itself — no helper process,
                      nothing installed on the side, nothing off this Mac. Your
                      model loads on your first dictation and stays warm after
                      that, so an idle Yap costs almost nothing. Smaller models
                      are faster and lighter; larger ones are more accurate on
                      long or technical dictation. Yap picks the recommended one
                      for you — swap it here if you'd rather choose.
                    </p>
                    <ModelPicker setup={modelSetup} />
                  </div>
                  <div className="panel">
                    <h3>AI polish model (optional)</h3>
                    <p className="muted">
                      Everything above runs with no extra download. This one is
                      a separate, optional language model that runs on your Mac
                      and rewrites a finished take into cleaner prose — it fixes
                      run-ons, drops fillers and shapes an email like an email.
                      It is off until you install it, it never leaves this Mac,
                      and with it off Yap formats exactly as it does today. It
                      is large, so Yap checks you have room before it starts.
                    </p>
                    <PolishModelPicker polish={polishSetup} />
                    <p className="muted tiny" style={{ marginTop: 8 }}>
                      {perms?.asrDetail}
                    </p>
                    {/* YV80 — Model & Speed: the memory-vs-first-take trade,
                        made explicit. Off by default; an idle Yap holds ~930 MB
                        less and the first take loads the engine while you talk. */}
                    <label className="toggle" style={{ marginTop: 12 }}>
                      <input
                        type="checkbox"
                        checked={settings.preloadModel ?? false}
                        onChange={(e) =>
                          saveSettings({
                            ...settings,
                            preloadModel: e.target.checked,
                          })
                        }
                      />
                      <span>
                        <strong>Keep the model loaded from launch</strong> —
                        Yap normally loads your speech model on your first
                        dictation, which keeps about 900 MB free while you are
                        not dictating and costs a few seconds on that first
                        take. Turn this on to load it at launch instead: every
                        take starts instantly, and Yap holds the memory the
                        whole time it is running
                      </span>
                    </label>
                  </div>
                  {/* YV42 — launch at login. Off by default; nothing installs a
                      login item behind your back. */}
                  <div className="panel">
                    <h3>Startup</h3>
                    <label className="toggle">
                      <input
                        type="checkbox"
                        checked={settings.autostart ?? false}
                        onChange={(e) =>
                          saveSettings({
                            ...settings,
                            autostart: e.target.checked,
                          })
                        }
                      />
                      <span>
                        <strong>Launch Yap at login</strong> — Yap has to be
                        running to catch your hold-to-talk key, so start it
                        automatically when you sign in
                      </span>
                    </label>
                  </div>
                  {/* YV44 — updates are opt-out and consent-based: Yap asks
                      before it ever downloads or installs anything. */}
                  <div className="panel">
                    <h3>Updates</h3>
                    <label className="toggle">
                      <input
                        type="checkbox"
                        checked={settings.checkUpdates ?? true}
                        onChange={(e) =>
                          saveSettings({
                            ...settings,
                            checkUpdates: e.target.checked,
                          })
                        }
                      />
                      <span>
                        <strong>Check for updates</strong> — look for a newer
                        Yap at launch and tell you about it. Nothing downloads
                        or installs until you say so; turn this off and Yap
                        never contacts the release page at all
                      </span>
                    </label>
                    {(settings.checkUpdates ?? true) && (
                      <div className="actions" style={{ marginTop: 10 }}>
                        <button onClick={checkForUpdateNow}>
                          Check for updates now
                        </button>
                      </div>
                    )}
                  </div>
                </section>
  );
}
