import { describe, expect, it } from "vitest";
import { testChipText } from "./buildChannel";

describe("testChipText", () => {
  it("is null on production — no chip ships in the real Yap", () => {
    expect(testChipText({ channel: "", sha: "7e3276c", branch: "main" })).toBeNull();
    expect(testChipText({ channel: "production", sha: "x", branch: "y" })).toBeNull();
  });
  it("names the short sha and branch on a test build", () => {
    expect(testChipText({ channel: "test", sha: "7e3276cdeadbeef", branch: "pr-240" })).toBe(
      "TEST BUILD 7e3276c · pr-240",
    );
  });
  it("falls back to 'local' with no sha and drops an empty branch", () => {
    expect(testChipText({ channel: "test", sha: "", branch: " " })).toBe("TEST BUILD local");
  });
});
