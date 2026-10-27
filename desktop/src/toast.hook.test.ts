/**
 * Y5-F — `useToasts`, the React binding of the toast queue, driven for real.
 *
 * This suite has no DOM (no jsdom, no react-test-renderer) and
 * `renderToStaticMarkup` never runs an effect, so the ONE timer that makes a
 * toast auto-dismiss had no test: `npm test -- toast` left toast.ts's hook
 * uncovered and failed its own coverage floor.
 *
 * Here `react` is replaced by a minimal hook runtime that keeps React's
 * contract for exactly the three hooks the binding uses — `useReducer` keeps
 * its state across renders and a dispatch re-renders; `useEffect` runs after
 * the render, and re-runs (cleaning up the previous run first) only when a dep
 * changed; `useCallback` keeps the first function while its deps hold. Fake
 * timers (which fake `Date.now` too) then drive the REAL `useToasts`.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const rt = vi.hoisted(() => {
  type Slot = { value?: unknown; dispatch?: (a: unknown) => void; deps?: readonly unknown[]; cleanup?: unknown };
  const st = { slots: [] as Slot[], i: 0, effects: [] as (() => void)[], component: null as null | (() => unknown), out: undefined as unknown };
  const changed = (a: readonly unknown[] | undefined, b: readonly unknown[]) =>
    !a || a.length !== b.length || a.some((v, k) => !Object.is(v, b[k]));
  function render() {
    st.i = 0;
    st.out = st.component!();
    const effects = st.effects;
    st.effects = [];
    for (const run of effects) run();
  }
  function slot(): Slot {
    const i = st.i++;
    return (st.slots[i] ??= {});
  }
  return {
    useReducer<S, A>(reducer: (s: S, a: A) => S, init: S): [S, (a: A) => void] {
      const s = slot();
      if (!s.dispatch) {
        s.value = init;
        s.dispatch = (a) => {
          s.value = reducer(s.value as S, a as A);
          render();
        };
      }
      return [s.value as S, s.dispatch as (a: A) => void];
    },
    useEffect(fn: () => void | (() => void), deps: readonly unknown[]) {
      const s = slot();
      if (!changed(s.deps, deps)) return;
      s.deps = deps;
      st.effects.push(() => {
        if (typeof s.cleanup === "function") s.cleanup();
        s.cleanup = fn();
      });
    },
    useCallback<F>(fn: F, deps: readonly unknown[]): F {
      const s = slot();
      if (changed(s.deps, deps)) {
        s.deps = deps;
        s.value = fn;
      }
      return s.value as F;
    },
    mount<T>(component: () => T): { current: () => T } {
      st.slots = [];
      st.effects = [];
      st.component = component;
      render();
      return { current: () => st.out as T };
    },
    unmount() {
      for (const s of st.slots) if (typeof s.cleanup === "function") s.cleanup();
      st.slots = [];
      st.component = null;
    },
  };
});

vi.mock("react", () => ({ useReducer: rt.useReducer, useEffect: rt.useEffect, useCallback: rt.useCallback }));

import { toastDuration, useToasts } from "./toast";

const T0 = 1_000_000;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(T0);
});

afterEach(() => {
  rt.unmount();
  vi.useRealTimers();
});

describe("useToasts", () => {
  it("a pushed toast auto-dismisses on its own duration, with nothing else ticking", () => {
    const hook = rt.mount(useToasts);
    expect(vi.getTimerCount()).toBe(0);
    hook.current().push({ line: "Copied" });
    expect(hook.current().toasts.map((t) => t.line)).toEqual(["Copied"]);
    expect(vi.getTimerCount()).toBe(1);

    vi.advanceTimersByTime(toastDuration("Copied") - 1);
    expect(hook.current().toasts).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(hook.current().toasts).toHaveLength(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("keeps ONE timer, re-armed for the next expiry as the queue moves up", () => {
    const hook = rt.mount(useToasts);
    const lines = ["one", "The speech engine could not transcribe that take.", "three"];
    for (const line of lines) hook.current().push({ line });
    expect(hook.current().toasts.map((t) => t.line)).toEqual(lines.slice(0, 2));
    expect(vi.getTimerCount()).toBe(1);

    // "one" is the shortest of the two visible, so it goes first and "three" moves up.
    vi.advanceTimersByTime(toastDuration("one"));
    expect(hook.current().toasts.map((t) => t.line)).toEqual(lines.slice(1));
    expect(vi.getTimerCount()).toBe(1);

    // The long line was shown at T0 and "three" only now, so the long line
    // still expires first: the timer follows the queue, not push order.
    const longDue = toastDuration(lines[1]);
    const threeDue = toastDuration("one") + toastDuration("three");
    expect(longDue).toBeLessThan(threeDue);
    vi.advanceTimersByTime(longDue - toastDuration("one"));
    expect(hook.current().toasts.map((t) => t.line)).toEqual(["three"]);
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(threeDue - longDue);
    expect(hook.current().toasts).toHaveLength(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("manual dismiss and dismissMatching take the toast down and its timer with it", () => {
    const hook = rt.mount(useToasts);
    const { push, dismiss, dismissMatching } = hook.current();
    push({ line: "Retry that take", key: "transcription_failed", action: { kind: "retry", label: "Retry", arg: "row-1" } });
    push({ line: "Copied" });
    // Stable across renders: the shell passes these to effects and children.
    expect(hook.current().push).toBe(push);

    dismissMatching((t) => t.action?.arg === "row-1");
    expect(hook.current().toasts.map((t) => t.line)).toEqual(["Copied"]);
    dismiss(hook.current().toasts[0].id);
    expect(hook.current().toasts).toHaveLength(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("unmounting clears the pending timer", () => {
    const hook = rt.mount(useToasts);
    hook.current().push({ line: "Copied" });
    expect(vi.getTimerCount()).toBe(1);
    rt.unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});
