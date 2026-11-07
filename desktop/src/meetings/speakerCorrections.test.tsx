/**
 * yap-1106-auto-label-and-correct — the webview half of speaker corrections.
 *
 * The relabel / merge / split decisions are Rust reducers
 * (`speaker_profiles::apply_correction`, unit-tested in `correction_tests`);
 * these cases pin what the screen does with the rows the backend returns, the
 * chip menu's pure helpers, and that a corrected name is what the list draws.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import TranscriptList, { showsSpeakers } from "./TranscriptList";
import {
  applyLabelRows,
  correctionSentence,
  mergeChoices,
  nameTarget,
  orderedTranscript,
  splitFromHere,
  UNDO_WINDOW_MS,
  type LabelRow,
  type TranscriptSegment,
} from "./transcript";

const seg = (
  id: string,
  startSeconds: number,
  clusterIndex: number | null,
  speakerName: string | null = null,
): TranscriptSegment => ({
  id,
  startSeconds,
  endSeconds: startSeconds + 3,
  text: `line ${id}`,
  track: 0,
  clusterIndex,
  speakerName,
});

/** In person: A (cluster 0, auto-labelled John) and B (cluster 1) alternate. */
const meeting = (): TranscriptSegment[] => [
  seg("s1", 0, 0, "John"),
  seg("s2", 3, 1),
  seg("s3", 6, 0, "John"),
  seg("s4", 9, 1),
  seg("s5", 12, 0, "John"),
];

const row = (segmentId: string, clusterIndex: number | null, speakerName: string | null): LabelRow => ({
  segmentId,
  track: 0,
  clusterIndex,
  profileId: speakerName ? 7 : null,
  speakerName,
  locked: 42,
});

describe("applyLabelRows", () => {
  it("a relabel changes the name on every returned line, and only those", () => {
    const out = applyLabelRows(meeting(), [row("s1", 0, "Alex"), row("s3", 0, "Alex"), row("s5", 0, "Alex")]);
    const speakers = orderedTranscript(out, "in_person").map((l) => l.speaker);
    expect(speakers).toEqual(["Alex", "Speaker 2", "Alex", "Speaker 2", "Alex"]);
  });

  it("a merge moves the cluster and the name together", () => {
    const out = applyLabelRows(meeting(), [row("s2", 0, "John"), row("s4", 0, "John")]);
    expect(out.map((s) => s.clusterIndex)).toEqual([0, 0, 0, 0, 0]);
    expect(orderedTranscript(out, "in_person").every((l) => l.speaker === "John")).toBe(true);
  });

  it("a split clears the name, so the line falls back to its new Speaker N", () => {
    const out = applyLabelRows(meeting(), [row("s5", 2, null)]);
    const lines = orderedTranscript(out, "in_person");
    const last = lines[lines.length - 1];
    expect(last.segment.clusterIndex).toBe(2);
    expect(last.speaker).toMatch(/^Speaker \d$/);
  });

  it("no rows is the identity", () => {
    expect(applyLabelRows(meeting(), [])).toEqual(meeting());
  });
});

describe("chip menu helpers", () => {
  it("merge offers every other speaker on the track once, labelled as shown", () => {
    expect(mergeChoices(meeting(), "in_person", 0, 1)).toEqual([{ clusterIndex: 0, label: "John" }]);
    expect(mergeChoices(meeting(), "in_person", 0, 0)).toEqual([{ clusterIndex: 1, label: "Speaker 2" }]);
  });

  it("split takes this line and the speaker's later lines, never all of them", () => {
    expect(splitFromHere(meeting(), 0, 0, "s3")).toEqual(["s3", "s5"]);
    expect(splitFromHere(meeting(), 0, 0, "s1")).toEqual([]);
    expect(splitFromHere(meeting(), 0, 0, "s2")).toEqual([]);
  });

  it("a blank name is no target", () => {
    expect(nameTarget("   ")).toBeNull();
    expect(nameTarget("  Alex \n Kim ")).toEqual({ kind: "newName", name: "Alex Kim" });
  });

  it("the undo bar says what happened, for ten seconds", () => {
    expect(UNDO_WINDOW_MS).toBe(10_000);
    expect(
      correctionSentence({ kind: "rename", track: 0, clusterIndex: 0, target: { kind: "newName", name: "Alex" } }),
    ).toBe("Renamed to Alex");
    expect(correctionSentence({ kind: "merge", track: 0, fromCluster: 1, intoCluster: 0 }, "John")).toBe(
      "Merged with John",
    );
    expect(correctionSentence({ kind: "split", track: 0, clusterIndex: 0, segmentIds: ["s5"] })).toBe(
      "Split into a new speaker",
    );
  });

  it("names every correction kind, including the fallbacks without a typed name", () => {
    expect(correctionSentence({ kind: "rename", track: 0, clusterIndex: 0, target: { kind: "profile", profileId: 7 } })).toBe(
      "Speaker renamed",
    );
    expect(
      correctionSentence({ kind: "someoneElse", track: 0, segmentIds: ["s3"], target: { kind: "newName", name: "Priya" } }),
    ).toBe("1 line moved to Priya");
    expect(correctionSentence({ kind: "someoneElse", track: 0, segmentIds: ["s3", "s5"], target: { kind: "me" } })).toBe(
      "2 lines moved to someone else",
    );
    expect(correctionSentence({ kind: "merge", track: 0, fromCluster: 1, intoCluster: 0 })).toBe("Speakers merged");
  });
});

describe("TranscriptList speaker chips", () => {
  it("an attributed in-person meeting shows its speakers; an unattributed mic does not", () => {
    expect(showsSpeakers(meeting())).toBe(true);
    expect(showsSpeakers([seg("a", 0, null), seg("b", 3, null)])).toBe(false);
  });

  it("with a correction handler, an attributed chip is a button that opens the menu", () => {
    const html = renderToStaticMarkup(
      <TranscriptList segments={meeting()} kind="in_person" onCorrect={async () => null} />,
    );
    expect(html).toContain("seg-who-chip");
    expect(html).toContain('aria-haspopup="dialog"');
    expect(html).toContain(">John<");
  });

  it("without a handler the chip stays a plain label", () => {
    const html = renderToStaticMarkup(<TranscriptList segments={meeting()} kind="in_person" />);
    expect(html).not.toContain("seg-who-chip");
    expect(html).toContain(">John<");
  });
});
