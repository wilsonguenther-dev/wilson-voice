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
import { beginPress, CLICK_SLOP, endPress, trackPress, type PressKind } from "./press";

/**
 * Travel (logical points) past which a press is a DRAG, not a click. WF-01 —
 * the SAME 4 points the click classifier uses (pill/press.ts), so the panel
 * never starts sliding on a press that would still count as a click.
 */
const DRAG_SLOP = CLICK_SLOP;

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
  /**
   * yap-wf-01-click-the-pill-to — how the LAST completed press classified
   * (pill/press.ts: under 4 points and under 300 ms is a click). `null` before
   * the first release, and for a press the system cancelled.
   */
  lastPress: () => PressKind | null;
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
  /**
   * yap-wf-01-click-the-pill-to — WF-01: the press was a CLICK (a pointer-up
   * under 4 points of travel and under 300 ms). Never called for a drag, a
   * long hold, or a cancelled pointer.
   */
  tap?(): void;
}

export function usePillDrag(feedback: PillGestureFeedback = {}): PillDrag {
  const st = useRef({ down: false, moved: false, x: 0, y: 0 });
  const press = useRef(beginPress(0, 0, 0));
  const last = useRef<PressKind | null>(null);
  const fb = useRef(feedback);
  fb.current = feedback;
  return useMemo<PillDrag>(() => {
    const end = (e: ReactPointerEvent<HTMLElement>) => {
      const s = st.current;
      if (!s.down) return;
      s.down = false;
      // WF-01 — classify BEFORE anything else reacts. A cancel is never a
      // click: the system took the pointer, the user did not let go.
      const kind: PressKind | null =
        e.type === "pointercancel" ? null : endPress(press.current, e.screenX, e.screenY, performance.now());
      last.current = kind;
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
      if (kind === "click") fb.current.tap?.();
    };
    return {
      handlers: {
        onPointerDown: (e) => {
          if (e.button !== 0) return; // left button only; right/middle are not a grab
          st.current = { down: true, moved: false, x: e.screenX, y: e.screenY };
          press.current = beginPress(e.screenX, e.screenY, performance.now());
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
          trackPress(press.current, e.screenX, e.screenY);
          if (!s.moved && Math.hypot(dx, dy) < DRAG_SLOP) return;
          s.moved = true;
          invoke("pill_drag_move", { dx, dy }).catch(() => {});
        },
        onPointerUp: end,
        onPointerCancel: end,
      },
      dragged: () => st.current.moved,
      lastPress: () => last.current,
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

/**
 * yap-1106-fb-02-pill-first-click — the PRECISE hit area: every rect the panel
 * must take the cursor over (the capsule, each control's own drawn rect, a
 * held card), as a list. The backend hit-tests the list, so the empty glass
 * between two controls stays click-through while every control — wherever the
 * layout put it — takes its click on the first press. The single bounding
 * union above is still sent for older listeners; the backend ignores it once
 * this list has arrived. Published on `window.__YAP_PILL_HITBOXES__` too, so
 * the headless click matrix (scripts/pill-matrix.mjs --clicks) can check that
 * every probe point lies inside what the NSPanel was told.
 */
let lastHitboxes = "";
function sendHitboxes(rects: Rect4[]): void {
  const live = rects.filter((r) => r[2] > 0 && r[3] > 0);
  const key = live.map((r) => r.map(Math.round).join(",")).join(";");
  if (key === lastHitboxes) return;
  lastHitboxes = key;
  (globalThis as unknown as { __YAP_PILL_HITBOXES__?: Rect4[] }).__YAP_PILL_HITBOXES__ = live;
  invoke("pill_set_hitboxes", { rects: live.map(([x, y, w, h]) => ({ x, y, w, h })) }).catch(() => {});
}

/** The owner `hitTargets.ts` publishes every `[data-pill-control]` rect under. */
export const CONTROLS_OWNER = "controls";

/** The capsule's latest rect, kept while a held rect overrides it. */
let capsuleRect: [number, number, number, number] | null = null;
/**
 * Held rects by owner. The Copy card and the meeting offer can both be up at
 * once (#272), so one card releasing its hold must never hand the hitbox back
 * to the capsule while the other card is still drawn.
 */
const holds = new Map<string, [number, number, number, number]>();
/**
 * yap24-NT7 fix — a control that floats OUTSIDE the capsule's own rect (Yappy's
 * meeting badge: the canvas capsule sits mid-window, the badge is pinned to the
 * window's bottom edge) must still receive clicks through the click-through
 * panel. The backend takes ONE rect, so the capsule and this extra rect are
 * sent as their bounding union. `null` = no extra; the capsule alone.
 */
/** Extra rects the hitbox covers beside the capsule, one per owner (WF-20: the
 *  style chip and the meeting control can be up together). */
const extraRects = new Map<string, Rect4[]>();

export type Rect4 = [number, number, number, number];

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
  let r: Rect4 | null = capsuleRect;
  const list: Rect4[] = capsuleRect ? [capsuleRect] : [];
  for (const rs of extraRects.values()) {
    for (const e of rs) {
      r = unionHitbox(r, e);
      list.push(e);
    }
  }
  if (r) sendHitbox(...r);
  sendHitboxes(list);
}

export function reportPillHitbox(x: number, y: number, w: number, h: number): void {
  capsuleRect = [x, y, w, h];
  if (holds.size) return;
  sendCapsule();
}

/**
 * yap24-NT7 fix — add (or, with `null`, remove) a rect the hitbox must cover IN
 * ADDITION to the capsule. Unlike `holdPillHitbox` this never replaces the
 * capsule: the pill stays grabbable while the extra control is clickable.
 */
export function extendPillHitbox(
  rect: { x: number; y: number; w: number; h: number } | null,
  owner = "capsule",
): void {
  if (rect) extraRects.set(owner, [[rect.x, rect.y, rect.w, rect.h]]);
  else extraRects.delete(owner);
  if (holds.size) {
    sendHeld();
    return;
  }
  sendCapsule();
}

/**
 * yap-1106-fb-02-pill-first-click — like `extendPillHitbox`, but an owner may
 * add SEVERAL rects (hitTargets.ts: one per `[data-pill-control]`). `null` or
 * an empty list removes the owner. The controls owner stays live while a card
 * holds the hitbox, so the meeting stop is never unreachable behind a card.
 */
export function extendPillHitboxRects(rects: Rect4[] | null, owner: string): void {
  if (rects && rects.length) extraRects.set(owner, rects.map((r) => [r[0], r[1], r[2], r[3]] as Rect4));
  else extraRects.delete(owner);
  if (holds.size) {
    sendHeld();
    return;
  }
  sendCapsule();
}

/** While a card holds: the held rects (legacy union), plus the controls (list). */
function sendHeld(): void {
  let u: Rect4 | null = null;
  const list: Rect4[] = [];
  for (const h of holds.values()) {
    u = unionHitbox(u, h);
    list.push(h);
  }
  if (u) sendHitbox(...u);
  for (const c of extraRects.get(CONTROLS_OWNER) ?? []) list.push(c);
  sendHitboxes(list);
}

/**
 * yap-p0-insertion-bridge-fallback — a card (the Copy card, the meeting offer)
 * takes the hitbox while it is up, so its buttons receive clicks through the
 * click-through panel. Each card holds under its own `owner`; while any hold is
 * live the hitbox is the union of the held rects. `null` releases only that
 * owner's hold, and the last release hands the hitbox back to the capsule.
 */
export function holdPillHitbox(
  rect: { x: number; y: number; w: number; h: number } | null,
  owner: string,
): void {
  if (rect) holds.set(owner, [rect.x, rect.y, rect.w, rect.h]);
  else holds.delete(owner);
  if (!holds.size) {
    sendCapsule();
    return;
  }
  sendHeld();
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
