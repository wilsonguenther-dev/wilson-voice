/**
 * yap-1106-yappy-states-and-motion — THE phase transition table.
 *
 * Every (from → to) pair of `LivePhase` has exactly one entry here: how long
 * the change takes, on which curve, and how long the shell waits first (the
 * bubble leaves before the shell shrinks). It is DATA, built once from the
 * approved transition sheet (design-2026-11-06/pill.html §03), and
 * transitions.test.ts walks all 17 x 17 pairs so a phase added to `LivePhase`
 * without a row here fails the build instead of morphing on a guessed curve.
 *
 * No time literal lives in this file: every number comes from `DURATION` and
 * every curve from the `EASE_*` constants in motion.ts (the
 * `no_duration_literal_outside_motion_ts` sweep enforces that).
 *
 * Reduce motion: every entry collapses to one 100 ms crossfade, no delay, no
 * spring, no shake — see [`phaseTransition`].
 */
import { ALL_LIVE_PHASES, type LivePhase } from "./live";
import { DURATION, EASE_LINEAR, EASE_OUT, EASE_SETTLE, EASE_SNAP, EASE_STATE_CHANGE } from "./motion";

/** The five curves the sheet allows. Two in practice: quick and snap. */
export type TransitionCurve = "quick" | "snap" | "settle" | "ease-out" | "linear";

/** The CSS easing for each curve name. */
export const CURVE_EASING: Record<TransitionCurve, string> = {
  quick: EASE_STATE_CHANGE,
  snap: EASE_SNAP,
  settle: EASE_SETTLE,
  "ease-out": EASE_OUT,
  linear: EASE_LINEAR,
};

/** One row of the table. */
export interface PhaseTransition {
  from: LivePhase;
  to: LivePhase;
  /** The shell's own change, in ms. 0 only for a phase re-entering itself. */
  durationMs: number;
  /** How long the shell waits before moving (the bubble's exit), in ms. */
  delayMs: number;
  curve: TransitionCurve;
  /** The CSS easing string for `curve`. */
  easing: string;
  /** Which row of the approved sheet this pair follows (plain words, for logs/tests). */
  rule: string;
}

/** The families the sheet talks in. */
type Family = "rest" | "talk" | "work" | "landed" | "outcome" | "refusal" | "fail";

const FAMILY: Record<LivePhase, Family> = {
  idle: "rest", sleepy: "rest", blind: "rest",
  listening: "talk", "listening-locked": "talk",
  "model-loading": "work", transcribing: "work", polishing: "work", pasting: "work", thinking: "work",
  done: "landed",
  empty: "outcome", cancelled: "outcome",
  error: "fail",
  gated: "refusal", waiting: "refusal", blocked: "refusal",
};

type Row = Pick<PhaseTransition, "durationMs" | "delayMs" | "curve" | "rule">;

/** The sheet, as rules. Order matters: the first rule that matches wins. */
function ruleFor(from: LivePhase, to: LivePhase): Row {
  const f = FAMILY[from], t = FAMILY[to];
  if (from === to) return { durationMs: 0, delayMs: 0, curve: "quick", rule: "same phase: nothing moves" };
  if (from === "idle" && to === "sleepy") return { durationMs: DURATION.sleepFade, delayMs: 0, curve: "quick", rule: "resting → asleep: eyes close, shell fades" };
  if (f === "rest" && to === "listening") return { durationMs: DURATION.grow, delayMs: 0, curve: "snap", rule: "resting → holding fn: shell grows" };
  if (f === "rest" && to === "listening-locked") {
    return { durationMs: DURATION.grow + DURATION.phaseMorph, delayMs: 0, curve: "snap", rule: "resting → hands-free: grow, then the lock segment fades in" };
  }
  if (f === "talk" && t === "talk") return { durationMs: DURATION.phaseMorph, delayMs: 0, curve: "quick", rule: "held ↔ hands-free: the right segment crossfades" };
  if (t === "fail") return { durationMs: DURATION.shake, delayMs: 0, curve: "linear", rule: "failed take: one shake, then still" };
  if (t === "rest" && f !== "rest") {
    return { durationMs: DURATION.collapse, delayMs: DURATION.bubbleOut, curve: "quick", rule: "any → resting: bubble leaves first, then the shell shrinks" };
  }
  if (t === "refusal" && f !== "refusal") return { durationMs: DURATION.expand, delayMs: 0, curve: "quick", rule: "a refusal: the capsule grows for its note" };
  if (f === "work" && t === "landed") return { durationMs: DURATION.phaseMorph, delayMs: 0, curve: "quick", rule: "writing it up → pasted: check and count crossfade in" };
  if (f === "talk" && t === "work") return { durationMs: DURATION.phaseMorph, delayMs: 0, curve: "quick", rule: "talking → writing it up: size holds, segment crossfades" };
  return { durationMs: DURATION.phaseMorph, delayMs: 0, curve: "quick", rule: "between two states: quick crossfade" };
}

function build(): Record<LivePhase, Record<LivePhase, PhaseTransition>> {
  const out = {} as Record<LivePhase, Record<LivePhase, PhaseTransition>>;
  for (const from of ALL_LIVE_PHASES) {
    const row = {} as Record<LivePhase, PhaseTransition>;
    for (const to of ALL_LIVE_PHASES) {
      const r = ruleFor(from, to);
      row[to] = { from, to, ...r, easing: CURVE_EASING[r.curve] };
    }
    out[from] = row;
  }
  return out;
}

/** THE table: `PHASE_TRANSITIONS[from][to]`, total over every phase pair. */
export const PHASE_TRANSITIONS: Readonly<Record<LivePhase, Readonly<Record<LivePhase, PhaseTransition>>>> = build();

/** The reduced-motion row: one short crossfade, nothing else. */
export const REDUCED_RULE = "reduce motion: one short crossfade";

/** The transition to run for `from → to`, honouring reduce motion. */
export function phaseTransition(from: LivePhase, to: LivePhase, reduceMotion = false): PhaseTransition {
  const t = PHASE_TRANSITIONS[from][to];
  if (!reduceMotion || from === to) return t;
  return { from, to, durationMs: DURATION.reducedCrossfade, delayMs: 0, curve: "quick", easing: CURVE_EASING.quick, rule: REDUCED_RULE };
}

/** `transition` shorthand for one property following a table row. */
export const transitionCss = (property: string, t: PhaseTransition): string =>
  `${property} ${t.durationMs}ms ${t.easing} ${t.delayMs}ms`;

/**
 * Publish a row onto an element as the CSS variables float.css consumes
 * (`--phase-dur`, `--phase-ease`, `--phase-delay`) plus `data-transition`, the
 * curve name a test or a screenshot can read back.
 */
export function publishPhaseTransition(el: HTMLElement, t: PhaseTransition): void {
  el.style.setProperty("--phase-dur", `${t.durationMs}ms`);
  el.style.setProperty("--phase-ease", t.easing);
  el.style.setProperty("--phase-delay", `${t.delayMs}ms`);
  el.dataset.transition = t.curve;
}
