/**
 * yap-1106-pill-vertical-dock-world (+ FB-20) — on a side dock the pill is a
 * TALL ROOM, never wider than 64 px (pill.html section 12, approved).
 *
 * Wilson, 2026-11-06: "why is it cut off like that you can't even see yappy …
 * make it actually look good vertically". The design page drew a 40 x 128 art
 * room stretched 1.4x into a 56 x 180 canvas, so the 40-art-px-wide chick ran
 * off the room's edge and the art was never on whole pixels. Here the room IS
 * 56 x 180 art px — one CSS px per art px, the same size Yappy is in the
 * horizontal pill — and every pose is chosen so his whole bounding box
 * (characterFit CHICK_EXTENT) sits inside the scene's rounded clip.
 *
 *   open   a 64 x 188 dark shell flush to the screen edge (rounded on the inner
 *          side only), 4 px padding, holding the 56 x 180 scene: a window with
 *          the real sky at the top, a shelf with the notepad and word jar, a
 *          shelf with the typewriter, the floor, and a ladder on the INNER side
 *          (the side that faces the middle of the screen). The right dock is
 *          the left dock mirrored.
 *   rest   a 30 x 40 tab tucked into the edge, a 22 x 32 sky window holding
 *          the whole chick (half size: 1 device px per art px on a 2x display).
 *
 * Yappy stays on the floor for quick takes, climbs the ladder to the notepad
 * shelf for notes, and works at the typewriter shelf for desk tiers and up.
 *
 * Pure: no DOM, no settings. YappyPill draws with `drawTallRoom` /
 * `drawTabChick` and places with `vdockFrame`; `vdock.test.ts` walks every
 * state x left/right x display scale with the same functions.
 */
import { drawChick, H, type ChickPose, type PixelCtx } from "../sprites/art";
import { CHICK_EXTENT, type FitBox } from "./characterFit";
import { backingScale } from "./pixelGrid";
import { mixHex, type SkyLook } from "./sky";

export type SideDock = "left" | "right";
/** The tiers the pill's prop ladder draws (YappyPill `Prop`). */
export type VProp = "none" | "pad" | "desk";
/** Where Yappy stands in the tall room. */
export type VTier = "floor" | "notepad" | "typewriter";

/** The shell is never wider than this on a side dock (the item's promise). */
export const VDOCK_MAX_W = 64;
/** Shell padding around the scene / the tab's window. */
export const VDOCK_PAD = 4;
/** The scene, in CSS px AND art px (one CSS px per art px). */
export const VDOCK_SCENE = { w: 56, h: 180 } as const;
/** The open shell: the scene plus its padding. */
export const VDOCK_OPEN = { w: VDOCK_SCENE.w + 2 * VDOCK_PAD, h: VDOCK_SCENE.h + 2 * VDOCK_PAD } as const;
/** The resting tab and its sky window. */
export const VDOCK_TAB = { w: 30, h: 40 } as const;
export const VDOCK_TAB_WINDOW = { w: VDOCK_TAB.w - 2 * VDOCK_PAD, h: VDOCK_TAB.h - 2 * VDOCK_PAD } as const;
/** Corner radii (CSS px): the shell's inner corners, the scene, the tab, its window. */
export const VDOCK_RADIUS = { open: 16, scene: 10, tab: 10, tabWindow: 8 } as const;
/** Above this open value the shell shows the room; below it, the tab. */
export const VDOCK_ROOM_AT = 0.5;

/** The tab's chick buffer: exactly the chick's bounding box (art px). */
export const TAB_ART = { w: CHICK_EXTENT.left + CHICK_EXTENT.right, h: CHICK_EXTENT.up + CHICK_EXTENT.down } as const;
const TAB_CHICK = { cx: CHICK_EXTENT.left, feetY: CHICK_EXTENT.up } as const;

/**
 * The room's parts in LEFT-dock art px (x grows away from the screen edge;
 * the ladder is on the inner side). `roomParts("right")` is the mirror.
 */
export const VDOCK_ROOM = {
  window: { x: 7, y: 8, w: 42, h: 36 },
  sill: { x: 5, y: 44, w: 46, h: 2 },
  notepadShelf: { x: 4, y: 76, w: 48, h: 3 },
  notepad: { x: 8, y: 62, w: 13, h: 14 },
  wordJar: { x: 25, y: 63, w: 10, h: 13 },
  typewriterShelf: { x: 4, y: 118, w: 48, h: 3 },
  typewriter: { x: 8, y: 103, w: 22, h: 15 },
  floor: { x: 0, y: 160, w: 56, h: 20 },
  plant: { x: 4, y: 139, w: 11, h: 21 },
  ladder: { x: 42, y: 48, w: 11, h: 112 },
} as const satisfies Record<string, FitBox>;
export type RoomPart = keyof typeof VDOCK_ROOM;

/** Where his feet go (left-dock art px). Every pose keeps CHICK_EXTENT inside the scene. */
export const VDOCK_POSE: Record<VTier, { cx: number; feetY: number }> = {
  floor: { cx: 26, feetY: VDOCK_ROOM.floor.y },
  notepad: { cx: 34, feetY: VDOCK_ROOM.notepadShelf.y },
  typewriter: { cx: 34, feetY: VDOCK_ROOM.typewriterShelf.y },
};
/** His x while on the ladder: as far in as his box allows (scene edge - CHICK_EXTENT.right). */
export const LADDER_CX = VDOCK_SCENE.w - CHICK_EXTENT.right;

const clamp01 = (v: number) => (Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 0);
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

/** The side dock, or null for the bottom / top dock (anything unknown is not a side). */
export function sideDock(d: string | null | undefined): SideDock | null {
  return d === "left" || d === "right" ? d : null;
}

/** Quick takes stay on the floor, notes climb to the notepad, desk tiers work at the typewriter. */
export function vTierFor(prop: VProp): VTier {
  return prop === "pad" ? "notepad" : prop === "desk" ? "typewriter" : "floor";
}

/**
 * His feet for a prop and its 0..1 transition: walk to the ladder, climb it,
 * step onto the shelf (each a third). Whole art px only.
 */
export function vChickAt(prop: VProp, propIn: number): { cx: number; feetY: number } {
  const floor = VDOCK_POSE.floor;
  const tier = vTierFor(prop);
  const t = clamp01(propIn);
  if (tier === "floor" || t === 0) return { ...floor };
  const goal = VDOCK_POSE[tier];
  if (t < 1 / 3) return { cx: Math.round(lerp(floor.cx, LADDER_CX, t * 3)), feetY: floor.feetY };
  if (t < 2 / 3) return { cx: LADDER_CX, feetY: Math.round(lerp(floor.feetY, goal.feetY, (t - 1 / 3) * 3)) };
  return { cx: Math.round(lerp(LADDER_CX, goal.cx, (t - 2 / 3) * 3)), feetY: goal.feetY };
}

/** The chick's bounding box for feet at (cx, feetY), art px. */
export function vChickBox(cx: number, feetY: number): FitBox {
  return { x: cx - CHICK_EXTENT.left, y: feetY - CHICK_EXTENT.up, w: TAB_ART.w, h: TAB_ART.h };
}

/** A left-dock art box as the right dock draws it (the room mirrored). */
export function mirrorBox(b: FitBox, dock: SideDock): FitBox {
  return dock === "left" ? { ...b } : { ...b, x: VDOCK_SCENE.w - b.x - b.w };
}

/** Every room part, in this dock's art px. */
export function roomParts(dock: SideDock): Record<RoomPart, FitBox> {
  const out = {} as Record<RoomPart, FitBox>;
  for (const k of Object.keys(VDOCK_ROOM) as RoomPart[]) out[k] = mirrorBox(VDOCK_ROOM[k], dock);
  return out;
}

/* ── geometry ───────────────────────────────────────────────────────────── */

/** Is `b` inside the rounded rect `r` (corner radius `rad`)? Checks the four corners of `b`. */
export function roundRectContains(r: FitBox, rad: number, b: FitBox, eps = 1e-6): boolean {
  if (b.x < r.x - eps || b.y < r.y - eps || b.x + b.w > r.x + r.w + eps || b.y + b.h > r.y + r.h + eps) return false;
  const k = Math.max(0, Math.min(rad, r.w / 2, r.h / 2));
  const corners: Array<[number, number]> = [[b.x, b.y], [b.x + b.w, b.y], [b.x, b.y + b.h], [b.x + b.w, b.y + b.h]];
  for (const [px, py] of corners) {
    const cx = Math.min(Math.max(px, r.x + k), r.x + r.w - k);
    const cy = Math.min(Math.max(py, r.y + k), r.y + r.h - k);
    if (Math.hypot(px - cx, py - cy) > k + eps) return false;
  }
  return true;
}

/**
 * The shell in panel CSS px: `openV` 0 = the resting tab, 1 = the open room,
 * flush to the dock edge of the layout's character box and centred on it
 * vertically (pill/state.ts sizes that box to the shell).
 */
export function vShellRect(dock: SideDock, openV: number, char: FitBox): FitBox {
  const o = clamp01(openV);
  const w = lerp(VDOCK_TAB.w, VDOCK_OPEN.w, o), h = lerp(VDOCK_TAB.h, VDOCK_OPEN.h, o);
  const x = dock === "left" ? char.x : char.x + char.w - w;
  return { x, y: char.y + char.h / 2 - h / 2, w, h };
}

/** The scene / tab window inside a shell: the shell inset by its padding. */
export function vInnerRect(shell: FitBox): FitBox {
  return { x: shell.x + VDOCK_PAD, y: shell.y + VDOCK_PAD, w: shell.w - 2 * VDOCK_PAD, h: shell.h - 2 * VDOCK_PAD };
}

/** Device px per art px for the tab's chick: the largest whole number that fits, else 1/n. */
export function tabCell(inner: FitBox): number {
  const raw = Math.min(inner.w / TAB_ART.w, inner.h / TAB_ART.h);
  if (raw >= 1) return Math.floor(raw);
  return raw > 0 ? 1 / Math.ceil(1 / raw) : 1;
}

export interface VFrame {
  dock: SideDock;
  mode: "room" | "tab";
  /** CSS px, panel coords. */
  shell: FitBox;
  inner: FitBox;
  /** Device px: the inner window, where the art lands, and the chick's box. */
  innerD: FitBox;
  art: FitBox;
  chick: FitBox;
  /** Device px per art px. */
  cell: number;
  /** Corner radius of the inner window, device px. */
  radiusD: number;
  /** The chick sits whole inside the inner window's rounded clip. */
  fits: boolean;
}

/**
 * Everything YappyPill needs to draw one side-dock frame, and everything the
 * test asserts: the shell, the scene, where the art is blitted (device px,
 * whole numbers) and where his whole bounding box lands.
 */
export function vdockFrame(dock: SideDock, openV: number, prop: VProp, propIn: number, dprRaw: number, char: FitBox): VFrame {
  const dpr = backingScale(dprRaw);
  const shell = vShellRect(dock, openV, char);
  const inner = vInnerRect(shell);
  const innerD = {
    x: Math.round(inner.x * dpr), y: Math.round(inner.y * dpr),
    w: Math.round(inner.w * dpr), h: Math.round(inner.h * dpr),
  };
  const mode: VFrame["mode"] = clamp01(openV) >= VDOCK_ROOM_AT ? "room" : "tab";
  let art: FitBox, chick: FitBox, cell: number, radiusD: number;
  if (mode === "room") {
    cell = Math.max(1, Math.floor(dpr + 1e-9));
    const w = VDOCK_SCENE.w * cell, h = VDOCK_SCENE.h * cell;
    // edge-aligned (the scene hugs the dock edge as the shell grows), centred vertically
    art = { x: dock === "left" ? innerD.x : innerD.x + innerD.w - w, y: innerD.y + Math.round((innerD.h - h) / 2), w, h };
    const at = vChickAt(prop, propIn);
    const b = mirrorBox(vChickBox(at.cx, at.feetY), dock);
    chick = { x: art.x + b.x * cell, y: art.y + b.y * cell, w: b.w * cell, h: b.h * cell };
    radiusD = VDOCK_RADIUS.scene * dpr;
  } else {
    cell = tabCell(innerD);
    const w = Math.round(TAB_ART.w * cell), h = Math.round(TAB_ART.h * cell);
    art = { x: innerD.x + Math.round((innerD.w - w) / 2), y: innerD.y + Math.round((innerD.h - h) / 2), w, h };
    chick = { ...art };
    radiusD = VDOCK_RADIUS.tabWindow * dpr;
  }
  return { dock, mode, shell, inner, innerD, art, chick, cell, radiusD, fits: roundRectContains(innerD, radiusD, chick) };
}

/** One structured line when a side-dock frame would crop him (never expected; vdock.test pins it). */
export function vdockCropWarning(f: VFrame, dpr: number): string {
  return `pill vdock_cropped dock=${f.dock} mode=${f.mode} shellW=${Math.round(f.shell.w)} shellH=${Math.round(f.shell.h)} dpr=${dpr}`;
}

/* ── the art ────────────────────────────────────────────────────────────── */

const SKY_FALLBACK = { top: "#8fd6ff", bot: "#c6ecff" } as const;
const WOOD = { frame: "#5f3c22", sill: "#a5704a", plank: "#8a5a36", plankHi: "#b8763f", rail: "#6b4428" } as const;

export interface RoomDraw {
  /** The prop tier and its transition (YappyPill wantProp / propV). */
  prop: VProp;
  propIn: number;
  /** The notepad's handwriting, 0..1 (YappyPill padInk). */
  ink: number;
  /** Seconds since mount, for the typewriter's lit key. */
  elapsed: number;
}

/**
 * Paint the 56 x 180 room (LEFT-dock orientation; the right dock blits it
 * mirrored) with Yappy in it. `look` is the pill's real sky (sky.ts), so the
 * window shows the same time of day as the horizontal pill.
 */
export function drawTallRoom(o: PixelCtx, look: SkyLook | null, pose: ChickPose, d: RoomDraw): void {
  const h = H(o);
  const R = VDOCK_ROOM;
  const W = VDOCK_SCENE.w;
  // wall, darker towards the ceiling
  h.r(0, 0, W, VDOCK_SCENE.h, "#eccf92", 1);
  for (let i = 0; i < 11; i++) h.r(0, i, W, 1, "#966432", 0.2 * (1 - i / 11));
  o.globalAlpha = 1;
  // window with the real sky
  const win = R.window;
  h.r(win.x, win.y, win.w, win.h, WOOD.frame);
  const gx = win.x + 3, gy = win.y + 3, gw = win.w - 6, gh = win.h - 6;
  const top = look ? look.top : SKY_FALLBACK.top, bot = look ? look.bot : SKY_FALLBACK.bot;
  for (let y = 0; y < gh; y++) h.r(gx, gy + y, gw, 1, mixHex(top, bot, y / (gh - 1)));
  const time = look ? look.time : "day";
  if (time === "night") {
    h.ell(gx + 29, gy + 6, 3, 3, "#f6f0cf");
    const stars: Array<[number, number]> = [[gx + 4, gy + 3], [gx + 12, gy + 8], [gx + 22, gy + 2], [gx + 33, gy + 14]];
    for (const [sx, sy] of stars) h.r(sx, sy, 1, 1, "#fffbe0");
  } else {
    const sunY = time === "day" ? gy + 7 : gy + gh - 7;
    h.ell(gx + 7, sunY, 4, 4, "#e8a41f");
    h.ell(gx + 7, sunY, 3, 3, "#ffd23f");
  }
  h.r(win.x + win.w / 2 - 1, gy, 2, gh, WOOD.frame);
  h.r(gx, gy + Math.floor(gh / 2) - 1, gw, 2, WOOD.frame);
  h.r(R.sill.x, R.sill.y, R.sill.w, R.sill.h, WOOD.sill);
  // top shelf: notepad + word jar
  const plank = (b: FitBox) => { h.r(b.x, b.y, b.w, b.h, WOOD.plank); h.r(b.x, b.y, b.w, 1, WOOD.plankHi); };
  plank(R.notepadShelf);
  const np = R.notepad;
  h.r(np.x, np.y, np.w, np.h, "#8a7a5a");
  h.r(np.x + 1, np.y + 1, np.w - 2, np.h - 2, "#fffdf5");
  h.r(np.x + 5, np.y - 1, 3, 2, "#9aa3ad");
  const noting = d.prop === "pad" && clamp01(d.propIn) >= 1;
  const lines = noting ? Math.max(1, Math.ceil(clamp01(d.ink) * 4)) : 3;
  for (let k = 0; k < lines; k++) h.r(np.x + 3, np.y + 4 + k * 2, k === lines - 1 && noting ? 4 : 7, 1, "#4a4f5c");
  const jar = R.wordJar;
  h.r(jar.x, jar.y, jar.w, 1, WOOD.plank);
  h.r(jar.x, jar.y + 1, jar.w, jar.h - 1, "#9fc8d0", 0.7);
  o.globalAlpha = 1;
  h.r(jar.x + 2, jar.y + 5, 4, 3, "#fffdf5");
  h.r(jar.x + 4, jar.y + 8, 4, 3, "#fff2b0");
  // middle shelf: typewriter
  plank(R.typewriterShelf);
  const tw = R.typewriter;
  h.r(tw.x + 4, tw.y, 14, 6, "#fffdf5");
  h.r(tw.x + 6, tw.y + 2, 10, 1, "#4a4f5c");
  h.r(tw.x, tw.y + 5, tw.w, tw.h - 5, "#2a2d35");
  h.r(tw.x + 2, tw.y + 6, tw.w - 4, 6, "#4a5060");
  const typing = d.prop === "desk" && clamp01(d.propIn) >= 1;
  const lit = typing ? Math.floor(Math.max(0, d.elapsed) * 8) % 5 : -1;
  for (let q = 0; q < 5; q++) h.r(tw.x + 3 + q * 3, tw.y + 11, 2, 1, q === lit ? "#ffd23f" : "#e8e6e1");
  // floor + plant
  const fl = R.floor;
  h.r(fl.x, fl.y, fl.w, fl.h, "#a5673f");
  h.r(fl.x, fl.y, fl.w, 1, "#bd8151");
  const pl = R.plant;
  h.r(pl.x + 1, pl.y + 10, 9, 11, "#c8443a");
  h.r(pl.x + 1, pl.y + 10, 9, 1, "#e0574a");
  h.ell(pl.x + 5, pl.y + 5, 5, 5, "#5f9f4e");
  h.ell(pl.x + 4, pl.y + 4, 3, 3, "#94dc7c");
  // ladder on the inner side
  const ld = R.ladder;
  h.r(ld.x, ld.y, 1, ld.h, WOOD.rail);
  h.r(ld.x + ld.w - 1, ld.y, 1, ld.h, WOOD.rail);
  for (let y = ld.y + 6; y < ld.y + ld.h; y += 9) h.r(ld.x, y, ld.w, 1, WOOD.plank);
  // Yappy: the floor, the ladder, or a shelf
  const at = vChickAt(d.prop, d.propIn);
  drawChick(o, at.cx, at.feetY, pose);
  o.globalAlpha = 1;
}

/** Paint the tab's chick buffer (TAB_ART): his whole box, nothing else (transparent around him). */
export function drawTabChick(o: PixelCtx, pose: ChickPose): void {
  drawChick(o, TAB_CHICK.cx, TAB_CHICK.feetY, pose);
  o.globalAlpha = 1;
}
