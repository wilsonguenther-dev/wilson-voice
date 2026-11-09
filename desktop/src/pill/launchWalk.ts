/**
 * yap-1106-launch-walk (+ FB-11) — what the launch walk shows, decided from
 * the engine's load percentage and nothing else.
 *
 * Approved design: design-2026-11-06/companions.html section 06 and
 * assets/walk.js. Yappy walks in place while the world scrolls past him in
 * layers (far hills 0.2, trees 0.5, fence / ground / houses / office 1.0 of
 * ground speed). The ground covers WALK_DIST scene px over the whole load, so
 * how far the world has scrolled IS how far the engine has loaded: a stalled
 * load stops the ground (he walks on the spot), and the walk can never end
 * before the engine says ready. He waits at the office door from 95 % until
 * the backend reports `ready`; a warm engine skips the walk for one 600 ms
 * hop; a failed load keeps the door shut, he shrugs, and the pill offers
 * Retry; recording during the walk turns it into a run.
 *
 * Pure (no DOM, no clock of its own) so `launchWalk.test.ts` drives it with a
 * fake loader and a fake clock. The clock it is given moves ONLY the legs, the
 * waves and the after-ready beats — never the scroll.
 */
import type { FriendId } from "../sprites/friends";

export const SCENE_W = 176;
export const SCENE_H = 44;
/** Ground line (walk.js GY). */
export const GROUND_Y = 30;
/** Yappy's centre; he never moves — the world does. */
export const YAPPY_X = 88;
/** walk.js DIST: scene px of ground over the whole load (eight houses ~65 px apart). */
export const WALK_DIST = 520;
/** He waits at the office door from here until the engine reports ready. */
export const HOLD_PCT = 95;
/** The office-door scroll (the design's waiting frames are drawn at 99.9 %). */
export const DOOR_SCROLL = WALK_DIST * 0.999;
/** Layer speeds as a share of ground speed (walk.js hills / trees / ground). */
export const LAYER_SPEEDS = { hills: 0.2, trees: 0.5, fence: 1, ground: 1, houses: 1 } as const;
/** Walk cycle: 8 steps a second; a run (recording, or catching up) is 12. */
export const WALK_STEP_MS = 125;
export const RUN_STEP_MS = 1000 / 12;
/** A warm engine: walk skipped, one hop. */
export const WARM_HOP_MS = 600;
/** Load finished early: the world catches up in at most this. */
export const READY_CATCHUP_MS = 300;
/** Door open + one hop after ready, then the pill folds back (300 ms, CSS). */
export const READY_SHOW_MS = READY_CATCHUP_MS + WARM_HOP_MS;
/** No new percentage for this long = the load is stalled: the ground stops. */
export const STALL_MS = 1500;
/** A friend waves while their house is within this of Yappy, 4 waves a second. */
export const WAVE_RANGE_PX = 30;
export const WAVE_FRAME_MS = 250;

export interface FriendHouse {
  /** Load % at which Yappy passes the house. */
  at: number;
  id: FriendId;
  wall: string;
  roof: string;
  from: "win" | "porch" | "tree" | "roof";
}

/** walk.js FH, verbatim: [load %, friend, wall, roof, where they wave from]. */
export const FRIEND_HOUSES: readonly FriendHouse[] = [
  { at: 5, id: "hoot", wall: "#cfd6e0", roof: "#4a5060", from: "win" },
  { at: 14, id: "shelly", wall: "#e6f0dc", roof: "#5f9f4e", from: "porch" },
  { at: 23, id: "tidy", wall: "#fbe0dc", roof: "#c8443a", from: "porch" },
  { at: 38, id: "gulp", wall: "#dff2d6", roof: "#3f8a3a", from: "porch" },
  { at: 47, id: "ember", wall: "#d9dcef", roof: "#3a3f5a", from: "win" },
  { at: 56, id: "sal", wall: "#ffe2cf", roof: "#c86a3a", from: "porch" },
  { at: 72, id: "mochi", wall: "#e6e6ee", roof: "#7c7c88", from: "tree" },
  { at: 84, id: "cinder", wall: "#fde3e0", roof: "#b82a33", from: "roof" },
];

// yap-1106-pill-warmup-engine — "testing" (the first test pass, step 3) waits
// at the door like "waiting": the walk never moves past the hold for it.
export type LoadStage = "starting" | "reading" | "waiting" | "testing" | "ready" | "failed";
const STAGES: readonly LoadStage[] = ["starting", "reading", "waiting", "testing", "ready", "failed"];

/** The backend's `engine_load_progress` payload (transcription.rs EngineLoadProgress). */
export interface EngineLoadProgress {
  pct: number;
  stage: LoadStage;
  modelId: string;
  elapsedMs: number;
  /**
   * yap-1106-pill-warmup-engine — the three-step signal (transcription.rs
   * `load_step`): 0 starting, 1 reading the file, 2 warming up, 3 first test
   * pass, 4 ready. Optional: an older backend omits it (the pill derives it).
   */
  step?: number;
}

export const ENGINE_LOAD_PROGRESS_EVENT = "engine_load_progress";
/**
 * The `extendPillHitbox` owner for the Retry button: the pill panel is
 * click-through outside the capsule, so while Retry shows its rect joins the
 * hitbox under this owner (never the capsule's own, never a gate owner).
 */
export const LAUNCH_WALK_HITBOX_OWNER = "launch-walk-retry";

/** Validate an event payload; anything malformed is `null` (the walk hides). */
export function parseLoadProgress(raw: unknown): EngineLoadProgress | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const pct = typeof r.pct === "number" && Number.isFinite(r.pct) ? Math.max(0, Math.min(100, Math.round(r.pct))) : null;
  const stage = typeof r.stage === "string" && (STAGES as readonly string[]).includes(r.stage) ? (r.stage as LoadStage) : null;
  if (pct === null || stage === null) return null;
  // Only `ready` may say 100 (the backend guarantees it; the pill re-checks).
  return {
    pct: stage === "ready" ? 100 : Math.min(pct, HOLD_PCT),
    stage,
    modelId: typeof r.modelId === "string" ? r.modelId.slice(0, 64) : "",
    elapsedMs: typeof r.elapsedMs === "number" && Number.isFinite(r.elapsedMs) ? r.elapsedMs : 0,
    ...(typeof r.step === "number" && Number.isInteger(r.step) && r.step >= 0 && r.step <= 4 ? { step: r.step } : {}),
  };
}

/** Load % (0..95) → ground scrolled. Position from the number, never a timer. */
export function scrollForPct(pct: number): number {
  const p = Math.max(0, Math.min(HOLD_PCT, Number.isFinite(pct) ? pct : 0));
  return (p / HOLD_PCT) * DOOR_SCROLL;
}

/** Where a friend's house sits on screen for a given scroll (walk.js). */
export function houseX(h: FriendHouse, scroll: number): number {
  return YAPPY_X + (h.at / 100) * WALK_DIST - scroll;
}

export type WalkMode = "hidden" | "walk" | "wait" | "ready" | "failed" | "hop";

export interface WalkInput {
  /** The last progress the backend reported, or null (no load seen). */
  progress: EngineLoadProgress | null;
  /** ms since the percentage last changed. */
  sinceChangeMs: number;
  /** ms since `ready` arrived in this pill's life; null if it has not. */
  sinceReadyMs: number | null;
  /** The last percentage before `ready` (where the catch-up starts). */
  readyFromPct: number;
  /** The pill opened onto a warm engine: ms since then; null otherwise. */
  warmHopMs: number | null;
  /** fn is held / a take is recording: the walk turns into a run. */
  recording: boolean;
  reduceMotion: boolean;
  /** Animation clock (legs, waves, after-ready beats). NEVER the scroll. */
  tMs: number;
}

export interface WalkPose {
  hop: number;
  flap: number;
  sway: number;
  mood: number;
  blink: number;
}

export interface WalkModel {
  mode: WalkMode;
  /** Ground scrolled, 0..WALK_DIST. */
  scroll: number;
  /** The load percentage shown (progress line, aria). */
  pct: number;
  /** The load has not moved for STALL_MS: the ground is stopped. */
  stalled: boolean;
  running: boolean;
  pose: WalkPose;
  houseOpen: boolean;
  officeOpen: boolean;
  /** Friends waving right now (house within WAVE_RANGE_PX of Yappy). */
  waving: FriendId[];
  /** 0 / 1: the friends' wave frame. */
  waveFrame: number;
  showRetry: boolean;
  /** Reduce motion: the still frame's progress line, 0..100. Null otherwise. */
  progressLine: number | null;
  /** Plain words for the screen reader / title. */
  label: string;
}

const STILL: WalkPose = { hop: 0, flap: 0, sway: 0, mood: 0.3, blink: 0 };

function hiddenModel(): WalkModel {
  return {
    mode: "hidden", scroll: 0, pct: 0, stalled: false, running: false, pose: STILL,
    houseOpen: false, officeOpen: false, waving: [], waveFrame: 0, showRetry: false, progressLine: null, label: "",
  };
}

function stepOf(tMs: number, stepMs: number): number {
  return Math.floor(Math.max(0, tMs) / stepMs) % 2;
}

function wavingAt(scroll: number, active: boolean): FriendId[] {
  if (!active) return [];
  return FRIEND_HOUSES.filter((h) => Math.abs(houseX(h, scroll) - YAPPY_X) < WAVE_RANGE_PX).map((h) => h.id);
}

/** The whole walk, as data. */
export function launchWalkModel(i: WalkInput): WalkModel {
  const p = i.progress;
  const rm = i.reduceMotion;
  if (!p) {
    // A warm engine: no walk, one 600 ms hop (a still frame under reduce motion).
    if (i.warmHopMs !== null && i.warmHopMs >= 0 && i.warmHopMs < WARM_HOP_MS) {
      const k = i.warmHopMs / WARM_HOP_MS;
      return {
        ...hiddenModel(), mode: "hop", scroll: WALK_DIST, pct: 100, officeOpen: true,
        pose: { ...STILL, mood: 1, hop: rm ? 0 : Math.round(Math.sin(k * Math.PI) * 3) },
        label: "Ready",
      };
    }
    return hiddenModel();
  }
  if (p.stage === "ready") {
    // The ONLY way into the ready pose: the backend said ready.
    const since = i.sinceReadyMs ?? READY_SHOW_MS;
    if (since >= READY_SHOW_MS) return hiddenModel();
    const k = rm ? 1 : Math.min(1, since / READY_CATCHUP_MS);
    const from = scrollForPct(i.readyFromPct);
    const catching = k < 1;
    const hopT = Math.max(0, since - READY_CATCHUP_MS) / WARM_HOP_MS;
    const step = stepOf(i.tMs, RUN_STEP_MS);
    return {
      ...hiddenModel(), mode: "ready", scroll: from + (WALK_DIST - from) * k, pct: 100,
      running: catching,
      officeOpen: !catching,
      pose: catching
        ? { hop: step, flap: step * 2, sway: step ? 1 : -1, mood: 1, blink: 0 }
        : { ...STILL, mood: 1, hop: rm ? 0 : Math.round(Math.sin(Math.min(1, hopT) * Math.PI) * 3) },
      progressLine: rm ? 100 : null,
      label: "Ready",
    };
  }
  const pct = Math.min(HOLD_PCT, p.pct);
  if (p.stage === "failed") {
    // Door stays shut, he shrugs (wings up, no steps), and the pill offers Retry.
    return {
      ...hiddenModel(), mode: "failed", scroll: rm ? DOOR_SCROLL : scrollForPct(pct), pct,
      pose: { hop: 0, flap: 2, sway: 0, mood: 0, blink: 0 },
      showRetry: true, progressLine: rm ? pct : null,
      label: "The speech engine did not load",
    };
  }
  const waiting = p.stage === "waiting" || p.stage === "testing" || pct >= HOLD_PCT;
  // Reduce motion: one still frame at the office door, with a progress line.
  const scroll = rm || waiting ? DOOR_SCROLL : scrollForPct(pct);
  const stalled = !waiting && i.sinceChangeMs >= STALL_MS;
  const running = i.recording && !waiting;
  const step = rm ? 0 : stepOf(i.tMs, running ? RUN_STEP_MS : WALK_STEP_MS);
  const walkingNow = !rm && !waiting && pct > 2;
  const pose: WalkPose = waiting
    // At the door: no scrolling, he taps his foot (a 1 px sway each step).
    ? { hop: 0, flap: 0, sway: rm ? 0 : (step ? 1 : 0), mood: 0.5, blink: 0 }
    : walkingNow
      ? { hop: step, flap: step * 2, sway: step ? 1 : -1, mood: 0.3, blink: 0 }
      : { ...STILL, blink: pct === 0 ? 1 : 0 };
  const waveFrame = rm ? 0 : Math.floor(Math.max(0, i.tMs) / WAVE_FRAME_MS) % 2;
  return {
    ...hiddenModel(),
    mode: waiting ? "wait" : "walk",
    scroll, pct, stalled, running, pose,
    houseOpen: !rm && !waiting && pct < 6,
    officeOpen: false,
    waving: rm ? [] : wavingAt(scroll, pct > 0),
    waveFrame,
    progressLine: rm ? pct : null,
    label: waiting ? "Almost ready" : `Getting ready, ${pct}%`,
  };
}

/**
 * A forced walk for headless captures (scripts/pill-matrix.mjs, dev builds
 * only — `FORCED` in float-main): `walk=40`, `walk=95:waiting`,
 * `walk=60:failed`, `walk=100:ready`. Anything else is null.
 */
export function forcedLaunchWalk(search: string): EngineLoadProgress | null {
  const raw = new URLSearchParams(search).get("walk");
  if (!raw) return null;
  const [pctS, stageS] = raw.split(":");
  const pct = Number(pctS);
  const stage = stageS ?? (pct >= 100 ? "ready" : pct >= HOLD_PCT ? "waiting" : pct <= 0 ? "starting" : "reading");
  return parseLoadProgress({ pct, stage, modelId: "forced", elapsedMs: 0 });
}

/** A key that changes only when a reduce-motion still frame must repaint. */
export function stillFrameKey(m: WalkModel): string {
  return `${m.mode}|${m.pct}|${m.showRetry ? 1 : 0}`;
}
