/**
 * WarmupEngine — yap-1106-pill-warmup-engine, the paint and the wiring.
 *
 * Every decision (beat, bar, label, sweat, notepad, Retry) is made by the pure
 * `warmupModel` (warmup.ts) from the backend's load signal; this file follows
 * `model_download_progress` + `engine_load_progress` and paints the approved
 * design-2026-11-06/pill.html section 10 frames: the compact pill with its fill
 * bar, the window widened to 72 x 24 so a crate engine (assets/pill-r3.js
 * `engine`, ported line for line) fits beside Yappy.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { drawChick, H, type ChickPose, type PixelCtx } from "../sprites/art";
import { backingScale } from "./pixelGrid";
import { prefersReducedMotion } from "./motion";
import { extendPillHitbox } from "./drag";
import { ENGINE_LOAD_PROGRESS_EVENT, parseLoadProgress, type EngineLoadProgress } from "./launchWalk";
import {
  BELL_SHOW_MS,
  SWEAT_MS,
  WARM_ART_H,
  WARM_ART_W,
  WARM_ENGINE_X,
  WARM_ENGINE_Y,
  WARM_FEET_Y,
  WARM_WIN_H,
  WARM_WIN_W,
  WARM_YAPPY_X,
  WARMUP_HITBOX_OWNER,
  downloadPct,
  loadStep,
  warmupAriaLabel,
  warmupModel,
  warmupStillKey,
  type ForcedWarmup,
  type WarmModel,
} from "./warmup";

/** pill-r3.js `engine(o, x, y, pct, t, stage)`, line for line (t in seconds). */
function paintEngine(o: PixelCtx, x: number, y: number, pct: number, t: number, stage: number, smoke: boolean): void {
  const h = H(o);
  const by = y;
  h.r(x, by, 18, 14, "#5a3420"); h.r(x + 1, by + 1, 16, 12, "#b8763f"); h.r(x + 1, by + 1, 16, 1, "#d8955a"); h.r(x + 1, by + 7, 16, 1, "#9a5c30");
  h.ell(x + 9, by + 5, 4, 3, "#2a2d35"); h.ell(x + 9, by + 5, 3, 2, "#fffdf5");
  const a = -Math.PI + (pct / 100) * Math.PI;
  for (let i = 1; i <= 3; i++) h.r(x + 9 + Math.round(Math.cos(a) * i), by + 5 + Math.round(Math.sin(a) * i * 0.7), 1, 1, pct >= 95 ? "#3fa36b" : "#b93a26");
  h.r(x + 13, by - 5, 3, 5, "#2a2d35");
  if (smoke) {
    // A failed load: the engine coughs grey smoke instead of steam.
    for (let k = 0; k < 3; k++) h.ell(x + 15 + k * 2, by - 7 - k * 3, 2 + k, 1.5 + k * 0.6, "#6b6e76", 0.85 - k * 0.2);
  } else if (stage >= 2) {
    const p = Math.floor(t * 3) % 3;
    h.ell(x + 15 + p, by - 7 - p * 2, 2 + p * 0.5, 1.5 + p * 0.5, "#ffffff", 0.85);
  }
  if (stage === 1) {
    const c = t * 8;
    h.r(x + 18, by + 8, 3, 2, "#2a2d35"); h.r(x + 20, by + 9 + Math.round(Math.sin(c) * 3), 1, 1, "#2a2d35");
    h.r(x + 19 + Math.round(Math.cos(c) * 3), by + 9 + Math.round(Math.sin(c) * 3), 3, 2, "#e8c86a");
  }
  if (stage === 3) {
    h.r(x + 3, by + 9, 12, 3, "#1e2025");
    const n = Math.floor(t * 6) % 4;
    for (let k = 0; k < 2; k++) h.r(x + 4, by + 9 + k, 2 + ((n + k * 2) % 5) * 2, 1, "#7fdc7a");
  }
  if (stage === 4) { h.r(x + 7, by - 4, 4, 3, "#f2c14e"); h.r(x + 8, by - 5, 2, 1, "#f2c14e"); h.r(x + 8, by - 1, 2, 1, "#c98a14"); }
}

/** Paint one frame into the 144 x 48 art space. */
export function paintWarmup(o: PixelCtx & { clearRect: (x: number, y: number, w: number, h: number) => void }, m: WarmModel, tMs: number): void {
  o.clearRect(0, 0, WARM_ART_W, WARM_ART_H);
  const h = H(o);
  const t = tMs / 1000;
  const pose: ChickPose = {
    hop: m.pose.hop, sx: 0, sy: m.pose.sy, blink: m.pose.blink, mood: m.pose.mood, beakF: m.pose.beakF,
    sway: 0, flap: m.pose.flap, look: m.pose.look, glasses: false, pencil: m.pose.pencil, droop: m.beat === "smoke",
  };
  drawChick(o, WARM_YAPPY_X, WARM_FEET_Y, pose);
  const headTop = WARM_FEET_Y - 22 - m.pose.hop;
  if (m.beat === "asleep") {
    // zzz above his head (design X.zzz).
    h.r(WARM_YAPPY_X + 8, headTop - 2, 3, 1, "#4a5060"); h.r(WARM_YAPPY_X + 9, headTop - 1, 1, 1, "#4a5060"); h.r(WARM_YAPPY_X + 8, headTop, 3, 1, "#4a5060");
    h.r(WARM_YAPPY_X + 13, headTop - 6, 2, 1, "#4a5060"); h.r(WARM_YAPPY_X + 13, headTop - 5, 2, 1, "#4a5060");
  }
  if (m.sweat) {
    // He wipes his brow (design X.sweat): one bead by the temple.
    h.r(WARM_YAPPY_X + 9, headTop + 3, 2, 3, "#8fd6ff"); h.r(WARM_YAPPY_X + 9, headTop + 3, 1, 1, "#ffffff");
  }
  paintEngine(o, WARM_ENGINE_X, WARM_ENGINE_Y, m.bar, t, m.engineStage, m.beat === "smoke");
}

interface WarmClock {
  download: number | null;
  downloadMovedAt: number;
  load: EngineLoadProgress | null;
  stepAt: number;
  /** A non-ready load was seen in this pill's life (so a later ready rings the bell). */
  sawRunning: boolean;
  readyAt: number | null;
}

const EMPTY: WarmClock = { download: null, downloadMovedAt: 0, load: null, stepAt: 0, sawRunning: false, readyAt: null };

/** Follows the download (first launch) and the three-step load signal. */
function useWarmClock(): WarmClock {
  const [clock, setClock] = useState<WarmClock>(EMPTY);
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const applyLoad = (p: EngineLoadProgress) => setClock((c) => {
      const now = performance.now();
      const step = loadStep(p);
      const prevStep = c.load ? loadStep(c.load) : -1;
      if (p.stage === "failed" && c.load?.stage !== "failed") console.warn(`pill warmup_failed step=${step} elapsed_ms=${p.elapsedMs}`);
      if (p.stage === "ready" && c.sawRunning && c.load?.stage !== "ready") console.info(`pill warmup_ready elapsed_ms=${p.elapsedMs}`);
      if (step !== prevStep && p.stage !== "ready" && p.stage !== "failed") console.info(`pill warmup_step step=${step}`);
      const running = p.stage !== "ready" && p.stage !== "failed";
      return {
        ...c,
        load: p,
        stepAt: step !== prevStep ? now : c.stepAt,
        sawRunning: c.sawRunning || running,
        readyAt: p.stage === "ready" ? (c.load?.stage === "ready" ? c.readyAt : c.sawRunning ? now : null) : null,
        // A running load outranks the download; a finished download is over.
        download: running ? null : c.download,
      };
    });
    listen<unknown>(ENGINE_LOAD_PROGRESS_EVENT, (e) => {
      const p = parseLoadProgress(e.payload);
      if (p) applyLoad(p);
    }).then((u) => (dead ? u() : unsubs.push(u))).catch(() => undefined);
    listen<unknown>("model_download_progress", (e) => {
      const pct = downloadPct(e.payload);
      if (pct === null) return;
      setClock((c) => {
        if (c.download === null) console.info("pill warmup_start source=download");
        return { ...c, download: pct >= 100 ? null : pct, downloadMovedAt: c.download !== pct ? performance.now() : c.downloadMovedAt };
      });
    }).then((u) => (dead ? u() : unsubs.push(u))).catch(() => undefined);
    // A pill that opens mid-load joins where the load is; one that opens onto
    // a warm engine stays its usual self (no bell for a load it never saw).
    Promise.all([
      invoke<unknown>("engine_load_progress"),
      invoke<{ loaded: boolean; loading: boolean }>("engine_status"),
    ]).then(([raw, st]) => {
      if (dead || (st.loaded && !st.loading)) return;
      const p = parseLoadProgress(raw);
      if (p && p.stage !== "ready" && p.stage !== "failed") setClock((c) => (c.load ? c : { ...c, load: p, stepAt: performance.now(), sawRunning: true }));
    }).catch((e) => console.warn(`pill warmup_snapshot_failed err=${String((e as Error)?.name ?? "unknown").slice(0, 32)}`));
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  return clock;
}

export default function WarmupEngine({
  recording,
  suppressed = false,
  forced = null,
}: {
  recording: boolean;
  /** The launch walk owns this load, a meeting owns the pill, or a matrix scene is forced. */
  suppressed?: boolean;
  /** A headless capture's fixed frame (`?warm=`); the live clock is ignored. */
  forced?: ForcedWarmup | null;
}) {
  const reduceMotion = useMemo(() => prefersReducedMotion(), []);
  const live = useWarmClock();
  const [now, setNow] = useState(() => performance.now());
  const [retrying, setRetrying] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const retryRef = useRef<HTMLButtonElement | null>(null);
  const lastKey = useRef("");
  const sweatLogged = useRef(-2);

  const model = forced
    ? warmupModel({
        download: forced.download, load: forced.load, stepAgeMs: forced.sweat ? SWEAT_MS + 1 : 0,
        sinceReadyMs: forced.load?.stage === "ready" ? BELL_SHOW_MS / 2 : null,
        recording, reduceMotion, tMs: now,
      })
    : warmupModel({
        download: live.download,
        load: live.load,
        stepAgeMs: live.load ? now - live.stepAt : now - live.downloadMovedAt,
        sinceReadyMs: live.readyAt === null ? null : now - live.readyAt,
        recording, reduceMotion, tMs: now,
      });
  const visible = !suppressed && model.mode !== "hidden";

  // Frame loop only while on screen (its clock moves the crank, puffs and
  // hop). Reduce motion: no loop — one wake-up for the sweat threshold and one
  // to end the bell, so every beat change is still painted as a still frame.
  useEffect(() => {
    if (!visible) return undefined;
    if (reduceMotion) {
      const ts = [window.setTimeout(() => setNow(performance.now()), SWEAT_MS + 50), window.setTimeout(() => setNow(performance.now()), BELL_SHOW_MS + 16)];
      return () => ts.forEach(clearTimeout);
    }
    let raf = 0;
    const tick = () => { setNow(performance.now()); raf = requestAnimationFrame(tick); };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [visible, reduceMotion, live]);
  // A new value while hidden (a load starting) must wake the pill.
  useEffect(() => { setNow(performance.now()); }, [live]);

  useEffect(() => {
    if (visible && model.sweat && sweatLogged.current !== model.step) {
      sweatLogged.current = model.step;
      console.info(`pill warmup_sweat step=${model.step}`);
    }
    if (!model.sweat) sweatLogged.current = -2;
  }, [visible, model.sweat, model.step]);

  useEffect(() => {
    if (visible && model.notepad) console.info(`pill warmup_record_during step=${model.step}`);
  }, [visible && model.notepad]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!visible) { lastKey.current = ""; return; }
    const cv = canvasRef.current;
    if (!cv) return;
    const key = reduceMotion ? warmupStillKey(model) : String(now);
    if (key === lastKey.current) return;
    lastKey.current = key;
    try {
      // 2 art px per CSS px: on a 2x display one art px is one device px.
      const s = Math.max(1, Math.round(backingScale(window.devicePixelRatio)));
      if (cv.width !== WARM_WIN_W * s) { cv.width = WARM_WIN_W * s; cv.height = WARM_WIN_H * s; }
      const o = cv.getContext("2d");
      if (!o) return;
      o.setTransform(s / 2, 0, 0, s / 2, 0, 0);
      o.imageSmoothingEnabled = false;
      paintWarmup(o, model, reduceMotion ? 0 : now);
    } catch (e) {
      console.warn(`pill warmup_paint_failed err=${String((e as Error)?.name ?? "unknown").slice(0, 32)}`);
    }
  });

  // The panel is click-through outside the capsule: while Retry shows, its
  // rect joins the hitbox under its own owner; unmount hands it back.
  const retryShown = visible && model.showRetry;
  useEffect(() => {
    const el = retryRef.current;
    if (!retryShown || !el) {
      extendPillHitbox(null, WARMUP_HITBOX_OWNER);
      return;
    }
    const push = () => {
      if (!el.isConnected) { extendPillHitbox(null, WARMUP_HITBOX_OWNER); return; }
      const r = el.getBoundingClientRect();
      extendPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, WARMUP_HITBOX_OWNER);
    };
    const ro = new ResizeObserver(push);
    ro.observe(el);
    push();
    return () => { ro.disconnect(); extendPillHitbox(null, WARMUP_HITBOX_OWNER); };
  }, [retryShown]);

  if (!visible) return null;
  const retry = () => {
    if (retrying) return;
    setRetrying(true);
    console.info(`pill warmup_retry step=${model.step}`);
    invoke<boolean>("retry_engine_load")
      .then((started) => { if (!started) console.warn("pill warmup_retry_no_model"); })
      .catch((e) => console.warn(`pill warmup_retry_failed err=${String((e as Error)?.name ?? "unknown").slice(0, 32)}`))
      .finally(() => setRetrying(false));
  };
  return (
    <div
      className="warmup-pill"
      data-warmup={model.mode}
      data-beat={model.beat}
      data-sweat={model.sweat ? "true" : undefined}
      data-notepad={model.notepad ? "true" : undefined}
      role="status"
      aria-live="polite"
      aria-label={warmupAriaLabel(model)}
    >
      <span className="warmup-win" aria-hidden>
        <canvas ref={canvasRef} className="warmup-canvas" style={{ width: `${WARM_WIN_W}px`, height: `${WARM_WIN_H}px` }} />
      </span>
      <span className="warmup-label" aria-hidden>{model.label}</span>
      <span
        className="warmup-bar"
        role="progressbar"
        aria-label="Speech engine"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(model.bar)}
      >
        <i style={{ transform: `scaleX(${(model.bar / 100).toFixed(4)})` }} />
      </span>
      {model.showRetry && (
        <button ref={retryRef} type="button" className="warmup-retry" onClick={retry} disabled={retrying}>
          {retrying ? "Retrying" : "Retry"}
        </button>
      )}
    </div>
  );
}
