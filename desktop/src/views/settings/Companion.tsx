import { useAppCtx } from "../../appShell";

export default function Companion() {
  const {
    saveSettings, settings,
  } = useAppCtx();
  if (!settings) return null;
  return (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Companion
                    <span className="sub">
                      How Yap looks and feels while you talk.
                    </span>
                  </h2>
                  <div className="panel">
                    <h3>Pill style</h3>
                    <p>
                      Pick your companion — the little helper that appears when
                      you dictate. It switches live, so try both.
                    </p>
                    <div className="profile-row">
                      {(
                        [
                          ["classic", "Classic", "A sleek waveform capsule"],
                          ["yappy", "Yappy 🐥", "A pixel pet in a little world"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.pillStyle ?? "classic") === id
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
                  </div>
                  <div className="panel">
                    <h3>Screen position</h3>
                    <p>
                      Where your companion sits. Bottom is the classic island;
                      the side docks pin it to that edge, halfway down the
                      screen, out of the way of what you’re typing into.
                    </p>
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
                  </div>
                  <div className="panel">
                    <h3>Companion tone</h3>
                    <p>
                      How Yappy talks back when you finish — the reactive line
                      keyed to how much you said. Warm, a little rude, or sweet.
                      Independent of which companion you picked.
                    </p>
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
                  </div>
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
                    <span>
                      Keep the companion on screen at all times (otherwise it
                      only appears while you talk)
                    </span>
                  </label>
                </section>
  );
}
