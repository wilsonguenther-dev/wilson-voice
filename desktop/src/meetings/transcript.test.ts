/**
 * YV108 — the frontend half of the mixed Me/Them transcript.
 *
 * `transcript.ts` is a mirror of `meetings::render_transcript` in Rust, and a
 * mirror is only worth having if something checks that it still reflects. These
 * cases are the SAME fixtures `tests/meeting_transcript_render_two_track.rs`
 * asserts against, with the same expected output — so the screen and the
 * exported file do not drift apart on who spoke, in what order, or with what
 * text.
 *
 * That last one is not hypothetical: the review of YV108 found the mirror
 * keeping whitespace-only spans Rust had already dropped, because that rule was
 * the one rule with a fixture on only ONE side of the language boundary. Every
 * rule now has a twin here; a Rust case added without one is the next drift.
 */
import { describe, expect, it } from "vitest";
import {
  isTwoTrack,
  oneLine,
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
    // Rust keeps the raw `start_seconds` on the line and clamps only where it
    // formats and compares (`a_broken_offset_sorts_as_zero_instead_of_poisoning_the_order`
    // asserts the same offset string) — so the mirror keeps the raw number too
    // rather than quietly healing it into a 0 the export never had.
    expect(lines[0].offset).toBe("00:00:00");
    expect(Number.isNaN(lines[0].startSeconds)).toBe(true);
  });

  /**
   * The fixture from Rust's `empty_spans_never_become_blank_turns`, verbatim.
   *
   * This is the case the first review of YV108 caught: Rust dropped the blank
   * span, the mirror did not, and the same three segments rendered as 2 lines in
   * the exported file and 3 on screen — the third being a labelled "Them" row
   * with a timestamp and nothing in it.
   */
  it("never turns an empty span into a blank turn", () => {
    const lines = orderedTranscript([
      seg("a", 0.0, MIC_TRACK, "real words"),
      seg("b", 1.0, SYSTEM_TRACK, "   "),
      seg("c", 2.0, SYSTEM_TRACK, "also real"),
    ]);
    expect(lines.length).toBe(2);
    expect(lines[0].text).toBe("real words");
    expect(lines[1].text).toBe("also real");
  });

  it("renders the collapsed text, so the screen shows what the file shows", () => {
    const lines = orderedTranscript([
      seg("m", 0, MIC_TRACK, "  so   we\nare\tagreed  "),
    ]);
    expect(lines[0].text).toBe("so we are agreed");
    // The raw segment is untouched: the collapsing belongs to the rendering,
    // not to the stored row.
    expect(lines[0].segment.text).toBe("  so   we\nare\tagreed  ");
  });
});

describe("oneLine", () => {
  it("collapses interior runs and trims, exactly as Rust's one_line does", () => {
    expect(oneLine("  so   we\nare\tagreed  ")).toBe("so we are agreed");
    expect(oneLine("already one line")).toBe("already one line");
    expect(oneLine("")).toBe("");
    expect(oneLine("   ")).toBe("");
    expect(oneLine("\n\t\u00a0\u3000")).toBe("");
  });

  /**
   * The mirror deliberately does NOT use JavaScript's `\s`, because `\s` is not
   * the set `char::is_whitespace` uses: it misses U+0085 (NEL) and adds U+FEFF.
   * `a_span_of_exotic_whitespace_is_collapsed_the_same_way_rust_collapses_it`
   * pins the identical two characters on the Rust side.
   */
  it("agrees with Rust on the two characters JS and Rust disagree about", () => {
    expect(oneLine("\u0085")).toBe(""); // NEL: whitespace to Rust, and so to us
    expect(oneLine("a\u0085b")).toBe("a b");
    expect(oneLine("\uFEFF")).toBe("\uFEFF"); // ZWNBSP: NOT whitespace to Rust
    expect(orderedTranscript([seg("nel", 0, SYSTEM_TRACK, "\u0085")])).toEqual([]);
    expect(orderedTranscript([seg("bom", 0, SYSTEM_TRACK, "\uFEFF")]).length).toBe(1);
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

  /**
   * The gutter follows the RENDERED lines, not the raw rows.
   *
   * A tap that recorded only silence still produces rows — the ASR emits a span
   * with no words in it — and those rows draw nothing. Widening the speaker
   * gutter for them would reserve room on screen for a "Them" the transcript
   * does not contain and the export never had. Mirrors `meetings::is_two_track`.
   */
  it("does not widen the gutter for a tap track that renders nothing", () => {
    const blankTap = [
      seg("mic", 0, MIC_TRACK, "real words"),
      seg("tap", 1, SYSTEM_TRACK, "   "),
    ];
    expect(isTwoTrack(blankTap)).toBe(false);
    expect(orderedTranscript(blankTap).every((l) => l.track === MIC_TRACK)).toBe(
      true,
    );
    // One real word from the far side and it IS a two-track meeting again.
    expect(
      isTwoTrack([...blankTap, seg("tap2", 2, SYSTEM_TRACK, "hello")]),
    ).toBe(true);
  });
});
