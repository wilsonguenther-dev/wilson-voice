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
import { useEffect, useMemo, useRef } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  beginPress,
  CLICK_SLOP,
  endPress,
  markPressed,
  pressLogLine,
  pressVisualLatency,
  trackPress,
  type PressKind,
} from "./press";
import { prefersReducedMotion } from "./motion";

/**
 * Travel (logical points) past which a press is a DRAG, not a click. WF-01 —
 * the SAME 4 points the click classifier uses (pill/press.ts), so the panel
 * never starts sliding on a press that would still count as a click.
 */
const DRAG_SLOP = CLICK_SLOP;

/* ── WF-02 — the drag reducer ─────────────────────────────────────────────
 *
 * The whole gesture as a pure state machine, so the parts that matter (when it
 * becomes a drag, what Esc does mid-drag, what a release after Esc does) are
 * tested without a DOM, a window or a backend. The hook below only feeds it
 * pointer/key events and carries out the effects it returns.
 *
 *   idle ──down──▶ pressed ──move past slop──▶ blob ──up──▶ idle  (+ snap to a dock)
 *                     │                          │
 *                     └──────── Esc ─────────────┴──▶ cancelled ──up──▶ idle  (old dock, no click)
 *
 * `blob` is WF-02's shape: the pill collapses to a 28 pt blob (Yappy's face
 * only) and the three drop zones (bottom, left, right) show, the one a release
 * would land in lit. Esc puts the pill back on the dock it had at pointer-down
 * — the backend re-parks it there (`pill_drag_end` with `snap: false`) — and
 * every later move or the release is inert, so the press can never turn into a
 * click (dictation) or a second dock change.
 */

/** The three docks — the same strings `pill_position` / `PillPosition` use. */
export type PillDock = "bottom" | "left" | "right";
export const PILL_DOCKS: readonly PillDock[] = ["bottom", "left", "right"];

/** float_pill.rs EDGE_SNAP_PT — a release within this many points of a side
 *  edge docks there; everything else lands on the bottom island. */
export const EDGE_SNAP_PT = 120;

/** WF-02 — the blob's diameter, in points: Yappy's face and nothing else. */
export const BLOB_PT = 28;

/** How long the squash-on-drop plays before the pill settles (ms). */
export const SQUASH_MS = 220;

/**
 * Mirror of float_pill.rs `snap_position`: which dock a release at screen-x `x`
 * lands in, on a monitor whose logical frame starts at `frameX` and is `frameW`
 * wide. Only the horizontal throw matters there, so only it matters here.
 */
export function snapTarget(x: number, frameX: number, frameW: number): PillDock {
  if (!(frameW > 0)) return "bottom";
  if (x - frameX <= EDGE_SNAP_PT) return "left";
  if (frameX + frameW - x <= EDGE_SNAP_PT) return "right";
  return "bottom";
}

export type DragPhase = "idle" | "pressed" | "blob" | "cancelled";

export interface DragState {
  phase: DragPhase;
  /** Screen point of the pointer-down; every delta is measured from it. */
  x0: number;
  y0: number;
  /** The dock the pill had at pointer-down — what Esc restores. */
  from: PillDock;
  /** The dock a release right now would land in (lit drop zone). */
  target: PillDock;
  /** True once the press passed the slop. Survives the release (and an Esc)
   *  so the `click` that follows a drag never fires the pill's click. */
  moved: boolean;
  /** Set by Esc; read by the hook's `cancelled()` and the telemetry line. */
  cancelled: boolean;
}

export type DragEvent =
  | { type: "down"; x: number; y: number; dock: PillDock }
  | { type: "move"; x: number; y: number; frameX: number; frameW: number }
  | { type: "up"; x: number; y: number }
  | { type: "cancel" }
  | { type: "escape" };

/** What the hook must do for a transition. Order matters; run them in order. */
export type DragEffect =
  | { kind: "start" }
  | { kind: "blob" }
  | { kind: "move"; dx: number; dy: number }
  /** Release the panel. `snap: false` re-parks it on the dock it already had. */
  | { kind: "end"; x: number; y: number; snap: boolean }
  /** Esc: put the pill back on `dock` (the visual half of `end` with no snap). */
  | { kind: "restore"; dock: PillDock }
  /** The pill has a dock again: squash-and-settle (or an instant change). */
  | { kind: "settle"; dock: PillDock };

export const DRAG_IDLE: DragState = {
  phase: "idle",
  x0: 0,
  y0: 0,
  from: "bottom",
  target: "bottom",
  moved: false,
  cancelled: false,
};

export function dragReduce(s: DragState, e: DragEvent): { state: DragState; effects: DragEffect[] } {
  switch (e.type) {
    case "down":
      // A new press always starts clean — even over a stuck gesture.
      return {
        state: { phase: "pressed", x0: e.x, y0: e.y, from: e.dock, target: e.dock, moved: false, cancelled: false },
        effects: [{ kind: "start" }],
      };
    case "move": {
      if (s.phase !== "pressed" && s.phase !== "blob") return { state: s, effects: [] };
      const dx = e.x - s.x0;
      const dy = e.y - s.y0;
      if (s.phase === "pressed" && Math.hypot(dx, dy) < DRAG_SLOP) return { state: s, effects: [] };
      const target = snapTarget(e.x, e.frameX, e.frameW);
      const effects: DragEffect[] = s.phase === "pressed" ? [{ kind: "blob" }] : [];
      effects.push({ kind: "move", dx, dy });
      return { state: { ...s, phase: "blob", moved: true, target }, effects };
    }
    case "escape": {
      if (s.phase === "blob") {
        return {
          state: { ...s, phase: "cancelled", target: s.from, cancelled: true },
          effects: [
            { kind: "end", x: s.x0, y: s.y0, snap: false },
            { kind: "restore", dock: s.from },
          ],
        };
      }
      if (s.phase === "pressed") {
        // Esc before the slop: no move happened, so nothing to restore — but
        // the press is spent, so its release is not a click either.
        return {
          state: { ...s, phase: "cancelled", cancelled: true },
          effects: [{ kind: "end", x: s.x0, y: s.y0, snap: false }],
        };
      }
      return { state: s, effects: [] };
    }
    case "up":
    case "cancel": {
      if (s.phase === "cancelled") return { state: { ...s, phase: "idle" }, effects: [] };
      if (s.phase === "pressed") {
        const at = e.type === "up" ? e : { x: s.x0, y: s.y0 };
        return { state: { ...s, phase: "idle" }, effects: [{ kind: "end", x: at.x, y: at.y, snap: false }] };
      }
      if (s.phase === "blob") {
        if (e.type === "cancel") {
          // The system took the pointer: no release point to throw to, so the
          // pill goes home exactly like Esc.
          return {
            state: { ...s, phase: "idle", target: s.from },
            effects: [
              { kind: "end", x: s.x0, y: s.y0, snap: false },
              { kind: "restore", dock: s.from },
            ],
          };
        }
        return {
          state: { ...s, phase: "idle" },
          effects: [
            { kind: "end", x: e.x, y: e.y, snap: true },
            { kind: "settle", dock: s.target },
          ],
        };
      }
      return { state: s, effects: [] };
    }
  }
}

/** The dock a press STARTS from, read off `<html data-dock>` (float-main sets it). */
export function currentDock(root: HTMLElement | null = typeof document === "undefined" ? null : document.documentElement): PillDock {
  const d = root?.dataset.dock;
  return d === "left" || d === "right" ? d : "bottom";
}

/** What the drag paints: the resting pill, the dragged blob, or the drop squash. */
export type DragVisual = "rest" | "blob" | "squash";

/**
 * WF-02 — publish the drag's look on `<html>` for float.css:
 *   data-pill-drag        = "blob" | "squash" | (absent at rest)
 *   data-pill-drag-target = the lit drop zone while a blob is up
 *   data-pill-drag-motion = "instant" under Reduce Motion, else "animate"
 * Reduce Motion is not a slower animation: the blob appears and disappears in
 * one frame and the squash never plays (`settleVisual`).
 */
export function applyDragVisual(
  root: HTMLElement,
  visual: DragVisual,
  target: PillDock | null,
  reduceMotion: boolean,
): void {
  if (visual === "rest") {
    delete root.dataset.pillDrag;
    delete root.dataset.pillDragTarget;
  } else {
    root.dataset.pillDrag = visual;
    if (target && visual === "blob") root.dataset.pillDragTarget = target;
    else delete root.dataset.pillDragTarget;
  }
  root.dataset.pillDragMotion = reduceMotion ? "instant" : "animate";
}

/** After a dock (or a restore): squash then rest — or straight to rest under
 *  Reduce Motion, where nothing bounces. Returns the visual to show NOW. */
export function settleVisual(reduceMotion: boolean): DragVisual {
  return reduceMotion ? "rest" : "squash";
}

/**
 * Backend → webview: the hover watch saw Esc go down mid-drag (the float panel
 * is non-activating, so the webview may never get the keydown itself). The
 * backend has already put the panel back; this lets the gesture catch up.
 */
export const PILL_DRAG_CANCEL_EVENT = "pill-drag-cancel";

/** DEV-ONLY matrix scene: `float.html?…&drag=blob` pins the pill in the blob. */
export function forcedDragScene(search: string): "blob" | null {
  return new URLSearchParams(search).get("drag") === "blob" ? "blob" : null;
}

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
  /**
   * WF-02 — true when Esc took the LAST press back mid-drag (the pill went home
   * to the dock it had). Cleared by the next pointer-down.
   */
  cancelled: () => boolean;
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
  const st = useRef<DragState>(DRAG_IDLE);
  const press = useRef(beginPress(0, 0, 0));
  const last = useRef<PressKind | null>(null);
  const fb = useRef(feedback);
  fb.current = feedback;
  // WF-02 — the drag's own bookkeeping: Reduce Motion is read once per press
  // (a mid-drag preference flip never half-animates), the squash timer is
  // cancelled by the next press, and Esc is listened for only while a press is
  // live (on the webview, and from the backend's hover watch below).
  const reduce = useRef(false);
  const squashTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const keyOff = useRef<(() => void) | null>(null);
  const t0 = useRef(0);

  const api = useMemo(() => {
    const root = (): HTMLElement | null => (typeof document === "undefined" ? null : document.documentElement);
    const clearSquash = () => {
      if (squashTimer.current) clearTimeout(squashTimer.current);
      squashTimer.current = null;
    };
    const settle = () => {
      const r = root();
      if (!r) return;
      clearSquash();
      const v = settleVisual(reduce.current);
      applyDragVisual(r, v, null, reduce.current);
      if (v === "squash") {
        squashTimer.current = setTimeout(() => {
          squashTimer.current = null;
          const rr = root();
          if (rr) applyDragVisual(rr, "rest", null, reduce.current);
        }, SQUASH_MS);
      }
    };
    const stopKeys = () => {
      keyOff.current?.();
      keyOff.current = null;
    };
    const run = (effects: DragEffect[], state: DragState) => {
      for (const fx of effects) {
        switch (fx.kind) {
          case "start":
            invoke("pill_drag_start").catch(() => {});
            break;
          case "blob": {
            const r = root();
            if (r) {
              markPressed(r, false); // a drag is not a press: the blob takes over
              applyDragVisual(r, "blob", state.target, reduce.current);
            }
            console.info(`pill drag event=blob from=${state.from} reduce_motion=${reduce.current}`);
            break;
          }
          case "move": {
            const r = root();
            if (r && r.dataset.pillDragTarget !== state.target) r.dataset.pillDragTarget = state.target;
            invoke("pill_drag_move", { dx: fx.dx, dy: fx.dy }).catch(() => {});
            break;
          }
          case "end":
            stopKeys();
            invoke("pill_drag_end", { x: fx.x, y: fx.y, snap: fx.snap }).catch(() => {});
            break;
          case "restore":
            console.info(
              `pill drag event=cancel dock=${fx.dock} ms=${Math.round(performance.now() - t0.current)}`,
            );
            settle();
            fb.current.dock?.();
            break;
          case "settle":
            console.info(
              `pill drag event=drop dock=${fx.dock} ms=${Math.round(performance.now() - t0.current)}`,
            );
            settle();
            break;
        }
      }
    };
    const step = (e: DragEvent) => {
      const { state, effects } = dragReduce(st.current, e);
      st.current = state;
      run(effects, state);
      return effects;
    };
    const escape = () => {
      if (st.current.phase === "pressed" || st.current.phase === "blob") step({ type: "escape" });
    };
    const end = (e: ReactPointerEvent<HTMLElement>) => {
      const s = st.current;
      if (s.phase === "idle") return;
      const wasCancelled = s.phase === "cancelled";
      // WF-01 — classify BEFORE anything else reacts. A cancel is never a
      // click: the system took the pointer, the user did not let go. WF-02 —
      // neither is a press Esc already took back.
      const kind: PressKind | null =
        e.type === "pointercancel" || wasCancelled
          ? null
          : endPress(press.current, e.screenX, e.screenY, performance.now());
      last.current = kind;
      // yap-1107-pill-click-always-registers — the pressed shell lets go on
      // every release path (click, drag end, cancel, Esc).
      const pr = root();
      if (pr) markPressed(pr, false);
      try {
        e.currentTarget.releasePointerCapture(e.pointerId);
      } catch {
        /* capture was already lost (window teardown) — the drag still ends */
      }
      const moved = s.moved;
      step(e.type === "pointercancel" ? { type: "cancel" } : { type: "up", x: e.screenX, y: e.screenY });
      stopKeys();
      if (wasCancelled) return; // Esc already rebounded the pill home
      // A drag that moved ends by SNAPPING into a dock, so it lands; a press
      // that never moved is a plain click, so it just rebounds.
      if (moved) fb.current.dock?.();
      else fb.current.release?.();
      if (kind === "click") fb.current.tap?.();
    };
    const handlers: PillDrag["handlers"] = {
      onPointerDown: (e) => {
        if (e.button !== 0) return; // left button only; right/middle are not a grab
        // WF-04 — Ctrl-click is the Mac's right-click: it opens the pill menu
        // (float-main's `contextmenu` listener) and must never start a take.
        if (e.ctrlKey) return;
        clearSquash();
        reduce.current = prefersReducedMotion();
        t0.current = performance.now();
        const r = root();
        if (r) {
          applyDragVisual(r, "rest", null, reduce.current);
          // yap-1107-pill-click-always-registers — the pressed shell (pill.html:
          // 96 percent) goes up HERE, synchronously, before any IPC: the press
          // is visible on the next frame whatever the backend is doing. The
          // frame that paints it is timed against the event (acceptance 2).
          markPressed(r, true);
          const downAt = e.timeStamp;
          if (typeof requestAnimationFrame === "function") {
            requestAnimationFrame(() => console.info(pressLogLine(pressVisualLatency(downAt, performance.now()))));
          }
        }
        press.current = beginPress(e.screenX, e.screenY, performance.now());
        try {
          // Capture so the gesture keeps reporting once the panel slides out
          // from under the pointer.
          e.currentTarget.setPointerCapture(e.pointerId);
        } catch {
          /* non-fatal: the drag just ends if the pointer leaves the capsule */
        }
        stopKeys();
        if (typeof window !== "undefined") {
          const onKey = (k: KeyboardEvent) => {
            if (k.key !== "Escape") return;
            k.preventDefault();
            escape();
          };
          window.addEventListener("keydown", onKey, true);
          keyOff.current = () => window.removeEventListener("keydown", onKey, true);
        }
        fb.current.press?.();
        step({ type: "down", x: e.screenX, y: e.screenY, dock: currentDock() });
      },
      onPointerMove: (e) => {
        if (st.current.phase !== "pressed" && st.current.phase !== "blob") return;
        trackPress(press.current, e.screenX, e.screenY);
        // The pill parks on the PRIMARY monitor (float_pill.rs monitor_frame);
        // the webview's `screen` is that monitor in logical points.
        const scr = typeof window !== "undefined" ? window.screen : null;
        const frameX = (scr as (Screen & { availLeft?: number }) | null)?.availLeft ?? 0;
        step({ type: "move", x: e.screenX, y: e.screenY, frameX, frameW: scr?.width ?? 0 });
      },
      onPointerUp: end,
      onPointerCancel: end,
    };
    return { handlers, escape, clearSquash, stopKeys };
  }, []);

  // WF-02 — the backend's Esc (the panel is non-activating, so the keydown may
  // never reach this webview). The backend has already re-parked the panel;
  // the reducer catches up so the release is inert and the blob goes home.
  useEffect(() => {
    let off: (() => void) | null = null;
    let gone = false;
    listen(PILL_DRAG_CANCEL_EVENT, () => api.escape())
      .then((u) => {
        if (gone) u();
        else off = u;
      })
      .catch(() => {});
    return () => {
      gone = true;
      off?.();
      api.clearSquash();
      api.stopKeys();
    };
  }, [api]);

  return useMemo<PillDrag>(
    () => ({
      handlers: api.handlers,
      dragged: () => st.current.moved || st.current.cancelled,
      lastPress: () => last.current,
      cancelled: () => st.current.cancelled,
    }),
    [api],
  );
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
const extraRects = new Map<string, [number, number, number, number]>();

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
  let r: Rect4 | null = capsuleRect;
  for (const e of extraRects.values()) r = unionHitbox(r, e);
  if (r) sendHitbox(...r);
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
  if (rect) extraRects.set(owner, [rect.x, rect.y, rect.w, rect.h]);
  else extraRects.delete(owner);
  if (holds.size) return;
  sendCapsule();
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
  let u: Rect4 | null = null;
  for (const h of holds.values()) u = unionHitbox(u, h);
  if (u) sendHitbox(...u);
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
