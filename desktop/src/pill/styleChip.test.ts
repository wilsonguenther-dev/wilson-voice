import { describe, expect, it } from "vitest";
import {
  chipAccessibleName, chipShows, HIDDEN_CHIP, minutesLeft, OVERRIDE_TTL_MS, parseChip,
  type StyleChip,
} from "./styleChip";

const SLACK: StyleChip = {
  visible: true,
  app: "Slack",
  mode: "casual",
  label: "Slack · casual",
  overridden: false,
  expiresInMs: null,
};

describe("WF-20 style chip", () => {
  it("shows while listening when app-aware modes are on", () => {
    expect(chipShows(SLACK, "listening")).toBe(true);
  });

  it("hides when the user has app-aware modes off", () => {
    // The backend sends visible:false for a fixed Formatting mode.
    expect(chipShows({ ...SLACK, visible: false }, "listening")).toBe(false);
    expect(chipShows(HIDDEN_CHIP, "listening")).toBe(false);
    expect(chipShows(null, "listening")).toBe(false);
  });

  it("hides outside listening, including the hands-free hint row", () => {
    for (const phase of ["idle", "thinking", "done", "listening-locked"] as const) {
      expect(chipShows(SLACK, phase)).toBe(false);
    }
    expect(chipShows({ ...SLACK, label: "" }, "listening")).toBe(false);
  });

  it("names what a click does, and the time left on a pick", () => {
    expect(chipAccessibleName(SLACK)).toBe(
      "Style for Slack: casual. Click to change it for this app for 15 minutes.",
    );
    const picked = { ...SLACK, mode: "plain", overridden: true, expiresInMs: 10 * 60000 - 1 };
    expect(chipAccessibleName(picked)).toBe(
      "Style for Slack: plain. Changed for this app, 10 min left. Click to change again.",
    );
    expect(chipAccessibleName({ ...picked, expiresInMs: null })).toContain("15 minutes");
  });

  it("rounds minutes up and never shows zero", () => {
    expect(minutesLeft(null)).toBe(null);
    expect(minutesLeft(0)).toBe(null);
    expect(minutesLeft(1)).toBe(1);
    expect(minutesLeft(OVERRIDE_TTL_MS)).toBe(15);
  });

  it("reads a malformed payload as hidden", () => {
    expect(parseChip(null)).toEqual(HIDDEN_CHIP);
    expect(parseChip("x")).toEqual(HIDDEN_CHIP);
    expect(parseChip({ visible: "yes", label: "a" })).toEqual(HIDDEN_CHIP);
    expect(parseChip({ visible: true, label: "Slack · casual" })).toEqual({
      ...HIDDEN_CHIP, visible: true, label: "Slack · casual",
    });
    expect(parseChip({ ...SLACK, overridden: true, expiresInMs: 5 })).toEqual({
      ...SLACK, overridden: true, expiresInMs: 5,
    });
  });
});

describe("WF-20 style chip matrix scene", () => {
  it("forces the sample chip only when the URL asks", async () => {
    const { forcedStyleChip, SAMPLE_STYLE_CHIP } = await import("./styleChip");
    expect(forcedStyleChip("?phase=listening&styleChip=1")).toEqual(SAMPLE_STYLE_CHIP);
    expect(forcedStyleChip("?phase=listening")).toBe(null);
    expect(chipShows(SAMPLE_STYLE_CHIP, "listening")).toBe(true);
  });
});
