/**
 * ClassicPill — the original obsidian Dictate island (waveform capsule).
 * Restored as a selectable pill style. Waveform driven by a --level CSS var each
 * rAF frame (smoother than re-rendering 9 bars per audio event).
 */
import React, { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { Mic, MicOff, Square } from "lucide-react";
import {
  PHASE_ID, phaseVisual, progressFraction, progressLabel, progressNumeral,
  type LivePhase, type PhaseArtTable, type TranscribeProgress,
} from "./live";
import { GATED_GLYPH, GATED_TITLE, type PillLicense } from "./license";
import { usePillDrag, watchPillHitbox } from "./drag";
import {
  applyMotionVars, createSoftBody, DURATION, prefersReducedMotion, type SoftBody,
} from "./motion";
import MeetingBadge, { useMeetingStatus } from "./MeetingBadge";

/**
 * Y5-C — Classic's art for EVERY phase, as DATA.
 *
 * Classic is a DOM capsule, so its "sprite" is the element the capsule swaps in
 * (the mic glyph, the waveform, the determinate fill, the check) and its
 * "motion" is the CSS animation the capsule wears while the phase holds. Both
 * ride out as `data-sprite` / `data-motion` on the capsule, so float.css can
 * key a treatment off a phase without this component growing a branch per
 * phase — and so a phase with no art is a TYPE ERROR here rather than an
 * invisible state in a running app.
 *
 * It lives in this component on purpose (Y5-C's brief): Y5-K's first act is to
 * lift exactly this table, and Yappy's twin, into the character registry. Until
 * then nothing outside these two components may branch on the pill style.
 */
export const CLASSIC_PHASE_ART: PhaseArtTable = {
  idle: { sprite: "seed", motion: "breathe" },
  sleepy: { sprite: "seed", motion: "still" },
  listening: { sprite: "wave", motion: "pulse" },
  "model-loading": { sprite: "mic", motion: "breathe" },
  transcribing: { sprite: "xscribe", motion: "work" },
  polishing: { sprite: "spark", motion: "work" },
  pasting: { sprite: "caret", motion: "work" },
  thinking: { sprite: "dots", motion: "work" },
  done: { sprite: "check", motion: "still" },
  empty: { sprite: "hush", motion: "still" },
  cancelled: { sprite: "hush", motion: "still" },
  error: { sprite: "alert", motion: "shake" },
  gated: { sprite: "lock", motion: "still" },
  waiting: { sprite: "mic-off", motion: "breathe" },
  blocked: { sprite: "mic-off", motion: "still" },
};

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
export default function ClassicPill(
  { gate = "idle", progress = null }: {
    gate?: LivePhase;
    progress?: TranscribeProgress | null;
    /**
     * Y2-A — the license as the pill is allowed to say it, already decided by
     * `pillLicense` in float-main. Accepted here so the wiring is complete and
     * typed; the capsule that DRAWS it is Y2-B (classic) / Y2-C (yappy), and
     * this component deliberately does not read it yet.
     */
    license?: PillLicense | null;
  },
) {
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
  // Y5-D — the soft body. One squash scalar, integrated by the rAF loop below,
  // painted as the capsule's `scale`. Created ONCE per mount, and created
  // already-inert when the OS asks for less motion, so there is no per-frame
  // branch and no way for a reduced-motion pill to animate by accident.
  const softRef = useRef<SoftBody | null>(null);
  if (softRef.current === null) {
    softRef.current = createSoftBody({ reduceMotion: prefersReducedMotion() });
  }
  // YV65 — press-and-drag the capsule to re-dock the pill.
  // Y5-D — ...and the same gesture drives the physics: press squashes, a plain
  // release rebounds through the stretch, a release that DRAGGED lands a dock
  // bounce on the slower spring.
  const drag = usePillDrag({
    press: () => { softRef.current?.press(); wakeRef.current(); },
    release: () => { softRef.current?.release(); wakeRef.current(); },
    dock: () => { softRef.current?.dock(); wakeRef.current(); },
  });
  // YV95 — the pill is the always-visible recording indicator for a meeting.
  const meeting = useMeetingStatus();

  // YV65 — publish the capsule's rect so the panel only takes the cursor over
  // the pill itself; the transparent shadow margin stays click-through.
  useEffect(() => {
    const el = pillRef.current;
    return el ? watchPillHitbox(el) : undefined;
  }, []);

  useEffect(() => {
    // Y5-D — the duration table owns the pill's CSS timings too (float.css
    // spells them `var(--dur-expand) var(--ease-state)`), so publish it once.
    applyMotionVars(document.documentElement);
    // prefers-reduced-motion: paint one calm static frame (level 0, waveform at
    // rest, capsule at its OWN shape) instead of the per-frame animation loop.
    // Y5-D: the information the pill carries — phase, numeral, progress — is
    // markup and CSS, never motion, so the static frame is complete. The only
    // thing skipped here is movement.
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      pillRef.current?.style.setProperty("--level", "0");
      pillRef.current?.style.setProperty("--squash-x", "1");
      pillRef.current?.style.setProperty("--squash-y", "1");
      return;
    }
    let raf = 0;
    let running = false;
    let last = 0;
    const loop = (now: number) => {
      // Injected timestep — the same number motion.test.ts feeds the spring.
      const dt = last === 0 ? 1 / 60 : (now - last) / 1000;
      last = now;
      smoothRef.current += (levelRef.current - smoothRef.current) * 0.3;
      pillRef.current?.style.setProperty("--level", smoothRef.current.toFixed(3));
      const soft = softRef.current;
      const softRunning = soft ? soft.tick(dt) : false;
      if (soft) {
        const sc = soft.scale();
        pillRef.current?.style.setProperty("--squash-x", sc.x.toFixed(4));
        pillRef.current?.style.setProperty("--squash-y", sc.y.toFixed(4));
      }
      // Settle-and-park: once the level AND the soft body are both at rest,
      // stop scheduling frames so the always-on pill doesn't burn a 60fps rAF
      // forever while idle (audit [0], YV81). The audio_level listener and the
      // gesture callbacks re-arm the loop via wakeRef. A spring that never
      // settles would defeat this, which is why the integrator's rest contract
      // is asserted in motion.test.ts rather than assumed.
      if (!softRunning && levelRef.current < 0.002 && smoothRef.current < 0.002) {
        smoothRef.current = 0;
        pillRef.current?.style.setProperty("--level", "0");
        running = false;
        last = 0;
        return;
      }
      raf = requestAnimationFrame(loop);
    };
    const wake = () => {
      if (running) return;
      running = true;
      last = 0;
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
        doneTimer.current = window.setTimeout(() => setDone(false), DURATION.doneFlash);
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
  // Y2-C — the trial ended and this press was refused. A SEPARATE reason from
  // `blocked`, deliberately: the capsule looks equally stopped, but the fix is a
  // license and the click goes somewhere else entirely. `blocked` still outranks
  // it (see PHASE_PRECEDENCE) — `gate` can only hold one phase, and
  // `reduceGatePhase` already decided which.
  const gated = gate === "gated";
  const live = status.recording && !blocked && !gated;
  // Y3-C — a chunked decode is no longer "busy". `busy` is a boolean, so a
  // 15-minute take across 12 chunks showed one undifferentiated state for
  // minutes; `transcribing` is a first-class phase carrying real, measured
  // progress. A single-window take never reports any, so it stays `busy`:
  // there is nothing to show and a 1/1 flicker is worse than silence.
  const transcribing = progress !== null && !live && !blocked && !gated;
  const busy = status.busy && !live && !blocked && !gated && !transcribing;
  // A meeting expands the capsule for as long as it runs, and outranks the
  // resting "seed" — but never the live dictation state, which is the shorter,
  // more urgent thing on screen.
  const base = blocked
    ? `pill blocked ${gate}`
    : gated ? "pill blocked gated"
    : live ? "pill live"
      : transcribing ? "pill busy transcribing"
        : busy ? "pill busy" : done ? "pill done" : "pill";
  const cls = meeting.recording && !blocked && !gated ? `${base} meeting` : base;
  // Y5-C — the phase's art, drawn from this character's own table. Every phase
  // has a row, so this is never undefined and there is no fallback to forget.
  const art = CLASSIC_PHASE_ART[gate];
  const onToggle = useCallback(() => {
    // YV65 — the click that closes a drag must never start/stop dictation.
    if (drag.dragged()) return;
    if (gated) {
      // Y2-C — a gated capsule is a BUTTON to the purchase surface, not a dead
      // pill: raise the main window and land on Settings → License, which owns
      // the checkout link. No new command and no new capability — this is the
      // `navigate` / `settings-tab` plumbing the tray menu already uses.
      invoke("show_main").catch(() => {});
      emit("navigate", "settings").catch(() => {});
      emit("settings-tab", "license").catch(() => {});
      return;
    }
    if (blocked) {
      // PERM-C — a blocked capsule is a BUTTON to the fix, not a dead pill:
      // raise the main window and land on the Permissions screen (PERM-B), which
      // owns the System Settings deep link.
      invoke("show_main").catch(() => {});
      emit("navigate", "permissions").catch(() => {});
      return;
    }
    if (busy || transcribing) return;
    invoke("manual_toggle").catch(() => {});
  }, [blocked, gated, busy, transcribing, drag]);

  return (
    <div className="stage">
      <div
        ref={pillRef}
        className={cls}
        data-phase={PHASE_ID[gate]}
        data-sprite={art.sprite}
        data-motion={art.motion}
        role="button"
        tabIndex={0}
        aria-label={
          blocked ? visual.label
            : gated ? visual.label
            : live ? "Stop dictation"
              : progress ? progressLabel(progress)
                : busy ? "Transcribing" : "Start dictation"
        }
        style={progress ? ({ ["--progress" as string]: progressFraction(progress).toFixed(4) } as React.CSSProperties) : undefined}
        title={blocked ? visual.label : gated ? GATED_TITLE : undefined}
        {...drag.handlers}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onToggle(); }
        }}
      >
        <MeetingBadge status={meeting} />
        <button type="button" className="ctrl" tabIndex={-1} aria-hidden onClick={(e) => { e.stopPropagation(); onToggle(); }}>
          {blocked || gated ? <MicOff strokeWidth={2.2} /> : live ? <Square fill="currentColor" strokeWidth={0} /> : <Mic strokeWidth={2.2} />}
        </button>
        {blocked ? (
          <span className="gate-note">{gate === "waiting" ? "Allow the mic…" : "Mic blocked"}</span>
        ) : gated ? (
          // The trial-ended glyph plus the SAME headline the License card
          // shows; the body rides in the tooltip. Reused, never rewritten.
          <span className="gate-note">
            <i className="gate-glyph" aria-hidden>{GATED_GLYPH}</i>
            {visual.label}
          </span>
        ) : progress ? (
          // A DETERMINATE fill — never a spinner. The count is knowable now, and
          // the bar advances only on a chunk that actually completed: no easing,
          // no interpolation between events, because a smooth bar that is lying
          // is exactly the defect this replaces.
          <>
            <span className="xscribe-track" aria-hidden>
              <i className="xscribe-fill" />
            </span>
            <span className="xscribe-count" aria-hidden>{progressNumeral(progress)}</span>
          </>
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
