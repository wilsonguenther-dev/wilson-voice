import { describe, expect, it } from "vitest";
import {
  MAX_POPOVER_MEETINGS,
  RECORD_NOW_TEXT,
  parseMeetingPopover,
  popoverEntries,
  recordRowText,
  type KindChoice,
} from "./meetingPopover";

// The exact JSON `calendar::meeting_popover` serialises (serde camelCase), one per state.
const WITH_MEETINGS = {
  state: "meetings",
  meetings: [
    { key: "cal:a:1", label: "Standup", clock: "2:00 PM", hasLink: true },
    { key: "cal:b:2", label: "Meeting", clock: "2:30 PM", hasLink: false },
    { key: "cal:c:3", label: "Design review", clock: "3:00 PM", hasLink: false },
  ],
  emptyText: null,
  connectText: null,
  connect: null,
  recordNowText: "Record now",
};
const EMPTY = {
  state: "empty",
  meetings: [],
  emptyText: "No upcoming meetings",
  connectText: null,
  connect: null,
  recordNowText: "Record now",
};
const NOT_CONNECTED = {
  state: "not_connected",
  meetings: [],
  emptyText: "No upcoming meetings",
  connectText: "Connect calendar",
  connect: { nav: "meetings", tab: null },
  recordNowText: "Record now",
};
const KINDS: KindChoice[] = [
  { kind: "in_person", label: "In person" },
  { kind: "virtual", label: "Virtual" },
];

describe("WF-08 meeting popover", () => {
  it("connected with meetings: three Record rows, then Record now with the kind picker", () => {
    const rows = popoverEntries(parseMeetingPopover(WITH_MEETINGS), KINDS);
    expect(rows).toEqual([
      { type: "record", key: "cal:a:1", text: "Record Standup · 2:00 PM" },
      { type: "record", key: "cal:b:2", text: "Record Meeting · 2:30 PM" },
      { type: "record", key: "cal:c:3", text: "Record Design review · 3:00 PM" },
      { type: "separator" },
      { type: "record-now", text: "Record now", kinds: KINDS },
    ]);
  });

  it("connected and empty: 'No upcoming meetings', no Connect, Record now", () => {
    const rows = popoverEntries(parseMeetingPopover(EMPTY), KINDS);
    expect(rows.map((r) => r.type)).toEqual(["note", "separator", "record-now"]);
    expect(rows[0]).toEqual({ type: "note", text: "No upcoming meetings" });
    expect(rows.some((r) => r.type === "connect")).toBe(false);
  });

  it("not connected: 'No upcoming meetings' + Connect calendar to the setup card, Record now", () => {
    const rows = popoverEntries(parseMeetingPopover(NOT_CONNECTED), []);
    expect(rows).toEqual([
      { type: "note", text: "No upcoming meetings" },
      { type: "connect", text: "Connect calendar", nav: "meetings", tab: null },
      { type: "separator" },
      { type: "record-now", text: "Record now", kinds: [] },
    ]);
    const toSettings = parseMeetingPopover({ ...NOT_CONNECTED, connect: { nav: "settings", tab: "meetings" } });
    expect(popoverEntries(toSettings, null)[1]).toEqual({ type: "connect", text: "Connect calendar", nav: "settings", tab: "meetings" });
  });

  it("an unusable answer degrades to Record now alone (the pre-WF-08 button)", () => {
    for (const bad of [null, undefined, 7, "x", {}, { state: "weird", meetings: [] }, { state: "empty", meetings: "no" }]) {
      expect(parseMeetingPopover(bad)).toBeNull();
    }
    expect(popoverEntries(null, KINDS)).toEqual([{ type: "record-now", text: RECORD_NOW_TEXT, kinds: KINDS }]);
    expect(popoverEntries(null, undefined)).toEqual([{ type: "record-now", text: RECORD_NOW_TEXT, kinds: [] }]);
  });

  it("parse keeps at most three valid rows, trims, and fills the defaults", () => {
    const many = {
      state: "meetings",
      meetings: [
        null,
        { key: "", label: "no key" },
        { key: "k1", label: " One ", clock: " 9:00 AM ", hasLink: "yes" },
        { key: "k2", label: "Two" },
        { key: "k3", label: "Three", clock: "11:00 AM" },
        { key: "k4", label: "Four", clock: "noon" },
      ],
      connect: { nav: "", tab: "x" },
    };
    const p = parseMeetingPopover(many)!;
    expect(p.meetings).toHaveLength(MAX_POPOVER_MEETINGS);
    expect(p.meetings[0]).toEqual({ key: "k1", label: "One", clock: "9:00 AM", hasLink: false });
    expect(p.meetings[1].clock).toBe("");
    expect(p.connect).toBeNull();
    expect(p.emptyText).toBeNull();
    expect(p.connectText).toBeNull();
    expect(p.recordNowText).toBe(RECORD_NOW_TEXT);
    expect(recordRowText(p.meetings[1])).toBe("Record Two");
  });

  it("falls back to its own words when the answer leaves a line out", () => {
    const rows = popoverEntries(
      { state: "not_connected", meetings: [], emptyText: null, connectText: null, connect: { nav: "meetings", tab: null }, recordNowText: "Record now" },
      [{ kind: "", label: "blank" }],
    );
    expect(rows[0]).toEqual({ type: "note", text: "No upcoming meetings" });
    expect(rows[1]).toEqual({ type: "connect", text: "Connect calendar", nav: "meetings", tab: null });
    expect(rows[3]).toEqual({ type: "record-now", text: "Record now", kinds: [] });
    // "meetings" with no rows left (all invalid) reads as empty, never a blank popover.
    const hollow = popoverEntries({ ...rowsModel(), meetings: [] }, []);
    expect(hollow[0].type).toBe("note");
  });
});

function rowsModel() {
  return parseMeetingPopover(WITH_MEETINGS)!;
}
