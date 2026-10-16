import { describe, expect, it } from "vitest";
import {
  STRUCTURE_CHOICES,
  structureEnabled,
  structureOverrideRows,
  withStructureOverride,
} from "./structureSettings";

describe("structure settings (yap-format-structure)", () => {
  it("the switch ships on and respects an explicit off", () => {
    expect(structureEnabled({})).toBe(true);
    expect(structureEnabled({ structureEnabled: false })).toBe(false);
  });

  it("adds, replaces and removes one app's override without touching the rest", () => {
    const a = withStructureOverride(undefined, " com.apple.mail ", "markdown");
    expect(a).toEqual({ "com.apple.mail": "markdown" });
    const b = withStructureOverride(a, "notion.id", "off");
    expect(b).toEqual({ "com.apple.mail": "markdown", "notion.id": "off" });
    expect(a).toEqual({ "com.apple.mail": "markdown" });
    expect(withStructureOverride(b, "com.apple.mail", null)).toEqual({ "notion.id": "off" });
    expect(withStructureOverride(b, "   ", "plain")).toEqual(b);
  });

  it("lists overrides sorted and drops values the backend would not accept", () => {
    expect(
      structureOverrideRows({ "notion.id": "off", "com.apple.mail": "plain", x: "bogus" }),
    ).toEqual([
      ["com.apple.mail", "plain"],
      ["notion.id", "off"],
    ]);
  });

  it("offers exactly the three backend values, in plain words", () => {
    expect(STRUCTURE_CHOICES.map(([id]) => id)).toEqual(["markdown", "plain", "off"]);
    for (const [, label] of STRUCTURE_CHOICES) expect(label).not.toMatch(/markdown|bundle/i);
  });
});
