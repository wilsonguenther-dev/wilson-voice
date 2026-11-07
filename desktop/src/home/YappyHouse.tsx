/**
 * YappyHouse — Home's room, now YAPPY'S STUDIO (yap-1106-studio-room + FB-08).
 *
 * The room is the approved layered side view (decisions.studio_style =
 * layered; companions.html section 3 "C · Layered side view", home.html
 * section 5): a 320 x 160 buffer in three depth layers (back wall, friends at
 * their stations, Yappy's desk in front) that shift 1, 3 and 6 whole pixels
 * with the pointer, inside a wooden pixel frame drawn in the buffer. The
 * drawing and every station rule live in ./studio/studio.ts; this component
 * owns the canvas, the clock, the events and the redraw budget.
 *
 *   (1) Yappy is driven by the REAL Tauri app events the pill already uses —
 *       "recording" / "status" (busy) / "transcript" / "audio_level" — through
 *       the habitat director: he types at his desk while you dictate.
 *       SP-11: "audio_level" reaches this window only while it is visible.
 *   (2) The sky follows the REAL system clock (the SKY keyframes below, the
 *       same table the pill sky reads).
 *   (3) Every station reads real app state (props from Home): Hoot's ON AIR
 *       light = a meeting recording, Tidy's counter = today's fixes, Gulp's jar
 *       = Dictionary size, Shelly = battery level and charging, Ember's
 *       fireflies = streak days at night (max 7).
 *   (4) Budget (FB-08): idle redraw at most once a minute (studio.redrawPlan);
 *       prefers-reduced-motion renders ONE still frame with no loop, repainted
 *       only when the state it shows changes.
 *   (5) yap-1106-home-full-width-room: the room is a whole-number scale
 *       (homeLayout.roomBox) of the main column.
 */
import { useEffect, useRef, useState } from "react";
import { ROOM_H, meetingRoomLabel, roomBox, type RoomScale } from "./homeLayout";
import { toChatTone, type ChatTone } from "../pill/live";
import { YAPPY_HABITAT_MOODS } from "../pill/characters/yappy";
import { createDirector, moodOf, reactTo, skyPhase, stepDirector } from "./habitat/habitat";
import { subscribeHabitat } from "./habitat/events";
import { duoAt } from "../news/duo";
// yap24-UI1 — the integer pixel grid (shared with the pill): the room is drawn at
// the largest WHOLE number of device px per art px that fits the stage.
import { backingScale, parseObjectPosition, pixelGrid, type PixelGrid } from "../pill/pixelGrid";
import {
  EMPTY_STUDIO_INPUTS,
  IDLE_REDRAW_MS,
  LAYERS,
  LIVE_FRAME_MS,
  PAD,
  STILL_T,
  STUDIO_H,
  STUDIO_W,
  composeStudio,
  createFrameCache,
  onAirLit,
  parallaxOffsets,
  parallaxTarget,
  redrawPlan,
  cinderFlight,
  studioAriaLabel,
  furnaceState,
  mochiQuietReason,
  yappyPose,
  type RedrawMode,
  type StudioInputs,
  type StudioSky,
  type StudioState,
} from "./studio/studio";

interface YappyHouseProps {
  /**
   * Companion tone (YV27): friendly | rude | rose — flavours Yappy's live mood
   * label. Read from settings (passed by App), never a hardcoded constant.
   */
  companionTone?: string;
  /**
   * The running meeting (shell meetingStatus, pushed by the backend's 1 Hz
   * `meeting` event). While it records, Hoot's ON AIR light is lit, he wears
   * his headphones at the mic, and the label reads "Recording meeting · mm:ss".
   */
  meeting?: { recording: boolean; elapsedSeconds: number } | null;
  /** yap-1106-studio-room — the stations' real app state (Home computes it). */
  studio?: Partial<StudioInputs> | null;
}

// ---- the real-clock sky -----------------------------------------------------
// phase: 0 dawn · .25 day · .5 dusk · .75 night (wraps). The pill sky
// (pill/sky.ts ROOM_SKY) is pinned to these exact pairs by its test.
const SKY = [
  { at: 0.0, top: "#f6a6a0", bot: "#ffd9a0", amb: "#ffb28a", ambA: 0.16, lamp: 0.15, stars: 0 }, // dawn
  { at: 0.25, top: "#7ec8ff", bot: "#cdeeff", amb: "#fff2d8", ambA: 0.0, lamp: 0.0, stars: 0 }, // day
  { at: 0.5, top: "#ff9d6b", bot: "#ffd27a", amb: "#ff8a4a", ambA: 0.22, lamp: 0.45, stars: 0 }, // dusk
  { at: 0.75, top: "#171a3d", bot: "#2c3060", amb: "#1a2050", ambA: 0.42, lamp: 0.85, stars: 1 }, // night
];
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
const hex = (h: string): [number, number, number] => {
  const s = h.replace("#", "");
  return [parseInt(s.slice(0, 2), 16), parseInt(s.slice(2, 4), 16), parseInt(s.slice(4, 6), 16)];
};
const mix = (h1: string, h2: string, t: number) => {
  const a = hex(h1),
    b = hex(h2);
  const c = (i: number) => Math.round(lerp(a[i], b[i], t)).toString(16).padStart(2, "0");
  return `#${c(0)}${c(1)}${c(2)}`;
};
/** The sky at a phase, blended between the two nearest keyframes. */
function skyAt(phase: number): StudioSky {
  const p = ((phase % 1) + 1) % 1;
  const seg = p * 4,
    i = Math.floor(seg) % 4,
    f = seg - Math.floor(seg);
  const a = SKY[i],
    b = SKY[(i + 1) % 4];
  return { top: mix(a.top, b.top, f), bot: mix(a.bot, b.bot, f), amb: mix(a.amb, b.amb, f), ambA: lerp(a.ambA, b.ambA, f) };
}

function inputsOf(studio: Partial<StudioInputs> | null | undefined): StudioInputs {
  return { ...EMPTY_STUDIO_INPUTS, ...(studio ?? {}) };
}

export default function YappyHouse({ companionTone, meeting, studio }: YappyHouseProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const frameRef = useRef<HTMLDivElement | null>(null);
  // Set by the mount effect: redraw now (a state change), whatever the mode.
  const wakeRef = useRef<(() => void) | null>(null);
  // The room's whole-number scale for the column it sits in (4x / 3x / 2x).
  const [scale, setScale] = useState<RoomScale>(3);
  useEffect(() => {
    const el = frameRef.current;
    if (!el) return;
    let last: RoomScale | null = null;
    const measure = () => {
      const cs = getComputedStyle(el);
      const inner = el.clientWidth - (parseFloat(cs.paddingLeft) || 0) - (parseFloat(cs.paddingRight) || 0);
      const next = roomBox(inner).scale;
      if (next !== last) {
        if (last !== null) console.info(`home room_scale from=${last} to=${next}`);
        last = next;
        setScale(next);
      }
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Read inside the mount-once loop, so a prop change never restarts it.
  const meetingRef = useRef<boolean>(!!meeting?.recording);
  useEffect(() => {
    meetingRef.current = !!meeting?.recording;
    wakeRef.current?.();
  }, [meeting?.recording]);
  const inputs = inputsOf(studio);
  const inputsRef = useRef<StudioInputs>(inputs);
  useEffect(() => {
    inputsRef.current = inputs;
    wakeRef.current?.();
    // Primitive deps: Home rebuilds the object each render; only a real change redraws.
  }, [inputs.fixesToday, inputs.dictionarySize, inputs.battery?.percent, inputs.battery?.charging, inputs.streakDays, inputs.newsReady, inputs.newsArrivedAtMs, inputs.newsDelivered, inputs.newsTurns, inputs.engineLoading, inputs.modelReady, inputs.screenSharing, inputs.doNotDisturb]);
  // yap-1106-sal-and-mochi — one log line when Sal's furnace or Mochi's spot changes.
  const furnace = furnaceState(inputs);
  const mochiWhy = mochiQuietReason(inputs);
  useEffect(() => {
    console.info(`home studio_furnace state=${furnace}`);
  }, [furnace]);
  useEffect(() => {
    console.info(`home studio_mochi on_door=${mochiWhy !== null} reason=${mochiWhy ?? "none"}`);
  }, [mochiWhy]);
  const moodRef = useRef<HTMLSpanElement | null>(null);
  // YV27 — the live tone in a ref so the mood label reads it without re-running the loop.
  const toneRef = useRef<ChatTone>(toChatTone(companionTone));
  useEffect(() => {
    toneRef.current = toChatTone(companionTone);
  }, [companionTone]);

  useEffect(() => {
    const stage = canvasRef.current;
    if (!stage) return;
    const dctx = stage.getContext("2d");
    if (!dctx) return;
    dctx.imageSmoothingEnabled = false;

    // The studio buffer; upscaled nearest-neighbour by an INTEGER cell (regrid).
    const W = STUDIO_W,
      H = STUDIO_H;
    const mk = (w: number, h: number) => {
      const c = document.createElement("canvas");
      c.width = w;
      c.height = h;
      return c;
    };
    // Three depth layers, each PAD wider on both sides so a shift never shows an edge.
    const layers = [0, 1, 2].map(() => mk(W + PAD * 2, H));
    const out = mk(W, H);
    const og = out.getContext("2d");
    if (!og) return;
    og.imageSmoothingEnabled = false;
    // yap-1106-fb-24 — the wooden pixel frame is painted ONCE into its own
    // offscreen buffer; every redraw blits it at (0, 0), so it never moves with
    // the parallax and costs one drawImage, not a repaint.
    const frameCache = createFrameCache(() => {
      const surface = mk(W, H);
      const ctx = surface.getContext("2d");
      if (!ctx) throw new Error("frame_ctx_unavailable");
      ctx.imageSmoothingEnabled = false;
      return { surface, ctx };
    });

    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    let grid: PixelGrid = pixelGrid({ w: W, h: H }, { w: W, h: H }, 1);
    const state = createDirector();
    const t0 = performance.now();
    const par = { px: 0, target: 0 };
    let tGlobal = 0,
      last = 0,
      lastDraw = 0,
      raf = 0,
      timer = 0,
      draws = 0,
      drawFailed = false,
      mode: RedrawMode | null = null;

    function duoFrame() {
      const i = inputsRef.current;
      return duoAt(i.newsReady ? i.newsArrivedAtMs : null, Date.now(), reduce, i.newsTurns ?? 0);
    }

    function duoStudio(): Pick<StudioState, "duoHandover" | "duoSpeaker"> {
      const d = duoFrame();
      return { duoHandover: d.handover, duoSpeaker: reduce ? null : d.speaker };
    }

    function snapshot(): StudioState {
      const phase = skyPhase(new Date());
      return {
        ...inputsRef.current,
        // yap-1106-news-home-briefing — Cinder's flight in with a new briefing.
        cinderFlight: cinderFlight(inputsRef.current.newsArrivedAtMs, Date.now(), reduce),
        // yap-1106-news-duo-delivery — the hand-over and whose turn it is, from
        // the same clock the briefing card reads (reduce motion: nobody animates).
        ...duoStudio(),
        phase,
        sky: skyAt(phase),
        meetingRecording: meetingRef.current,
        yappy: yappyPose(state),
      };
    }

    function setMood() {
      const el = moodRef.current;
      if (!el) return;
      const m = YAPPY_HABITAT_MOODS[toneRef.current][moodOf(state)];
      if (el.textContent !== m) el.textContent = m;
    }

    function render() {
      try {
        const s = snapshot();
        const t = reduce ? STILL_T : tGlobal;
        const offs = parallaxOffsets(reduce ? 0 : par.px);
        layers.forEach((c, i) => {
          const o = c.getContext("2d");
          if (!o) return;
          o.setTransform(1, 0, 0, 1, 0, 0);
          o.clearRect(0, 0, c.width, c.height);
          o.imageSmoothingEnabled = false;
          o.translate(PAD, 0);
          LAYERS[i](o, s, t);
        });
        og!.imageSmoothingEnabled = false;
        const firstFrame = frameCache.stats.renders === 0;
        composeStudio<HTMLCanvasElement>(og!, layers, frameCache.get(), offs, s, t);
        if (firstFrame) console.info(`home studio_frame_cached w=${W} h=${H} cell=${grid.cell}`);
        stage!.dataset.onAir = onAirLit(s) ? "on" : "off";
        // A display with a different backing scale re-grids here: no resize event fires for that.
        if (backingScale(devicePixelRatio) !== grid.dpr) regrid();
        dctx!.clearRect(0, 0, grid.backingW, grid.backingH);
        dctx!.drawImage(out, 0, 0, W, H, grid.offsetX, grid.offsetY, grid.drawW, grid.drawH);
        draws++;
      } catch (e) {
        if (!drawFailed) {
          drawFailed = true;
          console.warn(`home studio_draw_failed err=${e instanceof Error ? e.name : "unknown"}`);
        }
      }
    }

    // ---- the redraw budget: ONE rule (studio.redrawPlan) -------------------
    const reacting = () => state.reaction === "cheer" || state.appState === "done";
    const settling = () => Math.abs(par.target - par.px) > 0.01;
    function schedule() {
      const p = redrawPlan({
        reduceMotion: reduce,
        appState: state.appState,
        reacting: reacting(),
        parallaxSettling: settling(),
        meetingRecording: meetingRef.current,
        furnaceLighting: inputsRef.current.engineLoading,
        // yap-1106-news-duo-delivery — the room stays live through the flight,
        // the hand-over and the turns (reduce motion is "still" regardless).
        cinderFlying: inputsRef.current.newsReady && duoFrame().stage !== "done",
      });
      if (p.mode !== mode) {
        console.info(`home studio_mode from=${mode ?? "none"} to=${p.mode} draws=${draws}`);
        mode = p.mode;
        draws = 0;
      }
      if (p.mode === "still") return;
      if (p.mode === "live") raf = requestAnimationFrame(frame);
      else
        timer = window.setTimeout(() => {
          timer = 0;
          raf = requestAnimationFrame(frame);
        }, p.delayMs ?? IDLE_REDRAW_MS);
    }

    function frame(ms: number) {
      raf = 0;
      if (mode === "live" && lastDraw && ms - lastDraw < LIVE_FRAME_MS) {
        raf = requestAnimationFrame(frame);
        return;
      }
      const dt = last ? Math.min(0.05, (ms - last) / 1000) : 0.016;
      last = ms;
      lastDraw = ms;
      tGlobal = (ms - t0) / 1000;
      stepDirector(state, dt, new Date());
      par.px += (par.target - par.px) * 0.2;
      if (!settling()) par.px = par.target;
      setMood();
      render();
      schedule();
    }

    // A state change: redraw now (an idle or meeting wait is cut short).
    function wake() {
      if (reduce) {
        setMood();
        render();
        return;
      }
      if (timer) {
        clearTimeout(timer);
        timer = 0;
      }
      if (!raf) raf = requestAnimationFrame(frame);
    }
    wakeRef.current = wake;

    // ---- REAL app events → the director (pushed, never polled) ------------
    // A synchronous cleanup can run before the subscription lands; `dead` drops late events.
    let dead = false;
    const unsubs: Array<() => void> = [
      subscribeHabitat((ev) => {
        if (dead) return;
        const before = `${state.appState}|${state.reaction}`;
        reactTo(state, ev, Date.now());
        // Only a change of what Yappy is doing wakes an idle room; mic level
        // ticks ride the live loop that is already running.
        if (`${state.appState}|${state.reaction}` !== before) wake();
      }),
    ];

    // ---- the pointer: layers shift 1, 3, 6 whole pixels (never in reduce motion)
    const onMove = (e: MouseEvent) => {
      if (reduce) return;
      const r = stage!.getBoundingClientRect();
      par.target = parallaxTarget(e.clientX, r.left, r.width);
      wake();
    };
    const onLeave = () => {
      if (reduce) return;
      par.target = 0;
      wake();
    };
    stage.addEventListener("mousemove", onMove);
    stage.addEventListener("mouseleave", onLeave);

    // yap24-UI1 — size the backing store to the stage's device box and pick the
    // cell. Resizing a canvas clears it and resets its context, so every regrid
    // repaints. object-fit / object-position on .yh-stage are READ here.
    function regrid() {
      const r = stage!.getBoundingClientRect();
      const cs = getComputedStyle(stage!);
      grid = pixelGrid({ w: r.width, h: r.height }, { w: W, h: H }, devicePixelRatio, {
        fit: cs.objectFit === "cover" ? "cover" : "contain",
        pos: parseObjectPosition(cs.objectPosition),
      });
      if (stage!.width !== grid.backingW) stage!.width = grid.backingW;
      if (stage!.height !== grid.backingH) stage!.height = grid.backingH;
      dctx!.imageSmoothingEnabled = false;
    }
    regrid();
    const ro = new ResizeObserver(() => {
      regrid();
      render();
    });
    ro.observe(stage);

    console.info(`home studio_mount reduce_motion=${reduce}`);
    if (reduce) {
      // ONE still frame: the real clock's sky, every station's real state.
      setMood();
      render();
      schedule();
    } else {
      raf = requestAnimationFrame(frame);
    }

    return () => {
      dead = true;
      wakeRef.current = null;
      ro.disconnect();
      stage.removeEventListener("mousemove", onMove);
      stage.removeEventListener("mouseleave", onLeave);
      if (raf) cancelAnimationFrame(raf);
      if (timer) clearTimeout(timer);
      unsubs.forEach((u) => u());
      // One counter: the frame should read renders=1 for the whole mount.
      console.info(`home studio_frame_stats renders=${frameCache.stats.renders} hits=${frameCache.stats.hits}`);
    };
  }, []);

  const meetingLabel = meetingRoomLabel(meeting);
  const phase = skyPhase(new Date());
  const aria = studioAriaLabel({
    ...inputs,
    phase,
    sky: skyAt(phase),
    meetingRecording: !!meeting?.recording,
    yappy: "rest",
  });

  return (
    <div className="yappy-house" data-room-scale={scale}>
      <div className="yh-frame" ref={frameRef}>
        <canvas
          ref={canvasRef}
          className="yh-stage"
          width={STUDIO_W}
          height={STUDIO_H}
          data-on-air={meeting?.recording ? "on" : "off"}
          // Full column width, the room's height at the whole-number scale:
          // YappyHouse grids the art at exactly `scale` CSS px per art px.
          style={{ height: `${ROOM_H * scale}px` }}
          role="img"
          aria-label={aria}
        />
      </div>
      <p className="yh-label" data-room-label={meetingLabel ? "recording" : "mood"}>
        {meetingLabel ? (
          <span className="yh-meeting" aria-live="off">
            {meetingLabel}
          </span>
        ) : (
          <>
            Yappy ·{" "}
            {/* live mood is announced to assistive tech as Yappy's state changes */}
            <span ref={moodRef} aria-live="polite">
              content
            </span>
          </>
        )}
      </p>
    </div>
  );
}
