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
  CHICK_EXTENT, CHICK_LIMITS, FIT_DOCKS, OPEN_CAPSULE, OPEN_TIERS, PILL_CHARACTERS, PILL_SCENE, REST_CAPSULE, REST_MIN_W,
  capsuleForDock, chickBox, characterArt, clampChickPose, cropWarning, inStadium, largestFittingCell, pillCamera,
  placeCharacter, placeSprite, restCapsuleFor, stadiumContains, type FitBox, type FitDock,
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
  return { w: lerp(REST_CAPSULE.w, OPEN_CAPSULE.w * tier, t), h: lerp(REST_CAPSULE.h, OPEN_CAPSULE.h * tier, t) };
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
  const layout = pillLayout({ phase, meeting, dock: dock === "top" ? "bottom" : dock });
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
  });

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
      const tiers = RESTING_PHASES.has(phase) ? [1] : [...OPEN_TIERS];
      for (const tier of tiers) for (const dpr of DPRS) {
        const size = capsuleSize(phase, tier);
        const { cap, panel } = placeCapsule(phase, meeting, dock, size);
        // the capsule itself is inside the panel (the window)
        if (cap.x < -0.5 || cap.y < -0.5 || cap.x + cap.w > panel.w + 0.5 || cap.y + cap.h > panel.h + 0.5) failures.push(`${name} ${phase} ${dock} capsule outside the panel`);
        const p = placeCharacter(name, dock, cap, dpr);
        cases++;
        if (!p.fits || !stadiumContains(p.stadium, p.char) || p.cell < 1) {
          failures.push(`${name} ${phase} ${dock} ${meeting} tier=${tier} dpr=${dpr} route=${p.route} char=${JSON.stringify(p.char)} stadium=${JSON.stringify(p.stadium)}`);
        }
      }
    }
    expect(failures, failures.slice(0, 8).join("\n")).toEqual([]);
    expect(cases).toBeGreaterThan(6 * 17 * 5);
  });

  it("yappy_whole_while_the_capsule_opens_and_closes: every frame of the rest <-> open lerp keeps the chick inside", () => {
    for (const tier of OPEN_TIERS) for (const dpr of DPRS) for (let i = 0; i <= 20; i++) {
      const size = capsuleSize("listening", tier, i / 20);
      for (const dock of ["bottom", "left", "right"] as const) {
        const { cap } = placeCapsule("listening", "none", dock, size);
        const c = pillCamera(cap, dpr);
        expect(c.fits, `tier=${tier} dpr=${dpr} t=${i / 20} ${dock} ${JSON.stringify(c.char)} in ${JSON.stringify(c.stadium)}`).toBe(true);
        // the world still covers the clip (never letterboxed) and is blitted at a whole cell
        expect(Number.isInteger(c.blit.cell)).toBe(true);
        expect(c.blit.dst.w).toBeGreaterThanOrEqual(c.blit.clip.w);
        expect(c.blit.dst.h).toBeGreaterThanOrEqual(c.blit.clip.h);
      }
    }
  });

  it("yappy_camera_reads_only_the_scene_buffer and centres him, not his face", () => {
    const { cap } = placeCapsule("idle", "none", "bottom", capsuleSize("idle", 1));
    const c = pillCamera(cap, 1);
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

  it("rest_capsule_holds_the_largest_character_at_1x", () => {
    expect(CHARACTER_REST.w).toBe(REST_CAPSULE.w);
    expect(CHARACTER_REST.h).toBe(REST_CAPSULE.h);
    expect(REST_CAPSULE.w).toBeGreaterThanOrEqual(REST_MIN_W);
    expect(REST_CAPSULE.w).toBeLessThanOrEqual(CHARACTER_OPEN.w);
    expect(REST_CAPSULE.h).toBeLessThanOrEqual(CHARACTER_OPEN.h);
    for (const name of PILL_CHARACTERS) {
      expect(placeSprite({ x: 0, y: 0, w: REST_CAPSULE.w, h: REST_CAPSULE.h }, characterArt(name), 1).fits, name).toBe(true);
    }
    // the old 72 x 22 pill could not hold a whole character: the crop was structural
    expect(placeSprite({ x: 0, y: 0, w: 72, h: 22 }, characterArt("yappy"), 1).fits).toBe(false);
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
