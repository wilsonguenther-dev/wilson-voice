import { useAppCtx } from "../../appShell";
import MicrophonePicker from "./parts/MicrophonePicker";

/**
 * yap-1106-settings-redesign — the microphone picker lives in Settings ›
 * General and the "While you talk" cleanup in Settings › Dictation, so each
 * control shows exactly once. With no `part` both render (the old Audio tab).
 */
export type AudioPart = "input" | "cleanup";

export default function Audio({ part }: { part?: AudioPart } = {}) {
  const {
    saveSettings, settings,
  } = useAppCtx();
  if (!settings) return null;
  const showInput = part !== "cleanup";
  const showCleanup = part !== "input";
  return (
                <section className="settings-panel" data-settings-pane="audio">
                  {showInput && <MicrophonePicker />}
                  {showCleanup && (
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
                  <label className="toggle">
                    <input
                      type="checkbox"
                      checked={settings.earcons ?? false}
                      onChange={(e) =>
                        saveSettings({ ...settings, earcons: e.target.checked })
                      }
                    />
                    <span>
                      <strong>Sounds</strong> — a short chirp when you start, stop,
                      paste and hit a word milestone. Silent during meetings
                    </span>
                  </label>
                  </div>
                  )}
                </section>
  );
}
