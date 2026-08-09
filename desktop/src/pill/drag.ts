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

export function usePillDrag(): PillDrag {
  const st = useRef({ down: false, moved: false, x: 0, y: 0 });
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
export function reportPillHitbox(x: number, y: number, w: number, h: number): void {
  const key = `${Math.round(x)},${Math.round(y)},${Math.round(w)},${Math.round(h)}`;
  if (key === lastHitbox) return;
  lastHitbox = key;
  invoke("pill_set_hitbox", { x, y, w, h }).catch(() => {});
}

/**
 * Track a DOM-laid-out capsule's rect and keep the backend current. Resizes come
 * from the capsule's own expand/collapse transition; the `data-dock` attribute
 * moves it sideways without resizing it, so watch that too. Returns a teardown.
 */
export function watchPillHitbox(el: HTMLElement): () => void {
  const push = () => {
    const r = el.getBoundingClientRect();
    reportPillHitbox(r.left, r.top, r.width, r.height);
  };
  push();
  const ro = new ResizeObserver(push);
  ro.observe(el);
  const mo = new MutationObserver(push);
  mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-dock"] });
  window.addEventListener("resize", push);
  return () => {
    ro.disconnect();
    mo.disconnect();
    window.removeEventListener("resize", push);
  };
}
