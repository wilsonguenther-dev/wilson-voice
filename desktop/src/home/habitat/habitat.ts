/**
 * Y5-K — the habitat's DIRECTOR (the reinstated Y5-H; design note in
 * docs/loop/DEFERRED.md §1). Pure: no canvas, no Tauri, no dice.
 *
 * Lifted out of YappyHouse's closure, where it was a random walk between
 * stations. Now:
 *   - ROUTINES run on the REAL clock: the hour picks a routine, the minute picks
 *     which of its stations is the current INTENT, so the same moment always
 *     means the same place (sleep in the nest at 3am, the desk mid-morning).
 *   - INTENT PATHING: the creature walks straight to its intent and acts there;
 *     it never wanders past it and never picks a spot by dice.
 *   - EVENT-DRIVEN REACTIONS: a take starting, a paste landing, a model
 *     finishing its download. `reactTo` is fed by home/habitat/events.ts, which
 *     subscribes to the same backend events the pill does — it never polls.
 *
 * The scene (pod interior, depth, parallax) is drawn by home/YappyHouse.tsx off
 * this state; the creature's mood words come from its character module.
 */

export type HabitatAppState = "idle" | "listening" | "thinking" | "done";
export type StationId = "nest" | "window" | "plant" | "coffee" | "desk";

export interface Station {
  /** Floor position in scene art px. */
  x: number;
  pose: string;
  face?: number;
}

export const STATIONS: Record<StationId, Station> = {
  nest: { x: 26, pose: "sleep" },
  window: { x: 52, pose: "look" },
  plant: { x: 74, pose: "water" },
  coffee: { x: 96, pose: "sip" },
  desk: { x: 121, pose: "work", face: 1 },
};

export interface Routine {
  id: string;
  /** Local hour (0..23) the routine starts at; it runs until the next one. */
  from: number;
  /** Visited in order, one per `ROUTINE_STEP_MIN` minutes of real clock. */
  stations: readonly StationId[];
}

/** The day, in order of `from`. The first row must start at 0. */
export const ROUTINES: readonly Routine[] = [
  { id: "sleep", from: 0, stations: ["nest"] },
  { id: "wake", from: 7, stations: ["window", "coffee"] },
  { id: "work", from: 9, stations: ["desk", "coffee", "desk", "window"] },
  { id: "lunch", from: 12, stations: ["plant", "window"] },
  { id: "work", from: 13, stations: ["desk", "plant", "desk", "coffee"] },
  { id: "unwind", from: 18, stations: ["window", "plant", "coffee"] },
  { id: "wind-down", from: 21, stations: ["window", "nest"] },
  { id: "sleep", from: 22, stations: ["nest"] },
];

export const ROUTINE_STEP_MIN = 7;
/** Walk speed, art px per second; an app-state rush is `RUSH` times faster. */
export const WALK_PX_PER_S = 22;
export const RUSH = 1.7;
export const ARRIVE_PX = 1.2;
/** A landed paste holds the happy "done" reaction this long. */
export const DONE_HOLD_MS = 2000;
/** A finished model download gets a cheer, in place, this long. */
export const CHEER_MS = 2400;

/** Real clock -> sky phase. 0 dawn · .25 day · .5 dusk · .75 night (wraps). */
export function skyPhase(d: Date): number {
  const hoursDecimal = d.getHours() + d.getMinutes() / 60;
  return (((hoursDecimal - 6 + 24) % 24) / 24) % 1;
}

/** The routine running at `d` (local time). */
export function routineAt(d: Date): Routine {
  const h = d.getHours();
  let cur = ROUTINES[0];
  for (const r of ROUTINES) if (r.from <= h) cur = r;
  return cur;
}

/** Where the creature means to be at `d`: the routine's station for this minute. */
export function intentAt(d: Date): StationId {
  const r = routineAt(d);
  const minutesIn = (d.getHours() - r.from) * 60 + d.getMinutes();
  return r.stations[Math.floor(minutesIn / ROUTINE_STEP_MIN) % r.stations.length];
}

export interface DirectorState {
  appState: HabitatAppState;
  x: number;
  face: number;
  activity: string;
  station: StationId;
  mode: "walk" | "act";
  /** Epoch ms the "done" reaction holds until. */
  doneUntil: number;
  /** In-place reaction (a model landed) and its end, epoch ms. */
  reaction: "cheer" | null;
  reactUntil: number;
  /** Mic energy 0..1 from the live level event (lights the window beam). */
  energy: number;
}

export function createDirector(): DirectorState {
  return {
    appState: "idle",
    x: STATIONS.nest.x,
    face: 1,
    activity: "sleep",
    station: "nest",
    mode: "act",
    doneUntil: 0,
    reaction: null,
    reactUntil: 0,
    energy: 0,
  };
}

export type HabitatEvent =
  | { kind: "take-start" }
  | { kind: "take-busy" }
  | { kind: "take-idle" }
  | { kind: "paste" }
  | { kind: "model-ready" }
  | { kind: "level"; value: number };

/** The backend's `status` payload -> a habitat event. */
export function eventForStatus(s: { recording?: boolean; busy?: boolean } | null | undefined): HabitatEvent {
  if (s?.recording) return { kind: "take-start" };
  if (s?.busy) return { kind: "take-busy" };
  return { kind: "take-idle" };
}

/**
 * `model_download_progress` -> `model-ready`, ONCE per model id: the first
 * payload whose `downloaded` reached a positive `total`. `seen` is the caller's.
 */
export function modelReadyFrom(
  p: { model_id?: string; downloaded?: number; total?: number } | null | undefined,
  seen: Set<string>,
): HabitatEvent | null {
  if (!p || !(typeof p.total === "number" && p.total > 0)) return null;
  if (!(typeof p.downloaded === "number" && p.downloaded >= p.total)) return null;
  const id = p.model_id ?? "";
  if (seen.has(id)) return null;
  seen.add(id);
  return { kind: "model-ready" };
}

/** Apply one event. `nowMs` is epoch ms (injected so tests own the clock). */
export function reactTo(s: DirectorState, ev: HabitatEvent, nowMs: number): void {
  switch (ev.kind) {
    case "take-start":
      s.appState = "listening";
      return;
    case "take-busy":
      s.appState = "thinking";
      return;
    case "take-idle":
      // A landed paste keeps its reaction until the hold runs out.
      if (nowMs > s.doneUntil) s.appState = "idle";
      return;
    case "paste":
      s.appState = "done";
      s.doneUntil = nowMs + DONE_HOLD_MS;
      return;
    case "model-ready":
      s.reaction = "cheer";
      s.reactUntil = nowMs + CHEER_MS;
      return;
    case "level":
      s.energy = Math.max(0, Math.min(1, Number.isFinite(ev.value) ? ev.value : 0));
      return;
  }
}

function walkToward(s: DirectorState, tx: number, dt: number, speed: number): boolean {
  const step = WALK_PX_PER_S * speed * dt;
  if (Math.abs(s.x - tx) <= ARRIVE_PX) {
    s.x = tx;
    return true;
  }
  if (s.x < tx) {
    s.x = Math.min(tx, s.x + step);
    s.face = 1;
  } else {
    s.x = Math.max(tx, s.x - step);
    s.face = -1;
  }
  s.mode = "walk";
  s.activity = "walk";
  if (Math.abs(s.x - tx) > ARRIVE_PX) return false;
  s.x = tx;
  return true;
}

/** Advance the director by `dt` seconds at real time `now`. */
export function stepDirector(s: DirectorState, dt: number, now: Date): void {
  const nowMs = now.getTime();
  if (s.appState === "done" && nowMs > s.doneUntil) s.appState = "idle";
  if (s.reaction !== null && nowMs > s.reactUntil) s.reaction = null;

  // A take outranks everything: rush to the desk and work (or beam, once done).
  if (s.appState !== "idle") {
    s.station = "desk";
    if (!walkToward(s, STATIONS.desk.x, dt, RUSH)) return;
    s.face = 1;
    s.mode = "act";
    s.activity = s.appState === "done" ? "happy" : "work";
    return;
  }
  // A reaction plays where the creature stands.
  if (s.reaction === "cheer") {
    s.mode = "act";
    s.activity = "happy";
    return;
  }
  // The routine: walk to the clock's intent and act there.
  const intent = intentAt(now);
  s.station = intent;
  const st = STATIONS[intent];
  if (!walkToward(s, st.x, dt, 1)) return;
  s.mode = "act";
  s.activity = st.pose;
  s.face = st.face ?? s.face;
}

/** The mood the habitat names, from the director's real state. */
export function moodOf(s: DirectorState): "listening" | "thinking" | "done" | "nap" | "work" | "idle" {
  if (s.appState === "listening" || s.appState === "thinking" || s.appState === "done") return s.appState;
  if (s.reaction === "cheer") return "done";
  if (s.activity === "sleep") return "nap";
  if (s.activity === "work") return "work";
  return "idle";
}
