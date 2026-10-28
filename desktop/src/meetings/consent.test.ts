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

// yap-byom-0 — the notice's upload claim follows the model-provider choices.
import { consentNotice, leaveLines } from "./consent";

describe("consentNotice(activeLeaves) — yap-byom-0", () => {
  it("keeps 'Nothing is uploaded' while every choice stays on this Mac", () => {
    expect(consentNotice([]).paragraphs[0]).toContain("Nothing is uploaded");
  });

  it("drops 'Nothing is uploaded' the moment a Yes label is active", () => {
    const leaves = leaveLines([{ task: "notes", host: "openrouter.ai" }]);
    expect(leaves).toEqual(["Notes go to openrouter.ai"]);
    const n = consentNotice(leaves);
    const all = n.paragraphs.join(" ");
    expect(all).not.toContain("Nothing is uploaded");
    expect(n.paragraphs[0]).toContain("Notes go to openrouter.ai");
    expect(n.paragraphs).toHaveLength(3);
  });

  it("makes no upload claim while the choices are unknown", () => {
    expect(consentNotice(null).paragraphs[0]).not.toContain("Nothing is uploaded");
    expect(consentNotice(null).paragraphs[0]).toContain("on this machine");
  });

  it("names polish in the singular", () => {
    expect(leaveLines([{ task: "polish", host: "api.groq.com" }])).toEqual([
      "Polish goes to api.groq.com",
    ]);
  });
});
