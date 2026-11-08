/**
 * meeting — the pill's half of YV95's manual start/stop surface.
 *
 * The pure part lives here (and is unit-tested); the React bits live in
 * MeetingBadge.tsx. Split for the same reason live.ts is split from the pills:
 * the formatting rule is the thing that can be wrong, and it should not need a
 * canvas to check.
 *
 * ## Energy (OS-12 fix 1)
 *
 * The elapsed clock is NOT a repeating JS timer in the webview and NOT a rAF frame:
 * the backend emits one `meeting` event per second with the label ALREADY
 * rendered (`meetings::format_offset` in Rust), and this module only stores it.
 * A three-hour meeting therefore wakes the JS thread ~10,800 times, once per
 * visible change, instead of 648,000 times to redraw a clock that changed once a
 * second. The recording pulse is a CSS animation on opacity/transform — it runs
 * on the compositor and does not wake JS at all.
 *
 * Those two numbers only describe THIS module. The other half of fix (1) is that
 * the Yappy pill's canvas loop parks for the meeting (`meetingRecording` in
 * live.ts `framePlan`) — otherwise its settled-idle ambient tick redraws the
 * scene every 100 ms for the whole session, 108,000 frames over three hours,
 * which dwarfs the clock this module was written to make cheap.
 */

import type { LivePhase } from "./live";
import { SAVED_HOLD_MS, SAVED_LABEL } from "./meetingScene";
import {
  MEETING_PROGRESS_EVENT,
  meetingStateLabel,
  type MeetingProgressEvent,
} from "../meetings/progress";

/**
 * The Tauri event the backend broadcasts status on, re-exported from the ONE
 * place the frontend declares it (`meetings/consent.ts`).
 *
 * Three surfaces listen to this string — the main window's banner, the pill's
 * badge, and YV96's one-time capture notice — and they were written on three
 * different branches. `src-tauri/tests/meeting_event_contract.rs` reads the
 * declaration out of `consent.ts` and asserts it against Rust's
 * `meetings::MEETING_EVENT`, and `no_surface_retypes_the_meeting_event_name`
 * asserts that nothing re-types the literal, so a rename cannot leave one of the
 * three listening to a name nobody emits.
 */
export { MEETING_EVENT } from "../meetings/consent";

/** Mirrors `meeting_control::MeetingStatus` (serde camelCase). */
export interface MeetingStatus {
  recording: boolean;
  id?: string | null;
  title?: string | null;
  elapsedSeconds: number;
  elapsedLabel?: string;
  captureAvailable: boolean;
  unavailableReason?: string | null;
  /**
   * YV110 — the running meeting's honest system-audio sentence, or absent when
   * both tracks are recording and there is nothing to say.
   *
   * Two sentences reach this field, and they are matrix rows 1 and 2: why this
   * meeting is recording the microphone only (macOS too old, the setup step
   * never run, permission looks denied), and — for a meeting that DID attach
   * the call's audio and then lost it — that the track is gone and the mic is
   * still recording. Rendered rather than logged, because a recording that
   * quietly captured half of what the user expected is the failure this whole
   * phase is about.
   */
  systemAudio?: string | null;
  /**
   * yap-1106-call-audio-truth — the live "Call audio" chip's inputs, every
   * tick of a running meeting. Absent on a backend that predates it; `null`
   * when nothing is recording. Rendered through `meetings/callAudio.ts` only.
   */
  callAudio?: CallAudioLive | null;
}

/** Mirrors `meeting_control::CallAudioLive` (serde camelCase). */
export interface CallAudioLive {
  /** Was a system-audio tap wanted (a call, not an in-person room)? */
  planned: boolean;
  /** Is a tap open for this meeting? */
  attached: boolean;
  /** YV104's discriminator: has the tap EVER delivered a non-zero sample? */
  everNonzero: boolean;
  /** Seconds since the last non-zero sample (since start when none ever came). */
  silentSeconds: number;
  /** The ghost watchdog gave the track up. */
  lost: boolean;
  /** The setup row holds no proof of the System Audio Recording grant. */
  grantUnknown: boolean;
}

export const IDLE_MEETING: MeetingStatus = {
  recording: false,
  id: null,
  title: null,
  elapsedSeconds: 0,
  elapsedLabel: "00:00:00",
  captureAvailable: false,
};

/** `3725` → `01:02:05`. The TS mirror of `meetings::format_offset`. */
export function formatElapsed(seconds: number): string {
  const total = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(h)}:${pad(m)}:${pad(s)}`;
}

/**
 * The clock to render. Prefers the label the backend already rendered so the
 * pill and the main window can never disagree by a second at a boundary, and
 * falls back to formatting the count locally — a backend that predates the
 * field must show a time, never `undefined`.
 */
export function elapsedLabel(status: MeetingStatus | null | undefined): string {
  if (!status) return "00:00:00";
  if (status.elapsedLabel && status.elapsedLabel.length > 0) return status.elapsedLabel;
  return formatElapsed(status.elapsedSeconds);
}

/**
 * What the Record control says right now. One function so the tray item, the
 * pill's button and the Meetings empty state cannot describe the same action
 * three different ways.
 */
/**
 * yap-1107-one-pill-during-meeting — the stacked clock's text: "00:03:37" reads
 * "03:37" under an hour (the artifact's "03:37"), the full label after it.
 */
export function stackClockLabel(label: string): string {
  return /^00:\d\d:\d\d$/.test(label) ? label.slice(3) : label;
}

export function recordLabel(status: MeetingStatus, app?: string | null): string {
  if (status.recording) return "Stop meeting";
  // yap-call-1 — the call offer's one-click chip: "Record Zoom call".
  if (app && app.trim()) return `Record ${app.trim()} call`;
  return "Record this meeting";
}

/**
 * The system-audio sentence to show beside a running meeting, or `null`.
 *
 * A function rather than a raw field read for the same reason `elapsedLabel` is
 * one: it is rendered by three surfaces (the pill badge, the main window's
 * recording bar, and any future meeting detail), and "show it only while
 * recording" is a rule those three must not each decide for themselves. A
 * stale badge on an idle app would describe a meeting that is over.
 */
export function systemAudioBadge(status: MeetingStatus | null | undefined): string | null {
  if (!status || !status.recording) return null;
  const note = status.systemAudio;
  return note && note.trim().length > 0 ? note : null;
}

/** Why the control is disabled, or `null` when it is not. */
export function disabledReason(status: MeetingStatus): string | null {
  if (status.recording || status.captureAvailable) return null;
  return status.unavailableReason || "Meeting recording is unavailable in this build.";
}

// ─── yap24-NT7 — the pill follows the meeting AFTER stop ─────────────────────
//
// Until this item the badge knew `recording` and nothing after it: the moment a
// meeting stopped the pill went quiet while the transcription pipeline (NT2)
// did the real work. The pill now walks the same road the pipeline does —
// recording → processing (with percent) → notes-ready | failed — and it is
// driven by the EXISTING `meeting:progress` event (`meeting_pipeline.rs`
// `MEETING_PROGRESS_EVENT`, typed once in `meetings/progress.ts`). No new event
// and no polling: this module schedules nothing at all. The one deadline the
// phase has (notes-ready holds for `MEETING_RESULT_HOLD_MS`) is armed by the
// SHELL that owns the phase (float-main.tsx), exactly like `PHASE_HOLD_MS` is
// for the take, and it lands here as a `dismiss` input that a stale timer
// cannot misapply.


/**
 * Re-exported, never re-typed: the pill listens to the one name the pipeline
 * emits and the Meetings view already folds.
 */
export { MEETING_PROGRESS_EVENT };
export type { MeetingProgressEvent };

/** How long notes-ready (and failed) holds on the pill before it lets go. */
export const MEETING_RESULT_HOLD_MS = 10_000;

/** The sentence the pill shows when the pipeline failed without saying why. */
export const MEETING_FAILED_FALLBACK = "Yap could not transcribe this meeting.";

/**
 * yap24-NT2 R3 — the sentence the pill shows when a meeting is waiting for a
 * speech model and the event carried no sentence of its own.
 */
export const MEETING_WAITING_FALLBACK =
  "No speech model is downloaded yet. Download one in Settings and Yap transcribes this meeting automatically.";

/**
 * The meeting's phase on the pill. `recording` is the live meeting (YV95);
 * everything after it is new here.
 */
export type MeetingPillPhase =
  | { kind: "idle" }
  /**
   * `stopFailed` — fix-b8-1: `stop_meeting` returned an error while this
   * meeting was still recording. If the status then drops anyway within
   * SAVED_HOLD_MS of it, the pill shows that sentence instead of "Saved":
   * Saved over a failure is a lie. Older than that, a later Stop that worked
   * caused the drop, and it is saved.
   */
  | { kind: "recording"; id: string | null; stopFailed?: { sentence: string; atMs: number } }
  /**
   * fix-b8-1-meeting-saved-beat-gets-a-slot — the stop beat ("Hoot hands over
   * the tape", SAVED_LABEL) as a REAL phase, so the one-pill layout reserves the
   * clock box for it (`meetingAfterView` → `meetingMode` "after"). It used to be
   * MeetingBadge-local state the layout never heard of, and was clipped to a
   * 0x0 box. `until` is the wall-clock end of the hold (also the scope of its
   * one timer); `next` is the post-stop road (processing / notes-ready /
   * waiting) folded underneath while the beat holds, so no progress event that
   * lands during the 2.4 s is lost. A `failed` result replaces it at once.
   */
  | { kind: "saved"; id: string | null; until: number; elapsedS: number; next: MeetingPillPhase }
  /**
   * yap-1107-pill-writing-notes-always-clears — `stalled` is set (only) by a
   * `stall` input: no notes progress for NOTES_STALL_MS while "Writing notes",
   * so the pill says "Still writing... tap to open notes" and the tap opens
   * that meeting. Absent on every other processing phase.
   */
  | { kind: "processing"; id: string; percent: number; step: "transcribing" | "summarizing"; stalled?: true }
  | { kind: "notes-ready"; id: string }
  | { kind: "failed"; id: string; sentence: string; notes?: true }
  /**
   * yap24-NT2 R3 — the job ended `waiting_for_model`: no work is running, the
   * audio is kept, and a model download requeues it. Held like a result, so
   * the pill never sits on NT7's queued "Transcribing 0%" for a job that is
   * not transcribing.
   */
  | { kind: "waiting"; id: string; sentence: string };

export const MEETING_PILL_IDLE: MeetingPillPhase = { kind: "idle" };

export type MeetingPillInput =
  /**
   * The YV95 `meeting` status broadcast. `nowMs` is the dispatch time (the
   * saved beat's clock; defaults to `Date.now()`), `lastElapsedS` the meeting's
   * last recorded elapsed seconds (the stop beat's scene time; the status that
   * says "stopped" no longer carries it).
   */
  | { type: "status"; status: MeetingStatus; nowMs?: number; lastElapsedS?: number }
  /** One `meeting:progress` event from the pipeline. `nowMs` as above. */
  | { type: "progress"; event: MeetingProgressEvent; nowMs?: number }
  /** The user clicked the result, or its hold elapsed. Scoped to one meeting. */
  | { type: "dismiss"; id: string }
  /** fix-b8-1 — the saved beat's one timer fired. Scoped to that stop's `until`. */
  | { type: "saved_elapsed"; until: number }
  /** fix-b8-1 — `stop_meeting` returned an error; `sentence` is what to show. */
  | { type: "stop_failed"; sentence: string; nowMs?: number }
  /**
   * yap-1107-pill-writing-notes-always-clears — what `list_notes_jobs` (the
   * notes worker's store) says right now. Events are hints, the store is the
   * truth: "Writing notes" stays only while the store has an ACTIVE job for
   * that meeting.
   */
  | { type: "reconcile"; jobs: readonly NotesJobSnapshot[]; nowMs?: number }
  /** yap-1107 — NOTES_STALL_MS went by with no notes progress for `id`. */
  | { type: "stall"; id: string };

/**
 * yap-1107-pill-writing-notes-always-clears — one row of `list_notes_jobs`
 * (Rust `summary_jobs::NotesStatus`, serde camelCase).
 */
export interface NotesJobSnapshot {
  id: string;
  phase: string;
  percent: number;
  reason?: string | null;
}

/** yap-1107 — no notes progress for this long => "Still writing... tap to open notes". */
export const NOTES_STALL_MS = 90_000;
/** yap-1107 — the ONE early store re-check after the pill starts "Writing notes". */
export const NOTES_RECHECK_MS = 5_000;
/** yap-1107 — the slow store re-check while notes are still being written (never below 30 s). */
export const NOTES_RECHECK_SLOW_MS = 30_000;
/** yap-1107 — the pill's words once notes have gone quiet for NOTES_STALL_MS. */
export const STILL_WRITING_LABEL = "Still writing... tap to open notes";
/**
 * yap-1107 (bounce fix, batch 4) — the WORDS ON the pill for the same two
 * moments. The pill window is 300 px (float_pill.rs PILL_W) and the clock slot
 * holds about as much as the approved "Notes ready — Open" (pill.html: short
 * words + Open; "Failed, Retry"): the full "Still writing... tap to open
 * notes" measured 176 px and a worker's failure sentence 543 px, both off the
 * window on every dock (pill-matrix --phase meeting-notes clipped-text). So the
 * pill shows the short words and the full sentence is the tooltip and the
 * accessible name, the same split the "waiting" badge already uses.
 */
export const STILL_WRITING_SHORT = "Still writing…";
/** yap-1107 — a notes failure on the pill (the worker's sentence is the tooltip). */
export const NOTES_FAILED_SHORT = "Notes failed — Open";
/** yap-1107 — any other meeting failure on the pill (its sentence is the tooltip). */
export const MEETING_FAILED_SHORT = "Failed — Open";
/**
 * yap-1107 — the `meeting:progress` state `delete_meeting` emits (Rust
 * `summary_jobs::MEETING_DELETED_STATE`): any pill phase about that id clears.
 */
export const MEETING_DELETED_STATE = "deleted";
/** yap-1107 — the notes worker's active phases (mirrors `NotesPhase::is_active`). */
const NOTES_ACTIVE_PHASES: ReadonlySet<string> = new Set(["notes_queued", "preparing_notes", "summarizing"]);
/** yap-1107 — every notes phase a `meeting:progress` can carry (the hook re-checks the store on these). */
export const NOTES_EVENT_STATES: ReadonlySet<string> = new Set([
  ...NOTES_ACTIVE_PHASES,
  "notes_ready",
  "notes_failed",
  "notes_cancelled",
  MEETING_DELETED_STATE,
]);

/**
 * yap-1107 — the meeting whose notes the pill says it is writing (also under
 * the saved beat), or `null`. The hook arms its re-checks on this.
 */
export function notesWritingId(phase: MeetingPillPhase): string | null {
  if (phase.kind === "saved") return notesWritingId(phase.next);
  return phase.kind === "processing" && phase.step === "summarizing" ? phase.id : null;
}

/** yap-1107 — the meeting a post-stop phase is about, or `null`. */
function phaseMeetingId(phase: MeetingPillPhase): string | null {
  switch (phase.kind) {
    case "processing":
    case "notes-ready":
    case "failed":
    case "waiting":
      return phase.id;
    default:
      return null;
  }
}

/**
 * fix-b8-1 — the window-local DOM event the pill's Stop button raises when
 * `stop_meeting` rejects (the button lives in MeetingBadge, the phase in the
 * shell's `useMeetingPill`). Not a Tauri event: nothing crosses the IPC.
 */
export const MEETING_STOP_FAILED_DOM_EVENT = "yap:meeting-stop-failed";

/** The sentence the pill shows when stopping a meeting failed without saying why. */
export const MEETING_STOP_FAILED_FALLBACK = "Yap could not stop this meeting. Press Stop again.";

/** `stop_meeting`'s rejection → the one sentence the pill shows. */
export function stopFailedSentence(err: unknown): string {
  const raw = typeof err === "string" ? err : err instanceof Error ? err.message : "";
  return raw.trim().length > 0 ? raw.trim() : MEETING_STOP_FAILED_FALLBACK;
}

/** The saved beat's deadline, or `null` when the phase is not the saved beat. */
export function meetingSavedUntil(phase: MeetingPillPhase): number | null {
  return phase.kind === "saved" ? phase.until : null;
}

const clampPercent = (n: number) =>
  Number.isFinite(n) ? Math.max(0, Math.min(100, Math.round(n))) : 0;

/**
 * Fold one input into the pill's meeting phase. Pure.
 *
 * - A meeting that is RECORDING outranks everything after it: a new recording
 *   replaces a result the user never clicked, and a background job's progress
 *   (a reconcile re-queue of an older meeting) never hides the live clock.
 * - `status { recording: false }` only ends `recording`; the 1 Hz idle
 *   broadcast must not wipe a processing or notes-ready phase.
 * - fix-b8-1: recording → stopped is the `saved` beat for SAVED_HOLD_MS (one
 *   `saved_elapsed` ends it), never over a failure: a `failed` / `notes_failed`
 *   event or a `stop_failed` replaces it at once.
 * - Percent never runs backwards for the same meeting while it transcribes.
 * - `complete` and `partial` are notes-ready (both have a transcript; partial's
 *   own sentence is on the meeting's page). `failed` carries the pipeline's
 *   sentence verbatim.
 */
export function reduceMeetingPill(prev: MeetingPillPhase, input: MeetingPillInput): MeetingPillPhase {
  switch (input.type) {
    case "status": {
      const s = input.status;
      if (s.recording) {
        const id = s.id ?? null;
        return prev.kind === "recording" && prev.id === id ? prev : { kind: "recording", id };
      }
      if (prev.kind !== "recording") return prev;
      // fix-b8-1 — recording → stopped. A stop that already reported an error
      // never says "Saved"; otherwise the beat holds the clock slot.
      const now = input.nowMs ?? Date.now();
      if (prev.stopFailed && now - prev.stopFailed.atMs <= SAVED_HOLD_MS) {
        return prev.id ? { kind: "failed", id: prev.id, sentence: prev.stopFailed.sentence } : MEETING_PILL_IDLE;
      }
      const e = input.lastElapsedS;
      const elapsedS = typeof e === "number" && Number.isFinite(e) && e > 0 ? Math.floor(e) : 0;
      return { kind: "saved", id: prev.id, until: now + SAVED_HOLD_MS, elapsedS, next: MEETING_PILL_IDLE };
    }
    case "progress": {
      const e = input.event;
      if (!e || typeof e.id !== "string" || e.id.length === 0) return prev;
      if (prev.kind === "recording") return prev;
      if (prev.kind === "saved") {
        // fix-b8-1 — fold the road underneath the beat. A failure is never
        // covered by "Saved"; anything else waits for the hold to end.
        const next = reduceMeetingPill(prev.next, input);
        if (next.kind === "failed") return next;
        if ((input.nowMs ?? Date.now()) >= prev.until) return next;
        return next === prev.next ? prev : { ...prev, next };
      }
      switch (e.state) {
        case "transcribing": {
          const pct = clampPercent(e.percent);
          if (prev.kind === "processing" && prev.id === e.id && prev.step === "transcribing") {
            return pct > prev.percent ? { ...prev, percent: pct } : prev;
          }
          return { kind: "processing", id: e.id, percent: pct, step: "transcribing" };
        }
        // #250 — notes run on their own worker after the transcript lands, and
        // it reports its own phases on this same event.
        case "notes_queued":
        case "preparing_notes":
        case "summarizing":
          return { kind: "processing", id: e.id, percent: 100, step: "summarizing" };
        case "complete":
        case "partial":
        case "notes_ready":
          return prev.kind === "notes-ready" && prev.id === e.id ? prev : { kind: "notes-ready", id: e.id };
        case "notes_cancelled":
          // yap-1107 — a cancel is about ONE meeting: it ends that meeting's
          // "Writing notes" (the transcript stands, so notes-ready), and a
          // late cancel for a meeting the pill is not showing (a deleted one,
          // after its "deleted" already cleared the pill) changes nothing.
          if (phaseMeetingId(prev) !== e.id) return prev;
          return prev.kind === "notes-ready" ? prev : { kind: "notes-ready", id: e.id };
        case MEETING_DELETED_STATE:
          // yap-1107 — the meeting is gone: nothing about it stays on the pill.
          return phaseMeetingId(prev) === e.id ? MEETING_PILL_IDLE : prev;
        case "notes_failed": {
          // The transcript stands; the sentence is the notes worker's reason,
          // and the pill opens the meeting page that carries "Retry notes".
          const sentence = e.error && e.error.trim().length > 0 ? e.error.trim() : MEETING_FAILED_FALLBACK;
          return { kind: "failed", id: e.id, sentence, notes: true };
        }
        case "failed": {
          const sentence = e.error && e.error.trim().length > 0 ? e.error.trim() : MEETING_FAILED_FALLBACK;
          return { kind: "failed", id: e.id, sentence };
        }
        case "waiting_for_model": {
          const sentence = e.error && e.error.trim().length > 0 ? e.error.trim() : MEETING_WAITING_FALLBACK;
          return { kind: "waiting", id: e.id, sentence };
        }
        default:
          return prev;
      }
    }
    case "saved_elapsed":
      return prev.kind === "saved" && prev.until === input.until ? prev.next : prev;
    case "reconcile": {
      if (prev.kind === "saved") {
        const next = reduceMeetingPill(prev.next, input);
        if (next.kind === "failed") return next;
        return next === prev.next ? prev : { ...prev, next };
      }
      if (prev.kind !== "processing" || prev.step !== "summarizing") return prev;
      const writingId = prev.id;
      const jobs = Array.isArray(input.jobs) ? input.jobs : [];
      const job = jobs.find((j) => j && j.id === writingId);
      // No job in the store: nothing is writing these notes (the worker
      // restarted, or it never had them) — the pill rests.
      if (!job || typeof job.phase !== "string") return MEETING_PILL_IDLE;
      if (NOTES_ACTIVE_PHASES.has(job.phase)) return prev;
      // Settled in the store: fold it exactly as its (lost) event would have.
      const settled = reduceMeetingPill(prev, {
        type: "progress",
        event: {
          id: prev.id,
          state: job.phase,
          percent: Number.isFinite(job.percent) ? job.percent : 0,
          processedThroughSeconds: 0,
          durationSeconds: 0,
          error: job.reason ?? null,
        },
        nowMs: input.nowMs,
      });
      // A phase this build does not know is still not "Writing notes".
      return settled === prev ? MEETING_PILL_IDLE : settled;
    }
    case "stall": {
      if (prev.kind === "saved") {
        const next = reduceMeetingPill(prev.next, input);
        return next === prev.next ? prev : { ...prev, next };
      }
      if (prev.kind !== "processing" || prev.step !== "summarizing" || prev.id !== input.id || prev.stalled) return prev;
      return { ...prev, stalled: true };
    }
    case "stop_failed": {
      const sentence = input.sentence.trim().length > 0 ? input.sentence.trim() : MEETING_STOP_FAILED_FALLBACK;
      if (prev.kind === "recording") return { ...prev, stopFailed: { sentence, atMs: input.nowMs ?? Date.now() } };
      if (prev.kind === "saved") return prev.id ? { kind: "failed", id: prev.id, sentence } : prev.next;
      return prev;
    }
    case "dismiss":
      if ((prev.kind === "notes-ready" || prev.kind === "failed" || prev.kind === "waiting") && prev.id === input.id) {
        return MEETING_PILL_IDLE;
      }
      return prev;
  }
}

/**
 * Does this phase hold on a deadline? (The shell arms the timer.) The saved
 * beat is not a result: its own deadline is `meetingSavedUntil`.
 */
export function meetingPillHoldMs(phase: MeetingPillPhase): number | null {
  return phase.kind === "notes-ready" || phase.kind === "failed" || phase.kind === "waiting"
    ? MEETING_RESULT_HOLD_MS
    : null;
}

/**
 * The take phases in which no dictation is on screen. Anything else — a live
 * take, its decode, its result, a refused press — is the shorter, more urgent
 * thing, and it OUTRANKS the post-stop badge.
 */
const DICTATION_QUIET: ReadonlySet<LivePhase> = new Set<LivePhase>(["idle", "sleepy"]);

/** What the badge renders for a post-stop meeting phase. */
export interface MeetingAfterView {
  kind: "saved" | "processing" | "notes-ready" | "failed" | "waiting";
  /** The meeting this is about — the click opens it. */
  id: string;
  /** The words in the pill. */
  label: string;
  /** The full sentence for the tooltip and the accessible name. */
  title: string;
  /** 0–100 for processing, else null. */
  percent: number | null;
  /** Clicking opens the meeting in the Meetings view. */
  opens: boolean;
  /** fix-b8-1 — the saved beat's scene time (the meeting's last elapsed seconds). */
  elapsedS?: number;
}

/**
 * The post-stop view, or `null` when there is nothing to show — including
 * whenever a dictation is on screen (`dictation` is the shell's winning take
 * phase). The live recording badge is NOT this function's business: it is
 * rendered from the status itself and keeps its YV95 behaviour.
 */
export function meetingAfterView(phase: MeetingPillPhase, dictation: LivePhase): MeetingAfterView | null {
  // fix-b8-1 — the saved beat outranks nothing, but it has its box even while
  // a dictation is live (the user stopped the meeting mid-take): the clock
  // slot stays reserved for the 2.4 s hold, the same slot the recording badge
  // held beside that take a moment ago.
  if (phase.kind === "saved") {
    return { kind: "saved", id: phase.id ?? "", label: SAVED_LABEL, title: "Meeting saved", percent: null, opens: false, elapsedS: phase.elapsedS };
  }
  if (!DICTATION_QUIET.has(dictation)) return null;
  switch (phase.kind) {
    case "processing": {
      const label =
        phase.step === "summarizing"
          ? meetingStateLabel("summarizing", undefined, false)
          : meetingStateLabel("transcribing", { id: phase.id, state: "transcribing", percent: phase.percent, processedThroughSeconds: 0, durationSeconds: 0 }, false);
      if (phase.step === "summarizing" && phase.stalled) {
        // yap-1107 — notes went quiet for NOTES_STALL_MS: say so, and let the
        // tap open that meeting (its page shows where the notes are).
        // Short words on the pill; the full sentence is the tooltip + name.
        return { kind: "processing", id: phase.id, label: STILL_WRITING_SHORT, title: STILL_WRITING_LABEL, percent: 100, opens: true };
      }
      return { kind: "processing", id: phase.id, label, title: `${label} — your meeting notes are on their way`, percent: phase.step === "summarizing" ? 100 : Math.min(99, phase.percent), opens: false };
    }
    case "notes-ready":
      return { kind: "notes-ready", id: phase.id, label: "Notes ready — Open", title: "Meeting notes ready — open them in Yap", percent: null, opens: true };
    case "failed":
      // yap-1107 — short words on the pill (a sentence runs off the 300 px
      // window); the sentence — what went wrong, what to do — is the tooltip
      // and the accessible name, and the tap opens the page with Retry.
      return { kind: "failed", id: phase.id, label: phase.notes ? NOTES_FAILED_SHORT : MEETING_FAILED_SHORT, title: phase.sentence, percent: null, opens: true };
    case "waiting":
      // Short words in the pill (the Meetings view's own label); the full
      // sentence — what to do next — is the tooltip and accessible name.
      return { kind: "waiting", id: phase.id, label: meetingStateLabel("waiting_for_model", undefined, false), title: phase.sentence, percent: null, opens: true };
    default:
      return null;
  }
}

/**
 * The `navigate` payload that opens ONE meeting in the Meetings view. The Rust
 * side builds the same string (`meeting_notes_nav_payload` in lib.rs) for the
 * tray item; `parseNavigate` in the main window reads it.
 */
export const MEETING_NAV_PREFIX = "meetings:";
export function meetingNavPayload(id: string): string {
  return `${MEETING_NAV_PREFIX}${id}`;
}

/** Split a `navigate` payload into the view and (optionally) the meeting to open. */
export function parseNavigate(payload: string): { nav: string; meetingId: string | null } {
  if (typeof payload === "string" && payload.startsWith(MEETING_NAV_PREFIX)) {
    const id = payload.slice(MEETING_NAV_PREFIX.length);
    return { nav: "meetings", meetingId: id.length > 0 ? id : null };
  }
  return { nav: payload, meetingId: null };
}
