/**
 * YV108 — the mixed Me/Them transcript, frontend side.
 *
 * This is the TypeScript mirror of `meetings::render_transcript` /
 * `meetings::speaker_label` / `meetings::diarization_target` / `meetings::one_line`
 * in Rust. The two exist
 * separately because the Meetings UI renders segment rows it already has in
 * memory (the detail command hands them over whole) while the Markdown export
 * renders the same segments in Rust — but they must agree about who spoke and
 * in what order, or a user's exported file contradicts the screen they exported
 * it from.
 *
 * A mirror is a copy, not a shared function, so nothing about it is guaranteed
 * by construction: it holds only for the rules `transcript.test.ts` pins
 * against the SAME fixtures the Rust tests use. The first review of this item
 * found the gap that proves the point — Rust dropped whitespace-only spans and
 * this file did not, so a blank tap segment painted a labelled empty "Them" row
 * on screen that the export had already thrown away. Every rule below now has a
 * fixture on both sides of the language boundary; anything added here without
 * one is a fresh chance to diverge.
 *
 * Deliberately pure and DOM-free: ordering, labelling and text normalisation
 * are the parts with rules, so they are unit-tested rather than eyeballed
 * inside a component.
 */

/** Track 0 — the microphone: whoever is holding the Mac. */
export const MIC_TRACK = 0;
/** Track 1 — the process tap: everything that came out of the speakers. */
export const SYSTEM_TRACK = 1;

/** Mirrors `meetings::MIC_SPEAKER_LABEL`. */
export const MIC_SPEAKER_LABEL = "Me";
/** Mirrors `meetings::SYSTEM_SPEAKER_LABEL`. */
export const SYSTEM_SPEAKER_LABEL = "Them";
/**
 * YV125 — mirrors `meetings::UNCLUSTERED_SPEAKER_LABEL`: what the microphone is
 * called when it is the track that has to be CLUSTERED and clustering has not
 * run yet. "Me" is a claim about identity, and merged finding #4 is that
 * deriving it from a channel number holds for a call with a live second track
 * and is false for a room, a class, a hybrid meeting, or a call whose tap never
 * attached.
 */
export const UNCLUSTERED_SPEAKER_LABEL = "Speaker";

/**
 * YV127 — mirrors `meetings::OVERLAP_CAVEAT`, byte for byte (the Rust test
 * `overlap_column_absent_and_documented` compares the two strings).
 *
 * There is no `overlapped` column in the schema and there is no overlap flag on
 * the diarization wire: sherpa deletes overlapped frames before embedding, so a
 * stretch where two people talked at once arrives attributed to one of them,
 * with nothing recording that it happened. That is a real limit of the
 * mechanism, and the choice this item makes is to say so on the screen the
 * limit applies to rather than to leave it in a design document.
 *
 * It states a limit and claims no ability — which is why it is honest on a
 * build where clustering (YV126) has not landed and every microphone line still
 * reads [`UNCLUSTERED_SPEAKER_LABEL`].
 */
export const OVERLAP_CAVEAT =
  "Speech during overlapping talk is attributed to only one speaker.";

/** Mirrors `meetings::MeetingKind` — the `meetings.kind` column's values. */
export type MeetingKind = "virtual" | "in_person" | "unknown";

/** Mirrors `meetings::DiarizationTarget`. */
export type DiarizationTarget = "clusterTrackA" | "micIsMe";

/**
 * Mirrors `meetings::diarization_target`, including its lenient parse: any
 * value this build does not recognise — a kind written by a newer build, an
 * empty string — is `unknown`, which is the branch that CLUSTERS rather than
 * the one that asserts a single speaker.
 *
 * | kind        | live second track | target |
 * |-------------|-------------------|--------|
 * | `in_person` | either            | clusterTrackA |
 * | `unknown`   | either            | clusterTrackA |
 * | `virtual`   | yes               | micIsMe |
 * | `virtual`   | no                | clusterTrackA |
 */
export function diarizationTarget(
  kind: string | null | undefined,
  hasSystemTrack: boolean,
): DiarizationTarget {
  return kind === "virtual" && hasSystemTrack ? "micIsMe" : "clusterTrackA";
}

/** One chronological transcript segment, as the detail command sends it. */
export interface TranscriptSegment {
  id: string;
  startSeconds: number;
  text: string;
  /**
   * YV106 — which recorded track this came from. Optional on the wire: a build
   * of this UI can be pointed at a row written before migration 3, and a
   * missing track is the mic, exactly as the column's `DEFAULT 0` says.
   */
  track?: number | null;
}

/** One rendered line. Mirrors `meetings::TranscriptLine`. */
export interface TranscriptLine<S extends TranscriptSegment = TranscriptSegment> {
  segment: S;
  track: number;
  speaker: string;
  startSeconds: number;
  /** `hh:mm:ss` from the start of the meeting. Mirrors `TranscriptLine.offset`. */
  offset: string;
  /**
   * The text to DRAW — whitespace-collapsed to exactly one line by
   * [`oneLine`], mirroring `TranscriptLine.text`. Surfaces must render this and
   * not `segment.text`, or the screen shows raw ASR spacing the export does
   * not.
   */
  text: string;
}

/**
 * Unicode `White_Space=yes`, written out rather than spelled `\s`.
 *
 * This is the exact set Rust's `char::is_whitespace` (and therefore
 * `str::split_whitespace`, and therefore `meetings::one_line`) uses. JavaScript's
 * `\s` is a DIFFERENT set: it misses U+0085 NEL and adds U+FEFF, so `\s` would
 * make this mirror disagree with Rust on both of those characters — the second
 * one silently, by dropping a span Rust keeps. `transcript.test.ts` and
 * `meeting_transcript_render_two_track.rs` both pin those two characters.
 */
const WHITESPACE =
  /[\t\n\v\f\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+/;

/**
 * Collapse a span to a single line. Mirrors `meetings::one_line`
 * (`s.split_whitespace().collect::<Vec<_>>().join(" ")`): leading and trailing
 * whitespace go, interior runs become one space, and a span that was nothing
 * but whitespace becomes the empty string — which is what
 * [`orderedTranscript`] drops on.
 */
export function oneLine(text: string): string {
  return text.split(WHITESPACE).filter((part) => part.length > 0).join(" ");
}

/** `3725.4` → `01:02:05`. Mirrors `meetings::format_offset` in Rust. */
export function formatOffset(seconds: number): string {
  const total = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return [h, m, s].map((n) => String(n).padStart(2, "0")).join(":");
}

/** The track a segment belongs to, with the pre-migration-3 default applied. */
export function trackOf(segment: TranscriptSegment): number {
  return segment.track ?? MIC_TRACK;
}

/**
 * The speaker label for a track under this meeting's diarization target
 * (YV125). Anything that is not the mic did not come from this user's
 * microphone, so it is "Them" under BOTH targets — same catch-all as the Rust
 * side, for the same reason (`track` is an unconstrained INTEGER column, and a
 * transcript that renders beats one that refuses).
 *
 * Mirrors `meetings::speaker_label`.
 */
export function speakerLabel(track: number, target: DiarizationTarget): string {
  if (track !== MIC_TRACK) return SYSTEM_SPEAKER_LABEL;
  return target === "micIsMe" ? MIC_SPEAKER_LABEL : UNCLUSTERED_SPEAKER_LABEL;
}

/**
 * Does this meeting have a second track? A mic-only (22-A) meeting must render
 * exactly as it always did — no phantom "Them", no layout implying a speaker
 * who was never recorded.
 *
 * Asked of the segments that RENDER, not of the raw rows: a tap track whose
 * only spans collapse to nothing draws no lines at all (see
 * [`orderedTranscript`]), so widening the speaker gutter for it would reserve
 * room for a speaker who is not on the screen and is not in the exported file
 * either. Mirrors `meetings::is_two_track`, which drops the same spans.
 */
export function isTwoTrack(segments: readonly TranscriptSegment[]): boolean {
  return segments.some((s) => trackOf(s) !== MIC_TRACK && oneLine(s.text) !== "");
}

/**
 * The single interleaved transcript: one time-ordered list of lines, NOT two
 * columns. Both tracks' `startSeconds` are already on one common host-time
 * origin by the time they are stored, so comparing them across tracks is
 * meaningful.
 *
 * Stable sort, mic first on an exact tie, non-finite offsets sorted as 0 — the
 * same total order Rust's `render_transcript` uses. On the already-ascending
 * single-track input the backend returns this is the identity, which is what
 * makes "a mic-only meeting renders unchanged" true rather than hoped for.
 *
 * Spans that collapse to nothing are DROPPED, exactly as `render_transcript`
 * drops them: a labelled row with a timestamp and no words is noise, and a
 * surface that kept it would put a speaker on the screen that the exported file
 * does not contain. The line carries the collapsed [`text`] so surfaces render
 * that rather than the raw segment.
 */
export function orderedTranscript<S extends TranscriptSegment>(
  segments: readonly S[],
  kind: string | null | undefined,
): TranscriptLine<S>[] {
  // YV125 — whether this meeting HAS a live second track is read off the
  // segments that actually render, not taken as a separate argument, exactly as
  // `render_transcript` reads it: a tap that delivered nothing is a mic-only
  // meeting to the reader, so it must be one to the labeller too.
  const target = diarizationTarget(kind, isTwoTrack(segments));
  const lines: (TranscriptLine<S> & { index: number })[] = [];
  segments.forEach((segment, index) => {
    const text = oneLine(segment.text);
    if (text === "") return;
    const track = trackOf(segment);
    lines.push({
      segment,
      track,
      speaker: speakerLabel(track, target),
      offset: formatOffset(segment.startSeconds),
      startSeconds: segment.startSeconds,
      text,
      index,
    });
  });
  const key = (v: number) => (Number.isFinite(v) ? v : 0);
  lines.sort(
    (a, b) =>
      key(a.startSeconds) - key(b.startSeconds) ||
      a.track - b.track ||
      a.index - b.index,
  );
  return lines.map(({ segment, track, speaker, offset, startSeconds, text }) => ({
    segment,
    track,
    speaker,
    offset,
    startSeconds,
    text,
  }));
}

/**
 * YV127 — does [`OVERLAP_CAVEAT`] apply to this meeting's transcript?
 *
 * Two conditions, both about what actually happened rather than about what was
 * configured:
 *
 *   1. the microphone track is the one being CLUSTERED
 *      (`diarizationTarget(...) === "clusterTrackA"`). A `virtual` meeting whose
 *      second track really did record the other participants takes the
 *      `micIsMe` branch, its microphone is never split, and a caveat about
 *      splitting it would be describing work the app did not do. That is the
 *      "does not render for a virtual+tap meeting" half of the acceptance, and
 *      it is the same branch the speaker labels take, so the sentence and the
 *      labels can never disagree about which mechanism ran.
 *   2. at least one microphone line survived to the screen. The caveat
 *      qualifies those lines; under a transcript that has none of them (a room
 *      recording whose microphone produced nothing, an in-person meeting with
 *      only tap rows) it would qualify nothing. Asked of the RENDERED lines,
 *      like [`isTwoTrack`], so a microphone row whose text collapses to nothing
 *      cannot summon a caveat for a line the reader cannot see.
 *
 * Deliberately NOT a condition: whether clustering has actually shipped. The
 * sentence is a limit on attribution, not a claim of it, so it is true of a
 * `clusterTrackA` transcript on this base — where every microphone line reads
 * [`UNCLUSTERED_SPEAKER_LABEL`] — and stays exactly as true once YV126 splits
 * those lines into speakers.
 */
export function showsOverlapCaveat(
  segments: readonly TranscriptSegment[],
  kind: string | null | undefined,
): boolean {
  if (diarizationTarget(kind, isTwoTrack(segments)) !== "clusterTrackA") return false;
  return segments.some((s) => trackOf(s) === MIC_TRACK && oneLine(s.text) !== "");
}
