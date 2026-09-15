import { describe, expect, it } from "vitest";
import {
  levelNeedsPolishModel,
  selectedSpeed,
  type CleanupLevelOption,
  type PolishSpeedOption,
} from "./formatting";

const SPEEDS: PolishSpeedOption[] = [
  { id: "fast", label: "Fast", description: "", deadlineMs: 600 },
  { id: "balanced", label: "Balanced", description: "", deadlineMs: 1200 },
  { id: "careful", label: "Careful", description: "", deadlineMs: 2500 },
];

describe("selectedSpeed", () => {
  it("lights the exact match", () => {
    expect(selectedSpeed(1200, SPEEDS)).toBe("balanced");
    expect(selectedSpeed(600, SPEEDS)).toBe("fast");
    expect(selectedSpeed(2500, SPEEDS)).toBe("careful");
  });

  it("lights the NEAREST row for a value no button writes", () => {
    // The bug this kills: a deadline written by an older build leaves every
    // button dark, so the screen claims the user has chosen nothing.
    expect(selectedSpeed(900, SPEEDS)).toBe("fast");
    expect(selectedSpeed(1300, SPEEDS)).toBe("balanced");
    expect(selectedSpeed(9999, SPEEDS)).toBe("careful");
    expect(selectedSpeed(1, SPEEDS)).toBe("fast");
  });

  it("falls back to balanced when the setting is missing", () => {
    expect(selectedSpeed(undefined, SPEEDS)).toBe("balanced");
  });

  it("selects nothing before the backend has answered", () => {
    expect(selectedSpeed(1200, [])).toBeUndefined();
  });
});

describe("levelNeedsPolishModel", () => {
  const LEVELS: CleanupLevelOption[] = [
    {
      id: "light",
      label: "Tidy it up",
      description: "",
      stages: ["dictionary"],
      needsPolishModel: false,
    },
    {
      id: "high",
      label: "Rewrite it properly",
      description: "",
      stages: ["dictionary", "polish"],
      needsPolishModel: true,
    },
  ];

  it("is true only for the level that runs the model", () => {
    expect(levelNeedsPolishModel("high", LEVELS)).toBe(true);
    expect(levelNeedsPolishModel("light", LEVELS)).toBe(false);
  });

  it("is false for an unknown level and before the fetch lands", () => {
    expect(levelNeedsPolishModel("medium", LEVELS)).toBe(false);
    expect(levelNeedsPolishModel("high", [])).toBe(false);
  });
});
