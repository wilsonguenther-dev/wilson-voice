import { describe, expect, it } from "vitest";
import { SAMPLE_EXPORT, SAMPLE_NOTES, SAMPLE_SEGMENTS, SAMPLE_TITLE } from "./sampleMeeting";

// The tour's fixture is rendered by the REAL <TranscriptList>, so its shape has
// to be one a recorded meeting could have.
describe("sampleMeeting (the notetaker tour's fixture)", () => {
  it("is a two-track conversation in time order with unique ids", () => {
    expect(new Set(SAMPLE_SEGMENTS.map((s) => s.track))).toEqual(new Set([0, 1]));
    const starts = SAMPLE_SEGMENTS.map((s) => s.startSeconds);
    expect([...starts].sort((a, b) => a - b)).toEqual(starts);
    expect(new Set(SAMPLE_SEGMENTS.map((s) => s.id)).size).toBe(SAMPLE_SEGMENTS.length);
    expect(SAMPLE_SEGMENTS.every((s) => s.text.trim().length > 0)).toBe(true);
  });

  it("says it is a sample, so nobody mistakes it for a recorded meeting", () => {
    expect(SAMPLE_TITLE.startsWith("Sample:")).toBe(true);
  });

  it("exports every note under the title, as the real Markdown export does", () => {
    expect(SAMPLE_EXPORT.startsWith(`# ${SAMPLE_TITLE}\n`)).toBe(true);
    for (const n of SAMPLE_NOTES) expect(SAMPLE_EXPORT).toContain(`- ${n}`);
  });
});
