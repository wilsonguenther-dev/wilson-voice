/**
 * YV65 — drag the pill to dock it (Wispr-style direct manipulation).
 *
 * The float window is an NSPanel that is click-THROUGH everywhere except the
 * visible capsule: the pill reports that capsule's rect here, the backend turns
 * the cursor on only while the pointer is inside it (`pill_set_hitbox`), and
 * these pointer handlers then move the panel live and snap it on release.
 *
 * The gesture is deliberately measured in SCREEN coordinates: the window slides
 * out from under the pointer while you drag, so window-relative deltas would
 * fight themselves. `screenX/screenY` keep tracking the real cursor, and the
 * backend re-derives every position from the origin latched at pointer-down —
 * a dropped move can never accumulate drift.
 *
 * On release the backend snaps to the nearest dock and PERSISTS it into
 * `pill_position`, the same setting the Settings picker writes, so a drag
 * survives a restart and the picker follows.
 */
import { useMemo, useRef } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";

/** Travel (logical points) past which a press is a DRAG, not a click. */
const DRAG_SLOP = 3;

export interface PillDrag {
  /** Spread onto the element the user grabs (the capsule). */
  handlers: {
    onPointerDown: (e: ReactPointerEvent<HTMLElement>) => void;
    onPointerMove: (e: ReactPointerEvent<HTMLElement>) => void;
    onPointerUp: (e: ReactPointerEvent<HTMLElement>) => void;
    onPointerCancel: (e: ReactPointerEvent<HTMLElement>) => void;
  };
  /**
   * True once the press passed the slop, and still true for the `click` that
   * follows the release — so a drag never fires the pill's click behaviour
   * (dictation). Cleared by the next pointer-down.
   */
  dragged: () => boolean;
}

/**
 * Y5-D — the soft body listens to this gesture. The drag owns the only truthful
 * account of what the pointer did (press, plain release, release-after-a-drag =
 * a DOCK), so the physics is driven from here rather than from a second set of
 * pointer handlers that would have to re-derive the slop.
 *
 * Held in a ref and read at call time: the callbacks close over React state in
 * the caller, and the handlers below are memoised for the life of the mount.
 */
export interface PillGestureFeedback {
  press?(): void;
  release?(): void;
  dock?(): void;
}

export function usePillDrag(feedback: PillGestureFeedback = {}): PillDrag {
  const st = useRef({ down: false, moved: false, x: 0, y: 0 });
  const fb = useRef(feedback);
  fb.current = feedback;
  return useMemo<PillDrag>(() => {
    const end = (e: ReactPointerEvent<HTMLElement>) => {
      const s = st.current;
      if (!s.down) return;
      s.down = false;
      try {
        e.currentTarget.releasePointerCapture(e.pointerId);
      } catch {
        /* capture was already lost (window teardown) — the drag still ends */
      }
      invoke("pill_drag_end", { x: e.screenX, y: e.screenY, snap: s.moved }).catch(() => {});
      // A drag that moved ends by SNAPPING into a dock, so it lands; a press
      // that never moved is a plain click, so it just rebounds.
      if (s.moved) fb.current.dock?.();
      else fb.current.release?.();
    };
    return {
      handlers: {
        onPointerDown: (e) => {
          if (e.button !== 0) return; // left button only; right/middle are not a grab
          st.current = { down: true, moved: false, x: e.screenX, y: e.screenY };
          try {
            // Capture so the gesture keeps reporting once the panel slides out
            // from under the pointer.
            e.currentTarget.setPointerCapture(e.pointerId);
          } catch {
            /* non-fatal: the drag just ends if the pointer leaves the capsule */
          }
          fb.current.press?.();
          invoke("pill_drag_start").catch(() => {});
        },
        onPointerMove: (e) => {
          const s = st.current;
          if (!s.down) return;
          const dx = e.screenX - s.x;
          const dy = e.screenY - s.y;
          if (!s.moved && Math.hypot(dx, dy) < DRAG_SLOP) return;
          s.moved = true;
          invoke("pill_drag_move", { dx, dy }).catch(() => {});
        },
        onPointerUp: end,
        onPointerCancel: end,
      },
      dragged: () => st.current.moved,
    };
  }, []);
}

/**
 * Tell the backend where the visible capsule sits inside the float window, in
 * logical points. Deduped to whole points: the capsule animates open/closed, and
 * only a real geometry change is worth an IPC hop.
 */
let lastHitbox = "";
function sendHitbox(x: number, y: number, w: number, h: number): void {
  const key = `${Math.round(x)},${Math.round(y)},${Math.round(w)},${Math.round(h)}`;
  if (key === lastHitbox) return;
  lastHitbox = key;
  invoke("pill_set_hitbox", { x, y, w, h }).catch(() => {});
}

/** The capsule's latest rect, kept while a held rect overrides it. */
let capsuleRect: [number, number, number, number] | null = null;
let held = false;
/**
 * yap24-NT7 fix — a control that floats OUTSIDE the capsule's own rect (Yappy's
 * meeting badge: the canvas capsule sits mid-window, the badge is pinned to the
 * window's bottom edge) must still receive clicks through the click-through
 * panel. The backend takes ONE rect, so the capsule and this extra rect are
 * sent as their bounding union. `null` = no extra; the capsule alone.
 */
let extraRect: [number, number, number, number] | null = null;

type Rect4 = [number, number, number, number];

/** Bounding union of two x/y/w/h rects; an empty (w or h <= 0) side is ignored. */
export function unionHitbox(a: Rect4 | null, b: Rect4 | null): Rect4 | null {
  const live = (r: Rect4 | null): r is Rect4 => !!r && r[2] > 0 && r[3] > 0;
  if (!live(a)) return live(b) ? b : a;
  if (!live(b)) return a;
  const x = Math.min(a[0], b[0]);
  const y = Math.min(a[1], b[1]);
  const r = Math.max(a[0] + a[2], b[0] + b[2]);
  const btm = Math.max(a[1] + a[3], b[1] + b[3]);
  return [x, y, r - x, btm - y];
}

function sendCapsule(): void {
  const r = unionHitbox(capsuleRect, extraRect);
  if (r) sendHitbox(...r);
}

export function reportPillHitbox(x: number, y: number, w: number, h: number): void {
  capsuleRect = [x, y, w, h];
  if (held) return;
  sendCapsule();
}

/**
 * yap24-NT7 fix — add (or, with `null`, remove) a rect the hitbox must cover IN
 * ADDITION to the capsule. Unlike `holdPillHitbox` this never replaces the
 * capsule: the pill stays grabbable while the extra control is clickable.
 */
export function extendPillHitbox(rect: { x: number; y: number; w: number; h: number } | null): void {
  extraRect = rect ? [rect.x, rect.y, rect.w, rect.h] : null;
  if (held) return;
  sendCapsule();
}

/**
 * yap-p0-insertion-bridge-fallback — the Copy card takes the hitbox while it is
 * up (so its buttons receive clicks through the click-through panel); `null`
 * hands it back to the capsule's last reported rect.
 */
export function holdPillHitbox(rect: { x: number; y: number; w: number; h: number } | null): void {
  if (rect) {
    held = true;
    sendHitbox(rect.x, rect.y, rect.w, rect.h);
    return;
  }
  held = false;
  sendCapsule();
}

/** A rect in window-logical points, as `watchPillHitbox` publishes it. */
export interface HitRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface PillHitboxWatch {
  /** Re-measure and re-publish now (the hot area changed without a resize). */
  refresh(): void;
  /** Disconnect every observer. */
  stop(): void;
}

/**
 * Track a DOM-laid-out capsule's rect and keep the backend current. Resizes come
 * from the capsule's own expand/collapse transition; the `data-dock` attribute
 * moves it sideways without resizing it, so watch that too.
 *
 * Y5-E — `shape` maps the measured capsule to the rect the panel must take the
 * cursor over. ClassicPill passes `hotRect`, so while the pill is hover-expanded
 * the published hitbox is the capsule PLUS the alpha margin float.css paints —
 * the panel's ignore-mouse region and the painted hot area are one rect. When
 * the hover phase flips without a resize, the caller calls `refresh()`.
 */
export function watchPillHitbox(el: HTMLElement, shape: (capsule: HitRect) => HitRect = (r) => r): PillHitboxWatch {
  const push = () => {
    const b = el.getBoundingClientRect();
    const r = shape({ x: b.left, y: b.top, w: b.width, h: b.height });
    reportPillHitbox(r.x, r.y, r.w, r.h);
  };
  push();
  const ro = new ResizeObserver(push);
  ro.observe(el);
  const mo = new MutationObserver(push);
  mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-dock"] });
  window.addEventListener("resize", push);
  return {
    refresh: push,
    stop: () => {
      ro.disconnect();
      mo.disconnect();
      window.removeEventListener("resize", push);
    },
  };
}
