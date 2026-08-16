/**
 * YV130 — the correction affordances, as rules rather than as JSX.
 *
 * Everything a person can do to a wrong attribution is here as a pure function
 * of what is on screen, for the same reason `transcript.ts` exists next to
 * `TranscriptList.tsx`: "which merge targets does this cluster offer" and "what
 * does the retroactive-relabel prompt say when N is 1" are questions a test can
 * answer without a DOM, and they are exactly the questions that go wrong.
 *
 * The one rule the copy in this file exists to hold: **the retroactive relabel
 * is an offer, never an act** (finding #31). There is no "relabelling…" state
 * and no toast that reports something already done — there is a question with
 * two answers, and one of them is "Not now", which is what the absence of a call
 * looks like.
 */

/** A segment as the correction menu needs it. */
export type CorrectableSegment = {
  id: string;
  /** The enrolled voice it is attributed to, or `null` for unlabelled. */
  speakerId: string | null;
  /** A person decided this specific segment. */
  speakerLocked: boolean;
  /** Which offline cluster it landed in, or `null` if it was never clustered. */
  clusterIndex: number | null;
};

/** An enrolled voice, as the menu lists it. */
export type SpeakerChoice = { id: string; displayName: string };

/** One entry in the per-segment reassign menu. */
export type ReassignItem = {
  /** `null` is "not a person / unknown", which is a decision like any other. */
  speakerId: string | null;
  label: string;
  /** Already the segment's attribution — shown ticked, and not re-applied. */
  current: boolean;
};

/**
 * The per-segment reassign menu: every enrolled voice, plus the explicit
 * "unassign" that clearing a label needs.
 *
 * "Unknown speaker" sits at the BOTTOM and is never the default: a menu whose
 * first item removes an answer invites the misclick that loses one.
 */
export function reassignMenu(
  segment: CorrectableSegment,
  speakers: readonly SpeakerChoice[],
): ReassignItem[] {
  const items: ReassignItem[] = speakers.map((s) => ({
    speakerId: s.id,
    label: s.displayName,
    current: segment.speakerId === s.id,
  }));
  items.push({
    speakerId: null,
    label: "Unknown speaker",
    current: segment.speakerId === null,
  });
  return items;
}

/** One entry in the cluster-merge menu. */
export type MergeItem = { clusterIndex: number; label: string };

/**
 * The clusters this one can be merged INTO: every other cluster in the meeting,
 * in the order they first speak.
 *
 * A cluster is never offered itself — merging a thing into itself is a no-op
 * that reports success, which is how a broken affordance stays broken (the Rust
 * side refuses it outright; this stops it being reachable at all).
 */
export function mergeTargets(
  segment: CorrectableSegment,
  clusters: readonly number[],
  names: Readonly<Record<number, string>> = {},
): MergeItem[] {
  if (segment.clusterIndex === null) return [];
  return clusters
    .filter((c) => c !== segment.clusterIndex)
    .map((c) => ({ clusterIndex: c, label: names[c] ?? `Speaker ${c + 1}` }));
}

/**
 * Whether the "Split this speaker in two" affordance is offered at all.
 *
 * Offered only when the cluster has at least two members carrying a retained
 * embedding, because that is the only case `split_cluster` can answer — a
 * disabled-looking button that errors when pressed teaches the user that the
 * feature is broken, when the truth is that this particular meeting predates the
 * vectors being kept.
 */
export function canSplit(membersWithEmbedding: number): boolean {
  return membersWithEmbedding >= 2;
}

/** The retroactive-relabel prompt: a question, and its two answers. */
export type RelabelOfferCopy = {
  question: string;
  apply: string;
  dismiss: string;
};

/**
 * *"This voice appears in 3 earlier meetings — label them too?"*
 *
 * Singular and plural are separate strings rather than an `(s)`, because this
 * prompt is asking for trust and "1 earlier meetings" is the sort of thing that
 * withholds it. Returns `null` when there is nothing to offer: no prompt at all
 * beats a prompt whose Apply would change nothing, which still teaches the user
 * that Yap acts on its own.
 */
export function relabelOffer(
  displayName: string,
  meetings: number,
  segments: number,
): RelabelOfferCopy | null {
  if (meetings < 1 || segments < 1) return null;
  const where =
    meetings === 1 ? "1 earlier meeting" : `${meetings} earlier meetings`;
  const them = meetings === 1 ? "it" : "them";
  return {
    question: `${displayName}'s voice appears in ${where} — label ${them} too?`,
    apply: "Apply",
    dismiss: "Not now",
  };
}

/**
 * The line shown under a batch that HAS been applied, with its undo.
 *
 * `skippedLocked` is surfaced rather than swallowed: a batch that quietly did
 * less than it offered is a batch the user will not trust the second time.
 */
export function relabelResult(
  displayName: string,
  touched: number,
  skippedLocked: number,
): { message: string; undo: string } {
  const turns = touched === 1 ? "1 turn" : `${touched} turns`;
  const kept =
    skippedLocked === 0
      ? ""
      : skippedLocked === 1
        ? " 1 turn you had already named was left as it was."
        : ` ${skippedLocked} turns you had already named were left as they were.`;
  return {
    message: `Labelled ${turns} as ${displayName}.${kept}`,
    undo: "Undo",
  };
}
