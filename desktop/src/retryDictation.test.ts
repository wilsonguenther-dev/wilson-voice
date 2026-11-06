import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
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
