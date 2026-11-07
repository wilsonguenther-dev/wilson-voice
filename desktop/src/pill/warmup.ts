/**
 * yap-1106-pill-warmup-engine — the compact pill's "Warming up" beat, decided
 * from the engine's load signal and nothing else.
 *
 * Approved design: design-2026-11-06/pill.html section 10 ("Warming up: Yappy
 * starts the engine") and assets/pill-r3.js `engine`. Same compact pill and
 * fill bar, the window widened from 44 x 24 to 72 x 24 to fit a little crate
 * engine beside Yappy. Each beat is picked by how far the engine has got,
 * never by a timer:
 *   asleep (starting) -> crank (reading the file) -> typing code (warming up)
 *   -> steam (first test pass) -> bell (ready, one hop).
 * First launch: the bar is the model download (`model_download_progress`).
 * A load: the backend's three-step signal (transcription.rs `load_step`), one
 * third of the bar per step. A step over 8 s: Yappy wipes his brow and the
 * label reads "First start takes longer". fn while he works records anyway:
 * he drops the crank for his notepad. A failed load: the engine coughs smoke
 * and the pill offers Retry. A warm engine: nothing at all.
 *
 * One signal, two renderers: the launch walk (launchWalk.ts) paints the
 * window's FIRST load; this pill paints the first-launch download and every
 * later load in the same session (an idle unload's reload, a model switch).
 *
 * Pure (no DOM, no clock of its own) so `warmup.test.ts` drives it with a
 * slowed fake loader and a fake clock. The clock moves only the legs, the
 * crank and the steam puffs; it never picks a beat.
 */
import { HOLD_PCT, type EngineLoadProgress, type LoadStage } from "./launchWalk";

/** The pill window while warming up, CSS px (the design's `win: { w: 72, h: 24 }`). */
export const WARM_WIN_W = 72;
export const WARM_WIN_H = 24;
/** The art the window paints, 2 art px per CSS px (1:1 device px on Retina). */
export const WARM_ART_W = 144;
export const WARM_ART_H = 48;
/** Yappy's centre and feet in art px (the design's `cxOff: -6`). */
export const WARM_YAPPY_X = WARM_ART_W / 2 - 6;
export const WARM_FEET_Y = 44;
/** The crate engine's top-left in art px (the design's `R3.engine(o, 96, feet - 14, ...)`). */
export const WARM_ENGINE_X = 96;
export const WARM_ENGINE_Y = WARM_FEET_Y - 14;
/** One step running longer than this shows the sweat pose (design rule, section 10). */
export const SWEAT_MS = 8000;
/** The bell rings and Yappy hops once, then the pill folds back. */
export const BELL_SHOW_MS = 900;
/** The extendPillHitbox owner for the warm-up Retry button. */
export const WARMUP_HITBOX_OWNER = "warmup-retry";

export const WARM_LABELS = {
  asleep: "Waking up",
  working: "Warming up",
  almost: "Almost",
  sweat: "First start takes longer",
  ready: "Ready",
  failed: "Engine didn't start",
} as const;

export type WarmBeat = "asleep" | "crank" | "typing" | "steam" | "bell" | "smoke";

/** The design's `engine(..., stage)` index: 0 cold, 1 crank, 2 steam, 3 screen, 4 bell. */
export const ENGINE_ART_STAGE: Record<WarmBeat, number> = {
  asleep: 0, crank: 1, steam: 2, typing: 3, bell: 4, smoke: 0,
};

/** `model_download_progress` payload (models.rs, snake_case on the wire). */
export interface DownloadProgress {
  model_id?: string;
  downloaded: number;
  total: number;
}

/** Download bytes -> whole percent 0..100, or null for a malformed payload. */
export function downloadPct(raw: unknown): number | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const d = typeof r.downloaded === "number" && Number.isFinite(r.downloaded) ? r.downloaded : null;
  const t = typeof r.total === "number" && Number.isFinite(r.total) ? r.total : null;
  if (d === null || t === null || t <= 0) return null;
  return Math.max(0, Math.min(100, Math.floor((d / t) * 100)));
}

const STAGE_STEP: Record<LoadStage, number> = { starting: 0, reading: 1, waiting: 2, testing: 3, ready: 4, failed: 3 };

/** The load's step (0..4): the backend's `step` when sent, else derived from the stage. */
export function loadStep(p: EngineLoadProgress): number {
  if (typeof p.step === "number" && Number.isInteger(p.step) && p.step >= 0 && p.step <= 4) {
    // `ready` is the only way to step 4; a stray 4 on any other stage is not trusted.
    return p.stage === "ready" ? 4 : Math.min(3, p.step);
  }
  return STAGE_STEP[p.stage];
}

/**
 * The fill bar, 0..100, from the three-step signal: each step is one third.
 * Reading the file is the one step with a measured number, so it moves inside
 * its third; the native load and the first test pass start their third and
 * hold there (an honest bar never fakes motion it cannot measure). 100 only
 * on `ready`.
 */
export function loadBar(p: EngineLoadProgress): number {
  if (p.stage === "ready") return 100;
  const step = loadStep(p);
  if (step <= 0) return 0;
  const third = 100 / 3;
  if (step === 1) {
    const inStep = Math.max(0, Math.min(1, (p.pct - 1) / (HOLD_PCT - 1)));
    return Math.round(inStep * third * 10) / 10;
  }
  return Math.round((step - 1) * third * 10) / 10;
}

export interface WarmInput {
  /** The model download's whole percent (first launch), or null. */
  download: number | null;
  /** The last engine load progress this pill saw in its life, or null. */
  load: EngineLoadProgress | null;
  /** ms since the current step began (a load), or since the download % last moved. */
  stepAgeMs: number;
  /** ms since `ready` arrived AFTER a non-ready value in this pill's life; null otherwise. */
  sinceReadyMs: number | null;
  /** fn is held / a take is recording right now. */
  recording: boolean;
  reduceMotion: boolean;
  /** Animation clock (crank, puffs, typing, the hop). NEVER picks the beat. */
  tMs: number;
}

export interface WarmPose {
  hop: number;
  flap: number;
  sy: number;
  blink: number;
  mood: number;
  beakF: number;
  look: "down" | "fwd";
  pencil: boolean;
}

export type WarmMode = "hidden" | "download" | "load" | "ready" | "failed";

export interface WarmModel {
  mode: WarmMode;
  beat: WarmBeat;
  /** The design's engine art stage (crank hidden while he holds the notepad). */
  engineStage: number;
  /** Fill bar 0..100. */
  bar: number;
  label: string;
  sweat: boolean;
  /** fn during warm-up: recording runs, he holds his notepad. */
  notepad: boolean;
  showRetry: boolean;
  pose: WarmPose;
  /** The step the bar is in (0..4), or -1 for the download. */
  step: number;
}

const STILL: WarmPose = { hop: 0, flap: 0, sy: 0, blink: 0, mood: 0.4, beakF: 0, look: "fwd", pencil: false };

function hidden(): WarmModel {
  return { mode: "hidden", beat: "asleep", engineStage: 0, bar: 0, label: "", sweat: false, notepad: false, showRetry: false, pose: STILL, step: -1 };
}

function frame(tMs: number, perSecond: number): number {
  return Math.floor(Math.max(0, tMs) / (1000 / perSecond)) % 2;
}

function blinkAt(tMs: number): number {
  return Math.max(0, tMs) % 3200 < 120 ? 1 : 0;
}

/** The pose for a beat (design section 10's frames), still under reduce motion. */
function poseFor(beat: WarmBeat, i: WarmInput, sinceReady: number): WarmPose {
  const rm = i.reduceMotion;
  const t = i.tMs;
  const b = rm ? 0 : blinkAt(t);
  switch (beat) {
    case "asleep":
      return { ...STILL, blink: 1 };
    case "crank": {
      const f = rm ? 0 : frame(t, 8);
      return { ...STILL, flap: f * 2, hop: f, sy: 1, blink: b };
    }
    case "typing":
      return { ...STILL, look: "down", beakF: rm ? 0 : frame(t, 6), blink: b };
    case "steam":
      return { ...STILL, mood: 0.6, blink: b };
    case "bell": {
      const k = Math.min(1, Math.max(0, sinceReady) / BELL_SHOW_MS);
      return { ...STILL, mood: 1, hop: rm ? 0 : Math.round(Math.sin(k * Math.PI) * 4) };
    }
    case "smoke":
      return { ...STILL, mood: 0, blink: b };
  }
}

const BEAT_FOR_STEP: readonly WarmBeat[] = ["asleep", "crank", "typing", "steam"];

/** The whole warm-up, as data. */
export function warmupModel(i: WarmInput): WarmModel {
  const p = i.load;
  if (p && p.stage === "ready") {
    // The ONLY way to the bell: the backend said ready, after this pill saw the
    // load running. A ready read at open (a warm engine) shows nothing.
    if (i.sinceReadyMs === null || i.sinceReadyMs >= BELL_SHOW_MS) return hidden();
    return {
      ...hidden(), mode: "ready", beat: "bell", engineStage: ENGINE_ART_STAGE.bell, bar: 100,
      label: WARM_LABELS.ready, pose: poseFor("bell", i, i.sinceReadyMs), step: 4,
    };
  }
  if (p && p.stage === "failed") {
    const step = loadStep(p);
    return {
      ...hidden(), mode: "failed", beat: "smoke", engineStage: ENGINE_ART_STAGE.smoke,
      bar: loadBar({ ...p, stage: step >= 3 ? "testing" : step === 2 ? "waiting" : "reading" }),
      label: WARM_LABELS.failed, showRetry: true, pose: poseFor("smoke", i, 0), step,
    };
  }
  if (p) {
    const step = loadStep(p);
    const beat = BEAT_FOR_STEP[Math.max(0, Math.min(3, step))];
    const sweat = step >= 1 && i.stepAgeMs > SWEAT_MS;
    return withNotepad({
      ...hidden(), mode: "load", beat, engineStage: ENGINE_ART_STAGE[beat], bar: loadBar(p),
      label: sweat ? WARM_LABELS.sweat : step === 0 ? WARM_LABELS.asleep : step === 3 ? WARM_LABELS.almost : WARM_LABELS.working,
      sweat, pose: poseFor(beat, i, 0), step,
    }, i);
  }
  if (i.download !== null && i.download < 100) {
    const beat: WarmBeat = i.download <= 0 ? "asleep" : "crank";
    const sweat = i.download > 0 && i.stepAgeMs > SWEAT_MS;
    return withNotepad({
      ...hidden(), mode: "download", beat, engineStage: ENGINE_ART_STAGE[beat], bar: i.download,
      label: sweat ? WARM_LABELS.sweat : i.download <= 0 ? WARM_LABELS.asleep : WARM_LABELS.working,
      sweat, pose: poseFor(beat, i, 0), step: -1,
    }, i);
  }
  // A warm engine, or nothing loading: the pill is its usual self.
  return hidden();
}

/** fn during warm-up records anyway: he drops the crank for his notepad. */
function withNotepad(m: WarmModel, i: WarmInput): WarmModel {
  if (!i.recording) return m;
  return {
    ...m, notepad: true,
    engineStage: m.engineStage === ENGINE_ART_STAGE.crank ? 0 : m.engineStage,
    pose: { ...m.pose, pencil: true, look: "down", flap: 0, hop: 0, sy: 0 },
  };
}

/** Plain words for the screen reader (the label plus the bar, never jargon). */
export function warmupAriaLabel(m: WarmModel): string {
  if (m.mode === "hidden") return "";
  if (m.mode === "failed") return `${m.label}. Retry is available.`;
  if (m.mode === "ready") return "Ready";
  return `${m.label}, ${Math.round(m.bar)} percent${m.notepad ? ". Recording." : ""}`;
}

/** Reduce motion: one still frame per change of what is shown (never a paint loop). */
export function warmupStillKey(m: WarmModel): string {
  return [m.mode, m.beat, m.engineStage, Math.round(m.bar), m.sweat ? 1 : 0, m.notepad ? 1 : 0].join(":");
}

/**
 * A forced warm-up for headless captures (scripts/pill-matrix.mjs, dev builds
 * only — `FORCED` in float-main): `warm=dl:40`, `warm=15:reading`,
 * `warm=95:waiting`, `warm=95:testing`, `warm=95:testing:sweat`,
 * `warm=60:failed`, `warm=100:ready`. Anything else is null.
 */
export interface ForcedWarmup {
  download: number | null;
  load: EngineLoadProgress | null;
  sweat: boolean;
}

export function forcedWarmup(search: string): ForcedWarmup | null {
  const raw = new URLSearchParams(search).get("warm");
  if (!raw) return null;
  const parts = raw.split(":");
  if (parts[0] === "dl") {
    const n = Number(parts[1]);
    if (!Number.isFinite(n)) return null;
    return { download: Math.max(0, Math.min(99, Math.round(n))), load: null, sweat: parts[2] === "sweat" };
  }
  const pct = Number(parts[0]);
  const stage = parts[1] as LoadStage | undefined;
  const ok: readonly string[] = ["starting", "reading", "waiting", "testing", "ready", "failed"];
  if (!Number.isFinite(pct) || !stage || !ok.includes(stage)) return null;
  const clamped = stage === "ready" ? 100 : Math.max(0, Math.min(HOLD_PCT, Math.round(pct)));
  return {
    download: null,
    load: { pct: clamped, stage, modelId: "forced", elapsedMs: 0 },
    sweat: parts[2] === "sweat",
  };
}
