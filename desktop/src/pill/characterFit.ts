/**
 * yap-1106-fb-10 — no character is ever cropped in the pill.
 *
 * Wilson, 2026-11-06 ~17:00Z: "it looks like the heads cropped … in the pill".
 * Two causes, both fixed here as ONE pure rule the pill draws with:
 *
 *   1. The resting camera zoomed onto Yappy's FACE (a 26 art px view centred on
 *      it), so at idle / sleepy his tufts and the top of his head were cut off
 *      by the capsule by design. And the resting capsule (72 x 22) was too
 *      short to hold any whole character at a whole number of pixels.
 *   2. The open camera was centred on the scene buffer, not on him, and the
 *      buffer (176 x 44) was smaller than the open capsule, so the cell was
 *      forced UP to cover it (2x at 1x displays): his head, or a hop, ran off
 *      the top of the capsule.
 *
 * The rule now: every character that can sit in the pill has a BOUNDING BOX
 * in art px (its sprite manifest's `w x h`, which carries ears, hats, horns and
 * wings — the matrix test measures the painted pixels against it), and the
 * camera / placement picks the largest WHOLE number of device px per art px at
 * which that box, centred, sits inside the capsule's visible stadium (the
 * rounded clip, inset one device px for rounding). The resting capsule is sized
 * from the same boxes, so the smallest pill state still holds the largest
 * character at one CSS px per art px on a 1x display.
 *
 * Pure: no DOM, no settings. YappyPill draws through `pillCamera`, a companion
 * sprite through `placeSprite`; `characterFit.test.ts` walks every pill
 * character x all 17 pill states x every dock with these same functions.
 */
import { FRIENDS, type SpriteName } from "../sprites/art";
import { backingScale, coverCell, sceneBlit, snapRect, type CssRect, type SceneBlit, type Size } from "./pixelGrid";

/** A box in some pixel space: art px, CSS px or device px (the caller says which). */
export interface FitBox { x: number; y: number; w: number; h: number }

/**
 * Who can sit in the pill (companions.html §4, approved 2026-11-06): Yappy
 * (default), Hoot, Tidy, Gulp, Shelly, and later Cinder. Ember, Sal and Mochi
 * are room-only and are never placed in the pill.
 */
export const PILL_CHARACTERS: readonly SpriteName[] = ["yappy", "hoot", "tidy", "gulp", "shelly", "cinder"];

/**
 * Every dock a pill character is drawn at. `top` is the meeting dock (top
 * centre — the same horizontal capsule as `bottom`, mirrored); `side-tall` is a
 * side dock drawing a tall (portrait) scene: the stadium turned on its end.
 */
export const FIT_DOCKS = ["bottom", "top", "left", "right", "side-tall"] as const;
export type FitDock = (typeof FIT_DOCKS)[number];

/** The capsule's 1 CSS px border (YappyPill clips to `x0 + 1 … capW - 2`). */
export const CAPSULE_BORDER = 1;
/** Device px kept clear inside the clip, for the rounding of a fractional capsule edge. */
export const ROUNDING_GUARD = 1;
/** The resting capsule is never narrower than the historical 72 pt pill. */
export const REST_MIN_W = 72;

/* ── the pill scene (YappyPill's world buffer) ──────────────────────────── */

/**
 * Yappy's world buffer, in art px. Big enough that at ONE device px per art px
 * it still covers the widest open capsule (274 x 70 at the desk tier, less the
 * border), so the cell is never forced up past the one that fits him. The sky
 * rows above the ground give a hop room to stay inside the world.
 */
export const PILL_SCENE = { w: 280, h: 72, groundY: 50, faceX: 140 } as const;
/** The chick is drawn this far left of `faceX` at the desk tier (YappyPill drawScene). */
export const CHICK_DESK_SHIFT = 6;

/**
 * The pose range the pill chick is clamped to, so the box below is a promise.
 * `hop` lifts him (art px), `sx`/`sy` squash and stretch, `flap` lifts the
 * wings, `sway` moves the tufts.
 */
export const CHICK_LIMITS = {
  hop: [-2, 6], sx: [-1, 3], sy: [-4, 2], flap: [0, 2], sway: [-2, 2],
} as const satisfies Record<string, readonly [number, number]>;

const clampTo = (v: number, [lo, hi]: readonly [number, number]) => (Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : 0);

/** Clamp a chick pose into CHICK_LIMITS (fields not listed pass through). */
export function clampChickPose<P extends { hop: number; sx: number; sy: number; flap: number; sway: number }>(p: P): P {
  return {
    ...p,
    hop: clampTo(p.hop, CHICK_LIMITS.hop), sx: clampTo(p.sx, CHICK_LIMITS.sx), sy: clampTo(p.sy, CHICK_LIMITS.sy),
    flap: clampTo(p.flap, CHICK_LIMITS.flap), sway: clampTo(p.sway, CHICK_LIMITS.sway),
  };
}

/**
 * The pill chick's bounding box relative to `drawChick(cx, feetY)`, over every
 * pose inside CHICK_LIMITS: tufts (his ears), wings, tail, pencil and feet.
 * From the draw code: tufts top at feetY - 25 - hop - sy, tail right edge at
 * cx + rx + 3, pencil left edge at cx - rx - 4, feet bottom at cy + ry + 1,
 * plus slack for the ellipse rows' floor/ceil.
 */
export const CHICK_EXTENT = { left: 20, right: 20, up: 36, down: 7 } as const;

/** The chick's bounding box in the scene buffer, covering both the plain and the desk position. */
export function chickBox(): FitBox {
  const x = PILL_SCENE.faceX - CHICK_DESK_SHIFT - CHICK_EXTENT.left;
  const y = PILL_SCENE.groundY - CHICK_EXTENT.up;
  return { x, y, w: PILL_SCENE.faceX + CHICK_EXTENT.right - x, h: CHICK_EXTENT.up + CHICK_EXTENT.down };
}

/** A pill character's bounding box size in art px: the sprite manifest's, or the chick's for Yappy. */
export function characterArt(name: SpriteName): Size {
  if (name === "yappy") {
    const b = chickBox();
    // The pill draws the shipped chick; the round-two Yappy sprite must fit the same pill too.
    return { w: Math.max(b.w, FRIENDS.yappy.w), h: Math.max(b.h, FRIENDS.yappy.h) };
  }
  return { w: FRIENDS[name].w, h: FRIENDS[name].h };
}

/* ── geometry ───────────────────────────────────────────────────────────── */

/** Is the point inside the stadium (a rect whose short sides are semicircles)? */
export function inStadium(s: FitBox, px: number, py: number, eps = 1e-6): boolean {
  const r = Math.min(s.w, s.h) / 2;
  if (r <= 0) return false;
  if (s.w >= s.h) {
    const cx = Math.min(Math.max(px, s.x + r), s.x + s.w - r);
    return Math.hypot(px - cx, py - (s.y + r)) <= r + eps;
  }
  const cy = Math.min(Math.max(py, s.y + r), s.y + s.h - r);
  return Math.hypot(px - (s.x + r), py - cy) <= r + eps;
}

/** Does the stadium hold the whole box? (It is convex, so its four corners decide.) */
export function stadiumContains(s: FitBox, b: FitBox): boolean {
  if (!(b.w > 0 && b.h > 0)) return false;
  return inStadium(s, b.x, b.y) && inStadium(s, b.x + b.w, b.y) && inStadium(s, b.x, b.y + b.h) && inStadium(s, b.x + b.w, b.y + b.h);
}

/** A box shrunk by `d` on every side. */
export function inset(b: FitBox, d: number): FitBox {
  return { x: b.x + d, y: b.y + d, w: Math.max(0, b.w - 2 * d), h: Math.max(0, b.h - 2 * d) };
}

/** The visible stadium of a capsule (outer CSS rect), in DEVICE px: the clip, less the rounding guard. */
export function capsuleStadium(capsule: CssRect, dprRaw: number): { clip: FitBox; stadium: FitBox } {
  const dpr = backingScale(dprRaw);
  const clip = snapRect(inset(capsule, CAPSULE_BORDER), dpr);
  return { clip, stadium: inset(clip, ROUNDING_GUARD) };
}

/** The art box at `cell`, centred in `s` on whole device px. */
function centred(s: FitBox, art: Size, cell: number): FitBox {
  const w = art.w * cell, h = art.h * cell;
  return { x: s.x + Math.floor((s.w - w) / 2), y: s.y + Math.floor((s.h - h) / 2), w, h };
}

/** The largest whole cell (device px per art px) at which `art`, centred, sits inside `s`; 0 if none. */
export function largestFittingCell(s: FitBox, art: Size): number {
  if (!(art.w > 0 && art.h > 0)) return 0;
  for (let c = Math.floor(Math.min(s.w / art.w, s.h / art.h)); c >= 1; c--) {
    if (stadiumContains(s, centred(s, art, c))) return c;
  }
  return 0;
}

export interface SpritePlacement {
  /** Device px per art px (whole, >= 1 when `fits`). */
  cell: number;
  /** The sprite's box in device px, centred in the capsule. */
  box: FitBox;
  /** The capsule's visible stadium in device px. */
  stadium: FitBox;
  fits: boolean;
}

/** Place a companion sprite (art `w x h`) in a capsule at the largest whole cell that keeps all of it visible. */
export function placeSprite(capsule: CssRect, art: Size, dprRaw: number): SpritePlacement {
  const { stadium } = capsuleStadium(capsule, dprRaw);
  const cell = largestFittingCell(stadium, art);
  const box = centred(stadium, art, Math.max(1, cell));
  return { cell: Math.max(1, cell), box, stadium, fits: cell >= 1 && stadiumContains(stadium, box) };
}

export interface PillCamera {
  /** What YappyPill blits: src (art px) -> dst (device px), at `blit.cell`. */
  blit: SceneBlit;
  /** The chick's bounding box on screen, device px. */
  char: FitBox;
  stadium: FitBox;
  fits: boolean;
}

/**
 * Yappy's camera for a capsule: centred on his bounding box (never on his face
 * alone, never on the buffer), at the largest whole cell that keeps the whole
 * box inside the visible stadium — and never a cell so small the world stops
 * covering the capsule (the buffer is sized so that never bites).
 */
export function pillCamera(capsule: CssRect, dprRaw: number): PillCamera {
  const dpr = backingScale(dprRaw);
  const { clip, stadium } = capsuleStadium(capsule, dpr);
  const env = chickBox();
  const fit = Math.max(1, largestFittingCell(stadium, env));
  const cell = Math.max(fit, coverCell(clip.w, clip.h, PILL_SCENE.w, PILL_SCENE.h));
  const blit = sceneBlit({
    rect: inset(capsule, CAPSULE_BORDER),
    view: { w: clip.w / cell, h: clip.h / cell },
    centre: { x: env.x + env.w / 2, y: env.y + env.h / 2 },
    buffer: { w: PILL_SCENE.w, h: PILL_SCENE.h },
    dpr,
    cell,
  });
  const char: FitBox = {
    x: blit.dst.x + (env.x - blit.src.x) * blit.cell,
    y: blit.dst.y + (env.y - blit.src.y) * blit.cell,
    w: env.w * blit.cell,
    h: env.h * blit.cell,
  };
  return { blit, char, stadium, fits: stadiumContains(stadium, char) };
}

/* ── the capsule for a state ────────────────────────────────────────────── */

/** Yappy's open capsule at tier 1 (YappyPill: 236 x 60, x tier). */
export const OPEN_CAPSULE = { w: 236, h: 60 } as const;
/** The open tiers: plain, notepad, desk. */
export const OPEN_TIERS = [1, 1.05, 1.16] as const;

/**
 * The smallest resting capsule (CSS px, whole) that holds every pill
 * character's box at ONE device px per art px on a 1x display: least area,
 * then least height, never narrower than REST_MIN_W.
 */
export function restCapsuleFor(arts: readonly Size[]): Size {
  let best: Size | null = null;
  for (let h = 22; h <= 120; h++) {
    for (let w = REST_MIN_W; w <= 240; w++) {
      const { stadium } = capsuleStadium({ x: 0, y: 0, w, h }, 1);
      if (arts.every((a) => largestFittingCell(stadium, a) >= 1)) {
        if (!best || w * h < best.w * best.h) best = { w, h };
        break;
      }
    }
  }
  return best ?? { w: 240, h: 120 };
}

/** The resting capsule every pill character fits (computed once from the manifests). */
export const REST_CAPSULE: Readonly<Size> = Object.freeze(restCapsuleFor(PILL_CHARACTERS.map(characterArt)));

/** The capsule a dock draws, given the horizontal capsule size: `side-tall` turns it on its end. */
export function capsuleForDock(size: Size, dock: FitDock): Size {
  return dock === "side-tall" ? { w: size.h, h: size.w } : { w: size.w, h: size.h };
}

export interface CharacterPlacement {
  /** "scene": Yappy's world through `pillCamera`; "sprite": the character's own sprite through `placeSprite`. */
  route: "scene" | "sprite";
  cell: number;
  /** The character's bounding box on screen, device px. */
  char: FitBox;
  stadium: FitBox;
  fits: boolean;
}

/**
 * Where `name` is drawn in a capsule at `dock`. Yappy on a horizontal capsule
 * is his landscape world (`pillCamera`, what YappyPill draws); a tall side
 * scene, and every companion, is the character's own sprite (`placeSprite`) —
 * the landscape world strip is never stood on its end.
 */
export function placeCharacter(name: SpriteName, dock: FitDock, capsule: CssRect, dprRaw: number): CharacterPlacement {
  if (name === "yappy" && dock !== "side-tall") {
    const c = pillCamera(capsule, dprRaw);
    return { route: "scene", cell: c.blit.cell, char: c.char, stadium: c.stadium, fits: c.fits };
  }
  const p = placeSprite(capsule, characterArt(name), dprRaw);
  return { route: "sprite", cell: p.cell, char: p.box, stadium: p.stadium, fits: p.fits };
}

/** One structured line when a placement does not fit (never expected; the matrix test pins it). */
export function cropWarning(who: string, capsule: Size, dpr: number): string {
  return `pill character-cropped character=${who} capW=${Math.round(capsule.w)} capH=${Math.round(capsule.h)} dpr=${dpr}`;
}
