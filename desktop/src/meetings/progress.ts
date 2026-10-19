// yap-meetings-fix — what a meeting row (and the pill) says about the
// transcription pipeline. Pure: no Tauri, no React. Fed by the
// `meeting:progress` event `meeting_pipeline.rs` emits after every window.

/** Mirrors `meeting_pipeline::MeetingProgressEvent` (camelCase on the wire). */
export interface MeetingProgressEvent {
  id: string;
  state: string;
  percent: number;
  processedThroughSeconds: number;
  durationSeconds: number;
  error?: string | null;
}

export const MEETING_PROGRESS_EVENT = "meeting:progress";

/** Fold one event into the per-meeting map. Percent never goes backwards
 *  while a meeting is still transcribing. */
export function foldProgress(
  map: Record<string, MeetingProgressEvent>,
  event: MeetingProgressEvent,
): Record<string, MeetingProgressEvent> {
  const prev = map[event.id];
  const percent =
    prev && prev.state === "transcribing" && event.state === "transcribing"
      ? Math.max(prev.percent, event.percent)
      : event.percent;
  return { ...map, [event.id]: { ...event, percent } };
}

/** The one line a row shows for its pipeline state. */
export function meetingStateLabel(
  state: string,
  progress: MeetingProgressEvent | undefined,
  hasSummary: boolean,
): string {
  switch (state) {
    case "recording":
      return "Recording";
    case "transcribing": {
      const pct = progress && progress.state === "transcribing" ? progress.percent : 0;
      return `Transcribing ${Math.max(0, Math.min(99, Math.round(pct)))}%`;
    }
    case "summarizing":
      return "Writing notes";
    case "complete":
      return hasSummary ? "Notes ready" : "Transcript ready · Add a summary model in Settings";
    case "partial":
      return "Partly transcribed";
    case "failed":
      return "Failed";
    default:
      return state;
  }
}

/** A failed or partial meeting can be run through the pipeline again. */
export function canRetry(state: string): boolean {
  return state === "failed" || state === "partial";
}

/** The two numbers a transcribing row reports, from the freshest source. */
export interface TranscribeProgress {
  processedSeconds: number;
  durationSeconds: number;
}

/**
 * yap-uiux-2026-10-16-M1 — where a transcribing meeting has got to. The live
 * `meeting:progress` event wins when there is one; otherwise the row's own
 * `processedThroughSeconds` (written by the pipeline after every window) is
 * the answer, so a list opened mid-transcription — or reopened after a
 * relaunch, when no event has arrived yet — never shows a bare badge.
 * `null` for any state that is not transcribing, or when the duration is
 * unknown (nothing honest to divide by).
 */
export function transcribeProgress(
  row: { state: string; durationSeconds: number; processedThroughSeconds?: number | null },
  event: MeetingProgressEvent | undefined,
): TranscribeProgress | null {
  if (row.state !== "transcribing") return null;
  const live = event && event.state === "transcribing" ? event : undefined;
  const duration = live?.durationSeconds || row.durationSeconds;
  if (!duration || duration <= 0) return null;
  const processed = Math.max(
    live?.processedThroughSeconds ?? 0,
    row.processedThroughSeconds ?? 0,
  );
  return {
    processedSeconds: Math.max(0, Math.min(duration, processed)),
    durationSeconds: duration,
  };
}

/** Whole percent, 0–99 while still transcribing (100 means done, and it is not). */
export function transcribePercent(p: TranscribeProgress): number {
  return Math.max(0, Math.min(99, Math.floor((p.processedSeconds / p.durationSeconds) * 100)));
}

/**
 * "Transcribed 8 of 25 min". Minutes once the meeting is a minute or longer
 * (processed rounds DOWN, so it never claims work not yet done); seconds for a
 * meeting shorter than that.
 */
export function transcribedLabel(p: TranscribeProgress): string {
  if (p.durationSeconds < 60)
    return `Transcribed ${Math.floor(p.processedSeconds)} of ${Math.round(p.durationSeconds)} s`;
  return `Transcribed ${Math.floor(p.processedSeconds / 60)} of ${Math.max(1, Math.round(p.durationSeconds / 60))} min`;
}

/**
 * yap-uiux-2026-10-16-M2 — the one line of a meeting's notes a compact row has
 * room for: the first non-empty line, markdown bullet/heading marks stripped.
 * `null` when there are no notes.
 */
export function summaryFirstLine(summary: string | null | undefined): string | null {
  if (!summary) return null;
  for (const raw of summary.split(/\r?\n/)) {
    const line = raw.replace(/^\s*(?:[#>*-]+|\d+[.)])\s*/, "").trim();
    if (line) return line;
  }
  return null;
}
