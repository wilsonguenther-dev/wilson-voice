/**
 * Merge train (main green) — `meetings/notes.ts` is a pure module, so it
 * carries its own sibling test (purity.test.ts) beside the view-level checks
 * in NotesView.test.tsx.
 */
import { describe, expect, it } from "vitest";
import { inlineRuns, parseNotes } from "./notes";

describe("parseNotes", () => {
  it("reads the title, sections, lists, paragraphs and the italic note", () => {
    const md = [
      "## Weekly sync",
      "",
      "### Summary",
      "We agreed on the plan.",
      "It ships Friday.",
      "",
      "### Action items",
      "- Send the quote",
      "* Book the room",
      "",
      "_Notes were produced on this Mac._",
      "## Extra",
      "Tail paragraph",
      "- trailing item",
      "after the list",
      "#### Deep heading",
    ].join("\r\n");
    const p = parseNotes(md);
    expect(p.title).toBe("Weekly sync");
    expect(p.sections.map((s) => s.heading)).toEqual(["Summary", "Action items", "Extra", "Deep heading"]);
    expect(p.sections[0].blocks).toEqual([{ kind: "paragraph", text: "We agreed on the plan. It ships Friday." }]);
    expect(p.sections[1].blocks).toEqual([
      { kind: "list", items: ["Send the quote", "Book the room"] },
      { kind: "note", text: "Notes were produced on this Mac." },
    ]);
    expect(p.sections[2].blocks).toEqual([
      { kind: "paragraph", text: "Tail paragraph" },
      { kind: "list", items: ["trailing item"] },
      { kind: "paragraph", text: "after the list" },
    ]);
    expect(p.sections[3].blocks).toEqual([]);
  });

  it("an old-layout summary is one untitled section; empty input is no sections", () => {
    expect(parseNotes("One.\n\nTwo.")).toEqual({
      title: null,
      sections: [{ heading: null, blocks: [{ kind: "paragraph", text: "One." }, { kind: "paragraph", text: "Two." }] }],
    });
    expect(parseNotes("")).toEqual({ title: null, sections: [] });
  });
});

describe("inlineRuns", () => {
  it("splits bold runs and keeps plain text around them", () => {
    expect(inlineRuns("a **b** c **d**")).toEqual([
      { text: "a ", bold: false },
      { text: "b", bold: true },
      { text: " c ", bold: false },
      { text: "d", bold: true },
    ]);
    expect(inlineRuns("plain")).toEqual([{ text: "plain", bold: false }]);
    expect(inlineRuns("")).toEqual([]);
  });
});
