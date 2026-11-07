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
/**
 * The backend's `RETRY_DEADLINE_ERROR` (lib.rs), verbatim: the retry ran past
 * its deadline and the decode was left running unobserved on the ONE engine.
 * Pinned both ways — a vitest reads lib.rs, a cargo test reads this file — so
 * a reword on either side fails a test instead of silently turning a wedged
 * engine into "try the next clip" (fix-b5-2).
 */
export const RETRY_DEADLINE_ERROR = "Couldn't transcribe this clip — Try again";
/**
 * The backend's `RETRY_ENGINE_BUSY_ERROR` (lib.rs), verbatim: the retry queued
 * behind a live dictation and never got the engine. Pinned like the above.
 */
export const RETRY_ENGINE_BUSY_ERROR = "The speech engine stayed busy with a live dictation — Try again";
/** The one sentence a retry that ran out of time shows. The clip stays kept. */
export const RETRY_DEADLINE_MESSAGE = RETRY_DEADLINE_ERROR;
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
  /**
   * Resolves the moment the watchdog fires (right after `onExpire`); never
   * resolves if it is cleared first. What `raceRetryWatchdog` races against.
   */
  readonly whenExpired: Promise<void>;
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
  let fire!: () => void; // set synchronously by the Promise executor below
  const whenExpired = new Promise<void>((resolve) => {
    fire = resolve;
  });
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
        try {
          onExpire();
        } finally {
          fire();
        }
      }, ms);
    },
    clear,
    get expired() {
      return expired;
    },
    whenExpired,
  };
}

/** How a retry's command ended against its watchdog. */
export type RetryRace<T> =
  | { kind: "answered"; value: T }
  | { kind: "errored"; error: unknown }
  /** The watchdog fired first; the command is still out and may answer late. */
  | { kind: "expired" };

/**
 * fix-b5-2 — race a retry's command against its watchdog. A backend that never
 * answers (the hang the watchdog exists for) must still END the retry, or a
 * Retry-all run awaiting it would read "Retrying k of N…" forever with every
 * row's Retry disabled. The command is not cancelled: the caller keeps `work`
 * and applies a late answer quietly.
 */
export function raceRetryWatchdog<T>(
  work: Promise<T>,
  watchdog: Pick<RetryWatchdog, "whenExpired">,
): Promise<RetryRace<T>> {
  return new Promise<RetryRace<T>>((resolve) => {
    // First to settle wins; a later resolve() is a no-op.
    void work.then(
      (value) => resolve({ kind: "answered", value }),
      (error: unknown) => resolve({ kind: "errored", error }),
    );
    void watchdog.whenExpired.then(() => resolve({ kind: "expired" }));
  });
}

// ── yap-fx-07 — "Retry all" ────────────────────────────────────────────────

/** How one row's retry ended, as the Retry-all queue needs to know it. */
export type RetryOutcome =
  /** The clip transcribed; the row is now a history entry. */
  | "recovered"
  /** This clip failed (no speech, a bad clip). The next one may still work. */
  | "failed"
  /** The engine is not healthy (stuck on an earlier clip, or a retry ran out of time). Stop. */
  | "engine-busy"
  /** Another retry was already running; nothing was done. */
  | "skipped";

/**
 * The backend's ENGINE_STUCK sentence (transcription.rs), matched on its stable
 * opening so a reworded tail cannot silently turn a sick engine into "try the
 * next clip".
 */
export const ENGINE_STUCK_PREFIX = "The speech engine is still busy";

/**
 * Does a retry's error say the ENGINE is unhealthy (as opposed to this clip)?
 * Yes for ENGINE_STUCK, for a retry that ran out of time (the decode is still
 * running on the one engine — RetryOutcome's "a retry ran out of time"), for a
 * retry that never got the engine from a live dictation, and for a fired
 * watchdog. Anything else is this clip's own failure.
 */
export function isEngineUnhealthy(message: string, watchdogExpired = false): boolean {
  if (watchdogExpired) return true;
  const m = message.trim();
  return m.startsWith(ENGINE_STUCK_PREFIX) || m === RETRY_DEADLINE_ERROR || m === RETRY_ENGINE_BUSY_ERROR;
}

export interface RetryAllResult {
  recovered: number;
  failed: number;
  /** Rows the queue never reached (stopped for the engine, or cancelled). */
  remaining: number;
  /** True when the queue stopped because the engine was not healthy. */
  stoppedForEngine: boolean;
}

/**
 * Retry every row, ONE AT A TIME: each retry is awaited before the next is
 * started, so the one speech engine never has two clips queued from here. A
 * clip that fails moves on to the next; an unhealthy engine stops the queue —
 * piling more clips onto a stuck engine is the 2026-11-06 pile-up again.
 * `onProgress(done, total)` fires before each row; `cancelled()` is checked
 * between rows.
 */
export async function runRetryAll(
  ids: readonly string[],
  retryOne: (id: string) => Promise<RetryOutcome>,
  onProgress: (done: number, total: number) => void = () => {},
  cancelled: () => boolean = () => false,
): Promise<RetryAllResult> {
  const result: RetryAllResult = { recovered: 0, failed: 0, remaining: 0, stoppedForEngine: false };
  for (let i = 0; i < ids.length; i++) {
    if (cancelled()) {
      result.remaining = ids.length - i;
      return result;
    }
    onProgress(i, ids.length);
    const outcome = await retryOne(ids[i]);
    if (outcome === "recovered") result.recovered++;
    else if (outcome === "failed") result.failed++;
    else {
      // engine-busy, or another retry holds the engine: this row and the rest wait.
      result.stoppedForEngine = outcome === "engine-busy";
      result.remaining = ids.length - i;
      return result;
    }
  }
  return result;
}

/** The Retry-all button's label. */
export function retryAllLabel(a: { running: boolean; done: number; total: number }): string {
  if (!a.running) return "Retry all";
  return `Retrying ${Math.min(a.done + 1, a.total)} of ${a.total}…`;
}

/** The one sentence a finished Retry all shows — what happened and what to do next. */
export function retryAllSummary(r: RetryAllResult): string {
  const parts: string[] = [];
  if (r.recovered > 0) parts.push(`Recovered ${r.recovered} ${r.recovered === 1 ? "clip" : "clips"}`);
  if (r.failed > 0) parts.push(`${r.failed} couldn't be transcribed`);
  const head = parts.length > 0 ? parts.join(", ") : "Nothing was recovered";
  if (r.stoppedForEngine) return `${head}. The speech engine is busy — Retry all again in a minute`;
  if (r.remaining > 0) return `${head}. ${r.remaining} still waiting — Retry all to continue`;
  return `${head}.`;
}
