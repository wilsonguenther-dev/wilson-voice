/**
 * yap-1106-meetings-redesign — the two-pane Meetings view's decisions
 * (YAP-UI-SPEC-2026-11 §3.3), walked without a window.
 */
import { describe, expect, it } from "vitest";
import {
  DATE_GROUP_LABELS,
  DETAIL_TABS,
  NO_FILTER,
  PERSON_REASON,
  TWO_PANE_MIN_WIDTH,
  UNDO_DELETE_MS,
  dateGroupOf,
  defaultTab,
  detailPeople,
  duePendingDeletes,
  filterActive,
  filterMeetings,
  groupMeetings,
  isNarrow,
  listPeople,
  meetingsKeyAction,
  meetingsLogLine,
  moveSelection,
  peopleOptions,
  schedulePendingDelete,
  shortId,
  stopNeedsConfirm,
  sunProgress,
  talkTimeLabel,
  undoPendingDelete,
  undoSecondsLeft,
  unnamedVoices,
  withoutPending,
} from "./meetingsLayout";

// Local noon on 2026-11-06, so day arithmetic never straddles midnight.
const NOW = new Date(2026, 10, 6, 12, 0, 0);
const at = (daysAgo: number, hour = 9) => new Date(2026, 10, 6 - daysAgo, hour, 0, 0).toISOString();

type Row = { id: string; title: string; startedAt: string; kind?: string | null; state: string; summary?: string | null; sysWavPath?: string | null };
const row = (id: string, over: Partial<Row> = {}): Row => ({
  id,
  title: `Meeting ${id}`,
  startedAt: at(0),
  kind: "virtual",
  state: "complete",
  summary: null,
  sysWavPath: null,
  ...over,
});

describe("date groups", () => {
  it("puts a meeting in Today, Yesterday, This week or Earlier by local day", () => {
    expect(dateGroupOf(at(0, 8), NOW)).toBe("today");
    expect(dateGroupOf(at(1, 23), NOW)).toBe("yesterday");
    expect(dateGroupOf(at(2), NOW)).toBe("week");
    expect(dateGroupOf(at(6), NOW)).toBe("week");
    expect(dateGroupOf(at(7), NOW)).toBe("earlier");
    expect(dateGroupOf(at(40), NOW)).toBe("earlier");
  });

  it("never puts an unreadable timestamp at the top, and treats clock skew as today", () => {
    expect(dateGroupOf("not a date", NOW)).toBe("earlier");
    expect(dateGroupOf(new Date(2026, 10, 7, 9).toISOString(), NOW)).toBe("today");
  });

  it("groups in Today → Earlier order, drops empty groups and keeps row order", () => {
    const groups = groupMeetings([row("a", { startedAt: at(0) }), row("b", { startedAt: at(9) }), row("c", { startedAt: at(0, 7) })], NOW);
    expect(groups.map((g) => g.label)).toEqual([DATE_GROUP_LABELS.today, DATE_GROUP_LABELS.earlier]);
    expect(groups[0].meetings.map((m) => m.id)).toEqual(["a", "c"]);
    expect(groupMeetings([], NOW)).toEqual([]);
  });
});

describe("people", () => {
  it("shows You + Call for a two-track call and one unnamed Speaker for the room", () => {
    expect(listPeople(row("a", { sysWavPath: "/x/sys.wav" }))).toEqual(["You", "Call"]);
    expect(listPeople(row("b", { kind: "in_person" }))).toEqual(["Speaker"]);
    expect(listPeople(row("c", { kind: "in_person", sysWavPath: "/x/sys.wav" }))).toEqual(["Speaker", "Call"]);
    expect(listPeople(row("d", { state: "recording" }))).toEqual([]);
  });

  it("builds the People tab from the transcript, most lines first, each label with a reason", () => {
    const segs = [
      { id: "1", startSeconds: 0, endSeconds: 30, text: "hi there", track: 0 },
      { id: "2", startSeconds: 31, endSeconds: 100, text: "hello", track: 1 },
      { id: "3", startSeconds: 101, endSeconds: 200, text: "and so", track: 1 },
      { id: "4", startSeconds: 201, text: "   ", track: 0 },
    ];
    const people = detailPeople(segs, "virtual");
    expect(people.map((p) => [p.name, p.lines])).toEqual([
      ["Call", 2],
      ["You", 1],
    ]);
    expect(people[0].seconds).toBe(168);
    expect(people[0].firstOffset).toBe("00:31");
    expect(people[0].reason).toBe(PERSON_REASON.Call);
    expect(unnamedVoices(people)).toEqual([]);
  });

  it("marks the mic of a room meeting as an unnamed voice for the Who's talking? strip", () => {
    const people = detailPeople([{ id: "1", startSeconds: 0, endSeconds: null, text: "room", track: 0 }], "in_person");
    expect(people).toHaveLength(1);
    expect(people[0].named).toBe(false);
    expect(people[0].seconds).toBe(0);
    expect(unnamedVoices(people).map((p) => p.name)).toEqual(["Speaker"]);
  });

  it("says talk time in plain minutes", () => {
    expect(talkTimeLabel(30)).toBe("under a minute");
    expect(talkTimeLabel(Number.NaN)).toBe("under a minute");
    expect(talkTimeLabel(754)).toBe("13 min");
  });
});

describe("filters", () => {
  const list = [
    row("a", { kind: "virtual", summary: "# Notes", sysWavPath: "/s.wav" }),
    row("b", { kind: "in_person" }),
    row("c", { kind: null, summary: "  " }),
  ];

  it("keeps everything with no filter, and knows when one is on", () => {
    expect(filterMeetings(list, NO_FILTER)).toHaveLength(3);
    expect(filterActive(NO_FILTER)).toBe(false);
    expect(filterActive({ ...NO_FILTER, notes: "with" })).toBe(true);
  });

  it("filters by kind (a missing kind is unknown), notes, and person", () => {
    expect(filterMeetings(list, { ...NO_FILTER, kind: "in_person" }).map((m) => m.id)).toEqual(["b"]);
    expect(filterMeetings(list, { ...NO_FILTER, kind: "unknown" }).map((m) => m.id)).toEqual(["c"]);
    expect(filterMeetings(list, { ...NO_FILTER, notes: "with" }).map((m) => m.id)).toEqual(["a"]);
    expect(filterMeetings(list, { ...NO_FILTER, notes: "without" }).map((m) => m.id)).toEqual(["b", "c"]);
    expect(filterMeetings(list, { ...NO_FILTER, person: "Call" }).map((m) => m.id)).toEqual(["a"]);
  });

  it("offers each person once, in first-seen order", () => {
    expect(peopleOptions(list)).toEqual(["You", "Call", "Speaker"]);
  });
});

describe("detail tabs", () => {
  it("are Notes | Transcript | People on ⌥⌘1/2/3", () => {
    expect(DETAIL_TABS.map((t) => `${t.label} ${t.keys}`)).toEqual(["Notes ⌥⌘1", "Transcript ⌥⌘2", "People ⌥⌘3"]);
  });

  it("open on Notes when there are notes, otherwise on Transcript", () => {
    expect(defaultTab({ meeting: { summary: "# Notes" } as never })).toBe("notes");
    expect(defaultTab({ meeting: { summary: " " } as never })).toBe("transcript");
    expect(defaultTab({ meeting: { summary: null } as never })).toBe("transcript");
  });
});

describe("keyboard", () => {
  it("moves, opens, deletes and closes outside a text field", () => {
    expect(meetingsKeyAction({ key: "ArrowDown" }, false)).toEqual({ kind: "move", by: 1 });
    expect(meetingsKeyAction({ key: "ArrowUp" }, false)).toEqual({ kind: "move", by: -1 });
    expect(meetingsKeyAction({ key: "Enter" }, false)).toEqual({ kind: "open" });
    expect(meetingsKeyAction({ key: "Backspace" }, false)).toEqual({ kind: "delete" });
    expect(meetingsKeyAction({ key: "Delete" }, false)).toEqual({ kind: "delete" });
    expect(meetingsKeyAction({ key: "Escape" }, false)).toEqual({ kind: "close" });
    expect(meetingsKeyAction({ key: "x" }, false)).toBeNull();
  });

  it("leaves arrows, Return and Delete to a text field, but ⌘F and Escape still work", () => {
    expect(meetingsKeyAction({ key: "ArrowDown" }, true)).toBeNull();
    expect(meetingsKeyAction({ key: "Backspace" }, true)).toBeNull();
    expect(meetingsKeyAction({ key: "f", metaKey: true }, true)).toEqual({ kind: "search" });
    expect(meetingsKeyAction({ key: "Escape" }, true)).toEqual({ kind: "close" });
    expect(meetingsKeyAction({ key: "Backspace", metaKey: true }, false)).toBeNull();
  });

  it("switches tabs on ⌥⌘1/2/3 by code, never on ⌘1/2/3 (the sidebar's)", () => {
    expect(meetingsKeyAction({ key: "¡", code: "Digit1", metaKey: true, altKey: true }, false)).toEqual({ kind: "tab", tab: "notes" });
    expect(meetingsKeyAction({ key: "£", code: "Digit3", metaKey: true, altKey: true }, true)).toEqual({ kind: "tab", tab: "people" });
    expect(meetingsKeyAction({ key: "2", metaKey: true, altKey: true }, false)).toEqual({ kind: "tab", tab: "transcript" });
    expect(meetingsKeyAction({ key: "9", code: "Digit9", metaKey: true, altKey: true }, false)).toBeNull();
    expect(meetingsKeyAction({ key: "1", code: "Digit1", metaKey: true }, false)).toBeNull();
  });

  it("moves the selection within the list", () => {
    const ids = ["a", "b", "c"];
    expect(moveSelection(ids, null, 1)).toBe("a");
    expect(moveSelection(ids, null, -1)).toBe("c");
    expect(moveSelection(ids, "a", 1)).toBe("b");
    expect(moveSelection(ids, "c", 1)).toBe("c");
    expect(moveSelection(ids, "a", -1)).toBe("a");
    expect(moveSelection(ids, "gone", 1)).toBe("a");
    expect(moveSelection([], "a", 1)).toBeNull();
  });
});

describe("delete + restore (10 s Undo)", () => {
  it("waits 10 s before a delete is due, and Undo takes it back", () => {
    const t0 = 1_000_000;
    let pending = schedulePendingDelete([], "m1", "Standup", t0);
    pending = schedulePendingDelete(pending, "m2", "Retro", t0 + 4000);
    pending = schedulePendingDelete(pending, "m1", "Standup", t0 + 1000); // re-press restarts its clock
    expect(pending.map((p) => p.id)).toEqual(["m2", "m1"]);
    expect(undoSecondsLeft(pending[1], t0 + 1000)).toBe(10);
    expect(duePendingDeletes(pending, t0 + UNDO_DELETE_MS).due).toEqual([]);
    const later = duePendingDeletes(pending, t0 + 1000 + UNDO_DELETE_MS);
    expect(later.due.map((p) => p.id)).toEqual(["m1"]);
    expect(later.waiting.map((p) => p.id)).toEqual(["m2"]);
    expect(undoSecondsLeft(pending[1], t0 + 60_000)).toBe(0);
    expect(undoPendingDelete(pending, "m2").map((p) => p.id)).toEqual(["m1"]);
  });

  it("hides a meeting from the list while it waits out its Undo", () => {
    const list = [row("a"), row("b")];
    expect(withoutPending(list, []).map((m) => m.id)).toEqual(["a", "b"]);
    expect(withoutPending(list, [{ id: "a", title: "x", at: 0 }]).map((m) => m.id)).toEqual(["b"]);
  });
});

describe("layout + recording hero", () => {
  it("goes one pane at a time under 900 px", () => {
    expect(TWO_PANE_MIN_WIDTH).toBe(900);
    expect(isNarrow(899)).toBe(true);
    expect(isNarrow(900)).toBe(false);
    expect(isNarrow(0)).toBe(false);
  });

  it("asks before Stop only under 10 s or over 3 h", () => {
    expect(stopNeedsConfirm(3)).toBe(true);
    expect(stopNeedsConfirm(600)).toBe(false);
    expect(stopNeedsConfirm(3 * 3600 + 1)).toBe(true);
  });

  it("moves the sun across the first hour and holds it", () => {
    expect(sunProgress(0)).toBe(0);
    expect(sunProgress(Number.NaN)).toBe(0);
    expect(sunProgress(1800)).toBe(0.5);
    expect(sunProgress(7200)).toBe(1);
  });
});

describe("telemetry", () => {
  it("writes one ui meeting_<event> line with short ids and one-token values", () => {
    expect(shortId("1a2b3c4d-5e6f-7a8b")).toBe("1a2b3c4d");
    expect(meetingsLogLine("delete", { id: "1a2b3c4d", undone: true })).toBe("ui meeting_delete id=1a2b3c4d undone=true");
    expect(meetingsLogLine("filter", { kind: "in person" })).toBe("ui meeting_filter kind=in_person");
    expect(meetingsLogLine("close")).toBe("ui meeting_close");
  });
});
