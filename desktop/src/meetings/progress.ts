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

/** #250 round 3 — the notes worker's phases (`summary_jobs::NotesPhase`). */
export const NOTES_ACTIVE = ["notes_queued", "preparing_notes", "summarizing"] as const;
const NOTES_PHASES = [...NOTES_ACTIVE, "notes_ready", "notes_failed", "notes_cancelled"];

/** Mirrors `summary_jobs::OFFLINE_REASON`. */
export const OFFLINE_REASON =
  "Yap couldn't reach the internet to get the notes model. Try again when online.";

/** The notes part of a card's line, or null when no notes job is known. */
export function notesLabel(progress: MeetingProgressEvent | undefined): string | null {
  if (!progress || !NOTES_PHASES.includes(progress.state)) return null;
  switch (progress.state) {
    case "notes_queued":
      return "Notes queued";
    case "preparing_notes":
      return `Getting the notes model ${Math.max(0, Math.min(100, Math.round(progress.percent)))}%`;
    case "summarizing":
      return "Writing notes";
    case "notes_failed":
      // B1: the failure and its plain reason, never a promise.
      return `Notes failed — ${progress.error?.trim() || "something went wrong"}`;
    case "notes_cancelled":
      return "Notes cancelled";
    default:
      return null;
  }
}

/** The one line a row shows for its pipeline state. */
export function meetingStateLabel(
  state: string,
  progress: MeetingProgressEvent | undefined,
  hasSummary: boolean,
): string {
  const notes = notesLabel(progress);
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
      if (notes && notes !== "Writing notes" && !(hasSummary && progress?.state === "notes_ready")) {
        return `Transcript ready · ${notes}`;
      }
      if (notes === "Writing notes") return notes;
      if (hasSummary) return "Notes ready";
      // #250 round 3 (B1): no job known → no promise of notes that may never come.
      return "Transcript ready";
    case "partial":
      return notes && progress?.state !== "notes_ready" ? `Partly transcribed · ${notes}` : "Partly transcribed";
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

/** B2 — notes that are queued, fetching the model or being written can be cancelled. */
export function canCancelNotes(progress: MeetingProgressEvent | undefined): boolean {
  return !!progress && (NOTES_ACTIVE as readonly string[]).includes(progress.state);
}

/** B1 — failed or cancelled notes, or a transcript with no notes and no job
 *  (a past meeting the backlog banner was not used for), can be (re)queued. */
export function canRetryNotes(
  state: string,
  progress: MeetingProgressEvent | undefined,
  hasSummary: boolean,
): boolean {
  if (state !== "complete" && state !== "partial") return false;
  if (progress && (progress.state === "notes_failed" || progress.state === "notes_cancelled")) return true;
  return !hasSummary && !notesLabel(progress);
}

/** The label of that button: "Retry" after a failure, "Write notes" otherwise. */
export function retryNotesLabel(progress: MeetingProgressEvent | undefined): string {
  return progress && (progress.state === "notes_failed" || progress.state === "notes_cancelled")
    ? "Retry"
    : "Write notes";
}

/** A notes-worker status (`list_notes_jobs`) in the event shape the map holds. */
export interface NotesStatus {
  id: string;
  phase: string;
  percent: number;
  reason?: string | null;
}
export function notesStatusEvent(s: NotesStatus): MeetingProgressEvent {
  return { id: s.id, state: s.phase, percent: s.percent, processedThroughSeconds: 0, durationSeconds: 0, error: s.reason ?? null };
}

/** Events after which the list is re-read (not every download percent). */
export function reloadsMeetings(state: string): boolean {
  return state !== "transcribing" && state !== "preparing_notes";
}

/** focus-2026-10-19 — the notes' title (the `## ` line the summary opens with). */
export function summaryTitle(summary: string | null | undefined): string | null {
  const line = (summary ?? "").split("\n").find((l) => l.startsWith("## "));
  return line ? line.slice(3).trim() || null : null;
}
