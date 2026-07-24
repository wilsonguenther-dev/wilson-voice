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
import { DEFAULT_TONE, type Tone } from "./pill/tone";
import "./float.css";

interface Settings { pillStyle?: string; companionTone?: string }

// Only the pill-facing tones are user-selectable (YV27); anything unexpected
// (or "off") falls back to the default so the pill still has a voice.
function toneFrom(s?: Settings): Tone {
  const t = s?.companionTone;
  return t === "rude" || t === "friendly" || t === "rose" ? t : DEFAULT_TONE.tone;
}

function Float() {
  const [style, setStyle] = useState<string>("classic");
  const [tone, setTone] = useState<Tone>(DEFAULT_TONE.tone);
  useEffect(() => {
    // A synchronous cleanup can run before the listen() promise resolves
    // (StrictMode double-mount). A `dead` flag unsubscribes a listener that
    // lands after teardown so no native listener leaks.
    let dead = false;
    const unsubs: Array<() => void> = [];
    invoke<Settings>("get_settings").then((s) => { setStyle(s.pillStyle || "classic"); setTone(toneFrom(s)); }).catch(() => {});
    listen<Settings>("settings", (e) => { setStyle(e.payload?.pillStyle || "classic"); setTone(toneFrom(e.payload)); }).then((u) => (dead ? u() : unsubs.push(u)));
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  return style === "yappy" ? <YappyPill tone={tone} /> : <ClassicPill />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Float />
  </React.StrictMode>,
);
