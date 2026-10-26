import { useAppCtx } from "../../appShell";

export default function Audio() {
  const {
    saveSettings, settings,
  } = useAppCtx();
  if (!settings) return null;
  return (
                <section className="settings-panel" data-settings-pane="audio">
                  <div className="panel" data-row>
                  <h3>While you talk</h3>
                  <label className="toggle">
                    <input
                      type="checkbox"
                      checked={settings.denoise ?? true}
                      onChange={(e) =>
                        saveSettings({ ...settings, denoise: e.target.checked })
                      }
                    />
                    <span>
                      <strong>Denoise</strong> — remove fans, hum and keyboard
                      clatter before transcribing
                    </span>
                  </label>
                  <label className="toggle">
                    <input
                      type="checkbox"
                      checked={settings.muteWhileDictating ?? true}
                      onChange={(e) =>
                        saveSettings({
                          ...settings,
                          muteWhileDictating: e.target.checked,
                        })
                      }
                    />
                    <span>
                      <strong>Mute the Mac while dictating</strong> — your exact
                      volume comes back when you stop
                    </span>
                  </label>
                  </div>
                </section>
  );
}
