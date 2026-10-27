import { afterEach, describe, expect, it, vi } from "vitest";
import {
  formatDay, formatDayShort, formatMeetingDuration, formatMonth, formatTime, heatLevel,
  missingDictationGrants, monthName, polishSidecarLabel, readTrialWarnedFor, TRIAL_WARN_KEY,
  undoAiEditText, writeTrialWarnedFor, type PermissionReport, type TranscriptEntry,
} from "./appTypes";

const perms = (p: Partial<PermissionReport>) =>
  ({ microphone: true, microphoneStatus: "authorized", accessibility: true, ...p }) as PermissionReport;
const entry = (text: string, rawText?: string | null) => ({ text, rawText }) as TranscriptEntry;

describe("missingDictationGrants", () => {
  it("unknown perms is unknown, never missing", () => {
    expect(missingDictationGrants(null)).toEqual([]);
    expect(missingDictationGrants(undefined)).toEqual([]);
  });

  it("names Microphone and Accessibility, and nothing for a fully granted Mac", () => {
    expect(missingDictationGrants(perms({}))).toEqual([]);
    expect(missingDictationGrants(perms({ microphoneStatus: "denied" }))).toEqual(["Microphone"]);
    expect(missingDictationGrants(perms({ microphone: false }))).toEqual(["Microphone"]);
    expect(missingDictationGrants(perms({ accessibility: false, microphoneStatus: "notDetermined" })))
      .toEqual(["Microphone", "Accessibility"]);
  });
});

describe("polishSidecarLabel", () => {
  it("has a line per state, and names the failure tag", () => {
    expect(polishSidecarLabel({ state: "ready", reason: null })).toMatch(/^Ready/);
    expect(polishSidecarLabel({ state: "starting", reason: null })).toMatch(/^Starting/);
    expect(polishSidecarLabel({ state: "not-installed", reason: null })).toMatch(/^Not running/);
    expect(polishSidecarLabel({ state: "failed", reason: "died" })).toMatch(/^Stopped \(died\)/);
    expect(polishSidecarLabel({ state: "failed", reason: null })).toMatch(/^Stopped \(unknown\)/);
    expect(polishSidecarLabel(undefined)).toBe("Checking…");
  });
});

describe("undoAiEditText", () => {
  it("returns the raw take only when it differs from what was pasted", () => {
    expect(undoAiEditText(entry("Hello, world.", "hello world"))).toBe("hello world");
    expect(undoAiEditText(entry("same", " same "))).toBeNull();
    expect(undoAiEditText(entry("x", "   "))).toBeNull();
    expect(undoAiEditText(entry("x", null))).toBeNull();
    expect(undoAiEditText(entry("x"))).toBeNull();
  });
});

describe("formatMeetingDuration (mirrors meetings::format_duration)", () => {
  it("formats seconds, minutes and hours, and refuses nonsense", () => {
    expect(formatMeetingDuration(42)).toBe("42s");
    expect(formatMeetingDuration(750)).toBe("12m 30s");
    expect(formatMeetingDuration(3600 + 5 * 60)).toBe("1h 05m");
    expect(formatMeetingDuration(-3)).toBe("0s");
    expect(formatMeetingDuration(Number.NaN)).toBe("0s");
    expect(formatMeetingDuration(59.6)).toBe("1m 00s");
  });
});

describe("heatLevel", () => {
  it("buckets relative to the window's peak, 0 for nothing", () => {
    expect(heatLevel(0, 100)).toBe(0);
    expect(heatLevel(5, 0)).toBe(0);
    expect([5, 20, 50, 90].map((w) => heatLevel(w, 100))).toEqual([1, 2, 3, 4]);
  });
});

describe("date labels", () => {
  it("render a label that is not the raw key for valid input", () => {
    for (const s of [formatMonth("2026-03"), monthName("2026-03"), monthName("2026-03", "long"),
      formatTime("2026-03-04T15:06:00Z"), formatDay("2026-03-04"), formatDayShort("2026-03-04")]) {
      expect(typeof s).toBe("string");
      expect(s.length).toBeGreaterThan(0);
    }
    expect(monthName("2026-03", "long").length).toBeGreaterThanOrEqual(monthName("2026-03").length);
  });
});

describe("trial warning memo", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is null (warn once per launch) when storage is unavailable, and writing never throws", () => {
    vi.stubGlobal("window", {
      localStorage: {
        getItem: () => { throw new Error("SecurityError"); },
        setItem: () => { throw new Error("QuotaExceeded"); },
      },
    });
    expect(readTrialWarnedFor()).toBeNull();
    expect(() => writeTrialWarnedFor(1)).not.toThrow();
  });

  it("round-trips the expiry it was keyed on, and ignores garbage", () => {
    const store = new Map<string, string>();
    vi.stubGlobal("window", {
      localStorage: {
        getItem: (k: string) => store.get(k) ?? null,
        setItem: (k: string, v: string) => void store.set(k, v),
      },
    });
    expect(readTrialWarnedFor()).toBeNull();
    writeTrialWarnedFor(1_790_000_000_000);
    expect(store.get(TRIAL_WARN_KEY)).toBe("1790000000000");
    expect(readTrialWarnedFor()).toBe(1_790_000_000_000);
    store.set(TRIAL_WARN_KEY, "soon");
    expect(readTrialWarnedFor()).toBeNull();
  });
});
