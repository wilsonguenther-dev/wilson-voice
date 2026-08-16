import {
  canSplit,
  mergeTargets,
  reassignMenu,
  type CorrectableSegment,
  type SpeakerChoice,
} from "./corrections";

/**
 * YV130 — the two corrections finding #25 says real diarization actually needs,
 * offered where the mistake is visible: on the turn itself.
 *
 * **Reassign** (this turn is the wrong person) is a list of enrolled voices plus
 * an explicit "Unknown speaker". **Merge** (one person, two clusters) sits next
 * to it because that is the other half of the same mistake, and putting it in a
 * settings screen would mean noticing the error in the transcript and then
 * leaving the transcript to fix it. **Split** appears only when the cluster has
 * embeddings to split BY — a control that errors when pressed teaches the user
 * the feature is broken, when the truth is that this meeting predates the
 * vectors being retained.
 *
 * Every one of these is a database write and nothing else: no spinner, no
 * "re-analysing", no progress. The absence of a loading state here is a design
 * decision that follows from the mechanism, not an omission.
 *
 * A locked segment still shows the menu — a person may change their mind about
 * their own answer. What a lock stops is somebody ELSE'S batch, which is
 * enforced in `speaker_corrections.rs` and not here; a UI-side lock would be a
 * second, weaker statement of a rule the database already holds.
 */
export default function SegmentCorrectionMenu({
  segment,
  speakers,
  clusters,
  clusterNames,
  membersWithEmbedding,
  onReassign,
  onMerge,
  onSplit,
}: {
  segment: CorrectableSegment;
  speakers: readonly SpeakerChoice[];
  /** Every cluster index present in this meeting. */
  clusters: readonly number[];
  /** Names for the clusters that have one. */
  clusterNames?: Readonly<Record<number, string>>;
  /** How many members of this segment's cluster carry a retained embedding. */
  membersWithEmbedding: number;
  onReassign: (speakerId: string | null) => void;
  onMerge: (intoCluster: number) => void;
  onSplit: () => void;
}) {
  const items = reassignMenu(segment, speakers);
  const merges = mergeTargets(segment, clusters, clusterNames);

  return (
    <div className="seg-correct" role="group" aria-label="Correct this speaker">
      <p className="seg-correct-heading">Who is speaking?</p>
      <ul className="seg-correct-list">
        {items.map((item) => (
          <li key={item.speakerId ?? "__none__"}>
            <button
              type="button"
              aria-pressed={item.current}
              className={item.current ? "seg-correct-item current" : "seg-correct-item"}
              onClick={() => onReassign(item.speakerId)}
            >
              {item.label}
            </button>
          </li>
        ))}
      </ul>

      {merges.length > 0 && (
        <>
          <p className="seg-correct-heading">Same person as…</p>
          <ul className="seg-correct-list">
            {merges.map((m) => (
              <li key={m.clusterIndex}>
                <button
                  type="button"
                  className="seg-correct-item"
                  onClick={() => onMerge(m.clusterIndex)}
                >
                  {m.label}
                </button>
              </li>
            ))}
          </ul>
        </>
      )}

      {canSplit(membersWithEmbedding) && (
        <button type="button" className="seg-correct-split" onClick={onSplit}>
          Split this speaker in two
        </button>
      )}
    </div>
  );
}
