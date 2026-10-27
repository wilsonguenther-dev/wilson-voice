import BootNote from "../BootNote";
import { invoke } from "@tauri-apps/api/core";
import YappyHouse from "../home/YappyHouse";
import DiffView from "../DiffView";
import { type TranscriptEntry, undoAiEditText, type DictCandidate, formatTime, missingDictationGrants } from "../appTypes";
import { useAppCtx } from "../appShell";
import { nameProduct } from "../permission";
import { viewState } from "../viewState";
import { EmptyState, ErrorState, LoadingState, WaveGlyph } from "../ViewStates";

export default function Home() {
  const {
    bootError, bootErrors, booting, retryBoot, clearAll, clearing, copyText, diffId, refreshAll,
    failed, feedbackEdits, fixDraft, fixingId, history,
    insights, loadHistory, loadOlderHistory, historyHasMore, loadingOlder, pasteText, perms, query, queryRef, refreshInsights,
    retryFailed, retrying, setActiveNoteId, setCandidates, setDiffId, setFailed,
    setFeedbackEdits, setFixDraft, setFixingId, setHistory, setNav, setNoteBody,
    setNoteTitle, setQuery, dropRetry, setScratchComposing, settings, status, toast, toastError,
    toggleRecord,
  } = useAppCtx();
  // Y5-B — was `if (!insights) return null`: a heading over a blank page on
  // every fresh install, because insights is null until the first read lands.
  // Y5-B fix round (#192 BLOCKING) - bootError is shared by refreshAll's 11
  // calls plus loadHistory, so it only replaces THIS view when this view's own
  // data never landed. A later refresh that fails (model-ready edge,
  // silent_capture) keeps already-loaded data on screen; the bridge message
  // is in status, the same as on main.
  // DO 3 — history and stats are this view's data; each names its own error.
  const homeError =
    bootErrors?.get_history ?? bootErrors?.get_insights ?? (insights == null ? bootError : null);
  const state = viewState(history, booting || !insights, homeError);
  // H2 — nothing dictated yet, no search typed, and a dictation GRANT
  // (Microphone / Accessibility) missing. A missing speech model is NOT a
  // grant — ModelRibbon owns that case — so this never reads allCriticalOk.
  const missingGrants = missingDictationGrants(perms);
  const permsGate = history.length === 0 && !query && missingGrants.length > 0;
  async function removeEntry(id: string) {
    try {
      await invoke("delete_entry", { id });
      setHistory((h) => h.filter((e) => e.id !== id));
    } catch (e) {
      toastError(e);
      return;
    }
    await refreshInsights();
  }
  async function discardFailed(id: string) {
    try {
      await invoke("discard_failed_dictation", { id });
      setFailed((f) => f.filter((x) => x.id !== id));
      dropRetry(id);
    } catch (e) {
      toastError(e);
    }
  }
  function feedbackFor(e: TranscriptEntry): number | null {
    return e.id in feedbackEdits ? feedbackEdits[e.id] : (e.feedback ?? null);
  }
  function rateTake(id: string, value: number | null) {
    setFeedbackEdits((prev) => ({ ...prev, [id]: value }));
    invoke("set_take_feedback", { id, feedback: value }).catch((err) => {
      // A failed write costs a future evaluation signal and costs this take
      // nothing, so it must never surface as an error toast over the user's
      // transcript. Roll the button back so it does not lie.
      console.warn("set_take_feedback failed", err);
      setFeedbackEdits((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
    });
  }
  function startFix(e: TranscriptEntry) {
    setFixingId(e.id);
    setFixDraft(e.text);
  }
  async function saveFix() {
    if (!fixingId || !fixDraft.trim()) return;
    try {
      const learned = await invoke<number>("correct_transcript", {
        id: fixingId,
        text: fixDraft.trim(),
      });
      setFixingId(null);
      const [cands] = await Promise.all([
        invoke<DictCandidate[]>("list_dict_candidates"),
        loadHistory(queryRef.current),
      ]);
      setCandidates(cands);
      toast(
        learned > 0
          ? `Fixed — ${learned} term${learned === 1 ? "" : "s"} to review in Dictionary`
          : "Transcript fixed",
      );
    } catch (e) {
      toastError(e);
    }
  }
  if (state === "error")
    return (
      <ErrorState
        data-error-state="home"
        view="home"
        error={homeError}
        actionLabel="Reload history"
        onAction={() => void retryBoot(["get_history", "get_insights"])}
      />
    );
  if (state === "loading")
    return <LoadingState data-loading-state="home" noun="history" expected={insights?.totalSessions ?? null} onRetry={() => void refreshAll()} />;

  // Y5-B review fix — no model gate here. YV54 (#68): the recommended model
  // downloads on its own once onboarding finishes and the slim <ModelRibbon>
  // in App.tsx is the only surface; replacing Home would contradict that copy
  // and hide history, search and failed-take recovery from any install whose
  // selected model file is missing.
  return (
            <>
              <BootNote keys={["get_status", "list_failed_dictations", "list_crash_events"]} />
              <YappyHouse
                wordsToday={insights?.wordsToday}
                streakDays={insights?.streakDays}
                companionTone={settings?.companionTone}
              />

              {/* yap-uiux-2026-10-16-H2 — with Microphone or Accessibility
                  ungranted, "hold fn" cannot work, so the empty Home's first
                  card and its one action is the thing that unblocks it: the
                  Permissions view. It sits ABOVE the record button, so the
                  hotkey hint reads as the step that follows, not the first. */}
              {permsGate && (
                <EmptyState
                  data-empty-state="home:history"
                  glyph={<WaveGlyph />}
                  title="Grant permissions to start dictating"
                  body={nameProduct(`Yap needs ${missingGrants.join(" and ")} before it can hear you and type for you. Grant ${missingGrants.length > 1 ? "them" : "it"}, then hold fn⌃ and talk.`, perms?.productName)}
                  actionLabel="Grant permissions"
                  onAction={() => setNav("permissions")}
                />
              )}

              <button
                className={
                  status.recording
                    ? "record-btn live"
                    : status.busy
                      ? "record-btn busy"
                      : "record-btn"
                }
                onClick={toggleRecord}
                disabled={status.busy}
              >
                <span className="mic" aria-hidden>
                  {status.recording ? "■" : status.busy ? "…" : "●"}
                </span>
                <span>
                  {status.recording
                    ? "Listening — release fn⌃ or tap to stop hands-free"
                    : status.busy
                      ? "Transcribing…"
                      : "Hold fn⌃ · double-tap hands-free"}
                </span>
              </button>

              {/* Y3-D — the button that did not exist. The record button above
                  is DISABLED while `busy`, so from the moment the hold ended
                  until the decode finished there was nothing on screen to
                  press: a fifteen-minute take the user regretted the instant
                  they let go ran to completion anyway. This is live for BOTH
                  halves of a take (recording and decoding), because the user
                  pressing "stop this" does not know or care which stage it is
                  in. The audio is parked in recovery, not deleted — History
                  offers it back. */}
              {(status.recording || status.busy) && (
                <button
                  className="ghost cancel-take"
                  onClick={() => {
                    void invoke("cancel_transcription");
                  }}
                >
                  Cancel this take (esc)
                </button>
              )}

              <div className="toolbar">
                {/* yap-uiux-2026-10-16-H4 — the placeholder named the index
                    ("SQLite FTS5"), which is how it works, not what it does. */}
                <input
                  type="search"
                  aria-label="Search your dictations"
                  placeholder="Search everything you've said…"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
                <button className="ghost" onClick={clearAll} disabled={clearing}>
                  {clearing ? "Clearing…" : "Clear"}
                </button>
              </div>

              {/* YV52 — takes whose transcription failed. The audio is still on
                  disk, so nothing was lost: retry re-runs ASR on that same clip,
                  discard throws the row and its audio away. Clips are purged
                  automatically after 7 days. */}
              {failed.length > 0 && (
                <section className="failed-takes">
                  <div className="failed-head">
                    <h3>Failed and unfinished dictations</h3>
                    <span className="tiny">
                      Audio kept for 7 days — finish or retry when the engine is
                      ready, nothing was lost. Nothing here is pasted for you.
                    </span>
                  </div>
                  <ul className="feed">
                    {failed.map((f) => (
                      <li key={f.id} className="card failed">
                        <div className="card-meta">
                          <span>
                            {formatTime(f.createdAt)}
                            {f.sourceApp ? ` · ${f.sourceApp}` : ""}
                          </span>
                          <span>
                            {f.speechSeconds > 0
                              ? `${f.speechSeconds.toFixed(1)}s of audio`
                              : "audio saved"}
                          </span>
                        </div>
                        <p className="failed-why">{f.error}</p>
                        {f.partialText ? (
                          <p className="failed-partial" title={f.partialText}>
                            Already transcribed: “{f.partialText}”
                          </p>
                        ) : null}
                        {f.attemptText && f.attemptText !== f.partialText ? (
                          <p className="failed-partial" title={f.attemptText}>
                            Transcribed so far: “{f.attemptText}”
                          </p>
                        ) : null}
                        <div className="actions">
                          <button
                            className="primary"
                            disabled={retrying === f.id}
                            onClick={() => retryFailed(f.id)}
                          >
                            {retrying === f.id
                              ? f.partialText
                                ? "Finishing…"
                                : "Retrying…"
                              : f.partialText
                                ? "Finish transcribing"
                                : "Retry"}
                          </button>
                          {f.attemptText ? (
                            <button
                              className="ghost"
                              disabled={retrying === f.id}
                              onClick={() => {
                                if (
                                  confirm(
                                    f.partialText
                                      ? "Save the words shown for this take — the words recovered earlier plus everything transcribed since — and delete its audio? Parts that didn't transcribe can't be recovered after this."
                                      : "Save the words shown for this take and delete its audio? Parts that didn't transcribe can't be recovered after this.",
                                  )
                                )
                                  void retryFailed(f.id, true);
                              }}
                            >
                              Keep what was transcribed
                            </button>
                          ) : null}
                          <button
                            className="ghost danger"
                            onClick={() => discardFailed(f.id)}
                          >
                            Discard
                          </button>
                        </div>
                      </li>
                    ))}
                  </ul>
                </section>
              )}

              {permsGate ? null : history.length === 0 ? (
                <EmptyState
                  data-empty-state="home:history"
                  glyph={<WaveGlyph />}
                  title={query ? "Nothing matches that" : "No dictations yet"}
                  body={
                    query
                      ? "Search reads every word you have ever dictated. Try a shorter phrase."
                      : "Hold fn and say something. The text lands here, stored on this Mac and searchable forever."
                  }
                  actionLabel={query ? "Clear the search" : "Hold fn and say something"}
                  onAction={query ? () => setQuery("") : toggleRecord}
                />
              ) : (
                <ul className="feed">
                  {history.map((e) => (
                    <li key={e.id} className="card">
                      <div className="card-meta">
                        <span>
                          {formatTime(e.createdAt)}
                          {e.sourceApp ? ` · ${e.sourceApp}` : ""}
                        </span>
                        <span>
                          {e.wordCount} words · {e.backend}
                          {(e.speechSeconds ?? 0) > 0
                            ? ` · ${e.speechSeconds!.toFixed(1)}s speech`
                            : ""}
                          {` · ${e.asrSeconds.toFixed(1)}s asr`}
                          {(e.pipelineMs ?? 0) > 0
                            ? ` · ${e.pipelineMs}ms hold→clip`
                            : ""}
                        </span>
                      </div>
                      {fixingId === e.id ? (
                        <div className="fix-edit">
                          <textarea
                            value={fixDraft}
                            onChange={(ev) => setFixDraft(ev.target.value)}
                            aria-label="Corrected transcript"
                            autoFocus
                          />
                          <p className="tiny">
                            Correct the words Yap got wrong — it learns them as
                            dictionary suggestions.
                          </p>
                          <div className="actions">
                            <button className="primary" onClick={saveFix}>
                              Save fix
                            </button>
                            <button
                              className="ghost"
                              onClick={() => setFixingId(null)}
                            >
                              Cancel
                            </button>
                          </div>
                        </div>
                      ) : (
                        <>
                          <p>{e.text}</p>
                          <div className="actions">
                            <button onClick={() => copyText(e.text)}>
                              Copy
                            </button>
                            <button
                              className="primary"
                              onClick={() => pasteText(e.text)}
                            >
                              Paste
                            </button>
                            {/* YV51: only offered when the cleanup pipeline
                                actually changed this take — otherwise "Paste
                                raw" would be a duplicate of Paste. */}
                            {undoAiEditText(e) && (
                              <button
                                className="ghost"
                                title="Paste exactly what you said, before auto-cleanup"
                                onClick={() =>
                                  pasteText(undoAiEditText(e) as string)
                                }
                              >
                                Paste raw
                              </button>
                            )}
                            {/* Y4-G: offered on every row that stored a raw
                                take, INCLUDING the ones the pipeline left
                                alone — "nothing changed" is the answer a user
                                who distrusts the formatting most needs. */}
                            {e.rawText != null && (
                              <button
                                className="ghost"
                                aria-expanded={diffId === e.id}
                                onClick={() =>
                                  setDiffId(diffId === e.id ? null : e.id)
                                }
                              >
                                {diffId === e.id
                                  ? "Hide changes"
                                  : "See what changed"}
                              </button>
                            )}
                            <button
                              className="ghost"
                              onClick={() => startFix(e)}
                            >
                              Fix transcription
                            </button>
                            <button
                              className="ghost"
                              onClick={() => {
                                setNav("scratchpad");
                                setScratchComposing(true);
                                setNoteTitle(`From ${formatTime(e.createdAt)}`);
                                setNoteBody(e.text);
                                setActiveNoteId(null);
                              }}
                            >
                              Scratchpad
                            </button>
                            <button
                              className="ghost danger"
                              onClick={() => removeEntry(e.id)}
                            >
                              Delete
                            </button>
                          </div>
                          {diffId === e.id && e.rawText != null && (
                            <DiffView
                              raw={e.rawText}
                              formatted={e.text}
                              stages={e.stagesThatRan}
                              polishSkipReason={e.polishSkipReason}
                              feedback={feedbackFor(e)}
                              onFeedback={(v) => rateTake(e.id, v)}
                              onUndo={
                                undoAiEditText(e)
                                  ? () => pasteText(undoAiEditText(e) as string)
                                  : undefined
                              }
                            />
                          )}
                        </>
                      )}
                    </li>
                  ))}
                </ul>
              )}
              {!permsGate && history.length > 0 && historyHasMore ? (
                <button
                  type="button"
                  className="ghost"
                  data-testid="history-show-older"
                  disabled={loadingOlder}
                  onClick={() => void loadOlderHistory()}
                >
                  {loadingOlder ? "Loading older takes…" : "Show older takes"}
                </button>
              ) : null}
            </>
  );
}
