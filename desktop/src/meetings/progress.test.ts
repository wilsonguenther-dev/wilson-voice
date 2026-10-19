import { describe, expect, it } from "vitest";
import {
  canRetry, foldProgress, meetingStateLabel, summaryFirstLine, transcribedLabel, transcribePercent,
  transcribeProgress, type MeetingProgressEvent,
} from "./progress";

const ev = (percent: number, state = "transcribing"): MeetingProgressEvent => ({
  id: "m1",
  state,
  percent,
  processedThroughSeconds: percent,
  durationSeconds: 100,
});

describe("meeting progress", () => {
  it("shows Transcribing N% from the event, never 100 before it is done", () => {
    expect(meetingStateLabel("transcribing", ev(42), false)).toBe("Transcribing 42%");
    expect(meetingStateLabel("transcribing", ev(100), false)).toBe("Transcribing 99%");
    expect(meetingStateLabel("transcribing", undefined, false)).toBe("Transcribing 0%");
  });
  it("percent is monotonic while transcribing", () => {
    let m = foldProgress({}, ev(40));
    m = foldProgress(m, ev(30));
    expect(m.m1.percent).toBe(40);
    m = foldProgress(m, ev(100, "complete"));
    expect(m.m1.state).toBe("complete");
  });
  it("complete says Notes ready, or names the missing summary model as a state", () => {
    expect(meetingStateLabel("complete", undefined, true)).toBe("Notes ready");
    expect(meetingStateLabel("complete", undefined, false)).toContain("Add a summary model in Settings");
  });
  it("failed and partial rows offer Retry", () => {
    expect(canRetry("failed")).toBe(true);
    expect(canRetry("partial")).toBe(true);
    expect(canRetry("complete")).toBe(false);
    expect(canRetry("transcribing")).toBe(false);
  });
});

describe("yap-uiux-2026-10-16-M1 — transcribing rows report how far they got", () => {
  const row = { state: "transcribing", durationSeconds: 1500, processedThroughSeconds: 500 };
  it("falls back to the row's processedThroughSeconds when no event has arrived", () => {
    const p = transcribeProgress(row, undefined);
    expect(p).toEqual({ processedSeconds: 500, durationSeconds: 1500 });
    expect(transcribedLabel(p!)).toBe("Transcribed 8 of 25 min");
    expect(transcribePercent(p!)).toBe(33);
  });
  it("prefers a fresher live event, and never goes backwards from the row", () => {
    const live: MeetingProgressEvent = { id: "m", state: "transcribing", percent: 60, processedThroughSeconds: 900, durationSeconds: 1500 };
    expect(transcribeProgress(row, live)?.processedSeconds).toBe(900);
    const stale = { ...live, processedThroughSeconds: 100 };
    expect(transcribeProgress(row, stale)?.processedSeconds).toBe(500);
  });
  it("is null for any other state or an unknown duration", () => {
    expect(transcribeProgress({ ...row, state: "complete" }, undefined)).toBeNull();
    expect(transcribeProgress({ ...row, durationSeconds: 0 }, undefined)).toBeNull();
  });
  it("short meetings count in seconds and never reach 100%", () => {
    const p = { processedSeconds: 45, durationSeconds: 45 };
    expect(transcribedLabel(p)).toBe("Transcribed 45 of 45 s");
    expect(transcribePercent(p)).toBe(99);
  });
});

describe("yap-uiux-2026-10-16-M2 — a compact row shows the notes' first line", () => {
  it("takes the first non-empty line and strips list/heading marks", () => {
    expect(summaryFirstLine("\n## Decisions\n- ship it")).toBe("Decisions");
    expect(summaryFirstLine("- agreed on a date\n- next step")).toBe("agreed on a date");
    expect(summaryFirstLine("Plain sentence.")).toBe("Plain sentence.");
  });
  it("is null with no notes", () => {
    expect(summaryFirstLine(null)).toBeNull();
    expect(summaryFirstLine("  \n ")).toBeNull();
  });
});
