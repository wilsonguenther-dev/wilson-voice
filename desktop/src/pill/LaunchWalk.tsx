/**
 * LaunchWalk — yap-1106-launch-walk (+ FB-11), the paint and the wiring.
 *
 * Yappy walks to work while the speech engine loads. What to draw is decided
 * by `launchWalk.ts` (pure, fake-loader tested) from the backend's
 * `engine_load_progress` percentage; this file only paints it with the layered
 * scroll ported from the approved design-2026-11-06/assets/walk.js (far hills
 * 0.2, trees 0.5, fence / ground / houses / office 1.0 of ground speed) and
 * the friends at half size, nearest-neighbour, waving as he passes.
 *
 * The frame loop runs ONLY while the walk is on screen, and under reduce
 * motion it paints one still frame per progress change (a progress line under
 * it). It never blocks a take: it is pointer-transparent except the Retry
 * button, so fn still records during the walk (the walk becomes a run).
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { drawChick, FRIENDS, type ChickPose } from "../sprites/art";
import { drawFriendFrame } from "../sprites/friends";
import { backingScale } from "./pixelGrid";
import { prefersReducedMotion } from "./motion";
import { extendPillHitbox } from "./drag";
import {
  ENGINE_LOAD_PROGRESS_EVENT,
  FRIEND_HOUSES,
  GROUND_Y as GY,
  HOLD_PCT,
  LAUNCH_WALK_HITBOX_OWNER,
  LAYER_SPEEDS,
  READY_CATCHUP_MS,
  READY_SHOW_MS,
  SCENE_H,
  SCENE_W,
  WALK_DIST,
  YAPPY_X,
  launchWalkModel,
  parseLoadProgress,
  stillFrameKey,
  type EngineLoadProgress,
  type FriendHouse,
  type WalkModel,
} from "./launchWalk";

type Px = {
  r: (x: number, y: number, w: number, h: number, col: string) => void;
  ell: (cx: number, cy: number, rx: number, ry: number, col: string) => void;
};

function px(o: CanvasRenderingContext2D): Px {
  const r = (x: number, y: number, w: number, h: number, col: string) => {
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
  return { r, ell };
}

// ---- walk.js layers, line for line (only the types are new) ----
function hills(h: Px, s: number) { const k0 = LAYER_SPEEDS.hills; for (let k = -1; k < 6; k++) { const x = k * 52 - (s * k0) % 52; h.ell(x, GY + 2, 30, 11, "#a9d89a"); h.ell(x - 6, GY - 4, 10, 4, "#bfe6b0"); } }
function trees(h: Px, s: number) { const k0 = LAYER_SPEEDS.trees; for (let k = -1; k < 5; k++) { const x = k * 70 + 30 - (s * k0) % 70; h.r(x, GY - 9, 2, 9, "#7a4b2e"); h.ell(x + 1, GY - 13, 6, 6, "#4f9f42"); h.ell(x - 1, GY - 15, 3, 3, "#6cc35a"); } }
function ground(h: Px, s: number) { h.r(0, GY, SCENE_W, 14, "#8fd06a"); h.r(0, GY, SCENE_W, 2, "#6fb84e"); for (let x = -(s % 9); x < SCENE_W; x += 9) h.r(x, GY - 2, 1, 2, "#6fb84e"); for (let y = 0; y < 2; y++) for (let x2 = -(s % 18) + y * 9; x2 < SCENE_W; x2 += 18) h.r(x2, GY + 6 + y * 4, 4, 1, "#7cc45a"); }
function fence(h: Px, s: number) { const off = s % 14; h.r(0, GY - 5, SCENE_W, 1, "#c9a878"); for (let x = -off; x < SCENE_W; x += 14) { h.r(x, GY - 7, 2, 7, "#a5804a"); h.r(x, GY - 7, 2, 1, "#e8c890"); } }
function house(h: Px, x: number, open: boolean) { h.r(x - 18, GY - 18, 36, 18, "#f1e4c8"); h.r(x - 18, GY - 18, 36, 1, "#fffaf0"); for (let i = 0; i < 6; i++) h.r(x - 21 + i, GY - 19 - i * 2, 42 - i * 2, 2, i % 2 ? "#c8443a" : "#e0574a"); h.r(x - 4, GY - 12, 8, 12, open ? "#3a2418" : "#8a5a36"); if (!open) h.r(x + 2, GY - 6, 1, 1, "#e8c86a"); h.r(x - 15, GY - 14, 7, 6, "#8fd6ff"); h.r(x - 12, GY - 14, 1, 6, "#f1e4c8"); h.r(x + 9, GY - 14, 7, 6, "#ffd27a"); h.r(x + 12, GY - 14, 1, 6, "#f1e4c8"); }
function mailbox(h: Px, x: number, flag: boolean) { h.r(x, GY - 8, 2, 8, "#7a4b2e"); h.r(x - 3, GY - 12, 8, 5, "#3f6fb0"); h.r(x - 3, GY - 12, 8, 1, "#6a9ad8"); if (flag) { h.r(x + 5, GY - 15, 1, 5, "#2a2d35"); h.r(x + 6, GY - 15, 3, 2, "#e5484d"); } }
function office(h: Px, x: number, open: boolean) { h.r(x, GY - 26, 40, 26, "#c9cfd6"); h.r(x, GY - 26, 40, 2, "#e8ecef"); for (let r = 0; r < 2; r++) for (let c = 0; c < 4; c++) h.r(x + 4 + c * 9, GY - 22 + r * 7, 6, 4, "#8fd6ff"); h.r(x + 14, GY - 10, 10, 10, open ? "#3a2418" : "#4a5060"); if (open) { h.r(x + 12, GY - 10, 3, 10, "#4a5060"); h.r(x + 15, GY - 9, 8, 8, "#ffd27a"); } h.r(x + 10, GY - 30, 20, 4, "#2a2d35"); h.r(x + 12, GY - 29, 3, 2, "#ffd23f"); h.r(x + 17, GY - 29, 3, 2, "#ffd23f"); h.r(x + 22, GY - 29, 3, 2, "#ffd23f"); }
function shade(c: string): string { const n = parseInt(c.slice(1), 16); const r = (n >> 16) * 0.8, g = ((n >> 8) & 255) * 0.8, b = (n & 255) * 0.8; return `rgb(${[r, g, b].map(Math.round).join(",")})`; }

/** A friend at 0.5x, nearest-neighbour, so they fit the 44-pixel scene (walk.js mini). */
let miniBuf: HTMLCanvasElement | null = null;
function mini(o: CanvasRenderingContext2D, f: FriendHouse, x: number, y: number, waveFrame: number | null) {
  const d = FRIENDS[f.id];
  if (!miniBuf) miniBuf = document.createElement("canvas");
  miniBuf.width = d.w;
  miniBuf.height = d.h;
  const c = miniBuf.getContext("2d");
  if (!c) return;
  c.clearRect(0, 0, d.w, d.h);
  drawFriendFrame(c, f.id, waveFrame === null ? "idle" : "action", waveFrame ?? 0);
  o.imageSmoothingEnabled = false;
  const w = Math.round(d.w / 2), hh = Math.round(d.h / 2);
  o.drawImage(miniBuf, Math.round(x - w / 2), Math.round(y - hh), w, hh);
}

function friendHouse(h: Px, o: CanvasRenderingContext2D, x: number, f: FriendHouse, near: boolean, waveFrame: number) {
  h.r(x - 13, GY - 14, 26, 14, f.wall); h.r(x - 13, GY - 14, 26, 1, "#ffffff");
  for (let i = 0; i < 4; i++) h.r(x - 15 + i, GY - 15 - i * 2, 30 - i * 2, 2, i % 2 ? f.roof : shade(f.roof));
  h.r(x + 3, GY - 9, 6, 9, "#8a5a36"); h.r(x - 9, GY - 11, 7, 5, f.id === "ember" ? "#ffe27a" : "#8fd6ff"); h.r(x - 6, GY - 11, 1, 5, f.wall);
  const wave = near ? waveFrame : 0, bob = near && wave ? 1 : 0, frame = near ? wave : null;
  if (f.from === "win") { o.save(); o.beginPath(); o.rect(x - 9, GY - 11, 7, 5); o.clip(); mini(o, f, x - 5.5, GY - 4 - bob, frame); o.restore(); }
  else if (f.from === "porch") { h.r(x - 13, GY - 1, 26, 1, "#a5804a"); mini(o, f, x - 6, GY + 1 - bob, frame); }
  else if (f.from === "roof") mini(o, f, x, GY - 20 - bob, frame);
  else { h.r(x + 16, GY - 16, 3, 16, "#7a4b2e"); h.ell(x + 17, GY - 18, 7, 6, "#4f9f42"); mini(o, f, x + 14, GY - 8 - bob, 1); }
  if (near && wave) { const dx = f.from === "win" ? -12 : -14; h.r(x + dx, GY - 12, 1, 3, "#ffffff"); h.r(x + dx - 1, GY - 11, 1, 1, "#ffffff"); }
}

/** Paint one frame of the walk into an art-pixel context (SCENE_W x SCENE_H). */
export function paintLaunchWalk(o: CanvasRenderingContext2D, m: WalkModel): void {
  const h = px(o);
  const s = m.scroll;
  o.clearRect(0, 0, SCENE_W, SCENE_H);
  // sky (the pill's day sky), then the layers back to front
  h.r(0, 0, SCENE_W, GY, "#8fd6ff");
  h.r(0, 0, SCENE_W, 6, "#c6ecff");
  h.r(0, GY - 2, SCENE_W, 2, "#8fd6ff");
  hills(h, s);
  trees(h, s);
  ground(h, s);
  house(h, YAPPY_X - s, m.houseOpen);
  mailbox(h, YAPPY_X + 30 - s, m.houseOpen);
  const waving = new Set(m.waving);
  for (const f of FRIEND_HOUSES) {
    const x = YAPPY_X + (f.at / 100) * WALK_DIST - s;
    if (x > -30 && x < SCENE_W + 30) friendHouse(h, o, x + 20, f, waving.has(f.id), m.waveFrame);
  }
  fence(h, s);
  office(h, YAPPY_X + WALK_DIST - s - 14, m.officeOpen);
  const pose: ChickPose = {
    hop: m.pose.hop, sx: 0, sy: 0, blink: m.pose.blink, mood: m.pose.mood, beakF: 0,
    sway: m.pose.sway, flap: m.pose.flap, look: "fwd", glasses: false, pencil: false, droop: false,
  };
  drawChick(o, YAPPY_X, GY, pose);
}

// ---- the wiring ----

interface LoadClock {
  progress: EngineLoadProgress | null;
  changedAt: number;
  readyAt: number | null;
  readyFromPct: number;
  warmAt: number | null;
}

/**
 * Follows the backend's load percentage: one snapshot on mount (a pill that
 * opens mid-load joins the walk where it is; one that opens onto a warm
 * engine hops once), then every `engine_load_progress` event.
 */
function useEngineLoad(): LoadClock {
  const [clock, setClock] = useState<LoadClock>({ progress: null, changedAt: 0, readyAt: null, readyFromPct: 0, warmAt: null });
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const apply = (p: EngineLoadProgress) => setClock((c) => {
      const now = performance.now();
      const moved = !c.progress || c.progress.pct !== p.pct || c.progress.stage !== p.stage;
      if (p.stage === "starting") console.info(`pill launch_walk_start model=${p.modelId.slice(0, 32)}`);
      if (p.stage === "ready" && c.progress?.stage !== "ready") console.info(`pill launch_walk_ready elapsed_ms=${p.elapsedMs}`);
      if (p.stage === "failed" && c.progress?.stage !== "failed") console.warn(`pill launch_walk_failed pct=${p.pct} elapsed_ms=${p.elapsedMs}`);
      return {
        progress: p,
        changedAt: moved ? now : c.changedAt,
        readyAt: p.stage === "ready" ? (c.progress?.stage === "ready" ? c.readyAt : now) : null,
        readyFromPct: p.stage === "ready" ? (c.progress && c.progress.stage !== "ready" ? c.progress.pct : c.readyFromPct) : p.pct,
        warmAt: null,
      };
    });
    listen<unknown>(ENGINE_LOAD_PROGRESS_EVENT, (e) => {
      const p = parseLoadProgress(e.payload);
      if (p) apply(p);
    }).then((u) => (dead ? u() : unsubs.push(u))).catch(() => undefined);
    Promise.all([
      invoke<unknown>("engine_load_progress"),
      invoke<{ loaded: boolean; loading: boolean }>("engine_status"),
    ]).then(([raw, st]) => {
      if (dead) return;
      const p = parseLoadProgress(raw);
      if (st.loaded && !st.loading) {
        // Warm engine at launch: no walk, one 600 ms hop.
        setClock((c) => (c.progress ? c : { ...c, warmAt: performance.now() }));
      } else if (p && p.stage !== "ready") {
        setClock((c) => (c.progress ? c : { progress: p, changedAt: performance.now(), readyAt: null, readyFromPct: p.pct, warmAt: null }));
      }
    }).catch((e) => console.warn(`pill launch_walk_snapshot_failed err=${String((e as Error)?.name ?? "unknown").slice(0, 32)}`));
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  return clock;
}

export default function LaunchWalk({
  recording,
  suppressed = false,
  forced = null,
}: {
  recording: boolean;
  suppressed?: boolean;
  /** A headless capture's fixed percentage (`?walk=`); the live clock is ignored. */
  forced?: EngineLoadProgress | null;
}) {
  const reduceMotion = useMemo(() => prefersReducedMotion(), []);
  const live = useEngineLoad();
  const clock: LoadClock = forced
    ? { progress: forced, changedAt: 0, readyAt: null, readyFromPct: HOLD_PCT, warmAt: null }
    : live;
  const [now, setNow] = useState(() => performance.now());
  const [retrying, setRetrying] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const lastKey = useRef("");
  const retryRef = useRef<HTMLButtonElement | null>(null);

  const model = launchWalkModel({
    progress: clock.progress,
    sinceChangeMs: forced ? 0 : now - clock.changedAt,
    // A forced ready frame holds the open door (after the catch-up) for the capture.
    sinceReadyMs: forced ? (forced.stage === "ready" ? READY_CATCHUP_MS : null) : clock.readyAt === null ? null : now - clock.readyAt,
    readyFromPct: clock.readyFromPct,
    warmHopMs: clock.warmAt === null ? null : now - clock.warmAt,
    recording,
    reduceMotion,
    tMs: now,
  });
  const visible = !suppressed && model.mode !== "hidden";

  // The frame loop: only while the walk is on screen. Its clock moves the legs,
  // the waves and the after-ready beats; the scroll comes from the percentage.
  useEffect(() => {
    if (!visible) return undefined;
    if (reduceMotion) {
      // No frame loop under reduce motion: one wake-up to end the ready / hop beat.
      const t = window.setTimeout(() => setNow(performance.now()), READY_SHOW_MS + 16);
      return () => clearTimeout(t);
    }
    let raf = 0;
    const tick = () => { setNow(performance.now()); raf = requestAnimationFrame(tick); };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [visible, reduceMotion, clock]);
  // A progress change while hidden (a load starting) must wake the walk.
  useEffect(() => { setNow(performance.now()); }, [clock]);

  useEffect(() => {
    if (!visible) { lastKey.current = ""; return; }
    const cv = canvasRef.current;
    if (!cv) return;
    // Reduce motion: one still frame per progress change, never a loop of paints.
    const key = reduceMotion ? stillFrameKey(model) : String(now);
    if (key === lastKey.current) return;
    lastKey.current = key;
    try {
      const dpr = Math.max(1, Math.round(backingScale(window.devicePixelRatio)));
      if (cv.width !== SCENE_W * dpr) { cv.width = SCENE_W * dpr; cv.height = SCENE_H * dpr; }
      const o = cv.getContext("2d");
      if (!o) return;
      o.setTransform(dpr, 0, 0, dpr, 0, 0);
      o.imageSmoothingEnabled = false;
      paintLaunchWalk(o, model);
    } catch (e) {
      console.warn(`pill launch_walk_paint_failed err=${String((e as Error)?.name ?? "unknown").slice(0, 32)}`);
    }
  });

  useEffect(() => {
    if (visible && model.running && recording) console.info("pill launch_walk_run reason=recording");
  }, [visible && model.running && recording]); // eslint-disable-line react-hooks/exhaustive-deps

  // The pill panel is click-through outside the capsule: while Retry is on
  // screen its rect joins the hitbox under its own owner (like the gated chip),
  // so the click reaches it instead of the app underneath. Unmount hands it back.
  const retryShown = visible && model.showRetry;
  useEffect(() => {
    const el = retryRef.current;
    if (!retryShown || !el) {
      extendPillHitbox(null, LAUNCH_WALK_HITBOX_OWNER);
      return;
    }
    const push = () => {
      if (!el.isConnected) {
        extendPillHitbox(null, LAUNCH_WALK_HITBOX_OWNER);
        return;
      }
      const r = el.getBoundingClientRect();
      extendPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, LAUNCH_WALK_HITBOX_OWNER);
    };
    const ro = new ResizeObserver(push);
    ro.observe(el);
    push();
    return () => {
      ro.disconnect();
      extendPillHitbox(null, LAUNCH_WALK_HITBOX_OWNER);
    };
  }, [retryShown]);

  if (!visible) return null;
  const retry = () => {
    if (retrying) return;
    setRetrying(true);
    console.info("pill launch_walk_retry");
    invoke<boolean>("retry_engine_load")
      .then((started) => { if (!started) console.warn("pill launch_walk_retry_no_model"); })
      .catch((e) => console.warn(`pill launch_walk_retry_failed err=${String((e as Error)?.name ?? "unknown").slice(0, 32)}`))
      .finally(() => setRetrying(false));
  };
  return (
    <div
      className="launch-walk"
      data-launch-walk={model.mode}
      data-stalled={model.stalled ? "true" : undefined}
      data-running={model.running ? "true" : undefined}
      data-pct={model.pct}
      role="status"
      aria-live="polite"
      aria-label={model.label}
    >
      <canvas ref={canvasRef} className="launch-walk-canvas" style={{ width: `${SCENE_W}px`, height: `${SCENE_H}px` }} aria-hidden />
      {model.progressLine !== null && (
        <span className="launch-walk-line" aria-hidden>
          <span className="launch-walk-line-fill" style={{ width: `${model.progressLine}%` }} />
        </span>
      )}
      {model.showRetry && (
        <button ref={retryRef} type="button" className="launch-walk-retry" onClick={retry} disabled={retrying}>
          {retrying ? "Retrying" : "Retry"}
        </button>
      )}
    </div>
  );
}
