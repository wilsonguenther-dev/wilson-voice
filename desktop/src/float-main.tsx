/**
 * Wilson Voice — floating Dictate island (separate MPA entry).
 * Design: VoiceInk-compact + OpenWispr glass — not a dark folder chip.
 */
import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Mic, Square, Loader2 } from "lucide-react";
import "./float.css";

interface AppStatus {
  recording: boolean;
  busy: boolean;
  message: string;
}

const BAR_COUNT = 9;

function Waveform({
  level,
  live,
  busy,
}: {
  level: number;
  live: boolean;
  busy: boolean;
}) {
  // Smooth multi-bar shape around center peak
  const heights = useMemo(() => {
    const bars: number[] = [];
    const base = live ? Math.max(0.08, level) : busy ? 0.35 : 0.12;
    for (let i = 0; i < BAR_COUNT; i++) {
      const dist = Math.abs(i - (BAR_COUNT - 1) / 2) / ((BAR_COUNT - 1) / 2);
      const shape = 1 - dist * 0.72;
      // slight left-right phase so it feels organic
      const wobble = live
        ? 0.85 + 0.15 * Math.sin(Date.now() / 90 + i * 0.7)
        : 1;
      const h = Math.min(1, base * shape * wobble);
      bars.push(4 + h * 16); // 4..20 px
    }
    return bars;
  }, [level, live, busy]);

  return (
    <div className="wave" aria-hidden>
      {heights.map((h, i) => (
        <span key={i} style={{ height: `${h}px` }} />
      ))}
    </div>
  );
}

function FloatPill() {
  const [status, setStatus] = useState<AppStatus>({
    recording: false,
    busy: false,
    message: "Ready",
  });
  const [level, setLevel] = useState(0);
  const levelRef = useRef(0);
  const rafRef = useRef<number | null>(null);

  // Smooth level interpolation for buttery bars
  useEffect(() => {
    const tick = () => {
      const target = levelRef.current;
      setLevel((prev) => {
        const next = prev + (target - prev) * 0.35;
        return Math.abs(next - target) < 0.01 ? target : next;
      });
      rafRef.current = requestAnimationFrame(tick);
    };
    rafRef.current = requestAnimationFrame(tick);
    return () => {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
    };
  }, []);

  useEffect(() => {
    invoke<AppStatus>("get_status")
      .then((s) =>
        setStatus({
          recording: s.recording,
          busy: s.busy,
          message: s.message,
        }),
      )
      .catch(() => {});

    const unsubs: Array<() => void> = [];
    listen<AppStatus>("status", (e) => {
      setStatus({
        recording: e.payload.recording,
        busy: e.payload.busy,
        message: e.payload.message,
      });
      if (!e.payload.recording) {
        levelRef.current = 0;
      }
    }).then((u) => unsubs.push(u));

    listen<boolean>("recording", (e) => {
      setStatus((s) => ({
        ...s,
        recording: e.payload,
        message: e.payload ? "Listening…" : s.message,
      }));
      if (!e.payload) levelRef.current = 0;
    }).then((u) => unsubs.push(u));

    // 0..1 from Rust (audio_level event)
    listen<number>("audio_level", (e) => {
      const v = typeof e.payload === "number" ? e.payload : 0;
      levelRef.current = Math.max(0, Math.min(1, v));
    }).then((u) => unsubs.push(u));

    return () => unsubs.forEach((u) => u());
  }, []);

  const live = status.recording;
  const busy = status.busy && !live;
  const pillClass = live ? "pill live" : busy ? "pill busy" : "pill";

  const onToggle = useCallback(() => {
    if (busy) return;
    invoke("manual_toggle").catch(() => {});
  }, [busy]);

  const onKey = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        onToggle();
      }
    },
    [onToggle],
  );

  return (
    <div className="stage">
      <div
        className={pillClass}
        role="button"
        tabIndex={0}
        aria-label={
          live ? "Stop dictation" : busy ? "Transcribing" : "Start dictation"
        }
        data-tauri-drag-region
        onClick={onToggle}
        onKeyDown={onKey}
      >
        <button
          type="button"
          className="ctrl"
          disabled={busy}
          tabIndex={-1}
          aria-hidden
          onClick={(e) => {
            e.stopPropagation();
            onToggle();
          }}
        >
          {live ? (
            <Square fill="currentColor" strokeWidth={0} />
          ) : busy ? (
            <Loader2 />
          ) : (
            <Mic />
          )}
        </button>

        <Waveform level={level} live={live} busy={busy} />

        <span className="meta">{live ? "fn" : busy ? "…" : "fn"}</span>
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <FloatPill />
  </React.StrictMode>,
);
