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
import { EXPANDED_CLASSES, HOVER_ATTR, publishedHitbox, type HoverPhase } from "./hitbox";
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
 * Is the capsule at full size right now? Y5-E: derived from the SAME class list
 * and attribute float.css keys its expanded geometry off, so the rect handed to
 * the NSPanel and the box CSS paints can never disagree about which state the
 * pill is in.
 */
export function pillHoverPhase(el: HTMLElement): HoverPhase {
  if (el.getAttribute(HOVER_ATTR) === "expanded") return "expanded";
  return EXPANDED_CLASSES.some((c) => el.classList.contains(c)) ? "expanded" : "collapsed";
}

/**
 * Track a DOM-laid-out capsule's rect and keep the backend current. Resizes come
 * from the capsule's own expand/collapse transition; the `data-dock` attribute
 * moves it sideways without resizing it, so watch that too. Returns a teardown.
 *
 * Y5-E — what gets published is `publishedHitbox()`, not the bare rect: while
 * the capsule is expanded float.css paints an invisible `inset: -12px` alpha
 * margin around it (`.pill::before`), and the panel's mouse region has to be
 * that same box. If it were not, the cursor drifting off the capsule into its
 * own margin would leave the OS-level hot region, the webview would stop
 * receiving pointermove, and the hover machine would never see the samples that
 * are supposed to keep it expanded — the margin would be worse than none.
 * Collapsed, no `::before` exists and none is published, so an idle 16px seed
 * has no 12px dead zone around it. The element's own `class` and
 * `data-hover` are watched because a phase flip changes the published rect
 * even when the element has not finished resizing.
 */
export function watchPillHitbox(el: HTMLElement): () => void {
  const push = () => {
    const r = el.getBoundingClientRect();
    const box = publishedHitbox(
      { x: r.left, y: r.top, w: r.width, h: r.height },
      pillHoverPhase(el),
    );
    reportPillHitbox(box.x, box.y, box.w, box.h);
  };
  push();
  const ro = new ResizeObserver(push);
  ro.observe(el);
  const mo = new MutationObserver(push);
  mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-dock"] });
  const phaseMo = new MutationObserver(push);
  phaseMo.observe(el, { attributes: true, attributeFilter: ["class", HOVER_ATTR] });
  window.addEventListener("resize", push);
  return () => {
    ro.disconnect();
    mo.disconnect();
    phaseMo.disconnect();
    window.removeEventListener("resize", push);
  };
}
