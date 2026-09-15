import { useAppCtx } from "../../appShell";

export default function Audio() {
  const {
    saveSettings, settings,
  } = useAppCtx();
  if (!settings) return null;
  return (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Audio
                    <span className="sub">
                      Cleaning up your mic and quieting the room while you talk.
                    </span>
                  </h2>
                  <label className="toggle">
                    <input
                      type="checkbox"
                      checked={settings.denoise ?? true}
                      onChange={(e) =>
                        saveSettings({ ...settings, denoise: e.target.checked })
                      }
                    />
                    <span>
                      <strong>Denoise</strong> — remove steady background noise
                      like fans, hum, and keyboard clatter before transcribing
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
                      <strong>Mute the Mac while dictating</strong> — silence
                      system audio so nothing plays over you, and restore your
                      exact volume when you stop
                    </span>
                  </label>
                </section>
  );
}
