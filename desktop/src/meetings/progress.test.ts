import { describe, expect, it } from "vitest";
import {
  summaryTitle, canRetry, canCancelNotes, canRetryNotes, foldProgress, meetingStateLabel, notesStatusEvent,
  reloadsMeetings, retryNotesLabel, OFFLINE_REASON, type MeetingProgressEvent,
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
  it("complete says Notes ready, and never asks for a summary model", () => {
    expect(meetingStateLabel("complete", undefined, true)).toBe("Notes ready");
    // #250 round 3 (B1): no job known → no promise.
    expect(meetingStateLabel("complete", undefined, false)).toBe("Transcript ready");
  });
  it("card walks transcript-ready → (model fetch) → summarizing → summary shown", () => {
    const p = (state: string, percent: number) => ({ id: "m1", state, percent, processedThroughSeconds: 0, durationSeconds: 60, error: null });
    const labels = [
      meetingStateLabel("complete", p("notes_queued", 0) as never, false),
      meetingStateLabel("complete", p("preparing_notes", 42) as never, false),
      meetingStateLabel("summarizing", p("summarizing", 0) as never, false),
      meetingStateLabel("complete", p("notes_ready", 100) as never, true),
    ];
    expect(labels).toEqual([
      "Transcript ready · Notes queued",
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
  // #250 round 3 (B1) — every failure path is a visible state with Retry.
  it("failed notes say why, in plain words, and offer Retry", () => {
    const f = (error: string) => ({ id: "m1", state: "notes_failed", percent: 0, processedThroughSeconds: 0, durationSeconds: 60, error });
    for (const [reason, expected] of [
      [OFFLINE_REASON, `Transcript ready · Notes failed — ${OFFLINE_REASON}`],
      ["not enough free space: 1.6 GB needed, 0.2 GB free", "Transcript ready · Notes failed — not enough free space: 1.6 GB needed, 0.2 GB free"],
      ["the notes model didn't download correctly. Retry to fetch it again.", "Transcript ready · Notes failed — the notes model didn't download correctly. Retry to fetch it again."],
      ["the model had nothing to say about this meeting", "Transcript ready · Notes failed — the model had nothing to say about this meeting"],
    ] as const) {
      expect(meetingStateLabel("complete", f(reason), false)).toBe(expected);
      expect(canRetryNotes("complete", f(reason), false)).toBe(true);
      expect(retryNotesLabel(f(reason))).toBe("Retry");
      expect(canCancelNotes(f(reason))).toBe(false);
    }
    expect(OFFLINE_REASON).toContain("Try again when online");
    // A failure on a meeting that already HAD notes keeps them and still says so.
    expect(meetingStateLabel("complete", f("x"), true)).toBe("Transcript ready · Notes failed — x");
    expect(meetingStateLabel("partial", f("x"), false)).toBe("Partly transcribed · Notes failed — x");
  });
  // #250 round 3 (B2) — Cancel while queued, downloading or writing.
  it("active notes can be cancelled; cancelled notes can be retried", () => {
    const p = (state: string) => ({ id: "m1", state, percent: 10, processedThroughSeconds: 0, durationSeconds: 60, error: null });
    for (const s of ["notes_queued", "preparing_notes", "summarizing"]) {
      expect(canCancelNotes(p(s))).toBe(true);
      expect(canRetryNotes("complete", p(s), false)).toBe(false);
    }
    expect(canCancelNotes(p("notes_ready"))).toBe(false);
    expect(canCancelNotes(undefined)).toBe(false);
    expect(meetingStateLabel("complete", p("notes_cancelled"), false)).toBe("Transcript ready · Notes cancelled");
    expect(canRetryNotes("complete", p("notes_cancelled"), false)).toBe(true);
  });
  // A3 — a past meeting with no notes and no job offers "Write notes".
  it("a transcript with no notes and no job offers Write notes", () => {
    expect(canRetryNotes("complete", undefined, false)).toBe(true);
    expect(retryNotesLabel(undefined)).toBe("Write notes");
    expect(canRetryNotes("complete", undefined, true)).toBe(false);
    expect(canRetryNotes("transcribing", undefined, false)).toBe(false);
  });
  it("job statuses fold into the progress map; download percents do not reload the list", () => {
    const e = notesStatusEvent({ id: "m9", phase: "notes_failed", percent: 0, reason: "r" });
    expect(e).toMatchObject({ id: "m9", state: "notes_failed", error: "r" });
    expect(reloadsMeetings("preparing_notes")).toBe(false);
    expect(reloadsMeetings("transcribing")).toBe(false);
    expect(reloadsMeetings("notes_ready")).toBe(true);
    expect(reloadsMeetings("notes_failed")).toBe(true);
  });
});
