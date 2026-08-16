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
  diarizationTarget,
  isTwoTrack,
  oneLine,
  orderedTranscript,
  showsOverlapCaveat,
  speakerLabel,
  trackOf,
  MIC_TRACK,
  OVERLAP_CAVEAT,
  SYSTEM_TRACK,
  UNCLUSTERED_SPEAKER_LABEL,
  type MeetingKind,
  type TranscriptSegment,
} from "./transcript";

/**
 * YV125 — the kind every fixture below runs under unless it says otherwise: a
 * CALL, which is the one configuration in which the microphone holds exactly
 * one speaker and its lines read "Me". `meeting_kind_branch.rs` is the Rust
 * twin of the branch itself; the "diarizationTarget" block near the bottom is
 * this file's copy of that truth table.
 */
const CALL: MeetingKind = "virtual";

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
      orderedTranscript(CONVERSATION, CALL).map((l) => [l.speaker, l.segment.text]),
    ).toEqual([
      ["Me", "can we ship the notarised build on friday"],
      ["Them", "friday works if the signing cert lands"],
      ["Me", "then we hold the price where it is"],
      ["Them", "agreed nobody is discounting this quarter"],
    ]);
  });

  it("does not group by speaker — that is the failure mode, not the feature", () => {
    const tracks = orderedTranscript(CONVERSATION, CALL).map((l) => l.track);
    expect(tracks).toEqual([MIC_TRACK, SYSTEM_TRACK, MIC_TRACK, SYSTEM_TRACK]);
  });

  it("breaks an exact tie to the mic, whichever order the rows arrived in", () => {
    const forwards = orderedTranscript(
      [
        seg("m", 9, MIC_TRACK, "so we are agreed"),
        seg("s", 9, SYSTEM_TRACK, "so we are agreed"),
      ],
      CALL,
    );
    const backwards = orderedTranscript(
      [
        seg("s", 9, SYSTEM_TRACK, "so we are agreed"),
        seg("m", 9, MIC_TRACK, "so we are agreed"),
      ],
      CALL,
    );
    expect(forwards.map((l) => l.speaker)).toEqual(["Me", "Them"]);
    expect(backwards.map((l) => l.speaker)).toEqual(["Me", "Them"]);
  });

  it("leaves a mic-only transcript in exactly the order it arrived", () => {
    const micOnly = [
      seg("1", 0, MIC_TRACK, "first"),
      seg("2", 4, MIC_TRACK, "second"),
      seg("3", 8, MIC_TRACK, "third"),
    ];
    expect(orderedTranscript(micOnly, CALL).map((l) => l.segment.id)).toEqual([
      "1",
      "2",
      "3",
    ]);
    // NOT "Me" (YV125), and under `virtual` at that: the user said this was a
    // call and the call's audio never arrived on a second track, so the
    // microphone carried whatever was in the room. Rust's twin is
    // `a_mic_only_meeting_walks_the_same_chain_and_never_grows_a_them`.
    expect(
      orderedTranscript(micOnly, CALL).every(
        (l) => l.speaker === UNCLUSTERED_SPEAKER_LABEL,
      ),
    ).toBe(true);
  });

  it("treats a broken offset as zero rather than scrambling the list", () => {
    const lines = orderedTranscript(
      [
        seg("late", 5, MIC_TRACK, "second"),
        seg("nan", Number.NaN, SYSTEM_TRACK, "first, clamped"),
      ],
      CALL,
    );
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
    const lines = orderedTranscript(
      [
        seg("a", 0.0, MIC_TRACK, "real words"),
        seg("b", 1.0, SYSTEM_TRACK, "   "),
        seg("c", 2.0, SYSTEM_TRACK, "also real"),
      ],
      CALL,
    );
    expect(lines.length).toBe(2);
    expect(lines[0].text).toBe("real words");
    expect(lines[1].text).toBe("also real");
  });

  it("renders the collapsed text, so the screen shows what the file shows", () => {
    const lines = orderedTranscript(
      [seg("m", 0, MIC_TRACK, "  so   we\nare\tagreed  ")],
      CALL,
    );
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
    expect(orderedTranscript([seg("nel", 0, SYSTEM_TRACK, "\u0085")], CALL)).toEqual(
      [],
    );
    expect(
      orderedTranscript([seg("bom", 0, SYSTEM_TRACK, "\uFEFF")], CALL).length,
    ).toBe(1);
  });
});

describe("track and label", () => {
  it("defaults a missing track to the mic, as the column's DEFAULT 0 does", () => {
    expect(trackOf(seg("x", 0, undefined, "pre-migration-3 row"))).toBe(MIC_TRACK);
    expect(trackOf(seg("y", 0, null, "pre-migration-3 row"))).toBe(MIC_TRACK);
    // A pre-migration-3 row in a mic-only meeting: the mic, and — since YV125 —
    // not automatically this user.
    expect(
      orderedTranscript([seg("z", 0, undefined, "hi")], CALL)[0].speaker,
    ).toBe(UNCLUSTERED_SPEAKER_LABEL);
  });

  it("labels the mic Me only where the mechanism supports it", () => {
    expect(speakerLabel(MIC_TRACK, "micIsMe")).toBe("Me");
    expect(speakerLabel(MIC_TRACK, "clusterTrackA")).toBe(
      UNCLUSTERED_SPEAKER_LABEL,
    );
    // The system track is not this user's microphone under EITHER branch —
    // that is mechanical, not inferred, so no kind can move it.
    expect(speakerLabel(SYSTEM_TRACK, "micIsMe")).toBe("Them");
    expect(speakerLabel(SYSTEM_TRACK, "clusterTrackA")).toBe("Them");
    expect(speakerLabel(7, "micIsMe")).toBe("Them");
    expect(speakerLabel(7, "clusterTrackA")).toBe("Them");
    // Not vacuous: the two mic labels are different strings.
    expect(UNCLUSTERED_SPEAKER_LABEL).not.toBe("Me");
  });
});

/**
 * YV125 — the same truth table `meeting_kind_branch.rs` asserts in Rust, so the
 * screen and the exported file cannot disagree about WHICH BRANCH a meeting is
 * on, having already been held to agreeing about the labels on it.
 */
describe("diarizationTarget", () => {
  it("mirrors the Rust branch, row for row", () => {
    const table: [MeetingKind | string, boolean, string][] = [
      ["in_person", false, "clusterTrackA"],
      ["in_person", true, "clusterTrackA"],
      ["unknown", false, "clusterTrackA"],
      ["unknown", true, "clusterTrackA"],
      ["virtual", true, "micIsMe"],
      ["virtual", false, "clusterTrackA"],
    ];
    for (const [kind, hasSystemTrack, want] of table) {
      expect(diarizationTarget(kind, hasSystemTrack)).toBe(want);
    }
  });

  it("treats anything it does not recognise as unknown, which clusters", () => {
    for (const junk of ["hybrid", "", "VIRTUAL", "in-person", null, undefined]) {
      expect(diarizationTarget(junk, true)).toBe("clusterTrackA");
      expect(diarizationTarget(junk, false)).toBe("clusterTrackA");
    }
  });

  it("follows the segments, not a flag: a silent tap is a mic-only meeting", () => {
    const call = [
      seg("mic", 0, MIC_TRACK, "can we ship on friday"),
      seg("them", 4, SYSTEM_TRACK, "friday works for us"),
    ];
    expect(orderedTranscript(call, "virtual").map((l) => l.speaker)).toEqual([
      "Me",
      "Them",
    ]);
    const silentTap = [
      seg("mic", 0, MIC_TRACK, "can we ship on friday"),
      seg("them", 4, SYSTEM_TRACK, "   "),
    ];
    expect(orderedTranscript(silentTap, "virtual").map((l) => l.speaker)).toEqual([
      UNCLUSTERED_SPEAKER_LABEL,
    ]);
    // The hybrid case: the tap delivered, and the room is still clustered.
    expect(orderedTranscript(call, "in_person").map((l) => l.speaker)).toEqual([
      UNCLUSTERED_SPEAKER_LABEL,
      "Them",
    ]);
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
    expect(
      orderedTranscript(blankTap, CALL).every((l) => l.track === MIC_TRACK),
    ).toBe(true);
    // One real word from the far side and it IS a two-track meeting again.
    expect(
      isTwoTrack([...blankTap, seg("tap2", 2, SYSTEM_TRACK, "hello")]),
    ).toBe(true);
  });
});

/**
 * YV127 — the caveat's truth table.
 *
 * `overlap_column_absent_and_documented.rs` proves the column is absent and
 * documented; these cases prove the absence is SAID, and said in the right
 * places. Both halves matter: a caveat that never renders is a comment, and a
 * caveat that renders everywhere is a disclaimer.
 */
describe("showsOverlapCaveat", () => {
  const ROOM = [
    seg("a", 0, MIC_TRACK, "let us start with the release checklist"),
    seg("b", 4, MIC_TRACK, "friday works if the signing cert lands"),
  ];

  it("follows the diarization target, row for row", () => {
    const rows: [string | null | undefined, TranscriptSegment[], boolean, string][] = [
      ["in_person", ROOM, true, "the room is on the microphone and gets clustered"],
      ["unknown", ROOM, true, "the picker was skipped — the clustering branch"],
      [undefined, ROOM, true, "a row from before migration 4 is `unknown`"],
      ["nonsense", ROOM, true, "an unreadable kind resolves to the clustering branch"],
      [
        "virtual",
        ROOM,
        true,
        "a call the tap never attached to at all: no second-track rows exist",
      ],
      [
        // THE discriminating case, and the one `blank-tap.png` photographs:
        // the tap DID attach and DID produce rows — the ASR just found no
        // words in them. The kind says `virtual`, so a gate written against
        // the kind would withhold the caveat here; `isTwoTrack` looks at the
        // rows' text instead, resolves to `clusterTrackA`, and the caveat is
        // owed. Whitespace shapes copied from `transcriptPreview.tsx`'s
        // `BLANK_TAP` so the picture and this row are the same input.
        "virtual",
        [
          ...ROOM,
          seg("blank1", 5, SYSTEM_TRACK, "   "),
          seg("blank2", 7, SYSTEM_TRACK, "\n\t "),
        ],
        true,
        "a call whose tap delivered only blank spans still clusters Track A",
      ],
      [
        "virtual",
        [...ROOM, seg("c", 6, SYSTEM_TRACK, "sounds right to me")],
        false,
        "THE exception: a call with a live second track never clusters Track A",
      ],
      [
        "in_person",
        [...ROOM, seg("c", 6, SYSTEM_TRACK, "sounds right to me")],
        true,
        "a hybrid room still clusters the microphone, so the caveat still applies",
      ],
    ];
    for (const [kind, segments, expected, why] of rows) {
      expect(showsOverlapCaveat(segments, kind), `${kind}: ${why}`).toBe(expected);
      // The one rule this can never break: it agrees with the branch the
      // speaker labels took, so the sentence and the labels describe the same
      // mechanism.
      expect(showsOverlapCaveat(segments, kind)).toBe(
        diarizationTarget(kind, isTwoTrack(segments)) === "clusterTrackA",
      );
    }
  });

  it("needs a microphone line on the screen to qualify", () => {
    expect(showsOverlapCaveat([], "in_person")).toBe(false);
    // Rows exist, words do not: the same blank spans `orderedTranscript` drops.
    expect(
      showsOverlapCaveat([seg("mic", 0, MIC_TRACK, "  \n ")], "in_person"),
    ).toBe(false);
    // A tap-only transcript under a room kind: clustering is the branch, but
    // there is not one microphone line for the sentence to qualify.
    const tapOnly = [seg("t", 0, SYSTEM_TRACK, "somebody dialled in")];
    expect(diarizationTarget("in_person", isTwoTrack(tapOnly))).toBe("clusterTrackA");
    expect(showsOverlapCaveat(tapOnly, "in_person")).toBe(false);
    // …and one real microphone word brings it back.
    expect(
      showsOverlapCaveat([...tapOnly, seg("m", 1, MIC_TRACK, "we did")], "in_person"),
    ).toBe(true);
  });

  /**
   * The sentence states a LIMIT and claims no ability, which is what makes it
   * honest on a build where clustering has not shipped. Pinned as a string
   * rather than left to review: `overlap_column_absent_and_documented.rs`
   * compares this exact text against `meetings::OVERLAP_CAVEAT` in Rust.
   */
  it("says what it says", () => {
    expect(OVERLAP_CAVEAT).toBe(
      "Speech during overlapping talk is attributed to only one speaker.",
    );
  });
});
