import { describe, expect, it } from "vitest";
import {
  closeTab,
  insertAtCaret,
  landOn,
  needsFinalize,
  openedCaret,
  openTab,
  pruneTabs,
  UNTITLED,
} from "./notesModel";

describe("insertAtCaret", () => {
  it("lands at the caret and moves the caret past it", () => {
    expect(insertAtCaret("ab", 1, 1, "X")).toEqual({ content: "a X b", caret: 4 });
  });
  it("replaces a selection", () => {
    expect(insertAtCaret("call bob today", 5, 8, "alice")).toEqual({
      content: "call alice today",
      caret: 10,
    });
  });
  it("adds no double spaces at whitespace or the edges", () => {
    expect(insertAtCaret("", 0, 0, " hi ")).toEqual({ content: "hi", caret: 2 });
    expect(insertAtCaret("one ", 4, 4, "two")).toEqual({ content: "one two", caret: 7 });
    expect(insertAtCaret("line\n", 5, 5, "next")).toEqual({ content: "line\nnext", caret: 9 });
  });
  it("clamps a stale caret and ignores an empty take", () => {
    expect(insertAtCaret("abc", 99, 99, "d").content).toBe("abc d");
    expect(insertAtCaret("abc", 1, 1, "  ")).toEqual({ content: "abc", caret: 1 });
  });
});

describe("tabs", () => {
  it("opens without duplicates", () => {
    expect(openTab(["a"], "b")).toEqual(["a", "b"]);
    expect(openTab(["a", "b"], "a")).toEqual(["a", "b"]);
  });
  it("closing the active tab activates its neighbour", () => {
    expect(closeTab(["a", "b", "c"], "b", "b")).toEqual({ tabs: ["a", "c"], active: "c" });
    expect(closeTab(["a", "b", "c"], "c", "c")).toEqual({ tabs: ["a", "b"], active: "b" });
    expect(closeTab(["a"], "a", "a")).toEqual({ tabs: [], active: null });
  });
  it("closing another tab keeps the active one", () => {
    expect(closeTab(["a", "b"], "a", "b")).toEqual({ tabs: ["b"], active: "b" });
  });
  it("prunes deleted notes", () => {
    expect(pruneTabs(["a", "b"], [{ id: "b" }])).toEqual(["b"]);
  });
});

describe("needsFinalize", () => {
  it("is false for an untouched note and true after an edit", () => {
    const f = { title: "T", content: "x" };
    expect(needsFinalize({ ...f }, f)).toBe(false);
    expect(needsFinalize({ title: "T", content: "xy" }, f)).toBe(true);
    expect(needsFinalize({ title: "T2", content: "x" }, f)).toBe(true);
  });
  it("a brand-new empty note writes no version", () => {
    expect(needsFinalize({ title: UNTITLED, content: "" }, undefined)).toBe(false);
    expect(needsFinalize({ title: UNTITLED, content: "a" }, undefined)).toBe(true);
  });
});

// fix-b5-1 — the pill's Note opens the Scratchpad ON the note it just made.
describe("landOn (the note the window was opened for)", () => {
  // list_notes order: pinned DESC, modified DESC.
  const A = { id: "a" };
  const B = { id: "b" };

  it("opened_note_becomes_active_with_a_tab_over_the_active_note", () => {
    expect(landOn(["a"], "a", [B, A], "b")).toEqual({ tabs: ["a", "b"], active: "b" });
  });

  it("opened_note_wins_even_when_the_active_note_is_pinned", () => {
    // A pinned sorts first, so the old refresh kept A (or picked list[0] = A).
    expect(landOn(["a"], "a", [A, B], "b")).toEqual({ tabs: ["a", "b"], active: "b" });
    // First open of the session: no tabs, no active, a pinned A on top.
    expect(landOn([], null, [A, B], "b")).toEqual({ tabs: ["b"], active: "b" });
  });

  it("opened_note_already_in_a_tab_is_not_duplicated", () => {
    expect(landOn(["b", "a"], "a", [A, B], "b")).toEqual({ tabs: ["b", "a"], active: "b" });
  });

  it("without_an_opened_note_the_active_note_is_kept", () => {
    expect(landOn(["a", "b"], "a", [B, A], null)).toEqual({ tabs: ["a", "b"], active: "a" });
    expect(landOn([], null, [A, B], undefined)).toEqual({ tabs: ["a"], active: "a" });
    // The active note was deleted elsewhere: fall back to the list's first.
    expect(landOn(["x"], "x", [A, B], null)).toEqual({ tabs: ["a"], active: "a" });
  });

  it("an_opened_note_missing_from_the_list_falls_back", () => {
    expect(landOn(["a"], "a", [A], "gone")).toEqual({ tabs: ["a"], active: "a" });
  });

  it("an_empty_list_leaves_no_tabs_and_nothing_active", () => {
    expect(landOn(["a"], "a", [], "a")).toEqual({ tabs: [], active: null });
  });
});

describe("openedCaret", () => {
  it("caret_lands_after_the_timestamp_seed", () => {
    expect(openedCaret("b", "b", "[12:34] ")).toBe(8);
    expect(openedCaret("b", "b", "[1:02:03] ")).toBe("[1:02:03] ".length);
  });
  it("nothing_to_place_until_the_opened_note_is_active", () => {
    expect(openedCaret("b", "a", "[12:34] ")).toBeNull();
    expect(openedCaret(null, "b", "[12:34] ")).toBeNull();
    expect(openedCaret("b", "b", undefined)).toBeNull();
  });
});
