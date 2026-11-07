/**
 * yap-1106-main-character-switch — who is drawn IN the one pill.
 *
 * Batch 10 bounced this item because a friend in the pill fell back to the
 * retired 300 x 140 capsule (no sky, no one-pill sizing, overflow at the right
 * dock). Exactly one pill surface exists (Decisions, 2026-11-06), so every
 * character sits in the SAME stage Yappy does (pill/YappyPill.tsx): same sky,
 * same camera, same top-center meeting dock, same timer inside the pill. Only
 * the creature painted at the face spot changes, and that is this module.
 *
 * A friend is painted from its approved HD manifest (sprites/friends.ts), one
 * frame per draw, at 1 art px = 1 scene px, so the stage's whole-number camera
 * keeps it as crisp as Yappy. It stands inside Yappy's bounding box
 * (`chickBox`), so the camera that keeps Yappy whole keeps every friend whole.
 *
 * Pure: no React, no settings, no window. Painting goes through `PixelCtx`
 * (fillRect + fillStyle + globalAlpha), so tests can count the pixels.
 */
import { drawFriendFrame, friendManifest, type FrameKind, type FriendManifest } from "../sprites/friends";
import type { PixelCtx } from "../sprites/art";
import type { LivePhase } from "./live";
import { FRIEND_POSES, PILL_FRIENDS, type PillFriendId } from "./characters/friends";
import { PILL_SCENE, chickBox, type FitBox } from "./characterFit";

/** Is `id` a friend who can sit in the pill (Hoot, Tidy, Gulp, Shelly)? Yappy and unknown ids are not. */
export function isPillFriend(id: string | null | undefined): id is PillFriendId {
  return typeof id === "string" && (PILL_FRIENDS as readonly string[]).includes(id);
}

/** The scene row a friend's lowest art row sits on: the chick's shadow band (feet + 4). */
export const OCCUPANT_BOTTOM = PILL_SCENE.groundY + 4;

/** Which manifest frame the friend wears for a phase at a moment. */
export interface OccupantFrame {
  kind: FrameKind;
  index: 0 | 1;
}

/**
 * The frame for `phase`: the pose table every pill state has (FRIEND_POSES, all
 * 17). A working pose alternates its two action frames on the manifest's
 * beat unless motion is reduced; a resting pose blinks when the stage blinks
 * (never for a friend whose eyes rest shut).
 */
export function occupantFrame(
  m: Pick<FriendManifest, "eyesClosedAtRest" | "action">,
  phase: LivePhase,
  blink: number,
  tMs: number,
  reduceMotion: boolean,
): OccupantFrame {
  const pose = FRIEND_POSES[phase] ?? FRIEND_POSES.idle;
  if (pose.frame === "action") {
    const beat = Math.max(1, m.action.frameMs || 1);
    const index: 0 | 1 = reduceMotion || pose.motion !== "work" || !Number.isFinite(tMs) ? 0 : Math.floor(tMs / beat) % 2 === 1 ? 1 : 0;
    return { kind: "action", index };
  }
  if (pose.frame === "idle" && blink > 0.5 && !m.eyesClosedAtRest && !reduceMotion) return { kind: "blink", index: 0 };
  return { kind: pose.frame, index: 0 };
}

/**
 * Where a friend's art box goes in the scene: centred in Yappy's box, its last
 * row on `OCCUPANT_BOTTOM`, lifted by the stage's hop only as far as the box
 * allows. Whole scene px, always inside `chickBox()`.
 */
export function occupantPlacement(m: Pick<FriendManifest, "w" | "h">, hop = 0): FitBox {
  const b = chickBox();
  const w = Math.min(m.w, b.w), h = Math.min(m.h, b.h);
  const x = b.x + Math.floor((b.w - w) / 2);
  const room = Math.max(0, OCCUPANT_BOTTOM - h - b.y);
  const lift = Math.min(room, Math.max(0, Math.round(Number.isFinite(hop) ? hop : 0)));
  const y = Math.max(b.y, OCCUPANT_BOTTOM - h - lift);
  return { x, y, w, h };
}

/** A PixelCtx that paints into `o` shifted by (dx, dy) scene px. */
export function offsetCtx(o: PixelCtx, dx: number, dy: number): PixelCtx {
  return {
    get fillStyle() { return o.fillStyle; },
    set fillStyle(v) { o.fillStyle = v; },
    get globalAlpha() { return o.globalAlpha; },
    set globalAlpha(v) { o.globalAlpha = v; },
    fillRect(x: number, y: number, w: number, h: number) { o.fillRect(x + dx, y + dy, w, h); },
  };
}

let warned = new Set<string>();

/**
 * Paint the pill's occupant for this frame. Returns false (and paints nothing)
 * when `id` is not a pill friend, so the stage draws Yappy as before.
 */
export function drawOccupant(
  o: PixelCtx,
  id: string | null | undefined,
  phase: LivePhase,
  pose: { hop: number; blink: number },
  tMs: number,
  reduceMotion: boolean,
): boolean {
  if (!isPillFriend(id)) return false;
  const m = friendManifest(id);
  if (!m) {
    if (!warned.has(id)) {
      warned.add(id);
      console.warn(`pill occupant_missing_art id=${id}`);
    }
    return false;
  }
  const f = occupantFrame(m, phase, pose.blink, tMs, reduceMotion);
  const at = occupantPlacement(m, pose.hop);
  return drawFriendFrame(offsetCtx(o, at.x, at.y), id, f.kind, f.index);
}

/** Test seam: forget which ids already logged a missing-art warning. */
export function resetOccupantWarnings(): void {
  warned = new Set<string>();
}
