import { useAppCtx } from "../../appShell";

export default function Shortcut() {
  const {
    applyBinding, captureHint, capturing, saveSettings, setCaptureHint, setCapturing,
    settings,
  } = useAppCtx();
  if (!settings) return null;
  return (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Shortcut
                    <span className="sub">
                      The key you hold to start talking.
                    </span>
                  </h2>
                  <div className="panel">
                    <h3>Hold-to-talk key</h3>
                    <p>
                      Default is <strong>fn + Control</strong>. Hold it to talk,
                      double-tap to keep it running hands-free, then tap again to
                      stop. Needs Accessibility permission. Tip: set Keyboard →
                      “Press 🌐 key to” → <strong>Do Nothing</strong> so the
                      Globe key doesn’t open emoji.
                    </p>
                    <div className="profile-row">
                      {(
                        [
                          ["fn_control", "fn⌃", "Hold + double-tap hands-free"],
                          ["fn", "fn", "Bare Globe only"],
                          ["both", "fn / fn⌃", "Either combo"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.pttBinding ?? "fn_control") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() => applyBinding(id)}
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                    {/* YV15 — record the next combo instead of picking a preset. */}
                    <div
                      style={{
                        display: "flex",
                        gap: 8,
                        alignItems: "center",
                        flexWrap: "wrap",
                        marginTop: 12,
                      }}
                    >
                      <button
                        type="button"
                        className={capturing ? "primary" : "ghost"}
                        aria-pressed={capturing}
                        onClick={() => {
                          setCaptureHint(
                            capturing
                              ? null
                              : "Listening… hold your shortcut now (Esc to cancel).",
                          );
                          setCapturing((c) => !c);
                        }}
                      >
                        {capturing
                          ? "Listening… press your keys"
                          : "Set shortcut"}
                      </button>
                      <span className="muted">
                        Currently <strong>{settings.hotkeyLabel}</strong>
                      </span>
                    </div>
                    {captureHint && (
                      <p className="muted" style={{ marginTop: 8 }}>
                        {captureHint}
                      </p>
                    )}
                    <label className="toggle" style={{ marginTop: 12 }}>
                      <input
                        type="checkbox"
                        checked={settings.keepCmdShiftV ?? false}
                        onChange={(e) =>
                          saveSettings({
                            ...settings,
                            keepCmdShiftV: e.target.checked,
                          })
                        }
                      />
                      <span>Also let me hold ⌘⇧V as a backup</span>
                    </label>
                    <p style={{ marginTop: 12 }}>
                      Currently set to <strong>{settings.hotkeyLabel}</strong>.
                      The companion stays bottom-center and follows you across
                      every desktop. Yap also learns the words you use most over
                      time.
                    </p>
                  </div>
                  {/* YV49 — command mode: the same hold, plus one modifier,
                      edits the text you already selected instead of typing. */}
                  <div className="panel">
                    <h3>Command mode</h3>
                    <p>
                      Select some text, then hold{" "}
                      <strong>
                        {settings.hotkeyLabel}
                        {(settings.commandBinding ?? "command") === "option"
                          ? "⌥"
                          : "⌘"}
                      </strong>{" "}
                      and say what to do with it — “make it a list”, “make it
                      uppercase”, “wrap in quotes”, “replace Monday with
                      Tuesday”, “delete that”. Yap only runs commands it knows
                      exactly; anything else is ignored so your text is never
                      guessed at.
                    </p>
                    <div className="profile-row">
                      {(
                        [
                          ["command", "⌘", "Hold your key plus Command"],
                          ["option", "⌥", "Hold your key plus Option"],
                          ["off", "Off", "Every hold just dictates"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.commandBinding ?? "command") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, commandBinding: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                  </div>
                </section>
  );
}
