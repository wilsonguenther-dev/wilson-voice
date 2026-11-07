/**
 * yap-notetaker-notes — the light live transcript, frontend side.
 *
 * `live_transcript.rs` emits a `meeting:live` snapshot after every rolling
 * decode (~10 s) while a meeting records, and one empty, non-recording
 * snapshot when it stops — the stored transcript replaces it from there.
 * Pure and DOM-free so the rules are unit-tested, not eyeballed.
 */

/** The event `live_transcript::LIVE_EVENT` names. */
export const LIVE_EVENT = "meeting:live";

/**
 * Mirrors `live_transcript::LiveVoice` (yap-1106-live-whos-talking): who said
 * one live line. `name` null = the track's own word (You / Call, or nothing on
 * a mic-only meeting). `hint` is the reason behind a name ("Probably Alex —
 * voice match 0.82"): a live name is a suggestion, never a fact.
 */
export interface LiveVoice {
  name: string | null;
  hint: string | null;
}

/** Mirrors `live_transcript::LiveTrack`. */
export interface LiveTrack {
  track: number;
  /** "You" / "Call" on a two-track meeting; null when there is one track. */
  speaker: string | null;
  lines: string[];
  /** One per line, same order. Older payloads may omit it. */
  voices?: LiveVoice[];
}

/** Consecutive lines of one track said by the same voice, as the view draws them. */
export interface LiveRun {
  /** The name, "Someone new", the track's own word, or null (no label at all). */
  label: string | null;
  hint: string | null;
  lines: string[];
}

/** The word "Someone new" `live_transcript::SOMEONE_NEW` sends. */
export const SOMEONE_NEW = "Someone new";

/** Mirrors `live_transcript::LiveSnapshot`. */
export interface LiveSnapshot {
  recording: boolean;
  tracks: LiveTrack[];
}

export const NO_LIVE: LiveSnapshot = { recording: false, tracks: [] };

/**
 * The snapshot the view keeps. A stopped snapshot clears it, and so does the
 * meeting no longer recording (the banner's own status), whichever arrives
 * first. Lines are trimmed and empties dropped.
 */
export function reduceLive(next: LiveSnapshot | null | undefined, recording: boolean): LiveSnapshot {
  if (!recording || !next || !next.recording) return NO_LIVE;
  return {
    recording: true,
    tracks: (next.tracks ?? []).map((t) => {
      const lines: string[] = [];
      const voices: LiveVoice[] = [];
      (t.lines ?? []).forEach((raw, i) => {
        const line = raw.trim();
        if (line.length === 0) return;
        const v = t.voices?.[i];
        lines.push(line);
        voices.push({ name: v?.name?.trim() || null, hint: v?.hint ?? null });
      });
      return { track: t.track, speaker: t.speaker ?? null, lines, voices };
    }),
  };
}

/**
 * A track's lines grouped into runs by who said them: a line with a matched
 * voice carries that name, every other line the track's own word. A new label
 * starts a new run, so a name change mid-track is drawn where it happened.
 */
export function liveRuns(t: LiveTrack): LiveRun[] {
  const runs: LiveRun[] = [];
  t.lines.forEach((line, i) => {
    const v = t.voices?.[i];
    const label = v?.name ?? t.speaker;
    const hint = v?.name ? (v.hint ?? null) : null;
    const last = runs[runs.length - 1];
    if (last && last.label === label && last.hint === hint) {
      last.lines.push(line);
    } else {
      runs.push({ label, hint, lines: [line] });
    }
  });
  return runs;
}

/** Is there anything to draw yet? */
export function hasLiveLines(s: LiveSnapshot): boolean {
  return s.tracks.some((t) => t.lines.length > 0);
}
