/**
 * Wilson Voice — floating Dictate island (separate MPA entry).
 * Solid obsidian pill; waveform driven by a --level CSS var (set each rAF frame,
 * smoother than re-rendering 9 bars per audio event).
 */
import React, { useCallback, useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Mic, Square } from "lucide-react";
import "./float.css";

interface AppStatus {
  recording: boolean;
  busy: boolean;
  message: string;
}

const BARS = Array.from({ length: 9 }, (_, i) => i);

function FloatPill() {
  const [status, setStatus] = useState<AppStatus>({
    recording: false,
    busy: false,
    message: "Ready",
  });
  const [done, setDone] = useState(false);
  const levelRef = useRef(0); // raw 0..1 from audio_level
  const smoothRef = useRef(0); // smoothed value we paint
  const pillRef = useRef<HTMLDivElement | null>(null);
  const prevBusy = useRef(false);
  const doneTimer = useRef<number | null>(null);

  // Paint the waveform via a CSS var each frame — no React re-render per bar.
  useEffect(() => {
    let raf = 0;
    const loop = () => {
      smoothRef.current += (levelRef.current - smoothRef.current) * 0.3;
      pillRef.current?.style.setProperty("--level", smoothRef.current.toFixed(3));
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);

  useEffect(() => {
    invoke<AppStatus>("get_status").then(setStatus).catch(() => {});

    const unsubs: Array<() => void> = [];
    const apply = (s: AppStatus) => {
      setStatus(s);
      if (!s.recording) levelRef.current = 0;
      // Flash "done" (checkmark) when transcription finishes: busy true → false.
      if (prevBusy.current && !s.busy && !s.recording) {
        setDone(true);
        if (doneTimer.current) clearTimeout(doneTimer.current);
        doneTimer.current = window.setTimeout(() => setDone(false), 1100);
      }
      prevBusy.current = s.busy;
    };

    listen<AppStatus>("status", (e) => apply(e.payload)).then((u) => unsubs.push(u));
    listen<boolean>("recording", (e) =>
      setStatus((s) => {
        if (!e.payload) levelRef.current = 0;
        return { ...s, recording: e.payload };
      }),
    ).then((u) => unsubs.push(u));
    listen<number>("audio_level", (e) => {
      const v = typeof e.payload === "number" ? e.payload : 0;
      levelRef.current = Math.max(0, Math.min(1, v));
    }).then((u) => unsubs.push(u));

    return () => unsubs.forEach((u) => u());
  }, []);

  const live = status.recording;
  const busy = status.busy && !live;
  const cls = live ? "pill live" : busy ? "pill busy" : done ? "pill done" : "pill";

  const onToggle = useCallback(() => {
    if (busy) return;
    invoke("manual_toggle").catch(() => {});
  }, [busy]);

  return (
    <div className="stage">
      <div
        ref={pillRef}
        className={cls}
        role="button"
        tabIndex={0}
        aria-label={live ? "Stop dictation" : busy ? "Transcribing" : "Start dictation"}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onToggle();
          }
        }}
      >
        <button
          type="button"
          className="ctrl"
          tabIndex={-1}
          aria-hidden
          onClick={(e) => {
            e.stopPropagation();
            onToggle();
          }}
        >
          {live ? <Square fill="currentColor" strokeWidth={0} /> : <Mic strokeWidth={2.2} />}
        </button>

        {done ? (
          <svg className="check" viewBox="0 0 24 24" aria-hidden>
            <path d="M4 12.5l5 5L20 6.5" />
          </svg>
        ) : (
          <div className="wave" aria-hidden>
            {BARS.map((i) => (
              <i key={i} style={{ ["--i" as string]: String(i) } as React.CSSProperties} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <FloatPill />
  </React.StrictMode>,
);
