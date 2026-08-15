/**
 * YV108 — the SHIPPING component, not just the rules behind it.
 *
 * `transcript.test.ts` pins `orderedTranscript`/`isTwoTrack` against the Rust
 * fixtures, but the defect the review of this item found was only visible on
 * screen: a component can hold a correct list of lines and still draw
 * `segment.text` (the raw span) beside it, which is exactly what this one did.
 * So these cases render the real `<TranscriptList>` and read the markup.
 *
 * `renderToStaticMarkup` needs no DOM and no new dependency — `react-dom` is
 * already shipped in this app — which is why the component gets a test at all
 * rather than being left to the screenshots.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import TranscriptList from "./TranscriptList";
import { MIC_TRACK, SYSTEM_TRACK, type TranscriptSegment } from "./transcript";

function seg(id: string, startSeconds: number, track: number, text: string) {
  return { id, startSeconds, track, text } as TranscriptSegment;
}

/** How many `<li>` rows the list actually drew. */
function rows(html: string): number {
  return html.split("<li>").length - 1;
}

describe("TranscriptList", () => {
  it("draws one row per rendered line — a blank span is not a turn", () => {
    const html = renderToStaticMarkup(
      <TranscriptList
        segments={[
          seg("a", 0.0, MIC_TRACK, "real words"),
          seg("b", 1.0, SYSTEM_TRACK, "   "),
          seg("c", 2.0, MIC_TRACK, "also real"),
        ]}
      />,
    );
    expect(rows(html)).toBe(2);
    expect(html).toContain("real words");
    expect(html).toContain("also real");
    // The blank span drew no labelled row: "Them" appears nowhere.
    expect(html).not.toContain("Them");
  });

  it("shows the collapsed text, the same string the export writes", () => {
    const html = renderToStaticMarkup(
      <TranscriptList segments={[seg("m", 0, MIC_TRACK, "  so   we\nare  agreed ")]} />,
    );
    expect(html).toContain('<span class="seg-text">so we are agreed</span>');
  });

  it("only widens the gutter when a second speaker is really on screen", () => {
    const blankTap = [
      seg("mic", 0, MIC_TRACK, "real words"),
      seg("tap", 1, SYSTEM_TRACK, "   "),
    ];
    expect(renderToStaticMarkup(<TranscriptList segments={blankTap} />)).toContain(
      '<ol class="transcript">',
    );
    expect(
      renderToStaticMarkup(
        <TranscriptList segments={[...blankTap, seg("tap2", 2, SYSTEM_TRACK, "hello")]} />,
      ),
    ).toContain('<ol class="transcript two-track">');
  });

  it("interleaves the two tracks in one list, in clock order", () => {
    const html = renderToStaticMarkup(
      <TranscriptList
        segments={[
          seg("a", 0.0, MIC_TRACK, "can we ship on friday"),
          seg("c", 12.0, MIC_TRACK, "then we hold the price"),
          seg("b", 4.5, SYSTEM_TRACK, "friday works"),
        ]}
      />,
    );
    const order = [
      html.indexOf("can we ship on friday"),
      html.indexOf("friday works"),
      html.indexOf("then we hold the price"),
    ];
    expect(order).toEqual([...order].sort((x, y) => x - y));
    expect(order.every((i) => i >= 0)).toBe(true);
    expect(html).toContain('<span class="seg-time">00:00:04</span>');
  });
});
