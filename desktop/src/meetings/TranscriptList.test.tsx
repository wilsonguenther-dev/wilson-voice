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
import TranscriptList, { echoHiddenLine } from "./TranscriptList";
import {
  MIC_TRACK,
  SYSTEM_TRACK,
  UNCLUSTERED_SPEAKER_LABEL,
  type TranscriptSegment,
} from "./transcript";

function seg(id: string, startSeconds: number, track: number, text: string) {
  return { id, startSeconds, track, text } as TranscriptSegment;
}

/** How many `<li>` rows the list actually drew. */
function rows(html: string): number {
  return html.split("<li>").length - 1;
}

describe("TranscriptList", () => {
  it("yap24-NT11: hidden echo rows are counted on one line and never drawn until Show", () => {
    expect(echoHiddenLine(0)).toBeNull();
    expect(echoHiddenLine(1)).toBe("1 line hidden as speaker echo");
    expect(echoHiddenLine(3)).toBe("3 lines hidden as speaker echo");
    const html = renderToStaticMarkup(
      <TranscriptList
        kind="virtual"
        segments={[seg("a", 0.0, MIC_TRACK, "my own words")]}
        hidden={[seg("e1", 1.0, MIC_TRACK, "the far side again"), seg("e2", 2.0, MIC_TRACK, "and again")]}
      />,
    );
    expect(html).toContain("2 lines hidden as speaker echo");
    expect(html).toContain("Show");
    expect(html).not.toContain("the far side again");
    expect(rows(html)).toBe(1);
    const plain = renderToStaticMarkup(
      <TranscriptList kind="virtual" segments={[seg("a", 0.0, MIC_TRACK, "my own words")]} />,
    );
    expect(plain).not.toContain("hidden as speaker echo");
  });

  it("draws one row per rendered line — a blank span is not a turn", () => {
    const html = renderToStaticMarkup(
      <TranscriptList
        kind="virtual"
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
    // The blank span drew no labelled row: "Call" appears nowhere.
    expect(html).not.toContain("Call");
  });

  it("shows the collapsed text, the same string the export writes", () => {
    const html = renderToStaticMarkup(
      <TranscriptList
        kind="virtual"
        segments={[seg("m", 0, MIC_TRACK, "  so   we\nare  agreed ")]}
      />,
    );
    expect(html).toContain('<span class="seg-text">so we are agreed</span>');
  });

  it("only widens the gutter when a second speaker is really on screen", () => {
    const blankTap = [
      seg("mic", 0, MIC_TRACK, "real words"),
      seg("tap", 1, SYSTEM_TRACK, "   "),
    ];
    expect(
      renderToStaticMarkup(<TranscriptList kind="virtual" segments={blankTap} />),
    ).toContain('<ol class="transcript">');
    expect(
      renderToStaticMarkup(
        <TranscriptList
          kind="virtual"
          segments={[...blankTap, seg("tap2", 2, SYSTEM_TRACK, "hello")]}
        />,
      ),
    ).toContain('<ol class="transcript two-track">');
  });

  it("interleaves the two tracks in one list, in clock order", () => {
    const html = renderToStaticMarkup(
      <TranscriptList
        kind="virtual"
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

  /**
   * YV125 — the component reads the meeting's kind, not just its tracks. A
   * meeting whose kind was never answered draws the un-clustered label, and no
   * amount of segment shape brings "You" back.
   */
  it("does not call the microphone Me when the meeting was never a call", () => {
    const room = [
      seg("a", 0.0, MIC_TRACK, "let us start with the release checklist"),
      seg("b", 4.0, MIC_TRACK, "friday works if the signing cert lands"),
    ];
    for (const kind of ["unknown", "in_person", undefined]) {
      const html = renderToStaticMarkup(
        <TranscriptList kind={kind} segments={room} />,
      );
      // yap-notetaker-notes: a single mic track carries no speaker label at all.
      expect(html).not.toContain(`>${UNCLUSTERED_SPEAKER_LABEL}</span>`);
      expect(html).not.toContain("seg-who");
    }
    // …and the one branch that still does.
    const call = [...room, seg("c", 6.0, SYSTEM_TRACK, "sounds right")];
    const html = renderToStaticMarkup(
      <TranscriptList kind="virtual" segments={call} />,
    );
    expect(html).toContain(">You</span>");
    expect(html).toContain(">Call</span>");
  });
});
