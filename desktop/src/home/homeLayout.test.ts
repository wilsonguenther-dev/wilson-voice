// yap-1106-home-full-width-room — the sibling test PURE_MODULES requires for
// home/homeLayout.ts (added by the batch-2 integrator). The markup itself is
// covered by views/__tests__/home.fullWidthRoom.test.tsx; this pins the rules.
import { describe, expect, it } from "vitest";
import {
  HISTORY_PAGE, HOME_RECENT, ROOM_H, ROOM_W, attentionTitle, meetingClock, meetingRoomLabel,
  pageRangeLabel, recentTakes, requestHistoryFocus, roomBox, roomScale, seeAllLabel,
  showNeedsAttention, statsLine, takeHistoryFocus, takePreview,
} from "./homeLayout";

describe("homeLayout", () => {
  it("picks the largest whole-number scale that fits, never a fraction, 3x at the floor", () => {
    expect(roomScale(ROOM_W * 5)).toBe(5);
    expect(roomScale(ROOM_W * 5 - 1)).toBe(4);
    expect(roomScale(ROOM_W * 4)).toBe(4);
    expect(roomScale(ROOM_W * 4 - 1)).toBe(3);
    expect(roomScale(10)).toBe(3);
    expect(roomScale(null)).toBe(3);
    expect(roomScale(undefined)).toBe(3);
    expect(roomScale(Number.NaN)).toBe(3);
    expect(roomBox(2000)).toEqual({ scale: 5, width: ROOM_W * 5, height: ROOM_H * 5 });
  });

  it("Home keeps the last 5 takes; History pages 50", () => {
    expect(HOME_RECENT).toBe(5);
    expect(HISTORY_PAGE).toBe(50);
    expect(recentTakes([1, 2, 3, 4, 5, 6, 7])).toEqual([1, 2, 3, 4, 5]);
    expect(recentTakes([])).toEqual([]);
  });

  it("Needs attention shows only when something needs it", () => {
    expect(showNeedsAttention([])).toBe(false);
    expect(showNeedsAttention(null)).toBe(false);
    expect(showNeedsAttention(undefined)).toBe(false);
    expect(showNeedsAttention([{}])).toBe(true);
    expect(attentionTitle("Cancelled by user")).toBe("Cancelled take");
    expect(attentionTitle("decoder error")).toBe("Could not transcribe");
    expect(attentionTitle(null)).toBe("Could not transcribe");
  });

  it("the stats line holds real numbers only", () => {
    expect(statsLine({})).toEqual([]);
    expect(statsLine({ wordsToday: 1234, streakDays: 3, totalTakes: 1 })).toEqual(["1,234 words today", "3 day streak", "1 take"]);
    expect(statsLine({ totalTakes: 2, wordsToday: null })).toEqual(["2 takes"]);
  });

  it("the meeting label reads 'Recording meeting · mm:ss'", () => {
    expect(meetingClock(0)).toBe("00:00");
    expect(meetingClock(65)).toBe("01:05");
    expect(meetingClock(3725)).toBe("1:02:05");
    expect(meetingClock(-4)).toBe("00:00");
    expect(meetingClock(null)).toBe("00:00");
    expect(meetingRoomLabel(null)).toBeNull();
    expect(meetingRoomLabel({ recording: false, elapsedSeconds: 9 })).toBeNull();
    expect(meetingRoomLabel({ recording: true, elapsedSeconds: 9 })).toBe("Recording meeting · 00:09");
  });

  it("See all, page ranges, previews and the one-shot History focus", () => {
    expect(seeAllLabel(12, "")).toBe("See all 12 →");
    expect(seeAllLabel(0, "")).toBe("See all →");
    expect(seeAllLabel(null, "")).toBe("See all →");
    expect(seeAllLabel(12, "hello")).toBe("See every match in History");
    expect(pageRangeLabel(0, 0)).toBe("No takes");
    expect(pageRangeLabel(1, 50)).toBe("Takes 51–100");
    expect(takePreview("  a   b  ")).toBe("a b");
    expect(takePreview("abcdefghij", 5)).toBe("abcd…");
    requestHistoryFocus("t1");
    expect(takeHistoryFocus()).toBe("t1");
    expect(takeHistoryFocus()).toBeNull();
  });
});
