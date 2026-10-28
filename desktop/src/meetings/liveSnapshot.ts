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

/** Mirrors `live_transcript::LiveTrack`. */
export interface LiveTrack {
  track: number;
  /** "You" / "Call" on a two-track meeting; null when there is one track. */
  speaker: string | null;
  lines: string[];
}

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
    tracks: (next.tracks ?? []).map((t) => ({
      track: t.track,
      speaker: t.speaker ?? null,
      lines: (t.lines ?? []).map((l) => l.trim()).filter((l) => l.length > 0),
    })),
  };
}

/** Is there anything to draw yet? */
export function hasLiveLines(s: LiveSnapshot): boolean {
  return s.tracks.some((t) => t.lines.length > 0);
}
