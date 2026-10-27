/**
 * Y5-F — the ONE toast implementation.
 *
 * Before this the main window had a single `flash` string with fire-and-forget
 * `setTimeout`s racing to clear it (a second message erased the first, and the
 * first one's timer then erased the second early), plus two side-channel ids
 * (`retryId`, `copyAgainId`) bolted on for its buttons. This replaces all of it:
 *
 *   - a QUEUE: at most `TOAST_CAP` visible, the rest wait their turn;
 *   - DEDUPE by key (the error code, or the line): the same failure twice
 *     refreshes the toast that is already up and counts it, never stacks;
 *   - AUTO-DISMISS after a duration PROPORTIONAL TO LENGTH (and longer when
 *     there is a button to reach), plus manual dismiss;
 *   - a CAP on the waiting queue too, so a storm of failures cannot bury the
 *     app or replay for a minute after it stops.
 *
 * The queue is a pure reducer (tested in toast.test.ts); `useToasts` is the
 * React binding the shell uses.
 */
import { useCallback, useEffect, useReducer } from "react";
import type { AdviceAction } from "./errors";

/** Visible at once. Three visible toasts is a broken app. */
export const TOAST_CAP = 2;
/** Waiting behind the visible ones; the OLDEST waiting toast is dropped beyond this. */
export const QUEUE_LIMIT = 4;
export const MIN_TOAST_MS = 2500;
export const MAX_TOAST_MS = 9000;
/** Reading time per character (~200 wpm at ~6 chars per word). */
export const MS_PER_CHAR = 50;
/** Extra time to move the pointer to a button. */
export const ACTION_BONUS_MS = 2000;

export type ToastTone = "info" | "error";

/** A toast's button: an advice action plus the row it acts on, when it has one. */
export type ToastAction = AdviceAction & { arg?: string };

export type Toast = {
  id: number;
  key: string;
  line: string;
  tone: ToastTone;
  action: ToastAction | null;
  /** How many times this key was pushed while it was up or waiting. */
  count: number;
  durationMs: number;
  /** When it became visible (or was last refreshed); 0 while queued. */
  shownAt: number;
};

export type ToastInput = {
  line: string;
  /** Dedupe key; defaults to the line. Use the error code when there is one. */
  key?: string;
  tone?: ToastTone;
  action?: ToastAction | null;
  durationMs?: number;
};

export type ToastState = { visible: Toast[]; queued: Toast[]; nextId: number };

export const EMPTY_TOASTS: ToastState = { visible: [], queued: [], nextId: 1 };

/** How long a line stays up: proportional to its length, clamped. */
export function toastDuration(line: string, hasAction = false): number {
  const reading = Math.min(MAX_TOAST_MS, Math.max(MIN_TOAST_MS, 1000 + line.length * MS_PER_CHAR));
  return Math.min(MAX_TOAST_MS + ACTION_BONUS_MS, reading + (hasAction ? ACTION_BONUS_MS : 0));
}

function promote(visible: Toast[], queued: Toast[], now: number): ToastState["visible"] {
  const out = [...visible];
  while (out.length < TOAST_CAP && queued.length > 0) {
    const next = queued.shift()!;
    out.push({ ...next, shownAt: now });
  }
  return out;
}

export function pushToast(state: ToastState, input: ToastInput, now: number): ToastState {
  const key = input.key ?? input.line;
  const action = input.action ?? null;
  const durationMs = input.durationMs ?? toastDuration(input.line, !!action);
  const refresh = (t: Toast, shownAt: number): Toast => ({
    ...t,
    line: input.line,
    tone: input.tone ?? t.tone,
    action,
    durationMs,
    count: t.count + 1,
    shownAt,
  });
  const vi = state.visible.findIndex((t) => t.key === key);
  if (vi >= 0) {
    const visible = [...state.visible];
    visible[vi] = refresh(visible[vi], now);
    return { ...state, visible };
  }
  const qi = state.queued.findIndex((t) => t.key === key);
  if (qi >= 0) {
    const queued = [...state.queued];
    queued[qi] = refresh(queued[qi], 0);
    return { ...state, queued };
  }
  const toast: Toast = {
    id: state.nextId,
    key,
    line: input.line,
    tone: input.tone ?? "info",
    action,
    count: 1,
    durationMs,
    shownAt: 0,
  };
  if (state.visible.length < TOAST_CAP) {
    return { ...state, visible: [...state.visible, { ...toast, shownAt: now }], nextId: state.nextId + 1 };
  }
  const queued = [...state.queued, toast];
  while (queued.length > QUEUE_LIMIT) queued.shift();
  return { ...state, queued, nextId: state.nextId + 1 };
}

/** Remove every toast (visible or waiting) matching `pred`, then promote. */
export function dismissWhere(state: ToastState, pred: (t: Toast) => boolean, now: number): ToastState {
  const visible = state.visible.filter((t) => !pred(t));
  const queued = state.queued.filter((t) => !pred(t));
  if (visible.length === state.visible.length && queued.length === state.queued.length) return state;
  return { ...state, visible: promote(visible, queued, now), queued };
}

export function dismissToast(state: ToastState, id: number, now: number): ToastState {
  return dismissWhere(state, (t) => t.id === id, now);
}

/** Drop every visible toast whose time is up, and let the queue move up. */
export function expireToasts(state: ToastState, now: number): ToastState {
  return dismissWhere(state, (t) => t.shownAt > 0 && t.shownAt + t.durationMs <= now, now);
}

/** The earliest moment a visible toast expires, or null with nothing up. */
export function nextExpiry(state: ToastState): number | null {
  if (state.visible.length === 0) return null;
  return Math.min(...state.visible.map((t) => t.shownAt + t.durationMs));
}

type Act =
  | { type: "push"; input: ToastInput; now: number }
  | { type: "dismiss"; id: number; now: number }
  | { type: "dismissWhere"; pred: (t: Toast) => boolean; now: number }
  | { type: "expire"; now: number };

export function toastReducer(state: ToastState, act: Act): ToastState {
  switch (act.type) {
    case "push":
      return pushToast(state, act.input, act.now);
    case "dismiss":
      return dismissToast(state, act.id, act.now);
    case "dismissWhere":
      return dismissWhere(state, act.pred, act.now);
    case "expire":
      return expireToasts(state, act.now);
  }
}

/** The React binding: one timer, re-armed for the next expiry. */
export function useToasts() {
  const [state, dispatch] = useReducer(toastReducer, EMPTY_TOASTS);
  const due = nextExpiry(state);
  useEffect(() => {
    if (due === null) return;
    const t = setTimeout(() => dispatch({ type: "expire", now: Date.now() }), Math.max(0, due - Date.now()));
    return () => clearTimeout(t);
  }, [due]);
  const push = useCallback((input: ToastInput) => dispatch({ type: "push", input, now: Date.now() }), []);
  const dismiss = useCallback((id: number) => dispatch({ type: "dismiss", id, now: Date.now() }), []);
  const dismissMatching = useCallback(
    (pred: (t: Toast) => boolean) => dispatch({ type: "dismissWhere", pred, now: Date.now() }),
    [],
  );
  return { toasts: state.visible, push, dismiss, dismissMatching };
}
