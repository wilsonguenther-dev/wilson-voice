/**
 * YV130 — the SHIPPING components, not just the rules behind them.
 *
 * `corrections.test.ts` pins the copy and the menus; what these cases pin is
 * that the markup actually draws them, because the defect finding #31 is about
 * is a UI-shaped one: a "Not now" that exists in a helper and not on screen is
 * an automatic feature with a comment claiming otherwise.
 *
 * `renderToStaticMarkup` needs no DOM and no new dependency, the same reason
 * `TranscriptList.test.tsx` uses it.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import RelabelOffer from "./RelabelOffer";
import SegmentCorrectionMenu from "./SegmentCorrectionMenu";
import type { CorrectableSegment } from "./corrections";

const noop = () => {};

function seg(over: Partial<CorrectableSegment> = {}): CorrectableSegment {
  return {
    id: "s1",
    speakerId: null,
    speakerLocked: false,
    clusterIndex: 0,
    ...over,
  };
}

const speakers = [
  { id: "p-wilson", displayName: "Wilson" },
  { id: "p-jeisil", displayName: "Jeisil" },
];

describe("RelabelOffer", () => {
  it("draws the question and BOTH answers — never Apply alone", () => {
    const html = renderToStaticMarkup(
      <RelabelOffer
        displayName="Wilson"
        meetings={3}
        segments={9}
        onApply={noop}
        onDismiss={noop}
        onUndo={noop}
      />,
    );
    expect(html).toContain(
      "Wilson&#x27;s voice appears in 3 earlier meetings — label them too?",
    );
    expect(html).toContain(">Apply<");
    expect(html).toContain(">Not now<");
    // Nothing has happened yet, so there is nothing to undo.
    expect(html).not.toContain(">Undo<");
  });

  it("draws nothing at all when there is nothing to offer", () => {
    const html = renderToStaticMarkup(
      <RelabelOffer
        displayName="Wilson"
        meetings={0}
        segments={0}
        onApply={noop}
        onDismiss={noop}
        onUndo={noop}
      />,
    );
    expect(html).toBe("");
  });

  it("becomes the receipt and the undo once applied", () => {
    const html = renderToStaticMarkup(
      <RelabelOffer
        displayName="Wilson"
        meetings={3}
        segments={9}
        applied={{ touched: 8, skippedLocked: 1 }}
        onApply={noop}
        onDismiss={noop}
        onUndo={noop}
      />,
    );
    expect(html).toContain("Labelled 8 turns as Wilson.");
    expect(html).toContain("1 turn you had already named was left as it was.");
    expect(html).toContain(">Undo<");
    // The question is gone: this is a report now, not an offer.
    expect(html).not.toContain(">Apply<");
    expect(html).not.toContain(">Not now<");
  });
});

describe("SegmentCorrectionMenu", () => {
  it("offers every voice, the explicit unassign, and marks the current one", () => {
    const html = renderToStaticMarkup(
      <SegmentCorrectionMenu
        segment={seg({ speakerId: "p-jeisil" })}
        speakers={speakers}
        clusters={[0, 1]}
        membersWithEmbedding={4}
        onReassign={noop}
        onMerge={noop}
        onSplit={noop}
      />,
    );
    expect(html).toContain(">Wilson<");
    expect(html).toContain(">Jeisil<");
    expect(html).toContain(">Unknown speaker<");
    expect(html).toContain('aria-pressed="true"');
  });

  it("offers the merge next to the turn, and never the cluster itself", () => {
    const html = renderToStaticMarkup(
      <SegmentCorrectionMenu
        segment={seg({ clusterIndex: 1 })}
        speakers={speakers}
        clusters={[0, 1, 2]}
        membersWithEmbedding={4}
        onReassign={noop}
        onMerge={noop}
        onSplit={noop}
      />,
    );
    expect(html).toContain("Same person as…");
    expect(html).toContain(">Speaker 1<");
    expect(html).toContain(">Speaker 3<");
    expect(html).not.toContain(">Speaker 2<");
  });

  it("hides split where there are no retained embeddings to split by", () => {
    const html = renderToStaticMarkup(
      <SegmentCorrectionMenu
        segment={seg()}
        speakers={speakers}
        clusters={[0, 1]}
        membersWithEmbedding={1}
        onReassign={noop}
        onMerge={noop}
        onSplit={noop}
      />,
    );
    expect(html).not.toContain("Split this speaker in two");
  });

  it("shows split when the cluster has two members to split", () => {
    const html = renderToStaticMarkup(
      <SegmentCorrectionMenu
        segment={seg()}
        speakers={speakers}
        clusters={[0, 1]}
        membersWithEmbedding={2}
        onReassign={noop}
        onMerge={noop}
        onSplit={noop}
      />,
    );
    expect(html).toContain("Split this speaker in two");
  });

  it("draws no loading state, because a correction is a database write", () => {
    const html = renderToStaticMarkup(
      <SegmentCorrectionMenu
        segment={seg()}
        speakers={speakers}
        clusters={[0, 1]}
        membersWithEmbedding={2}
        onReassign={noop}
        onMerge={noop}
        onSplit={noop}
      />,
    );
    expect(html).not.toMatch(/spinner|analysing|analyzing|progress/i);
  });
});
