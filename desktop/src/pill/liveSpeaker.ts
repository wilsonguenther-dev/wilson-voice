/**
 * fix-l3b4-1-pill-shows-live-speaker — who is talking, in the pill.
 *
 * yap-1106-live-whos-talking promised that the live transcript AND the pill
 * show the name ("Alex"), "Someone new" or the track's own word ("Call").
 * The pill keeps ONE current speaker word: the voice behind the newest
 * non-empty live line across tracks. The `meeting:live` snapshot carries no
 * clock per line, so "newest" is the track whose lines changed since the
 * previous snapshot (a tick decodes tracks in order, so the later changed
 * track spoke last); when nothing changed the previous speaker's track holds.
 *
 * Pure and DOM-free (no @tauri-apps): MeetingBadge.tsx owns the listener and
 * feeds `stepLiveSpeaker`; the snapshot rules themselves are reduceLive's in
 * meetings/liveSnapshot.ts, reused here, never forked.
 */
import { NO_LIVE, SOMEONE_NEW, reduceLive, type LiveSnapshot, type LiveTrack } from "../meetings/liveSnapshot";

/** What the pill shows — also the one telemetry value (`pill live_speaker shown=`). */
export type LiveSpeakerShown = "name" | "someone_new" | "track" | "none";

export interface LiveSpeaker {
  shown: LiveSpeakerShown;
  /** The word the pill draws: "Alex", "Someone new", "You" / "Call", or null. */
  word: string | null;
  /** The reason behind a name ("Probably Alex — voice match 0.82"); null otherwise. */
  hint: string | null;
  /** The track the word came from; null when nothing is shown. */
  track: number | null;
}

export const NO_SPEAKER: LiveSpeaker = { shown: "none", word: null, hint: null, track: null };

/** The listener's state: the last kept snapshot (to tell which track changed) and its speaker. */
export interface LiveSpeakerState {
  snap: LiveSnapshot;
  speaker: LiveSpeaker;
}

export const LIVE_SPEAKER_IDLE: LiveSpeakerState = { snap: NO_LIVE, speaker: NO_SPEAKER };

function sameLines(a: string[] | undefined, b: string[]): boolean {
  if (!a || a.length !== b.length) return false;
  return a.every((line, i) => line === b[i]);
}

/**
 * The track holding the newest non-empty line: a track whose lines changed
 * since `prev` (the later one in tick order when several did), else the
 * previous speaker's track while it still has lines, else the last track
 * with lines. Null when no track has a line.
 */
export function newestTrack(prev: LiveSnapshot, next: LiveSnapshot, prevTrack: number | null): LiveTrack | null {
  const withLines = next.tracks.filter((t) => t.lines.length > 0);
  if (withLines.length === 0) return null;
  const changed = withLines.filter((t) => !sameLines(prev.tracks.find((p) => p.track === t.track)?.lines, t.lines));
  if (changed.length > 0) return changed[changed.length - 1];
  const kept = prevTrack === null ? undefined : withLines.find((t) => t.track === prevTrack);
  return kept ?? withLines[withLines.length - 1];
}

/**
 * The speaker of a track's newest line: a matched name beats the track's own
 * word, "Someone new" is its own kind, and a mic-only meeting (one track, no
 * word) with no match shows nothing.
 */
export function speakerOfTrack(t: LiveTrack): LiveSpeaker {
  const i = t.lines.length - 1;
  if (i < 0) return NO_SPEAKER;
  const v = t.voices?.[i];
  const name = v?.name?.trim() || null;
  if (name === SOMEONE_NEW) return { shown: "someone_new", word: SOMEONE_NEW, hint: v?.hint ?? null, track: t.track };
  if (name) return { shown: "name", word: name, hint: v?.hint ?? null, track: t.track };
  const own = t.speaker?.trim() || null;
  if (own) return { shown: "track", word: own, hint: null, track: t.track };
  return NO_SPEAKER;
}

function sameSpeaker(a: LiveSpeaker, b: LiveSpeaker): boolean {
  return a.shown === b.shown && a.word === b.word && a.hint === b.hint && a.track === b.track;
}

/**
 * One `meeting:live` payload folded into the pill's state. `recording` is
 * the meeting's own status: a stopped snapshot, or the meeting no longer
 * recording, clears the speaker (reduceLive's rule). Returns `state` itself
 * when nothing the pill draws changed, so React can skip the render.
 */
export function stepLiveSpeaker(
  state: LiveSpeakerState,
  payload: LiveSnapshot | null | undefined,
  recording: boolean,
): LiveSpeakerState {
  const snap = reduceLive(payload, recording);
  if (!snap.recording) return state.speaker === NO_SPEAKER && state.snap === NO_LIVE ? state : LIVE_SPEAKER_IDLE;
  const t = newestTrack(state.snap, snap, state.speaker.track);
  const next = t ? speakerOfTrack(t) : NO_SPEAKER;
  return { snap, speaker: sameSpeaker(next, state.speaker) ? state.speaker : next };
}

/** The speaker of one snapshot on its own (no history): the render path's fixture input. */
export function pickLiveSpeaker(snapshot: LiveSnapshot | null | undefined, recording: boolean): LiveSpeaker {
  return stepLiveSpeaker(LIVE_SPEAKER_IDLE, snapshot, recording).speaker;
}

/**
 * The tooltip: a name always reads as a suggestion — its hint, or
 * "Probably <name>" when the backend sent none. Null when nothing is shown.
 */
export function liveSpeakerTitle(s: LiveSpeaker): string | null {
  switch (s.shown) {
    case "name":
      return s.hint ?? `Probably ${s.word}`;
    case "someone_new":
      return s.hint ?? "Someone new — a voice Yap does not know yet";
    case "track":
      return `Talking now: ${s.word}`;
    default:
      return null;
  }
}

/** The accessible label / the stack tooltip's line. Null when nothing is shown. */
export function liveSpeakerLine(s: LiveSpeaker): string | null {
  switch (s.shown) {
    case "name":
    case "someone_new":
      return `Talking now: ${liveSpeakerTitle(s)}`;
    case "track":
      return liveSpeakerTitle(s);
    default:
      return null;
  }
}

/** The one log line, on change: the kind only — never a name or a word. */
export function liveSpeakerLog(s: LiveSpeaker): string {
  return `pill live_speaker shown=${s.shown}`;
}
