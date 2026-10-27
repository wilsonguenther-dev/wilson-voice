import { describe, expect, it } from "vitest";
import {
  canRetry, canRetryTranscript, canCancelNotes, canRetryNotes, foldProgress, meetingStateLabel, notesStatusEvent,
  reloadsMeetings, retryNotesLabel, OFFLINE_REASON, summaryFirstLine, summaryTitle, transcribedLabel, transcribePercent,
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
  it("waiting_for_model (yap24-NT2 R3) names the wait and is not a Retry state", () => {
    expect(meetingStateLabel("waiting_for_model", undefined, false)).toBe(
      "Waiting for a speech model",
    );
    expect(canRetry("waiting_for_model")).toBe(false);
  });
  // #250 fix round 4 (independent B1): the transcript Retry deletes the segments a
  // notes job is about to read, so it is hidden for EVERY active notes phase —
  // not only "summarizing" (preparing_notes is the 1.1 GB first-run download).
  it("a partial meeting hides the transcript Retry while any notes job is active", () => {
    const p = (state: string): MeetingProgressEvent => ({ id: "m1", state, percent: 0, processedThroughSeconds: 0, durationSeconds: 60 });
    for (const s of ["notes_queued", "preparing_notes", "summarizing"]) {
      expect(canRetryTranscript("partial", p(s))).toBe(false);
      expect(canRetryTranscript("failed", p(s))).toBe(false);
    }
    for (const s of ["notes_ready", "notes_failed", "notes_cancelled"]) {
      expect(canRetryTranscript("partial", p(s))).toBe(true);
    }
    expect(canRetryTranscript("partial", undefined)).toBe(true);
    expect(canRetryTranscript("complete", undefined)).toBe(false);
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
    expect(summaryFirstLine("\n## Decisions\n- ship it")).toBe("Decisions: ship it");
    expect(summaryFirstLine("- agreed on a date\n- next step")).toBe("agreed on a date");
    expect(summaryFirstLine("Plain sentence.")).toBe("Plain sentence.");
  });
  it("a notes body with no narrative names its section, never just the heading", () => {
    // render_summary() opens with "### Action items" when the model wrote no narrative.
    expect(summaryFirstLine("### Action items\n\n- send the pricing doc (Jeisil, 00:04:12)\n"))
      .toBe("Action items: send the pricing doc (Jeisil, 00:04:12)");
    expect(summaryFirstLine("### Open questions\n")).toBe("Open questions");
  });
  it("drops inline markdown emphasis so no stray ** or backticks reach the row", () => {
    expect(summaryFirstLine("**Pricing** was agreed, see `v2`.")).toBe("Pricing was agreed, see v2.");
    expect(summaryFirstLine("- __Ship__ on Friday")).toBe("Ship on Friday");
  });
  it("is null with no notes", () => {
    expect(summaryFirstLine(null)).toBeNull();
    expect(summaryFirstLine("  \n ")).toBeNull();
  });
});
