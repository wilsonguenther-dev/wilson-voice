import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import {
  canRetryTranscript, meetingActions, meetingStateLabel, notesLabel, retryNotesLabel,
  transcribedLabel, transcribePercent, transcribeProgress,
} from "../meetings/progress";
import NotesView, { StreamingNotes } from "../meetings/NotesView";
import ExportMenu, { FloatingMenu, exportItems, type ExportAction } from "../meetings/ExportMenu";
import { disabledReason, elapsedLabel, systemAudioBadge, recordLabel, type MeetingStatus } from "../pill/meeting";
import TranscriptList from "../meetings/TranscriptList";
import LiveTranscript from "../meetings/LiveTranscript";
import SpeechEngineNotice from "../meetings/SpeechEngineNotice";
import { CALL_AUDIO_ACTION_LABEL, callAudioChip, recordedTracks } from "../meetings/callAudio";
import "../meetings/meetings.css";
import NotetakerSetupCard from "../meetings/NotetakerSetupCard";
import WhoIsTalking from "../meetings/WhoIsTalking";
import { NOTES_ACTIVE } from "../meetings/progress";
import YappySprite from "../license/YappySprite";
import {
  applyLabelRows,
  clusterLabel,
  withSpeakerNames,
  type CorrectionApplied,
  type CorrectionUndone,
  type LabelRow,
  type SpeakerCorrection,
} from "../meetings/transcript";
import {
  NO_FILTER, defaultTab, detailPeople, duePendingDeletes, filterActive, filterMeetings, groupMeetings, isNarrow,
  listPeople, meetingsKeyAction, meetingsLogLine, moveSelection, peopleOptions, rowTabStop, schedulePendingDelete, shortId,
  stopNeedsConfirm, undoPendingDelete, unnamedVoices, withoutPending,
  type DetailTab, type MeetingFilter, type PendingDelete,
} from "../meetings/meetingsLayout";
import {
  DetailTabs, ListSkeleton, MeetingFilters, PeopleChips, PeopleTab, PickAMeeting, RecordingHero, UndoDeleteBar,
  WhoIsTalkingStrip, reserveStatusWidth, useTruncatedTitles,
} from "../meetings/MeetingsPanes";
import { formatMeetingDuration, formatTime } from "../appTypes";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { ErrorState, RoomGlyph } from "../ViewStates";

/** yap-1106-meetings-redesign — one line per Meetings action (§1 telemetry). */
function logUi(event: string, fields: Record<string, string | number | boolean> = {}): void {
  console.info(meetingsLogLine(event, fields));
}

function inTextField(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.tagName !== "string") return false;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable;
}

/**
 * yap-1106-meetings-redesign (YAP-UI-SPEC-2026-11 §3.3) — two panes: the list
 * on the left (date groups, people chips, state chip), the detail on the
 * right (title, date, duration, tracks chip, people chips; Notes |
 * Transcript | People), and the recording state as its own hero card with the
 * live transcript. Under 900 px the detail covers the list and has a Back
 * button. Delete takes effect after a 10 s Undo.
 */
export default function Meetings() {
  const {
    booting, insights, loadMeetings, meetingBusy, meetingKind, meetingKinds,
    meetingsError, meetingsLoading,
    meetingQuery, meetingStatus, meetings, modelSetup, openMeeting, openMeetingDetail,
    openModelSettings,
    setMeetingBusy, setMeetingKind, setMeetingQuery, setMeetingStatus, setOpenMeeting,
    runSystemAudioSetup, sysAudioBusy, toast, toastError,
  } = useAppCtx();
  // yap-1106-call-audio-truth — the live chip, decided once in callAudio.ts.
  const callAudio = callAudioChip(meetingStatus);
  const { meetingProgress: progress, retryMeeting: retry, retryNotes, cancelNotes } = useAppCtx();

  const [filter, setFilter] = useState<MeetingFilter>(NO_FILTER);
  const [tab, setTab] = useState<DetailTab>("transcript");
  const [selected, setSelected] = useState<string | null>(null);
  // yap-1107-meetings-row-actions-no-overlay — the row's right-click menu
  // (Export ×3 + Delete), opened at the pointer; one at a time.
  const [rowMenu, setRowMenu] = useState<{ id: string; title: string; x: number; y: number } | null>(null);
  const [pending, setPending] = useState<PendingDelete[]>([]);
  const [now, setNow] = useState(() => Date.now());
  const [narrow, setNarrow] = useState(() => (typeof window === "undefined" ? false : isNarrow(window.innerWidth)));
  const [confirmStop, setConfirmStop] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const pendingRef = useRef<PendingDelete[]>(pending);
  pendingRef.current = pending;

  const retryMeeting = async (id: string) => {
    try {
      await retry(id);
    } catch (e) {
      toastError(e);
    }
  };
  const notesAction = async (run: (id: string) => Promise<void>, id: string) => {
    try {
      await run(id);
    } catch (e) {
      toastError(e);
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
      toastError(e);
    }
  }

  // ── Layout: two panes, or one under 900 px ──────────────────────────────
  useEffect(() => {
    const onResize = () => setNarrow(isNarrow(window.innerWidth));
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  // ── Open: a new detail opens on Notes when it has notes, else Transcript ─
  const openId = openMeeting?.meeting.id ?? null;
  useEffect(() => {
    if (!openMeeting) return;
    setTab(defaultTab(openMeeting));
    setSelected(openMeeting.meeting.id);
    logUi("open", { id: shortId(openMeeting.meeting.id), segments: openMeeting.segments.length });
    // Only a DIFFERENT meeting resets the tab; a refresh of the same one keeps it.
  }, [openId]);

  // ── Delete + restore: the 10 s Undo ─────────────────────────────────────
  async function commitDelete(p: PendingDelete) {
    const started = Date.now();
    try {
      await invoke("delete_meeting", { id: p.id });
      logUi("delete", { id: shortId(p.id), undone: false, ms: Date.now() - started });
    } catch (e) {
      logUi("delete_failed", { id: shortId(p.id) });
      toastError(e, "Couldn't delete this meeting, so it is back in the list.");
    }
    await loadMeetings(meetingQuery);
  }
  function requestDelete(id: string, title: string) {
    setPending((list) => schedulePendingDelete(list, id, title, Date.now()));
    setNow(Date.now());
    setOpenMeeting((m) => (m?.meeting.id === id ? null : m));
    setSelected((s) => (s === id ? null : s));
    logUi("delete_requested", { id: shortId(id) });
  }
  function undoDelete(id: string) {
    setPending((list) => undoPendingDelete(list, id));
    logUi("delete", { id: shortId(id), undone: true });
    toast("Meeting restored");
  }
  // A 1 s local tick, only while something waits out its Undo (no I/O).
  useEffect(() => {
    if (pending.length === 0) return;
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [pending.length]);
  useEffect(() => {
    const { due, waiting } = duePendingDeletes(pending, now);
    if (due.length === 0) return;
    // The ref moves now, so a leave in the same tick cannot delete twice.
    pendingRef.current = waiting;
    setPending(waiting);
    for (const p of due) void commitDelete(p);
  }, [now, pending]);
  // Leaving the view keeps the promise the Undo made: the time ran with the
  // person's attention, so whatever is still waiting is deleted now.
  useEffect(
    () => () => {
      for (const p of pendingRef.current) {
        invoke("delete_meeting", { id: p.id })
          .then(() => logUi("delete", { id: shortId(p.id), undone: false, on: "leave" }))
          .catch(() => logUi("delete_failed", { id: shortId(p.id), on: "leave" }));
      }
    },
    [],
  );

  // ── Find: search + filters, then date groups ───────────────────────────
  const visible = useMemo(() => filterMeetings(withoutPending(meetings, pending), filter), [meetings, pending, filter]);
  const groups = useMemo(() => groupMeetings(visible, new Date()), [visible]);
  const people = useMemo(() => peopleOptions(meetings), [meetings]);
  const ids = useMemo(() => groups.flatMap((g) => g.meetings.map((m) => m.id)), [groups]);
  // yap-1107-meetings-row-no-stale-tooltip — the row has no title attribute;
  // a row's title span gets one only while its ellipsis cuts it, measured by
  // ONE ResizeObserver over the list (re-armed when rows or titles change).
  const titlesKey = useMemo(
    () => groups.flatMap((g) => g.meetings.map((m) => `${m.id}\u0001${m.title}`)).join("\u0000"),
    [groups],
  );
  const titlesRef = useTruncatedTitles(titlesKey, (truncated, rows) => logUi("row_titles_cut", { truncated, rows }));
  // yap-1107-meetings-row-whole-click-target: the list is ONE Tab stop (roving
  // tabindex): the selected row, else the open one, else the first. ↑/↓ move it.
  const tabStopId = rowTabStop(ids, selected, openId);
  const detailPeopleRows = useMemo(
    () => (openMeeting ? detailPeople(openMeeting.segments, openMeeting.meeting.kind) : []),
    [openMeeting],
  );
  useEffect(() => {
    if (filterActive(filter)) logUi("filter", { kind: filter.kind, notes: filter.notes, person: Boolean(filter.person) });
  }, [filter]);

  function openRow(id: string, via: "pointer" | "keyboard" = "pointer") {
    setSelected(id);
    logUi("row_open", { id: shortId(id), via });
    void openMeetingDetail(id);
  }
  function openRowMenu(e: { preventDefault: () => void; clientX: number; clientY: number; currentTarget: Element }, id: string, title: string) {
    e.preventDefault();
    // The keyboard context-menu key reports 0,0: anchor to the row instead.
    const r = e.currentTarget.getBoundingClientRect();
    const keyboard = e.clientX === 0 && e.clientY === 0;
    setSelected(id);
    setRowMenu({ id, title, x: keyboard ? r.left + 24 : e.clientX, y: keyboard ? r.top + r.height / 2 : e.clientY });
    logUi("row_menu", { id: shortId(id), keyboard });
  }
  function closeDetail() {
    if (!openMeeting) return;
    logUi("close", { id: shortId(openMeeting.meeting.id) });
    setOpenMeeting(null);
  }

  // ── Keyboard (§3.3): ↑/↓, Return, ⌫, ⌘F, Escape, ⌥⌘1/2/3 ─────────────
  const keyState = useRef({ ids, selected, openMeeting, meetings });
  keyState.current = { ids, selected, openMeeting, meetings };
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const field = inTextField(e.target);
      const action = meetingsKeyAction(e, field);
      if (!action) return;
      const k = keyState.current;
      switch (action.kind) {
        case "move": {
          e.preventDefault();
          const next = moveSelection(k.ids, k.selected, action.by);
          setSelected(next);
          if (next) {
            // yap-1107-meetings-row-whole-click-target: when a row has keyboard
            // focus, focus follows the selection (roving tab stop), so the ring
            // and the selected fill never sit on two different rows.
            const rowButton = document.querySelector<HTMLElement>(`[data-meeting-id="${next}"] .meeting-row-main`);
            if (rowButton && document.activeElement?.classList.contains("meeting-row-main")) rowButton.focus();
            document.querySelector(`[data-meeting-id="${next}"]`)?.scrollIntoView({ block: "nearest" });
          }
          break;
        }
        case "open":
          if (k.selected) {
            e.preventDefault();
            openRow(k.selected, "keyboard");
          }
          break;
        case "delete": {
          const id = k.selected ?? k.openMeeting?.meeting.id ?? null;
          const row = id ? k.meetings.find((m) => m.id === id) : undefined;
          if (row) {
            e.preventDefault();
            requestDelete(row.id, row.title);
          }
          break;
        }
        case "search":
          e.preventDefault();
          searchRef.current?.focus();
          break;
        case "close":
          if (field) (e.target as HTMLElement).blur();
          else if (k.openMeeting) {
            // Read through the ref: this listener is installed once.
            logUi("close", { id: shortId(k.openMeeting.meeting.id) });
            setOpenMeeting(null);
          }
          break;
        case "tab":
          if (k.openMeeting) {
            e.preventDefault();
            setTab(action.tab);
          }
          break;
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

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
      toastError(e, "Couldn't export this meeting. Check the export folder in Settings › Meetings.");
    } finally {
      setMeetingBusy(false);
    }
  }
  // yap-1106 — the three export actions, from the row menu and the detail
  // header alike. Each one is a Rust command that logs its own outcome; the
  // toast here is the one sentence the person sees.
  async function copyMeeting(id: string) {
    try {
      await invoke<number>("copy_meeting_markdown", { id });
      toast("Copied as Markdown");
    } catch (e) {
      toastError(e, "Couldn't copy this meeting. Try Export Markdown instead.");
    }
  }
  async function revealMeeting(id: string) {
    try {
      const r = await invoke<{ path: string; target: "file" | "folder" }>(
        "reveal_meeting_export",
        { id },
      );
      if (r.target === "folder") toast("No export yet — opened the meetings folder");
    } catch (e) {
      toastError(e, "Couldn't open the export in Finder. Export it first from this menu.");
    }
  }
  function onExportAction(action: ExportAction, id: string) {
    logUi("export", { id: shortId(id), action });
    switch (action) {
      case "export":
        void exportMeeting(id);
        break;
      case "copy":
        void copyMeeting(id);
        break;
      case "reveal":
        void revealMeeting(id);
        break;
    }
  }
  async function toggleMeetingRecording() {
    // §2 ways out: Stop asks first only under 10 s (a mis-click) or over 3 h.
    if (meetingStatus.recording && stopNeedsConfirm(meetingStatus.elapsedSeconds) && !confirmStop) {
      setConfirmStop(true);
      return;
    }
    const wasRecording = meetingStatus.recording;
    const recordingId = meetingStatus.id ?? null;
    setMeetingBusy(true);
    try {
      const next = await invoke<MeetingStatus>("toggle_meeting_recording", {
        // Ignored by the backend on a press that STOPS a meeting: the recording
        // that is ending was started under whatever it was started under.
        kind: meetingKind,
      });
      setMeetingStatus(next);
      logUi(next.recording ? "record" : "stop", next.recording ? { kind: meetingKind } : {});
      if (!next.recording) {
        await loadMeetings(meetingQuery);
        // The hero becomes the meeting's detail once it stops (§2).
        if (wasRecording && recordingId) void openMeetingDetail(recordingId);
      }
    } catch (e) {
      logUi(wasRecording ? "stop_failed" : "record_failed");
      toastError(e);
    } finally {
      setConfirmStop(false);
      setMeetingBusy(false);
    }
  }
  async function renameMeeting(id: string, title: string) {
    const next = title.trim();
    if (!next || next === openMeeting?.meeting.title) return;
    try {
      await invoke("rename_meeting", { id, title: next });
      logUi("rename", { id: shortId(id) });
      setOpenMeeting((m) =>
        m?.meeting.id === id
          ? { ...m, meeting: { ...m.meeting, title: next } }
          : m,
      );
      await loadMeetings(meetingQuery);
    } catch (e) {
      logUi("rename_failed", { id: shortId(id) });
      toastError(e, "Couldn't rename this meeting. Try again.");
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
  const loading = state === "loading" || !insights;

  const kindPicker = meetingKinds.length > 0 && !meetingStatus.recording && (
    // YV125 — the start-of-meeting kind picker, beside Record. "Not sure" is a
    // real answer; the choice decides whether diarization clusters the mic.
    <div className="meeting-kind-picker" role="radiogroup" aria-label="What kind of meeting is this?">
      {meetingKinds.map((c) => (
        <button
          key={c.kind}
          type="button"
          role="radio"
          aria-checked={meetingKind === c.kind}
          className={meetingKind === c.kind ? "chip selected" : "chip"}
          onClick={() => setMeetingKind(c.kind)}
        >
          {c.label}
        </button>
      ))}
    </div>
  );

  const hero = meetingStatus.recording && (
    <RecordingHero
      title={meetingStatus.title || ""}
      clock={elapsedLabel(meetingStatus)}
      elapsedSeconds={meetingStatus.elapsedSeconds}
      note={systemAudioBadge(meetingStatus)}
      busy={meetingBusy}
      stopLabel={confirmStop ? "Stop now? Press again" : "Stop meeting"}
      onStop={() => void toggleMeetingRecording()}
      callAudio={
        callAudio && (
          <p className="tiny rec-call-audio" data-call-audio={callAudio.state}>
            <span className={`call-audio-chip is-${callAudio.state}`}>{callAudio.label}</span>
            <span>{callAudio.detail}</span>
            {callAudio.action === "turn-on" && (
              <button type="button" className="ghost" disabled={sysAudioBusy} onClick={() => void runSystemAudioSetup()}>
                {CALL_AUDIO_ACTION_LABEL}
              </button>
            )}
          </p>
        )
      }
      live={<LiveTranscript recording={meetingStatus.recording} />}
    />
  );

  // ── Empty: no meetings at all and no search (§3.3 Empty) ──────────────
  if (!loading && meetings.length === 0 && !meetingQuery) {
    return (
      <div className="meetings-view">
        <NotetakerSetupCard onImported={() => void loadMeetings(meetingQuery)} />
        {hero}
        <div className="empty" data-empty-state="meetings">
          {/* Y5-B — Meetings gets its OWN drawing. */}
          <div className="view-state-glyph">
            <RoomGlyph />
          </div>
          <h3>No meetings yet</h3>
          <p>
            Record one, or Yap will offer when a call starts. Audio is kept for{" "}
            {/* 0 is "no meetings yet" in Insights, never a real policy. */}
            {insights?.meetings?.audioRetentionDays || 7} days; the transcript is kept for good.
          </p>
          {!meetingStatus.recording && (
            <>
              {kindPicker}
              <button
                className="primary big"
                disabled={meetingBusy || !!disabledReason(meetingStatus)}
                title={disabledReason(meetingStatus) || undefined}
                onClick={() => void toggleMeetingRecording()}
              >
                {recordLabel(meetingStatus)}
              </button>
              <p className="tiny">
                {disabledReason(meetingStatus) ??
                  `Or press ⌃⌘M from anywhere, or pick “Record a meeting” in the menu bar. Everything stays on this Mac.`}
              </p>
            </>
          )}
          <SpeechEngineNotice setup={modelSetup} />
        </div>
        <UndoDeleteBar pending={pending} now={now} onUndo={undoDelete} />
      </div>
    );
  }

  const paneMode = narrow ? (openMeeting ? "detail" : "list") : "split";
  // The Notes tab's one action when there are no notes: write them (the same
  // pure decision the row uses), or point at the notes model on a finished
  // meeting that has nothing to write them with.
  const notesTabAction: "notes" | "model" | null = !openMeeting
    ? null
    : meetingActions(openMeeting.meeting, progress[openMeeting.meeting.id], false).includes("notes")
      ? "notes"
      : openMeeting.meeting.state === "complete"
        ? "model"
        : null;

  // yap-1106-auto-label-and-correct — a speaker chip's correction: the backend
  // decides which whole turns move and locks them; the lines change in place,
  // and the returned function is the 10 s Undo (which asks the lock).
  const mergeLabelRows = (meetingId: string, rows: LabelRow[]) =>
    setOpenMeeting((m) =>
      m && m.meeting.id === meetingId
        ? {
            ...m,
            segments: applyLabelRows(m.segments, rows),
            echoSegments: m.echoSegments ? applyLabelRows(m.echoSegments, rows) : m.echoSegments,
          }
        : m,
    );
  const correctSpeakers = async (
    meetingId: string,
    correction: SpeakerCorrection,
  ): Promise<(() => Promise<void>) | null> => {
    try {
      const applied = await invoke<CorrectionApplied>("correct_meeting_speakers", {
        meetingId,
        correction,
      });
      mergeLabelRows(meetingId, applied.rows);
      return async () => {
        try {
          const undone = await invoke<CorrectionUndone>("undo_speaker_correction", {
            meetingId,
            undo: applied.undo,
          });
          mergeLabelRows(meetingId, undone.rows);
          const kept = undone.skippedChanged.length;
          if (kept > 0) {
            toast(`Undone. ${kept} ${kept === 1 ? "line keeps" : "lines keep"} a later correction.`);
          }
        } catch (e) {
          toast(`Couldn't undo that: ${String(e)}`);
        }
      };
    } catch (e) {
      toast(`Couldn't change the speaker: ${String(e)}`);
      return null;
    }
  };
  return (
    <div className="meetings-view" data-meetings-pane={paneMode}>
      {/* yap-notetaker-onboard — the Set up Notetaker card. */}
      <NotetakerSetupCard onImported={() => void loadMeetings(meetingQuery)} />
      {/* YV95 + §3.3 — the recording hero, pinned above both panes. */}
      {hero}
      <div className="meetings-panes">
        <aside className="meetings-list-pane" aria-label="Meetings list">
          <div className="toolbar">
            {!meetingStatus.recording && (
              <button
                className="primary"
                disabled={meetingBusy || !!disabledReason(meetingStatus)}
                title={disabledReason(meetingStatus) || undefined}
                onClick={() => void toggleMeetingRecording()}
              >
                Record a meeting
              </button>
            )}
            {kindPicker}
          </div>
          <div className="meetings-find">
            <input
              ref={searchRef}
              type="search"
              value={meetingQuery}
              onChange={(e) => setMeetingQuery(e.target.value)}
              placeholder="Search titles and every word said (⌘F)"
              aria-label="Search meetings"
            />
            {meetingQuery && (
              <button className="ghost" onClick={() => setMeetingQuery("")}>
                Clear
              </button>
            )}
          </div>
          <MeetingFilters filter={filter} kinds={meetingKinds} people={people} onChange={setFilter} />
          {/* The needs-model state: one click installs the speech engine. */}
          <SpeechEngineNotice setup={modelSetup} />
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
          {loading ? (
            <ListSkeleton />
          ) : groups.length === 0 ? (
            <div className="empty" data-empty-state="meetings:filtered">
              <h3>No meetings match that</h3>
              <p>Search covers meeting titles and every word transcribed inside them.</p>
              <div className="actions">
                {meetingQuery && <button className="ghost" onClick={() => setMeetingQuery("")}>Clear search</button>}
                {filterActive(filter) && <button className="ghost" onClick={() => setFilter(NO_FILTER)}>Clear filters</button>}
              </div>
            </div>
          ) : (
            <div className="meetings-groups" data-meetings-list ref={titlesRef}>
              {groups.map((g) => (
                <section key={g.id} className="meetings-group" aria-label={g.label}>
                  <h4 className="meetings-group-label">{g.label}</h4>
                  <ul className="meetings-list">
                    {g.meetings.map((m) => {
                      const tp = transcribeProgress(m, progress[m.id]);
                      const tpPct = tp ? transcribePercent(tp) : null;
                      const stateLabel = tp
                        ? `Transcribing ${tpPct}%`
                        : meetingStateLabel(m.state, progress[m.id], Boolean(m.summary));
                      // yap24-NT6: one pure decision for every button.
                      const actions = meetingActions(m, progress[m.id], Boolean(m.summary));
                      const isOpen = openId === m.id;
                      const isSelected = selected === m.id;
                      const hasStatusActions =
                        actions.includes("retry") || actions.includes("cancel-notes") || actions.includes("notes");
                      return (
                        <li
                          key={m.id}
                          className={`meeting-row${isOpen ? " open" : ""}${isSelected ? " selected" : ""}${hasStatusActions ? " has-status" : ""}`}
                          data-meeting-id={m.id}
                          data-meeting-state={m.state}
                          onContextMenu={(e) => openRowMenu(e, m.id, m.title)}
                        >
                          {/* yap-1107-meetings-row-whole-click-target — this button IS the
                              card: it fills the li edge to edge (the padding lives inside
                              it), so a press anywhere on the row opens the meeting. */}
                          {/* yap-1107-meetings-row-chips-baseline — §3.3 row anatomy, two
                              lines, every row the same height: line 1 = title (ellipsis) |
                              time · duration (right); line 2 = the people chips on ONE
                              line (overflow folds into "+N", never wraps) then the state
                              chip, the same box as a chip so the line shares one baseline.
                              The notes' first line and the failure text moved off the row
                              (the detail beside it shows the notes; the failure reason is the
                              failed chip's tooltip — the only row tooltip besides a cut title,
                              see yap-1107-meetings-row-no-stale-tooltip): a third line on some rows made the list ragged. A
                              transcribing row's real progress fills its own state chip. */}
                          <button
                            type="button"
                            className="meeting-row-main"
                            aria-current={isOpen ? "true" : undefined}
                            tabIndex={m.id === tabStopId ? 0 : -1}
                            onFocus={() => setSelected(m.id)}
                            onClick={(e) => openRow(m.id, e.detail === 0 ? "keyboard" : "pointer")}
                          >
                            <span className="meeting-row-line1">
                              <span className="meeting-row-title">{m.title}</span>
                              <span className="meeting-row-when">
                                {formatTime(m.startedAt)} · {formatMeetingDuration(m.durationSeconds)}
                              </span>
                            </span>
                            <span className="meeting-row-chips" data-meeting-row-chips>
                              <PeopleChips people={listPeople(m)} fit />
                              {tp && tpPct !== null ? (
                                <span
                                  className={`meeting-state ${m.state}`}
                                  data-meeting-progress
                                  role="progressbar"
                                  aria-label={`${m.title} transcription`}
                                  aria-valuenow={tpPct}
                                  aria-valuemin={0}
                                  aria-valuemax={100}
                                  aria-valuetext={transcribedLabel(tp)}
                                  style={{ "--meeting-progress": `${tpPct}%` } as CSSProperties}
                                >
                                  {stateLabel}
                                </span>
                              ) : (
                                // yap-1107-meetings-row-no-stale-tooltip — the one title a row keeps
                                // besides a cut title: a FAILED row's reason, on its small state chip
                                // (the detail pane does not show m.error). "Complete" / "Notes ready"
                                // repeat the chip's own text, so those rows get none.
                                <span className={`meeting-state ${m.state}`} title={m.error || undefined}>
                                  {stateLabel}
                                </span>
                              )}
                            </span>
                          </button>
                          {/* yap-1107-meetings-row-actions-no-overlay — the row keeps no
                              hover bar: Export and Delete live in the detail header, on
                              ⌫ for the selected row, and in the right-click menu. Only
                              status buttons stay, in a column that is laid out in every
                              state (hover changes colour, never layout). */}
                          {hasStatusActions && (
                            <div className="meeting-row-status" data-meeting-row-status ref={reserveStatusWidth}>
                              {actions.includes("retry") && (
                                <button onClick={() => void retryMeeting(m.id)}>Retry</button>
                              )}
                              {actions.includes("cancel-notes") && (
                                <button className="ghost" onClick={() => void notesAction(cancelNotes, m.id)}>
                                  Cancel notes
                                </button>
                              )}
                              {actions.includes("notes") && (
                                <button onClick={() => void notesAction(retryNotes, m.id)}>
                                  {actions.includes("retry") && retryNotesLabel(progress[m.id]) === "Retry"
                                    ? "Retry notes"
                                    : retryNotesLabel(progress[m.id])}
                                </button>
                              )}
                            </div>
                          )}
                        </li>
                      );
                    })}
                  </ul>
                </section>
              ))}
            </div>
          )}
        </aside>

        <section className="meetings-detail-pane" aria-label="Meeting detail">
          {!openMeeting ? (
            loading ? (
              <div className="meeting-detail is-skeleton" aria-hidden data-loading-state="meetings:detail">
                <span className="skeleton-line wide" />
                <span className="skeleton-line" />
                <span className="skeleton-line" />
              </div>
            ) : (
              <PickAMeeting count={visible.length} />
            )
          ) : (
            <article className="meeting-detail" aria-label={openMeeting.meeting.title}>
              {/* §9 — the "Who's talking?" strip sits at the top of the detail. */}
              <WhoIsTalkingStrip unnamed={unnamedVoices(detailPeopleRows)} onOpenPeople={() => setTab("people")} />
              {/* yap-1106-fb-22b — Stop opens the meeting here while the summary is
                  still being written; the cards can be answered meanwhile. */}
              {(NOTES_ACTIVE as readonly string[]).includes(progress[openMeeting.meeting.id]?.state ?? "") && (
                <p className="who-summary-writing tiny muted" role="status" aria-live="polite">
                  <YappySprite size={24} className="who-summary-yappy" />
                  Writing the summary…
                </p>
              )}
              {/* yap-1106-who-is-this — "Who was this?" cards, only after the meeting. */}
              <WhoIsTalking
                meetingId={openMeeting.meeting.id}
                title={openMeeting.meeting.title}
                labelFor={(track, cluster) =>
                  clusterLabel(openMeeting.segments, openMeeting.meeting.kind, track, cluster)
                }
                audioOnDisk={openMeeting.audioOnDisk}
                retentionDays={insights?.meetings?.audioRetentionDays ?? 7}
                recording={Boolean(meetingStatus.recording && meetingStatus.id === openMeeting.meeting.id)}
                onNames={(names) =>
                  setOpenMeeting((m) =>
                    m && m.meeting.id === openMeeting.meeting.id
                      ? { ...m, segments: withSpeakerNames(m.segments, names) }
                      : m,
                  )
                }
                toast={toast}
                voiceCount={
                  new Set(
                    openMeeting.segments
                      .filter((s) => s.clusterIndex !== null && s.clusterIndex !== undefined)
                      .map((s) => `${s.track}:${s.clusterIndex}`),
                  ).size || null
                }
                onLabelRows={(rows) => mergeLabelRows(openMeeting.meeting.id, rows)}
              />
              <header className="meeting-detail-head">
                <div className="meeting-detail-bar">
                  {narrow ? (
                    <button className="ghost" onClick={closeDetail}>
                      ← All meetings
                    </button>
                  ) : (
                    <span />
                  )}
                  <div className="actions">
                    {/* yap24-NT6: Retry only when the WAV is still on disk. */}
                    {canRetryTranscript(
                      { ...openMeeting.meeting, audioKept: openMeeting.meeting.audioKept && openMeeting.audioOnDisk },
                      progress[openMeeting.meeting.id],
                    ) && (
                      <button
                        onClick={async () => {
                          await retryMeeting(openMeeting.meeting.id);
                          await openMeetingDetail(openMeeting.meeting.id);
                        }}
                      >
                        Retry
                      </button>
                    )}
                    <ExportMenu meetingId={openMeeting.meeting.id} disabled={meetingBusy} primary onAction={onExportAction} />
                    <button
                      className="ghost danger"
                      disabled={meetingBusy}
                      onClick={() => requestDelete(openMeeting.meeting.id, openMeeting.meeting.title)}
                    >
                      Delete meeting
                    </button>
                    {!narrow && (
                      <button className="ghost" aria-label="Close meeting" title="Close (Escape)" onClick={closeDetail}>
                        Close
                      </button>
                    )}
                  </div>
                </div>
                {/* Editable in place; Return saves, Escape puts the old title back. */}
                <input
                  className="meeting-title-edit"
                  defaultValue={openMeeting.meeting.title}
                  aria-label="Meeting title"
                  key={openMeeting.meeting.id}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") e.currentTarget.blur();
                    if (e.key === "Escape") e.currentTarget.value = openMeeting.meeting.title;
                  }}
                  onBlur={(e) => void renameMeeting(openMeeting.meeting.id, e.target.value)}
                />
                <p className="card-meta">
                  <span>
                    {formatTime(openMeeting.meeting.startedAt)} ·{" "}
                    {formatMeetingDuration(openMeeting.meeting.durationSeconds)} ·{" "}
                    {meetingStateLabel(
                      openMeeting.meeting.state,
                      progress[openMeeting.meeting.id],
                      Boolean(openMeeting.meeting.summary),
                    )}
                  </span>
                  {/* yap-1106-call-audio-truth — the tracks chip. */}
                  <span
                    className="meeting-tracks"
                    data-meeting-tracks={recordedTracks(openMeeting.meeting).twoTrack ? "two" : "mic"}
                  >
                    {recordedTracks(openMeeting.meeting).label}
                  </span>
                  <span>
                    {openMeeting.audioOnDisk
                      ? "audio kept"
                      : `audio deleted after ${insights?.meetings?.audioRetentionDays ?? 7} days`}
                  </span>
                </p>
                {recordedTracks(openMeeting.meeting).detail && (
                  <p className="tiny meeting-tracks-why">{recordedTracks(openMeeting.meeting).detail}</p>
                )}
                <PeopleChips people={detailPeopleRows.map((p) => p.name)} />
              </header>

              <DetailTabs
                tab={tab}
                counts={{ transcript: openMeeting.segments.length, people: detailPeopleRows.length }}
                onTab={(t) => {
                  setTab(t);
                  logUi("tab", { tab: t });
                }}
              />
              <div
                className="meeting-tab-panel"
                role="tabpanel"
                id={`meeting-panel-${tab}`}
                aria-labelledby={`meeting-tab-${tab}`}
              >
                {tab === "notes" &&
                  (openMeeting.meeting.summary ? (
                    <NotesView markdown={openMeeting.meeting.summary} meetingId={openMeeting.meeting.id} />
                  ) : (
                    <StreamingNotes meetingId={openMeeting.meeting.id}>
                      <div className="empty" data-empty-state="meetings:notes">
                        <h3>{notesLabel(progress[openMeeting.meeting.id]) ?? "No notes yet"}</h3>
                        <p>Notes are written on this Mac once the transcript is ready.</p>
                        <div className="actions">
                          {notesTabAction === "notes" ? (
                            <button
                              className="primary"
                              onClick={() => void notesAction(retryNotes, openMeeting.meeting.id)}
                            >
                              {retryNotesLabel(progress[openMeeting.meeting.id])}
                            </button>
                          ) : notesTabAction === "model" ? (
                            <button type="button" className="ghost" onClick={openModelSettings}>
                              Add a notes model
                            </button>
                          ) : null}
                        </div>
                      </div>
                    </StreamingNotes>
                  ))}
                {tab === "transcript" &&
                  (openMeeting.segments.length === 0 ? (
                    <div className="empty" data-empty-state="meetings:segments">
                      <h3>No transcript yet</h3>
                      <p>
                        {openMeeting.meeting.state === "recording" ||
                        openMeeting.meeting.state === "transcribing"
                          ? "Yap is still working through the audio. This page does not refresh itself."
                          : openMeeting.meeting.state === "waiting_for_model"
                            ? "No speech model is downloaded yet. Download one in Settings and Yap transcribes this meeting automatically. The audio is kept until then."
                            : openMeeting.audioOnDisk
                              ? `Nothing was transcribed from this recording. The audio is still on disk for ${
                                  insights?.meetings?.audioRetentionDays ?? 7
                                } days.`
                              : "Nothing was transcribed from this recording, and its audio is gone, so it can't be transcribed again."}
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
                      hidden={openMeeting.echoSegments}
                      kind={openMeeting.meeting.kind}
                      onCorrect={(c) => correctSpeakers(openMeeting.meeting.id, c)}
                    />
                  ))}
                {tab === "people" && <PeopleTab people={detailPeopleRows} />}
              </div>
            </article>
          )}
        </section>
      </div>
      {rowMenu && (
        <FloatingMenu
          key={rowMenu.id + ":" + rowMenu.x + ":" + rowMenu.y}
          anchor={() => ({ left: rowMenu.x, right: rowMenu.x, top: rowMenu.y, bottom: rowMenu.y })}
          align="start"
          label={`Actions for ${rowMenu.title}`}
          name="row"
          meetingId={rowMenu.id}
          items={[
            ...exportItems(rowMenu.id, onExportAction, meetingBusy),
            {
              key: "delete",
              label: "Delete meeting",
              danger: true,
              disabled: meetingBusy,
              onSelect: () => requestDelete(rowMenu.id, rowMenu.title),
            },
          ]}
          onClose={() => setRowMenu(null)}
        />
      )}
      <UndoDeleteBar pending={pending} now={now} onUndo={undoDelete} />
    </div>
  );
}
