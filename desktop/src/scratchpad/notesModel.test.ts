import { describe, expect, it } from "vitest";
import { closeTab, insertAtCaret, needsFinalize, openTab, pruneTabs, UNTITLED } from "./notesModel";

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
