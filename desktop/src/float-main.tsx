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
import { acceptProgress, reduceGatePhase, type LivePhase, type TranscribeProgress } from "./pill/live";
import "./float.css";

interface Settings { pillStyle?: string; pillPosition?: string }

/**
 * PERM-C — the backend's refusal payload. `prompting` is true only while the
 * system dialog is on its way (NotDetermined), which is the difference between
 * "waiting" and "blocked" on the pill.
 */
interface MicGateNotice { status?: string; prompting?: boolean }

/**
 * Y3-C — the backend's per-chunk decode progress. Emitted ONCE per COMPLETED
 * chunk and never for a single-window take, so the mere arrival of one of these
 * is what puts the pill into the `transcribing` phase.
 */
interface ProgressNotice { chunk?: number; of?: number; words_so_far?: number }

const DOCKS = ["bottom", "left", "right"];
const dockOf = (s?: Settings) => {
  const p = s?.pillPosition || "bottom";
  return DOCKS.includes(p) ? p : "bottom";
};

function Float() {
  const [style, setStyle] = useState<string>("classic");
  // PERM-C — the permission phase lives HERE, above both pill styles, because it
  // is a property of the app and not of whichever capsule happens to be drawn.
  // Both pills receive it; neither invents it.
  const [gate, setGate] = useState<LivePhase>("idle");
  // Y3-C — decode progress lives HERE for the same reason `gate` does: it is a
  // property of the take, not of whichever capsule happens to be drawn. Both
  // pills receive it; neither invents it, and neither interpolates it.
  const [progress, setProgress] = useState<TranscribeProgress | null>(null);
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    listen<ProgressNotice>("transcribe_progress", (e) =>
      setProgress((p) =>
        acceptProgress(p, {
          chunk: Number(e.payload?.chunk),
          of: Number(e.payload?.of),
          wordsSoFar: Number(e.payload?.words_so_far),
        }),
      ),
    ).then(push);
    // A NEW take wipes the last take's progress, and a finished transcript ends
    // the phase. Without both, a stale "9/12" outlives the decode that made it.
    listen<boolean>("recording", (e) => { if (e.payload) setProgress(null); }).then(push);
    listen<unknown>("transcript", () => setProgress(null)).then(push);
    listen<unknown>("transcript_error", () => setProgress(null)).then(push);
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    // A press was refused for the microphone. The pill must SHOW it — an
    // invisible refusal is the bug PERM-C exists to fix.
    listen<MicGateNotice>("mic_permission_required", (e) =>
      setGate((p) =>
        reduceGatePhase(p, {
          type: "mic_permission_required",
          status: e.payload?.status ?? "denied",
          prompting: e.payload?.prompting,
        }),
      ),
    ).then(push);
    // TCC answered (the non-blocking request, or the Permissions pane polling).
    listen<string>("microphone-status", (e) =>
      setGate((p) => reduceGatePhase(p, { type: "microphone-status", status: e.payload })),
    ).then(push);
    // A take starting or stopping. Never clears a permission phase — see
    // `reduceGatePhase`.
    listen<boolean>("recording", (e) =>
      setGate((p) => reduceGatePhase(p, { type: "recording", recording: e.payload })),
    ).then(push);
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
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
  return style === "yappy"
    ? <YappyPill gate={gate} progress={progress} />
    : <ClassicPill gate={gate} progress={progress} />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Float />
  </React.StrictMode>,
);
