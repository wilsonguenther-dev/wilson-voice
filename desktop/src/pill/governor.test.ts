import { describe, expect, it } from "vitest";
import {
  GOVERNOR_FULL,
  governorChip,
  governorMainBanner,
  governorSentence,
  pauseBelowFromInput,
  type GovernorView,
} from "./governor";

const light: GovernorView = {
  ...GOVERNOR_FULL,
  level: "light",
  reason: "battery",
  batteryPercent: 15,
  onAc: false,
  sentence: "Yap is going easy: 15% battery",
  pillLabel: "Saving battery: notes after you plug in",
  mainSentence: "Yap is saving power — your meeting will transcribe when you're charged",
  heavyWorkDeferred: true,
  waitingJobs: ["m1"],
};

describe("governorChip", () => {
  it("draws nothing at full", () => {
    expect(governorChip(GOVERNOR_FULL)).toBeNull();
    expect(governorChip(null)).toBeNull();
  });
  it("draws the backend's label and offers Run anyway only when a job waits", () => {
    expect(governorChip(light)).toEqual({
      label: "Saving battery: notes after you plug in",
      tone: "light",
      runAnyway: true,
    });
    expect(governorChip({ ...light, waitingJobs: [] })?.runAnyway).toBe(false);
    expect(governorChip({ ...light, level: "minimal" })?.tone).toBe("minimal");
  });
  it("a thermal light with nothing waiting has no Run anyway", () => {
    const hot: GovernorView = {
      ...GOVERNOR_FULL,
      level: "light",
      reason: "thermal",
      pillLabel: "Running hot: notes when this Mac cools",
      heavyWorkDeferred: true,
    };
    expect(governorChip(hot)).toEqual({ label: "Running hot: notes when this Mac cools", tone: "light", runAnyway: false });
  });
});

describe("governorMainBanner", () => {
  it("shows the backend's one sentence only while meeting work is held", () => {
    expect(governorMainBanner(light)).toBe(
      "Yap is saving power — your meeting will transcribe when you're charged",
    );
    expect(governorMainBanner({ ...light, heavyWorkDeferred: false })).toBeNull();
    expect(governorMainBanner(GOVERNOR_FULL)).toBeNull();
    expect(governorMainBanner(null)).toBeNull();
    expect(governorMainBanner({ ...light, mainSentence: null })).toBeNull();
  });
});

describe("governorSentence / pauseBelowFromInput", () => {
  it("reads the backend sentence, with a quiet default", () => {
    expect(governorSentence(light)).toBe("Yap is going easy: 15% battery");
    expect(governorSentence(GOVERNOR_FULL)).toBe("Yap is running at full speed.");
  });
  it("clamps the percent like the backend", () => {
    expect(pauseBelowFromInput("0", 20)).toBe(1);
    expect(pauseBelowFromInput("150", 20)).toBe(99);
    expect(pauseBelowFromInput("35", 20)).toBe(35);
    expect(pauseBelowFromInput("", 20)).toBe(20);
  });
});
