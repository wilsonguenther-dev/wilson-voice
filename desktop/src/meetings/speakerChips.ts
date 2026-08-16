/**
 * YV129 — the "who is this?" chip row's rules, mirrored from Rust.
 *
 * `speaker_profiles::who_is_this_chips` owns these rules; the Meetings UI is
 * React and cannot call it, so it runs this hand-written mirror. **A mirror
 * agrees only where something checks that it does**, which is why every rule
 * below has the SAME fixture asserted on both sides — the discipline
 * `meetings::render_transcript` writes down at its own header, after the first
 * review of YV108 found the two surfaces disagreeing about blank spans.
 *
 * The rules, in the order they apply:
 *
 *  1. A meeting that has not ended produces NO row. The chip row is a
 *     meeting-detail affordance; asking mid-recording is the modal interruption
 *     the plan's F2 flow rules out. Rust returns `MeetingStillLive`; here the
 *     absence of a row is the same refusal, and `chipRow` throws rather than
 *     returning an empty row so a caller cannot mistake "refused" for "nobody
 *     to ask about".
 *  2. One chip per CLUSTER. Never per segment, never per turn. A six-person
 *     class costs a handful of questions, not dozens.
 *  3. An auto-confirmed (`known`) cluster asks nothing — that is what the
 *     auto-confirm band is for.
 *  4. A cluster the user has already answered is never re-offered.
 *  5. Clusters under the floor roll into one "Other" count rather than becoming
 *     chips (YV126's ranking + floor, which replaced a hard reject that
 *     misfired on exactly the six-person room this backlog prioritises).
 *  6. What remains sorts by speech time, descending, tie-broken by cluster
 *     index: the voice that talked most is worth naming first.
 *
 * No threshold lives in this file, for the same reason none lives in the Rust
 * module: the bands and the floor are measured, not chosen, and neither has
 * been measured yet. The floor is a parameter here exactly as it is there.
 */

/** Cosine similarity, `[-1, 1]`, larger is more similar. Mirrors YV120's newtype. */
export type CosineSimilarity = number;

/** What the matcher concluded about one cluster — `speaker_profiles::MatchResult`. */
export type MatchResult =
  | { kind: "known"; profileId: string; score: CosineSimilarity; conditionKey: string }
  | { kind: "suggested"; profileId: string; score: CosineSimilarity; conditionKey: string }
  | { kind: "new" };

export interface SpeakerProfileView {
  id: string;
  displayName: string;
  isMe: boolean;
}

export interface ClusterSummaryView {
  clusterIndex: number;
  /** What the transcript calls this voice — echoed verbatim onto the chip. */
  label: string;
  speechSeconds: number;
  turns: number;
}

export interface ClusterDecisionView {
  cluster: ClusterSummaryView;
  result: MatchResult;
}

/** YV126's ranking floor. A parameter, never a constant — see the header. */
export interface ChipFloor {
  minSpeechSeconds: number;
  minTurns: number;
}

export interface ChipCandidate {
  profileId: string;
  displayName: string;
}

export interface ChipSuggestion extends ChipCandidate {
  score: CosineSimilarity;
}

export interface SpeakerChip {
  clusterIndex: number;
  clusterLabel: string;
  suggested: ChipSuggestion | null;
  alternatives: ChipCandidate[];
  allowNew: boolean;
  speechSeconds: number;
}

export interface ChipRow {
  chips: SpeakerChip[];
  /** Clusters under the floor: counted, never listed. The count is the honest
   *  thing to show ("3 quieter voices"); the list would be the spam. */
  rolledIntoOther: number;
}

/** Rule 1's refusal, as a distinguishable failure rather than an empty row. */
export class MeetingStillLiveError extends Error {
  constructor() {
    super(
      "the who-is-this row is a completed-meeting affordance; this meeting has not ended",
    );
    this.name = "MeetingStillLiveError";
  }
}

export function admittedByFloor(
  cluster: ClusterSummaryView,
  floor: ChipFloor,
): boolean {
  return (
    cluster.speechSeconds >= floor.minSpeechSeconds && cluster.turns >= floor.minTurns
  );
}

/** A match that still needs a human. `known` is the only one that does not. */
export function needsAHuman(result: MatchResult): boolean {
  return result.kind !== "known";
}

/**
 * Build the row for a FINISHED meeting.
 *
 * @throws {MeetingStillLiveError} when `meetingEnded` is false.
 */
export function chipRow(
  meetingEnded: boolean,
  decisions: readonly ClusterDecisionView[],
  answered: readonly number[],
  roster: readonly SpeakerProfileView[],
  floor: ChipFloor,
): ChipRow {
  if (!meetingEnded) throw new MeetingStillLiveError();

  const chips: SpeakerChip[] = [];
  let rolledIntoOther = 0;

  for (const decision of decisions) {
    if (answered.includes(decision.cluster.clusterIndex)) continue;
    if (!needsAHuman(decision.result)) continue;
    if (!admittedByFloor(decision.cluster, floor)) {
      rolledIntoOther += 1;
      continue;
    }

    // Narrowed into a local first: the pre-selected name is looked up in the
    // roster, and a suggestion for a profile the roster no longer carries
    // degrades to a plain "who is this?" rather than a chip naming a ghost.
    const result = decision.result;
    let suggested: ChipSuggestion | null = null;
    if (result.kind === "suggested") {
      const match = roster.find((p) => p.id === result.profileId);
      if (match) {
        suggested = {
          profileId: match.id,
          displayName: match.displayName,
          score: result.score,
        };
      }
    }

    chips.push({
      clusterIndex: decision.cluster.clusterIndex,
      clusterLabel: decision.cluster.label,
      suggested,
      alternatives: roster
        .filter((p) => p.id !== suggested?.profileId)
        .map((p) => ({ profileId: p.id, displayName: p.displayName })),
      allowNew: true,
      speechSeconds: decision.cluster.speechSeconds,
    });
  }

  chips.sort(
    (a, b) => b.speechSeconds - a.speechSeconds || a.clusterIndex - b.clusterIndex,
  );

  return { chips, rolledIntoOther };
}

/**
 * "3 min 30 s of speech" — why this voice is worth naming, in the units a
 * person reads. Whole seconds under a minute, never a bare float.
 */
export function speechLabel(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s}s of speech`;
  const m = Math.floor(s / 60);
  const rem = s % 60;
  return rem === 0 ? `${m}m of speech` : `${m}m ${rem}s of speech`;
}
