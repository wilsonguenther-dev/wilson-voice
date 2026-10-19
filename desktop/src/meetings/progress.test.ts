import { describe, expect, it } from "vitest";
import { summaryTitle, canRetry, foldProgress, meetingStateLabel, type MeetingProgressEvent } from "./progress";

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
  it("complete says Notes ready, and never asks for a summary model", () => {
    expect(meetingStateLabel("complete", undefined, true)).toBe("Notes ready");
    expect(meetingStateLabel("complete", undefined, false)).toBe("Transcript ready · Notes on the way");
  });
  it("card walks transcript-ready → (model fetch) → summarizing → summary shown", () => {
    const p = (state: string, percent: number) => ({ id: "m1", state, percent, processedThroughSeconds: 0, durationSeconds: 60, error: null });
    const labels = [
      meetingStateLabel("complete", undefined, false),
      meetingStateLabel("complete", p("preparing_notes", 42) as never, false),
      meetingStateLabel("summarizing", p("summarizing", 0) as never, false),
      meetingStateLabel("complete", p("complete", 100) as never, true),
    ];
    expect(labels).toEqual([
      "Transcript ready · Notes on the way",
      "Transcript ready · Getting the notes model 42%",
      "Writing notes",
      "Notes ready",
    ]);
    for (const l of labels) expect(l.toLowerCase()).not.toContain("add a summary model");
    expect(summaryTitle("## Pricing review\n\n### Summary\n\nWe met.")).toBe("Pricing review");
    expect(summaryTitle("plain")).toBeNull();
  });
  it("failed and partial rows offer Retry", () => {
    expect(canRetry("failed")).toBe(true);
    expect(canRetry("partial")).toBe(true);
    expect(canRetry("complete")).toBe(false);
    expect(canRetry("transcribing")).toBe(false);
  });
});
