/**
 * ClassicPill — the original obsidian Dictate island (waveform capsule).
 * Restored as a selectable pill style. Waveform driven by a --level CSS var each
 * rAF frame (smoother than re-rendering 9 bars per audio event).
 */
import React, { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { Mic, MicOff, Square } from "lucide-react";
import { phaseVisual, type LivePhase } from "./live";
import { usePillDrag, watchPillHitbox } from "./drag";
import MeetingBadge, { useMeetingStatus } from "./MeetingBadge";

interface AppStatus {
  recording: boolean;
  busy: boolean;
  message: string;
}

const BARS = Array.from({ length: 9 }, (_, i) => i);

/**
 * PERM-C — `gate` is the microphone permission phase, owned by `float-main` and
 * passed down: "blocked" (macOS is refusing, and will not re-prompt) or
 * "waiting" (the system dialog is up). It OUTRANKS every take state, because a
 * refused press must not look like a normal recording.
 */
export default function ClassicPill({ gate = "idle" }: { gate?: LivePhase }) {
  const [status, setStatus] = useState<AppStatus>({ recording: false, busy: false, message: "Ready" });
  const [done, setDone] = useState(false);
  const levelRef = useRef(0);
  const smoothRef = useRef(0);
  const pillRef = useRef<HTMLDivElement | null>(null);
  const prevBusy = useRef(false);
  const doneTimer = useRef<number | null>(null);
  // Set by the rAF effect; called by the audio_level listener to re-arm the
  // smoothing loop after it parks itself at rest (audit [0]).
  const wakeRef = useRef<() => void>(() => {});
  // YV65 — press-and-drag the capsule to re-dock the pill.
  const drag = usePillDrag();
  // YV95 — the pill is the always-visible recording indicator for a meeting.
  const meeting = useMeetingStatus();

  // YV65 — publish the capsule's rect so the panel only takes the cursor over
  // the pill itself; the transparent shadow margin stays click-through.
  useEffect(() => {
    const el = pillRef.current;
    return el ? watchPillHitbox(el) : undefined;
  }, []);

  useEffect(() => {
    // prefers-reduced-motion: paint one calm static frame (level 0, waveform at
    // rest) instead of the per-frame --level animation loop.
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      pillRef.current?.style.setProperty("--level", "0");
      return;
    }
    let raf = 0;
    let running = false;
    const loop = () => {
      smoothRef.current += (levelRef.current - smoothRef.current) * 0.3;
      pillRef.current?.style.setProperty("--level", smoothRef.current.toFixed(3));
      // Settle-and-park: once both the target level and the smoothed value are
      // at rest, stop scheduling frames so the always-on pill doesn't burn a
      // 60fps rAF forever while idle (audit [0]). The audio_level listener
      // re-arms the loop via wakeRef when new audio arrives.
      if (levelRef.current < 0.002 && smoothRef.current < 0.002) {
        smoothRef.current = 0;
        pillRef.current?.style.setProperty("--level", "0");
        running = false;
        return;
      }
      raf = requestAnimationFrame(loop);
    };
    const wake = () => {
      if (running) return;
      running = true;
      raf = requestAnimationFrame(loop);
    };
    wakeRef.current = wake;
    wake(); // paint down to rest once, then park
    return () => {
      cancelAnimationFrame(raf);
      wakeRef.current = () => {};
    };
  }, []);

  useEffect(() => {
    invoke<AppStatus>("get_status").then(setStatus).catch(() => {});
    // A synchronous cleanup can run before these listen() promises resolve
    // (StrictMode double-mount). A `dead` flag unsubscribes any listener that
    // lands after teardown so no native listener leaks.
    let dead = false;
    const unsubs: Array<() => void> = [];
    const apply = (s: AppStatus) => {
      setStatus(s);
      if (!s.recording) levelRef.current = 0;
      if (prevBusy.current && !s.busy && !s.recording) {
        setDone(true);
        if (doneTimer.current) clearTimeout(doneTimer.current);
        doneTimer.current = window.setTimeout(() => setDone(false), 1100);
      }
      prevBusy.current = s.busy;
    };
    listen<AppStatus>("status", (e) => apply(e.payload)).then((u) => (dead ? u() : unsubs.push(u)));
    listen<boolean>("recording", (e) =>
      setStatus((s) => {
        if (!e.payload) levelRef.current = 0;
        return { ...s, recording: e.payload };
      }),
    ).then((u) => (dead ? u() : unsubs.push(u)));
    listen<number>("audio_level", (e) => {
      const v = typeof e.payload === "number" ? e.payload : 0;
      levelRef.current = Math.max(0, Math.min(1, v));
      if (levelRef.current > 0.002) wakeRef.current(); // re-arm the smoothing loop
    }).then((u) => (dead ? u() : unsubs.push(u)));
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
      // YV73 — the "done" flash timer is armed from inside the status listener,
      // so it has to be disarmed with it: a pending timeout would otherwise
      // outlive the unmount and setState into a dead component.
      if (doneTimer.current) {
        clearTimeout(doneTimer.current);
        doneTimer.current = null;
      }
    };
  }, []);

  // PERM-C — a permission refusal outranks the take states below it: the whole
  // point is that a press which cannot record NEVER paints as one that can.
  const visual = phaseVisual(gate);
  const blocked = visual.needsPermission;
  const live = status.recording && !blocked;
  const busy = status.busy && !live && !blocked;
  // A meeting expands the capsule for as long as it runs, and outranks the
  // resting "seed" — but never the live dictation state, which is the shorter,
  // more urgent thing on screen.
  const base = blocked
    ? `pill blocked ${gate}`
    : live ? "pill live" : busy ? "pill busy" : done ? "pill done" : "pill";
  const cls = meeting.recording && !blocked ? `${base} meeting` : base;
  const onToggle = useCallback(() => {
    // YV65 — the click that closes a drag must never start/stop dictation.
    if (drag.dragged()) return;
    if (blocked) {
      // PERM-C — a blocked capsule is a BUTTON to the fix, not a dead pill:
      // raise the main window and land on the Permissions screen (PERM-B), which
      // owns the System Settings deep link.
      invoke("show_main").catch(() => {});
      emit("navigate", "permissions").catch(() => {});
      return;
    }
    if (busy) return;
    invoke("manual_toggle").catch(() => {});
  }, [blocked, busy, drag]);

  return (
    <div className="stage">
      <div
        ref={pillRef}
        className={cls}
        role="button"
        tabIndex={0}
        aria-label={blocked ? visual.label : live ? "Stop dictation" : busy ? "Transcribing" : "Start dictation"}
        title={blocked ? visual.label : undefined}
        {...drag.handlers}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onToggle(); }
        }}
      >
        <MeetingBadge status={meeting} />
        <button type="button" className="ctrl" tabIndex={-1} aria-hidden onClick={(e) => { e.stopPropagation(); onToggle(); }}>
          {blocked ? <MicOff strokeWidth={2.2} /> : live ? <Square fill="currentColor" strokeWidth={0} /> : <Mic strokeWidth={2.2} />}
        </button>
        {blocked ? (
          <span className="gate-note">{gate === "waiting" ? "Allow the mic…" : "Mic blocked"}</span>
        ) : done ? (
          <svg className="check" viewBox="0 0 24 24" aria-hidden><path d="M4 12.5l5 5L20 6.5" /></svg>
        ) : (
          <>
            <span className="seed" aria-hidden />
            <div className="wave" aria-hidden>
              {BARS.map((i) => (
                <i key={i} style={{ ["--i" as string]: String(i) } as React.CSSProperties} />
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
