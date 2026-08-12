import { describe, expect, it } from "vitest";
import {
  CONSENT_NOTICE,
  acknowledgedLabel,
  shouldOpenNotice,
  type MeetingConsent,
} from "./consent";

const PENDING: MeetingConsent = {
  shouldShow: true,
  acknowledgedAt: null,
  blocksRecording: false,
};
const SEEN: MeetingConsent = {
  shouldShow: false,
  acknowledgedAt: "2026-08-11T18:04:00Z",
  blocksRecording: false,
};

describe("the one-time notice opens exactly once", () => {
  it("opens on a record attempt that has never seen it", () => {
    expect(shouldOpenNotice(PENDING, { recording: true })).toBe(true);
  });

  it("never opens once the settings_kv ack is set", () => {
    expect(shouldOpenNotice(SEEN, { recording: true })).toBe(false);
  });

  it("never opens just because the app is running", () => {
    // The plan's own framing: this is a notice about capture, shown at the
    // moment of capture — not a launch-time interstitial.
    expect(shouldOpenNotice(PENDING, { recording: false })).toBe(false);
    expect(shouldOpenNotice(SEEN, { recording: false })).toBe(false);
  });

  it("stays shut while the answer is unknown", () => {
    // Before the backend has replied there is no proof this is the first time,
    // and guessing "show it" is how a one-time notice shows twice.
    expect(shouldOpenNotice(null, { recording: true })).toBe(false);
  });

  it("is idempotent across a whole meeting of 1 Hz ticks", () => {
    const opens = Array.from({ length: 3600 }, () =>
      shouldOpenNotice(SEEN, { recording: true }),
    );
    expect(opens.some(Boolean)).toBe(false);
  });
});

describe("the copy says the two things this item exists to say", () => {
  const body = CONSENT_NOTICE.paragraphs.join(" ");

  it("puts the legal responsibility on the user", () => {
    expect(body).toMatch(/your responsibility/i);
    expect(body).toMatch(/laws differ/i);
  });

  it("says plainly that Yap does not announce itself", () => {
    expect(body).toMatch(/does not announce itself/i);
  });

  it("names what is actually captured, in plain words", () => {
    expect(body).toMatch(/microphone/i);
    expect(body).toMatch(/transcript/i);
  });

  it("never claims Yap makes the user compliant", () => {
    // §8's own stated non-goal: Yap reminds, it does not adjudicate.
    expect(body).not.toMatch(/\b(keeps|makes) you (compliant|legal)/i);
    expect(body).not.toMatch(/legal advice/i);
    expect(body).toMatch(/will not decide this for you/i);
  });

  it("points at the Terms rather than restating them", () => {
    expect(CONSENT_NOTICE.fine).toMatch(/Terms/);
    expect(CONSENT_NOTICE.fine).toMatch(/only see this once/i);
  });
});

describe("the Settings line", () => {
  it("tells a user who has not seen it when it will appear", () => {
    expect(acknowledgedLabel(PENDING)).toMatch(/first time you record/i);
    expect(acknowledgedLabel(null)).toMatch(/first time you record/i);
  });

  it("dates the notice once it has been shown", () => {
    expect(acknowledgedLabel(SEEN)).toMatch(/Recording notice shown/);
    expect(acknowledgedLabel(SEEN)).toMatch(/2026/);
  });

  it("does not fall over on a value it cannot parse", () => {
    const junk = { ...SEEN, acknowledgedAt: "not-a-date" };
    expect(acknowledgedLabel(junk)).toContain("not-a-date");
  });
});
