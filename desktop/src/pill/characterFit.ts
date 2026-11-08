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
 * rounded clip, inset one device px for rounding) with POSITION_GUARD to spare.
 * The resting capsule is sized
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
/**
 * yap-1107-parity-pill — the approved shell's padding (pill.html .ypill
 * padding:4px, tokens.css --pill-shell-pad): the dark shell is painted by the
 * DOM (.yappy-capsule-box) and the sky window inside it is the capsule less
 * SHELL_PAD on every side (the 72 x 28 nub holds a 64 x 20 window).
 */
export const SHELL_PAD = 4;
/** The sky window inside a capsule (pill.html PK.pill: win = shell less the padding). */
export function shellWindow(capsule: { x: number; y: number; w: number; h: number }): { x: number; y: number; w: number; h: number; r: number } {
  const w = Math.max(1, capsule.w - 2 * SHELL_PAD), h = Math.max(1, capsule.h - 2 * SHELL_PAD);
  // pill-kit.js: win.r ?? max(4, shell.h / 2 - 4), concentric with the shell's ends
  return { x: capsule.x + SHELL_PAD, y: capsule.y + SHELL_PAD, w, h, r: Math.max(4, capsule.h / 2 - SHELL_PAD) };
}
/** Device px kept clear inside the clip, for the rounding of a fractional capsule edge. */
export const ROUNDING_GUARD = 1;
/**
 * Device px of play a fitted box is checked with: the scene camera lands the
 * box within one device px of the ideal centre (whole-px origins), so a fit
 * that holds with this much clearance holds wherever the rounding puts it.
 */
export const POSITION_GUARD = 2;
/** The resting capsule is never narrower than the historical 72 pt pill. */
export const REST_MIN_W = 72;

/* ── the pill scene (YappyPill's world buffer) ──────────────────────────── */

/**
 * Yappy's world buffer, in art px. Big enough that at ONE device px per art px
 * it still covers every capsule the camera fits him into at cell 1 — the
 * widest is a 1.5x display mid-lerp (about 370 x 92 device px), and the resting
 * capsule at 1.5x is 123 x 78 — so the cover cell is never forced up past the
 * one that fits him (a 72-row buffer forced cell 2 at rest on 1.5x and cropped
 * him). The sky rows above the ground give a hop room to stay inside the world.
 */
export const PILL_SCENE = { w: 420, h: 104, groundY: 66, faceX: 210 } as const;
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

/**
 * The most the chick ever rises above his resting pose, in art px: a full hop
 * plus a full stretch (his tufts top out at feetY - 25 - hop - sy).
 */
export const MAX_LIFT = CHICK_LIMITS.hop[1] + CHICK_LIMITS.sy[1];

/** A lift budget as a whole number of art px in 0 … MAX_LIFT (anything else is the full lift). */
export function liftBudget(lift: number): number {
  return Number.isFinite(lift) ? Math.min(MAX_LIFT, Math.max(0, Math.floor(lift))) : MAX_LIFT;
}

/**
 * Clamp a chick pose into CHICK_LIMITS (fields not listed pass through).
 *
 * yap-1107-pill-matches-artifact-size — `lift` caps how far the pose may rise
 * (hop first, then stretch) so the pose stays inside `chickBoxFor(lift)`: the
 * camera can then draw him at the approved artifact's size without his tufts
 * or a hop leaving the capsule. Crouch, squash, wings and sway are untouched.
 */
export function clampChickPose<P extends { hop: number; sx: number; sy: number; flap: number; sway: number }>(p: P, lift: number = MAX_LIFT): P {
  const l = liftBudget(lift);
  const hopHi = Math.min(CHICK_LIMITS.hop[1], l);
  const syHi = Math.min(CHICK_LIMITS.sy[1], l - hopHi);
  return {
    ...p,
    hop: clampTo(p.hop, [CHICK_LIMITS.hop[0], hopHi]), sx: clampTo(p.sx, CHICK_LIMITS.sx), sy: clampTo(p.sy, [CHICK_LIMITS.sy[0], syHi]),
    flap: clampTo(p.flap, CHICK_LIMITS.flap), sway: clampTo(p.sway, CHICK_LIMITS.sway),
  };
}

/**
 * The pill chick's bounding box relative to `drawChick(cx, feetY)`, over every
 * pose inside CHICK_LIMITS: sprout (his ears), wings, tail, pencil, toes and
 * the ground shadow. From the draw code (yap-1106-yappy-detail-pass): sprout
 * top at feetY - 26 - hop - sy, tail right edge at cx + rx + 3, pencil left
 * edge at cx - rx - 5, toes bottom at cy + ry + 2 (feetY + 6 at the limits),
 * shadow rows feetY .. feetY + 3 within cx +- 10, plus slack for the ellipse
 * rows' floor/ceil. `chick_box_holds_every_pose` paints every pose to prove it.
 */
export const CHICK_EXTENT = { left: 20, right: 20, up: 36, down: 7 } as const;

/** The chick's bounding box in the scene buffer, covering both the plain and the desk position. */
export function chickBox(): FitBox {
  const x = PILL_SCENE.faceX - CHICK_DESK_SHIFT - CHICK_EXTENT.left;
  const y = PILL_SCENE.groundY - CHICK_EXTENT.up;
  return { x, y, w: PILL_SCENE.faceX + CHICK_EXTENT.right - x, h: CHICK_EXTENT.up + CHICK_EXTENT.down };
}

/**
 * yap-1107-pill-matches-artifact-size (bounce fix) — the chick's MEASURED rows
 * relative to feetY, from drawChick (sprites/art.ts): the sprout top is the row
 * feetY - 26 - hop - sy (cy = feetY - 11 - hop, ry = 11 + sy, sprout at
 * cy - ry - 4), and the toes' last row is feetY + 2 - hop + sy (cy + ry + 2), so
 * the box ends one row lower; the ground shadow (ell at feetY + 2, ry 1.6) ends
 * at feetY + 4. CHICK_EXTENT keeps two rows of slack on top for the full-range
 * box (fb-10, unchanged); a clamped pose uses these exact rows, which is what
 * lets the artifact-height shell (52, built at 53) hold him whole at its scale.
 */
export const CHICK_ROWS = { sproutAbove: 26, toeBelow: 3, shadowBelow: 4 } as const;

/**
 * The chick's bounding box when his pose is clamped to `lift` (clampChickPose).
 * `chickBoxFor(MAX_LIFT)` is `chickBox()` (the fb-10 box, slack included). Any
 * smaller budget is the exact painted extent of every clamped pose: the top is
 * the sprout at the highest hop + stretch the budget allows, the bottom the toes
 * at the deepest crouch with the stretch the budget allows (a squash only
 * raises them). Same left and right as the full box; the box only shrinks.
 * `chick_box_for_lift_holds_every_clamped_pose` paints every pose to prove it.
 */
export function chickBoxFor(lift: number): FitBox {
  const b = chickBox();
  const l = liftBudget(lift);
  if (l >= MAX_LIFT) return b;
  const hopHi = Math.min(CHICK_LIMITS.hop[1], l);
  const syHi = Math.min(CHICK_LIMITS.sy[1], l - hopHi);
  const up = CHICK_ROWS.sproutAbove + l;
  const down = Math.max(CHICK_ROWS.shadowBelow, CHICK_ROWS.toeBelow - CHICK_LIMITS.hop[0] + syHi);
  return { x: b.x, y: PILL_SCENE.groundY - up, w: b.w, h: up + down };
}

/* ── the approved artifact's sprite scale ───────────────────────────────── */

/**
 * yap-1107-pill-matches-artifact-size — the approved Pill artifact
 * (design-2026-11-06 pill.html, the listening pill): a 220 CSS px window shows
 * 0.8 x 176 art px of the scene, so Yappy is drawn at 1.5625 CSS px per art px.
 * Wilson 2026-11-07: "make sure they look exactly like the artifacts" — the
 * app drew him at 1.0 on 1x and 2x (64 percent of the approved size).
 */
export const ARTIFACT_SPRITE_SCALE = 1.5625;
/** A whole cell may land this far above the artifact's scale and still count as it (the item's 5 percent). */
export const ARTIFACT_TOLERANCE = 0.05;

export interface PillFit {
  /** Device px per art px. */
  cell: number;
  /** The lift budget the pose is clamped to at that cell (MAX_LIFT = a full hop). */
  lift: number;
}

/**
 * The cell and lift Yappy is drawn at in a capsule's visible stadium (device px).
 *
 * The rule: the largest whole cell at or under the artifact's scale (plus the
 * tolerance) at which his WHOLE box still fits, giving up only hop height to
 * get there — never a pixel of him (yap-1106-fb-10 still holds: the box for the
 * clamped pose sits inside the stadium). When no cell above the full-hop fit
 * holds even a resting pose, the full-hop fit is used, as before. On a 1x
 * display the artifact's 1.56 rounds down to cell 1, so 1x is unchanged; on
 * 2x the open capsule (228 x 53) draws cell 3 (1.5 CSS px per art px, 96
 * percent of the artifact) with a one-art-px hop.
 */
export function pillFit(stadium: FitBox, dprRaw: number, boxFor: (lift: number) => FitBox = chickBoxFor): PillFit {
  const dpr = backingScale(dprRaw);
  // yap-1106-main-character-switch — `boxFor` is the box of whoever sits in the
  // pill at a lift budget: Yappy's (`chickBoxFor`, the default) or a friend's
  // (pill/occupant.ts `occupantBoxFor`), so a taller friend is never cropped.
  const whole = Math.max(1, largestFittingCell(stadium, boxFor(MAX_LIFT), POSITION_GUARD));
  const ceiling = Math.floor(ARTIFACT_SPRITE_SCALE * (1 + ARTIFACT_TOLERANCE) * dpr + 1e-9);
  for (let c = ceiling; c > whole; c--) {
    for (let l = MAX_LIFT - 1; l >= 0; l--) {
      if (largestFittingCell(stadium, boxFor(l), POSITION_GUARD) >= c) return { cell: c, lift: l };
    }
  }
  return { cell: whole, lift: MAX_LIFT };
}

/** CSS px per art px a fit draws at (what the artifact's 1.5625 is compared with). */
export function cssPerArtPx(fit: Pick<PillFit, "cell">, dprRaw: number): number {
  return fit.cell / backingScale(dprRaw);
}

/** One structured line when the pill's sprite scale changes (no content, numbers only). */
/**
 * yap-1107-parity-pill — one structured line when Yappy's camera framing changes
 * ("whole" <-> "head", i.e. the 72 x 28 resting nub on a display below 2x).
 * Numbers and enums only, never content.
 */
export function restFramingEvent(framing: PillCamera["framing"], capsule: Size, dpr: number): string {
  return `pill camera-framing framing=${framing} capW=${Math.round(capsule.w)} capH=${Math.round(capsule.h)} dpr=${dpr}`;
}

export function scaleEvent(fit: PillFit, capsule: Size, dpr: number): string {
  return `pill sprite-scale cell=${fit.cell} lift=${fit.lift} cssPerArt=${cssPerArtPx(fit, dpr).toFixed(3)} capW=${Math.round(capsule.w)} capH=${Math.round(capsule.h)} dpr=${dpr}`;
}

/** A pill character's bounding box size in art px: the sprite manifest's, or the chick's for Yappy. */
export function characterArt(name: SpriteName): Size {
  if (name === "yappy") {
    const b = chickBox();
    // The pill chick IS the round-two Yappy (detail pass); the sprite manifest must fit the same pill too.
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

/**
 * The largest whole cell (device px per art px) at which `art`, centred
 * exactly, sits inside `s` with `guard` device px to spare; 0 if none.
 */
export function largestFittingCell(s: FitBox, art: Size, guard = 0): number {
  if (!(art.w > 0 && art.h > 0)) return 0;
  const t = inset(s, guard);
  for (let c = Math.floor(Math.min(t.w / art.w, t.h / art.h)); c >= 1; c--) {
    const w = art.w * c, h = art.h * c;
    if (stadiumContains(t, { x: t.x + (t.w - w) / 2, y: t.y + (t.h - h) / 2, w, h })) return c;
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
  const cell = largestFittingCell(stadium, art, POSITION_GUARD);
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
  /** The lift budget the scene must clamp his pose to this frame (clampChickPose(pose, lift)). */
  lift: number;
  /**
   * yap-1107-parity-pill — "whole": his whole box is centred inside the stadium
   * (`fits`). "head": the capsule is too short for any whole cell on this
   * display (the approved 72 x 28 resting nub on a 1x / 1.5x screen), so the
   * camera pins the TOP of his box under the stadium's top edge: the sprout,
   * eyes and beak stay on screen and only his feet leave it — never his face
   * alone, never a cropped head (the fb-10 regression Wilson saw).
   */
  framing: "whole" | "head";
}

/**
 * Yappy's camera for a capsule: centred on his bounding box (never on his face
 * alone, never on the buffer), at the largest whole cell that keeps the whole
 * box inside the visible stadium — and never a cell so small the world stops
 * covering the capsule (the buffer is sized so that never bites).
 */
export function pillCamera(capsule: CssRect, dprRaw: number, boxFor: (lift: number) => FitBox = chickBoxFor): PillCamera {
  const dpr = backingScale(dprRaw);
  const { clip, stadium } = capsuleStadium(capsule, dpr);
  // yap-1107-pill-matches-artifact-size — the approved artifact's size, whole.
  // yap-1106-main-character-switch — framed on whoever sits in the pill.
  const fit = pillFit(stadium, dpr, boxFor);
  const env = boxFor(fit.lift);
  const cell = Math.max(fit.cell, coverCell(clip.w, clip.h, PILL_SCENE.w, PILL_SCENE.h));
  const view = { w: clip.w / cell, h: clip.h / cell };
  // yap-1107-parity-pill — whole when a whole cell holds his box; otherwise
  // (only the 28 px resting nub below 2x) the top of his box sits
  // ROUNDING_GUARD + POSITION_GUARD device px under the clip's top edge.
  const whole = largestFittingCell(stadium, env, POSITION_GUARD) >= 1;
  const headPad = (ROUNDING_GUARD + POSITION_GUARD) / cell;
  const centre = whole
    ? { x: env.x + env.w / 2, y: env.y + env.h / 2 }
    : { x: env.x + env.w / 2, y: env.y - headPad + view.h / 2 };
  const blit = sceneBlit({
    rect: inset(capsule, CAPSULE_BORDER),
    view,
    centre,
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
  return { blit, char, stadium, fits: stadiumContains(stadium, char), lift: fit.lift, framing: whole ? "whole" : "head" };
}

/* ── the capsule for a state ────────────────────────────────────────────── */

/**
 * The approved Pill artifact's shells (design-2026-11-06 pill.html): the
 * listening / hands-free / done shell is 228 x 52 (pill-kit 4 px padding round a
 * 220 x 44 window). The resting nub is 72 x 28 (pill.html: Resting, Asleep,
 * the hover tray and every storyboard frame draw PK.pill({ h: 28, win: { w: 64,
 * h: 20 } }), 4 px padding round the window; "Shell grows from 72 x 28 to
 * 228 x 52 from its top edge").
 *
 * yap-1107-parity-pill (batch-7 bounce fix): the item's own text said the idle
 * pill is 56 tall; the senior standard says "if the artifact and your item
 * conflict, build the artifact", so the resting capsule IS the page's 72 x 28.
 */
export const ARTIFACT_OPEN_SHELL = { w: 228, h: 52 } as const;
export const ARTIFACT_REST_SHELL = { w: 72, h: 28 } as const;
export const ARTIFACT_REST_H = ARTIFACT_REST_SHELL.h;

/**
 * Yappy's open capsule at tier 1 (YappyPill: openCapsuleFor(tier)).
 * yap-1107-pill-matches-artifact-size (bounce fix): the artifact's 228 wide;
 * was 236 x 60 (+15 percent tall), which the batch-10 gate bounced.
 * yap-1107-parity-pill (batch-5 bounce fix): 53 tall, not 54. The parity rule is
 * 3 percent of the artifact's 52 (54 was 3.8 percent off); 53 is 1.9 percent and
 * is still the least height at which a 2x display draws him at the artifact's
 * scale (cell 3 = 1.5 CSS px per art px) whole AND hopping one art px: the 2x
 * clip is (53 - 2) x 2 = 102 device px, the stadium 100, less the 2 px position
 * guard each side 96 = 3 x 32 (chickBoxFor(1) is 32 art px tall). At 52 the hop
 * budget is 0 (a frozen done hop), so 52 stays the artifact's number, 53 the build.
 */
export const OPEN_CAPSULE = { w: 228, h: 53 } as const;
/** The open tiers: plain, notepad, desk. */
export const OPEN_TIERS = [1, 1.05, 1.16] as const;

/**
 * yap-1107-parity-pill — the open capsule at a prop tier. The approved page
 * keeps EVERY open shell 52 tall (pill.html: Holding fn, Hands-free with the
 * notepad and Long take with the desk all draw PK.pill({ h: 52, win: { h: 44 } })),
 * so a prop tier widens the capsule for the notepad / desk but never makes it
 * taller (it used to scale the height too: 54 x 1.16 = 63, +21 percent).
 */
export function openCapsuleFor(tier: number): Size {
  const t = Number.isFinite(tier) && tier > 0 ? tier : 1;
  return { w: OPEN_CAPSULE.w * t, h: OPEN_CAPSULE.h };
}

/**
 * yap-1107-pill-matches-artifact-size (bounce fix) — restCapsuleFor's search cap:
 * never taller than 56 when a capsule that short holds everyone at 1x (the
 * least-area search alone picked 80 x 59). No longer the resting capsule's size
 * (yap-1107-parity-pill: REST_CAPSULE is the page's 72 x 28).
 */
export const REST_MAX_H = 56;

/**
 * The smallest resting capsule (CSS px, whole) that holds every pill
 * character's box at ONE device px per art px on a 1x display, lying down and
 * stood on its end (the side-tall dock): least area,
 * then least height, never narrower than REST_MIN_W, and no taller than
 * `maxH` (REST_MAX_H) unless nothing that short holds them all.
 */
export function restCapsuleFor(arts: readonly Size[], maxH: number = REST_MAX_H): Size {
  return restCapsuleUpTo(arts, maxH) ?? restCapsuleUpTo(arts, 120) ?? { w: 240, h: 120 };
}

function restCapsuleUpTo(arts: readonly Size[], maxH: number): Size | null {
  let best: Size | null = null;
  for (let h = 22; h <= Math.min(120, maxH); h++) {
    for (let w = REST_MIN_W; w <= 240; w++) {
      const { stadium } = capsuleStadium({ x: 0, y: 0, w, h }, 1);
      // Both orientations: a side dock stands the same capsule on its end
      // (capsuleForDock "side-tall"), which is the art turned a quarter in this one.
      if (arts.every((a) => largestFittingCell(stadium, a, POSITION_GUARD) >= 1 && largestFittingCell(stadium, { w: a.h, h: a.w }, POSITION_GUARD) >= 1)) {
        if (!best || w * h < best.w * best.h) best = { w, h };
        break;
      }
    }
  }
  return best;
}

/**
 * The resting capsule: the approved page's 72 x 28 nub, exactly (yap-1107-parity-pill).
 * On a 2x display every pill character sits in it whole at one device px per
 * art px (2x clip 140 x 52, stadium 138 x 50, the largest box 46 x 43 with the
 * 2 px guard each side); below 2x no whole cell fits 26 px, and Yappy's camera
 * keeps his head in view ("head" framing). restCapsuleFor stays: it is the
 * least capsule that holds everyone at 1x (80 x 56, the build before this fix).
 */
export const REST_CAPSULE: Readonly<Size> = Object.freeze({ w: ARTIFACT_REST_SHELL.w, h: ARTIFACT_REST_SHELL.h });

/**
 * The least width a side-tall capsule is drawn at (CSS px). The open shell is
 * 53 tall (yap-1107-parity-pill), but a side dock is the tall vdock shell, up to
 * 64 wide (vdock.ts VDOCK_MAX_W, pill.html #vdock "never wider than 64 px"), not
 * the horizontal capsule turned: stood on its end it keeps 54, the least width
 * that holds the widest pill character (46 art px) at one device px per art px
 * on 1x (clip 52, stadium 50, less the 2 px position guard each side = 46).
 */
export const SIDE_TALL_MIN_W = 54;

/** The capsule a dock draws, given the horizontal capsule size: `side-tall` turns it on its end. */
export function capsuleForDock(size: Size, dock: FitDock): Size {
  return dock === "side-tall" ? { w: Math.max(size.h, SIDE_TALL_MIN_W), h: size.w } : { w: size.w, h: size.h };
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
