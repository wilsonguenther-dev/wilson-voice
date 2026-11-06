// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ENGINE_STUCK_PREFIX,
  type RetryOutcome,
  isEngineUnhealthy,
  retryAllLabel,
  retryAllSummary,
  runRetryAll,
  RETRY_DEADLINE_MESSAGE,
  RETRY_QUEUED_LABEL,
  RETRY_WATCHDOG_GRACE_MS,
  createRetryWatchdog,
  retryButtonLabel,
  retryDeadlineMs,
} from "./retryDictation";

describe("retryDeadlineMs mirrors lib.rs retry_deadline", () => {
  it("is 15 s plus the clip, floored at 30 s and capped at 180 s", () => {
    expect(retryDeadlineMs(5)).toBe(30_000);
    expect(retryDeadlineMs(15)).toBe(30_000);
    expect(retryDeadlineMs(82.3)).toBe(97_300);
    expect(retryDeadlineMs(153.1)).toBe(168_100);
    expect(retryDeadlineMs(600)).toBe(180_000);
  });
  it("gives a bad length the floor", () => {
    expect(retryDeadlineMs(0)).toBe(30_000);
    expect(retryDeadlineMs(-4)).toBe(30_000);
    expect(retryDeadlineMs(Number.NaN)).toBe(30_000);
  });
});

describe("retryButtonLabel", () => {
  it("rests on Retry / Finish transcribing", () => {
    expect(retryButtonLabel({ retrying: false, phase: null, resumable: false })).toBe("Retry");
    expect(retryButtonLabel({ retrying: false, phase: null, resumable: true })).toBe("Finish transcribing");
  });
  it("says it is waiting for the engine while queued, whatever the row", () => {
    expect(retryButtonLabel({ retrying: true, phase: "queued", resumable: false })).toBe(RETRY_QUEUED_LABEL);
    expect(retryButtonLabel({ retrying: true, phase: "queued", resumable: true })).toBe(RETRY_QUEUED_LABEL);
  });
  it("shows the loading words while decoding (and before the first phase event)", () => {
    expect(retryButtonLabel({ retrying: true, phase: "decoding", resumable: false })).toBe("Retrying…");
    expect(retryButtonLabel({ retrying: true, phase: null, resumable: true })).toBe("Finishing…");
  });
});

describe("the retry watchdog: the button never stays loading past the deadline", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  /** The shell's state, as Home reads it: who is retrying, and what the button says. */
  function shell() {
    const state = { retrying: null as string | null, phase: null as "queued" | "decoding" | null, toast: null as string | null };
    const watchdog = createRetryWatchdog(() => {
      state.retrying = null;
      state.phase = null;
      state.toast = RETRY_DEADLINE_MESSAGE;
    });
    const label = () => retryButtonLabel({ retrying: state.retrying === "row", phase: state.phase, resumable: false });
    return { state, watchdog, label };
  }

  it("a backend that never answers: loading ends at the deadline with the plain sentence", () => {
    const { state, watchdog, label } = shell();
    state.retrying = "row";
    watchdog.arm(retryDeadlineMs(82.3) + RETRY_WATCHDOG_GRACE_MS);
    expect(label()).toBe("Retrying…");
    vi.advanceTimersByTime(retryDeadlineMs(82.3) + RETRY_WATCHDOG_GRACE_MS - 1);
    expect(label()).toBe("Retrying…");
    vi.advanceTimersByTime(1);
    expect(watchdog.expired).toBe(true);
    expect(label()).toBe("Retry");
    expect(state.toast).toBe(RETRY_DEADLINE_MESSAGE);
  });

  it("queued → decoding → the backend's deadline re-arms the clock, and an answer clears it", () => {
    const { state, watchdog, label } = shell();
    state.retrying = "row";
    watchdog.arm(retryDeadlineMs(10) + RETRY_WATCHDOG_GRACE_MS);
    state.phase = "queued";
    expect(label()).toBe(RETRY_QUEUED_LABEL);
    // The backend's first phase event carries ITS deadline: longer than the
    // row's speech length suggested (the clip is longer than its speech).
    vi.advanceTimersByTime(20_000);
    watchdog.arm(60_000 + RETRY_WATCHDOG_GRACE_MS);
    state.phase = "decoding";
    expect(label()).toBe("Retrying…");
    vi.advanceTimersByTime(30_000);
    expect(label()).toBe("Retrying…");
    // The command answered: the clock stops and never fires.
    watchdog.clear();
    state.retrying = null;
    vi.advanceTimersByTime(10 * 60_000);
    expect(watchdog.expired).toBe(false);
    expect(state.toast).toBeNull();
    expect(label()).toBe("Retry");
  });

  it("fires once: a late re-arm after expiry is ignored, and clear() is safe twice", () => {
    let fired = 0;
    const watchdog = createRetryWatchdog(() => {
      fired += 1;
    });
    watchdog.clear();
    watchdog.arm(100);
    vi.advanceTimersByTime(100);
    expect(fired).toBe(1);
    watchdog.arm(100);
    vi.advanceTimersByTime(1000);
    expect(fired).toBe(1);
    watchdog.clear();
    watchdog.clear();
    expect(watchdog.expired).toBe(true);
  });
});

// ── yap-fx-07 — Retry all ──────────────────────────────────────────────────

describe("isEngineUnhealthy", () => {
  it("matches the backend's ENGINE_STUCK sentence, verbatim from transcription.rs", () => {
    const rs = readFileSync(new URL("../src-tauri/src/transcription.rs", import.meta.url), "utf8") as string;
    expect(rs).toContain(`"${ENGINE_STUCK_PREFIX}`);
    expect(isEngineUnhealthy(`${ENGINE_STUCK_PREFIX} with an earlier clip — try again in a minute`)).toBe(true);
  });
  it("treats a clip's own failure as the clip's, and a watchdog expiry as the engine's", () => {
    expect(isEngineUnhealthy("No speech detected in that clip")).toBe(false);
    expect(isEngineUnhealthy("No speech detected in that clip", true)).toBe(true);
  });
});

describe("runRetryAll runs one at a time on a healthy engine", () => {
  it("awaits each retry before starting the next", async () => {
    const order: string[] = [];
    let inFlight = 0;
    let maxInFlight = 0;
    const progress: Array<[number, number]> = [];
    const result = await runRetryAll(
      ["b1d90963", "882c587d", "c3"],
      async (id) => {
        inFlight++;
        maxInFlight = Math.max(maxInFlight, inFlight);
        order.push(`start ${id}`);
        await new Promise((r) => setTimeout(r, 1));
        order.push(`end ${id}`);
        inFlight--;
        return "recovered";
      },
      (done, total) => progress.push([done, total]),
    );
    expect(maxInFlight).toBe(1);
    expect(order).toEqual([
      "start b1d90963",
      "end b1d90963",
      "start 882c587d",
      "end 882c587d",
      "start c3",
      "end c3",
    ]);
    expect(progress).toEqual([
      [0, 3],
      [1, 3],
      [2, 3],
    ]);
    expect(result).toEqual({ recovered: 3, failed: 0, remaining: 0, stoppedForEngine: false });
  });

  it("moves past a clip that fails", async () => {
    const outcomes: Record<string, RetryOutcome> = { a: "failed", b: "recovered" };
    const result = await runRetryAll(["a", "b"], async (id) => outcomes[id]);
    expect(result).toEqual({ recovered: 1, failed: 1, remaining: 0, stoppedForEngine: false });
  });

  it("stops the queue when the engine is not healthy", async () => {
    const seen: string[] = [];
    const result = await runRetryAll(["a", "b", "c"], async (id) => {
      seen.push(id);
      return id === "b" ? "engine-busy" : "recovered";
    });
    expect(seen).toEqual(["a", "b"]);
    expect(result).toEqual({ recovered: 1, failed: 0, remaining: 2, stoppedForEngine: true });
  });

  it("stops without blaming the engine when another retry holds it", async () => {
    const result = await runRetryAll(["a", "b"], async () => "skipped");
    expect(result).toEqual({ recovered: 0, failed: 0, remaining: 2, stoppedForEngine: false });
  });

  it("stops between rows when cancelled", async () => {
    let calls = 0;
    const result = await runRetryAll(
      ["a", "b", "c"],
      async () => {
        calls++;
        return "recovered";
      },
      undefined,
      () => calls >= 1,
    );
    expect(calls).toBe(1);
    expect(result).toEqual({ recovered: 1, failed: 0, remaining: 2, stoppedForEngine: false });
  });

  it("does nothing for an empty list", async () => {
    expect(await runRetryAll([], async () => "recovered")).toEqual({
      recovered: 0,
      failed: 0,
      remaining: 0,
      stoppedForEngine: false,
    });
  });
});

describe("retryAllLabel / retryAllSummary", () => {
  it("labels the idle and running button", () => {
    expect(retryAllLabel({ running: false, done: 0, total: 3 })).toBe("Retry all");
    expect(retryAllLabel({ running: true, done: 0, total: 3 })).toBe("Retrying 1 of 3…");
    expect(retryAllLabel({ running: true, done: 3, total: 3 })).toBe("Retrying 3 of 3…");
  });
  it("is one sentence with one next step", () => {
    const base = { recovered: 0, failed: 0, remaining: 0, stoppedForEngine: false };
    expect(retryAllSummary({ ...base, recovered: 1 })).toBe("Recovered 1 clip.");
    expect(retryAllSummary({ ...base, recovered: 2, failed: 1 })).toBe(
      "Recovered 2 clips, 1 couldn't be transcribed.",
    );
    expect(retryAllSummary({ ...base, failed: 2 })).toBe("2 couldn't be transcribed.");
    expect(retryAllSummary({ ...base, remaining: 2, stoppedForEngine: true })).toBe(
      "Nothing was recovered. The speech engine is busy — Retry all again in a minute",
    );
    expect(retryAllSummary({ ...base, recovered: 1, remaining: 2 })).toBe(
      "Recovered 1 clip. 2 still waiting — Retry all to continue",
    );
  });
});
