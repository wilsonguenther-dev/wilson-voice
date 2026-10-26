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
                <section className="settings-panel" data-settings-pane="advanced">
                  <div className="panel" data-row>
                    <h3>Speech model</h3>
                    <ModelPicker setup={modelSetup} />
                    <p className="settings-hint">
                      Runs inside Yap, on this Mac: smaller is faster, larger is
                      more accurate on long or technical dictation.
                    </p>
                  </div>
                  <div className="panel" data-row>
                    <h3>AI polish model (optional)</h3>
                    <PolishModelPicker polish={polishSetup} />
                    <p className="settings-hint">
                      An optional local model that rewrites a finished take into
                      cleaner prose; off until you install it, never off this Mac.
                    </p>
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
                        every take starts instantly; Yap holds about 900 MB
                        the whole time
                      </span>
                    </label>
                  </div>
                  {/* YV42 — launch at login. Off by default; nothing installs a
                      login item behind your back. */}
                  <div className="panel" data-row>
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
                        <strong>Launch Yap at login</strong> — it has to be
                        running to catch your hold-to-talk key
                      </span>
                    </label>
                  </div>
                  {/* YV44 — updates are opt-out and consent-based: Yap asks
                      before it ever downloads or installs anything. */}
                  <div className="panel" data-row>
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
                        <strong>Check for updates</strong> — at launch; nothing
                        downloads or installs until you say so
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
