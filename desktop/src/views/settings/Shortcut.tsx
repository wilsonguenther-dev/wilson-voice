import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import HotkeyRecorder from "../../HotkeyRow";
import { recorderPrompt, type HotkeyBindingsReport } from "../../hotkeyRecorder";
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
  // focus-2026-10-19 — the live hotkey table: one row per action with its
  // status chip and the key that works right now (re-read on every status).
  const [report, setReport] = useState<HotkeyBindingsReport | null>(null);
  const [tableError, setTableError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    invoke<HotkeyBindingsReport>("hotkey_bindings")
      .then((r) => live && setReport(r))
      .catch(() => live && setReport(null));
    return () => {
      live = false;
    };
  }, [status, settings]);
  const rowFor = (a: string) => report?.rows.find((r) => r.action === a);
  const save = async (next: typeof settings) => {
    setTableError(null);
    try {
      await saveSettings(next!);
    } catch (e) {
      setTableError(String(e));
    }
  };
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
                <section className="settings-panel" data-settings-pane="shortcut">
                  {/* yap-p0-hotkeys-no-permission — the zero-permission chord. */}
                  <div className="panel" data-row>
                    <h3>
                      Works everywhere: <strong>{dictateLabel(status)}</strong>
                    </h3>
                    <div
                      style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}
                    >
                      <button
                        type="button"
                        className={rebinding ? "primary" : "ghost"}
                        aria-pressed={rebinding}
                        onClick={() => {
                          setRebindHint(rebinding ? null : recorderPrompt("chord").hint);
                          setRebinding((r) => !r);
                        }}
                      >
                        {rebinding ? recorderPrompt("chord").listening : "Change"}
                      </button>
                    </div>
                    {rebindHint && (
                      <p className="muted" style={{ marginTop: 8 }}>
                        {rebindHint}
                      </p>
                    )}
                    <p className="settings-hint">
                      Press to start, press again to stop — or hold and let go;
                      no permission needed, even under Secure Keyboard Entry.
                    </p>
                  </div>
                  {/* focus-2026-10-19 — every action: key, live status, recorder, reset. */}
                  <div className="panel" data-row data-testid="hotkey-table">
                    <h3>Hotkeys</h3>
                    {report?.liveDictateKey && rowFor("dictate_hold")?.fallback && (
                      <p className="muted">
                        The fn hold is not working right now — <strong>{report.liveDictateKey}</strong> starts a take instead.
                      </p>
                    )}
                    <HotkeyRecorder
                      title="Dictate (hold)"
                      keyLabel={settings.hotkeyLabel ?? "fn⌃"}
                      defaultLabel="fn⌃"
                      isDefault={(settings.pttBinding ?? "fn_control") === "fn_control"}
                      mode="hold"
                      row={rowFor("dictate_hold")}
                      onCapture={(s) => s.kind === "hold" && applyBinding(s.binding)}
                      onReset={() => applyBinding("fn_control")}
                    />
                    <HotkeyRecorder
                      title="Hands-free"
                      keyLabel={`double-tap ${settings.hotkeyLabel ?? "fn⌃"}`}
                      defaultLabel="fn⌃"
                      isDefault
                      mode="hold"
                      row={rowFor("hands_free")}
                      onCapture={(s) => s.kind === "hold" && applyBinding(s.binding)}
                      onReset={() => applyBinding("fn_control")}
                    />
                    <HotkeyRecorder
                      title="Dictate (toggle)"
                      keyLabel={dictateLabel(status)}
                      defaultLabel="⌃⌥⌘Space"
                      isDefault={!settings.dictateToggle}
                      mode="chord"
                      row={rowFor("dictate_toggle")}
                      onCapture={(s) => s.kind === "chord" && save({ ...settings, dictateToggle: s.chord })}
                      onReset={() => save({ ...settings, dictateToggle: null })}
                    />
                    <HotkeyRecorder
                      title="Cancel take"
                      keyLabel={rowFor("cancel")?.key ?? "esc"}
                      defaultLabel="esc"
                      isDefault={!settings.cancelChord}
                      mode="chord"
                      allowBare
                      row={rowFor("cancel")}
                      onCapture={(s) => s.kind === "chord" && save({ ...settings, cancelChord: s.chord })}
                      onReset={() => save({ ...settings, cancelChord: null })}
                    />
                    <HotkeyRecorder
                      title="Record meeting"
                      keyLabel={rowFor("record_meeting")?.key ?? "⌃⌘M"}
                      defaultLabel="⌃⌘M"
                      isDefault={!settings.meetingChord}
                      mode="chord"
                      row={rowFor("record_meeting")}
                      onCapture={(s) => s.kind === "chord" && save({ ...settings, meetingChord: s.chord })}
                      onReset={() => save({ ...settings, meetingChord: null })}
                    />
                    {tableError && <p className="error">{tableError}</p>}
                  </div>
                  <div className="panel" data-row>
                    <h3>Hold-to-talk key</h3>
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
                    <p className="settings-hint">
                      Hold to talk, double-tap for hands-free; needs Accessibility.
                      Set Keyboard → “Press 🌐 key to” → Do Nothing so Globe
                      doesn’t open emoji.
                    </p>
                  </div>
                  {/* YV49 — command mode: the same hold, plus one modifier,
                      edits the text you already selected instead of typing. */}
                  <div className="panel" data-row>
                    <h3>Command mode</h3>
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
                    <p className="settings-hint">
                      Select text, hold{" "}
                      <strong>
                        {settings.hotkeyLabel}
                        {(settings.commandBinding ?? "command") === "option"
                          ? "⌥"
                          : "⌘"}
                      </strong>{" "}
                      and say “make it a list” or “delete that”; anything Yap
                      doesn’t know exactly is ignored.
                    </p>
                  </div>
                  {/* yap-uiux-2026-10-16-SETTINGS — the pane's reset sits in
                      the pinned footer, not inside the card it resets. */}
                  {settings.dictateToggle && (
                    <div className="settings-footer">
                      <button
                        type="button"
                        className="ghost"
                        onClick={() => saveSettings({ ...settings, dictateToggle: null })}
                      >
                        Reset to ⌃⌥⌘Space
                      </button>
                    </div>
                  )}
                </section>
  );
}
