import BootNote from "../BootNote";
import { invoke } from "@tauri-apps/api/core";
import YappyHouse from "../home/YappyHouse";
import { type FailedDictation, type TranscriptEntry, retainedClipsLabel, formatTime, missingDictationGrants } from "../appTypes";
import { useAppCtx } from "../appShell";
import { nameProduct } from "../permission";
import { viewState } from "../viewState";
import { type RetryPhase, retryAllLabel, retryButtonLabel } from "../retryDictation";
import { EmptyState, ErrorState, LoadingState, WaveGlyph } from "../ViewStates";
import {
  HOME_RECENT, attentionTitle, recentTakes, requestHistoryFocus, seeAllLabel, showNeedsAttention, statsLine, takePreview,
} from "../home/homeLayout";

/**
 * yap-1106-home-full-width-room — "Needs attention": failed and cancelled
 * takes, beside the latest take. The audio is still on disk, so nothing was
 * lost: Retry re-runs speech on that clip, Discard throws the row and its audio
 * away (clips are purged after 7 days). It renders NOTHING when empty.
 */
export function NeedsAttention({
  failed, retrying, retryAll, retryPhase, onRetry, onRetryAll, onDiscard,
}: {
  failed: readonly FailedDictation[];
  retrying: string | null;
  retryAll: { done: number; total: number } | null;
  retryPhase?: RetryPhase | null;
  onRetry: (id: string, keepPartial?: boolean) => void;
  onRetryAll: () => void;
  onDiscard: (id: string) => void;
}) {
  if (!showNeedsAttention(failed)) return null;
  return (
    <section className="needs-attention" data-home="needs-attention" aria-labelledby="needs-attention-h">
      <div className="failed-head">
        <h3 id="needs-attention-h">
          Needs attention <span className="count">{failed.length}</span>
        </h3>
        {/* PRIV-B — retained audio is VISIBLE: the count, the window, the way out. */}
        <span className="tiny" data-testid="retained-clips">
          {retainedClipsLabel(failed.length)} — retry or discard. Nothing here is pasted for you.
        </span>
        {failed.length > 1 ? (
          <button
            className="ghost failed-retry-all"
            data-testid="retry-all"
            disabled={retryAll !== null || retrying !== null}
            aria-busy={retryAll !== null}
            onClick={onRetryAll}
          >
            {retryAllLabel({ running: retryAll !== null, done: retryAll?.done ?? 0, total: retryAll?.total ?? failed.length })}
          </button>
        ) : null}
      </div>
      <ul className="attention-list">
        {failed.map((f) => (
          <li key={f.id} className="attention-row">
            <div className="card-meta">
              <strong>{attentionTitle(f.error)}</strong>
              <span>
                {formatTime(f.createdAt)}
                {f.sourceApp ? ` · ${f.sourceApp}` : ""}
                {f.speechSeconds > 0 ? ` · ${f.speechSeconds.toFixed(1)}s of audio kept` : " · audio kept"}
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
                disabled={retrying === f.id || retryAll !== null}
                onClick={() => onRetry(f.id)}
              >
                {retryButtonLabel({ retrying: retrying === f.id, phase: retryPhase ?? null, resumable: !!f.partialText })}
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
                      onRetry(f.id, true);
                  }}
                >
                  Keep what was transcribed
                </button>
              ) : null}
              <button className="ghost danger" onClick={() => onDiscard(f.id)}>
                Discard
              </button>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

export default function Home() {
  const {
    bootError, bootErrors, booting, retryBoot, copyText, refreshAll,
    failed, history, insights, meetingStatus, pasteText, perms, query,
    retryAll, retryAllFailed, retryFailed, retrying, retryPhase, setFailed,
    setNav, setQuery, dropRetry, settings, status, toastError, toggleRecord,
  } = useAppCtx();
  // Y5-B fix round (#192) — bootError only replaces THIS view when this view's
  // own data never landed; each of history and stats names its own error.
  const homeError =
    bootErrors?.get_history ?? bootErrors?.get_insights ?? (insights == null ? bootError : null);
  const state = viewState(history, booting || !insights, homeError);
  // H2 — nothing dictated yet, no search typed, and a dictation GRANT missing.
  const missingGrants = missingDictationGrants(perms);
  const permsGate = history.length === 0 && !query && missingGrants.length > 0;

  async function discardFailed(id: string) {
    try {
      await invoke("discard_failed_dictation", { id });
      setFailed((f) => f.filter((x) => x.id !== id));
      dropRetry(id);
      console.info("home attention_discarded");
    } catch (e) {
      console.warn("home attention_discard_failed");
      toastError(e);
    }
  }
  function openInHistory(e: TranscriptEntry | null, from: string) {
    requestHistoryFocus(e ? e.id : null);
    console.info(`home open_history from=${from}`);
    setNav("history");
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

  const latest: TranscriptEntry | null = !query && history.length > 0 ? history[0] : null;
  // The last 5 takes (the latest included, as on the approved mock), or the
  // top 5 matches while a search is typed. Never more: Home has an end.
  const recent = recentTakes(history);
  const stats = statsLine({
    wordsToday: insights?.wordsToday,
    streakDays: insights?.streakDays,
    totalTakes: insights?.totalSessions,
  });
  const total = insights?.totalSessions ?? null;

  return (
    <div className="home-view">
      <BootNote keys={["get_status", "list_failed_dictations", "list_crash_events"]} />
      {/* The room, full column width at a whole-number scale. During a meeting
          Hoot sits in it and its label carries the meeting clock — Home shows
          no other timer. */}
      <YappyHouse companionTone={settings?.companionTone} meeting={meetingStatus} />

      {/* The three stat tiles, now one quiet line under the room. */}
      <p className="home-stats" data-home="stats">
        {stats.length > 0 ? <span>{stats.join(" · ")}</span> : null}
        <button type="button" className="link" onClick={() => setNav("insights")}>
          Insights →
        </button>
      </p>

      {/* yap-uiux-2026-10-16-H2 — with Microphone or Accessibility ungranted,
          the empty Home's first card is the thing that unblocks dictation. */}
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

      {/* Y3-D — a take the user regrets can be stopped in either half
          (recording or decoding); the audio is parked in Needs attention. */}
      {(status.recording || status.busy) && (
        <button
          className="ghost cancel-take"
          onClick={() => {
            console.info("home take_cancel_requested");
            void invoke("cancel_transcription");
          }}
        >
          Cancel this take (esc)
        </button>
      )}

      <div className={showNeedsAttention(failed) && latest ? "home-split" : "home-split single"}>
        {latest ? (
          <article className="card latest-take" data-home="latest" aria-live="polite">
            <div className="card-meta">
              <span>
                <strong>Last take</strong> {formatTime(latest.createdAt)}
                {latest.sourceApp ? ` · ${latest.sourceApp}` : ""}
              </span>
              <span>{(latest.wordCount ?? 0).toLocaleString("en-US")} words</span>
            </div>
            <p className="latest-text">{latest.text}</p>
            <div className="actions">
              <button onClick={() => copyText(latest.text)}>Copy</button>
              <button className="primary" onClick={() => openInHistory(latest, "latest")}>
                Open
              </button>
              <button className="ghost" onClick={() => pasteText(latest.text)}>
                Paste again
              </button>
            </div>
          </article>
        ) : null}
        <NeedsAttention
          failed={failed}
          retrying={retrying}
          retryAll={retryAll}
          retryPhase={retryPhase}
          onRetry={(id, keep) => void retryFailed(id, keep)}
          onRetryAll={() => void retryAllFailed()}
          onDiscard={(id) => void discardFailed(id)}
        />
      </div>

      <div className="toolbar">
        {/* yap-uiux-2026-10-16-H4 — what it does, not how it works. */}
        <input
          type="search"
          aria-label="Search your dictations"
          placeholder="Search everything you've said…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>

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
        <>
          {recent.length > 0 ? (
            <ul className="recent-takes" data-home="recent">
              {recent.map((e) => (
                <li key={e.id}>
                  <button type="button" className="recent-row" onClick={() => openInHistory(e, "recent")}>
                    <span className="recent-when">{formatTime(e.createdAt)}</span>
                    <span className="recent-text">{takePreview(e.text)}</span>
                    <span className="recent-words">{(e.wordCount ?? 0).toLocaleString("en-US")} w</span>
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
          {/* Home's definite end: no list grows as you scroll. */}
          <div className="home-end" data-home="see-all">
            <span className="tiny">
              {query
                ? `The top ${HOME_RECENT} matches. Every match lives in History.`
                : `That's the last ${HOME_RECENT}. Older takes live in History.`}
            </span>
            <button type="button" className="ghost" data-testid="home-see-all" onClick={() => openInHistory(null, "see_all")}>
              {seeAllLabel(total, query)}
            </button>
          </div>
        </>
      )}
    </div>
  );
}
