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
import { FRIENDS, H } from "../sprites/art";
import { crispPlan, domBuffer } from "../sprites/crisp";
import { cadenceOf, topicLabel, loadNews, type NewsCadence, type NewsRefreshOutcome, type NewsView } from "./newsPrefs";
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

/**
 * The pair, drawn at a whole-number scale (2x on the design page).
 * yap-1107-parity-news (batch-2 bounce: "duo sprite 104 vs 208"): the approved
 * card (news.html §02 #duo) draws Cinder + Yappy as ONE 104x44 art canvas —
 * Yappy stamped at (2,6), Cinder at (54,2), the paper between them at (44,30)
 * — shown at 208x88 CSS px. Two separate 104x88 canvases measured as two
 * 104-wide sprites against the page's one 208-wide sprite, so the pair is now
 * the same single canvas, composited exactly as the page composites it.
 */
export const DUO_ART = { w: 104, h: 44 } as const;
export const DUO_SCALE = 2;
export const PAIR_BOX = { w: DUO_ART.w * DUO_SCALE, h: DUO_ART.h * DUO_SCALE };
/** Where each friend's art box sits inside the duo art canvas (art px, news.html §02). */
export const DUO_AT = { yappy: { x: 2, y: 6 }, cinder: { x: 54, y: 2 } } as const;
/** The folded paper Cinder hands over: sprite-art pixels, copied from news.html §02. */
const DUO_PAPER: readonly { fill: string; x: number; y: number; w: number; h: number }[] = [
  { fill: "#c9c3ad", x: 44, y: 30, w: 12, h: 9 },
  { fill: "#fffdf5", x: 45, y: 30, w: 10, h: 8 },
  { fill: "#4a4f5c", x: 46, y: 32, w: 8, h: 1 },
  { fill: "#4a4f5c", x: 46, y: 34, w: 6, h: 1 },
];
/** How often the speaker's mouth opens and closes while it has the turn (ms). */
const DUO_MOUTH_MS = 250;

/** Paint the duo art canvas into `canvas` at the 208x88 box. Returns false (and logs once) on failure. */
export function paintDuo(canvas: HTMLCanvasElement, talking: "cinder" | "yappy" | null, mouthOpen: boolean, dprRaw: number | undefined | null): boolean {
  try {
    const plan = crispPlan(PAIR_BOX.w, PAIR_BOX.h, DUO_ART.w, DUO_ART.h, dprRaw);
    if (canvas.width !== plan.backingW) canvas.width = plan.backingW;
    if (canvas.height !== plan.backingH) canvas.height = plan.backingH;
    canvas.style.width = `${plan.cssW}px`;
    canvas.style.height = `${plan.cssH}px`;
    canvas.style.imageRendering = "pixelated";
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      console.warn("sprites paint-failed name=news-duo reason=no-2d-context");
      return false;
    }
    const art = domBuffer(DUO_ART.w, DUO_ART.h);
    const actx = art.image.getContext("2d");
    if (!actx) throw new Error("no-2d-context");
    actx.imageSmoothingEnabled = false;
    for (const name of ["yappy", "cinder"] as const) {
      const sprite = FRIENDS[name];
      const buf = domBuffer(sprite.w, sprite.h);
      const open = talking === name && mouthOpen;
      sprite.draw(buf.ctx, H(buf.ctx), name === "yappy" && open ? 2 : 0, name === "cinder" ? { talk: open, t: 0 } : undefined);
      actx.drawImage(buf.image, DUO_AT[name].x, DUO_AT[name].y);
    }
    for (const r of DUO_PAPER) {
      actx.fillStyle = r.fill;
      actx.fillRect(r.x, r.y, r.w, r.h);
    }
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, plan.backingW, plan.backingH);
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(art.image, 0, 0, DUO_ART.w, DUO_ART.h, plan.dx, plan.dy, DUO_ART.w * plan.scale, DUO_ART.h * plan.scale);
    return true;
  } catch (e) {
    console.warn(`sprites paint-failed name=news-duo reason=${e instanceof Error ? e.name : "unknown"}`);
    return false;
  }
}

function PairSprite({ talking = null }: { talking?: "cinder" | "yappy" | null }) {
  const ref = useRef<HTMLCanvasElement | null>(null);
  useEffect(() => {
    const c = ref.current;
    if (!c) return;
    let open = talking !== null;
    paintDuo(c, talking, open, window.devicePixelRatio);
    // Whoever has the turn talks (mouth open/closed, whole frames); reduce
    // motion keeps the pair still and only the words change.
    if (talking === null || prefersReducedMotion()) return;
    const id = window.setInterval(() => {
      open = !open;
      paintDuo(c, talking, open, window.devicePixelRatio);
    }, DUO_MOUTH_MS);
    return () => window.clearInterval(id);
  }, [talking]);
  return (
    <canvas
      ref={ref}
      className="news-briefing-sprite"
      width={PAIR_BOX.w}
      height={PAIR_BOX.h}
      style={{ width: PAIR_BOX.w, height: PAIR_BOX.h }}
      data-news-sprite="duo"
      data-news-pair="cinder yappy"
      data-news-talking={talking ?? undefined}
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

/**
 * yap-1107-parity-news — the approved card's header ends in "8:02 AM ·" plus a
 * small amber tag (news.html §02 `.q`, #a06a00 on #fff4d6, 11 px mono). The page
 * tags its made-up stories "sample stories"; real stories are not samples, so
 * the app's tag names which paper Cinder brought: the refresh runs after 6 AM
 * and after 4 PM (§03), so a briefing made from 6 AM until 4 PM is the morning
 * paper and any other is the evening paper. Same place, size and colours, a
 * true word, and both labels are 13 characters so the header's width (and the
 * bubble's four lines, .bhead 98.56 px) does not move with the hour.
 */
export function paperLabel(ms: number): "morning paper" | "evening paper" {
  const h = new Date(ms).getHours();
  return h >= 6 && h < 16 ? "morning paper" : "evening paper";
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
  const chipLabel = topicLabel;
  return (
    <section className="card news-briefing" data-news-briefing={phase} aria-labelledby="news-briefing-title" aria-busy={phase === "loading"}>
      <header className="news-briefing-head">
        <div className="news-briefing-pair">
          <PairSprite talking={turn ? turn.who : null} />
        </div>
        <div className="news-briefing-says">
          {/* yap-1107-parity-news — the approved card draws no title; the
              heading stays for screen readers and the time moves to the
              header's third column, as on the page. */}
          <h3 id="news-briefing-title" className="sr-only">
            Yappy News{briefing && stories.length > 0 ? ` · ${paperLabel(briefing.deliveredMs)} · ${timeLabel(briefing.deliveredMs)}` : ""}
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
          {/* One line of speech for both, as the page's .bsay draws it: the
              bubble wraps like the artifact instead of two short paragraphs. */}
          {lines && phase === "ready" && !live && (
            <p data-news-said="pair">
              <span data-news-says="cinder">
                <b>Cinder:</b> “{lines.cinder}”
              </span>{" "}
              <span data-news-says="yappy">
                <b>Yappy:</b> “{lines.yappy}”
              </span>
            </p>
          )}
        </div>
        <span className="news-briefing-when" aria-hidden="true" data-news-briefing-when>
          {briefing && stories.length > 0 ? (
            <>
              <span className="news-briefing-clock">{timeLabel(briefing.deliveredMs)}</span> ·{" "}
              <span className="news-briefing-paper" data-news-paper>
                {paperLabel(briefing.deliveredMs)}
              </span>
            </>
          ) : (
            ""
          )}
        </span>
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
              {s.why ? (
                <p className="news-briefing-why" data-news-why aria-label={`Why this story: ${s.why}`}>
                  {s.why}
                </p>
              ) : null}
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
          <span className="muted tiny news-briefing-meta">Your topics</span>
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
          <button type="button" className="news-briefing-chip news-briefing-chip--add" onClick={onEditTopics} data-news-edit-topics>
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
