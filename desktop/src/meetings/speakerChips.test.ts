/**
 * YV129 — the chip row's rules, held to the SAME fixtures as the Rust side.
 *
 * Each case below names the Rust test it mirrors. That pairing is the whole
 * value of the file: `speakerChips.ts` is a hand-written mirror of
 * `speaker_profiles::who_is_this_chips`, and a mirror agrees only where
 * something checks that it does. Adding a rule to the Rust module without
 * adding its fixture here reopens the seam the first review of YV108 found
 * (Rust dropped whitespace-only spans, the mirror did not, and a blank tap
 * segment drew a labelled empty row on a screen whose export had no such line).
 */
import { describe, expect, it } from "vitest";
import {
  chipRow,
  MeetingStillLiveError,
  speechLabel,
  type ChipFloor,
  type ClusterDecisionView,
  type MatchResult,
  type SpeakerProfileView,
} from "./speakerChips";

/** The backlog's sketched floor, supplied by the CALLER — never a constant. */
const FLOOR: ChipFloor = { minSpeechSeconds: 30, minTurns: 3 };

const ROSTER: SpeakerProfileView[] = [
  { id: "p_jeisil", displayName: "Jeisil", isMe: false },
  { id: "p_aidan", displayName: "Aidan", isMe: false },
];

function decision(
  clusterIndex: number,
  speechSeconds: number,
  turns: number,
  result: MatchResult = { kind: "new" },
): ClusterDecisionView {
  return {
    cluster: {
      clusterIndex,
      label: `Speaker ${clusterIndex + 1}`,
      speechSeconds,
      turns,
    },
    result,
  };
}

describe("chipRow", () => {
  // Mirrors `who_is_this_never_modal_never_live` (part 1).
  it("refuses a meeting that has not ended, and accepts the same input once it has", () => {
    const decisions = [decision(0, 120, 9)];
    expect(() => chipRow(false, decisions, [], ROSTER, FLOOR)).toThrow(
      MeetingStillLiveError,
    );
    expect(chipRow(true, decisions, [], ROSTER, FLOOR).chips).toHaveLength(1);
  });

  // Mirrors `a_six_speaker_classroom_asks_four_questions_not_six`.
  it("asks four questions about a six-speaker classroom, not six", () => {
    const row = chipRow(
      true,
      [
        decision(0, 210, 18),
        decision(1, 95, 11),
        decision(2, 61, 7),
        decision(3, 33, 4),
        decision(4, 12, 3), // under the 30 s floor
        decision(5, 44, 2), // over on seconds, under on turns
      ],
      [],
      ROSTER,
      FLOOR,
    );
    expect(row.chips.map((c) => c.clusterIndex)).toEqual([0, 1, 2, 3]);
    expect(row.rolledIntoOther).toBe(2);
    for (const chip of row.chips) {
      expect(chip.allowNew).toBe(true);
      expect(chip.suggested).toBeNull();
      expect(chip.alternatives.map((a) => a.displayName)).toEqual([
        "Jeisil",
        "Aidan",
      ]);
    }
  });

  // Mirrors `an_auto_confirmed_cluster_gets_no_chip`.
  it("asks nothing about an auto-confirmed cluster", () => {
    const row = chipRow(
      true,
      [
        decision(0, 200, 12, {
          kind: "known",
          profileId: "p_jeisil",
          score: 1.0,
          conditionKey: "laptop_mic_near",
        }),
        decision(1, 150, 10),
      ],
      [],
      ROSTER,
      FLOOR,
    );
    expect(row.chips.map((c) => c.clusterIndex)).toEqual([1]);
    expect(row.rolledIntoOther).toBe(0);
  });

  // Mirrors `a_cluster_the_user_has_already_answered_is_never_re_offered`.
  it("never re-offers a cluster the user already answered", () => {
    const decisions = [decision(0, 200, 12), decision(1, 150, 10)];
    expect(
      chipRow(true, decisions, [0], ROSTER, FLOOR).chips.map((c) => c.clusterIndex),
    ).toEqual([1]);
    expect(chipRow(true, decisions, [], ROSTER, FLOOR).chips).toHaveLength(2);
  });

  // Mirrors `a_suggested_cluster_arrives_pre_selected_with_the_rest_of_the_roster_beside_it`.
  it("pre-selects a suggestion and does not repeat it among the alternatives", () => {
    const row = chipRow(
      true,
      [
        decision(3, 88, 9, {
          kind: "suggested",
          profileId: "p_jeisil",
          score: 0.7071,
          conditionKey: "laptop_mic_near",
        }),
      ],
      [],
      ROSTER,
      FLOOR,
    );
    expect(row.chips).toHaveLength(1);
    expect(row.chips[0].suggested).toEqual({
      profileId: "p_jeisil",
      displayName: "Jeisil",
      score: 0.7071,
    });
    expect(row.chips[0].alternatives.map((a) => a.displayName)).toEqual(["Aidan"]);
    expect(row.chips[0].clusterLabel).toBe("Speaker 4");
  });

  it("degrades a suggestion for a profile the roster no longer carries to a plain question", () => {
    const row = chipRow(
      true,
      [
        decision(0, 90, 5, {
          kind: "suggested",
          profileId: "p_deleted",
          score: 0.66,
          conditionKey: "laptop_mic_near",
        }),
      ],
      [],
      ROSTER,
      FLOOR,
    );
    expect(row.chips[0].suggested).toBeNull();
    expect(row.chips[0].alternatives).toHaveLength(2);
  });

  it("ranks by speech time, not by cluster order", () => {
    const row = chipRow(
      true,
      [decision(0, 40, 5), decision(1, 300, 20), decision(2, 120, 8)],
      [],
      ROSTER,
      FLOOR,
    );
    expect(row.chips.map((c) => c.clusterIndex)).toEqual([1, 2, 0]);
  });

  it("breaks a speech-time tie on the cluster index, so the order is stable", () => {
    const row = chipRow(
      true,
      [decision(5, 100, 6), decision(2, 100, 6), decision(9, 100, 6)],
      [],
      ROSTER,
      FLOOR,
    );
    expect(row.chips.map((c) => c.clusterIndex)).toEqual([2, 5, 9]);
  });
});

describe("speechLabel", () => {
  it("reads as a person would say it", () => {
    expect(speechLabel(0)).toBe("0s of speech");
    expect(speechLabel(33.4)).toBe("33s of speech");
    expect(speechLabel(60)).toBe("1m of speech");
    expect(speechLabel(210)).toBe("3m 30s of speech");
    expect(speechLabel(-5)).toBe("0s of speech");
  });
});
