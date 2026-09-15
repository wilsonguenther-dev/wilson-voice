import { invoke } from "@tauri-apps/api/core";
import YappyHouse from "../home/YappyHouse";
import DiffView from "../DiffView";
import { type TranscriptEntry, undoAiEditText, type DictCandidate, formatTime } from "../appTypes";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { EmptyState, ErrorState, LoadingState, WaveGlyph } from "../ViewStates";

export default function Home() {
  const {
    bootError, booting, clearAll, clearing, copyText, diffId, openModelSettings, refreshAll,
    failed, feedbackEdits, fixDraft, fixingId, history,
    insights, loadHistory, pasteText, query, queryRef, refreshInsights,
    retryFailed, retrying, setActiveNoteId, setCandidates, setDiffId, setFailed,
    setFeedbackEdits, setFixDraft, setFixingId, setHistory, setNav, setNoteBody,
    setNoteTitle, setQuery, setRetryId, settings, status, toast,
    toggleRecord,
  } = useAppCtx();
  // Y5-B — was `if (!insights) return null`: a heading over a blank page on
  // every fresh install, because insights is null until the first read lands.
  const state = viewState(history, booting || !insights, bootError);
  async function removeEntry(id: string) {
    try {
      await invoke("delete_entry", { id });
      setHistory((h) => h.filter((e) => e.id !== id));
    } catch (e) {
      toast(String(e));
      return;
    }
    await refreshInsights();
  }
  async function discardFailed(id: string) {
    try {
      await invoke("discard_failed_dictation", { id });
      setFailed((f) => f.filter((x) => x.id !== id));
      setRetryId((cur) => (cur === id ? null : cur));
    } catch (e) {
      toast(String(e));
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
      toast(String(e));
    }
  }
  if (state === "error")
    return (
      <ErrorState
        data-error-state="home"
        view="home"
        error={bootError}
        actionLabel="Reload history"
        onAction={() => void refreshAll()}
      />
    );
  if (state === "loading")
    return <LoadingState data-loading-state="home" noun="history" expected={insights?.totalSessions ?? null} onRetry={() => void refreshAll()} />;

  // Y5-B — the first of the two cases the item names. `status.modelReady` used
  // to gate a warning STRIP over an otherwise normal view: a record button that
  // cannot record, a search box over a history that cannot grow. Before the
  // model exists there is exactly one thing to do, so this is the whole view.
  if (!status.modelReady)
    return (
      <EmptyState
        data-empty-state="home:model"
        glyph={<WaveGlyph />}
        title="Yap needs a speech model before it can listen"
        body="The model runs on this Mac and never leaves it. Pick one and the download starts; it is the last setup step."
        actionLabel="Choose a speech model"
        onAction={openModelSettings}
      />
    );

  return (
            <>
              <YappyHouse
                wordsToday={insights?.wordsToday}
                streakDays={insights?.streakDays}
                companionTone={settings?.companionTone}
              />

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
                <input
                  type="search"
                  placeholder="Search transcripts (SQLite FTS5)…"
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
                    <h3>Failed dictations</h3>
                    <span className="tiny">
                      Audio kept for 7 days — retry when the engine is ready,
                      nothing was lost.
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
                        <div className="actions">
                          <button
                            className="primary"
                            disabled={retrying === f.id}
                            onClick={() => retryFailed(f.id)}
                          >
                            {retrying === f.id ? "Retrying…" : "Retry"}
                          </button>
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

              {history.length === 0 ? (
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
            </>
  );
}
