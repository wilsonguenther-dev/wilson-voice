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
  phaseVisual, progressFraction, progressLabel, progressNumeral,
  type LivePhase, type TranscribeProgress,
} from "./live";
import {
  GATED_GLYPH, GATED_TITLE, REVEAL_PURCHASE_COMMAND, SHOW_MAIN_COMMAND,
  type PillLicense,
} from "./license";
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
export default function ClassicPill(
  { gate = "idle", progress = null, license = null }: {
    gate?: LivePhase;
    progress?: TranscribeProgress | null;
    /**
     * Y2-A — the license as the pill is allowed to say it, already decided by
     * `pillLicense` in float-main. Y2-D READS it: `license.action` is what
     * decides where a press on the upgrade mark goes, and `license.show` is what
     * decides whether that mark exists at all. The capsule never re-derives
     * either — a licensed install passes `show: false` and the mark is simply
     * not rendered, which is why a paid-up pill has no dead click region.
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
  /**
   * Y2-D — where a license-driven press goes, decided by `pillLicense`'s
   * `action` and by nothing in this component.
   *
   *   `purchase` → `reveal_purchase_prompt`: the main window comes forward and
   *                the EXISTING PurchasePrompt sheet rises. The float window
   *                does not open a browser and never sees a URL.
   *   `license`  → the re-activate box (Y2-F's `problem` tone: somebody who
   *                already PAID and whose key stopped granting). Showing that
   *                person a price is the single most expensive sentence this app
   *                can say, so the two routes are kept physically apart.
   *   `none`     → nothing. A licensed pill is silent and inert.
   */
  const openLicenseSurface = useCallback(() => {
    // YV65 — a press that only closed a drag is not a press on the mark.
    if (drag.dragged()) return;
    if (!license?.show) return;
    if (license.action === "purchase") {
      invoke(REVEAL_PURCHASE_COMMAND).catch(() => {});
      return;
    }
    if (license.action === "license") {
      invoke(SHOW_MAIN_COMMAND).catch(() => {});
      emit("navigate", "settings").catch(() => {});
      emit("settings-tab", "license").catch(() => {});
    }
  }, [license, drag]);

  const onToggle = useCallback(() => {
    // YV65 — the click that closes a drag must never start/stop dictation.
    if (drag.dragged()) return;
    if (gated) {
      // Y2-D — a gated capsule is a BUTTON to the purchase SHEET, not to the
      // Settings tab it used to land on: the sheet is where the price, the
      // founding code, the seats line and the keep-forever promise live, and
      // Settings → License made a person hunt for them. Routed through the same
      // policy-driven handler the upgrade mark uses, so the whole capsule and
      // the mark can never disagree about where a press goes.
      if (license?.show) { openLicenseSurface(); return; }
      // The gate phase says "gated" but no license payload has arrived yet
      // (boot, or a dropped event). Fall back to the surface that is correct
      // under every license state rather than guessing at a purchase.
      invoke(SHOW_MAIN_COMMAND).catch(() => {});
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
  }, [blocked, gated, busy, transcribing, drag, license, openLicenseSurface]);

  return (
    <div className="stage">
      <div
        ref={pillRef}
        className={cls}
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
        {/* ── Y2-D · the upgrade affordance ──
            Rendered ONLY when the policy says `show`, so a licensed pill has no
            dead click region to hunt for. It lives INSIDE the capsule, which is
            the element `watchPillHitbox` publishes (YV65), so it shares that one
            hitbox: the transparent shadow margin around the pill stays
            click-through and the panel still only takes the cursor over the
            capsule itself.

            Both handlers stop propagation, and that is the point of the item:
            `pointerdown` so a press on the mark never starts a re-dock DRAG, and
            `click` so it never reaches `onToggle` and starts a DICTATION. There
            is no hover handler anywhere on it — passing the cursor over the pill
            must not pull focus off whatever the person is typing into. */}
        {license?.show && license.action !== "none" && !blocked && !gated ? (
          <button
            type="button"
            className={`pill-upgrade lic-${license.tone}`}
            title={license.title}
            aria-label={license.title}
            tabIndex={-1}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => { e.stopPropagation(); openLicenseSurface(); }}
          >
            <i className="pill-upgrade-glyph" aria-hidden>{license.glyph}</i>
            {license.value ? <span className="pill-upgrade-value">{license.value}</span> : null}
          </button>
        ) : null}
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
