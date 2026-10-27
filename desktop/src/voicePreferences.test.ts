import { describe, expect, it } from "vitest";
import { describeFilter, heardLine, preferenceStatus, type VoicePreference } from "./voicePreferences";

const row = (over: Partial<VoicePreference>): VoicePreference => ({
  id: "1",
  spoken: "never use exclamation marks in email",
  filter: "mode:email",
  rule: "Never use exclamation marks.",
  createdAt: "2026-10-27T12:00:00Z",
  active: false,
  appliedAt: null,
  retractedAt: null,
  ...over,
});

describe("Y9-C spoken preference rules", () => {
  it("shows the derived rule back before Apply, in the item's words", () => {
    expect(heardLine(row({}))).toBe("I heard: never use exclamation marks. I'll apply it to: email");
  });

  it("names every allowlisted filter shape", () => {
    expect(describeFilter("*")).toBe("everything I dictate");
    expect(describeFilter("mode:chat")).toBe("chat");
    expect(describeFilter("app:com.tinyspeck.slackmacgap")).toBe("Slack");
    expect(describeFilter("app:com.example.editor")).toBe("com.example.editor");
    expect(describeFilter("lang:de")).toBe("German");
  });

  it("walks the one-way life of a row: pending, active, retracted, discarded", () => {
    expect(preferenceStatus(row({}))).toBe("pending");
    const applied = row({ active: true, appliedAt: "2026-10-27T12:01:00Z" });
    expect(preferenceStatus(applied)).toBe("active");
    expect(preferenceStatus({ ...applied, active: false, retractedAt: "2026-10-27T12:02:00Z" })).toBe("retracted");
    expect(preferenceStatus(row({ retractedAt: "2026-10-27T12:02:00Z" }))).toBe("discarded");
  });
});
