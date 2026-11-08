/**
 * yap-1106-fb-10 — no character is ever cropped in the pill.
 *
 * Acceptance: for every character that can sit in the pill x all 17 pill
 * states x every dock (bottom, top, left, right, side-dock tall scene), the
 * character's bounding box — ears, hats, horns and wings included — sits fully
 * inside the visible window. This walks that matrix with the sprite manifests
 * (sprites/art.ts FRIENDS w x h, and the pill chick's box), after first proving
 * the manifests are honest: every painted pixel of every frame lies inside them.
 */
import { describe, expect, it } from "vitest";
import { FRIENDS, H, SPRITE_FRAMES, drawChick, type ChickPose, type SpriteOpt } from "../sprites/art";
import { Raster } from "../sprites/raster";
import {
  CHICK_EXTENT, CHICK_LIMITS, FIT_DOCKS, OPEN_CAPSULE, OPEN_TIERS, openCapsuleFor, PILL_CHARACTERS, PILL_SCENE, REST_CAPSULE, REST_MIN_W,
  capsuleForDock, chickBox, characterArt, clampChickPose, cropWarning, inStadium, largestFittingCell, pillCamera,
  placeCharacter, placeSprite, restCapsuleFor, stadiumContains, type FitBox, type FitDock,
  ARTIFACT_SPRITE_SCALE, ARTIFACT_TOLERANCE, MAX_LIFT, chickBoxFor, cssPerArtPx, liftBudget, pillFit, scaleEvent,
  ARTIFACT_OPEN_SHELL, ARTIFACT_REST_H, CHICK_DESK_SHIFT, REST_MAX_H,
  ARTIFACT_REST_SHELL, restFramingEvent,
} from "./characterFit";
import { ALL_LIVE_PHASES, type LivePhase } from "./live";
import { CHARACTER_OPEN, CHARACTER_REST, PANEL_MARGIN, RESTING_PHASES, capsuleCentre, pillLayout, type MeetingMode } from "./state";

/** The painted pixels' bounding box (alpha > 0), or null when nothing was drawn. */
function paintedBox(r: Raster): FitBox | null {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (let y = 0; y < r.height; y++) for (let x = 0; x < r.width; x++) {
    if (r.px(x, y).a > 0) { x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x + 1); y1 = Math.max(y1, y + 1); }
  }
  return x1 < 0 ? null : { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

const inside = (b: FitBox, o: FitBox) => b.x >= o.x && b.y >= o.y && b.x + b.w <= o.x + o.w && b.y + b.h <= o.y + o.h;
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
const DPRS = [1, 1.5, 2] as const;
const MEETINGS: readonly MeetingMode[] = ["none", "recording"];

/** Every pose at the corners of CHICK_LIMITS (plus a fractional hop), with every prop the pill draws. */
function* chickPoses(): Generator<ChickPose> {
  for (const hop of [CHICK_LIMITS.hop[0], 0, 5.5, CHICK_LIMITS.hop[1]])
    for (const sx of CHICK_LIMITS.sx) for (const sy of CHICK_LIMITS.sy) for (const flap of CHICK_LIMITS.flap)
      for (const sway of CHICK_LIMITS.sway) for (const droop of [false, true]) for (const pencil of [false, true])
        yield { hop, sx, sy, flap, sway, droop, pencil, glasses: pencil, blink: 0, mood: pencil ? 1 : 0, beakF: pencil ? 2 : 0, look: "fwd" };
}

/** The capsule a state draws (CSS px): resting phases the rest capsule, every other phase the open one at `tier`. */
function capsuleSize(phase: LivePhase, tier: number, t = 1): { w: number; h: number } {
  if (RESTING_PHASES.has(phase)) return { w: REST_CAPSULE.w, h: REST_CAPSULE.h };
  const open = openCapsuleFor(tier);
  return { w: lerp(REST_CAPSULE.w, open.w, t), h: lerp(REST_CAPSULE.h, open.h, t) };
}

/**
 * The capsule's rect in its panel at `dock`, as the pill places it: the one
 * layout's character box + capsuleCentre (YappyPill). `top` is the bottom
 * layout mirrored; `side-tall` is the capsule on its end at the panel margin.
 */
function placeCapsule(phase: LivePhase, meeting: MeetingMode, dock: FitDock, size: { w: number; h: number }) {
  if (dock === "side-tall") {
    const s = capsuleForDock(size, dock);
    const cap = { x: PANEL_MARGIN, y: PANEL_MARGIN, w: s.w, h: s.h };
    return { cap, panel: { w: s.w + 2 * PANEL_MARGIN, h: s.h + 2 * PANEL_MARGIN } };
  }
  // yap-1106-pill-vertical-dock-world — on a left / right dock Yappy's layout
  // box is the tall shell (vdock.ts, proven by vdock.test.ts); the horizontal
  // capsule is checked here inside the horizontal layout's panel.
  const layout = pillLayout({ phase, meeting, dock: "bottom" });
  const c = capsuleCentre(layout, size.w);
  const y = c.y - size.h / 2;
  const cap = { x: c.x - size.w / 2, y: dock === "top" ? layout.panel.h - (y + size.h) : y, w: size.w, h: size.h };
  return { cap, panel: layout.panel };
}

describe("yap-1106-fb-10 the sprite manifests are honest", () => {
  it("chick_box_holds_every_pose: the pill chick's painted pixels never leave CHICK_EXTENT", () => {
    const cx = 60, feetY = 70;
    const box = { x: cx - CHICK_EXTENT.left, y: feetY - CHICK_EXTENT.up, w: CHICK_EXTENT.left + CHICK_EXTENT.right, h: CHICK_EXTENT.up + CHICK_EXTENT.down };
    let n = 0;
    for (const pose of chickPoses()) {
      const r = new Raster(120, 100);
      drawChick(r, cx, feetY, clampChickPose(pose));
      const got = paintedBox(r);
      expect(got, JSON.stringify(pose)).not.toBeNull();
      expect(inside(got!, box), `${JSON.stringify(got)} outside ${JSON.stringify(box)} for ${JSON.stringify(pose)}`).toBe(true);
      n++;
    }
    expect(n).toBeGreaterThan(100);
  }, 30_000);

  it("chick_pose_is_clamped_into_the_box_range", () => {
    const wild = clampChickPose({ hop: 40, sx: -9, sy: 9, flap: 7, sway: Number.NaN, extra: 1 });
    expect(wild).toEqual({ hop: CHICK_LIMITS.hop[1], sx: CHICK_LIMITS.sx[0], sy: CHICK_LIMITS.sy[1], flap: CHICK_LIMITS.flap[1], sway: 0, extra: 1 });
  });

  it("manifest_holds_every_painted_pixel: ears, hats, horns and wings sit inside each pill character's w x h", () => {
    const pad = 20;
    for (const name of PILL_CHARACTERS) {
      const s = FRIENDS[name];
      const opts: Array<SpriteOpt | undefined> = name === "hoot" ? [undefined, { bare: true }] : name === "cinder" ? [undefined, { talk: true, t: 0 }, { talk: true, t: 1 / 6 }] : [undefined];
      for (const f of SPRITE_FRAMES[name]) for (const opt of opts) {
        const r = new Raster(s.w + 2 * pad, s.h + 2 * pad);
        r.translate(pad, pad);
        s.draw(r, H(r), f, opt);
        const got = paintedBox(r);
        expect(got, `${name} f=${f}`).not.toBeNull();
        expect(inside(got!, { x: pad, y: pad, w: s.w, h: s.h }), `${name} f=${f} ${JSON.stringify(opt)} painted ${JSON.stringify(got)} past its ${s.w}x${s.h} manifest`).toBe(true);
      }
      const art = characterArt(name);
      expect(art.w).toBeGreaterThanOrEqual(s.w);
      expect(art.h).toBeGreaterThanOrEqual(s.h);
    }
  });

  it("room_only_friends_never_sit_in_the_pill (companions §4)", () => {
    expect(PILL_CHARACTERS).toEqual(["yappy", "hoot", "tidy", "gulp", "shelly", "cinder"]);
    for (const roomOnly of ["ember", "sal", "mochi"] as const) expect(PILL_CHARACTERS).not.toContain(roomOnly);
  });
});

describe("yap-1106-fb-10 no character is ever cropped in the pill", () => {
  it("walks_all_17_states_and_every_dock: every pill character x 17 states x 5 docks x tiers x displays sits whole inside the visible stadium", () => {
    expect(ALL_LIVE_PHASES).toHaveLength(17);
    expect([...FIT_DOCKS]).toEqual(["bottom", "top", "left", "right", "side-tall"]);
    const failures: string[] = [];
    let cases = 0;
    for (const name of PILL_CHARACTERS) for (const phase of ALL_LIVE_PHASES) for (const dock of FIT_DOCKS) for (const meeting of MEETINGS) {
      // yap-1107-one-pill-during-meeting — a resting recording meeting draws the
      // meeting scene AS the pill: no sky capsule is painted, so there is none to fit.
      if (meeting === "recording" && RESTING_PHASES.has(phase)) continue;
      const tiers = RESTING_PHASES.has(phase) ? [1] : [...OPEN_TIERS];
      for (const tier of tiers) for (const dpr of DPRS) {
        const size = capsuleSize(phase, tier);
        const { cap, panel } = placeCapsule(phase, meeting, dock, size);
        // the capsule itself is inside the panel (the window)
        if (cap.x < -0.5 || cap.y < -0.5 || cap.x + cap.w > panel.w + 0.5 || cap.y + cap.h > panel.h + 0.5) failures.push(`${name} ${phase} ${dock} capsule outside the panel`);
        const p = placeCharacter(name, dock, cap, dpr);
        cases++;
        // yap-1107-parity-pill — the resting capsule is the approved 72 x 28 nub.
        // On 2x everyone is whole in it (checked below like every other state);
        // below 2x no whole cell fits its 26 px clip, so Yappy's camera keeps his
        // head in view ("head" framing) and the whole-box rule is the 2x one.
        if (RESTING_PHASES.has(phase) && dpr < 2) {
          if (name === "yappy" && dock !== "side-tall") {
            const c = pillCamera(cap, dpr);
            if (c.framing !== "head" || c.char.y < c.stadium.y) failures.push(`${name} ${phase} ${dock} dpr=${dpr} head not in view framing=${c.framing}`);
          }
          continue;
        }
        if (!p.fits || !stadiumContains(p.stadium, p.char) || p.cell < 1) {
          failures.push(`${name} ${phase} ${dock} ${meeting} tier=${tier} dpr=${dpr} route=${p.route} char=${JSON.stringify(p.char)} stadium=${JSON.stringify(p.stadium)}`);
        }
      }
    }
    expect(failures, failures.slice(0, 8).join("\n")).toEqual([]);
    expect(cases).toBeGreaterThan(6 * 17 * 5);
  }, 30_000);

  it("yappy_whole_while_the_capsule_opens_and_closes: every frame of the rest <-> open lerp keeps the chick inside", () => {
    for (const tier of OPEN_TIERS) for (const dpr of DPRS) for (let i = 0; i <= 20; i++) {
      const size = capsuleSize("listening", tier, i / 20);
      for (const dock of ["bottom", "left", "right"] as const) {
        const { cap } = placeCapsule("listening", "none", dock, size);
        const c = pillCamera(cap, dpr);
        // yap-1107-parity-pill — from the 72 x 28 nub below 2x the first frames
        // keep his head in view until a whole cell fits; on 2x every frame is whole.
        const ok = c.fits || (dpr < 2 && c.framing === "head" && c.char.y >= c.stadium.y);
        expect(ok, `tier=${tier} dpr=${dpr} t=${i / 20} ${dock} ${c.framing} ${JSON.stringify(c.char)} in ${JSON.stringify(c.stadium)}`).toBe(true);
        if (dpr === 2) expect(c.framing).toBe("whole");
        // the world still covers the clip (never letterboxed) and is blitted at a whole cell
        expect(Number.isInteger(c.blit.cell)).toBe(true);
        expect(c.blit.dst.w).toBeGreaterThanOrEqual(c.blit.clip.w);
        expect(c.blit.dst.h).toBeGreaterThanOrEqual(c.blit.clip.h);
      }
    }
  });

  it("yappy_camera_reads_only_the_scene_buffer and centres him, not his face", () => {
    // yap-1107-parity-pill: on the 72 x 28 nub he is centred whole on 2x (below
    // 2x the head framing applies: idle_and_sleepy_on_the_72_x_28_nub).
    const { cap } = placeCapsule("idle", "none", "bottom", capsuleSize("idle", 1));
    const c = pillCamera(cap, 2);
    expect(c.framing).toBe("whole");
    expect(c.blit.src.x).toBeGreaterThanOrEqual(0);
    expect(c.blit.src.y).toBeGreaterThanOrEqual(0);
    expect(c.blit.src.x + c.blit.src.w).toBeLessThanOrEqual(PILL_SCENE.w);
    expect(c.blit.src.y + c.blit.src.h).toBeLessThanOrEqual(PILL_SCENE.h);
    const env = chickBox();
    expect(env.y).toBeGreaterThanOrEqual(0);
    expect(env.y + env.h).toBeLessThanOrEqual(PILL_SCENE.h);
    // his tufts (the top of the box) are on screen at rest: the regression Wilson saw
    expect(c.char.y).toBeGreaterThanOrEqual(c.stadium.y);
  });

  it("rest_capsule_is_the_artifact_nub_and_holds_every_character_at_2x", () => {
    // yap-1107-parity-pill (batch-7 bounce fix): pill.html draws the resting,
    // asleep and hover-tray shell as PK.pill({ h: 28, win: { w: 64, h: 20 } }) = 72 x 28.
    expect({ w: REST_CAPSULE.w, h: REST_CAPSULE.h }).toEqual({ w: 72, h: 28 });
    expect({ w: ARTIFACT_REST_SHELL.w, h: ARTIFACT_REST_SHELL.h }).toEqual({ w: 72, h: 28 });
    expect(CHARACTER_REST.w).toBe(REST_CAPSULE.w);
    expect(CHARACTER_REST.h).toBe(REST_CAPSULE.h);
    expect(REST_CAPSULE.w).toBeGreaterThanOrEqual(REST_MIN_W);
    expect(REST_CAPSULE.w).toBeLessThanOrEqual(CHARACTER_OPEN.w);
    expect(REST_CAPSULE.h).toBeLessThanOrEqual(CHARACTER_OPEN.h);
    for (const name of PILL_CHARACTERS) {
      const p = placeSprite({ x: 0, y: 0, w: REST_CAPSULE.w, h: REST_CAPSULE.h }, characterArt(name), 2);
      expect(p.fits, name).toBe(true);
      expect(p.cell, name).toBe(1);
    }
    // below 2x no whole cell holds a character in the nub's 26 px clip
    expect(placeSprite({ x: 0, y: 0, w: REST_CAPSULE.w, h: REST_CAPSULE.h }, characterArt("yappy"), 1).fits).toBe(false);
    expect(restCapsuleFor(PILL_CHARACTERS.map(characterArt)).h).toBeLessThanOrEqual(REST_MAX_H);
    expect(restCapsuleFor([{ w: 10, h: 10 }])).toEqual(restCapsuleFor([{ w: 10, h: 10 }]));
  });
});

describe("yap-1106-fb-10 geometry", () => {
  it("inStadium: straight edges, rounded caps, both orientations", () => {
    const wide = { x: 0, y: 0, w: 100, h: 40 };
    expect(inStadium(wide, 50, 0)).toBe(true);
    expect(inStadium(wide, 0, 20)).toBe(true);
    expect(inStadium(wide, 1, 1)).toBe(false); // the rounded corner is not visible
    const tall = { x: 0, y: 0, w: 40, h: 100 };
    expect(inStadium(tall, 20, 0)).toBe(true);
    expect(inStadium(tall, 0, 50)).toBe(true);
    expect(inStadium(tall, 1, 99)).toBe(false);
    expect(inStadium({ x: 0, y: 0, w: 0, h: 10 }, 0, 0)).toBe(false);
  });

  it("largestFittingCell picks the biggest whole cell whose centred box stays inside", () => {
    const s = { x: 0, y: 0, w: 200, h: 60 };
    const c = largestFittingCell(s, { w: 20, h: 20 });
    expect(c).toBeGreaterThanOrEqual(1);
    expect(stadiumContains(s, { x: 100 - 10 * c, y: 30 - 10 * c, w: 20 * c, h: 20 * c })).toBe(true);
    expect(largestFittingCell(s, { w: 300, h: 10 })).toBe(0);
    expect(largestFittingCell(s, { w: 0, h: 10 })).toBe(0);
    expect(stadiumContains(s, { x: 10, y: 10, w: 0, h: 5 })).toBe(false);
  });

  it("cropWarning is one structured line with no content", () => {
    expect(cropWarning("yappy", { w: 72.4, h: 22.6 }, 2)).toBe("pill character-cropped character=yappy capW=72 capH=23 dpr=2");
  });
});

/**
 * yap-1107-pill-matches-artifact-size — Wilson 2026-11-07: the pill "looks small
 * and shrunken" next to the approved Pill artifact. The artifact draws Yappy at
 * 1.5625 CSS px per art px (listening: 0.8 x 176 art px across a 220 px window);
 * the app drew him at 1.0 on 2x. On 2x the open capsule now draws him at 1.5
 * (cell 3, 96 percent), still whole: only the height of a hop is given up.
 */
describe("yap-1107-pill-matches-artifact-size the pill draws Yappy at the artifact's size", () => {
  const OPEN_PHASES: readonly LivePhase[] = ["listening", "listening-locked", "transcribing", "polishing", "pasting", "done"];

  it("chick_box_for_lift_holds_every_clamped_pose: a smaller lift budget keeps every painted pixel inside chickBoxFor(lift)", () => {
    const cx = 60, feetY = 70, full = chickBox();
    let n = 0;
    let prevH = 0;
    for (let lift = 0; lift <= MAX_LIFT; lift++) {
      const b = chickBoxFor(lift);
      // same left / right as the full box, inside it, and never shorter than a smaller budget's
      expect(b.x).toBe(full.x);
      expect(b.w).toBe(full.w);
      expect(b.y).toBeGreaterThanOrEqual(full.y);
      expect(b.y + b.h).toBeLessThanOrEqual(full.y + full.h);
      expect(b.h).toBeGreaterThanOrEqual(prevH);
      prevH = b.h;
      // the scene box, moved onto this raster's (cx, feetY): groundY is the feet row
      const up = PILL_SCENE.groundY - b.y, down = b.y + b.h - PILL_SCENE.groundY;
      expect(full.x + CHICK_DESK_SHIFT + CHICK_EXTENT.left).toBe(PILL_SCENE.faceX);
      const box = { x: cx - CHICK_EXTENT.left, y: feetY - up, w: CHICK_EXTENT.left + CHICK_EXTENT.right, h: up + down };
      for (const pose of chickPoses()) {
        if (pose.sway !== 0 && pose.sway !== CHICK_LIMITS.sway[1]) continue; // sway never moves the top; keeps the run short
        const r = new Raster(120, 100);
        drawChick(r, cx, feetY, clampChickPose(pose, lift));
        const got = paintedBox(r);
        expect(got).not.toBeNull();
        expect(inside(got!, box), `lift=${lift} ${JSON.stringify(got)} outside ${JSON.stringify(box)} for ${JSON.stringify(pose)}`).toBe(true);
        n++;
      }
    }
    expect(n).toBeGreaterThan(100);
    expect(chickBoxFor(MAX_LIFT)).toEqual(full);
  }, 60_000);

  it("lift_budget_caps_hop_then_stretch_and_leaves_the_rest", () => {
    expect(MAX_LIFT).toBe(CHICK_LIMITS.hop[1] + CHICK_LIMITS.sy[1]);
    expect(liftBudget(Number.NaN)).toBe(MAX_LIFT);
    expect(liftBudget(-3)).toBe(0);
    expect(liftBudget(99)).toBe(MAX_LIFT);
    const p = clampChickPose({ hop: 6, sx: 3, sy: 2, flap: 2, sway: 2 }, 1);
    expect(p).toEqual({ hop: 1, sx: 3, sy: 0, flap: 2, sway: 2 });
    // a crouch and a squash are never cut
    expect(clampChickPose({ hop: -2, sx: -1, sy: -4, flap: 0, sway: -2 }, 0)).toEqual({ hop: -2, sx: -1, sy: -4, flap: 0, sway: -2 });
    // the default is the full range (yap-1106-fb-10 unchanged)
    expect(clampChickPose({ hop: 6, sx: 0, sy: 2, flap: 0, sway: 0 })).toEqual({ hop: 6, sx: 0, sy: 2, flap: 0, sway: 0 });
  });

  it("open_pill_on_2x_draws_yappy_within_5_percent_of_the_artifact_and_whole", () => {
    for (const phase of OPEN_PHASES) for (const dock of ["bottom", "top", "left", "right"] as const) for (const meeting of MEETINGS) {
      const { cap } = placeCapsule(phase, meeting, dock, capsuleSize(phase, 1));
      const c = pillCamera(cap, 2);
      const scale = cssPerArtPx(c.blit, 2);
      expect(Math.abs(scale - ARTIFACT_SPRITE_SCALE) / ARTIFACT_SPRITE_SCALE, `${phase} ${dock} ${meeting} scale=${scale}`).toBeLessThanOrEqual(ARTIFACT_TOLERANCE);
      expect(c.fits, `${phase} ${dock} ${meeting} ${JSON.stringify(c.char)} in ${JSON.stringify(c.stadium)}`).toBe(true);
      expect(c.lift).toBeGreaterThanOrEqual(0);
    }
  });

  it("retina_never_halves_the_sprite: 2x draws at least the 1x size, and never past the artifact's", () => {
    // yap-1107-parity-pill: the resting 72 x 28 nub is the one exception (no
    // whole cell above one device px fits its 26 px clip on 2x; see
    // idle_and_sleepy_on_the_72_x_28_nub); every open state keeps the rule.
    for (const phase of OPEN_PHASES) for (const tier of OPEN_TIERS) {
      const size = capsuleSize(phase, tier);
      const one = pillCamera({ x: 0, y: 0, ...size }, 1), two = pillCamera({ x: 0, y: 0, ...size }, 2);
      expect(cssPerArtPx(two.blit, 2), `${phase} tier=${tier}`).toBeGreaterThanOrEqual(cssPerArtPx(one.blit, 1));
      expect(cssPerArtPx(two.blit, 2)).toBeLessThanOrEqual(ARTIFACT_SPRITE_SCALE * (1 + ARTIFACT_TOLERANCE));
      expect(two.fits && one.fits).toBe(true);
    }
  });

  it("1x_is_unchanged: cell 1 and the full hop (the artifact's 1.56 rounds down to one device px)", () => {
    const { stadium } = { stadium: pillCamera({ x: 0, y: 0, w: OPEN_CAPSULE.w, h: OPEN_CAPSULE.h }, 1).stadium };
    expect(pillFit(stadium, 1)).toEqual({ cell: 1, lift: MAX_LIFT });
  });

  it("shell_matches_the_artifact_within_3_percent: open 228 x 53 vs the artifact's 228 x 52, resting 72 x 28 exactly", () => {
    expect(OPEN_CAPSULE.w).toBe(ARTIFACT_OPEN_SHELL.w);
    // yap-1107-parity-pill: the parity rule is 3 percent (batch 5 bounced 54 and 60)
    expect(Math.abs(OPEN_CAPSULE.h - ARTIFACT_OPEN_SHELL.h) / ARTIFACT_OPEN_SHELL.h).toBeLessThanOrEqual(0.03);
    expect(OPEN_CAPSULE.h).not.toBe(54);
    // yap-1107-parity-pill (batch-7 bounce fix): the resting nub is the page's 72 x 28 exactly (0 percent)
    expect(REST_CAPSULE.w).toBe(ARTIFACT_REST_SHELL.w);
    expect(REST_CAPSULE.h).toBe(ARTIFACT_REST_SHELL.h);
    expect(ARTIFACT_REST_H).toBe(28);
    expect(REST_CAPSULE.h).not.toBe(56);
    expect(REST_CAPSULE.h).toBeLessThanOrEqual(REST_MAX_H);
    // the batch-10 bounce: 80 x 59 resting and 236 x 60 open were both outside the 5 percent
    expect(REST_CAPSULE.h).not.toBe(59);
    expect(OPEN_CAPSULE.h).not.toBe(60);
  });

  it("artifact_height_shell_holds_him_whole_at_its_scale_on_2x: cell 3 and a hop of at least one art px in the plain open pill", () => {
    const { cap } = placeCapsule("listening", "none", "bottom", capsuleSize("listening", 1));
    const c = pillCamera(cap, 2);
    expect(c.blit.cell).toBe(3);
    expect(c.lift).toBeGreaterThanOrEqual(1);
    expect(c.fits).toBe(true);
    const done = pillCamera(placeCapsule("done", "none", "bottom", capsuleSize("done", 1)).cap, 2);
    expect(done.blit.cell).toBe(3);
    expect(done.fits).toBe(true);
  });

  it("idle_and_sleepy_on_the_72_x_28_nub: whole at one device px per art px on 2x, head kept in view below 2x", () => {
    for (const phase of ["idle", "sleepy"] as const) for (const dpr of DPRS) {
      const c = pillCamera({ x: 0, y: 0, ...capsuleSize(phase, 1) }, dpr);
      expect(Number.isInteger(c.blit.cell), `${phase} dpr=${dpr}`).toBe(true);
      expect(c.blit.cell, `${phase} dpr=${dpr}`).toBeGreaterThanOrEqual(1);
      expect(c.blit.dst.w).toBeGreaterThanOrEqual(c.blit.clip.w);
      expect(c.blit.dst.h).toBeGreaterThanOrEqual(c.blit.clip.h);
      if (dpr === 2) {
        expect(c.framing, `${phase} dpr=2`).toBe("whole");
        expect(c.fits, `${phase} dpr=2`).toBe(true);
        expect(c.blit.cell).toBe(1);
      } else {
        // the top of his box (sprout, eyes, beak) is inside the stadium; only his feet leave the nub
        expect(c.framing, `${phase} dpr=${dpr}`).toBe("head");
        expect(c.char.y, `${phase} dpr=${dpr}`).toBeGreaterThanOrEqual(c.stadium.y);
        expect(c.char.x).toBeGreaterThanOrEqual(c.stadium.x);
        expect(c.char.x + c.char.w).toBeLessThanOrEqual(c.stadium.x + c.stadium.w);
      }
    }
  });

  it("restFramingEvent is one structured line with numbers and enums only", () => {
    expect(restFramingEvent("head", { w: 72, h: 28 }, 1)).toBe("pill camera-framing framing=head capW=72 capH=28 dpr=1");
    expect(restFramingEvent("whole", { w: 228.4, h: 52.6 }, 2)).toBe("pill camera-framing framing=whole capW=228 capH=53 dpr=2");
  });

  it("rest_capsule_cap_falls_back_when_nothing_that_short_fits", () => {
    // a 70 art px tall character cannot fit a 56 px capsule: the uncapped search answers
    const big = restCapsuleFor([{ w: 30, h: 70 }]);
    expect(big.h).toBeGreaterThan(REST_MAX_H);
    expect(restCapsuleFor([{ w: 30, h: 30 }]).h).toBeLessThanOrEqual(REST_MAX_H);
  });

  it("scaleEvent is one structured line with numbers only", () => {
    expect(scaleEvent({ cell: 3, lift: 1 }, { w: 236.2, h: 59.8 }, 2)).toBe("pill sprite-scale cell=3 lift=1 cssPerArt=1.500 capW=236 capH=60 dpr=2");
  });
});

describe("yap-1107-parity-pill the open shell is the approved page's height at every prop tier", () => {
  it("open_tiers_keep_the_artifact_height_within_3_percent: the notepad and desk widen the shell, never heighten it", () => {
    for (const tier of OPEN_TIERS) {
      const c = openCapsuleFor(tier);
      expect(Math.abs(c.h - ARTIFACT_OPEN_SHELL.h) / ARTIFACT_OPEN_SHELL.h, `tier=${tier}`).toBeLessThanOrEqual(0.03);
      expect(c.h, `tier=${tier}`).toBe(OPEN_CAPSULE.h);
      expect(c.w, `tier=${tier}`).toBeCloseTo(OPEN_CAPSULE.w * tier, 6);
      // the panel envelope still holds the widest tier (the window does not move)
      expect(c.w).toBeLessThanOrEqual(CHARACTER_OPEN.w);
      expect(c.h).toBeLessThanOrEqual(CHARACTER_OPEN.h);
    }
    // a bad tier never yields a zero or NaN capsule
    expect(openCapsuleFor(Number.NaN)).toEqual({ w: OPEN_CAPSULE.w, h: OPEN_CAPSULE.h });
    expect(openCapsuleFor(0)).toEqual({ w: OPEN_CAPSULE.w, h: OPEN_CAPSULE.h });
  });

  it("artifact_height_holds_him_whole_at_every_tier_on_2x_and_1x: cell 3 with a one-art-px hop on 2x, the full hop on 1x", () => {
    for (const tier of OPEN_TIERS) for (const phase of ["listening", "transcribing", "done"] as const) {
      const { cap } = placeCapsule(phase, "none", "bottom", capsuleSize(phase, tier));
      expect(cap.h, `${phase} tier=${tier}`).toBe(OPEN_CAPSULE.h);
      const two = pillCamera(cap, 2);
      expect(two.blit.cell, `${phase} tier=${tier} 2x`).toBe(3);
      expect(two.lift, `${phase} tier=${tier} 2x`).toBeGreaterThanOrEqual(1);
      expect(two.fits, `${phase} tier=${tier} 2x`).toBe(true);
      const one = pillCamera(cap, 1);
      expect(one.blit.cell, `${phase} tier=${tier} 1x`).toBe(1);
      expect(one.lift, `${phase} tier=${tier} 1x`).toBe(MAX_LIFT);
      expect(one.fits, `${phase} tier=${tier} 1x`).toBe(true);
    }
  });
});
