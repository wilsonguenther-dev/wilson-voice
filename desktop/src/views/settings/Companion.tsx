import { useAppCtx } from "../../appShell";

/**
 * The pill styles a user can pick. yap-1106-retire-classic-pill: Classic is
 * retired, so it is not offered; a saved "classic" (or nothing saved) is
 * migrated to "yappy" at launch and reads as Yappy here.
 */
export const PILL_STYLE_CHOICES = [
  ["yappy", "Yappy", "A pixel pet in a little world"],
] as const;

/** The choice to highlight: the saved style when it is still offered, else Yappy. */
export function activePillStyle(saved?: string | null): string {
  return PILL_STYLE_CHOICES.some(([id]) => id === saved) ? (saved as string) : PILL_STYLE_CHOICES[0][0];
}

export default function Companion() {
  const {
    saveSettings, settings,
  } = useAppCtx();
  if (!settings) return null;
  return (
                <section className="settings-panel" data-settings-pane="companion">
                  <div className="panel" data-row>
                    <h3>Pill style</h3>
                    <div className="profile-row">
                      {PILL_STYLE_CHOICES.map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            activePillStyle(settings.pillStyle) === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, pillStyle: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                    <p className="settings-hint">It appears while you dictate.</p>
                  </div>
                  <div className="panel" data-row>
                    <h3>Screen position</h3>
                    <div className="profile-row">
                      {(
                        [
                          ["bottom", "Bottom", "Centred along the screen bottom"],
                          ["left", "Left edge", "Docked left, halfway down"],
                          ["right", "Right edge", "Docked right, halfway down"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.pillPosition ?? "bottom") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, pillPosition: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                    <p className="settings-hint">The side docks pin it halfway down that edge, out of the way of your text.</p>
                  </div>
                  <div className="panel" data-row>
                    <h3>Companion tone</h3>
                    <div className="profile-row">
                      {(
                        [
                          ["friendly", "Friendly", "Warm and encouraging"],
                          ["rude", "Rude 😏", "Sassy — teases when you ramble"],
                          ["rose", "Rose 🌹", "Sweet and adoring"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.companionTone ?? "friendly") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, companionTone: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                    <p className="settings-hint">How Yappy reacts when you finish, whichever companion you picked.</p>
                  </div>
                  <div className="panel" data-row>
                  <h3>Always on screen</h3>
                  <label className="toggle">
                    <input
                      type="checkbox"
                      checked={settings.showFloatingPill ?? true}
                      onChange={(e) =>
                        saveSettings({
                          ...settings,
                          showFloatingPill: e.target.checked,
                        })
                      }
                    />
                    <span>Keep the companion on screen at all times</span>
                  </label>
                  <p className="settings-hint">Off, it only appears while you talk.</p>
                  </div>
                </section>
  );
}
