import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import {
  canCancelNotes, canRetryNotes, canRetryTranscript, meetingStateLabel, notesLabel, retryNotesLabel,
  summaryFirstLine, transcribedLabel, transcribePercent, transcribeProgress,
} from "../meetings/progress";
import NotesView from "../meetings/NotesView";
import { errorText } from "../errors";
import { disabledReason, elapsedLabel, systemAudioBadge, recordLabel, type MeetingStatus } from "../pill/meeting";
import TranscriptList from "../meetings/TranscriptList";
import SpeechEngineNotice from "../meetings/SpeechEngineNotice";
import "../meetings/meetings.css";
import NotetakerSetupCard from "../meetings/NotetakerSetupCard";
import { formatMeetingDuration, formatTime } from "../appTypes";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { ErrorState, LoadingState, RoomGlyph } from "../ViewStates";

export default function Meetings() {
  const {
    booting, confirmDeleteMeeting, insights, loadMeetings, meetingBusy, meetingKind, meetingKinds,
    meetingsError, meetingsLoading,
    meetingQuery, meetingStatus, meetings, modelSetup, openMeeting, openMeetingDetail,
    openModelSettings, setConfirmDeleteMeeting,
    setMeetingBusy, setMeetingKind, setMeetingQuery, setMeetingStatus, setOpenMeeting,
    toast,
  } = useAppCtx();
  const { meetingProgress: progress, retryMeeting: retry, retryNotes, cancelNotes } = useAppCtx();
  const retryMeeting = async (id: string) => {
    try {
      await retry(id);
    } catch (e) {
      toast(errorText(e));
    }
  };
  const notesAction = async (run: (id: string) => Promise<void>, id: string) => {
    try {
      await run(id);
    } catch (e) {
      toast(errorText(e));
    }
  };
  // #250 round 3 (A3) — past meetings are summarized only on this one click;
  // an upgrade never starts the notes-model download on its own.
  const [backlog, setBacklog] = useState<{ count: number; offer: boolean } | null>(null);
  useEffect(() => {
    invoke<{ count: number; offer: boolean }>("notes_backlog")
      .then(setBacklog)
      .catch(() => setBacklog(null));
  }, [meetings.length]);
  async function answerBacklog(accept: boolean) {
    try {
      if (accept) await invoke<number>("summarize_notes_backlog");
      else await invoke("dismiss_notes_backlog");
      setBacklog(null);
    } catch (e) {
      toast(errorText(e));
    }
  }
  // Y5-B — a third `return null` blank page, and the worst of them: the read
  // that feeds it (loadMeetings) swallowed its own rejection, so a failed
  // meetings query rendered as "No meetings yet" — Yap telling a user their
  // recordings were gone.
  const state = viewState(
    meetings,
    booting || (meetingsLoading && meetings.length === 0),
    meetingsError,
  );
  async function exportMeeting(id: string) {
    setMeetingBusy(true);
    try {
      const { path } = await invoke<{ path: string; count: number }>(
        "export_meeting_markdown",
        { id },
      );
      toast(`Exported → ${path}`);
    } catch (e) {
      toast(errorText(e));
    } finally {
      setMeetingBusy(false);
    }
  }
  async function toggleMeetingRecording() {
    setMeetingBusy(true);
    try {
      const next = await invoke<MeetingStatus>("toggle_meeting_recording", {
        // Ignored by the backend on a press that STOPS a meeting: the recording
        // that is ending was started under whatever it was started under.
        kind: meetingKind,
      });
      setMeetingStatus(next);
      if (!next.recording) await loadMeetings(meetingQuery);
    } catch (e) {
      toast(errorText(e));
    } finally {
      setMeetingBusy(false);
    }
  }
  async function removeMeeting(id: string) {
    setMeetingBusy(true);
    try {
      await invoke("delete_meeting", { id });
      setConfirmDeleteMeeting(null);
      setOpenMeeting((m) => (m?.meeting.id === id ? null : m));
      await loadMeetings(meetingQuery);
      toast("Meeting deleted");
    } catch (e) {
      toast(errorText(e));
    } finally {
      setMeetingBusy(false);
    }
  }
  async function renameMeeting(id: string, title: string) {
    const next = title.trim();
    if (!next) return;
    try {
      await invoke("rename_meeting", { id, title: next });
      setOpenMeeting((m) =>
        m?.meeting.id === id
          ? { ...m, meeting: { ...m.meeting, title: next } }
          : m,
      );
      await loadMeetings(meetingQuery);
    } catch (e) {
      toast(errorText(e));
    }
  }
  if (state === "error")
    return (
      <ErrorState
        data-error-state="meetings"
        view="meetings"
        error={meetingsError}
        actionLabel="Open meetings again"
        onAction={() => void loadMeetings(meetingQuery)}
      />
    );
  if (state === "loading" || !insights)
    return (
      <LoadingState
        data-loading-state="meetings"
        noun="meetings"
        expected={insights?.meetings?.totalMeetings ?? null}
        onRetry={() => void loadMeetings(meetingQuery)}
      />
    );
  return (
    <>
      {!openMeeting && (
            <>
              {/* yap-notetaker-onboard — the Set up Notetaker card: top of the
                  Meetings view until every step is done or it is closed. */}
              <NotetakerSetupCard onImported={() => void loadMeetings(meetingQuery)} />
              {/* YV95 — the live recording banner. Present ONLY while a meeting
                  is running, and it carries the same clock the pill shows,
                  rendered once in Rust and emitted at 1 Hz. */}
              {meetingStatus.recording && (
                <div className="recording-bar" role="status">
                  <span className="rec-dot" aria-hidden />
                  <div className="recording-copy">
                    <strong>{meetingStatus.title || "Recording a meeting"}</strong>
                    <span className="tiny">
                      Recording for {elapsedLabel(meetingStatus)} · stop from here, the
                      menu bar, ⌃⌘M, or the pill.
                    </span>
                    {/* YV110 — matrix rows 1 and 2. A meeting that is recording
                        the microphone only, or that lost the call's audio
                        mid-way, says so here in full while it is still
                        recording — never afterwards, and never as a dialog. */}
                    {systemAudioBadge(meetingStatus) && (
                      <span className="tiny rec-sysaudio-note">
                        {systemAudioBadge(meetingStatus)}
                      </span>
                    )}
                  </div>
                  <span className="rec-clock">{elapsedLabel(meetingStatus)}</span>
                  <button
                    className="primary danger"
                    disabled={meetingBusy}
                    onClick={toggleMeetingRecording}
                  >
                    Stop meeting
                  </button>
                </div>
              )}

              {/* The toolbar exists once there is a list to search (or a
                  search to clear). With zero meetings the empty state below
                  carries the picker and the record button, so neither is
                  shown twice. Record is the FIRST control (walk rule). */}
              {(meetings.length > 0 || meetingQuery) && (
              <div className="toolbar">
                {!meetingStatus.recording && (
                  <button
                    className="primary"
                    disabled={meetingBusy || !!disabledReason(meetingStatus)}
                    title={disabledReason(meetingStatus) || undefined}
                    onClick={toggleMeetingRecording}
                  >
                    Record a meeting
                  </button>
                )}
                <input
                  type="search"
                  value={meetingQuery}
                  onChange={(e) => setMeetingQuery(e.target.value)}
                  placeholder="Search meetings — titles and every word said in them…"
                  aria-label="Search meetings"
                />
                {meetingQuery && (
                  <button className="ghost" onClick={() => setMeetingQuery("")}>
                    Clear
                  </button>
                )}
                {/* YV125 — the start-of-meeting kind picker. It sits BESIDE
                    the record button rather than in front of it: there is no
                    calendar in this phase to infer the kind from, so it is
                    asked, and "Not sure" is a real answer that changes nothing
                    about whether the recording starts. Which choice is
                    selected decides whether diarization clusters the
                    microphone or treats it as one speaker. */}
                {meetingKinds.length > 0 && !meetingStatus.recording && (
                  <div
                    className="meeting-kind-picker"
                    role="radiogroup"
                    aria-label="What kind of meeting is this?"
                  >
                    {meetingKinds.map((c) => (
                      <button
                        key={c.kind}
                        type="button"
                        role="radio"
                        aria-checked={meetingKind === c.kind}
                        className={
                          meetingKind === c.kind ? "chip selected" : "chip"
                        }
                        onClick={() => setMeetingKind(c.kind)}
                      >
                        {c.label}
                      </button>
                    ))}
                  </div>
                )}
              </div>
              )}

              {/* yap-uiux-2026-10-16-MEETINGS — the needs-model state: one
                  click installs the speech engine; never an error. It sits
                  AFTER the record control (record/stop is always the first
                  control); with no meetings it lives inside the empty state. */}
              {(meetings.length > 0 || meetingQuery) && (
                <SpeechEngineNotice setup={modelSetup} />
              )}

              {meetings.length === 0 ? (
                <div className="empty" data-empty-state="meetings">
                  {/* Y5-B — Meetings gets its OWN drawing. The item forbids one
                      generic illustration reused across seven views. */}
                  <div className="view-state-glyph">
                    <RoomGlyph />
                  </div>
                  <h3>
                    {meetingQuery ? "No meetings match that" : "No meetings yet"}
                  </h3>
                  <p>
                    {meetingQuery
                      ? "Search covers meeting titles and every word transcribed inside them."
                      : `Recorded meetings land here — searchable and exportable. Audio is kept for ${
                          // 0 is "no meetings yet" in Insights, never a real policy.
                          insights?.meetings?.audioRetentionDays || 7
                        } days; the transcript is kept for good.`}
                  </p>
                  {/* YV95 / finding #6 — the empty state IS the entry point.
                      Everything above this item shipped a feature with no way
                      to reach it; this is the one big button that fixes that. */}
                  {!meetingQuery && (
                    <>
                      {/* The same three choices as the toolbar's, from the same
                          list — a first-time user gets the question too. */}
                      {meetingKinds.length > 0 && !meetingStatus.recording && (
                        <div
                          className="meeting-kind-picker"
                          role="radiogroup"
                          aria-label="What kind of meeting is this?"
                        >
                          {meetingKinds.map((c) => (
                            <button
                              key={c.kind}
                              type="button"
                              role="radio"
                              aria-checked={meetingKind === c.kind}
                              className={
                                meetingKind === c.kind ? "chip selected" : "chip"
                              }
                              onClick={() => setMeetingKind(c.kind)}
                            >
                              {c.label}
                            </button>
                          ))}
                        </div>
                      )}
                      <button
                        className="primary big"
                        disabled={meetingBusy || !!disabledReason(meetingStatus)}
                        title={disabledReason(meetingStatus) || undefined}
                        onClick={toggleMeetingRecording}
                      >
                        {recordLabel(meetingStatus)}
                      </button>
                      <p className="tiny">
                        {disabledReason(meetingStatus) ??
                          `Or press ⌃⌘M from anywhere, or pick “Record a meeting” in the menu bar. Everything stays on this Mac.`}
                      </p>
                      <SpeechEngineNotice setup={modelSetup} />
                    </>
                  )}
                </div>
              ) : (
                <>
                {backlog?.offer && (
                  <div className="card notes-backlog" role="region" aria-label="Notes for past meetings">
                    <p className="meeting-title">
                      Summarize {backlog.count} past meeting{backlog.count === 1 ? "" : "s"}?
                    </p>
                    <p className="tiny muted">
                      Yap writes notes on this Mac, one meeting at a time. If the notes model isn't here yet, it
                      downloads once first (about 1.1 GB).
                    </p>
                    <div className="actions">
                      <button className="primary" onClick={() => void answerBacklog(true)}>Summarize</button>
                      <button className="ghost" onClick={() => void answerBacklog(false)}>Not now</button>
                    </div>
                  </div>
                )}
                <ul className="meetings-list" data-meetings-list>
                  {meetings.map((m) => {
                    const tp = transcribeProgress(m, progress[m.id]);
                    const notes = summaryFirstLine(m.summary);
                    // #250: meetingStateLabel already says "Transcript ready" when no
                    // notes job is known, and names every notes phase when one is.
                    const stateLabel = tp
                      ? `Transcribing ${transcribePercent(tp)}%`
                      : meetingStateLabel(m.state, progress[m.id], Boolean(m.summary));
                    const notesJob = notesLabel(progress[m.id]);
                    return (
                      <li key={m.id} className="meeting-row" data-meeting-state={m.state}>
                        <div className="meeting-row-main">
                          <div className="meeting-row-head">
                            <button
                              type="button"
                              className="meeting-row-title"
                              title={m.title}
                              onClick={() => openMeetingDetail(m.id)}
                            >
                              {m.title}
                            </button>
                            <span className={`meeting-state ${m.state}`}>{stateLabel}</span>
                            <span className="meeting-row-when">
                              {formatMeetingDuration(m.durationSeconds)} · {formatTime(m.startedAt)}
                            </span>
                          </div>
                          {/* One line of detail, in priority order: the failure,
                              the transcription's progress, the notes, or what to
                              do about missing notes. */}
                          {m.error ? (
                            <p className="meeting-row-detail bad" title={m.error}>
                              <span>{m.error}</span>
                            </p>
                          ) : tp ? (
                            <p className="meeting-row-detail" data-meeting-progress>
                              <span
                                className="meeting-row-track"
                                role="progressbar"
                                aria-label={`${m.title} transcription`}
                                aria-valuenow={transcribePercent(tp)}
                                aria-valuemin={0}
                                aria-valuemax={100}
                              >
                                <span style={{ width: `${transcribePercent(tp)}%` }} />
                              </span>
                              <span>{transcribedLabel(tp)}</span>
                            </p>
                          ) : notes ? (
                            <p className="meeting-row-detail" title={notes}>
                              <span>{notes}</span>
                            </p>
                          ) : m.state === "complete" && notesJob && canCancelNotes(progress[m.id]) ? (
                            <p className="meeting-row-detail">
                              <span>{notesJob}</span>
                            </p>
                          ) : m.state === "complete" ? (
                            <p className="meeting-row-detail">
                              <span>No notes yet — </span>
                              <button type="button" className="link" onClick={openModelSettings}>
                                add a notes model
                              </button>
                            </p>
                          ) : (
                            <p className="meeting-row-detail">
                              <span>
                                {m.segmentCount} {m.segmentCount === 1 ? "segment" : "segments"}
                                {m.audioKept ? "" : " · audio expired"}
                              </span>
                            </p>
                          )}
                        </div>
                        <div className="actions">
                          <button className="primary" onClick={() => openMeetingDetail(m.id)}>
                            Open
                          </button>
                          {canRetryTranscript(m.state, progress[m.id]) && (
                            <button onClick={() => void retryMeeting(m.id)}>Retry</button>
                          )}
                          {canCancelNotes(progress[m.id]) && (
                            <button className="ghost" onClick={() => void notesAction(cancelNotes, m.id)}>
                              Cancel notes
                            </button>
                          )}
                          {canRetryNotes(m.state, progress[m.id], Boolean(m.summary)) && (
                            <button onClick={() => void notesAction(retryNotes, m.id)}>
                              {canRetryTranscript(m.state, progress[m.id]) && retryNotesLabel(progress[m.id]) === "Retry"
                                ? "Retry notes"
                                : retryNotesLabel(progress[m.id])}
                            </button>
                          )}
                          <button
                            className="ghost"
                            disabled={meetingBusy}
                            onClick={() => exportMeeting(m.id)}
                          >
                            Export
                          </button>
                          <button
                            className="ghost danger"
                            disabled={meetingBusy}
                            onClick={() =>
                              confirmDeleteMeeting === m.id
                                ? removeMeeting(m.id)
                                : setConfirmDeleteMeeting(m.id)
                            }
                          >
                            {confirmDeleteMeeting === m.id
                              ? "Delete for good?"
                              : "Delete"}
                          </button>
                        </div>
                      </li>
                    );
                  })}
                </ul>
                </>
              )}
            </>
      )}
      {openMeeting && (
            <div className="meeting-detail">
              <div className="actions">
                <button className="ghost" onClick={() => setOpenMeeting(null)}>
                  ← All meetings
                </button>
                <button
                  className="primary"
                  disabled={meetingBusy}
                  onClick={() => exportMeeting(openMeeting.meeting.id)}
                >
                  Export Markdown
                </button>
                <button
                  className="ghost danger"
                  disabled={meetingBusy}
                  onClick={() =>
                    confirmDeleteMeeting === openMeeting.meeting.id
                      ? removeMeeting(openMeeting.meeting.id)
                      : setConfirmDeleteMeeting(openMeeting.meeting.id)
                  }
                >
                  {confirmDeleteMeeting === openMeeting.meeting.id
                    ? "Delete for good?"
                    : "Delete meeting"}
                </button>
              </div>

              {/* The title is editable in place — ASR names a meeting
                  "Meeting", a person names it after what it was. */}
              <input
                className="meeting-title-edit"
                defaultValue={openMeeting.meeting.title}
                aria-label="Meeting title"
                key={openMeeting.meeting.id}
                onBlur={(e) =>
                  renameMeeting(openMeeting.meeting.id, e.target.value)
                }
              />
              <p className="card-meta">
                <span>
                  {formatTime(openMeeting.meeting.startedAt)} ·{" "}
                  {formatMeetingDuration(openMeeting.meeting.durationSeconds)} ·{" "}
                  {openMeeting.meeting.state}
                </span>
                <span>
                  {openMeeting.audioOnDisk
                    ? "audio kept"
                    : `audio deleted after ${
                        insights?.meetings?.audioRetentionDays ?? 7
                      } days`}
                </span>
              </p>

              {openMeeting.meeting.summary && (
                <div className="panel">
                  <h3>Notes</h3>
                  <NotesView markdown={openMeeting.meeting.summary} />
                </div>
              )}

              {openMeeting.segments.length === 0 ? (
                <div className="empty" data-empty-state="meetings:segments">
                  <h3>No transcript yet</h3>
                  <p>
                    {openMeeting.meeting.state === "recording" ||
                    openMeeting.meeting.state === "transcribing"
                      ? "Yap is still working through the audio. This page does not refresh itself."
                      : openMeeting.meeting.state === "waiting_for_model"
                        ? "No speech model is downloaded yet. Download one in Settings and Yap transcribes this meeting automatically. The audio is kept until then."
                        : "Nothing was transcribed from this recording. The audio is still on disk for 7 days."}
                  </p>
                  <div className="actions">
                    <button
                      type="button"
                      className="primary"
                      onClick={() => void openMeetingDetail(openMeeting.meeting.id)}
                    >
                      Check again
                    </button>
                  </div>
                </div>
              ) : (
                <TranscriptList
                  segments={openMeeting.segments}
                  kind={openMeeting.meeting.kind}
                />
              )}
            </div>
      )}
    </>
  );
}
