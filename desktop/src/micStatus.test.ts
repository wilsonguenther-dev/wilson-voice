import { describe, expect, it, vi } from "vitest";
import {
  awaitMicDecision,
  isFinalMicStatus,
  normalizeMicStatus,
} from "./micStatus";

const noSleep = () => Promise.resolve();

describe("normalizeMicStatus", () => {
  it("passes the four AVFoundation statuses through", () => {
    for (const s of [
      "not_determined",
      "denied",
      "restricted",
      "authorized",
    ] as const) {
      expect(normalizeMicStatus(s)).toBe(s);
    }
  });

  it("never turns an unknown value into a grant", () => {
    for (const bad of [undefined, null, true, 3, "", "AUTHORIZED", "ok"]) {
      expect(normalizeMicStatus(bad)).toBe("restricted");
    }
  });
});

describe("awaitMicDecision", () => {
  it("does not poll at all when TCC already has a final answer", async () => {
    // This is the denial case: macOS returns immediately and never shows a
    // dialog, so the old fixed 900 ms wait was pure dead time.
    const read = vi.fn();
    const status = await awaitMicDecision({
      request: async () => "denied",
      read,
      sleep: noSleep,
    });
    expect(status).toBe("denied");
    expect(read).not.toHaveBeenCalled();
  });

  it("polls past not_determined until the human decides", async () => {
    const answers = [
      "not_determined",
      "not_determined",
      "not_determined",
      "authorized",
    ];
    const read = vi.fn(async () => answers.shift() ?? "authorized");
    const status = await awaitMicDecision({
      request: async () => "not_determined",
      read,
      sleep: noSleep,
    });
    expect(status).toBe("authorized");
    expect(read).toHaveBeenCalledTimes(4);
  });

  it("is bounded — an unanswered dialog does not poll forever", async () => {
    const read = vi.fn(async () => "not_determined");
    const status = await awaitMicDecision({
      request: async () => "not_determined",
      read,
      sleep: noSleep,
      maxPolls: 5,
    });
    expect(status).toBe("not_determined");
    expect(read).toHaveBeenCalledTimes(5);
  });

  it("isFinalMicStatus is true for everything except not_determined", () => {
    expect(isFinalMicStatus("not_determined")).toBe(false);
    expect(isFinalMicStatus("denied")).toBe(true);
    expect(isFinalMicStatus("restricted")).toBe(true);
    expect(isFinalMicStatus("authorized")).toBe(true);
  });
});
