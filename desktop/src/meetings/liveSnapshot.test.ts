import { describe, expect, it } from "vitest";
import { hasLiveLines, NO_LIVE, reduceLive } from "./liveSnapshot";

describe("reduceLive", () => {
  it("keeps a recording snapshot, trimming lines and dropping empties", () => {
    const s = reduceLive(
      { recording: true, tracks: [{ track: 1, speaker: "Call", lines: [" ok ", "  "] }] },
      true,
    );
    expect(s).toEqual({ recording: true, tracks: [{ track: 1, speaker: "Call", lines: ["ok"] }] });
    expect(hasLiveLines(s)).toBe(true);
  });

  it("tolerates missing fields from an older backend", () => {
    const s = reduceLive({ recording: true, tracks: [{ track: 0 } as never] }, true);
    expect(s.tracks[0]).toEqual({ track: 0, speaker: null, lines: [] });
    expect(reduceLive({ recording: true } as never, true).tracks).toEqual([]);
    expect(hasLiveLines(s)).toBe(false);
  });

  it("clears on a stopped snapshot, a stopped meeting, or nothing at all", () => {
    expect(reduceLive({ recording: false, tracks: [] }, true)).toBe(NO_LIVE);
    expect(reduceLive({ recording: true, tracks: [] }, false)).toBe(NO_LIVE);
    expect(reduceLive(null, true)).toBe(NO_LIVE);
  });
});
