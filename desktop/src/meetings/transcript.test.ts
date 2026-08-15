/**
 * YV108 — the frontend half of the mixed Me/Them transcript.
 *
 * `transcript.ts` is a mirror of `meetings::render_transcript` in Rust, and a
 * mirror is only worth having if something checks that it still reflects. These
 * cases are the SAME fixtures `tests/meeting_transcript_render_two_track.rs`
 * asserts against, with the same expected output — so the screen and the
 * exported file cannot drift apart on who spoke or in what order.
 */
import { describe, expect, it } from "vitest";
import {
  isTwoTrack,
  orderedTranscript,
  speakerLabel,
  trackOf,
  MIC_TRACK,
  SYSTEM_TRACK,
  type TranscriptSegment,
} from "./transcript";

function seg(id: string, startSeconds: number, track: number | null | undefined, text: string) {
  return { id, startSeconds, track, text } as TranscriptSegment;
}

/** The same four-turn conversation the Rust test uses, grouped by track on the
 *  way in — the shape two transcription passes produce. */
const CONVERSATION: TranscriptSegment[] = [
  seg("a", 0.0, MIC_TRACK, "can we ship the notarised build on friday"),
  seg("c", 12.0, MIC_TRACK, "then we hold the price where it is"),
  seg("b", 4.5, SYSTEM_TRACK, "friday works if the signing cert lands"),
  seg("d", 17.25, SYSTEM_TRACK, "agreed nobody is discounting this quarter"),
];

describe("orderedTranscript", () => {
  it("interleaves both tracks into one time-ordered conversation", () => {
    expect(
      orderedTranscript(CONVERSATION).map((l) => [l.speaker, l.segment.text]),
    ).toEqual([
      ["Me", "can we ship the notarised build on friday"],
      ["Them", "friday works if the signing cert lands"],
      ["Me", "then we hold the price where it is"],
      ["Them", "agreed nobody is discounting this quarter"],
    ]);
  });

  it("does not group by speaker — that is the failure mode, not the feature", () => {
    const tracks = orderedTranscript(CONVERSATION).map((l) => l.track);
    expect(tracks).toEqual([MIC_TRACK, SYSTEM_TRACK, MIC_TRACK, SYSTEM_TRACK]);
  });

  it("breaks an exact tie to the mic, whichever order the rows arrived in", () => {
    const forwards = orderedTranscript([
      seg("m", 9, MIC_TRACK, "so we are agreed"),
      seg("s", 9, SYSTEM_TRACK, "so we are agreed"),
    ]);
    const backwards = orderedTranscript([
      seg("s", 9, SYSTEM_TRACK, "so we are agreed"),
      seg("m", 9, MIC_TRACK, "so we are agreed"),
    ]);
    expect(forwards.map((l) => l.speaker)).toEqual(["Me", "Them"]);
    expect(backwards.map((l) => l.speaker)).toEqual(["Me", "Them"]);
  });

  it("leaves a mic-only transcript in exactly the order it arrived", () => {
    const micOnly = [
      seg("1", 0, MIC_TRACK, "first"),
      seg("2", 4, MIC_TRACK, "second"),
      seg("3", 8, MIC_TRACK, "third"),
    ];
    expect(orderedTranscript(micOnly).map((l) => l.segment.id)).toEqual([
      "1",
      "2",
      "3",
    ]);
    expect(orderedTranscript(micOnly).every((l) => l.speaker === "Me")).toBe(true);
  });

  it("treats a broken offset as zero rather than scrambling the list", () => {
    const lines = orderedTranscript([
      seg("late", 5, MIC_TRACK, "second"),
      seg("nan", Number.NaN, SYSTEM_TRACK, "first, clamped"),
    ]);
    expect(lines.map((l) => l.segment.id)).toEqual(["nan", "late"]);
  });
});

describe("track and label", () => {
  it("defaults a missing track to the mic, as the column's DEFAULT 0 does", () => {
    expect(trackOf(seg("x", 0, undefined, "pre-migration-3 row"))).toBe(MIC_TRACK);
    expect(trackOf(seg("y", 0, null, "pre-migration-3 row"))).toBe(MIC_TRACK);
    expect(orderedTranscript([seg("z", 0, undefined, "hi")])[0].speaker).toBe("Me");
  });

  it("labels the mic Me and anything else Them", () => {
    expect(speakerLabel(MIC_TRACK)).toBe("Me");
    expect(speakerLabel(SYSTEM_TRACK)).toBe("Them");
    expect(speakerLabel(7)).toBe("Them");
  });
});

describe("isTwoTrack", () => {
  it("is true only when a second track really was recorded", () => {
    expect(isTwoTrack(CONVERSATION)).toBe(true);
    expect(isTwoTrack([seg("only", 0, MIC_TRACK, "just me")])).toBe(false);
    expect(isTwoTrack([seg("old", 0, undefined, "22-A row")])).toBe(false);
    expect(isTwoTrack([])).toBe(false);
  });
});
