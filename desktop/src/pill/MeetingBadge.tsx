/**
 * MeetingBadge — the pill's persistent recording state (YV95, finding #6 item 3).
 *
 * A red dot, the elapsed time, and a stop control. Rendered by BOTH pill styles
 * (classic and yappy) from the same component, because "is Yap recording this
 * meeting" must not look like two different features depending on a cosmetic
 * setting.
 *
 * Energy, per OS-12: the dot's pulse is a CSS `@keyframes` on opacity — a
 * compositor animation that never wakes the JS thread — and the clock text is
 * whatever the backend's 1 Hz emit last said. The only timer here is
 * yap24-NT7's one-shot result hold (notes-ready / failed let go after 10 s) —
 * one `setTimeout` per result, never an interval.
 *
 * yap24-NT7 — the badge no longer goes quiet at stop: it follows the meeting
 * through the transcription pipeline (processing with percent) to
 * "Notes ready — Open" or the failure's sentence.
 */
import { useCallback, useEffect, useReducer, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  elapsedLabel,
  IDLE_MEETING,
  MEETING_EVENT,
  MEETING_PILL_IDLE,
  MEETING_PROGRESS_EVENT,
  meetingPillHoldMs,
  reduceMeetingPill,
  systemAudioBadge,
  type MeetingAfterView,
  type MeetingPillPhase,
  type MeetingProgressEvent,
  type MeetingStatus,
} from "./meeting";

interface MeetingPillState {
  status: MeetingStatus;
  phase: MeetingPillPhase;
}

type MeetingPillAction =
  | { type: "status"; status: MeetingStatus }
  | { type: "progress"; event: MeetingProgressEvent }
  | { type: "dismiss"; id: string };

function meetingPillReducer(s: MeetingPillState, a: MeetingPillAction): MeetingPillState {
  const status = a.type === "status" ? a.status : s.status;
  const phase = reduceMeetingPill(s.phase, a);
  return status === s.status && phase === s.phase ? s : { status, phase };
}

/**
 * yap24-NT7 — the meeting, as the SHELL owns it (float-main.tsx): the live
 * status (YV95) and the post-stop phase (processing → notes-ready | failed),
 * from the current value on mount and then two events the backend already
 * emits — `meeting` (status) and `meeting:progress` (the NT2 pipeline). The
 * one deadline (notes-ready / failed hold) is a single `setTimeout` per
 * result, re-armed only when the phase changes; nothing here polls.
 */
export function useMeetingPill(): {
  status: MeetingStatus;
  phase: MeetingPillPhase;
  dismiss: (id: string) => void;
} {
  const [state, dispatch] = useReducer(meetingPillReducer, { status: IDLE_MEETING, phase: MEETING_PILL_IDLE });
  useEffect(() => {
    // A synchronous cleanup can run before listen() resolves (StrictMode
    // double-mount); the `dead` flag unsubscribes a late listener so no native
    // listener leaks. Same pattern as every other subscription in this app.
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    invoke<MeetingStatus>("meeting_status")
      .then((s) => { if (!dead) dispatch({ type: "status", status: s }); })
      .catch(() => {});
    listen<MeetingStatus>(MEETING_EVENT, (e) => dispatch({ type: "status", status: e.payload })).then(push);
    listen<MeetingProgressEvent>(MEETING_PROGRESS_EVENT, (e) =>
      dispatch({ type: "progress", event: e.payload }),
    ).then(push);
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  // The hold: notes-ready (and failed) let go after MEETING_RESULT_HOLD_MS.
  // `dismiss` is scoped to the meeting id, so a timer that outlives its phase
  // (a newer meeting's result replaced it) cannot clear the newer one.
  const { phase } = state;
  useEffect(() => {
    const hold = meetingPillHoldMs(phase);
    if (hold === null || (phase.kind !== "notes-ready" && phase.kind !== "failed")) return;
    const id = phase.id;
    const t = window.setTimeout(() => dispatch({ type: "dismiss", id }), hold);
    return () => clearTimeout(t);
  }, [phase]);
  const dismiss = useCallback((id: string) => dispatch({ type: "dismiss", id }), []);
  return { status: state.status, phase: state.phase, dismiss };
}

/**
 * Open one meeting in the main window's Meetings view: the backend shows the
 * window, sends the `navigate` event (`meetings:<id>`) and clears the tray's
 * "Meeting notes ready — Open" line, so the pill and the tray are one action.
 */
export function openMeetingNotes(id: string): Promise<void> {
  return invoke<void>("open_meeting_notes", { id });
}

export default function MeetingBadge({
  status,
  after = null,
  onOpen,
}: {
  status: MeetingStatus;
  /** yap24-NT7 — the post-stop phase, already ranked below dictation by the shell. */
  after?: MeetingAfterView | null;
  onOpen?: (id: string) => void;
}) {
  if (!status.recording) {
    if (!after) return null;
    return <MeetingAfter view={after} onOpen={onOpen} />;
  }
  // YV110 — matrix rows 1 and 2 both end in "banner in the pill". This is that
  // banner: one dot of colour and one short sentence, never a dialog, because
  // the meeting is still recording and nothing here needs an answer.
  const systemAudio = systemAudioBadge(status);
  return (
    <div className="meeting-badge" role="status" aria-live="off">
      <span className="rec-dot" aria-hidden />
      <span className="rec-time">{elapsedLabel(status)}</span>
      {systemAudio && (
        <span className="rec-sysaudio" title={systemAudio} aria-label={systemAudio}>
          mic only
        </span>
      )}
      <button
        type="button"
        className="rec-stop"
        aria-label={`Stop meeting — recording for ${elapsedLabel(status)}`}
        onClick={(e) => {
          e.stopPropagation();
          invoke("stop_meeting").catch(() => {});
        }}
      >
        <svg viewBox="0 0 24 24" aria-hidden>
          <rect x="7" y="7" width="10" height="10" rx="1.5" />
        </svg>
      </button>
    </div>
  );
}

/**
 * yap24-NT7 — after stop: "Transcribing 42%" with a bar, "Notes ready — Open",
 * or the failure's own sentence. Character-agnostic DOM, rendered by both
 * capsules through the same component (Y5-K: the shell owns phases, a
 * character only owns art). The bar is a CSS custom property, not a timer.
 */
function MeetingAfter({ view, onOpen }: { view: MeetingAfterView; onOpen?: (id: string) => void }) {
  const style =
    view.percent !== null
      ? ({ ["--meeting-progress" as string]: (view.percent / 100).toFixed(3) } as CSSProperties)
      : undefined;
  const body = (
    <>
      {view.kind === "processing" && <span className="meeting-after-bar" aria-hidden><i /></span>}
      {view.kind === "notes-ready" && <span className="meeting-after-dot" aria-hidden />}
      {view.kind === "failed" && <span className="meeting-after-dot failed" aria-hidden />}
      <span className="meeting-after-label">{view.label}</span>
    </>
  );
  if (view.opens && onOpen) {
    return (
      <button
        type="button"
        className={`meeting-badge meeting-after ${view.kind}`}
        data-meeting-phase={view.kind}
        title={view.title}
        aria-label={view.title}
        onClick={(e) => {
          e.stopPropagation();
          onOpen(view.id);
        }}
      >
        {body}
      </button>
    );
  }
  return (
    <div
      className={`meeting-badge meeting-after ${view.kind}`}
      data-meeting-phase={view.kind}
      role="status"
      aria-live="polite"
      aria-label={view.title}
      title={view.title}
      style={style}
    >
      {body}
    </div>
  );
}
