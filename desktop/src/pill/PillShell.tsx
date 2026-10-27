/**
 * PillShell — the pill's SHELL (Y5-K), ported from ClassicPill.
 *
 * Everything that is not the creature lives here, once, for every character:
 * the capsule chrome, the slots, the box from dock-geometry.json, hover
 * hysteresis, the drag gesture and its soft body, the license chip, the gate
 * note, the determinate progress fill and the aria names. The CHARACTER is
 * handed a frame (pill/characters/types.ts) and paints inside the body slot;
 * it is resolved from `pill_style` once, by float-main, never by this file.
 * Waveform level rides a --level CSS var each rAF frame.
 */
import React, { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import {
  DEFAULT_STOP_KEY, isListening, PHASE_ID, phaseAccessibleName, phaseVisual, progressFraction, progressNumeral,
  type ChatTone, type LivePhase, type TranscribeProgress,
} from "./live";
import {
  GATED_GLYPH, GATED_TITLE, REVEAL_PURCHASE_COMMAND, pillLicenseRoute,
  type PillLicense,
} from "./license";
import { usePillDrag, watchPillHitbox } from "./drag";
import {
  collapseDueIn, hotRect, initialHover, stepHover, PILL_HOVER_OUT_EVENT,
  type HoverInput, type HoverPhase, type HoverState, type Rect,
} from "./hitbox";
import {
  applyMotionVars, createSoftBody, DURATION, prefersReducedMotion, type SoftBody,
} from "./motion";
import MeetingBadge from "./MeetingBadge";
import LanguageChip from "./LanguageChip";
import PhaseAnnouncer from "./PhaseAnnouncer";
import { IDLE_MEETING, type MeetingAfterView, type MeetingStatus } from "./meeting";
import { barBox, barDock, shellBarPhase, waveAxis } from "./dock";
import { resolveCharacter } from "./characters/registry";
import type { Character, CharacterFrame, CharacterPaint } from "./characters/types";
import { FlowBarSlots, layoutSlots, slotById, tooltipSide, type SlotCtx } from "./slots";

/** Y8-B — the centre slot: the capsule's own mic/stop control is the Dictate slot. */
const DICTATE = slotById("dictate");

interface AppStatus {
  recording: boolean;
  busy: boolean;
  message: string;
}

/**
 * PERM-C — `gate` is the microphone permission phase, owned by `float-main` and
 * passed down: "blocked" (macOS is refusing, and will not re-prompt) or
 * "waiting" (the system dialog is up). It OUTRANKS every take state, because a
 * refused press must not look like a normal recording.
 */
export default function ClassicPill(
  { gate = "idle", chordLabel = DEFAULT_STOP_KEY, progress = null, meeting = IDLE_MEETING, meetingAfter = null, onMeetingOpen, license = null, slots = null, character = resolveCharacter(), tone = "friendly" }: {
    gate?: LivePhase;
    /** Y5-K — the creature, resolved from `pill_style` ONCE by float-main. Absent: classic. */
    character?: Character;
    /** Y5-K — the companion tone, decided by the shell (float-main) and handed down in the frame. */
    tone?: ChatTone;
    /**
     * yap24-PILL4 — the key that ends a hands-free take (`AppStatus.chord_label`),
     * decided by the SHELL. Only the accessible name and the announcement read
     * it here; the visible stop hint and lock chip are the shell's.
     */
    chordLabel?: string;
    /**
     * Y8-B — the flow-bar slot context, decided by the SHELL (float-main).
     * `null` (a test mount, the dev matrix) draws the capsule with only its
     * centre slot, at the bottom dock.
     */
    slots?: SlotCtx | null;
    progress?: TranscribeProgress | null;
    /**
     * YV95 / yap24-NT7 — the meeting, owned by the SHELL (float-main) like
     * `gate` and `progress`: the live status, and the post-stop view already
     * ranked below dictation. This capsule draws them; it decides neither.
     */
    meeting?: MeetingStatus;
    meetingAfter?: MeetingAfterView | null;
    onMeetingOpen?: (id: string) => void;
    /**
     * Y2-A — the license as the pill is allowed to say it, already decided by
     * `pillLicense` in float-main. Y2-D READS it, through `pillLicenseRoute`:
     * the route decides where a press on the upgrade mark (or on a gated
     * capsule) goes, and whether that mark exists at all. The capsule never
     * re-derives either — a licensed install passes `show: false`, the route
     * is null and the mark is simply not rendered, which is why a paid-up pill
     * has no dead click region.
     */
    license?: PillLicense | null;
  },
) {
  const [status, setStatus] = useState<AppStatus>({ recording: false, busy: false, message: "Ready" });
  const [done, setDone] = useState(false);
  const levelRef = useRef(0);
  const smoothRef = useRef(0);
  const pillRef = useRef<HTMLDivElement | null>(null);
  // Y5-E — the hot area wraps the capsule: its layout box IS the capsule's
  // (untransformed by the squash), and its `::before` paints the alpha margin
  // while hover-expanded. Measured for both the hover reducer and the hitbox.
  const hotElRef = useRef<HTMLDivElement | null>(null);
  const [hover, setHover] = useState<HoverPhase>("collapsed");
  const hoverRef = useRef<HoverState>(initialHover());
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

  // YV65 — publish the capsule's rect so the panel only takes the cursor over
  // the pill itself; the transparent shadow margin stays click-through.
  // Y5-E — ...and while hover-expanded, the capsule PLUS the alpha margin
  // float.css paints, through the same `hotRect` the reducer tests against. The
  // hover machine: expand the moment the cursor is on the capsule, collapse only
  // once it has been outside the EXPANDED hot rect for `HOVER.collapseAfterMs`.
  useEffect(() => {
    const el = hotElRef.current;
    if (!el) return undefined;
    const viewport = (): Rect => ({ x: 0, y: 0, w: window.innerWidth, h: window.innerHeight });
    const capsule = (): Rect => {
      const b = el.getBoundingClientRect();
      return { x: b.left, y: b.top, w: b.width, h: b.height };
    };
    const watch = watchPillHitbox(el, (r) => hotRect(r, hoverRef.current.phase, viewport()));
    let timer: number | null = null;
    let dead = false;
    let unlisten: (() => void) | null = null;
    const feed = (input: HoverInput) => {
      if (dead) return;
      const prev = hoverRef.current;
      const next = stepHover(prev, input, capsule(), viewport());
      hoverRef.current = next;
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
      // The debounce's clock: nothing else may arrive once the cursor is gone
      // (the panel is click-through again), so the collapse is a timer, not a
      // wait for the next pointer event.
      const due = collapseDueIn(next, input.at);
      if (due !== null) {
        timer = window.setTimeout(() => {
          timer = null;
          feed({ at: performance.now() });
        }, due);
      }
      if (next.phase !== prev.phase) {
        setHover(next.phase);
        watch.refresh();
      }
    };
    const onMove = (e: PointerEvent) => feed({ at: performance.now(), x: e.clientX, y: e.clientY });
    const onGone = () => feed({ at: performance.now(), gone: true });
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerdown", onMove);
    document.documentElement.addEventListener("pointerleave", onGone);
    // The backend owns click-through: once the cursor leaves the published
    // hitbox the webview stops getting pointer events, so it says so instead.
    listen(PILL_HOVER_OUT_EVENT, onGone)
      .then((u) => (dead ? u() : (unlisten = u)))
      .catch(() => {});
    return () => {
      dead = true;
      if (timer !== null) clearTimeout(timer);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerdown", onMove);
      document.documentElement.removeEventListener("pointerleave", onGone);
      unlisten?.();
      watch.stop();
    };
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
  const visual = phaseVisual(gate, chordLabel);
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
  // yap24-NT7 — the post-stop phases (processing / notes-ready / failed) wear
  // the same expanded capsule; `meetingAfter` is already null whenever a
  // dictation is on screen, so the take always outranks it.
  const cls = (meeting.recording || meetingAfter !== null) && !blocked && !gated ? `${base} meeting` : base;
  /**
   * Y2-D — where a license-driven press goes, decided by `pillLicenseRoute`
   * (i.e. by `pillLicense().action`) and by nothing in this component.
   *
   *   reveal_purchase → `reveal_purchase_prompt`: the main window comes forward
   *                     and the EXISTING PurchasePrompt sheet rises. The float
   *                     window opens no browser and never sees a URL.
   *   license_tab     → the re-activate box (Y2-F's `problem` tone: somebody who
   *                     already PAID). Also the fallback for a gated capsule
   *                     whose license payload has not arrived yet (boot, or a
   *                     dropped event): it is correct under every license state,
   *                     a purchase is not.
   */
  const licenseRoute = pillLicenseRoute(license);
  /*
   * Deliberately NO drag guard here. The upgrade mark calls this directly, and
   * the mark cannot start or close a drag: its `pointerdown` stops before the
   * capsule's drag handler sees it. That same stop means a press on the mark
   * never resets `usePillDrag`'s `moved` flag, so after any re-dock
   * `drag.dragged()` stays true until the next press on the capsule body.
   * Consulting it here made the mark dead after a drag (Y2-D review BLOCK on
   * a01f20d). The capsule's own click keeps its YV65 guard in `onToggle`,
   * which runs before it reaches this.
   */
  const openLicenseSurface = useCallback(() => {
    if (licenseRoute === "reveal_purchase") {
      invoke(REVEAL_PURCHASE_COMMAND).catch(() => {});
      return;
    }
    invoke("show_main").catch(() => {});
    emit("navigate", "settings").catch(() => {});
    emit("settings-tab", "license").catch(() => {});
  }, [licenseRoute]);
  const onToggle = useCallback(() => {
    // YV65 — the click that closes a drag must never start/stop dictation.
    if (drag.dragged()) return;
    if (gated) {
      // Y2-C/D — a gated capsule is a BUTTON to the purchase SHEET, not a dead
      // pill. Routed through the same policy-driven handler the upgrade mark
      // uses, so the capsule and the mark can never disagree about where a
      // press goes. (The drag guard above already ran for this path.)
      openLicenseSurface();
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
  }, [blocked, gated, busy, transcribing, drag, openLicenseSurface]);
  // Y8-B — the bar's slots. The capsule owns WHERE they sit (lead · centre ·
  // trail along the length axis); `pill/slots.tsx` owns whether each is shown.
  const slotCtx: SlotCtx = slots ?? {
    phase: gate,
    // No `document` under a server render (ClassicPill.mark.test): bottom dock.
    dock: barDock(typeof document === "undefined" ? undefined : document.documentElement.dataset.dock),
    meetingOnScreen: meeting.recording || meetingAfter !== null,
    progress,
    listen: { estWords: 0, sec: 0 },
    scratchpadOnBar: false,
  };
  const barAxis = layoutSlots(slotCtx).axis;
  const tipSide = tooltipSide(slotCtx.dock);
  const slotActions = { toggleDictation: onToggle };
  // Y5-K — the FRAME: what the shell already computed, handed to the character.
  // The box is this phase's row of dock-geometry.json at this dock (Y5-I); the
  // dock reaches the character only as an axis hint.
  const shellPhase = shellBarPhase(gate, meeting.recording || meetingAfter !== null);
  const bb = barBox(shellPhase, slotCtx.dock);
  const frame: CharacterFrame = {
    phase: gate,
    tone,
    level: levelRef.current,
    box: { w: bb.width, h: bb.height },
    dock: waveAxis(shellPhase, slotCtx.dock),
    reducedMotion: prefersReducedMotion(),
    chordLabel,
  };
  // A character with nothing for a frame never blanks the pill: classic paints it.
  const paintOf = (f: CharacterFrame): CharacterPaint =>
    character.render(f) ?? (resolveCharacter().render(f) as CharacterPaint);
  const paint = paintOf(frame);
  const art = paint.art;
  // The take-landed flash is the shell's timer; the creature only draws `done`.
  const body = done && gate !== "done" ? paintOf({ ...frame, phase: "done" }).node : paint.node;

  return (
    <div className="stage">
      <PhaseAnnouncer phase={gate} chordLabel={chordLabel} />
      <div ref={hotElRef} className="pill-hot" data-hover={hover}>
      <div
        ref={pillRef}
        className={cls}
        data-phase={PHASE_ID[gate]}
        data-sprite={art.sprite}
        data-motion={art.motion}
        data-bar-axis={barAxis}
        data-bar-position={slotCtx.dock}
        role="button"
        tabIndex={0}
        // Y5-J — one name per phase, from the phase table. This used to
        // spell three labels by hand and called every other phase (polishing,
        // pasting, error, empty, cancelled, model-loading…) "Start dictation".
        aria-label={phaseAccessibleName(gate, progress, chordLabel)}
        style={progress ? ({ ["--progress" as string]: progressFraction(progress).toFixed(4) } as React.CSSProperties) : undefined}
        title={blocked ? visual.label : gated ? GATED_TITLE : undefined}
        {...drag.handlers}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onToggle(); }
        }}
      >
        <FlowBarSlots ctx={slotCtx} group="lead" actions={slotActions} dragged={drag.dragged} />
        <MeetingBadge status={meeting} after={meetingAfter} onOpen={onMeetingOpen} />
        {/* ── Y2-D · the upgrade affordance ──
            Rendered ONLY when the policy gives it a route, so a licensed pill
            has no dead click region to hunt for. It lives INSIDE the capsule,
            which is the element `watchPillHitbox` publishes (YV65), so it shares
            that one hitbox: the transparent shadow margin stays click-through.
            `pointerdown` stops so a press on the mark never starts a re-dock
            drag, and `click` stops so it never reaches `onToggle` and starts a
            dictation. Because a mark press can never be the tail of a drag, its
            click does NOT consult `drag.dragged()` (that flag is stale after
            any re-dock; see ClassicPill.mark.test.tsx). There is no hover
            handler and no tab stop on it: passing the cursor over the pill
            must not pull focus off what the person is typing into. The gated capsule is ITSELF the button, so the mark
            stands down there (and under a permission block, which outranks). */}
        {/* yap24-PILL1 — the countdown is shown for the WHOLE trial, whether or
            not it has a route. With checkout closed (`purchase_open` false) the
            trial tones carry action `none`: the numeral is an inert <span> with
            its title, never a button to a Buy that dead-ends. `trial-quiet`
            (days 14…8) drops the hourglass and keeps only the numeral, at
            reduced contrast. */}
        {license?.show && !blocked && !gated ? (() => {
          const glyph = license.tone === "trial-quiet" ? null : <i className="pill-upgrade-glyph" aria-hidden>{license.glyph}</i>;
          const value = license.value ? <span className="pill-upgrade-value">{license.value}</span> : null;
          return licenseRoute !== null ? (
            <button
              type="button"
              className={`pill-upgrade lic-${license.tone}`}
              title={license.title}
              aria-label={license.title}
              tabIndex={-1}
              onPointerDown={(e) => e.stopPropagation()}
              onClick={(e) => { e.stopPropagation(); openLicenseSurface(); }}
            >
              {glyph}{value}
            </button>
          ) : (
            <span className={`pill-upgrade pill-upgrade-info lic-${license.tone}`} title={license.title} aria-label={license.title} role="note">
              {glyph}{value}
            </span>
          );
        })() : null}
        {/* Y10-A · the language picker slot. At rest only: mid-take the
            language is already decided, and a blocked/gated pill has one job. */}
        {!blocked && !gated && !live && !progress && !busy ? <LanguageChip /> : null}
        <button type="button" className="ctrl" data-slot="dictate" data-tip-side={tipSide} tabIndex={-1} aria-hidden onClick={(e) => { e.stopPropagation(); onToggle(); }}>
          {DICTATE.glyph({ ...slotCtx, phase: blocked || gated ? gate : live ? "listening" : isListening(slotCtx.phase) ? "idle" : slotCtx.phase })}
          <span className="slot-tip" data-tip-side={tipSide}>{DICTATE.tooltip({ ...slotCtx, phase: live ? "listening" : slotCtx.phase })}</span>
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
        ) : (
          // Y5-K — the creature's body slot: whatever the character painted.
          <span className="char-body" data-character={character.id}>{body}</span>
        )}
        <FlowBarSlots ctx={slotCtx} group="trail" actions={slotActions} dragged={drag.dragged} />
      </div>
      </div>
    </div>
  );
}
