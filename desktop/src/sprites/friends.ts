/**
 * yap-1106-friends-art-hd — the eight friends as CHARACTER MANIFESTS.
 *
 * The art itself is the approved round-two redraw (design-2026-11-06
 * friends-hd.js, ported verbatim into ./art.ts by FB-09 and pixel-locked by
 * art.snapshot.test.ts): Yappy's density rules, light from the top left, a
 * base tone plus a light patch and a shade patch, a warm dark outline, a
 * two-pixel eye shine, blush at 70 percent and a ground shadow, at Yappy's
 * pixel size. The approved draw bodies stay untouched.
 *
 * What the approved art did NOT give every friend is a full frame set: some
 * had a blink and no action, some an action and no blink. A manifest gives
 * each friend exactly four frames, all in one shape:
 *
 *   idle            the approved resting frame
 *   blink           eyes shut for a beat (Mochi, whose eyes rest shut, peeks)
 *   action[0..1]    a two-frame loop of the friend's own thing
 *
 * Every frame is an approved base frame (`f` + `opt`) plus, where needed, a
 * small art-pixel layer drawn UNDER or OVER it with the same helpers, so a
 * frame stays a grid of whole art pixels and scales crisply (crisp.ts).
 * Ember floats, so the approved sprite had no ground shadow and no blush; its
 * manifest adds both to every frame (`base`).
 *
 * Pure and deterministic: no DOM, no timers. `pickFrame` turns a clock into a
 * frame so the caller owns the animation loop (and reduce-motion).
 */
import { FRIENDS, H, eye, shadow, type Helpers, type PixelCtx, type SpriteName, type SpriteOpt } from "./art";

/** Every friend (Yappy is the reference, not a friend). */
export type FriendId = Exclude<SpriteName, "yappy">;

/** The friends, in the order the companions page names them. */
export const FRIEND_IDS: readonly FriendId[] = ["hoot", "shelly", "tidy", "gulp", "ember", "sal", "mochi", "cinder"];

/** A layer of extra art pixels, drawn with the sprite's own helpers. */
export type Layer = (h: Helpers) => void;

/** One drawable frame: an approved base frame plus optional layers. */
export interface FrameSpec {
  /** The approved draw's frame number. */
  f: number;
  opt?: SpriteOpt;
  /** Drawn before the base frame (behind it). */
  under?: Layer;
  /** Drawn after the base frame (on top of it). */
  over?: Layer;
}

export type FrameKind = "idle" | "blink" | "action";

/** The character manifest of one friend. */
export interface FriendManifest {
  id: FriendId;
  label: string;
  /** Art box in art pixels (the approved sprite's w x h). */
  w: number;
  h: number;
  /** Layers drawn on EVERY frame (Ember's added shadow and blush). */
  base?: { under?: Layer; over?: Layer };
  /** Mochi: eyes rest shut, so the "blink" frame is a one-eyed peek. */
  eyesClosedAtRest?: boolean;
  idle: FrameSpec;
  blink: FrameSpec;
  action: {
    /** What the two frames show, as a short verb for logs and alt text. */
    name: string;
    frames: readonly [FrameSpec, FrameSpec];
    /** How long each action frame holds. */
    frameMs: number;
  };
  /** Blinks recur every `min..max` ms (a per-friend offset inside it). */
  blinkEveryMs: readonly [number, number];
  /** How long a blink holds. */
  blinkMs: number;
}

/* ── small art layers ─────────────────────────────────────────────────── */

const INK = "#2b2118";

/** A quaver: 2x2 head, a 6 px stem, a 2 px flag. Top-left at (x, y). */
function note(h: Helpers, x: number, y: number, col: string): void {
  h.r(x, y + 4, 2, 2, col); h.r(x + 2, y, 1, 6, col); h.r(x + 3, y, 1, 1, col); h.r(x + 4, y + 1, 1, 1, col);
  h.r(x, y + 4, 1, 1, "#c8956a");
}

function dust(h: Helpers, pts: readonly (readonly [number, number])[]): void {
  for (const [x, y] of pts) h.r(x, y, 1, 1, "#d9cdb4", .8);
}

/** Tidy's f === 1 shuts both eyes; paint them open again (the f0 pixels). */
function tidyEyesOpen(h: Helpers): void {
  h.r(15, 11, 3, 1, "#fff"); h.r(23, 11, 3, 1, "#fff");
  h.r(16, 11, 2, 2, INK); h.r(24, 11, 2, 2, INK); h.r(16, 11, 1, 1, "#fff"); h.r(24, 11, 1, 1, "#fff");
}

/* ── the manifests ────────────────────────────────────────────────────── */

const box = (id: FriendId) => ({ w: FRIENDS[id].w, h: FRIENDS[id].h });

const MANIFESTS: Readonly<Record<FriendId, FriendManifest>> = {
  hoot: {
    id: "hoot", label: "Hoot", ...box("hoot"),
    idle: { f: 0 },
    blink: { f: 1 },
    // Hoot runs the tape machine: he bobs along to it, a note either side.
    action: {
      name: "listening",
      frames: [
        { f: 0, over: (h) => note(h, 31, 2, "#4a2c1e") },
        { f: 0, over: (h) => note(h, 3, 0, "#4a2c1e") },
      ],
      frameMs: 260,
    },
    blinkEveryMs: [3200, 5600], blinkMs: 150,
  },
  shelly: {
    id: "shelly", label: "Shelly", ...box("shelly"),
    idle: { f: 0 },
    // Lids in shell-skin tone with a warm lash line, over both stalk eyes.
    blink: {
      f: 0,
      over: (h) => {
        h.ell(35.5, 7, 1.6, 1.6, "#e8d9b5"); h.r(34, 7, 3, 1, "#5a4632");
        h.ell(40.5, 7, 1.6, 1.6, "#e8d9b5"); h.r(39, 7, 3, 1, "#5a4632");
      },
    },
    // Charging: stalks up, three bars, the top bar flashing.
    action: {
      name: "charging",
      frames: [{ f: 1 }, { f: 1, over: (h) => h.r(26, 3, 3, 2, "#fff6c0") }],
      frameMs: 320,
    },
    blinkEveryMs: [3600, 6000], blinkMs: 160,
  },
  tidy: {
    id: "tidy", label: "Tidy", ...box("tidy"),
    idle: { f: 0 },
    blink: { f: 1 },
    // Sweeping: the broom swings a pixel, a puff of dust each way.
    action: {
      name: "sweeping",
      frames: [
        { f: 0, over: (h) => dust(h, [[37, 30], [38, 28]]) },
        { f: 1, over: (h) => { tidyEyesOpen(h); dust(h, [[36, 31], [37, 29], [39, 30]]); } },
      ],
      frameMs: 200,
    },
    blinkEveryMs: [2800, 5000], blinkMs: 130,
  },
  gulp: {
    id: "gulp", label: "Gulp", ...box("gulp"),
    idle: { f: 0 },
    // Frog lids: skin over the eyeball, a lash line across.
    blink: {
      f: 0,
      over: (h) => {
        h.ell(14, 14, 2.8, 3, "#6cc35a"); h.r(12, 14, 4, 1, INK);
        h.ell(28, 14, 2.8, 3, "#6cc35a"); h.r(26, 14, 4, 1, INK);
      },
    },
    // Eating a page: mouth open on it, then shut with the corner sticking out.
    action: {
      name: "eating",
      frames: [
        { f: 1 },
        { f: 0, over: (h) => { h.r(30, 19, 3, 1, "#c9c3ad"); h.r(30, 20, 3, 2, "#fffdf5"); h.r(33, 20, 1, 2, "#c9c3ad"); } },
      ],
      frameMs: 240,
    },
    blinkEveryMs: [3000, 5400], blinkMs: 150,
  },
  ember: {
    id: "ember", label: "Ember", ...box("ember"),
    // Ember floats: a smaller shadow a few pixels below, and his blush.
    base: {
      under: (h) => shadow(h, 14, 25, 6),
      over: (h) => h.r(9, 16, 2, 1, "#ff89a8", .7),
    },
    idle: { f: 0 },
    blink: { f: 0, over: (h) => { h.ell(7, 14, 1.6, 1.8, "#3e3450"); h.r(6, 14, 3, 1, INK); } },
    // Glowing: a dim pulse with three sparks, then the bright one with five.
    action: {
      name: "glowing",
      frames: [
        { f: 0, over: (h) => { for (const [x, y] of [[27, 17], [25, 22], [18, 23]] as const) h.r(x, y, 1, 1, "#fff6a0", .45); } },
        { f: 1 },
      ],
      frameMs: 300,
    },
    blinkEveryMs: [2600, 4800], blinkMs: 130,
  },
  sal: {
    id: "sal", label: "Sal", ...box("sal"),
    idle: { f: 0 },
    blink: { f: 1 },
    // Wagging: the tail up a pixel, then down a pixel, eyes open both times.
    action: {
      name: "wagging",
      frames: [{ f: -1 }, { f: 1, over: (h) => { eye(h, 7, 15, 0); eye(h, 12, 15, 0); } }],
      frameMs: 180,
    },
    blinkEveryMs: [3000, 5200], blinkMs: 140,
  },
  mochi: {
    id: "mochi", label: "Mochi", ...box("mochi"), eyesClosedAtRest: true,
    idle: { f: 0 },
    // Mochi dozes with her eyes shut, so her "blink" is a one-eyed peek.
    blink: { f: 0, over: (h) => eye(h, 15, 15, 0) },
    // Dozing: one z, then a second, bigger one drifting up.
    action: {
      name: "dozing",
      frames: [
        { f: 1 },
        { f: 1, over: (h) => { h.r(33, 0, 3, 1, "#5a6280", .7); h.r(34, 1, 1, 1, "#5a6280", .7); h.r(33, 2, 3, 1, "#5a6280", .7); } },
      ],
      frameMs: 600,
    },
    blinkEveryMs: [6000, 9000], blinkMs: 400,
  },
  cinder: {
    id: "cinder", label: "Cinder", ...box("cinder"),
    idle: { f: 0 },
    blink: { f: 1 },
    // Breathing fire (and delivering the news): mouth open, chest flame flickers.
    action: {
      name: "breathing",
      frames: [{ f: 0, opt: { talk: true, t: 0 } }, { f: 0, opt: { talk: true, t: 0.2 } }],
      frameMs: 160,
    },
    blinkEveryMs: [3000, 5600], blinkMs: 140,
  },
};

const isFriend = (id: string): id is FriendId => (FRIEND_IDS as readonly string[]).includes(id);

/** The manifest for `id`, or null for anything that is not a friend. */
export function friendManifest(id: string): FriendManifest | null {
  return isFriend(id) ? MANIFESTS[id] : null;
}

/** The frame spec for `kind` (`index` picks the action frame, 0 or 1). */
export function frameSpec(m: FriendManifest, kind: FrameKind, index = 0): FrameSpec {
  if (kind === "idle") return m.idle;
  if (kind === "blink") return m.blink;
  return m.action.frames[index === 1 ? 1 : 0];
}

/**
 * Draw one frame of a friend at the context's origin, in art pixels (scale
 * with crisp.ts). Returns false and draws nothing for an unknown id.
 */
export function drawFriendFrame(o: PixelCtx, id: string, kind: FrameKind, index = 0): boolean {
  const m = friendManifest(id);
  if (!m) {
    console.warn(`sprites friend_frame_unknown id=${String(id).slice(0, 32)} kind=${kind}`);
    return false;
  }
  const s = frameSpec(m, kind, index);
  const h = H(o);
  m.base?.under?.(h);
  s.under?.(h);
  FRIENDS[m.id].draw(o, h, s.f, s.opt);
  s.over?.(h);
  m.base?.over?.(h);
  return true;
}

export interface FramePick {
  kind: FrameKind;
  index: number;
}

/** A stable per-friend offset so the friends never blink in unison. */
function offsetFor(id: string, span: number): number {
  let n = 0;
  for (let i = 0; i < id.length; i++) n = (n * 31 + id.charCodeAt(i)) % 100003;
  return span > 0 ? n % (span + 1) : 0;
}

/**
 * Which frame a friend shows at `tMs`. Acting loops the two action frames;
 * resting blinks once per (per-friend) period. Reduce-motion never animates:
 * it holds idle, or the first action frame while acting, so the state still
 * reads. A clock that is not a finite, non-negative number shows idle.
 */
export function pickFrame(m: FriendManifest, tMs: number, opts: { acting?: boolean; reducedMotion?: boolean } = {}): FramePick {
  const acting = opts.acting === true;
  if (opts.reducedMotion) return acting ? { kind: "action", index: 0 } : { kind: "idle", index: 0 };
  if (!Number.isFinite(tMs) || tMs < 0) return { kind: "idle", index: 0 };
  if (acting) return { kind: "action", index: Math.floor(tMs / m.action.frameMs) % 2 };
  const [lo, hi] = m.blinkEveryMs;
  const period = lo + offsetFor(m.id, hi - lo);
  return tMs % period >= period - m.blinkMs ? { kind: "blink", index: 0 } : { kind: "idle", index: 0 };
}
