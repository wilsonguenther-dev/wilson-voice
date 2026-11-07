/**
 * engineHealth — the speech engine's one health state, in plain words
 * (yap-1106-engine-owner).
 *
 * The Rust owner (`engine_owner.rs`) emits `engine_health` with one of six
 * lowercase names on every transition. The pill shows ONE small state; the
 * Diagnostics row shows the same words. Never a vendor or model name.
 */

export const ENGINE_HEALTH_EVENT = "engine_health";

export const ENGINE_HEALTH_STATES = [
  "idle",
  "loading",
  "ready",
  "busy",
  "stuck",
  "restarting",
] as const;

export type EngineHealth = (typeof ENGINE_HEALTH_STATES)[number];

/** What the pill says for each state. "stuck" is the FX-02 "Catching up…". */
export const ENGINE_HEALTH_LABEL: Record<EngineHealth, string> = {
  idle: "Resting",
  loading: "Warming up…",
  ready: "Ready",
  busy: "Transcribing…",
  stuck: "Catching up…",
  restarting: "Restarting…",
};

/** Whether the pill should show the chip at all: quiet states stay hidden. */
export function engineHealthVisible(state: EngineHealth): boolean {
  return state === "loading" || state === "stuck" || state === "restarting";
}

/** Parse an event payload; anything unknown is treated as idle. */
export function parseEngineHealth(payload: unknown): EngineHealth {
  return typeof payload === "string" &&
    (ENGINE_HEALTH_STATES as readonly string[]).includes(payload)
    ? (payload as EngineHealth)
    : "idle";
}

export function engineHealthLabel(state: EngineHealth): string {
  return ENGINE_HEALTH_LABEL[state];
}

/**
 * Pill phases (their `data-phase` ids) in which the engine's state is the
 * thing worth saying. yap-1106-stuck-pill-keeps-its-scene — `thinking` joins:
 * a take stuck before its first chunk sits there, and the pill must name the
 * stuck engine instead of a generic "working on it". Only there does the chip take the strip's one line; a
 * listening, error or hands-free line is never displaced.
 */
export const ENGINE_HEALTH_PHASES: readonly string[] = ["transcribing", "model_loading", "thinking"];

/** Whether the pill's strip shows the engine chip instead of its phase line. */
export function engineHealthTakesStrip(state: EngineHealth, phaseId: string): boolean {
  return engineHealthVisible(state) && ENGINE_HEALTH_PHASES.includes(phaseId);
}
