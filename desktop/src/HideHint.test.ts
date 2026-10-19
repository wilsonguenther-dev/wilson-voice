// yap-lifecycle fix round 1 — review BLOCKING 2: the one-time hide hint was
// used up at the hide, before anything could show it (HideHint is not mounted
// during onboarding). The component now CLAIMS the hint: on mount, and on each
// nudge. These tests drive wireHideHint against a fake backend gate.
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { HIDE_HINT_EVENT, wireHideHint } from "./HideHint";

/** A fake of lifecycle::HideHintGate: hide = pending, claim = once. */
function fakeGate() {
  let pending = false;
  let marker = false;
  const listeners = new Map<string, () => void>();
  return {
    hide() {
      if (marker) return;
      pending = true;
      listeners.get(HIDE_HINT_EVENT)?.();
    },
    get marker() {
      return marker;
    },
    deps: {
      claim: async () => {
        if (marker || !pending) return false;
        pending = false;
        marker = true;
        return true;
      },
      listen: async (event: string, cb: () => void) => {
        listeners.set(event, cb);
        return () => listeners.delete(event);
      },
    },
  };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("the one-time hide hint is claimed by a mounted listener", () => {
  it("a red X during onboarding (nothing mounted) shows the hint when the main view mounts", async () => {
    const gate = fakeGate();
    gate.hide(); // onboarding: no HideHint mounted, the nudge reaches no one
    expect(gate.marker).toBe(false);
    const show = vi.fn();
    const stop = wireHideHint(gate.deps, show); // onboarding done, main view mounts
    await flush();
    expect(show).toHaveBeenCalledTimes(1);
    expect(gate.marker).toBe(true);
    stop();
  });

  it("a red X while mounted shows the hint once; later hides show nothing", async () => {
    const gate = fakeGate();
    const show = vi.fn();
    const stop = wireHideHint(gate.deps, show);
    await flush();
    expect(show).not.toHaveBeenCalled(); // no hide yet: mount claims nothing
    gate.hide();
    await flush();
    expect(show).toHaveBeenCalledTimes(1);
    gate.hide();
    await flush();
    expect(show).toHaveBeenCalledTimes(1);
    stop();
  });

  it("an unmounted HideHint never consumes the hint", async () => {
    const gate = fakeGate();
    const show = vi.fn();
    const stop = wireHideHint(gate.deps, show);
    await flush();
    stop();
    gate.hide();
    await flush();
    expect(show).not.toHaveBeenCalled();
    expect(gate.marker).toBe(false);
  });
});
