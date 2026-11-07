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
 * tightens the rest threshold for the soft body, to `SOFT_REST_DELTA`. That
 * alone is NOT enough to be seen: on this integrator the free overshoot is
 * ~2.1% of the squash, a sub-pixel stretch. The visible bounce comes from the
 * release/dock velocity impulses (`RELEASE_KICK`, `DOCK_KICK`), and
 * `motion.test.ts` asserts it in pixels against float.css's seed capsule.
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
  /**
   * Y5-E — how long the cursor must stay OUTSIDE the expanded hot area before
   * the hover-expanded pill collapses. Collapse only: expanding on enter is
   * immediate (a lag there makes the pill feel dead). This is a debounce on the
   * way OUT, not the oscillation fix — the 12 px margin below is (geometry).
   */
  hoverCollapse: 160,
} as const;

/**
 * Y5-E — the hover hysteresis thresholds, in the same table as the durations.
 *
 * `marginPx` is the invisible alpha margin the EXPANDED pill paints around its
 * capsule (`.pill-hot[data-hover="expanded"]::before { inset: -12px }` in
 * float.css) and publishes to the panel's hitbox. hitbox.test.ts reads the CSS
 * literal back and fails if the two ever disagree: a margin CSS believes in and
 * the panel does not is worse than no margin.
 */
export const HOVER = {
  marginPx: 12,
  collapseAfterMs: DURATION.hoverCollapse,
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
   * Add `dv` (value-units per second) to the CURRENT velocity, keeping the
   * value, and wake the spring. This is how a gesture puts energy into the body
   * without teleporting it: the value stays continuous, so nothing pops.
   * Under Reduce Motion it does nothing at all.
   */
  kick(dv: number): void;
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
    kick(dv: number) {
      if (reduce || dv === 0) return;
      velocity += dv;
      rest = false;
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
 * back at 0 on the SNAP spring with a velocity impulse sized to the stored
 * squash (RELEASE_KICK), so it swings past zero into a visible stretch, and
 * the decay of that oscillation is the settle. Nothing here keyframes.
 *
 * `dock()` uses the SETTLE spring instead, because a capsule dropped into a
 * corner should land heavier and slower than a capsule you tapped.
 */
export const SQUASH_PRESS = 1;
/**
 * The least landing energy a dock carries, as a squash-equivalent. A dock
 * normally follows a press (the drag starts on pointerdown), so the body is
 * already near +1; this floor only matters if a dock ever arrives on a body at
 * rest, so the landing still bounces instead of doing nothing.
 */
export const SQUASH_DOCK = 0.55;
/**
 * Scale change at full squash. Soft, never cartoonish.
 *
 * Y5-I — 0.06 was tuned on the old 56px resting capsule (a 3.36px squash). The
 * resting bar is now Wispr's 40px (pill/dock-geometry.json), so the scale rises
 * to keep the SAME pixel amplitude on it: 0.084 x 40 = 3.36px. Without this a
 * released tap stretched the resting bar 0.8px, under one device pixel at 1x
 * (motion.test `release_stretch_is_visible_in_pixels_on_the_seed_capsule`).
 */
export const SQUASH_SCALE = 0.084;

/**
 * Y5-D review fix — WHY THE REBOUND NEEDS A KICK. The snap spring's free
 * overshoot past 0, measured on this integrator, is ~2.1% of the squash (the
 * 4.04% analytic figure is for the continuous spring; semi-implicit Euler at
 * 1/120s damps more). Times SQUASH_SCALE that is a 0.13% scale change: 0.07px
 * on the 56px seed capsule. Nobody can see that, so a plain release looked
 * rigid.
 *
 * A real soft body that is let go releases the energy it stored in the squash:
 * it does not merely relax, it springs back PAST its shape. That is modelled as
 * a velocity impulse proportional to the stored squash, applied on release, on
 * top of the same snap spring. The value never jumps (no teleport), the damping
 * is untouched (so it still settles in a few hundred ms with no wobble), and the
 * trough becomes a stretch a human can see — `motion.test.ts` asserts it in
 * PIXELS against the seed capsule, not as a sign.
 *
 * Units: value-units per second, per unit of stored squash.
 */
export const RELEASE_KICK = 70;
/**
 * The landing impulse when a drag drops the pill into a dock, on the slower
 * SETTLE spring: a heavier, longer bounce than a tap's rebound.
 */
export const DOCK_KICK = 60;

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
    release() {
      use(SPRING_SNAP);
      // Release the energy stored in the squash (see RELEASE_KICK): the value
      // stays where it is, only the velocity changes.
      const stored = Math.max(spring.value, 0);
      spring.setTarget(0);
      spring.kick(-RELEASE_KICK * stored);
    },
    dock() {
      // KEEP value and velocity — the body is squashed from the press that
      // started the drag, and teleporting it would pop the capsule's shape in a
      // single frame. Swap to the slower spring and kick it: it rebounds out of
      // the squash into a visible stretch, then settles.
      use(SPRING_SETTLE);
      const stored = Math.max(spring.value, SQUASH_DOCK);
      spring.setTarget(0);
      spring.kick(-DOCK_KICK * stored);
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
