/**
 * homeLayout.test — yap-1106-home-full-width-room. The pure half of the round-two
 * Home (approved 2026-11-06 17:08Z), pinned function by function: the room's
 * whole-number scale, the quiet stats line, the last-5 list and its "See all"
 * row, History's 50-row pages, the Needs attention gate, and the meeting label.
 * The markup half lives in views/__tests__/home.fullWidthRoom.test.tsx.
 */
import { describe, expect, it } from "vitest";
import { STUDIO_H, STUDIO_W } from "./studio/studio";
import {
  HISTORY_PAGE,
  HOME_RECENT,
  ROOM_H,
  ROOM_SCALES,
  ROOM_W,
  attentionTitle,
  meetingClock,
  meetingRoomLabel,
  pageRangeLabel,
  recentTakes,
  requestHistoryFocus,
  roomBox,
  roomScale,
  seeAllLabel,
  showNeedsAttention,
  statsLine,
  statsParts,
  takeHistoryFocus,
  takePreview,
} from "./homeLayout";

describe("roomScale / roomBox — whole-number scale, never a fraction (option A)", () => {
  it("the scene is the 320 x 160 studio and the only scales are 3, 2 (yap-1107-parity-home: 3x is the approved ceiling)", () => {
    expect([ROOM_W, ROOM_H]).toEqual([320, 160]);
    expect([ROOM_W, ROOM_H]).toEqual([STUDIO_W, STUDIO_H]);
    expect([...ROOM_SCALES]).toEqual([3, 2]);
  });

  it("picks the largest scale whose room fits the column (3x on the design page's 960 px)", () => {
    // yap-1107-parity-home — never 4x: the approved page draws exactly 3x (960 x 480).
    expect(roomScale(1280)).toBe(3);
    expect(roomScale(1600)).toBe(3);
    expect(roomScale(2400)).toBe(3);
    expect(roomScale(1279)).toBe(3);
    expect(roomScale(960)).toBe(3);
    expect(roomScale(959)).toBe(2);
    expect(roomScale(640)).toBe(2);
  });

  it("a column narrower than 2x still draws at 2x (the frame crops); junk reads as 2x", () => {
    expect(roomScale(400)).toBe(2);
    expect(roomScale(0)).toBe(2);
    expect(roomScale(-50)).toBe(2);
    expect(roomScale(Number.NaN)).toBe(2);
    expect(roomScale(Number.POSITIVE_INFINITY)).toBe(2);
    expect(roomScale(null)).toBe(2);
    expect(roomScale(undefined)).toBe(2);
  });

  it("every width maps to an integer scale and an integer box", () => {
    for (let w = 300; w <= 2400; w += 7) {
      const b = roomBox(w);
      expect(Number.isInteger(b.scale)).toBe(true);
      expect(b).toEqual({ scale: b.scale, width: ROOM_W * b.scale, height: ROOM_H * b.scale });
    }
    expect(roomBox(1000)).toEqual({ scale: 3, width: 960, height: 480 });
    expect(roomBox(null)).toEqual({ scale: 2, width: 640, height: 320 });
  });
});

describe("recentTakes — Home ends after 5 takes", () => {
  it("never more than HOME_RECENT, newest first as given", () => {
    expect(HOME_RECENT).toBe(5);
    const rows = Array.from({ length: 12 }, (_, i) => i);
    expect(recentTakes(rows)).toEqual([0, 1, 2, 3, 4]);
    expect(recentTakes([7, 8])).toEqual([7, 8]);
    expect(recentTakes([])).toEqual([]);
  });

  it("does not mutate the history it is handed", () => {
    const rows = [1, 2, 3, 4, 5, 6];
    recentTakes(rows);
    expect(rows).toHaveLength(6);
  });
});

describe("showNeedsAttention — hidden when empty", () => {
  it("shows only when a failed or cancelled take exists", () => {
    expect(showNeedsAttention([])).toBe(false);
    expect(showNeedsAttention(null)).toBe(false);
    expect(showNeedsAttention(undefined)).toBe(false);
    expect(showNeedsAttention([{ id: "a" }])).toBe(true);
  });

  it("titles a cancelled take apart from a failed one", () => {
    expect(attentionTitle("Cancelled by you")).toBe("Cancelled take");
    expect(attentionTitle("take was CANCELED")).toBe("Cancelled take");
    expect(attentionTitle("engine timed out")).toBe("Could not transcribe");
    expect(attentionTitle(null)).toBe("Could not transcribe");
    expect(attentionTitle(undefined)).toBe("Could not transcribe");
  });
});

describe("statsLine — one quiet line, only real numbers", () => {
  it("lists the numbers that loaded, in order", () => {
    expect(statsLine({ wordsToday: 1234, streakDays: 3, totalTakes: 48 })).toEqual([
      "1,234 words today",
      "3 day streak",
      "48 takes",
    ]);
  });

  it("singular take, rounding, and never a negative", () => {
    expect(statsLine({ totalTakes: 1 })).toEqual(["1 take"]);
    expect(statsLine({ wordsToday: 12.6 })).toEqual(["13 words today"]);
    expect(statsLine({ streakDays: -2 })).toEqual(["0 day streak"]);
  });

  it("a value that has not loaded is left out, never shown as 0", () => {
    expect(statsLine({})).toEqual([]);
    expect(statsLine({ wordsToday: null, streakDays: undefined, totalTakes: null })).toEqual([]);
    expect(statsLine({ wordsToday: 0 })).toEqual(["0 words today"]);
  });
});

describe("meetingClock / meetingRoomLabel — 'Recording meeting · mm:ss'", () => {
  it("mm:ss under an hour, h:mm:ss after", () => {
    expect(meetingClock(0)).toBe("00:00");
    expect(meetingClock(59.9)).toBe("00:59");
    expect(meetingClock(336)).toBe("05:36");
    expect(meetingClock(3599)).toBe("59:59");
    expect(meetingClock(3600)).toBe("1:00:00");
    expect(meetingClock(3725)).toBe("1:02:05");
  });

  it("junk and negatives read as 00:00", () => {
    expect(meetingClock(-5)).toBe("00:00");
    expect(meetingClock(Number.NaN)).toBe("00:00");
    expect(meetingClock(null)).toBe("00:00");
    expect(meetingClock(undefined)).toBe("00:00");
  });

  it("labels the room only while a meeting records", () => {
    expect(meetingRoomLabel({ recording: true, elapsedSeconds: 336 })).toBe("Recording meeting · 05:36");
    expect(meetingRoomLabel({ recording: true })).toBe("Recording meeting · 00:00");
    expect(meetingRoomLabel({ recording: false, elapsedSeconds: 336 })).toBeNull();
    expect(meetingRoomLabel({})).toBeNull();
    expect(meetingRoomLabel(null)).toBeNull();
    expect(meetingRoomLabel(undefined)).toBeNull();
  });
});

describe("seeAllLabel / pageRangeLabel — a definite end, History 50 at a time", () => {
  it("the row that ends Home's list", () => {
    expect(seeAllLabel(1234, "")).toBe("See all 1,234 →");
    expect(seeAllLabel(0, "")).toBe("See all →");
    expect(seeAllLabel(null, "")).toBe("See all →");
    expect(seeAllLabel(undefined, "")).toBe("See all →");
    expect(seeAllLabel(40, "standup")).toBe("See every match in History");
  });

  it("History pages hold HISTORY_PAGE rows", () => {
    expect(HISTORY_PAGE).toBe(50);
    expect(pageRangeLabel(0, 50)).toBe("Takes 1–50");
    expect(pageRangeLabel(1, 50)).toBe("Takes 51–100");
    expect(pageRangeLabel(20, 3)).toBe("Takes 1,001–1,003");
    expect(pageRangeLabel(0, 0)).toBe("No takes");
    expect(pageRangeLabel(3, -1)).toBe("No takes");
  });
});

describe("takePreview — one line per take", () => {
  it("collapses whitespace and keeps short text whole", () => {
    expect(takePreview("  hello \n\n  there\tfriend ")).toBe("hello there friend");
    expect(takePreview("x".repeat(90))).toBe("x".repeat(90));
  });

  it("truncates long text to max with an ellipsis", () => {
    const out = takePreview("word ".repeat(40));
    expect(out.length).toBeLessThanOrEqual(90);
    expect(out.endsWith("…")).toBe(true);
    expect(takePreview("abcdefghij", 5)).toBe("abcd…");
  });
});

describe("requestHistoryFocus / takeHistoryFocus — Open hands one take to History once", () => {
  it("is one-shot: read once, then cleared", () => {
    expect(takeHistoryFocus()).toBeNull();
    requestHistoryFocus("take-7");
    expect(takeHistoryFocus()).toBe("take-7");
    expect(takeHistoryFocus()).toBeNull();
  });

  it("a later request replaces an unread one; null clears it", () => {
    requestHistoryFocus("a");
    requestHistoryFocus("b");
    expect(takeHistoryFocus()).toBe("b");
    requestHistoryFocus("c");
    requestHistoryFocus(null);
    expect(takeHistoryFocus()).toBeNull();
  });
});

describe("statsParts — yap-1107-parity-home: the approved stats line, number then label", () => {
  it("bold number + label per figure, in the page's order", () => {
    expect(statsParts({ wordsToday: 2632, streakDays: 17, totalTakes: 288 })).toEqual([
      { key: "words", value: "2,632", label: "words today" },
      { key: "streak", value: "17", label: "day streak" },
      { key: "takes", value: "288", label: "takes" },
    ]);
  });
  it("leaves out what has not loaded, never a fake 0; one take is singular", () => {
    expect(statsParts({})).toEqual([]);
    expect(statsParts({ wordsToday: null, streakDays: undefined, totalTakes: 1 })).toEqual([
      { key: "takes", value: "1", label: "take" },
    ]);
  });
  it("agrees with statsLine figure for figure", () => {
    const o = { wordsToday: 0, streakDays: 3, totalTakes: 12 };
    expect(statsParts(o).map((p) => `${p.value} ${p.label}`)).toEqual(statsLine(o));
  });
});
