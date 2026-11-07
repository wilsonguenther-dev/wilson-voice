/**
 * yap-1106-settings-redesign — Settings › Meetings › Meeting kinds: the kind a
 * new meeting starts as survives a relaunch, and anything unreadable is the
 * skip answer (`unknown`), never a gate on recording.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  KNOWN_MEETING_KINDS,
  MEETING_KIND_DEFAULT_KEY,
  isKnownMeetingKind,
  readMeetingKindDefault,
  writeMeetingKindDefault,
} from "./kindDefault";

describe("meeting kind default", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is unknown with no window, with storage refused, and writing never throws", () => {
    expect(readMeetingKindDefault()).toBe("unknown");
    vi.stubGlobal("window", {
      localStorage: {
        getItem: () => { throw new Error("SecurityError"); },
        setItem: () => { throw new Error("QuotaExceeded"); },
      },
    });
    expect(readMeetingKindDefault()).toBe("unknown");
    expect(writeMeetingKindDefault("virtual")).toBe(false);
  });

  it("round-trips each published kind and ignores garbage", () => {
    const store = new Map<string, string>();
    vi.stubGlobal("window", {
      localStorage: {
        getItem: (k: string) => store.get(k) ?? null,
        setItem: (k: string, v: string) => void store.set(k, v),
      },
    });
    expect(readMeetingKindDefault()).toBe("unknown");
    for (const kind of KNOWN_MEETING_KINDS) {
      expect(writeMeetingKindDefault(kind)).toBe(true);
      expect(store.get(MEETING_KIND_DEFAULT_KEY)).toBe(kind);
      expect(readMeetingKindDefault()).toBe(kind);
    }
    store.set(MEETING_KIND_DEFAULT_KEY, "boardroom");
    expect(readMeetingKindDefault()).toBe("unknown");
    writeMeetingKindDefault("boardroom");
    expect(store.get(MEETING_KIND_DEFAULT_KEY)).toBe("unknown");
  });

  it("knows exactly the kinds meeting_control::KIND_PICKER publishes", () => {
    expect([...KNOWN_MEETING_KINDS].sort()).toEqual(["in_person", "unknown", "virtual"]);
    expect(isKnownMeetingKind(null)).toBe(false);
    expect(isKnownMeetingKind("")).toBe(false);
  });
});
