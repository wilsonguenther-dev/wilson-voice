/**
 * #250 round 2 — Open transcript draws the notes as sections, not raw
 * Markdown. Rendered with `renderToStaticMarkup` (no DOM, no new dependency),
 * the same way `TranscriptList.test.tsx` pins the transcript.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import NotesView from "./NotesView";
import { inlineRuns, parseNotes } from "./notes";

/** The notes `summarize::render_notes` stores, for the PR #250 fixture. */
const NOTES = `## Pricing Review for Drivia Consulting Tier

### Summary

The pricing review for the Drivia consulting tier is complete. The starter package will remain at forty-nine dollars a month.

### Key points

- The starter package will remain at forty-nine dollars a month.
- A new pricing document will be sent to the team by Friday.
- Wilson will book a demo with Liberty University next week.

### Action items

- **Jeisil** — Jeisil will send the updated pricing doc to the team by Friday (00:00:04)
- **Wilson** — Wilson to book the demo with Liberty University next week (00:00:12)

### Decisions

- We also decided to drop the free trial for enterprise accounts (00:00:20)
- The starter package will remain at forty-nine dollars a month (00:00:04)

### Open questions

- The open question is whether annual billing gets a discount (00:00:16)`;

describe("meeting notes view", () => {
  it("parses the stored notes into a title and the five sections", () => {
    const parsed = parseNotes(NOTES);
    expect(parsed.title).toBe("Pricing Review for Drivia Consulting Tier");
    expect(parsed.sections.map((s) => s.heading)).toEqual([
      "Summary",
      "Key points",
      "Action items",
      "Decisions",
      "Open questions",
    ]);
    expect(parsed.sections[0].blocks[0].kind).toBe("paragraph");
    expect(parsed.sections[2].blocks).toEqual([
      {
        kind: "list",
        items: [
          "**Jeisil** — Jeisil will send the updated pricing doc to the team by Friday (00:00:04)",
          "**Wilson** — Wilson to book the demo with Liberty University next week (00:00:12)",
        ],
      },
    ]);
  });

  it("renders sections, lists and bold owners — no raw Markdown left", () => {
    const html = renderToStaticMarkup(<NotesView markdown={NOTES} />);
    expect(html).not.toMatch(/(^|>)\s*#{2,3} /);
    expect(html).not.toContain("**");
    expect(html).not.toContain("<li>- ");
    expect(html).toContain("<strong>Jeisil</strong>");
    expect(html.split("<li>").length - 1).toBe(8);
    expect(html).toMatchSnapshot();
  });

  it("shows a truncation note as a muted line and escapes text", () => {
    const html = renderToStaticMarkup(
      <NotesView
        markdown={"A <script>x</script> line.\n\n_Part of this meeting did not fit the model in one pass._"}
      />,
    );
    expect(html).toContain("&lt;script&gt;");
    expect(html).not.toContain("<script>");
    expect(html).toContain('<p class="muted meeting-notes-note"><em>Part of this meeting');
  });

  it("an old-layout summary (no headings) still reads as paragraphs", () => {
    const parsed = parseNotes("One paragraph.\n\nAnother one.");
    expect(parsed.title).toBeNull();
    expect(parsed.sections).toHaveLength(1);
    expect(parsed.sections[0].blocks.map((b) => b.kind)).toEqual(["paragraph", "paragraph"]);
  });

  it("splits bold runs", () => {
    expect(inlineRuns("**A** — b")).toEqual([
      { text: "A", bold: true },
      { text: " — b", bold: false },
    ]);
  });
});
