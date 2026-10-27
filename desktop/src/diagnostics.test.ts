import { describe, expect, it } from "vitest";
import {
  type DiagHealth,
  formatEvent,
  formatHealth,
  formatStat,
  formatTimestamp,
  levelTone,
  sortedStats,
} from "./diagnostics";

const health: DiagHealth = {
  permissions: {
    microphone: "authorized",
    accessibility: "authorized",
    input_monitoring: "denied",
    post_event: "authorized",
  },
  model: { selected: "base.en", loaded: true, loadedModel: "base.en" },
  hotkey: { registered: false, chord: "⌘⇧V", fnTap: "running" },
  meeting: { state: "processing", lastStage: "vad" },
  insert: { lastPath: "PasteBridge/none", lastOutcome: "pasted" },
};

describe("formatHealth", () => {
  it("renders every health input on one line, in the Rust order", () => {
    expect(formatHealth(health)).toBe(
      "mic=authorized ax=authorized input=denied post_event=authorized · model=base.en loaded=yes · hotkey=⌘⇧V registered=no · meeting=processing stage=vad · insert=PasteBridge/none outcome=pasted",
    );
  });
  it("marks an unknown permission with ? and no model as none", () => {
    const line = formatHealth({ ...health, permissions: {}, model: { selected: "", loaded: false, loadedModel: null } });
    expect(line).toContain("mic=? ax=? input=? post_event=?");
    expect(line).toContain("model=none loaded=no");
  });
});

describe("formatEvent", () => {
  it("prints time, level, module, message and fields in key order", () => {
    const line = formatEvent({
      tsMs: Date.UTC(2026, 9, 27, 19, 17, 40, 5),
      level: "warn",
      module: "paste",
      msg: "paste failed, showing the Copy card",
      fields: { reason: "timeout", attempt: "2" },
    });
    expect(line).toBe("19:17:40.005Z WARN paste paste failed, showing the Copy card attempt=2 reason=timeout");
  });
  it("has no trailing space when there are no fields", () => {
    expect(formatEvent({ tsMs: 0, level: "INFO", module: "m", msg: "x", fields: {} })).toBe("00:00:00.000Z INFO m x");
  });
  it("falls back to the raw number for an invalid time", () => {
    expect(formatTimestamp(Number.NaN)).toBe("NaN");
  });
});

describe("levelTone / formatStat / sortedStats", () => {
  it("maps levels to tones", () => {
    expect(levelTone("ERROR")).toBe("error");
    expect(levelTone("warn")).toBe("warn");
    expect(levelTone("INFO")).toBe("info");
    expect(levelTone("DEBUG")).toBe("info");
  });
  it("formats a stat and sorts by name", () => {
    expect(formatStat("paste.insert_ms", { last: 12, count: 3, max: 40 })).toBe("paste.insert_ms  last=12 count=3 max=40");
    const sorted = sortedStats({ "z.a": { last: 1, count: 1, max: 1 }, "a.b": { last: 2, count: 2, max: 2 } });
    expect(sorted.map(([k]) => k)).toEqual(["a.b", "z.a"]);
  });
});
