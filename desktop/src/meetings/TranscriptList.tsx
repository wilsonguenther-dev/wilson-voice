/**
 * YV108 — the meeting transcript, as the Meetings detail draws it.
 *
 * ONE interleaved list, never two columns: both tracks are already on a single
 * time base by the time they are stored, and two parallel dumps would hand the
 * reader back exactly the work the merge did. A mic-only (22-A) meeting takes
 * the same path and comes out as the all-"Me" list it has always been — no
 * phantom speaker, and no widened gutter reserving room for one.
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
}: {
  segments: readonly S[];
}) {
  return (
    // Both the class and the rows come from the same rendering rules the export
    // uses: a track that contributes no lines is not a second speaker, so it
    // does not widen the gutter either.
    <ol className={isTwoTrack(segments) ? "transcript two-track" : "transcript"}>
      {orderedTranscript(segments).map(({ segment, track, speaker, offset, text }) => (
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
