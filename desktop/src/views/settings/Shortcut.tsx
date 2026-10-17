import { useEffect, useState } from "react";
import { useAppCtx } from "../../appShell";
import type { ChordSetting } from "../../appTypes";
import { dictateLabel } from "../../hotkeyStatus";

const MODIFIER_CODES = new Set([
  "ControlLeft", "ControlRight", "AltLeft", "AltRight", "MetaLeft", "MetaRight",
  "ShiftLeft", "ShiftRight", "CapsLock", "Fn",
]);

/**
 * yap-p0-hotkeys-no-permission — turn one keydown into a Carbon chord, or say
 * why it cannot be one. Code + modifiers only; ⌃, ⌥ or ⌘ is required so the
 * chord can never swallow a bare key system-wide.
 */
export function chordFromKeyEvent(e: {
  code: string; ctrlKey: boolean; altKey: boolean; metaKey: boolean; shiftKey: boolean;
}): ChordSetting | string {
  if (MODIFIER_CODES.has(e.code)) return "Keep holding the modifiers and press a key.";
  if (!(e.ctrlKey || e.altKey || e.metaKey)) return "Add ⌃, ⌥ or ⌘ to the key.";
  return { code: e.code, ctrl: e.ctrlKey, alt: e.altKey, cmd: e.metaKey, shift: e.shiftKey };
}

export default function Shortcut() {
  const {
    applyBinding, captureHint, capturing, saveSettings, setCaptureHint, setCapturing,
    settings, status,
  } = useAppCtx();
  const [rebinding, setRebinding] = useState(false);
  const [rebindHint, setRebindHint] = useState<string | null>(null);
  useEffect(() => {
    if (!rebinding || !settings) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      if (e.code === "Escape" && !(e.ctrlKey || e.altKey || e.metaKey)) {
        setRebinding(false);
        setRebindHint(null);
        return;
      }
      const chord = chordFromKeyEvent(e);
      if (typeof chord === "string") {
        setRebindHint(chord);
        return;
      }
      setRebinding(false);
      setRebindHint(null);
      saveSettings({ ...settings, dictateToggle: chord });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [rebinding, settings, saveSettings]);
  if (!settings) return null;
  return (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Shortcut
                    <span className="sub">
                      The key you hold to start talking.
                    </span>
                  </h2>
                  {/* yap-p0-hotkeys-no-permission — the zero-permission chord. */}
                  <div className="panel">
                    <h3>Works everywhere</h3>
                    <p>
                      Press <strong>{dictateLabel(status)}</strong> to start a
                      take and press it again to stop — or hold it and let go.
                      It needs no Accessibility or Input Monitoring permission
                      and keeps working when another app has Secure Keyboard
                      Entry on.
                    </p>
                    <div
                      style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}
                    >
                      <button
                        type="button"
                        className={rebinding ? "primary" : "ghost"}
                        aria-pressed={rebinding}
                        onClick={() => {
                          setRebindHint(rebinding ? null : "Press the new chord (Esc to cancel).");
                          setRebinding((r) => !r);
                        }}
                      >
                        {rebinding ? "Listening… press your chord" : "Change"}
                      </button>
                      {settings.dictateToggle && (
                        <button
                          type="button"
                          className="ghost"
                          onClick={() => saveSettings({ ...settings, dictateToggle: null })}
                        >
                          Reset to ⌃⌥⌘Space
                        </button>
                      )}
                    </div>
                    {rebindHint && (
                      <p className="muted" style={{ marginTop: 8 }}>
                        {rebindHint}
                      </p>
                    )}
                  </div>
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
