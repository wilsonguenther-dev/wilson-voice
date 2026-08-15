/**
 * YV108 — the mixed Me/Them transcript, frontend side.
 *
 * This is the TypeScript mirror of `meetings::render_transcript` /
 * `meetings::speaker_label` in Rust. The two exist separately because the
 * Meetings UI renders segment rows it already has in memory (the detail command
 * hands them over whole) while the Markdown export renders the same segments in
 * Rust — but they must agree about who spoke and in what order, or a user's
 * exported file contradicts the screen they exported it from.
 * `transcript.test.ts` pins the two against the same fixtures the Rust tests
 * use, which is the only thing that keeps a mirror honest.
 *
 * Deliberately pure and DOM-free: ordering and labelling are the part with
 * rules, so they are unit-tested rather than eyeballed inside a component.
 */

/** Track 0 — the microphone: whoever is holding the Mac. */
export const MIC_TRACK = 0;
/** Track 1 — the process tap: everything that came out of the speakers. */
export const SYSTEM_TRACK = 1;

/** Mirrors `meetings::MIC_SPEAKER_LABEL`. */
export const MIC_SPEAKER_LABEL = "Me";
/** Mirrors `meetings::SYSTEM_SPEAKER_LABEL`. */
export const SYSTEM_SPEAKER_LABEL = "Them";

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
 * The speaker label for a track. Anything that is not the mic did not come from
 * this user's microphone, so it is "Them" — same catch-all as the Rust side,
 * for the same reason (`track` is an unconstrained INTEGER column, and a
 * transcript that renders beats one that refuses).
 */
export function speakerLabel(track: number): string {
  return track === MIC_TRACK ? MIC_SPEAKER_LABEL : SYSTEM_SPEAKER_LABEL;
}

/**
 * Does this meeting have a second track? A mic-only (22-A) meeting must render
 * exactly as it always did — no phantom "Them", no layout implying a speaker
 * who was never recorded.
 */
export function isTwoTrack(segments: readonly TranscriptSegment[]): boolean {
  return segments.some((s) => trackOf(s) !== MIC_TRACK);
}

/**
 * The single interleaved transcript: one time-ordered list of lines, NOT two
 * columns. Both tracks' `startSeconds` are already on one common host-time
 * origin by the time they are stored, so comparing them across tracks is
 * meaningful.
 *
 * Stable sort, mic first on an exact tie, non-finite offsets treated as 0 — the
 * same total order Rust's `render_transcript` uses. On the already-ascending
 * single-track input the backend returns this is the identity, which is what
 * makes "a mic-only meeting renders unchanged" true rather than hoped for.
 */
export function orderedTranscript<S extends TranscriptSegment>(
  segments: readonly S[],
): TranscriptLine<S>[] {
  const lines = segments.map((segment, index) => {
    const track = trackOf(segment);
    const raw = segment.startSeconds;
    return {
      segment,
      track,
      speaker: speakerLabel(track),
      startSeconds: Number.isFinite(raw) ? raw : 0,
      index,
    };
  });
  lines.sort(
    (a, b) =>
      a.startSeconds - b.startSeconds || a.track - b.track || a.index - b.index,
  );
  return lines.map(({ segment, track, speaker, startSeconds }) => ({
    segment,
    track,
    speaker,
    startSeconds,
  }));
}
