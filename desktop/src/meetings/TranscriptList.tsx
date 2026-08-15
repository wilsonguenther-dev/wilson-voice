/**
 * YV108 — the meeting transcript, as the Meetings detail draws it.
 *
 * ONE interleaved list, never two columns: both tracks are already on a single
 * time base by the time they are stored, and two parallel dumps would hand the
 * reader back exactly the work the merge did. A mic-only meeting takes the same
 * path and comes out as one speaker — no phantom second one, and no widened
 * gutter reserving room for one.
 *
 * YV125 — the speaker column depends on the MEETING as well as its segments,
 * so the meeting's `kind` is a prop. "Me" is a claim about identity, and it is
 * only true where the mechanism makes it true (a call whose other participants
 * are on their own track); everywhere else the microphone is the track being
 * clustered and its lines say only that somebody spoke.
 *
 * It is a component rather than JSX inlined in `App.tsx` so the dev-tooling
 * preview (`dev/meeting-transcript-preview.html`) renders the REAL thing: a
 * two-track meeting needs a second recorded track to look at, which means a
 * screenshot of it would otherwise mean holding a live call.
 */
import {
  isTwoTrack,
  orderedTranscript,
  MIC_TRACK,
  type TranscriptSegment,
} from "./transcript";

export default function TranscriptList<S extends TranscriptSegment>({
  segments,
  kind,
}: {
  segments: readonly S[];
  /**
   * The meeting's `meetings.kind` (YV125). Optional so a caller rendering a row
   * from before migration 4 does not have to invent one — a missing kind is
   * `unknown`, which is the branch that does NOT claim the microphone is you.
   */
  kind?: string | null;
}) {
  return (
    // Both the class and the rows come from the same rendering rules the export
    // uses: a track that contributes no lines is not a second speaker, so it
    // does not widen the gutter either.
    <ol className={isTwoTrack(segments) ? "transcript two-track" : "transcript"}>
      {orderedTranscript(segments, kind).map(({ segment, track, speaker, offset, text }) => (
        <li key={segment.id}>
          {/* Timestamp and speaker are DATA, so they wear the pixel voice the
              rest of the app gives numbers. */}
          <span className="seg-time">{offset}</span>
          <span className={track === MIC_TRACK ? "seg-who" : "seg-who them"}>
            {speaker}
          </span>
          {/* The line's collapsed text, never `segment.text` — the export
              renders the collapsed form and the screen must not differ. */}
          <span className="seg-text">{text}</span>
        </li>
      ))}
    </ol>
  );
}
