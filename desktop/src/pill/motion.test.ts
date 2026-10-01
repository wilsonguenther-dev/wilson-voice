/**
 * motion.test — the pill's physics, proved without a clock.
 *
 * Every test injects its own timestep. Nothing here reads `performance.now`,
 * nothing sleeps, and nothing depends on a browser painting: that is the whole
 * reason the integrator takes `dt` instead of owning a rAF. A spring that only
 * "looks right" in a running app is a spring nobody can regression-test.
 */
import { describe, it, expect } from "vitest";
// The float window's stylesheet as TEXT. `?raw` is Vite's own loader, typed by
// `vite/client`, so this is the same file the bundle ships and cannot drift.
import FLOAT_CSS from "../float.css?raw";
import {
  createSpring, createSoftBody, DURATION, EASE_STATE_CHANGE, morphTransition,
  SOFT_LIMIT, SOFT_REST_DELTA, softened, SPRING_SETTLE, SPRING_SNAP, SQUASH_SCALE,
} from "./motion";

/**
 * The pill's own sources, read through Vite rather than `node:fs`.
 *
 * Deliberate: this package has no `@types/node`, and the sweep below has to run
 * in the SAME resolution universe the app is built in — a glob Vite itself
 * expands cannot drift from what actually ships, and cannot silently sweep an
 * empty directory the way a hand-built filesystem path can.
 */
const PILL_SOURCES = import.meta.glob("./*.{ts,tsx}", {
  query: "?raw", import: "default", eager: true,
}) as Record<string, string>;


/** One 60fps frame, in seconds — the timestep the pill's rAF loop really sees. */
const FRAME = 1 / 60;

/** Drive a spring to rest, returning how many frames it took (capped). */
function run(spring: ReturnType<typeof createSpring>, cap = 1000) {
  let ticks = 0;
  let peak = spring.value;
  let trough = spring.value;
  while (ticks < cap && spring.tick(FRAME)) {
    ticks += 1;
    peak = Math.max(peak, spring.value);
    trough = Math.min(trough, spring.value);
  }
  return { ticks, peak, trough, running: spring.tick(FRAME) };
}

describe("spring integrator", () => {
  it("spring_600_35_settles_within_the_expected_tick_budget", () => {
    // zeta = 35 / (2*sqrt(600)) = 0.714, natural frequency sqrt(600) = 24.5 rad/s,
    // so the envelope exp(-zeta*wn*t) reaches restDelta 0.05 at t ~= 0.171s.
    // At 60fps that is ~11 frames; 24 is the budget with the velocity gate and
    // the overshoot's return trip paid for. If a future constant change blows
    // this, the pill is no longer snappy and this test is the alarm.
    const s = createSpring(SPRING_SNAP);
    s.setTarget(1);
    const { ticks } = run(s);
    expect(ticks).toBeGreaterThan(0);
    expect(ticks).toBeLessThanOrEqual(24);
    expect(s.value).toBe(1);
    expect(s.atRest).toBe(true);

    // The SLOW spring is genuinely slower — otherwise there is no reason for
    // two of them — but still lands inside a third of a second.
    const slow = createSpring(SPRING_SETTLE);
    slow.setTarget(1);
    const slowRun = run(slow);
    expect(slowRun.ticks).toBeGreaterThan(ticks);
    expect(slowRun.ticks).toBeLessThanOrEqual(30);
  });

  it("spring_never_overshoots_past_the_soft_limit", () => {
    for (const cfg of [SPRING_SNAP, SPRING_SETTLE]) {
      const s = createSpring(softened(cfg));
      s.setTarget(1);
      const { peak, trough } = run(s);
      // It MUST overshoot — a spring that never passes its target has no bounce
      // and Wilson's ask was "bounces when touched".
      expect(peak).toBeGreaterThan(1);
      // ...and it must not overshoot far enough to read as rubbery.
      expect(peak).toBeLessThanOrEqual(1 + SOFT_LIMIT);
      expect(trough).toBeGreaterThanOrEqual(-SOFT_LIMIT);
    }
  });

  it("wispr_own_rest_delta_swallows_the_bounce_which_is_why_we_tighten_it", () => {
    // THE MEASUREMENT behind SOFT_REST_DELTA, kept as a test so the departure
    // from Wispr's config is justified by evidence and not by taste. At Wispr's
    // published restDelta the snap spring stops BEFORE it ever passes 1.0.
    const rigid = createSpring(SPRING_SNAP);
    rigid.setTarget(1);
    const { peak } = run(rigid);
    expect(SPRING_SNAP.restDelta).toBe(0.05);
    expect(peak).toBeLessThan(1);          // no bounce at all
    expect(peak).toBeGreaterThan(0.95);    // ...it just stops inside restDelta
    expect(SOFT_REST_DELTA).toBeLessThan(SPRING_SNAP.restDelta);
  });

  it("a_long_frame_cannot_launch_the_spring", () => {
    // A backgrounded float window hands the loop a multi-second dt on its first
    // frame back. An unclamped integrator turns that into a scale of 40.
    const s = createSpring(SPRING_SNAP);
    s.setTarget(1);
    s.tick(3);
    expect(Math.abs(s.value)).toBeLessThanOrEqual(1 + SOFT_LIMIT);
    expect(Number.isFinite(s.value)).toBe(true);
  });

  it("reduce_motion_returns_the_target_immediately", () => {
    const s = createSpring(SPRING_SNAP, 0, { reduceMotion: true });
    s.setTarget(1);
    // No tick has run yet and the value is ALREADY there: under Reduce Motion
    // there is no in-between frame to paint at all.
    expect(s.value).toBe(1);
    expect(s.velocity).toBe(0);
    expect(s.atRest).toBe(true);
    // ...and the loop is told, on its very first ask, that it may park.
    expect(s.tick(FRAME)).toBe(false);

    const body = createSoftBody({ reduceMotion: true });
    body.press();
    expect(body.squash).toBe(1);
    expect(body.tick(FRAME)).toBe(false);
    body.release();
    expect(body.squash).toBe(0);
    body.dock();
    expect(body.squash).toBe(0);
    expect(body.atRest).toBe(true);
    // The static frame is the capsule's OWN shape — never a frozen mid-squash.
    expect(body.scale()).toEqual({ x: 1, y: 1 });
  });

  it("integrator_reports_at_rest_and_stops", () => {
    const s = createSpring(SPRING_SNAP);
    // A spring nobody aimed never asks for a frame.
    expect(s.atRest).toBe(true);
    expect(s.tick(FRAME)).toBe(false);

    s.setTarget(1);
    expect(s.atRest).toBe(false);
    let frames = 0;
    // The loop contract: tick() returning false is the signal to STOP
    // scheduling. Once it says false it must keep saying false forever, or the
    // pill silently re-arms a 60fps rAF while idle (the YV81 regression).
    while (s.tick(FRAME)) {
      frames += 1;
      expect(frames).toBeLessThan(200);
    }
    expect(s.atRest).toBe(true);
    for (let i = 0; i < 50; i += 1) expect(s.tick(FRAME)).toBe(false);

    // Same contract for the whole soft body, across the real gesture sequence.
    const body = createSoftBody();
    for (const gesture of [() => body.press(), () => body.release(), () => body.dock()]) {
      gesture();
      let n = 0;
      while (body.tick(FRAME)) {
        n += 1;
        expect(n).toBeLessThan(200);
      }
      expect(body.atRest).toBe(true);
      expect(body.tick(FRAME)).toBe(false);
    }
  });

  it("press_squashes_wider_and_release_rebounds_through_a_stretch", () => {
    const body = createSoftBody();
    expect(body.scale()).toEqual({ x: 1, y: 1 });
    body.press();
    while (body.tick(FRAME)) { /* settle into the press */ }
    const pressed = body.scale();
    // Squashed = WIDER and SHORTER. Volume is not conserved exactly (this is a
    // pill, not a water balloon) but the signs are the whole illusion.
    expect(pressed.x).toBeGreaterThan(1);
    expect(pressed.y).toBeLessThan(1);
    expect(pressed.x).toBeCloseTo(1 + SQUASH_SCALE, 5);

    body.release();
    let stretched = false;
    let n = 0;
    while (body.tick(FRAME) && n < 200) {
      n += 1;
      if (body.squash < 0) stretched = true; // passed neutral: the rebound
    }
    expect(stretched).toBe(true);
    expect(body.squash).toBe(0);
  });

  it("dock_lands_squashed_and_bounces_back_out", () => {
    const body = createSoftBody();
    body.dock();
    expect(body.squash).toBeGreaterThan(0); // landed compressed
    const landed = body.squash;
    let n = 0;
    let bounced = false;
    while (body.tick(FRAME) && n < 300) {
      n += 1;
      if (body.squash < 0) bounced = true;
    }
    expect(landed).toBeGreaterThan(0);
    expect(bounced).toBe(true);
    expect(body.atRest).toBe(true);
  });
});

describe("the duration table", () => {
  it("carries Wispr's measured state-change and expand timings", () => {
    expect(DURATION.phaseMorph).toBe(100);
    expect(DURATION.expand).toBeGreaterThanOrEqual(300);
    expect(DURATION.expand).toBeLessThanOrEqual(400);
    expect(DURATION.collapse).toBeGreaterThanOrEqual(300);
    expect(DURATION.collapse).toBeLessThanOrEqual(400);
    expect(EASE_STATE_CHANGE).toBe("cubic-bezier(0.05, 0.6, 0.4, 0.95)");
    expect(morphTransition("height")).toBe(
      "height 100ms cubic-bezier(0.05, 0.6, 0.4, 0.95)",
    );
  });

  it("reduce_motion_keeps_every_informational_element", () => {
    // The item's hard requirement: under Reduce Motion the pill paints ONE calm
    // static frame, and that frame must still carry the phase, the numeral and
    // the progress. Motion is what gets dropped — never information.
    //
    // Asserted against the SOURCE, because that is where the failure would be
    // introduced: a future `@media (prefers-reduced-motion)` rule that reaches
    // for `display: none` on the transcribe count, or an early return in
    // ClassicPill that skips painting it.
    const css = FLOAT_CSS;
    expect(css.length).toBeGreaterThan(1000); // the sweep really read the file

    // Every reduced-motion block in float.css, brace-matched.
    const blocks: string[] = [];
    const marker = /@media \(prefers-reduced-motion: reduce\)\s*\{/g;
    for (;;) {
      const hit = marker.exec(css);
      if (hit === null) break;
      let depth = 1;
      let i = marker.lastIndex;
      while (i < css.length && depth > 0) {
        if (css[i] === "{") depth += 1;
        else if (css[i] === "}") depth -= 1;
        i += 1;
      }
      blocks.push(css.slice(marker.lastIndex, i - 1));
    }
    expect(blocks.length).toBeGreaterThan(0);

    for (const block of blocks) {
      // Nothing may be REMOVED from the frame to satisfy a motion preference.
      expect(block).not.toMatch(/display\s*:\s*none/);
      expect(block).not.toMatch(/visibility\s*:\s*hidden/);
      expect(block).not.toMatch(/content-visibility/);
      expect(block).not.toMatch(/opacity\s*:\s*0(?![.\d])/);
      // Nor may the informational elements be touched at all.
      for (const sel of [".xscribe-count", ".xscribe-fill", ".gate-note", ".trial"]) {
        expect(block).not.toContain(sel);
      }
    }
    // ...and the block that DOES exist pins the capsule's own shape.
    expect(blocks.some((b) => /\.pill\s*\{[^}]*scale:\s*1 1/.test(b))).toBe(true);

    // ClassicPill's reduced-motion branch paints the static frame and returns
    // BEFORE the rAF loop is ever armed.
    const pill = PILL_SOURCES["./ClassicPill.tsx"];
    expect(pill).toBeTypeOf("string");
    const guard = pill.slice(pill.indexOf("(prefers-reduced-motion: reduce)"));
    const stop = guard.indexOf("requestAnimationFrame");
    expect(stop).toBeGreaterThan(0);
    const branch = guard.slice(0, stop);
    expect(branch).toContain('setProperty("--squash-x", "1")');
    expect(branch).toContain('setProperty("--squash-y", "1")');
    expect(branch).toContain("return;");
  });

  it("no_duration_literal_outside_motion_ts", () => {
    // A SOURCE SWEEP, not a spot check: every non-test source file under
    // src/pill is read, its comments stripped (the prose in this module talks
    // about "100ms" constantly and must not trip its own guard), and the
    // remaining CODE searched for the two shapes a hardcoded duration takes.
    const files = Object.keys(PILL_SOURCES)
      .filter((f) => !/\.test\.tsx?$/.test(f))
      .filter((f) => f !== "./motion.ts");
    expect(files.length).toBeGreaterThan(4); // the sweep found real files to sweep
    expect(files).toContain("./ClassicPill.tsx");
    expect(files).toContain("./YappyPill.tsx");

    const stripComments = (src: string) =>
      src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/[^\n]*/g, "$1");

    // Shape 1: a CSS time literal anywhere in the code ("120ms", "0.3s").
    const cssTime = /\b\d+(?:\.\d+)?m?s\b/;
    // Shape 2: an easing curve, which is a duration's other half.
    const bezier = /cubic-bezier\s*\(/;
    // Shape 3: a bare numeric literal handed to a timer — the way ClassicPill's
    // 1100ms done-flash and YappyPill's 1600ms done-hold used to be written.
    const rawTimer = /\bset(?:Timeout|Interval)\s*\([\s\S]{0,400}?,\s*\d+\s*\)/;

    const offenders: string[] = [];
    for (const f of files) {
      const code = stripComments(PILL_SOURCES[f]);
      for (const [name, re] of [["css-time", cssTime], ["bezier", bezier], ["raw-timer", rawTimer]] as const) {
        const m = re.exec(code);
        if (m) offenders.push(`${f}: ${name}: ${m[0].slice(0, 80).replace(/\s+/g, " ")}`);
      }
    }
    expect(offenders).toEqual([]);

    // PROVE THE SWEEP CAN BITE. A guard that cannot match the shape it claims
    // to forbid is a green test over an unguarded tree, which is exactly the
    // failure mode this repo has shipped before.
    expect(cssTime.test("transition: height 340ms linear")).toBe(true);
    expect(cssTime.test("animation: x 0.3s ease")).toBe(true);
    expect(bezier.test("cubic-bezier(0.4, 0, 0.2, 1)")).toBe(true);
    expect(rawTimer.test("window.setTimeout(() => setDone(false), 1100);")).toBe(true);
    // ...and that it tolerates the legal form: a timer fed from the table.
    expect(rawTimer.test("window.setTimeout(fn, DURATION.doneFlash)")).toBe(false);
  });
});
