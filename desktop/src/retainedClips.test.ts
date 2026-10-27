import { describe, expect, it } from "vitest";
import { RETAINED_CLIP_DAYS, retainedClipsLabel } from "./appTypes";

// PRIV-B — retained audio has to be VISIBLE in History: the count and the
// window, in plain words. The window mirrors FAILED_TAKE_RETENTION_DAYS (7).
describe("retainedClipsLabel", () => {
  it("names the count and the retention window", () => {
    expect(retainedClipsLabel(3)).toBe("3 clips kept for 7 days");
  });
  it("singular for one clip", () => {
    expect(retainedClipsLabel(1)).toBe("1 clip kept for 7 days");
  });
  it("matches the Rust retention constant", () => {
    expect(RETAINED_CLIP_DAYS).toBe(7);
  });
});
