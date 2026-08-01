/**
 * Float entry — picks the pill style from settings and renders it.
 *   "classic" → the original obsidian waveform capsule (ClassicPill)
 *   "yappy"   → the pixel-art chick companion (YappyPill)
 * Live-switches when settings are saved (backend emits "settings").
 *
 * YV53: the same settings carry the dock edge (bottom | left | right). The
 * backend moves the NSPanel; here we only mirror the edge onto <html> so the
 * stage can align the pill flush to it — the pill/world itself is never
 * stretched or letterboxed, it just anchors to that side.
 */
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ClassicPill from "./pill/ClassicPill";
import YappyPill from "./pill/YappyPill";
import "./float.css";

interface Settings { pillStyle?: string; pillPosition?: string }

const DOCKS = ["bottom", "left", "right"];
const dockOf = (s?: Settings) => {
  const p = s?.pillPosition || "bottom";
  return DOCKS.includes(p) ? p : "bottom";
};

function Float() {
  const [style, setStyle] = useState<string>("classic");
  useEffect(() => {
    // A synchronous cleanup can run before the listen() promise resolves
    // (StrictMode double-mount). A `dead` flag unsubscribes a listener that
    // lands after teardown so no native listener leaks.
    let dead = false;
    const unsubs: Array<() => void> = [];
    // The dock edge rides on <html> (not React state) — it only drives CSS
    // alignment, so flipping it must not re-render (and remount) the pill.
    const apply = (s?: Settings) => {
      setStyle(s?.pillStyle || "classic");
      document.documentElement.dataset.dock = dockOf(s);
    };
    invoke<Settings>("get_settings").then(apply).catch(() => {});
    listen<Settings>("settings", (e) => apply(e.payload)).then((u) => (dead ? u() : unsubs.push(u)));
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  return style === "yappy" ? <YappyPill /> : <ClassicPill />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Float />
  </React.StrictMode>,
);
