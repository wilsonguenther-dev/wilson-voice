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
      // focus-2026-10-19: notes always come — no setup step to name.
      if (hasSummary) return "Notes ready";
      if (progress && progress.state === "preparing_notes") {
        return `Transcript ready · Getting the notes model ${Math.max(0, Math.min(100, Math.round(progress.percent)))}%`;
      }
      if (progress && progress.state === "summarizing") return "Writing notes";
      return "Transcript ready · Notes on the way";
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

/** focus-2026-10-19 — the notes' title (the `## ` line the summary opens with). */
export function summaryTitle(summary: string | null | undefined): string | null {
  const line = (summary ?? "").split("\n").find((l) => l.startsWith("## "));
  return line ? line.slice(3).trim() || null : null;
}
