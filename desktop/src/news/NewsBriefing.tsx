// yap-1106-news-home-briefing — the Home briefing: one contained card under
// the studio with Cinder and Yappy in its header (Cinder names it, Yappy
// explains), up to three stories in a row, each with a link to its source,
// any ticker that moved 3% or more (always marked delayed), your topics, and
// "Hide for today". News lives on Home only: nothing here reaches the pill.
//
// When it refreshes: on the first open after 6 AM and after 4 PM (the
// backend's `news_refresh` decides, with the 30-minute floor and the quiet
// rules), and the card only changes when something new ranks higher. While
// Home is open the tickers are checked every 30 minutes; the backend only
// fetches in market hours. Nothing is fetched during a meeting, a screen
// share, Do Not Disturb or low battery.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppCtx } from "../appShell";
import { FRIENDS } from "../sprites/art";
import { domBuffer, paintCrisp } from "../sprites/crisp";
import { TOPIC_CHIPS, cadenceOf, loadNews, type NewsCadence, type NewsRefreshOutcome, type NewsView } from "./newsPrefs";
import {
  QUOTE_CHECK_MS,
  buildBriefing,
  cadenceLine,
  hiddenToday,
  hideToday,
  isNewBriefing,
  loadStoredBriefing,
  pairLines,
  quietNow,
  quoteLine,
  ranksHigher,
  saveStoredBriefing,
  sourceLabel,
  topStories,
  topicTag,
  type Briefing,
  type CachedStory,
  type ExplainedLine,
  type QuoteMover,
  type QuotesOutcome,
} from "./briefing";
import { duoAt, duoTurns, skippedArrival, type DuoFrame, type DuoTurn } from "./duo";

export type BriefingPhase = "loading" | "ready" | "empty" | "error";

/** The pair, drawn at a whole-number scale (2x on the design page). */
const PAIR_BOX = { w: 92, h: 84 };

function PairSprite({ name, talking = false }: { name: "cinder" | "yappy"; talking?: boolean }) {
  const ref = useRef<HTMLCanvasElement | null>(null);
  useEffect(() => {
    const c = ref.current;
    if (!c) return;
    paintCrisp<HTMLCanvasElement>(c, FRIENDS[name], 0, undefined, PAIR_BOX, window.devicePixelRatio, domBuffer, name);
  }, [name]);
  return (
    <canvas
      ref={ref}
      className={talking ? "news-briefing-sprite news-briefing-sprite--talking" : "news-briefing-sprite"}
      data-news-sprite={name}
      data-news-talking={talking ? "yes" : undefined}
      aria-hidden="true"
    />
  );
}

function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

/** yap-1106-news-duo-delivery — the joint delivery as the card shows it. */
export interface DuoView {
  frame: DuoFrame;
  turns: readonly DuoTurn[];
}

function prefersReducedMotion(): boolean {
  try {
    return typeof window !== "undefined" && !!window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    return false;
  }
}

function timeLabel(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

export function NewsBriefingView({
  phase,
  briefing,
  movers,
  tickers,
  topics,
  cadence,
  onOpen,
  onHide,
  onRetry,
  onEditTopics,
  duo = null,
  onSkipDuo,
}: {
  phase: BriefingPhase;
  briefing: Briefing | null;
  movers: readonly QuoteMover[];
  tickers: readonly string[];
  topics: readonly string[];
  cadence: NewsCadence;
  onOpen: (url: string) => void;
  onHide: () => void;
  onRetry: () => void;
  onEditTopics: () => void;
  /** yap-1106-news-duo-delivery: the joint delivery in progress (null = finished or none). */
  duo?: DuoView | null;
  onSkipDuo?: () => void;
}) {
  const stories = briefing?.stories ?? [];
  const lines = briefing && stories.length > 0 ? pairLines(briefing, tickers) : null;
  // The delivery only shows while the card is ready and the script is running.
  const live = phase === "ready" && lines && duo && duo.frame.stage !== "done" ? duo : null;
  const turn = live && live.frame.stage === "turns" && live.frame.turn >= 0 ? (live.turns[live.frame.turn] ?? null) : null;
  const storyCount = live ? Math.ceil(live.turns.length / 2) : 0;
  const chipLabel = (id: string) => TOPIC_CHIPS.find((c) => c.id === id)?.label ?? id;
  return (
    <section className="card news-briefing" data-news-briefing={phase} aria-labelledby="news-briefing-title" aria-busy={phase === "loading"}>
      <header className="news-briefing-head">
        <div className="news-briefing-pair">
          <PairSprite name="cinder" talking={turn?.who === "cinder"} />
          <PairSprite name="yappy" talking={turn?.who === "yappy"} />
        </div>
        <div className="news-briefing-says">
          <h3 id="news-briefing-title">
            Yappy News{briefing && stories.length > 0 ? <span className="muted"> · {timeLabel(briefing.deliveredMs)}</span> : null}
          </h3>
          {phase === "loading" && <p className="muted">Yappy is reading the news…</p>}
          {phase === "empty" && (
            <p className="muted" data-news-briefing-empty>
              Nothing on your topics yet. Yappy reads the news after 6 AM and after 4 PM.
            </p>
          )}
          {phase === "error" && (
            <p className="news-error" role="alert">
              Yap couldn't get today's news.{" "}
              <button type="button" className="link" onClick={onRetry} data-news-briefing-retry>
                Try again
              </button>
            </p>
          )}
          {live && !turn && (
            <p className="muted" data-news-duo="arriving" aria-live="polite">
              Cinder is bringing Yappy the paper…
            </p>
          )}
          {live && turn && live.frame.turn >= 0 && (
            <p data-news-says={turn.who} data-news-turn={live.frame.turn} aria-live="polite">
              <b>{turn.who === "cinder" ? "Cinder" : "Yappy"}:</b> “{turn.text}”{" "}
              <span className="muted tiny">
                · story {Math.floor(live.frame.turn / 2) + 1} of {storyCount}
              </span>
            </p>
          )}
          {live && onSkipDuo && (
            <button type="button" className="link" onClick={onSkipDuo} data-news-duo-skip>
              Skip to the stories
            </button>
          )}
          {lines && phase === "ready" && !live && (
            <>
              <p data-news-says="cinder">
                <b>Cinder:</b> “{lines.cinder}”
              </p>
              <p data-news-says="yappy">
                <b>Yappy:</b> “{lines.yappy}”
              </p>
            </>
          )}
        </div>
      </header>

      {phase === "ready" && stories.length > 0 && (
        <ol className="news-briefing-stories" data-news-stories={stories.length}>
          {stories.map((s) => (
            <li
              key={s.urlKey}
              className={turn?.urlKey === s.urlKey ? "news-briefing-story news-briefing-story--current" : "news-briefing-story"}
              data-news-story={s.urlKey}
              data-news-story-current={turn?.urlKey === s.urlKey ? "yes" : undefined}
            >
              <span className="news-briefing-tag">{topicTag(s, tickers)}</span>
              <h4>{s.headline}</h4>
              {s.line ? <p className="news-briefing-line">“{s.line}”</p> : null}
              <button
                type="button"
                className="link news-briefing-source"
                onClick={() => onOpen(s.url)}
                data-news-source={s.source}
                aria-label={`Open the story: ${s.headline} (${sourceLabel(s, tickers)})`}
              >
                {sourceLabel(s, tickers)} · open ↗
              </button>
            </li>
          ))}
        </ol>
      )}

      {movers.length > 0 && (
        <p className="news-briefing-quotes" data-news-quotes>
          {movers.map((q) => (
            <span key={q.symbol} data-news-quote={q.symbol}>
              {quoteLine(q)}
            </span>
          ))}
        </p>
      )}

      <footer className="news-briefing-foot">
        <div className="news-briefing-topics" aria-label="Your topics">
          <span className="muted tiny">Your topics</span>
          {topics.map((t) => (
            <span key={t} className="news-briefing-chip">
              {chipLabel(t)}
            </span>
          ))}
          {tickers.map((t) => (
            <span key={t} className="news-briefing-chip">
              {t}
            </span>
          ))}
          <button type="button" className="link" onClick={onEditTopics} data-news-edit-topics>
            + Add
          </button>
        </div>
        <p className="muted tiny">
          {cadenceLine(cadence)} ·{" "}
          <button type="button" className="link" onClick={onHide} data-news-hide>
            Hide for today
          </button>
        </p>
      </footer>
    </section>
  );
}

/**
 * fix-l3b3-2-briefing-no-cloud-when-quiet — the explain step. While a meeting,
 * a screen share or Do Not Disturb is on it is not even asked for (the
 * backend also refuses during low battery): `null` = the headlines stand
 * alone, exactly as on an explain failure, and every story keeps its link.
 */
export async function explainForBriefing(quiet: boolean, tone: string | undefined): Promise<ExplainedLine[] | null> {
  if (quiet) {
    console.info("news briefing explain_held reason=quiet");
    return null;
  }
  try {
    const ex = await invoke<{ lines: ExplainedLine[]; fallback: string }>("news_explain_top", { voice: tone ?? "" });
    return Array.isArray(ex?.lines) ? ex.lines : null;
  } catch {
    // The headlines stand alone; every story still links to its source.
    console.warn("news briefing explain_failed");
    return null;
  }
}

/** A briefing of `stories` (top three), explained unless quiet. */
export async function composeBriefing(
  stories: readonly CachedStory[],
  quiet: boolean,
  tone: string | undefined,
  now: number,
): Promise<Briefing> {
  return buildBriefing(stories, await explainForBriefing(quiet, tone), now);
}

/** No briefing yet today and no refresh due: the cached top three (null = nothing cached). */
export async function cachedBriefing(quiet: boolean, tone: string | undefined, now: number): Promise<Briefing | null> {
  const cached = topStories(await invoke<unknown[]>("news_briefing_cached"));
  if (cached.length === 0) return null;
  return composeBriefing(cached, quiet, tone, now);
}

export interface NewsBriefingProps {
  meetingRecording: boolean;
  screenSharing: boolean;
  doNotDisturb: boolean;
  /** The stored `companionTone`: Yappy's voice for the one-line explanations. */
  tone: string | undefined;
  /** A NEW briefing landed (Cinder flies in, unless reduce motion is on); `turns` = the joint delivery's turn count. */
  onDelivered: (deliveredMs: number, turns: number) => void;
  /**
   * yap-1106-news-duo-delivery: when the joint delivery started, or null while
   * none runs (none yet, or held for a meeting, a screen share or Do Not Disturb).
   */
  duoArrivedAtMs?: number | null;
  /** Skip the delivery: the arrival time that makes it read as finished. */
  onSkipDuo?: (arrivedAtMs: number) => void;
  /** Whether a briefing is on the card (Cinder waits at the back door). */
  onReady: (ready: boolean) => void;
  onEditTopics: () => void;
}

export default function NewsBriefing({
  meetingRecording,
  screenSharing,
  doNotDisturb,
  tone,
  onDelivered,
  onReady,
  onEditTopics,
  duoArrivedAtMs = null,
  onSkipDuo,
}: NewsBriefingProps) {
  const { toastError } = useAppCtx();
  const [view, setView] = useState<NewsView | null>(null);
  const [briefing, setBriefing] = useState<Briefing | null>(() => loadStoredBriefing(storage(), Date.now()));
  const [movers, setMovers] = useState<QuoteMover[]>([]);
  const [phase, setPhase] = useState<BriefingPhase>("loading");
  const [hidden, setHidden] = useState(() => hiddenToday(storage(), Date.now()));
  const briefingRef = useRef(briefing);
  briefingRef.current = briefing;
  const quietRef = useRef(false);
  quietRef.current = quietNow({ meetingRecording, screenSharing, doNotDisturb });

  const deliver = useCallback(
    async (next: Briefing, from: string) => {
      const fresh = isNewBriefing(briefingRef.current, next);
      saveStoredBriefing(storage(), next);
      setBriefing(next);
      setPhase("ready");
      console.info(
        `news briefing_delivered from=${from} stories=${next.stories.length} explained=${next.stories.filter((s) => s.line).length}`,
      );
      if (fresh) onDelivered(next.deliveredMs, next.stories.length * 2);
    },
    [onDelivered],
  );

  const load = useCallback(async () => {
    const started = Date.now();
    let v: NewsView;
    try {
      v = await loadNews();
    } catch {
      console.warn("news briefing view_failed");
      setPhase(briefingRef.current ? "ready" : "error");
      return;
    }
    setView(v);
    if (!v.active) return;
    const current = briefingRef.current;
    try {
      const out = await invoke<NewsRefreshOutcome>("news_refresh", { trigger: "home_opened" });
      const fresh = topStories(out.stories);
      if (out.ran && ranksHigher(current, fresh)) {
        await deliver(await composeBriefing(fresh, quietRef.current, tone, Date.now()), "refresh");
      } else if (current) {
        if (out.ran) console.info("news briefing_kept reason=nothing_ranks_higher");
        setPhase("ready");
      } else {
        // No briefing yet today and no refresh due: the cached top three.
        const cached = await cachedBriefing(quietRef.current, tone, Date.now());
        if (cached) await deliver(cached, "cache");
        else setPhase("empty");
      }
      console.info(`news briefing_opened ran=${out.ran} skipped=${out.skipped ?? "none"} ms=${Date.now() - started}`);
    } catch {
      console.warn("news briefing refresh_failed");
      setPhase(current ? "ready" : "error");
    }
  }, [deliver, tone]);

  useEffect(() => {
    void load();
    // Once per Home open (a later open mounts Home again and runs it again).
  }, []);

  const active = !!view?.active;
  const tickerCount = view?.tickers.length ?? 0;
  // Market quotes: every 30 minutes while Home is open (the backend checks
  // market hours, the floor and the quiet rules; this side skips a tick while
  // a meeting, share or Do Not Disturb is on, so nothing is even asked for).
  useEffect(() => {
    if (!active || tickerCount === 0) return;
    let live = true;
    const tick = async () => {
      if (quietRef.current) {
        console.info("news quotes_held reason=quiet");
        return;
      }
      try {
        const r = await invoke<QuotesOutcome>("news_quotes_check", { trigger: "home_tick" });
        if (live && r.ran) setMovers(Array.isArray(r.movers) ? r.movers : []);
      } catch {
        console.warn("news quotes_check_failed");
      }
    };
    void tick();
    const id = window.setInterval(() => void tick(), QUOTE_CHECK_MS);
    return () => {
      live = false;
      window.clearInterval(id);
    };
  }, [active, tickerCount]);

  const showing = active && !hidden && (phase !== "ready" || (briefing?.stories.length ?? 0) > 0);

  // yap-1106-news-duo-delivery — Cinder names each topic, Yappy explains it in
  // the chosen voice, on the same clock the studio reads. The card re-renders
  // only when the stage or the turn changes (one timer, no polling).
  const reduce = useRef(prefersReducedMotion()).current;
  const turns = useMemo(() => duoTurns(briefing, view?.tickers ?? [], tone), [briefing, view?.tickers, tone]);
  const [now, setNow] = useState(() => Date.now());
  const frame = duoAt(duoArrivedAtMs, now, reduce, turns.length);
  const duoKey = `${duoArrivedAtMs ?? "none"}:${frame.stage}:${frame.turn}`;
  useEffect(() => {
    if (frame.nextChangeMs == null) return;
    const id = window.setTimeout(() => setNow(Date.now()), Math.max(16, Math.ceil(frame.nextChangeMs) + 16));
    return () => window.clearTimeout(id);
  }, [duoKey]);
  useEffect(() => {
    setNow(Date.now());
    if (duoArrivedAtMs != null && turns.length > 0 && Date.now() - duoArrivedAtMs < 1000) {
      console.info(`news duo_started turns=${turns.length} reduce=${reduce}`);
    }
  }, [duoArrivedAtMs]);
  const prevStage = useRef(frame.stage);
  useEffect(() => {
    if (prevStage.current !== "done" && frame.stage === "done" && duoArrivedAtMs != null) {
      console.info(`news duo_finished turns=${turns.length}`);
    }
    prevStage.current = frame.stage;
  }, [frame.stage]);
  const skipDuo = useCallback(() => {
    const at = Date.now();
    console.info(`news duo_skipped at_turn=${frame.turn} stage=${frame.stage}`);
    onSkipDuo?.(skippedArrival(at, turns.length, reduce));
    setNow(at);
  }, [frame.turn, frame.stage, onSkipDuo, turns.length, reduce]);
  useEffect(() => {
    onReady(showing && phase === "ready" && (briefing?.stories.length ?? 0) > 0);
  }, [showing, phase, briefing, onReady]);

  if (!view || !showing) return null;

  async function open(url: string) {
    try {
      await invoke("news_open_story", { url });
    } catch (e) {
      toastError(e, "Yap couldn't open that story. Try again.");
    }
  }

  function hide() {
    hideToday(storage(), Date.now());
    setHidden(true);
    console.info("news briefing_hidden until=tomorrow");
  }

  return (
    <NewsBriefingView
      phase={phase}
      briefing={briefing}
      movers={movers}
      tickers={view.tickers}
      topics={view.topics}
      cadence={cadenceOf(view)}
      onOpen={(u) => void open(u)}
      onHide={hide}
      onRetry={() => {
        setPhase("loading");
        void load();
      }}
      onEditTopics={onEditTopics}
      duo={frame.stage === "done" ? null : { frame, turns }}
      onSkipDuo={onSkipDuo ? skipDuo : undefined}
    />
  );
}
