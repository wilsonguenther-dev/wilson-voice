/**
 * YV95 — the label rules shared by the pill, the Meetings banner and the empty
 * state. They are unit-tested for the same reason live.ts is: the frontend gate
 * used to be a type check, and a type check has no opinion about a clock that
 * renders "undefined" or a disabled button that never says why.
 */
import { describe, expect, it } from "vitest";
import {
  disabledReason,
  elapsedLabel,
  formatElapsed,
  IDLE_MEETING,
  recordLabel,
  type MeetingStatus,
} from "./meeting";

const recording = (over: Partial<MeetingStatus> = {}): MeetingStatus => ({
  recording: true,
  id: "m1",
  title: "Meeting 3:04 PM",
  elapsedSeconds: 3725,
  elapsedLabel: "01:02:05",
  captureAvailable: true,
  ...over,
});

describe("formatElapsed", () => {
  it("is always hh:mm:ss so a 3-hour and a 3-minute meeting line up", () => {
    expect(formatElapsed(0)).toBe("00:00:00");
    expect(formatElapsed(9)).toBe("00:00:09");
    expect(formatElapsed(750)).toBe("00:12:30");
    expect(formatElapsed(3725)).toBe("01:02:05");
    expect(formatElapsed(3 * 3600)).toBe("03:00:00");
  });

  it("never renders a negative or a NaN clock", () => {
    expect(formatElapsed(-5)).toBe("00:00:00");
    expect(formatElapsed(Number.NaN)).toBe("00:00:00");
    expect(formatElapsed(Number.POSITIVE_INFINITY)).toBe("00:00:00");
  });

  it("matches the Rust renderer's rule of truncating, not rounding", () => {
    // `meetings::format_offset` floors — a meeting is not 13 seconds old until
    // it is, and a clock that rounds up starts at 00:00:01.
    expect(formatElapsed(12.9)).toBe("00:00:12");
  });
});

describe("elapsedLabel", () => {
  it("prefers the label the backend already rendered", () => {
    expect(elapsedLabel(recording())).toBe("01:02:05");
  });

  it("falls back to formatting locally rather than showing undefined", () => {
    expect(elapsedLabel(recording({ elapsedLabel: undefined }))).toBe("01:02:05");
    expect(elapsedLabel(recording({ elapsedLabel: "" }))).toBe("01:02:05");
  });

  it("has an answer for no status at all", () => {
    expect(elapsedLabel(null)).toBe("00:00:00");
    expect(elapsedLabel(IDLE_MEETING)).toBe("00:00:00");
  });
});

describe("the record control's copy", () => {
  it("names the action, in both states", () => {
    expect(recordLabel(IDLE_MEETING)).toBe("Record this meeting");
    expect(recordLabel(recording())).toBe("Stop meeting");
  });

  it("is enabled when a capture engine is installed", () => {
    expect(disabledReason({ ...IDLE_MEETING, captureAvailable: true })).toBeNull();
  });

  it("says WHY it is disabled instead of just being grey", () => {
    const why = disabledReason({
      ...IDLE_MEETING,
      captureAvailable: false,
      unavailableReason: "Meeting recording needs the capture engine — it is not installed in this build.",
    });
    expect(why).toContain("capture engine");
  });

  it("still has a reason when the backend forgot to send one", () => {
    expect(disabledReason({ ...IDLE_MEETING, captureAvailable: false })).toBeTruthy();
  });

  it("never disables the STOP control — a running meeting can always be stopped", () => {
    expect(disabledReason(recording({ captureAvailable: false }))).toBeNull();
  });
});
