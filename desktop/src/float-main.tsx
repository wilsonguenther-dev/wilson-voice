/**
 * Yap — "Kami" floating companion (separate MPA entry).
 * A folded-paper origami creature (Canvas2D hinge fold, see pill/fold.ts) that
 * peeks at rest, unfolds while listening with a beak driven by the live mic
 * level (pill/mouth.ts), chews while transcribing, then yaps a word-count line
 * (pill/tone.ts) from a speech bubble and folds back — the exact reverse.
 */
import React, { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { PETS, renderPet } from "./pill/fold";
import { MouthDriver } from "./pill/mouth";
import { reactiveLine, DEFAULT_TONE } from "./pill/tone";
import "./float.css";

interface AppStatus { recording: boolean; busy: boolean; message: string }
interface Transcript { wordCount: number; text: string }

type Phase = "idle" | "listening" | "thinking" | "done";
const FOLD: Record<Phase, number> = { idle: 0.08, listening: 1.0, thinking: 0.82, done: 1.0 };

function Kami() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [line, setLine] = useState("");
  const phaseRef = useRef<Phase>("idle");
  const tRef = useRef(0.08);
  const levelRef = useRef(0);
  const mouthRef = useRef(new MouthDriver());
  const chewRef = useRef(0);
  const doneUntil = useRef(0);
  const doneTimer = useRef<number | null>(null);
  const bubbleTimer = useRef<number | null>(null);

  // ── render loop: paint the fold every frame from a single scalar t ──
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const DPR = Math.min(devicePixelRatio || 1, 2);
    let W = 0, H = 0;
    const resize = () => {
      const r = canvas.getBoundingClientRect();
      W = r.width; H = r.height;
      canvas.width = W * DPR; canvas.height = H * DPR;
      ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    let raf = 0, last = 0;
    const loop = (ts: number) => {
      if (!last) last = ts;
      const dt = Math.min(0.05, (ts - last) / 1000);
      last = ts;
      const ph = phaseRef.current;
      chewRef.current += dt;
      const chew = ph === "thinking" ? Math.sin(chewRef.current * 7) * 0.06 : 0;
      const target = FOLD[ph] + chew;
      tRef.current += Math.sign(target - tRef.current) * Math.min(Math.abs(target - tRef.current), 2.8 * dt);
      const rms = ph === "listening" ? levelRef.current : 0;
      const mouthOpen = mouthRef.current.push(rms, dt);
      ctx.clearRect(0, 0, W, H);
      renderPet(ctx, W, H, PETS.crane, Math.max(0, tRef.current), mouthOpen, {});
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => { cancelAnimationFrame(raf); ro.disconnect(); };
  }, []);

  // ── backend events → phase + mouth + reactive line ──
  useEffect(() => {
    const unsubs: Array<() => void> = [];
    const setPhase = (p: Phase) => { phaseRef.current = p; };
    // status idle transitions must not stomp the brief "done" celebration.
    const toIdleIfClear = () => { if (Date.now() > doneUntil.current) setPhase("idle"); };

    invoke<AppStatus>("get_status")
      .then((s) => setPhase(s.recording ? "listening" : s.busy ? "thinking" : "idle"))
      .catch(() => {});

    listen<AppStatus>("status", (e) => {
      const s = e.payload;
      if (s.recording) setPhase("listening");
      else if (s.busy) setPhase("thinking");
      else toIdleIfClear();
    }).then((u) => unsubs.push(u));

    listen<boolean>("recording", (e) => {
      if (e.payload) { setPhase("listening"); mouthRef.current.reset(); }
    }).then((u) => unsubs.push(u));

    listen<number>("audio_level", (e) => {
      const v = typeof e.payload === "number" ? e.payload : 0;
      levelRef.current = Math.max(0, Math.min(1, v));
    }).then((u) => unsubs.push(u));

    listen<Transcript>("transcript", (e) => {
      const wc = e.payload?.wordCount ?? 0;
      const l = reactiveLine(wc, DEFAULT_TONE, wc); // nonce=wc → varies with length
      setLine(l);
      setPhase("done");
      doneUntil.current = Date.now() + 1400;
      if (bubbleTimer.current) clearTimeout(bubbleTimer.current);
      bubbleTimer.current = window.setTimeout(() => setLine(""), 2600);
      if (doneTimer.current) clearTimeout(doneTimer.current);
      doneTimer.current = window.setTimeout(() => { if (phaseRef.current === "done") setPhase("idle"); }, 1400);
    }).then((u) => unsubs.push(u));

    return () => unsubs.forEach((u) => u());
  }, []);

  const onToggle = () => { invoke("manual_toggle").catch(() => {}); };

  return (
    <div className="kami-stage">
      <div className={"kami-bubble" + (line ? " show" : "")}>{line}</div>
      <canvas
        ref={canvasRef}
        className="kami-canvas"
        role="button"
        tabIndex={0}
        aria-label="Toggle dictation"
        onClick={onToggle}
        onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onToggle(); } }}
      />
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Kami />
  </React.StrictMode>,
);
