/**
 * WF-18 — dev tooling: the extra-shortcuts editor in each of its states, so the
 * integrator's headless PNGs show the REAL component without the app.
 * A BUILD ENTRY behind `YAP_DEV_TOOLING=1` (dist/dev/shortcuts-editor-preview.html),
 * like `support/preview.tsx`; no shipped build carries it.
 *
 * Scenes (URL hash): #empty (one shortcut each), #several (two extras),
 * #full (four of four), #overlap (the warning after ⌃⌘H on Record meeting).
 */
import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import ExtraBindings from "./ExtraBindings";
import type { AppSettings, ChordSetting } from "./appTypes";
import { handsFreeLine } from "./shortcutEditor";
import "./App.css";

const c = (code: string, ctrl: boolean, alt: boolean, cmd: boolean): ChordSetting =>
  ({ code, ctrl, alt, cmd, shift: false });

const SCENES: Record<string, AppSettings> = {
  empty: {} as AppSettings,
  several: {
    extraChords: [
      { action: "record_meeting", chord: c("KeyR", true, true, false) },
      { action: "record_meeting", chord: c("KeyT", true, true, false) },
    ],
  } as AppSettings,
  full: {
    extraChords: ["KeyR", "KeyT", "KeyY"].map((k) => ({ action: "record_meeting" as const, chord: c(k, true, true, false) })),
  } as AppSettings,
  overlap: {} as AppSettings,
};

function Preview() {
  const scene = window.location.hash.replace("#", "") || "several";
  const [settings, setSettings] = useState<AppSettings>(SCENES[scene] ?? SCENES.several);
  return (
    <div className="settings-panel" style={{ padding: 24, maxWidth: 640 }}>
      <div className="panel" data-row>
        <h3>Hotkeys</h3>
        <div className="hotkey-row" style={{ display: "flex", gap: 8, alignItems: "center", padding: "6px 0" }}>
          <span style={{ minWidth: 130 }}>Record meeting</span>
          <kbd>⌃⌘M</kbd>
        </div>
        <ExtraBindings action="record_meeting" title="Record meeting" settings={settings} save={setSettings} />
        <p className="settings-hint">{handsFreeLine("fn")}</p>
        {scene === "overlap" && (
          <p className="warning hotkey-extras-warning" role="status">
            ⌃⌘H is already Hands-free (toggle) — remove it there first, or pick another chord
          </p>
        )}
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Preview />
  </React.StrictMode>,
);
