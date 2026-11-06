/**
 * yap-1106-retry-failed-dictation — the Retry button's clock and words.
 *
 * Wilson: "I've never pressed retry and it worked, ever. It always infinitely
 * loads, brings the wall of doom and crashes." The backend now runs a retry
 * off the main thread, on the ONE speech engine, under a deadline scaled to
 * the clip (`retry_deadline` in lib.rs — mirrored here as `retryDeadlineMs`).
 * This module is the UI half: the button never stays in its loading state
 * past that deadline even if the backend never answers (the watchdog), it
 * says "Waiting for the speech engine…" while the retry queues behind a live
 * dictation, and the failure is one plain sentence with one action.
 */

/** Seconds every retry gets on top of the clip's own length. */
export const RETRY_DEADLINE_BASE_S = 15;
/** No retry deadline is shorter than this … */
export const RETRY_DEADLINE_MIN_S = 30;
/** … or longer than this. */
export const RETRY_DEADLINE_MAX_S = 180;
/**
 * How long after the backend's own deadline the UI waits before it gives up
 * on its own. The backend answers at its deadline; this only fires if it is
 * wedged (the hang this item exists for).
 */
export const RETRY_WATCHDOG_GRACE_MS = 15_000;
/** The one sentence a retry that ran out of time shows. The clip stays kept. */
export const RETRY_DEADLINE_MESSAGE = "Couldn't transcribe this clip — Try again";
/** The button while the retry waits for a live dictation to let go of the engine. */
export const RETRY_QUEUED_LABEL = "Waiting for the speech engine…";
/** The backend's `retry-dictation` event: where the retry is, and its deadline. */
export const RETRY_PHASE_EVENT = "retry-dictation";

export type RetryPhase = "queued" | "decoding";

export interface RetryPhaseEvent {
  id: string;
  phase: RetryPhase;
  /** The whole retry's deadline in ms, counted from when the backend admitted it. */
  deadlineMs: number;
}

/**
 * The deadline the backend holds a retry of `clipSeconds` of audio to, in ms:
 * `clamp(15 + clip, 30, 180)` seconds. A bad or missing length gets the floor.
 */
export function retryDeadlineMs(clipSeconds: number): number {
  const clip = Number.isFinite(clipSeconds) && clipSeconds > 0 ? clipSeconds : 0;
  const s = Math.min(RETRY_DEADLINE_MAX_S, Math.max(RETRY_DEADLINE_MIN_S, RETRY_DEADLINE_BASE_S + clip));
  return Math.round(s * 1000);
}

/** The Retry / Finish button's label for one row. */
export function retryButtonLabel(a: { retrying: boolean; phase: RetryPhase | null; resumable: boolean }): string {
  if (!a.retrying) return a.resumable ? "Finish transcribing" : "Retry";
  if (a.phase === "queued") return RETRY_QUEUED_LABEL;
  return a.resumable ? "Finishing…" : "Retrying…";
}

export interface RetryWatchdog {
  /** (Re)start the clock: `onExpire` fires once `ms` from now unless cleared or re-armed. */
  arm(ms: number): void;
  /** Stop the clock. A watchdog that already fired stays `expired`. */
  clear(): void;
  /** True once `onExpire` has fired. */
  readonly expired: boolean;
}

/**
 * The clock that ends a retry's loading state on its own. Armed from the
 * row's length the moment Retry is pressed, re-armed from the backend's own
 * deadline when its first phase event arrives, cleared when the command
 * answers. It fires at most once.
 */
export function createRetryWatchdog(onExpire: () => void): RetryWatchdog {
  let handle: ReturnType<typeof setTimeout> | null = null;
  let expired = false;
  const clear = () => {
    if (handle !== null) clearTimeout(handle);
    handle = null;
  };
  return {
    arm(ms: number) {
      clear();
      if (expired) return;
      handle = setTimeout(() => {
        handle = null;
        expired = true;
        onExpire();
      }, ms);
    },
    clear,
    get expired() {
      return expired;
    },
  };
}
