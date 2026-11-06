/**
 * WF-19 — dev tooling: the Shortcuts screen's mouse-button panel in each of
 * its states, so the integrator's headless PNGs show the REAL component
 * without the app. A BUILD ENTRY behind `YAP_DEV_TOOLING=1`
 * (dist/dev/mouse-ptt-preview.html), like the WF-18 shortcuts preview; no
 * shipped build carries it.
 *
 * Scenes (URL hash): #off (nothing bound), #hold (button 4, hold to talk),
 * #handsfree (middle button, click to toggle), #listening (the recorder
 * waiting), #refused (a right click was pressed while listening).
 */
import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import MousePttPanel, { MOUSE_CAPTURE_PROMPT } from "./MousePttPanel";
import type { AppSettings } from "./appTypes";
import { MOUSE_PRIMARY_REFUSED } from "./mouseBinding";
import "./App.css";

const SCENES: Record<string, { settings: AppSettings; capturing?: boolean; hint?: string }> = {
  off: { settings: { pttMouseBinding: "" } as AppSettings },
  hold: { settings: { pttMouseBinding: "button4" } as AppSettings },
  handsfree: { settings: { pttMouseBinding: "middle:hands_free" } as AppSettings },
  listening: { settings: { pttMouseBinding: "" } as AppSettings, capturing: true, hint: MOUSE_CAPTURE_PROMPT },
  refused: { settings: { pttMouseBinding: "" } as AppSettings, capturing: true, hint: MOUSE_PRIMARY_REFUSED },
};

function Preview() {
  const scene = window.location.hash.replace("#", "") || "hold";
  const s = SCENES[scene] ?? SCENES.hold;
  const [settings, setSettings] = useState<AppSettings>(s.settings);
  return (
    <div className="settings-panel" style={{ padding: 24, maxWidth: 640 }}>
      <MousePttPanel
        settings={settings}
        save={setSettings}
        initialCapturing={s.capturing ?? false}
        initialHint={s.hint ?? null}
      />
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Preview />
  </React.StrictMode>,
);
