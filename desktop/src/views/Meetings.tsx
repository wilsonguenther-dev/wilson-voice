import { invoke } from "@tauri-apps/api/core";
import { errorText } from "../errors";
import { disabledReason, elapsedLabel, systemAudioBadge, recordLabel, type MeetingStatus } from "../pill/meeting";
import TranscriptList from "../meetings/TranscriptList";
import { formatMeetingDuration, formatTime } from "../appTypes";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { ErrorState, LoadingState, RoomGlyph } from "../ViewStates";

export default function Meetings() {
  const {
    booting, confirmDeleteMeeting, insights, loadMeetings, meetingBusy, meetingKind, meetingKinds,
    meetingsError, meetingsLoading,
    meetingQuery, meetingStatus, meetings, openMeeting, openMeetingDetail, setConfirmDeleteMeeting,
    setMeetingBusy, setMeetingKind, setMeetingQuery, setMeetingStatus, setOpenMeeting, 
    toast,
  } = useAppCtx();
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

              <div className="toolbar">
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
                {/* Once there ARE meetings the CTA is a toolbar button; the
                    big empty-state version below is for the first one. */}
                {meetings.length > 0 && !meetingStatus.recording && (
                  <button
                    className="primary"
                    disabled={meetingBusy || !!disabledReason(meetingStatus)}
                    title={disabledReason(meetingStatus) || undefined}
                    onClick={toggleMeetingRecording}
                  >
                    Record a meeting
                  </button>
                )}
              </div>

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
                      : "Recorded meetings land here — searchable, exportable, and deletable in one click. Audio is kept for 7 days; the transcript is kept for good."}
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
                    </>
                  )}
                </div>
              ) : (
                <ul className="feed">
                  {meetings.map((m) => (
                    <li key={m.id} className="card">
                      <div className="card-meta">
                        <span>
                          {formatTime(m.startedAt)} ·{" "}
                          {formatMeetingDuration(m.durationSeconds)}
                        </span>
                        <span>
                          {m.segmentCount}{" "}
                          {m.segmentCount === 1 ? "segment" : "segments"}
                          {m.audioKept ? "" : " · audio expired"}
                        </span>
                      </div>
                      <p className="meeting-title">
                        {m.title}
                        <span className={`meeting-state ${m.state}`}>
                          {m.state}
                        </span>
                      </p>
                      {m.error && <p className="tiny bad">{m.error}</p>}
                      <div className="actions">
                        <button
                          className="primary"
                          onClick={() => openMeetingDetail(m.id)}
                        >
                          Open transcript
                        </button>
                        <button
                          className="ghost"
                          disabled={meetingBusy}
                          onClick={() => exportMeeting(m.id)}
                        >
                          Export Markdown
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
                  ))}
                </ul>
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
                  <h3>Summary</h3>
                  <p>{openMeeting.meeting.summary}</p>
                </div>
              )}

              {openMeeting.segments.length === 0 ? (
                <div className="empty" data-empty-state="meetings:segments">
                  <h3>No transcript yet</h3>
                  <p>
                    {openMeeting.meeting.state === "recording" ||
                    openMeeting.meeting.state === "transcribing"
                      ? "Yap is still working through the audio. This page does not refresh itself."
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
