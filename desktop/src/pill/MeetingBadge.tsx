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
import { useCallRecordingView } from "./CallOffer";
import { badgeCallLabel } from "./callOfferModel";
import { useCallback, useEffect, useReducer, useRef, type CSSProperties, type ReactNode } from "react";
import MeetingWorld from "./MeetingWorld";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  elapsedLabel,
  stackClockLabel,
  IDLE_MEETING,
  MEETING_EVENT,
  MEETING_PILL_IDLE,
  MEETING_PROGRESS_EVENT,
  MEETING_STOP_FAILED_DOM_EVENT,
  meetingPillHoldMs,
  meetingSavedUntil,
  reduceMeetingPill,
  systemAudioBadge,
  type MeetingAfterView,
  type MeetingPillInput,
  type MeetingPillPhase,
  type MeetingProgressEvent,
  type MeetingStatus,
} from "./meeting";
import { callAudioChip } from "../meetings/callAudio";
import { LIVE_EVENT, type LiveSnapshot } from "../meetings/liveSnapshot";
import {
  LIVE_SPEAKER_IDLE,
  liveSpeakerLine,
  liveSpeakerLog,
  liveSpeakerTitle,
  pickLiveSpeaker,
  stepLiveSpeaker,
  type LiveSpeaker,
  type LiveSpeakerState,
} from "./liveSpeaker";
import { stopMeetingFromPill } from "./MeetingDictationCard";

interface MeetingPillState {
  status: MeetingStatus;
  phase: MeetingPillPhase;
}

type MeetingPillAction = MeetingPillInput;

function meetingPillReducer(s: MeetingPillState, a: MeetingPillAction): MeetingPillState {
  const status = a.type === "status" ? a.status : s.status;
  // fix-b8-1 — the stop beat's scene time is the meeting's LAST recorded
  // elapsed seconds: the status that says "stopped" carries 0.
  const input: MeetingPillInput =
    a.type === "status" && s.status.recording ? { ...a, lastElapsedS: s.status.elapsedSeconds } : a;
  const phase = reduceMeetingPill(s.phase, input);
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
      .then((s) => { if (!dead) dispatch({ type: "status", status: s, nowMs: Date.now() }); })
      .catch(() => {});
    // The dispatch time is read HERE, not in the reducer: a reducer must stay
    // pure (StrictMode runs it twice).
    listen<MeetingStatus>(MEETING_EVENT, (e) => dispatch({ type: "status", status: e.payload, nowMs: Date.now() })).then(push);
    listen<MeetingProgressEvent>(MEETING_PROGRESS_EVENT, (e) =>
      dispatch({ type: "progress", event: e.payload, nowMs: Date.now() }),
    ).then(push);
    // fix-b8-1 — the Stop button's rejection (window-local, see MeetingBadge).
    const onStopFailed = (e: Event) => {
      const detail = (e as CustomEvent<unknown>).detail;
      dispatch({ type: "stop_failed", sentence: typeof detail === "string" ? detail : "", nowMs: Date.now() });
    };
    window.addEventListener(MEETING_STOP_FAILED_DOM_EVENT, onStopFailed);
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
      window.removeEventListener(MEETING_STOP_FAILED_DOM_EVENT, onStopFailed);
    };
  }, []);
  // The hold: notes-ready (and failed) let go after MEETING_RESULT_HOLD_MS.
  // `dismiss` is scoped to the meeting id, so a timer that outlives its phase
  // (a newer meeting's result replaced it) cannot clear the newer one.
  const { phase } = state;
  useEffect(() => {
    const hold = meetingPillHoldMs(phase);
    if (hold === null || (phase.kind !== "notes-ready" && phase.kind !== "failed" && phase.kind !== "waiting")) return;
    const id = phase.id;
    const t = window.setTimeout(() => dispatch({ type: "dismiss", id }), hold);
    return () => clearTimeout(t);
  }, [phase]);
  // fix-b8-1 — the stop beat (yap-1106-pill-rec-headphones-hoot-tape): ONE
  // setTimeout per stop, keyed on that stop's `until`, so a progress event
  // folding the road underneath the beat (a new phase object, same `until`)
  // never re-arms it. The timer's input is scoped to `until`: a stale one is a
  // no-op on a newer stop.
  const savedUntil = meetingSavedUntil(phase);
  const savedElapsedS = phase.kind === "saved" ? phase.elapsedS : 0;
  useEffect(() => {
    if (savedUntil === null) return;
    console.info(`pill meeting_saved elapsed_s=${savedElapsedS}`);
    const until = savedUntil;
    const t = window.setTimeout(() => dispatch({ type: "saved_elapsed", until }), Math.max(0, until - Date.now()));
    return () => clearTimeout(t);
  }, [savedUntil, savedElapsedS]);
  const dismiss = useCallback((id: string) => dispatch({ type: "dismiss", id }), []);
  return { status: state.status, phase: state.phase, dismiss };
}

/** The last live state of the recording meeting, across slot remounts (see useLiveSpeaker). */
let liveCarry: { id: string; state: LiveSpeakerState } | null = null;

type LiveSpeakerAction = { type: "live"; payload: LiveSnapshot | null } | { type: "clear" };

function liveSpeakerReducer(s: LiveSpeakerState, a: LiveSpeakerAction): LiveSpeakerState {
  // A payload only arrives while the listener is armed, i.e. while the
  // meeting records; "clear" is the stop / unmount.
  return a.type === "clear" ? LIVE_SPEAKER_IDLE : stepLiveSpeaker(s, a.payload, true);
}

/**
 * fix-l3b4-1-pill-shows-live-speaker — who is talking right now, from the
 * `meeting:live` snapshot live_transcript.rs broadcasts (app.emit, so the
 * pill window receives it). Listens ONLY while `recording`; stop or unmount
 * unlistens and clears. One `pill live_speaker shown=<kind>` line per change
 * of what the pill shows — never the name or any transcript text.
 */
export function useLiveSpeaker(recording: boolean, meetingId: string | null = null): LiveSpeaker {
  // The slot remounts when the badge flips between the scene and the stack
  // (a take starts or ends); the last speaker of THIS meeting carries over so
  // the word does not blank until the next ~10 s tick. A new meeting id, or
  // none, starts empty.
  const [state, dispatch] = useReducer(liveSpeakerReducer, meetingId, (id: string | null): LiveSpeakerState => {
    const carry = liveCarry;
    return id !== null && carry !== null && carry.id === id ? carry.state : LIVE_SPEAKER_IDLE;
  });
  useEffect(() => {
    if (recording && meetingId !== null) liveCarry = { id: meetingId, state };
  }, [recording, meetingId, state]);
  useEffect(() => {
    if (!recording) return;
    // Same late-listener guard as useMeetingPill: a cleanup that runs before
    // listen() resolves unsubscribes the listener when it lands.
    let dead = false;
    let un: (() => void) | null = null;
    listen<LiveSnapshot>(LIVE_EVENT, (e) => dispatch({ type: "live", payload: e.payload }))
      .then((u) => (dead ? u() : (un = u)))
      .catch((err) => console.warn(`pill live_speaker listen_failed error=${String(err).slice(0, 80)}`));
    return () => {
      dead = true;
      un?.();
      dispatch({ type: "clear" });
    };
  }, [recording]);
  const speaker = recording ? state.speaker : LIVE_SPEAKER_IDLE.speaker;
  // The change key holds the word so a new name logs again, but the line
  // itself carries only the kind. A mount (fresh, or a scene/stack remount
  // that carried the speaker over) is not a change and logs nothing.
  const key = `${speaker.shown}|${speaker.word ?? ""}`;
  const lastKey = useRef(key);
  useEffect(() => {
    if (lastKey.current === key) return;
    lastKey.current = key;
    console.info(liveSpeakerLog(speaker));
  }, [key, speaker]);
  return speaker;
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
  stacked = false,
  live,
}: {
  status: MeetingStatus;
  /** yap24-NT7 — the post-stop phase, already ranked below dictation by the shell. */
  after?: MeetingAfterView | null;
  onOpen?: (id: string) => void;
  /**
   * fix-l3b1-1-meeting-stack-matches-layout — the layout reserved the small
   * stack box (CLOCK_STACK_W) for the clock: the shells pass
   * `meetingClockStacked(gate)` (state.ts), the same test `clockWidth` uses,
   * so the badge never draws the 112 px scene into the 96 px box. True for
   * every phase Yappy's own capsule is open in (a take, transcribing, done,
   * error, ...), not only while listening.
   */
  stacked?: boolean;
  /**
   * fix-l3b4-1 — a fixed live snapshot (render tests, the pill matrix). When
   * given, the badge draws its speaker and does not listen; when omitted the
   * badge listens to `meeting:live` itself while the meeting records.
   */
  live?: LiveSnapshot | null;
}) {
  // yap-call-1 — the pill never lies: a recording that started from a call
  // offer names the app ("Zoom"), and FaceTime / Phone say "your side".
  const callApp = badgeCallLabel(useCallRecordingView());
  // fix-l3b4-1 — who is talking. The badge itself stays hook-free beyond the
  // call-offer view (tests call it as a plain function): the listener lives in
  // the LiveSpeakerSlot child, and a fixture snapshot skips it entirely.
  const speakerSlot = (mode: "row" | "stack", children: ReactNode, baseTitle = "") =>
    live === undefined ? (
      <LiveSpeakerSlot recording={status.recording} meetingId={status.id ?? null} mode={mode} baseTitle={baseTitle}>
        {children}
      </LiveSpeakerSlot>
    ) : (
      <LiveSpeakerView speaker={pickLiveSpeaker(live, status.recording)} mode={mode} baseTitle={baseTitle}>
        {children}
      </LiveSpeakerView>
    );
  if (!status.recording) {
    // yap-1106-pill-rec-headphones-hoot-tape / fix-b8-1 — the stop beat:
    // Hoot hands over the tape and the pill reads "Saved". It is the shell's
    // `saved` meeting phase now (pill/meeting.ts), so the layout reserved its
    // clock box (meetingMode "after") and its one timer lives in useMeetingPill.
    if (after?.kind === "saved") {
      return (
        <div className="meeting-badge meeting-saved" data-pill-slot="clock" data-meeting-phase="saved" role="status" aria-label={after.title}>
          <MeetingWorld elapsedS={after.elapsedS ?? 0} handover />
          <span className="meeting-saved-label">{after.label}</span>
        </div>
      );
    }
    if (!after) return null;
    return <MeetingAfter view={after} onOpen={onOpen} />;
  }
  // YV110 — matrix rows 1 and 2 both end in "banner in the pill". This is that
  // banner: one dot of colour and one short sentence, never a dialog, because
  // the meeting is still recording and nothing here needs an answer.
  const systemAudio = systemAudioBadge(status);
  // yap-1106-call-audio-truth — the live chip: one word in the pill (space is
  // pixels there), the sentence on the tooltip and the accessible label. A
  // backend without the field falls back to YV110's two-word "mic only".
  const callAudio = callAudioChip(status);
  // yap-1107-one-pill-during-meeting — dictating mid-meeting: Yappy's own capsule
  // opens for the take, so the scene (and its second Yappy) steps out and the
  // clock "shrinks into a small stack next to stop" (Pill artifact, "Meeting +
  // dictating"), inside the same shell. The call-audio truth rides the stack's
  // tooltip and label, so it is never lost. Stop still ends the meeting.
  // fix-l3b1-1 — whenever the capsule is open (meetingClockStacked), not only
  // while listening: the layout's box and this branch share one source.
  if (stacked) {
    const audioLine = callAudio ? `${callAudio.label}. ${callAudio.detail}` : systemAudio ?? "";
    const time = stackClockLabel(elapsedLabel(status));
    return (
      <div className="meeting-badge meeting-stacked" data-pill-slot="clock" data-meeting-phase="stacked" role="status" aria-live="off">
        {/* fix-l3b4-1 — the 96 px stack has no room for a word: who is
            talking rides the stack's tooltip and label, after the call-audio line. */}
        {speakerSlot(
          "stack",
          <span className="meeting-stack-time" data-scene-time>{time}</span>,
          [callApp ? `Recording ${callApp} call` : "Recording a meeting", audioLine].filter(Boolean).join(" · "),
        )}
        <button
          type="button"
          className="rec-stop"
          aria-label={`Stop meeting — recording for ${elapsedLabel(status)}${audioLine ? `. ${audioLine}` : ""}`}
          onClick={(e) => {
            e.stopPropagation();
            // yap-fx-06 — the same one-click Stop as the scene's: a failure is
            // never swallowed (the stop-failed card, and no "Saved" after it).
            void stopMeetingFromPill("pill_stack");
          }}
        >
          <svg viewBox="0 0 24 24" aria-hidden>
            <rect x="7" y="7" width="10" height="10" rx="1.5" />
          </svg>
        </button>
      </div>
    );
  }
  const audioChip = callAudio ? (
    <span
      className={`rec-sysaudio call-audio-chip is-${callAudio.state}`}
      data-call-audio={callAudio.state}
      title={systemAudio ? `${callAudio.detail} ${systemAudio}` : callAudio.detail}
      aria-label={`${callAudio.label}. ${callAudio.detail}`}
    >
      call {callAudio.state}
    </span>
  ) : (
    systemAudio && (
      <span className="rec-sysaudio" title={systemAudio} aria-label={systemAudio}>
        mic only
      </span>
    )
  );
  return (
    <div className="meeting-badge" data-pill-slot="clock" role="status" aria-live="off">
      {/* yap-1106-pill-rec-headphones-hoot-tape — no separate red dot: the
          right ear cup of Yappy's headphones is the recording light, and the
          elapsed time sits under the sun inside the scene. */}
      <MeetingWorld elapsedS={status.elapsedSeconds} />
      {callApp && (
        <span className="rec-app" data-call-app title={`Recording ${callApp} call`}>
          {callApp}
        </span>
      )}
      {/* fix-l3b4-1 — who is talking: one word stacked OVER the call-audio
          chip, so the clock row (CLOCK_W 208) gains no width. */}
      {speakerSlot("row", audioChip)}
      <button
        type="button"
        className="rec-stop"
        aria-label={`Stop meeting — recording for ${elapsedLabel(status)}`}
        onClick={(e) => {
          e.stopPropagation();
          // yap-fx-06 — the one-click Stop, always in the pill while a meeting
          // records (dictating or not). A failure is never swallowed: it
          // opens the stop-failed card (Try again / Quit Yap). fix-b8-1 — the
          // same window event (MEETING_STOP_FAILED_EVENT === the shell's
          // MEETING_STOP_FAILED_DOM_EVENT) reaches useMeetingPill, so a failed
          // stop is never followed by "Saved".
          void stopMeetingFromPill("badge");
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
      {view.kind === "waiting" && <span className="meeting-after-dot waiting" aria-hidden />}
      <span className="meeting-after-label">{view.label}</span>
    </>
  );
  if (view.opens && onOpen) {
    return (
      <button
        type="button"
        className={`meeting-badge meeting-after ${view.kind}`}
      data-pill-slot="clock"
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
        data-pill-slot="clock"
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

/**
 * fix-l3b4-1 — the live listener as a child of the badge: listens to
 * `meeting:live` only while the meeting records (useLiveSpeaker), then draws
 * through the same view a fixture snapshot uses.
 */
function LiveSpeakerSlot({
  recording,
  meetingId,
  mode,
  baseTitle,
  children,
}: {
  recording: boolean;
  meetingId: string | null;
  mode: "row" | "stack";
  baseTitle: string;
  children: ReactNode;
}) {
  const speaker = useLiveSpeaker(recording, meetingId);
  return (
    <LiveSpeakerView speaker={speaker} mode={mode} baseTitle={baseTitle}>
      {children}
    </LiveSpeakerView>
  );
}

/**
 * fix-l3b4-1 — the speaker inside the one meeting surface.
 * "row" (the scene badge): the word stacked over the call-audio chip in one
 *   column; the hint is its title and label, so a name reads as a guess.
 *   No speaker = exactly the children, the row unchanged.
 * "stack" (mid-take, 96 px): no room for a word — the stack's tooltip gets
 *   the speaker line after the call-audio line, and a screen-reader-only line
 *   carries it to the accessible text.
 */
function LiveSpeakerView({
  speaker,
  mode,
  baseTitle,
  children,
}: {
  speaker: LiveSpeaker;
  mode: "row" | "stack";
  baseTitle: string;
  children: ReactNode;
}) {
  const line = liveSpeakerLine(speaker);
  if (mode === "stack") {
    return (
      <span
        className="meeting-stack"
        data-meeting-stack
        data-live-speaker={line ? speaker.shown : undefined}
        title={[baseTitle, line ?? ""].filter(Boolean).join(" · ")}
      >
        {children}
        {line && <span className="sr-only">{line}</span>}
      </span>
    );
  }
  if (!speaker.word) return <>{children}</>;
  return (
    <span className="rec-side" data-live-speaker-side>
      <span
        className={`rec-speaker is-${speaker.shown}`}
        data-live-speaker={speaker.shown}
        title={liveSpeakerTitle(speaker) ?? undefined}
        aria-label={line ?? undefined}
      >
        {speaker.word}
      </span>
      {children}
    </span>
  );
}
