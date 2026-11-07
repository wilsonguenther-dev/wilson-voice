/**
 * MeetingScene — yap-1106-pill-rec-headphones-hoot-tape (+ FB-05), the paint.
 *
 * The meeting's little world inside the one pill: sky + sun (moon in hour
 * two), the elapsed time under it, Hoot at the reel-to-reel on Yappy's left,
 * and Yappy in headphones whose RIGHT ear cup is the recording light.
 *
 * What to draw is decided by `meetingScene.ts` (pure, fake-clock tested). This
 * component repaints its canvas only when `meetingFrameKey` changes — once a
 * minute idle, plus each glance — and the two things that move faster (the
 * 1.6 s cup pulse and the turning reels) are CSS compositor animations on DOM
 * overlays, so a three-hour meeting never wakes a frame loop (OS-12).
 */
import { useEffect, useMemo, useRef, type CSSProperties } from "react";
import { drawChick, type ChickPose } from "../sprites/art";
import { drawFriendFrame } from "../sprites/friends";
import { backingScale } from "./pixelGrid";
import {
  meetingFrameKey,
  meetingScene,
  REC_LIGHT_PULSE_MS,
  SCENE_H,
  SCENE_LAYOUT,
  SCENE_W,
  type MeetingSceneModel,
} from "./meetingScene";

const SKY = {
  day: { top: "#c6ecff", body: "#8fd6ff" },
  dusk: { top: "#4b3f72", body: "#e48a6a" },
} as const;
const GRASS = { day: "#8fd06a", dusk: "#5f8f4e" } as const;
const INK = "#2a2d35";

function px(o: CanvasRenderingContext2D) {
  const rct = (x: number, y: number, w: number, h: number, col: string) => {
    o.fillStyle = col;
    o.fillRect(Math.round(x), Math.round(y), w, h);
  };
  const ell = (cx: number, cy: number, rx: number, ry: number, col: string) => {
    o.fillStyle = col;
    for (let y = Math.floor(cy - ry); y <= Math.ceil(cy + ry); y++) {
      const t = (y + 0.5 - cy) / ry;
      if (Math.abs(t) > 1) continue;
      const hw = rx * Math.sqrt(1 - t * t);
      o.fillRect(Math.round(cx - hw), y, Math.max(1, Math.round(2 * hw)), 1);
    }
  };
  return { rct, ell };
}

/** Hoot at 24 px from his 40 px sprite, nearest-neighbour (pill-r3.js). */
let hootBuf: HTMLCanvasElement | null = null;
function drawHoot(o: CanvasRenderingContext2D, handingTape: boolean) {
  const { x, y, size } = SCENE_LAYOUT.hoot;
  if (!hootBuf) {
    hootBuf = document.createElement("canvas");
    hootBuf.width = 40;
    hootBuf.height = 40;
  }
  const c = hootBuf.getContext("2d");
  if (!c) return;
  c.clearRect(0, 0, 40, 40);
  drawFriendFrame(c, "hoot", handingTape ? "action" : "idle");
  o.imageSmoothingEnabled = false;
  o.drawImage(hootBuf, x, y, size, size);
}

/** Paint one frame of the scene into an art-pixel context (SCENE_W x SCENE_H). */
export function paintMeetingScene(o: CanvasRenderingContext2D, m: MeetingSceneModel): void {
  const { rct, ell } = px(o);
  const L = SCENE_LAYOUT;
  o.clearRect(0, 0, SCENE_W, SCENE_H);
  // sky — day for the first hour, dusk from 1 h
  rct(0, 0, SCENE_W, L.groundY, SKY[m.sky].body);
  rct(0, 0, SCENE_W, 6, SKY[m.sky].top);
  if (m.sun) {
    ell(m.sun.x, m.sun.y, 3, 3, "#e8a41f");
    ell(m.sun.x, m.sun.y, 2, 2, "#ffd23f");
  }
  if (m.moon) {
    ell(m.moon.x, m.moon.y, 3, 3, "#f6f0cf");
    rct(m.moon.x - 1, m.moon.y - 1, 1, 1, "#d8d0a8");
  }
  rct(0, L.groundY, SCENE_W, SCENE_H - L.groundY, GRASS[m.sky]);
  // the reel-to-reel (the spokes that turn are a DOM overlay)
  const t = L.tape;
  rct(t.x, t.y, t.w, t.h, INK);
  rct(t.x + 1, t.y + 1, t.w - 2, t.h - 2, "#4a5060");
  rct(t.x + 1, t.y + 1, t.w - 2, 1, "#6a7284");
  for (const r of t.reels) {
    ell(r.x, r.y, 4, 4, "#1e2025");
    ell(r.x, r.y, 3, 3, "#8b6a3a");
    rct(r.x, r.y, 1, 1, "#e8e6e1");
  }
  rct(t.reels[0].x, t.y + t.h - 3, t.reels[1].x - t.reels[0].x, 1, "#1e2025"); // tape path
  drawHoot(o, m.hoot.handingTape);
  // the stop beat: the tape itself, on its way from Hoot to Yappy
  if (m.hoot.handingTape) {
    rct(50, 24, 9, 6, INK);
    rct(51, 25, 7, 4, "#e8e6e1");
    rct(52, 26, 2, 2, "#8b6a3a");
    rct(55, 26, 2, 2, "#8b6a3a");
  }
  // Yappy
  const { cx, feetY } = L.yappy;
  const pose: ChickPose = {
    hop: 0, sx: 0, sy: 0, blink: 0, mood: 0, beakF: 0, sway: 0, flap: 0,
    look: m.pose === "notepad" ? "down" : "fwd",
    glasses: false,
    pencil: m.pose === "notepad",
    droop: false,
  };
  drawChick(o, cx, feetY, pose);
  const cy = feetY - 11, rx = 11, ry = 11;
  if (m.pose === "glance") {
    // eyes up at the sky for the one-second glance
    for (const s of [-1, 1]) rct(cx + s * 5 - 1, cy - 3, 2, 1, "#3a2f36");
  }
  if (m.pose === "notepad") {
    const nx = cx - rx - 12, ny = feetY - 12;
    rct(nx, ny, 9, 10, "#c9c3ad");
    rct(nx + 1, ny + 1, 7, 8, "#fffdf5");
    for (let i = 0; i < 3; i++) rct(nx + 2, ny + 3 + i * 2, 5, 1, "#c9c3ad");
  }
  // headphones: band, left cup, and the right cup that IS the recording light
  rct(cx - rx + 1, cy - ry - 1, 2 * rx - 2, 2, INK);
  rct(cx - rx - 2, cy - ry + 1, 2, 6, INK);
  rct(cx + rx, cy - ry + 1, 2, 6, INK);
  rct(cx - rx - 3, cy - 3, 4, 6, "#3b3f4a");
  rct(cx - rx - 2, cy - 2, 2, 1, "#5a606e");
  const c = L.cup;
  rct(c.x - 1, c.y - 1, c.w + 2, c.h + 2, INK);
  rct(c.x, c.y, c.w, c.h, m.recLight.on ? "#9a2a24" : "#5a2420");
}

export default function MeetingScene({
  elapsedS,
  dictating = false,
  handover = false,
}: {
  elapsedS: number;
  dictating?: boolean;
  handover?: boolean;
}) {
  const reduceMotion = useMemo(
    () => typeof window !== "undefined" && typeof window.matchMedia === "function"
      && window.matchMedia("(prefers-reduced-motion: reduce)").matches,
    [],
  );
  const input = { elapsedS, dictating, handover, reduceMotion };
  const model = meetingScene(input);
  const key = meetingFrameKey(input);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const paints = useRef(0);
  const modelRef = useRef(model);
  modelRef.current = model;

  // The ONLY repaint: when the frame key changes (a minute, a glance edge, a
  // dictation or the hand-over). The per-second status tick re-renders the
  // time text and nothing else.
  useEffect(() => {
    const cv = canvasRef.current;
    if (!cv) return;
    try {
      const dpr = Math.max(1, Math.round(backingScale(window.devicePixelRatio)));
      if (cv.width !== SCENE_W * dpr) { cv.width = SCENE_W * dpr; cv.height = SCENE_H * dpr; }
      const o = cv.getContext("2d");
      if (!o) return;
      o.setTransform(dpr, 0, 0, dpr, 0, 0);
      o.imageSmoothingEnabled = false;
      paintMeetingScene(o, modelRef.current);
      paints.current += 1;
      cv.dataset.paints = String(paints.current);
    } catch (e) {
      console.warn(`pill meeting_scene_paint_failed err=${String((e as Error)?.name ?? "unknown").slice(0, 32)}`);
    }
  }, [key]);

  useEffect(() => {
    if (model.pose === "glance") console.info(`pill meeting_scene_glance minute=${Math.floor(elapsedS / 60)}`);
  }, [model.pose === "glance"]); // eslint-disable-line react-hooks/exhaustive-deps

  const L = SCENE_LAYOUT;
  const style = {
    width: `${SCENE_W}px`,
    height: `${SCENE_H}px`,
    ["--rec-pulse" as string]: `${REC_LIGHT_PULSE_MS}ms`,
  } as CSSProperties;
  return (
    <span
      className={`meeting-scene sky-${model.sky}`}
      data-meeting-scene
      data-pose={model.pose}
      data-handover={handover ? "true" : undefined}
      style={style}
      aria-hidden
    >
      <canvas ref={canvasRef} className="meeting-scene-canvas" style={{ width: `${SCENE_W}px`, height: `${SCENE_H}px` }} />
      {L.tape.reels.map((r, i) => (
        <i
          key={i}
          className={`meeting-reel${model.reelsTurning ? " turning" : ""}`}
          style={{ left: `${r.x - 3}px`, top: `${r.y - 3}px` }}
        />
      ))}
      <i
        className={`meeting-cup-light${model.recLight.pulsing ? " pulsing" : ""}${model.recLight.on ? "" : " off"}`}
        data-rec-light
        style={{ left: `${L.cup.x}px`, top: `${L.cup.y}px`, width: `${L.cup.w}px`, height: `${L.cup.h}px` }}
      />
      <span
        className="meeting-scene-time"
        data-scene-time
        style={{ left: `${model.timeAnchor.x}px`, top: `${model.timeAnchor.y}px` }}
      >
        {model.timeLabel}
      </span>
    </span>
  );
}
