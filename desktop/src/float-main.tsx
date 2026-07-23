/**
 * Float entry — picks the pill style from settings and renders it.
 *   "classic" → the original obsidian waveform capsule (ClassicPill)
 *   "yappy"   → the pixel-art chick companion (YappyPill)
 * Live-switches when settings are saved (backend emits "settings").
 */
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ClassicPill from "./pill/ClassicPill";
import YappyPill from "./pill/YappyPill";
import "./float.css";

interface Settings { pillStyle?: string }

function Float() {
  const [style, setStyle] = useState<string>("classic");
  useEffect(() => {
    invoke<Settings>("get_settings").then((s) => setStyle(s.pillStyle || "classic")).catch(() => {});
    const un = listen<Settings>("settings", (e) => setStyle(e.payload?.pillStyle || "classic"));
    return () => { un.then((u) => u()); };
  }, []);
  return style === "yappy" ? <YappyPill /> : <ClassicPill />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Float />
  </React.StrictMode>,
);
