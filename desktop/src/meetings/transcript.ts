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
export const MIC_SPEAKER_LABEL = "You";
/** Mirrors `meetings::SYSTEM_SPEAKER_LABEL`. */
export const SYSTEM_SPEAKER_LABEL = "Call";
/**
 * YV125 — mirrors `meetings::UNCLUSTERED_SPEAKER_LABEL`: what the microphone is
 * called when it is the track that has to be CLUSTERED and clustering has not
 * run yet. "Me" is a claim about identity, and merged finding #4 is that
 * deriving it from a channel number holds for a call with a live second track
 * and is false for a room, a class, a hybrid meeting, or a call whose tap never
 * attached.
 */
export const UNCLUSTERED_SPEAKER_LABEL = "Speaker";

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
  /** yap-1106-diarize-wire — the row's speaker cluster (`meeting_segments.cluster_index`); null = unattributed. */
  clusterIndex?: number | null;
  /** End time; only used to weigh a cluster's talk time for its rank. */
  endSeconds?: number | null;
  /**
   * yap-1106-who-is-this — the name a saved voice gave this line
   * (`meeting_segments.speaker_name`, from "Who was this?"); wins over "Speaker N".
   */
  speakerName?: string | null;
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
 * yap-1106-diarize-wire — rank each (track, cluster) by talk time, most first
 * (ties: earliest start, then cluster id), 1-based per track. The mic of a
 * virtual call ("micIsMe") is never ranked: it is "You" by mechanism. Key is
 * `${track}:${cluster}`. Mirrors `meetings::cluster_ranks`.
 */
export function clusterRanks(
  segments: readonly TranscriptSegment[],
  target: DiarizationTarget,
): Map<string, number> {
  const talk = new Map<string, { track: number; cluster: number; seconds: number; first: number }>();
  for (const s of segments) {
    const cluster = s.clusterIndex;
    if (cluster === null || cluster === undefined || oneLine(s.text) === "") continue;
    const track = trackOf(s);
    if (track === MIC_TRACK && target === "micIsMe") continue;
    const key = `${track}:${cluster}`;
    const entry = talk.get(key) ?? { track, cluster, seconds: 0, first: Number.POSITIVE_INFINITY };
    const end = typeof s.endSeconds === "number" ? s.endSeconds : s.startSeconds;
    entry.seconds += Math.max(0, end - s.startSeconds);
    entry.first = Math.min(entry.first, s.startSeconds);
    talk.set(key, entry);
  }
  const ordered = [...talk.values()].sort(
    (a, b) => a.track - b.track || b.seconds - a.seconds || a.first - b.first || a.cluster - b.cluster,
  );
  const ranks = new Map<string, number>();
  let lastTrack: number | null = null;
  let rank = 0;
  for (const e of ordered) {
    if (e.track !== lastTrack) {
      lastTrack = e.track;
      rank = 0;
    }
    rank += 1;
    ranks.set(`${e.track}:${e.cluster}`, rank);
  }
  return ranks;
}

/**
 * The label a line shows: "Speaker 2" on a clustered mic track, "Call · Speaker 2"
 * on a clustered call track, else the plain track label. Mirrors `TranscriptLine::label`.
 */
export function clusteredSpeakerLabel(
  track: number,
  target: DiarizationTarget,
  rank: number | undefined,
): string {
  if (rank === undefined) return speakerLabel(track, target);
  const name = `${UNCLUSTERED_SPEAKER_LABEL} ${rank}`;
  return track === MIC_TRACK ? name : `${SYSTEM_SPEAKER_LABEL} · ${name}`;
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
  const ranks = clusterRanks(segments, target);
  const lines: (TranscriptLine<S> & { index: number })[] = [];
  segments.forEach((segment, index) => {
    const text = oneLine(segment.text);
    if (text === "") return;
    const track = trackOf(segment);
    lines.push({
      segment,
      track,
      speaker:
        segment.speakerName?.trim() ||
        clusteredSpeakerLabel(
          track,
          target,
          segment.clusterIndex === null || segment.clusterIndex === undefined
            ? undefined
            : ranks.get(`${track}:${segment.clusterIndex}`),
        ),
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
 * yap-1106-who-is-this — merge the names saved voices gave (`meeting_speaker_names`,
 * `[segment id, name]` pairs) into the segments, so a relabel shows at once.
 */
export function withSpeakerNames<S extends TranscriptSegment>(
  segments: readonly S[],
  names: readonly (readonly [string, string])[],
): S[] {
  if (names.length === 0) return segments.slice();
  const byId = new Map(names.map(([id, name]) => [id, name] as const));
  return segments.map((s) => (byId.has(s.id) ? { ...s, speakerName: byId.get(s.id) } : s));
}

/** The "Speaker N" a cluster reads as in this meeting (the card's fallback label). */
export function clusterLabel(
  segments: readonly TranscriptSegment[],
  kind: string | null | undefined,
  track: number,
  clusterIndex: number,
): string {
  const target = diarizationTarget(kind, isTwoTrack(segments));
  return clusteredSpeakerLabel(track, target, clusterRanks(segments, target).get(`${track}:${clusterIndex}`));
}

// ---------------------------------------------------------------------------
// yap-1106-auto-label-and-correct — speaker corrections, webview side.
//
// The decisions live in Rust (`speaker_profiles::apply_correction`, unit-tested
// there); the webview builds the request, then merges the rows the backend
// says changed. These helpers are pure so the chip menu is tested without a DOM.
// ---------------------------------------------------------------------------

/** How long "Undo" stays after a correction. Mirrors `UNDO_WINDOW_SECONDS`. */
export const UNDO_WINDOW_MS = 10_000;

/** One line's speaker state. Mirrors `speaker_profiles::LabelRow`. */
export interface LabelRow {
  segmentId: string;
  track: number;
  clusterIndex: number | null;
  profileId: number | null;
  speakerName: string | null;
  locked: number;
}

/** Mirrors `speaker_profiles::NameTarget`. */
export type NameTarget =
  | { kind: "profile"; profileId: number }
  | { kind: "newName"; name: string }
  | { kind: "me" };

/** Mirrors `speaker_profiles::SpeakerCorrection`. */
export type SpeakerCorrection =
  | { kind: "rename"; track: number; clusterIndex: number; target: NameTarget }
  | { kind: "someoneElse"; track: number; segmentIds: string[]; target: NameTarget }
  | { kind: "merge"; track: number; fromCluster: number; intoCluster: number }
  | { kind: "split"; track: number; clusterIndex: number; segmentIds: string[] };

/** Mirrors `speaker_profiles::CorrectionUndo` (handed back untouched on Undo). */
export interface CorrectionUndo {
  lock: number;
  prior: LabelRow[];
  positiveSampleIds: number[];
  negativeSampleIds: number[];
  expiresAtMs: number;
}

export interface CorrectionApplied {
  rows: LabelRow[];
  undo: CorrectionUndo;
}

export interface CorrectionUndone {
  rows: LabelRow[];
  skippedChanged: string[];
}

/**
 * Merge changed rows into the segments: cluster and name move together, and a
 * row whose name was cleared (a split, an undo) falls back to "Speaker N".
 */
export function applyLabelRows<S extends TranscriptSegment>(
  segments: readonly S[],
  rows: readonly LabelRow[],
): S[] {
  if (rows.length === 0) return segments.slice();
  const byId = new Map(rows.map((r) => [r.segmentId, r] as const));
  return segments.map((s) => {
    const r = byId.get(s.id);
    return r ? { ...s, clusterIndex: r.clusterIndex, speakerName: r.speakerName } : s;
  });
}

/** The other speakers on a line's track, for "Merge with…" (label as shown). */
export function mergeChoices(
  segments: readonly TranscriptSegment[],
  kind: string | null | undefined,
  track: number,
  clusterIndex: number,
): { clusterIndex: number; label: string }[] {
  const seen = new Map<number, string>();
  for (const line of orderedTranscript(segments, kind)) {
    const c = line.segment.clusterIndex;
    if (line.track !== track || c === null || c === undefined || c === clusterIndex) continue;
    if (!seen.has(c)) seen.set(c, line.speaker);
  }
  return [...seen.entries()].map(([c, label]) => ({ clusterIndex: c, label }));
}

/**
 * "Split this speaker": this line and every LATER line of the same speaker
 * become a new speaker — the usual shape of a label that drifted onto someone
 * else partway through. Whole rows only (rows are word-exact turns). Empty
 * when that would be every line of the speaker (that is a rename, not a split).
 */
export function splitFromHere(
  segments: readonly TranscriptSegment[],
  track: number,
  clusterIndex: number,
  fromSegmentId: string,
): string[] {
  const mine = segments
    .filter((s) => trackOf(s) === track && s.clusterIndex === clusterIndex && oneLine(s.text) !== "")
    .slice()
    .sort((a, b) => a.startSeconds - b.startSeconds);
  const at = mine.findIndex((s) => s.id === fromSegmentId);
  if (at <= 0) return [];
  return mine.slice(at).map((s) => s.id);
}

/** A typed name as a target; null when it is blank. */
export function nameTarget(input: string): NameTarget | null {
  const name = oneLine(input);
  return name === "" ? null : { kind: "newName", name };
}

/** The undo bar's sentence for a correction. */
export function correctionSentence(c: SpeakerCorrection, mergedInto?: string): string {
  switch (c.kind) {
    case "rename":
      return c.target.kind === "newName" ? `Renamed to ${c.target.name}` : "Speaker renamed";
    case "someoneElse": {
      const who = c.target.kind === "newName" ? c.target.name : "someone else";
      const n = c.segmentIds.length;
      return `${n} ${n === 1 ? "line" : "lines"} moved to ${who}`;
    }
    case "merge":
      return mergedInto ? `Merged with ${mergedInto}` : "Speakers merged";
    case "split":
      return "Split into a new speaker";
  }
}
