/**
 * yap-1106-home-full-width-room (home-contained-scroll-footer) — History, the
 * full list of takes that used to grow under Home. Home now ends after the
 * last 5 takes with a "See all" row that opens this view. History pages 50 at
 * a time with Newer / Older buttons: nothing loads more as you scroll, so the
 * page always has an end (and the footer under it).
 *
 * Paging is keyset, the same cursor `get_history` already takes for DB-C
 * (`before: { createdAt, id }` of the last row shown): a page boundary inside a
 * run of identical timestamps neither repeats nor drops a take. Newer pops the
 * cursor stack, so going back is exact too.
 *
 * Every per-take action the old Home card had (Copy, Paste, Paste raw, See
 * what changed, Fix transcription, Scratchpad, Delete) lives here unchanged.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import DiffView from "../DiffView";
import { type TranscriptEntry, type DictCandidate, undoAiEditText, formatTime } from "../appTypes";
import { useAppCtx } from "../appShell";
import { EmptyState, ErrorState, LoadingState, WaveGlyph } from "../ViewStates";
import { HISTORY_PAGE, pageRangeLabel, takeHistoryFocus } from "../home/homeLayout";

type Cursor = { createdAt: string; id: string } | null;

export default function History() {
  const {
    copyText, diffId, feedbackEdits, fixDraft, fixingId, loadHistory, pasteText, query, queryRef,
    refreshInsights, setActiveNoteId, setCandidates, setDiffId, setFeedbackEdits, setFixDraft,
    setFixingId, setHistory, setNav, setNoteBody, setNoteTitle, setQuery, setScratchComposing,
    toast, toastError,
  } = useAppCtx();

  // cursors[i] is the `before` cursor that produced page i (page 0 = null).
  const [cursors, setCursors] = useState<Cursor[]>([null]);
  const [rows, setRows] = useState<TranscriptEntry[] | null>(null);
  const [hasOlder, setHasOlder] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(false);
  const [focusId] = useState<string | null>(() => takeHistoryFocus());
  const seq = useRef(0);
  const listTop = useRef<HTMLDivElement | null>(null);
  const page = cursors.length - 1;

  const loadPage = useCallback(
    async (before: Cursor, q: string, pageNo: number) => {
      const mine = ++seq.current;
      setLoading(true);
      const t0 = performance.now();
      try {
        // One extra row answers "is there an older page?" without a count query.
        const got = await invoke<TranscriptEntry[]>("get_history", {
          query: q || null,
          limit: HISTORY_PAGE + 1,
          ...(before ? { before } : {}),
        });
        if (mine !== seq.current) return;
        setRows(got.slice(0, HISTORY_PAGE));
        setHasOlder(got.length > HISTORY_PAGE);
        setError(null);
        console.info(
          `history page_loaded page=${pageNo} rows=${Math.min(got.length, HISTORY_PAGE)} search=${q ? 1 : 0} ms=${Math.round(performance.now() - t0)}`,
        );
      } catch (e) {
        if (mine !== seq.current) return;
        console.warn(`history page_failed search=${q ? 1 : 0}`);
        setError(e);
      } finally {
        if (mine === seq.current) setLoading(false);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  // A new search starts again from the newest page (debounced like Home's).
  useEffect(() => {
    const t = setTimeout(() => {
      setCursors([null]);
      void loadPage(null, query, 1);
    }, 180);
    return () => clearTimeout(t);
  }, [query, loadPage]);

  // "Open" from Home: bring that take into view once its page is on screen.
  useEffect(() => {
    if (!focusId || !rows) return;
    const el = document.querySelector(`[data-take-id="${CSS.escape(focusId)}"]`);
    if (el && "scrollIntoView" in el) (el as HTMLElement).scrollIntoView({ block: "center" });
  }, [focusId, rows]);

  function goOlder() {
    const last = rows?.[rows.length - 1];
    if (!last || loading) return;
    const c: Cursor = { createdAt: last.createdAt, id: last.id };
    setCursors((s) => [...s, c]);
    console.info(`history page_older page=${page + 2}`);
    void loadPage(c, queryRef.current, page + 2).then(() => listTop.current?.scrollIntoView({ block: "start" }));
  }
  function goNewer() {
    if (page === 0 || loading) return;
    const next = cursors.slice(0, -1);
    setCursors(next);
    console.info(`history page_newer page=${page}`);
    void loadPage(next[next.length - 1], queryRef.current, page).then(() => listTop.current?.scrollIntoView({ block: "start" }));
  }
  const reload = () => void loadPage(cursors[cursors.length - 1], queryRef.current, page + 1);

  async function removeEntry(id: string) {
    try {
      await invoke("delete_entry", { id });
      setRows((r) => (r ? r.filter((e) => e.id !== id) : r));
      setHistory((h) => h.filter((e) => e.id !== id));
      console.info("history take_deleted");
    } catch (e) {
      console.warn("history take_delete_failed");
      toastError(e);
      return;
    }
    await refreshInsights();
  }
  function feedbackFor(e: TranscriptEntry): number | null {
    return e.id in feedbackEdits ? feedbackEdits[e.id] : (e.feedback ?? null);
  }
  function rateTake(id: string, value: number | null) {
    setFeedbackEdits((prev) => ({ ...prev, [id]: value }));
    invoke("set_take_feedback", { id, feedback: value }).catch((err) => {
      // A failed write costs a future evaluation signal, never this take: roll
      // the button back so it does not lie, and say nothing over the transcript.
      console.warn("set_take_feedback failed", err);
      setFeedbackEdits((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
    });
  }
  async function saveFix() {
    if (!fixingId || !fixDraft.trim()) return;
    try {
      const learned = await invoke<number>("correct_transcript", { id: fixingId, text: fixDraft.trim() });
      setFixingId(null);
      const [cands] = await Promise.all([
        invoke<DictCandidate[]>("list_dict_candidates"),
        loadHistory(queryRef.current),
      ]);
      setCandidates(cands);
      reload();
      toast(learned > 0 ? `Fixed — ${learned} term${learned === 1 ? "" : "s"} to review in Dictionary` : "Transcript fixed");
    } catch (e) {
      toastError(e);
    }
  }

  if (error && !rows)
    return (
      <ErrorState
        data-error-state="history"
        view="home"
        error={error}
        actionLabel="Reload history"
        onAction={reload}
      />
    );
  if (!rows) return <LoadingState data-loading-state="history" noun="history" onRetry={reload} />;

  return (
    <div className="history-view" data-history-page={page + 1}>
      <div className="toolbar" ref={listTop}>
        <input
          type="search"
          aria-label="Search your dictations"
          placeholder="Search everything you've said…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button className="ghost" onClick={() => setNav("home")}>
          Back to Home
        </button>
      </div>

      {rows.length === 0 ? (
        <EmptyState
          data-empty-state="history"
          glyph={<WaveGlyph />}
          title={query ? "Nothing matches that" : "No dictations yet"}
          body={
            query
              ? "Search reads every word you have ever dictated. Try a shorter phrase."
              : "Hold fn and say something. Every take lands here, stored on this Mac."
          }
          actionLabel={query ? "Clear the search" : "Back to Home"}
          onAction={query ? () => setQuery("") : () => setNav("home")}
        />
      ) : (
        <ul className="feed" aria-busy={loading}>
          {rows.map((e) => (
            <li key={e.id} className={e.id === focusId ? "card focused" : "card"} data-take-id={e.id}>
              <div className="card-meta">
                <span>
                  {formatTime(e.createdAt)}
                  {e.sourceApp ? ` · ${e.sourceApp}` : ""}
                </span>
                <span>
                  {e.wordCount} words · {e.backend}
                  {(e.speechSeconds ?? 0) > 0 ? ` · ${e.speechSeconds!.toFixed(1)}s speech` : ""}
                  {` · ${e.asrSeconds.toFixed(1)}s asr`}
                  {(e.pipelineMs ?? 0) > 0 ? ` · ${e.pipelineMs}ms hold→clip` : ""}
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
                  <p className="tiny">Correct the words Yap got wrong — it learns them as dictionary suggestions.</p>
                  <div className="actions">
                    <button className="primary" onClick={saveFix}>
                      Save fix
                    </button>
                    <button className="ghost" onClick={() => setFixingId(null)}>
                      Cancel
                    </button>
                  </div>
                </div>
              ) : (
                <>
                  <p>{e.text}</p>
                  <div className="actions">
                    <button onClick={() => copyText(e.text)}>Copy</button>
                    <button className="primary" onClick={() => pasteText(e.text)}>
                      Paste
                    </button>
                    {undoAiEditText(e) && (
                      <button
                        className="ghost"
                        title="Paste exactly what you said, before auto-cleanup"
                        onClick={() => pasteText(undoAiEditText(e) as string)}
                      >
                        Paste raw
                      </button>
                    )}
                    {e.rawText != null && (
                      <button
                        className="ghost"
                        aria-expanded={diffId === e.id}
                        onClick={() => setDiffId(diffId === e.id ? null : e.id)}
                      >
                        {diffId === e.id ? "Hide changes" : "See what changed"}
                      </button>
                    )}
                    <button
                      className="ghost"
                      onClick={() => {
                        setFixingId(e.id);
                        setFixDraft(e.text);
                      }}
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
                    <button className="ghost danger" onClick={() => removeEntry(e.id)}>
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
                      onUndo={undoAiEditText(e) ? () => pasteText(undoAiEditText(e) as string) : undefined}
                    />
                  )}
                </>
              )}
            </li>
          ))}
        </ul>
      )}

      {/* The page's definite end: where you are, and the two ways to move. */}
      <nav className="history-pager" aria-label="History pages" data-testid="history-pager">
        <button className="ghost" onClick={goNewer} disabled={page === 0 || loading}>
          ← Newer
        </button>
        <span className="tiny">{pageRangeLabel(page, rows.length)}</span>
        <button className="ghost" data-testid="history-older" onClick={goOlder} disabled={!hasOlder || loading}>
          Older →
        </button>
      </nav>
      {error ? (
        <p className="tiny history-error" role="alert">
          That page did not load.{" "}
          <button className="ghost" onClick={reload}>
            Try again
          </button>
        </p>
      ) : null}
    </div>
  );
}
