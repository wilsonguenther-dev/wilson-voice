/**
 * yap-1107-parity-companions — a visiting friend stands INSIDE the pill window.
 *
 * The approved Companions page (companions.html s09, "Yappy visits for your
 * 100th take", line 289-297) draws the visitor in the open pill's sky window,
 * beside whoever sits in the pill: `Y.drawChick` translated to `(cx + 26, feet)`
 * on the occupant's feet line, while the segment reads "✓ 100th take" and the
 * bubble says "that's a hundred! — Yappy". Before this item the app painted the
 * visitor in a separate 28 px box pinned to the window corner (.friend-visit).
 *
 * Who visits, when, and the quiet rules stay `friendVisits.ts`'s (FriendVisit
 * decides). This module is the hand-off between the two components that share
 * the pill window:
 *   - FriendVisit publishes the visitor on screen (`publishPillVisitor`);
 *   - YappyPill paints it into its window world beside the occupant
 *     (`visitorStamp` + `paintPillVisitor`) whenever the whole stamp is inside
 *     what the window's camera shows, and says so (`setVisitorInWindow`);
 *   - FriendVisit keeps its old corner box only while the window cannot hold
 *     the visitor whole (the 64 x 20 resting window, which the page never
 *     draws with a visitor), so a visitor is never cropped and never drawn twice.
 * Pure and canvas-free: painting goes through PixelCtx, tested on a Raster.
 */
import { drawChick, type ChickPose, type PixelCtx } from "../sprites/art";
import { drawFriendFrame, friendManifest } from "../sprites/friends";
import type { VisitorId } from "./friendVisits";

/** The visitor's centre, in art px right of the occupant's cx (companions.html line 289, `cx + 26`). */
export const VISITOR_DX = 26;

/**
 * The visiting Yappy's pose: the page's `Y.base({ mood: 1 })` — the rest pose,
 * smiling, eyes open, no props.
 */
export const VISITOR_YAPPY_POSE: ChickPose = {
  hop: 0, sx: 0, sy: 0, blink: 0, mood: 1, beakF: 0, sway: 0, flap: 0,
  look: "fwd", glasses: false, pencil: false, droop: false,
};

/**
 * Yappy's painted box relative to (cx, feetY): tail to cx + 14, wing to
 * cx - 13, leaf sprout to feetY - 26, toes and ground shadow to feetY + 3
 * (sprites/art.ts CHICK_DETAIL_BOX: body centre 11 rows above the feet).
 * One spare px each side, so the box is never tighter than the art.
 */
const YAPPY_EXTENT = { left: 15, right: 15, up: 27, down: 4 } as const;

/** What a visitor on screen is, as FriendVisit publishes it. */
export interface PillVisitor {
  friend: VisitorId;
  /** The event it visits for (friendVisits.ts FriendEvent), null for a pinned matrix visitor. */
  event: string | null;
  /** The bubble line the page shows with it (milestoneLine), if any. */
  line: string | null;
  /** The segment text the page shows with it ("100th take"), if any. */
  segment: string | null;
}

export interface ArtRect { x: number; y: number; w: number; h: number }

/**
 * Where the visitor stands in the pill's art buffer: centred VISITOR_DX right
 * of the occupant's `cx`, its feet on the occupant's feet line. A friend is
 * stamped whole at 1:1 the way the page stamps Hoot (`cx - w/2, feet - (h - 2)`),
 * never scaled; Yappy is drawn by drawChick at (vx, feet). Null for an id
 * with no art.
 */
export function visitorStamp(id: VisitorId, cx: number, feet: number): (ArtRect & { vx: number; ox: number; oy: number }) | null {
  const vx = Math.round(cx + VISITOR_DX);
  if (id === "yappy") {
    const e = YAPPY_EXTENT;
    return { vx, ox: vx, oy: feet, x: vx - e.left, y: feet - e.up, w: e.left + e.right, h: e.up + e.down };
  }
  const m = friendManifest(id);
  if (!m) return null;
  const x = vx - Math.floor(m.w / 2), y = feet - (m.h - 2);
  return { vx, ox: x, oy: y, x, y, w: m.w, h: m.h };
}

/** True when `r` lies wholly inside `view` (the camera's art-px source rect). */
export function rectInside(r: ArtRect, view: ArtRect): boolean {
  return r.x >= view.x && r.y >= view.y && r.x + r.w <= view.x + view.w && r.y + r.h <= view.y + view.h;
}

/**
 * What the window's camera actually SHOWS, in art px: the blit's clip (the
 * window interior on the device grid) mapped back through dst -> src, with
 * `cornerDevPx` taken off the left and right ends (the window's rounded
 * corners). `blit.src` alone is wider: it also carries the scene that fills
 * the margin outside the clip.
 */
export function cameraView(
  blit: { cell: number; clip: ArtRect; src: ArtRect; dst: ArtRect },
  cornerDevPx = 0,
): ArtRect {
  const cell = Math.max(1, blit.cell);
  const inset = Math.max(0, cornerDevPx);
  const x = blit.src.x + (blit.clip.x + inset - blit.dst.x) / cell;
  const y = blit.src.y + (blit.clip.y - blit.dst.y) / cell;
  return { x, y, w: Math.max(0, blit.clip.w - 2 * inset) / cell, h: blit.clip.h / cell };
}

/** A PixelCtx that draws into `o` shifted by (dx, dy) — friends draw from their own (0, 0). */
function shifted(o: PixelCtx, dx: number, dy: number): PixelCtx {
  return {
    get fillStyle() { return o.fillStyle; },
    set fillStyle(v) { o.fillStyle = v; },
    get globalAlpha() { return o.globalAlpha; },
    set globalAlpha(v) { o.globalAlpha = v; },
    fillRect: (x: number, y: number, w: number, h: number) => o.fillRect(x + dx, y + dy, w, h),
  };
}

/**
 * Paint the visitor beside the occupant (cx, feet) at 1:1 into the art buffer.
 * `blink` picks the blink frame (a friend's `blink` kind, Yappy's eyes shut).
 * Returns false when there is nothing to paint (unknown id).
 */
export function paintPillVisitor(o: PixelCtx, id: VisitorId, cx: number, feet: number, blink = false): boolean {
  const s = visitorStamp(id, cx, feet);
  if (!s) return false;
  if (id === "yappy") {
    drawChick(o, s.ox, s.oy, blink ? { ...VISITOR_YAPPY_POSE, blink: 1 } : VISITOR_YAPPY_POSE);
    return true;
  }
  return drawFriendFrame(shifted(o, s.ox, s.oy), id, blink ? "blink" : "idle");
}

/**
 * The segment the page shows while Yappy visits for a milestone: "✓ 100th
 * take" (companions.html line 297). Only the take-count milestone is drawn on
 * the page; every other visit keeps the phase's own segment.
 */
export function visitorSegment(event: string | null, takesSoFar: number): string | null {
  if (event !== "milestone") return null;
  return takesSoFar >= 1000 ? "1000th take" : "100th take";
}

/** One log line when the visitor moves between the window and the corner box. */
export function placementLogLine(friend: VisitorId | null, inWindow: boolean): string {
  return `pill friend_visit placed=${inWindow ? "window" : "corner"} friend=${friend ?? "none"}`;
}

/* ── the hand-off (module state; one pill per webview) ─────────────────── */

type Listener<T> = (v: T) => void;
let visitor: PillVisitor | null = null;
let inWindow = false;
const visitorListeners = new Set<Listener<PillVisitor | null>>();
const windowListeners = new Set<Listener<boolean>>();

function sameVisitor(a: PillVisitor | null, b: PillVisitor | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return a.friend === b.friend && a.event === b.event && a.line === b.line && a.segment === b.segment;
}

/** FriendVisit: the visitor now on screen (null when none). Listeners hear changes only. */
export function publishPillVisitor(v: PillVisitor | null): void {
  if (sameVisitor(visitor, v)) return;
  visitor = v;
  if (!v) setVisitorInWindow(false);
  visitorListeners.forEach((fn) => fn(v));
}
export function currentPillVisitor(): PillVisitor | null { return visitor; }
export function subscribePillVisitor(fn: Listener<PillVisitor | null>): () => void {
  visitorListeners.add(fn);
  return () => { visitorListeners.delete(fn); };
}

/** YappyPill: whether its window is painting the visitor this frame. */
export function setVisitorInWindow(on: boolean): void {
  if (inWindow === on) return;
  inWindow = on;
  console.info(placementLogLine(visitor ? visitor.friend : null, on));
  windowListeners.forEach((fn) => fn(on));
}
export function visitorInWindow(): boolean { return inWindow; }
export function subscribeVisitorInWindow(fn: Listener<boolean>): () => void {
  windowListeners.add(fn);
  return () => { windowListeners.delete(fn); };
}

/** Tests only: back to no visitor, nothing listening. */
export function resetPillVisitorForTests(): void {
  visitor = null;
  inWindow = false;
  visitorListeners.clear();
  windowListeners.clear();
}
