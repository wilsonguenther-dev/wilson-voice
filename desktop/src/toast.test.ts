import { describe, expect, it } from "vitest";
import {
  EMPTY_TOASTS, MAX_TOAST_MS, MIN_TOAST_MS, QUEUE_LIMIT, TOAST_CAP, dismissToast, dismissWhere,
  expireToasts, nextExpiry, pushToast, toastDuration, type ToastState,
} from "./toast";

const T0 = 1_000;
const push = (s: ToastState, line: string, key?: string, now = T0) => pushToast(s, { line, key }, now);

describe("toast queue", () => {
  it("queues past the cap instead of stacking a third toast", () => {
    let s = EMPTY_TOASTS;
    s = push(s, "one");
    s = push(s, "two");
    s = push(s, "three");
    expect(TOAST_CAP).toBeLessThan(3);
    expect(s.visible.map((t) => t.line)).toEqual(["one", "two"]);
    expect(s.queued.map((t) => t.line)).toEqual(["three"]);
  });

  it("promotes the next waiting toast when one is dismissed by hand", () => {
    let s = EMPTY_TOASTS;
    for (const l of ["one", "two", "three"]) s = push(s, l);
    s = dismissToast(s, s.visible[0].id, T0 + 10);
    expect(s.visible.map((t) => t.line)).toEqual(["two", "three"]);
    expect(s.visible[1].shownAt).toBe(T0 + 10);
    expect(s.queued).toHaveLength(0);
  });
});

describe("toast dedupe", () => {
  it("the same code twice refreshes the toast that is up and counts it", () => {
    let s = EMPTY_TOASTS;
    s = push(s, "The speech engine failed", "transcription_failed");
    s = push(s, "The speech engine failed", "transcription_failed", T0 + 500);
    expect(s.visible).toHaveLength(1);
    expect(s.visible[0].count).toBe(2);
    expect(s.visible[0].shownAt).toBe(T0 + 500);
  });

  it("dedupes a waiting toast in place rather than queueing it twice", () => {
    let s = EMPTY_TOASTS;
    s = push(s, "a");
    s = push(s, "b");
    s = push(s, "c", "code_c");
    s = push(s, "c again", "code_c");
    expect(s.queued).toHaveLength(1);
    expect(s.queued[0].line).toBe("c again");
    expect(s.queued[0].count).toBe(2);
  });

  it("dismissWhere drops a toast whose row no longer exists", () => {
    let s = pushToast(EMPTY_TOASTS, { line: "x", action: { kind: "retry", label: "Retry", arg: "row-1" } }, T0);
    s = dismissWhere(s, (t) => t.action?.arg === "row-1", T0);
    expect(s.visible).toHaveLength(0);
  });
});

describe("toast cap", () => {
  it("a storm of distinct failures never shows more than the cap or waits more than the limit", () => {
    let s = EMPTY_TOASTS;
    for (let i = 0; i < 50; i++) s = push(s, `failure ${i}`);
    expect(s.visible).toHaveLength(TOAST_CAP);
    expect(s.queued).toHaveLength(QUEUE_LIMIT);
    // The newest failures are the ones still waiting; the oldest were dropped.
    expect(s.queued[QUEUE_LIMIT - 1].line).toBe("failure 49");
  });

  it("a storm of the SAME failure is one toast", () => {
    let s = EMPTY_TOASTS;
    for (let i = 0; i < 50; i++) s = push(s, "same", "take_failed");
    expect(s.visible).toHaveLength(1);
    expect(s.queued).toHaveLength(0);
    expect(s.visible[0].count).toBe(50);
  });
});

describe("toast duration", () => {
  it("is proportional to length, clamped", () => {
    const short = toastDuration("Copied");
    const mid = toastDuration("The speech engine could not transcribe that take.");
    const long = toastDuration("x".repeat(2000));
    expect(short).toBe(MIN_TOAST_MS);
    expect(mid).toBeGreaterThan(short);
    expect(long).toBe(MAX_TOAST_MS);
  });

  it("gives a toast with a button longer to reach it", () => {
    const line = "No speech model installed — download one to start dictating.";
    expect(toastDuration(line, true)).toBeGreaterThan(toastDuration(line, false));
  });

  it("auto-dismisses on its own duration and lets the queue move up", () => {
    let s = EMPTY_TOASTS;
    for (const l of ["one", "two", "three"]) s = push(s, l);
    const due = nextExpiry(s)!;
    expect(due).toBe(T0 + toastDuration("one"));
    expect(expireToasts(s, due - 1).visible).toHaveLength(2);
    s = expireToasts(s, due);
    expect(s.visible.map((t) => t.line)).toEqual(["three"]);
    expect(nextExpiry(s)).toBe(due + toastDuration("three"));
    s = expireToasts(s, due + toastDuration("three"));
    expect(s.visible).toHaveLength(0);
    expect(nextExpiry(s)).toBeNull();
  });
});
