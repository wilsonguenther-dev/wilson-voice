/**
 * YV129 — the SHIPPING component, not just the rules behind it.
 *
 * `speakerChips.test.ts` pins the batching rules against the Rust fixtures. The
 * class of defect that survives that, and only shows up on screen, is the one
 * YV108's review found: a component can hold a correct list and still draw the
 * wrong field beside it. So these cases render the real `<SpeakerChipRow>` and
 * read the markup — in particular that **one cluster draws one row**, which is
 * the item's whole claim and is a property of the rendering as much as of the
 * list.
 *
 * `renderToStaticMarkup` needs no DOM and no new dependency, the same reason
 * `TranscriptList.test.tsx` uses it.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import SpeakerChipRow from "./SpeakerChipRow";
import { chipRow, type ChipFloor, type ClusterDecisionView } from "./speakerChips";

const FLOOR: ChipFloor = { minSpeechSeconds: 30, minTurns: 3 };
const ROSTER = [
  { id: "p_jeisil", displayName: "Jeisil", isMe: false },
  { id: "p_aidan", displayName: "Aidan", isMe: false },
];

function decision(
  clusterIndex: number,
  speechSeconds: number,
  turns: number,
): ClusterDecisionView {
  return {
    cluster: {
      clusterIndex,
      label: `Speaker ${clusterIndex + 1}`,
      speechSeconds,
      turns,
    },
    result: { kind: "new" },
  };
}

/** How many `<li>` rows the row actually drew. */
function rows(html: string): number {
  return html.split("<li>").length - 1;
}

describe("SpeakerChipRow", () => {
  it("draws one row per cluster — six speakers, four questions", () => {
    const row = chipRow(
      true,
      [
        decision(0, 210, 18),
        decision(1, 95, 11),
        decision(2, 61, 7),
        decision(3, 33, 4),
        decision(4, 12, 3),
        decision(5, 44, 2),
      ],
      [],
      ROSTER,
      FLOOR,
    );
    const html = renderToStaticMarkup(<SpeakerChipRow row={row} />);
    expect(rows(html)).toBe(4);
    expect(html).toContain("4 voices this recording could not name.");
    expect(html).toContain("2 quieter voices were grouped as Other.");
    // Every chip offers the whole roster plus `+ new`, and nothing else.
    expect(html.split("Jeisil").length - 1).toBe(4);
    expect(html.split("+ new").length - 1).toBe(4);
  });

  it("draws the pre-selected suggestion once, and the rest of the roster beside it", () => {
    const row = chipRow(
      true,
      [
        {
          cluster: {
            clusterIndex: 3,
            label: "Speaker 4",
            speechSeconds: 210,
            turns: 9,
          },
          result: {
            kind: "suggested",
            profileId: "p_jeisil",
            score: 0.7071,
            conditionKey: "laptop_mic_near",
          },
        },
      ],
      [],
      ROSTER,
      FLOOR,
    );
    const html = renderToStaticMarkup(<SpeakerChipRow row={row} />);
    expect(rows(html)).toBe(1);
    expect(html).toContain("chip-candidate suggested");
    expect(html.split("Jeisil").length - 1).toBe(1);
    expect(html).toContain("Aidan");
    expect(html).toContain("Speaker 4");
    // The speech time is on screen, in a person's units.
    expect(html).toContain("3m 30s of speech");
    expect(html).toContain("One voice this recording could not name.");
  });

  it("draws nothing at all when every voice is already named", () => {
    const html = renderToStaticMarkup(
      <SpeakerChipRow row={{ chips: [], rolledIntoOther: 0 }} />,
    );
    expect(html).toBe("");
  });

  it("still draws the Other line when the only clusters left were quiet ones", () => {
    const row = chipRow(true, [decision(0, 4, 1)], [], ROSTER, FLOOR);
    const html = renderToStaticMarkup(<SpeakerChipRow row={row} />);
    expect(rows(html)).toBe(0);
    expect(html).toContain("1 quieter voice was grouped as Other.");
    // No question was asked, so the count of unnamed voices is not claimed.
    expect(html).not.toContain("could not name");
  });
});
