/**
 * yap-1106-yappy-states-and-motion — the phase transition table is TOTAL and
 * follows the approved sheet (design-2026-11-06/pill.html §03).
 */
import { describe, it, expect } from "vitest";
import { ALL_LIVE_PHASES, PHASE_NEXT, type LivePhase } from "./live";
import { DURATION, EASE_LINEAR, EASE_SNAP, EASE_STATE_CHANGE } from "./motion";
import {
  CURVE_EASING, PHASE_TRANSITIONS, REDUCED_RULE, phaseTransition, publishPhaseTransition, transitionCss,
} from "./transitions";

describe("PHASE_TRANSITIONS", () => {
  it("every_phase_pair_has_a_transition_entry", () => {
    // 17 x 17: every (from, to), including a phase re-entering itself.
    expect(ALL_LIVE_PHASES.length).toBe(17);
    expect(Object.keys(PHASE_TRANSITIONS).sort()).toEqual([...ALL_LIVE_PHASES].sort());
    let n = 0;
    for (const from of ALL_LIVE_PHASES) {
      expect(Object.keys(PHASE_TRANSITIONS[from]).sort(), from).toEqual([...ALL_LIVE_PHASES].sort());
      for (const to of ALL_LIVE_PHASES) {
        const t = PHASE_TRANSITIONS[from][to];
        expect(t, `${from} → ${to}`).toBeDefined();
        expect(t.from).toBe(from);
        expect(t.to).toBe(to);
        expect(Number.isFinite(t.durationMs) && t.durationMs >= 0, `${from} → ${to} duration`).toBe(true);
        expect(Number.isFinite(t.delayMs) && t.delayMs >= 0, `${from} → ${to} delay`).toBe(true);
        expect(t.easing).toBe(CURVE_EASING[t.curve]);
        expect(t.rule.length).toBeGreaterThan(0);
        if (from !== to) expect(t.durationMs, `${from} → ${to} must move`).toBeGreaterThan(0);
        n++;
      }
    }
    expect(n).toBe(17 * 17);
  });

  it("every_legal_transition_is_covered_and_short", () => {
    // Nothing the state machine can actually do takes longer than the slowest
    // row of the sheet (the 600 ms fall asleep) plus a bubble exit.
    const ceiling = DURATION.sleepFade + DURATION.bubbleOut;
    for (const from of ALL_LIVE_PHASES) {
      for (const to of PHASE_NEXT[from]) {
        const t = PHASE_TRANSITIONS[from][to];
        expect(t.durationMs + t.delayMs, `${from} → ${to}`).toBeLessThanOrEqual(ceiling);
      }
    }
  });

  it("follows_the_approved_sheet_rows", () => {
    expect(PHASE_TRANSITIONS.idle.listening).toMatchObject({ durationMs: DURATION.grow, curve: "snap", easing: EASE_SNAP });
    expect(PHASE_TRANSITIONS.idle["listening-locked"].durationMs).toBe(DURATION.grow + DURATION.phaseMorph);
    expect(PHASE_TRANSITIONS.listening.thinking).toMatchObject({ durationMs: DURATION.phaseMorph, curve: "quick", easing: EASE_STATE_CHANGE });
    expect(PHASE_TRANSITIONS.done.idle).toMatchObject({ durationMs: DURATION.collapse, delayMs: DURATION.bubbleOut, curve: "quick" });
    expect(PHASE_TRANSITIONS.idle.sleepy.durationMs).toBe(DURATION.sleepFade);
    expect(PHASE_TRANSITIONS.transcribing.error).toMatchObject({ durationMs: DURATION.shake, curve: "linear", easing: EASE_LINEAR });
    expect(PHASE_TRANSITIONS.idle.blocked.durationMs).toBe(DURATION.expand);
    expect(PHASE_TRANSITIONS.listening["listening-locked"].durationMs).toBe(DURATION.phaseMorph);
    expect(PHASE_TRANSITIONS.idle.idle.durationMs).toBe(0);
  });

  it("reduce_motion_turns_every_change_into_one_short_crossfade", () => {
    for (const from of ALL_LIVE_PHASES) {
      for (const to of ALL_LIVE_PHASES) {
        const t = phaseTransition(from, to, true);
        if (from === to) { expect(t.durationMs).toBe(0); continue; }
        expect(t).toMatchObject({ durationMs: DURATION.reducedCrossfade, delayMs: 0, curve: "quick", rule: REDUCED_RULE });
      }
    }
    expect(phaseTransition("idle", "listening", false)).toBe(PHASE_TRANSITIONS.idle.listening);
  });

  it("publishes_the_row_as_css_variables", () => {
    const set = new Map<string, string>();
    const el = { style: { setProperty: (k: string, v: string) => set.set(k, v) }, dataset: {} as Record<string, string> };
    const t = PHASE_TRANSITIONS.done.idle;
    publishPhaseTransition(el as unknown as HTMLElement, t);
    expect(set.get("--phase-dur")).toBe(`${t.durationMs}ms`);
    expect(set.get("--phase-ease")).toBe(t.easing);
    expect(set.get("--phase-delay")).toBe(`${t.delayMs}ms`);
    expect(el.dataset.transition).toBe("quick");
    expect(transitionCss("opacity", t)).toBe(`opacity ${t.durationMs}ms ${t.easing} ${t.delayMs}ms`);
  });

  it("phases_are_the_live_union", () => {
    // A compile-time guard as much as a runtime one: the table is keyed by
    // LivePhase, so this assignment fails tsc if a key goes missing.
    const keys: Record<LivePhase, true> = Object.fromEntries(ALL_LIVE_PHASES.map((p) => [p, true])) as Record<LivePhase, true>;
    expect(Object.keys(keys).length).toBe(Object.keys(PHASE_TRANSITIONS).length);
  });
});
