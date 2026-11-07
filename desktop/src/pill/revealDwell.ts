// yap-1107-reveal-stays-until-read — how long the "Polished — Reveal" toast and
// the opened "What polish changed" card stay up. Wilson, 2026-11-07: the reveal
// left faster than Wispr Flow's (4 s, no pause on hover). The rule now:
//   - never closes on the timer before REVEAL_MIN_DWELL_MS after it was shown;
//   - never closes on the timer while the pointer is inside it;
//   - never closes on the timer within REVEAL_POINTER_GRACE_MS of the last
//     pointer move inside it (leaving counts as a move);
//   - never closes on the timer when "Keep the reveal open until I close it"
//     is on, and the opened card never closes on the timer at all (it waits
//     for an answer, as before).
// The close paths (Esc from the backend or the webview, a click outside, the
// close button, Keep / Dismiss, Use original, a new recording, muting Tips) are
// immediate and ignore the dwell.
// Pure: `mayAutoClose` is the rule, `createRevealDwell` the timer around it with
// the clock and timers injectable, so vitest drives it with fake timers.
import { REVEAL_EDIT_TTL_MS } from "./polishResult";

/** The reveal is never closed by its timer sooner than this after it shows. */
export const REVEAL_MIN_DWELL_MS = REVEAL_EDIT_TTL_MS;
/** ...nor sooner than this after the pointer last moved inside it. */
export const REVEAL_POINTER_GRACE_MS = 2_000;

/** Which face is up. Only the toast has a timer; the card waits for an answer. */
export type RevealFace = "toast" | "card";

/** Why the reveal went away. Mirrors `float_pill.rs` `REVEAL_CLOSE_REASONS`. */
export type RevealCloseReason =
  | "timer"
  | "esc"
  | "click_outside"
  | "close_button"
  | "keep"
  | "use_original"
  | "new_recording"
  | "muted";

export const REVEAL_CLOSE_REASONS: readonly RevealCloseReason[] = [
  "timer",
  "esc",
  "click_outside",
  "close_button",
  "keep",
  "use_original",
  "new_recording",
  "muted",
];

export function isRevealCloseReason(v: unknown): v is RevealCloseReason {
  return typeof v === "string" && (REVEAL_CLOSE_REASONS as readonly string[]).includes(v);
}

/** Everything the rule needs to know, in milliseconds on one clock. */
export interface RevealDwellState {
  /** When the reveal was shown. */
  shownAt: number;
  /** The face that is up. */
  face: RevealFace;
  /** Is the pointer inside the reveal right now? */
  pointerInside: boolean;
  /** The last pointer move inside (or the leave), or null if it never entered. */
  lastMoveInsideAt: number | null;
  /** Settings › Notifications "Keep the reveal open until I close it". */
  keepOpen: boolean;
}

/** May the TIMER close the reveal at `now`? The explicit close paths never ask. */
export function mayAutoClose(s: RevealDwellState, now: number): boolean {
  return revealCloseAt(s) !== null && now >= (revealCloseAt(s) as number);
}

/**
 * The earliest time the timer may close the reveal, or null when the timer may
 * not close it at all right now (keep-open, the card, or the pointer inside).
 */
export function revealCloseAt(s: RevealDwellState): number | null {
  if (s.keepOpen || s.face !== "toast" || s.pointerInside) return null;
  const dwell = s.shownAt + REVEAL_MIN_DWELL_MS;
  const grace = s.lastMoveInsideAt === null ? dwell : s.lastMoveInsideAt + REVEAL_POINTER_GRACE_MS;
  return Math.max(dwell, grace);
}

export interface RevealDwellDeps {
  /** Called once per close, whatever closed it. */
  onClose: (reason: RevealCloseReason, dwellMs: number) => void;
  now?: () => number;
  setTimer?: (fn: () => void, ms: number) => unknown;
  clearTimer?: (handle: unknown) => void;
}

export interface RevealDwell {
  /** A reveal appears (or a newer one replaces it): the dwell clock restarts. */
  show(face: RevealFace): void;
  /** Toast -> card (Reveal pressed). */
  setFace(face: RevealFace): void;
  setKeepOpen(on: boolean): void;
  pointerEnter(): void;
  pointerMove(): void;
  pointerLeave(): void;
  /** An explicit close path: immediate, ignores the dwell. No-op when closed. */
  close(reason: RevealCloseReason): void;
  isOpen(): boolean;
  /** Stop the timer without calling onClose (unmount). */
  dispose(): void;
}

export function createRevealDwell(
  keepOpen: boolean,
  {
    onClose,
    now = () => Date.now(),
    setTimer = (fn, ms) => setTimeout(fn, ms),
    clearTimer = (h) => clearTimeout(h as ReturnType<typeof setTimeout>),
  }: RevealDwellDeps,
): RevealDwell {
  let open = false;
  let timer: unknown = null;
  const state: RevealDwellState = {
    shownAt: 0,
    face: "toast",
    pointerInside: false,
    lastMoveInsideAt: null,
    keepOpen,
  };

  const stop = () => {
    if (timer !== null) clearTimer(timer);
    timer = null;
  };
  const schedule = () => {
    stop();
    if (!open) return;
    const at = revealCloseAt(state);
    if (at === null) return;
    timer = setTimer(tick, Math.max(0, at - now()));
  };
  const tick = () => {
    timer = null;
    if (!open) return;
    if (mayAutoClose(state, now())) api.close("timer");
    else schedule();
  };

  const api: RevealDwell = {
    show(face) {
      open = true;
      state.shownAt = now();
      state.face = face;
      state.pointerInside = false;
      state.lastMoveInsideAt = null;
      schedule();
    },
    setFace(face) {
      state.face = face;
      schedule();
    },
    setKeepOpen(on) {
      state.keepOpen = on;
      schedule();
    },
    pointerEnter() {
      state.pointerInside = true;
      state.lastMoveInsideAt = now();
      stop();
    },
    pointerMove() {
      state.pointerInside = true;
      state.lastMoveInsideAt = now();
      stop();
    },
    pointerLeave() {
      if (!state.pointerInside) return;
      state.pointerInside = false;
      state.lastMoveInsideAt = now();
      schedule();
    },
    close(reason) {
      if (!open) return;
      open = false;
      stop();
      onClose(reason, Math.max(0, now() - state.shownAt));
    },
    isOpen: () => open,
    dispose() {
      open = false;
      stop();
    },
  };
  return api;
}
