/**
 * motion — Yap's soft-body pill physics, and the ONE table of durations the
 * pill is allowed to animate with.
 *
 * WHY THESE NUMBERS. They are measured, not invented: the Wispr Flow parity
 * teardown pulled the app's own `motion` spring configs out of its asar bundle.
 * Wispr morphs its pill with `stiffness: 600, damping: 35, restDelta: 0.05`,
 * runs its slower transitions on `stiffness: 300, damping: 28`, and eases plain
 * state changes with `cubic-bezier(0.05, 0.6, 0.4, 0.95)` over 100ms, reserving
 * 300-400ms for expand/collapse. Yap uses the same constants so the pill feels
 * like the thing Wilson benchmarked it against — and then goes further, because
 * Wispr's pill is rigid and Wilson's ask was that Yappy "bounces when touched,
 * feels soft not stiff".
 *
 * WHY THERE IS NO MOTION LIBRARY. Three reasons, all binding:
 *   1. The float window runs under a CSP that blocks external hosts, and adding
 *      framer-motion/GSAP to the float bundle is ~30-100kB on a window that has
 *      to paint before the first syllable lands.
 *   2. YV81's energy pass deleted the pill's busy timers on purpose and YV24
 *      idle-throttles its canvases. A library's animation loop owns its own
 *      rAF; this integrator is TICKED BY the loop `ClassicPill` already runs, so
 *      the park-at-rest discipline survives intact.
 *   3. A spring you can't inject a timestep into can't be unit-tested. Every
 *      claim in `motion.test.ts` is made against an injected `dt` and real time
 *      is never read.
 *
 * THE ONE PLACE YAP DEPARTS FROM WISPR, AND WHY. Wispr's `restDelta: 0.05` is
 * 5% of a 0..1 morph, and the spring's OWN analytic overshoot at damping 35 is
 * 4.04%. The rest threshold is therefore larger than the bounce: Wispr's pill
 * provably cannot overshoot its own target, which is exactly why it reads as
 * rigid. That is measured, not asserted — `motion.test.ts` runs the integrator
 * at Wispr's raw config and records a peak of 0.987, never past 1.0.
 *
 * So Yap keeps Wispr's stiffness and damping verbatim (the SPEED is right) and
 * tightens ONLY the rest threshold for the soft body, to `SOFT_REST_DELTA`.
 * Same physics, finer stopping rule, and the 4% bounce survives to the screen.
 *
 * WHAT "CRITICALLY DAMPED" MEANS HERE, PRECISELY. With mass 1, critical damping
 * is `2*sqrt(stiffness)`: 48.99 for k=600, 34.64 for k=300. Both named springs
 * sit just UNDER that (zeta = 0.714 and 0.808), which is deliberate — a
 * perfectly critically-damped spring never overshoots, and the overshoot IS the
 * bounce. The overshoot of a unit step for damping ratio z is
 * `exp(-pi*z/sqrt(1-z*z))`: ~4.0% for the snap spring and ~1.3% for the settle
 * spring. Both sit far under SOFT_LIMIT, which `motion.test.ts` asserts, so
 * "soft" can never silently become "rubbery".
 */

/** A named spring, in the units Wispr's own config uses (mass is always 1). */
export interface SpringConfig {
  /** Restoring force per unit of displacement. */
  stiffness: number;
  /** Velocity-proportional drag. Below `2*sqrt(stiffness)` the spring bounces. */
  damping: number;
  /** Displacement under which the spring is considered arrived. */
  restDelta: number;
}

/**
 * The SNAP spring — phase morphs, the press squash, the release stretch.
 * Wispr's measured `stiffness: 600 damping: 35 restDelta: 0.05`.
 */
export const SPRING_SNAP: SpringConfig = { stiffness: 600, damping: 35, restDelta: 0.05 };

/**
 * The SETTLE spring — the slower one: the bounce the capsule does when a drag
 * drops it into a dock. Wispr's measured `stiffness: 300 damping: 28`; it does
 * not publish a restDelta for this one, so it inherits the snap spring's.
 */
export const SPRING_SETTLE: SpringConfig = { stiffness: 300, damping: 28, restDelta: 0.05 };

/**
 * The largest fraction of the travel a spring in this module may overshoot its
 * target by. Both named springs overshoot ~4% and ~1.3% analytically; this is
 * the guard rail that keeps a future constant change from turning a soft pill
 * into a wobbling one, and `spring_never_overshoots_past_the_soft_limit`
 * enforces it against the integrator rather than against the algebra.
 */
export const SOFT_LIMIT = 0.12;

/**
 * The soft body's rest threshold, replacing Wispr's 0.05 (see the header).
 *
 * It is deliberately ~20x finer: the squash scalar is multiplied by
 * `SQUASH_SCALE` on its way to a CSS transform, so stopping it 0.05 short of
 * target would clip the entire gesture. At 0.002 the spring's own 4% overshoot
 * is 20x the threshold and survives, while the loop still parks in well under
 * half a second (`integrator_reports_at_rest_and_stops` bounds it).
 */
export const SOFT_REST_DELTA = 0.002;

/** Wispr's spring, at Yap's finer stopping rule. Stiffness/damping untouched. */
export const softened = (cfg: SpringConfig): SpringConfig =>
  ({ ...cfg, restDelta: SOFT_REST_DELTA });

/**
 * THE duration table. Nothing under `src/pill` may hardcode a duration; the
 * sweep test `no_duration_literal_outside_motion_ts` fails the build if it does.
 * Values are milliseconds.
 */
export const DURATION = {
  /** Every morph between two Y5-C phases. Wispr's state-change timing. */
  phaseMorph: 100,
  /** The capsule growing (blocked note, meeting, transcribe bar). */
  expand: 340,
  /** The capsule shrinking back to the seed. Slightly quicker than expand. */
  collapse: 300,
  /** How long Classic holds its completion check before returning to rest. */
  doneFlash: 1100,
  /** How long Yappy holds its `done` phase before falling back to `idle`. */
  doneHold: 1600,
} as const;

/** Wispr's measured state-change easing, for the CSS side of a morph. */
export const EASE_STATE_CHANGE = "cubic-bezier(0.05, 0.6, 0.4, 0.95)";

/** `transition` shorthand for one property morphing between phases. */
export const morphTransition = (property: string): string =>
  `${property} ${DURATION.phaseMorph}ms ${EASE_STATE_CHANGE}`;

/**
 * True when the OS asks for less motion. Every animated thing in this module
 * routes through here, and callers hand the result to `createSpring` so the
 * decision is made once per mount instead of per frame.
 *
 * Safe outside a browser (the unit tests run in plain node), where it reports
 * false — the tests that care inject the flag explicitly.
 */
export function prefersReducedMotion(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** The largest timestep a single integration substep may take, in seconds. */
const MAX_SUBSTEP = 1 / 120;
/** dt above this is a tab that was backgrounded, not a frame. Clamp, never leap. */
const MAX_FRAME = 1 / 15;

export interface Spring {
  /** Current value. Read this; never write it. */
  readonly value: number;
  /** Current velocity, in value-units per second. */
  readonly velocity: number;
  /** Where the spring is heading. */
  readonly target: number;
  /** True once the spring has arrived AND stopped moving. */
  readonly atRest: boolean;
  /** Aim the spring somewhere new. Wakes it if it had parked. */
  setTarget(next: number): void;
  /** Teleport: value and target both, velocity zeroed. Used by Reduce Motion. */
  jumpTo(next: number): void;
  /**
   * Swap which named spring the integrator obeys, KEEPING the current value and
   * velocity. `SoftBody` needs this: a drag that ends while the press spring is
   * still moving hands its momentum to the settle spring, and re-creating the
   * spring would throw that momentum away and make the landing look stapled on.
   */
  setConfig(next: SpringConfig): void;
  /**
   * Integrate one frame. Returns TRUE while the caller must keep scheduling
   * frames and FALSE the moment the spring is at rest — so the rAF loop that
   * ticks it can park itself instead of burning 60fps on a still pill.
   */
  tick(dtSec: number): boolean;
}

/**
 * A hand-written semi-implicit (symplectic) Euler spring.
 *
 * Semi-implicit rather than explicit Euler because explicit Euler ADDS energy
 * to an oscillator every step: an explicit integrator at 60fps on k=600 drifts
 * outward and the "soft" pill slowly shakes itself apart. Semi-implicit is
 * energy-stable for this stiffness at this timestep, and the substep clamp
 * keeps it stable if a frame arrives late.
 *
 * `reduceMotion` is not a cosmetic switch: with it on the spring NEVER animates
 * at all — `setTarget` lands on the value immediately and `tick` reports at rest
 * on its first call, so the caller's loop parks on frame one.
 */
export function createSpring(
  config: SpringConfig,
  initial = 0,
  opts: { reduceMotion?: boolean } = {},
): Spring {
  const reduce = opts.reduceMotion === true;
  let cfg = config;
  let value = initial;
  let velocity = 0;
  let target = initial;
  let rest = true;
  // A spring is "stopped" when it is BOTH close enough and slow enough. Tying
  // the speed threshold to restDelta and the spring's own natural frequency
  // (sqrt(k), rad/s) means a stiffer spring is allowed to stop while moving
  // faster — the alternative, a fixed speed constant, leaves the snap spring
  // jittering for frames after it is visually still.
  const restSpeed = () => cfg.restDelta * Math.sqrt(cfg.stiffness);

  const settle = () => {
    value = target;
    velocity = 0;
    rest = true;
  };

  return {
    get value() { return value; },
    get velocity() { return velocity; },
    get target() { return target; },
    get atRest() { return rest; },
    setTarget(next: number) {
      if (next === target && rest) return;
      target = next;
      if (reduce) { settle(); return; }
      rest = false;
    },
    jumpTo(next: number) {
      target = next;
      settle();
    },
    setConfig(next: SpringConfig) {
      cfg = next;
    },
    tick(dtSec: number): boolean {
      if (reduce) { settle(); return false; }
      if (rest) return false;
      // Clamp a long frame instead of integrating it: a 2s dt would throw the
      // spring across the screen, and a backgrounded window is exactly where
      // that happens.
      let remaining = Math.min(Math.max(dtSec, 0), MAX_FRAME);
      while (remaining > 0) {
        const h = Math.min(remaining, MAX_SUBSTEP);
        remaining -= h;
        const displacement = value - target;
        const accel = -cfg.stiffness * displacement - cfg.damping * velocity;
        velocity += accel * h;   // velocity FIRST — this is what makes it symplectic
        value += velocity * h;
      }
      if (Math.abs(value - target) < cfg.restDelta && Math.abs(velocity) < restSpeed()) {
        settle();
        return false;
      }
      return true;
    },
  };
}

/**
 * The soft body: one scalar that IS the pill's squash.
 *
 * `+1` is fully squashed (wider, shorter — pressed), `-1` fully stretched
 * (narrower, taller — the rebound), `0` is the capsule's own shape. The bounce
 * is not scripted: `press()` drives the spring to +1 and `release()` aims it
 * back at 0 on the SNAP spring, whose 4% undershoot past zero is the stretch,
 * and the decay of that oscillation is the settle. Nothing here keyframes.
 *
 * `dock()` uses the SETTLE spring instead, because a capsule dropped into a
 * corner should land heavier and slower than a capsule you tapped.
 */
export const SQUASH_PRESS = 1;
/** How much of the squash a landing dock kicks up. Lighter than a press. */
export const SQUASH_DOCK = 0.55;
/** Scale change at full squash: 6% wider, 6% shorter. Soft, never cartoonish. */
export const SQUASH_SCALE = 0.06;

export interface SoftBody {
  /** The squash scalar, -1..+1 (bounded in practice by SOFT_LIMIT overshoot). */
  readonly squash: number;
  /** True when the body is still and the caller may stop scheduling frames. */
  readonly atRest: boolean;
  /** Pointer went down on the capsule. */
  press(): void;
  /** Pointer came back up — rebound through the stretch and settle. */
  release(): void;
  /** A drag just dropped the pill into a dock: land, then bounce out of it. */
  dock(): void;
  /** Integrate; returns true while frames are still needed. */
  tick(dtSec: number): boolean;
  /** `scaleX,scaleY` for the current squash, ready for a CSS transform. */
  scale(): { x: number; y: number };
}

export function createSoftBody(opts: { reduceMotion?: boolean } = {}): SoftBody {
  const reduce = opts.reduceMotion === true;
  // Two configs, one spring: swapped by re-creating is wasteful and loses
  // velocity mid-gesture, so the active config is a variable the integrator
  // reads. A drag that ends while the press spring is still moving therefore
  // keeps its momentum into the dock bounce, which is what "soft" means.
  const spring = createSpring(softened(SPRING_SNAP), 0, { reduceMotion: reduce });
  const use = (next: SpringConfig) => spring.setConfig(softened(next));
  return {
    get squash() { return spring.value; },
    get atRest() { return spring.atRest; },
    press() { use(SPRING_SNAP); spring.setTarget(SQUASH_PRESS); },
    release() { use(SPRING_SNAP); spring.setTarget(0); },
    dock() {
      use(SPRING_SETTLE);
      // Land squashed, then let the settle spring carry it back out: one call,
      // and the overshoot past 0 is the bounce Wilson asked for.
      spring.jumpTo(SQUASH_DOCK);
      spring.setTarget(0);
    },
    tick(dtSec: number): boolean {
      return spring.tick(dtSec);
    },
    scale() {
      const s = spring.value * SQUASH_SCALE;
      return { x: 1 + s, y: 1 - s };
    },
  };
}

/**
 * Publish the duration table to CSS as custom properties, once, on the float
 * document's root.
 *
 * This is what makes "one table" TRUE rather than aspirational: `float.css`
 * spells its pill transitions `var(--dur-expand) var(--ease-state)`, so the
 * timings a human actually sees come from `DURATION` above and from nowhere
 * else. The CSS keeps literal fallbacks only for the frames before this runs.
 */
export function applyMotionVars(root: HTMLElement): void {
  root.style.setProperty("--dur-morph", `${DURATION.phaseMorph}ms`);
  root.style.setProperty("--dur-expand", `${DURATION.expand}ms`);
  root.style.setProperty("--dur-collapse", `${DURATION.collapse}ms`);
  root.style.setProperty("--ease-state", EASE_STATE_CHANGE);
}
