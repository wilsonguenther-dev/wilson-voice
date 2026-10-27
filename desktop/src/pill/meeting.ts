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
export function recordLabel(status: MeetingStatus): string {
  if (status.recording) return "Stop meeting";
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
  | { kind: "recording"; id: string | null }
  | { kind: "processing"; id: string; percent: number; step: "transcribing" | "summarizing" }
  | { kind: "notes-ready"; id: string }
  | { kind: "failed"; id: string; sentence: string }
  /**
   * yap24-NT2 R3 — the job ended `waiting_for_model`: no work is running, the
   * audio is kept, and a model download requeues it. Held like a result, so
   * the pill never sits on NT7's queued "Transcribing 0%" for a job that is
   * not transcribing.
   */
  | { kind: "waiting"; id: string; sentence: string };

export const MEETING_PILL_IDLE: MeetingPillPhase = { kind: "idle" };

export type MeetingPillInput =
  /** The YV95 `meeting` status broadcast. */
  | { type: "status"; status: MeetingStatus }
  /** One `meeting:progress` event from the pipeline. */
  | { type: "progress"; event: MeetingProgressEvent }
  /** The user clicked the result, or its hold elapsed. Scoped to one meeting. */
  | { type: "dismiss"; id: string };

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
      return prev.kind === "recording" ? MEETING_PILL_IDLE : prev;
    }
    case "progress": {
      const e = input.event;
      if (!e || typeof e.id !== "string" || e.id.length === 0) return prev;
      if (prev.kind === "recording") return prev;
      switch (e.state) {
        case "transcribing": {
          const pct = clampPercent(e.percent);
          if (prev.kind === "processing" && prev.id === e.id && prev.step === "transcribing") {
            return pct > prev.percent ? { ...prev, percent: pct } : prev;
          }
          return { kind: "processing", id: e.id, percent: pct, step: "transcribing" };
        }
        case "summarizing":
          return { kind: "processing", id: e.id, percent: 100, step: "summarizing" };
        case "complete":
        case "partial":
          return prev.kind === "notes-ready" && prev.id === e.id ? prev : { kind: "notes-ready", id: e.id };
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
    case "dismiss":
      if ((prev.kind === "notes-ready" || prev.kind === "failed" || prev.kind === "waiting") && prev.id === input.id) {
        return MEETING_PILL_IDLE;
      }
      return prev;
  }
}

/** Does this phase hold on a deadline? (The shell arms the timer.) */
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
  kind: "processing" | "notes-ready" | "failed" | "waiting";
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
}

/**
 * The post-stop view, or `null` when there is nothing to show — including
 * whenever a dictation is on screen (`dictation` is the shell's winning take
 * phase). The live recording badge is NOT this function's business: it is
 * rendered from the status itself and keeps its YV95 behaviour.
 */
export function meetingAfterView(phase: MeetingPillPhase, dictation: LivePhase): MeetingAfterView | null {
  if (!DICTATION_QUIET.has(dictation)) return null;
  switch (phase.kind) {
    case "processing": {
      const label =
        phase.step === "summarizing"
          ? meetingStateLabel("summarizing", undefined, false)
          : meetingStateLabel("transcribing", { id: phase.id, state: "transcribing", percent: phase.percent, processedThroughSeconds: 0, durationSeconds: 0 }, false);
      return { kind: "processing", id: phase.id, label, title: `${label} — your meeting notes are on their way`, percent: phase.step === "summarizing" ? 100 : Math.min(99, phase.percent), opens: false };
    }
    case "notes-ready":
      return { kind: "notes-ready", id: phase.id, label: "Notes ready — Open", title: "Meeting notes ready — open them in Yap", percent: null, opens: true };
    case "failed":
      return { kind: "failed", id: phase.id, label: phase.sentence, title: phase.sentence, percent: null, opens: true };
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
