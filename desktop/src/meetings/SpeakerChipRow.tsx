/**
 * YV129 — the inline "who is this?" row, as the Meetings detail draws it.
 *
 * `Speaker 2 → [Jeisil ▾] [Aidan] [+ new]`, inline, above the transcript,
 * exactly as the plan's F2 flow describes it. Three things it is NOT, each one
 * deliberate:
 *
 *  * **Not a modal.** It is a row of chips in the page, and dismissing it is
 *    reading past it. A dialog over a finished transcript would demand an
 *    answer to a question the user may not care about.
 *  * **Not live.** It renders from a finished meeting's clusters.
 *    `chipRow` throws for a meeting that has not ended, and
 *    `who_is_this_never_modal_never_live.rs` proves the live capture module
 *    cannot even name the Rust half.
 *  * **Not one chip per voice change.** One per CLUSTER, after YV126's floor,
 *    with the quiet remainder as a single "Other" line.
 *
 * A component rather than JSX inlined in `App.tsx` so
 * `dev/speaker-chips-preview.html` renders the REAL thing: the interesting
 * states need a six-person far-field recording and an enrolled roster, which is
 * not something you can produce on demand at a desk.
 *
 * **It has no call site in `App.tsx` yet, and that is honest rather than
 * unfinished.** Its inputs are `meeting_segments.cluster_index` (YV126, PR
 * #141) and the `speaker_profiles` table (YV128, PR #143) — both open, neither
 * on `main` at the time this landed. Mounting it against a payload that can
 * only ever be empty would be a wired-up UI that is wired to nothing. The
 * assembly point is `speaker_profiles::who_is_this_chips`; when those two land,
 * `MeetingDetail` carries the row and this mounts in one line.
 */
import {
  speechLabel,
  type ChipCandidate,
  type ChipRow,
  type SpeakerChip,
} from "./speakerChips";

function CandidateButton({
  candidate,
  onPick,
}: {
  candidate: ChipCandidate;
  onPick?: (profileId: string) => void;
}) {
  return (
    <button
      type="button"
      className="chip-candidate"
      onClick={() => onPick?.(candidate.profileId)}
    >
      {candidate.displayName}
    </button>
  );
}

export default function SpeakerChipRow({
  row,
  onPick,
  onNew,
}: {
  row: ChipRow;
  /** The user chose an enrolled person for this cluster. */
  onPick?: (clusterIndex: number, profileId: string) => void;
  /** The user chose `+ new` — YV130 opens enrollment from here. */
  onNew?: (clusterIndex: number) => void;
}) {
  // Nothing to ask and nothing rolled away: draw nothing at all. An empty
  // panel headed "Who is this?" over a transcript with every voice already
  // named is a question about nobody.
  if (row.chips.length === 0 && row.rolledIntoOther === 0) return null;

  return (
    <div className="panel speaker-chips">
      <h3>Who is this?</h3>
      {row.chips.length > 0 && (
        <p className="card-meta">
          {/* Says what the row costs before it is read: the batching promise,
              on screen. */}
          {row.chips.length === 1
            ? "One voice this recording could not name."
            : `${row.chips.length} voices this recording could not name.`}
        </p>
      )}
      <ul className="chip-rows">
        {row.chips.map((chip: SpeakerChip) => (
          <li key={chip.clusterIndex}>
            <span className="chip-cluster">{chip.clusterLabel}</span>
            <span className="chip-speech">{speechLabel(chip.speechSeconds)}</span>
            <span className="chip-choices">
              {chip.suggested && (
                <button
                  type="button"
                  className="chip-candidate suggested"
                  onClick={() => onPick?.(chip.clusterIndex, chip.suggested!.profileId)}
                >
                  {chip.suggested.displayName}
                </button>
              )}
              {chip.alternatives.map((candidate) => (
                <CandidateButton
                  key={candidate.profileId}
                  candidate={candidate}
                  onPick={(profileId) => onPick?.(chip.clusterIndex, profileId)}
                />
              ))}
              {chip.allowNew && (
                <button
                  type="button"
                  className="chip-candidate new"
                  onClick={() => onNew?.(chip.clusterIndex)}
                >
                  + new
                </button>
              )}
            </span>
          </li>
        ))}
      </ul>
      {row.rolledIntoOther > 0 && (
        // The quiet remainder, counted. Never listed — listing it is the spam.
        <p className="card-meta chip-other">
          {row.rolledIntoOther === 1
            ? "1 quieter voice was grouped as Other."
            : `${row.rolledIntoOther} quieter voices were grouped as Other.`}
        </p>
      )}
    </div>
  );
}
