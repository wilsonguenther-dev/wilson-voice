import { useEffect, useState, type KeyboardEvent, type ReactNode } from "react";
import BootNote from "../BootNote";
import { heatLevel, HEAT_FILL, formatMonth, monthName, formatDay, formatDayShort } from "../appTypes";
import { carbonRegistered, dictateLabel, fnDelivering } from "../hotkeyStatus";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { EmptyState, ErrorState, SeriesGlyph } from "../ViewStates";
import {
  CHART_EMPTY_COPY,
  CHART_ERROR_COPY,
  chartState,
  formatMinutes,
  headlineNumbers,
  insightSentence,
  meetingHours,
  nextPointIndex,
  pointAnnouncement,
  topPeople,
  uiLog,
  type ChartState,
} from "./insightsModel";

export default function Insights() {
  const { bootError, bootErrors, booting, retryBoot, insights, refreshAll, setNav, toggleRecord } = useAppCtx();
  // Y5-B — the second case the item names by hand. App.tsx rendered
  // `nav === "insights" && insights && <Insights/>` and this module opened with
  // `if (!insights) return null`, so zero takes was a heading over nothing.
  // Y5-B review fix — totalSessions counts dictations only (db.rs: COUNT(*)
  // FROM transcripts), so an install with meetings but no dictations is NOT
  // empty: the Meetings strip below has real data to show.
  const state = viewState(
    insights === null
      ? null
      : insights.totalSessions + (insights.meetings?.totalMeetings ?? 0),
    booting,
    // Y5-B fix round (#192): only when insights never landed - see Home.tsx.
    bootErrors?.get_insights ?? (insights == null ? bootError : null),
  );
  if (state === "error")
    return (
      <ErrorState
        data-error-state="insights"
        view="insights"
        error={bootErrors?.get_insights ?? bootError}
        actionLabel="Read stats again"
        onAction={() => void (bootErrors?.get_insights ? retryBoot(["get_insights"]) : refreshAll())}
      />
    );
  // yap-1106-insights-redesign — loading keeps the final layout: the numbers
  // read "—" and every chart frame holds its size with a quiet skeleton, so
  // nothing jumps when the stats land (UI spec §3.4, Loading).
  if (state === "loading") return <InsightsBody loading />;
  if (state === "empty" || !insights)
    return (
      <EmptyState
        data-empty-state="insights"
        glyph={<SeriesGlyph />}
        title="No takes to measure yet"
        body="Words, speed, streaks and a year of activity chart here once you have dictated something. One take is enough to start."
        actionLabel="Hold fn and say something"
        onAction={() => {
          setNav("home");
          toggleRecord();
        }}
      />
    );
  return <InsightsBody loading={false} />;
}

/**
 * yap-1106-insights-redesign — the screen in reading order: three headline
 * numbers and one sentence, then the charts, then meetings, then the closed
 * "Speed and setup" reference. Every chart sits in a {@link ChartFrame} that
 * owns its loading / empty / error state, so one failed series never blanks
 * the others.
 */
function InsightsBody({ loading }: { loading: boolean }) {
  const {
    booting, bootErrors, retryBoot, daily30, hasActivity, heat, heatSummary, insights, setNav,
    maxApp, maxDaily30, maxWeek, monthlySeries, monthlyView, settings, status,
  } = useAppCtx();
  const busy = loading || !!booting;
  const ins = loading ? null : insights;
  const dailyErr = bootErrors?.daily_series ?? null;
  const monthlyErr = bootErrors?.monthly_series ?? null;
  const days = daily30 ?? [];
  const headline = headlineNumbers(ins);
  const sentence = insightSentence(ins, days);
  const week = ins?.wordsLast7 ?? [];
  const apps = ins?.topApps ?? [];
  const months = monthlyView?.rows ?? [];
  const meetings = ins?.meetings;
  const people = topPeople(meetings);
  const retry = (key: "daily_series" | "monthly_series") => () => void retryBoot([key]);

  useEffect(() => {
    uiLog("insights_view", { state: loading ? "loading" : "ready", meetings: meetings?.totalMeetings ?? 0 });
    // One line per arrival on the screen, not per re-render.
  }, [loading]);

  return (
    <div className="insights" data-loading-state={loading ? "insights" : undefined} aria-busy={loading || undefined}>
      <BootNote keys={["daily_series", "monthly_series"]} />

      <section className="insights-lead" aria-label="Your numbers">
        <div className="insights-headline">
          {headline.map((h) => (
            <div key={h.key} className="stat big insights-number" data-headline={h.key} title={h.hint}>
              <div className="stat-n">{h.value}</div>
              <div className="stat-l">{h.label}</div>
            </div>
          ))}
        </div>
        {loading ? (
          <div className="insights-sentence chart-skeleton" aria-hidden="true" />
        ) : sentence ? (
          <p className="insights-sentence" data-insight-sentence>
            {sentence}
          </p>
        ) : null}
      </section>

      <div className="insights-charts">
        <ChartFrame
          id="week"
          title="Last 7 days"
          state={chartState({ loading: busy, hasData: week.some((d) => d.words > 0) })}
          labels={week.map((d) => pointAnnouncement(formatDay(d.date), d.words))}
          minHeight={200}
        >
          {(active) => (
            <div className="bars">
              {week.map((d, i) => (
                <div key={d.date} className={`bar-row${i === active ? " is-active" : ""}`}>
                  <span className="bar-label">{formatDay(d.date)}</span>
                  <div className="bar-track">
                    <div className="bar-fill" style={{ width: `${(d.words / maxWeek) * 100}%` }} />
                  </div>
                  <span className="bar-n">{d.words.toLocaleString()}</span>
                </div>
              ))}
            </div>
          )}
        </ChartFrame>

        <ChartFrame
          id="daily30"
          title="Daily words · last 30 days"
          state={chartState({ loading: busy, error: dailyErr, hasData: days.some((d) => d.words > 0) })}
          labels={days.map((d) => pointAnnouncement(formatDay(d.date), d.words))}
          minHeight={160}
          onRetry={retry("daily_series")}
        >
          {(active) => (
            <svg className="chart-svg" viewBox="0 0 640 160" preserveAspectRatio="none" aria-hidden="true">
              {days.map((d, i) => {
                const step = 640 / Math.max(1, days.length);
                const bw = step * 0.66;
                const h = (d.words / maxDaily30) * 148;
                return (
                  <rect
                    key={d.date}
                    className={i === active ? "chart-mark is-active" : "chart-mark"}
                    x={i * step + (step - bw) / 2}
                    y={156 - h}
                    width={bw}
                    height={Math.max(d.words > 0 ? 2 : 0, h)}
                    rx={Math.min(4, bw / 2)}
                    fill="var(--accent-data)"
                  >
                    <title>
                      {formatDay(d.date)} — {d.words.toLocaleString()} words
                    </title>
                  </rect>
                );
              })}
            </svg>
          )}
        </ChartFrame>

        <ChartFrame
          id="year"
          title="Activity · last 365 days"
          state={chartState({ loading: busy, error: dailyErr, hasData: !!hasActivity })}
          labels={(heat?.cells ?? []).map(({ d }) => pointAnnouncement(formatDay(d.date), d.words))}
          minHeight={140}
          onRetry={retry("daily_series")}
        >
          {(active) => (
            <div className="heatmap-wrap">
              <p className="heat-summary">
                <strong>{heatSummary.words.toLocaleString()}</strong> words ·{" "}
                <strong>{heatSummary.sessions.toLocaleString()}</strong> sessions
                {heatSummary.best && (
                  <>
                    {" "}
                    · best day <strong>{formatDayShort(heatSummary.best.date)}</strong>
                  </>
                )}
              </p>
              <svg
                className="heatmap"
                viewBox={`0 0 ${heat.cols * 13} ${7 * 13 + 14}`}
                preserveAspectRatio="xMinYMid meet"
                aria-hidden="true"
              >
                {heat.months.map((m) => (
                  <text key={m.key} className="heat-month" x={m.col * 13} y={10}>
                    {m.label}
                  </text>
                ))}
                {heat.cells.map(({ d, col, row }, i) => (
                  <rect
                    key={d.date}
                    className={i === active ? "chart-mark is-active" : "chart-mark"}
                    x={col * 13}
                    y={row * 13 + 14}
                    width={10}
                    height={10}
                    rx={2}
                    fill={HEAT_FILL[heatLevel(d.words, heat.max)]}
                  >
                    <title>
                      {formatDay(d.date)} — {d.words.toLocaleString()} words
                    </title>
                  </rect>
                ))}
              </svg>
              <div className="heat-legend">
                <span>Less</span>
                {[0, 1, 2, 3, 4].map((lvl) => (
                  <span key={lvl} className="heat-swatch" style={{ background: HEAT_FILL[lvl] }} />
                ))}
                <span>More</span>
              </div>
            </div>
          )}
        </ChartFrame>

        <ChartFrame
          id="months"
          title="Words by month · last 12 months"
          state={chartState({ loading: busy, error: monthlyErr, hasData: (monthlySeries ?? []).some((d) => d.words > 0) })}
          labels={months.map((d) => pointAnnouncement(formatMonth(d.date), d.words))}
          minHeight={200}
          onRetry={retry("monthly_series")}
        >
          {(active) => (
            <>
              {monthlyView.quiet > 0 && months.length > 0 && (
                <p className="quiet-months">
                  {monthlyView.quiet} quiet {monthlyView.quiet === 1 ? "month" : "months"} before{" "}
                  {monthName(months[0].date, "long")}
                </p>
              )}
              <div className="bars">
                {months.map((d, i) => (
                  <div key={d.date} className={`bar-row${i === active ? " is-active" : ""}`}>
                    <span className="bar-label">{formatMonth(d.date)}</span>
                    <div className="bar-track">
                      <div className="bar-fill" style={{ width: `${(d.words / monthlyView.max) * 100}%` }} />
                    </div>
                    <span className="bar-n">{d.words.toLocaleString()}</span>
                  </div>
                ))}
              </div>
            </>
          )}
        </ChartFrame>

        <ChartFrame
          id="apps"
          title="Top apps"
          state={chartState({ loading: busy, hasData: apps.length > 0 })}
          labels={apps.map((a) => `${a.app}, ${a.words.toLocaleString()} words in ${a.sessions} ${a.sessions === 1 ? "take" : "takes"}`)}
          minHeight={160}
        >
          {(active) => (
            <div className="bars apps">
              {apps.map((a, i) => (
                <div key={a.app} className={`bar-row${i === active ? " is-active" : ""}`}>
                  <span className="bar-label" title={a.app}>
                    {a.app}
                  </span>
                  <div className="bar-track">
                    <div className="bar-fill" style={{ width: `${(a.words / maxApp) * 100}%` }} />
                  </div>
                  <span className="bar-n">
                    {a.words.toLocaleString()} w · {a.sessions}
                  </span>
                </div>
              ))}
            </div>
          )}
        </ChartFrame>
      </div>

      {/* YV94 / finding #29, redesigned — the meetings section: hours, the
          people who talked most (from speaker profiles, item 8), and the
          reliability lines. Yap ships no telemetry; this local rollup is the
          only way to see whether the Notetaker is used and trusted. */}
      <section className="panel meeting-strip insights-meetings" aria-labelledby="insights-meetings-h">
        <h3 id="insights-meetings-h">Meetings</h3>
        {loading ? (
          <div className="chart-skeleton" style={{ height: 96 }} aria-hidden="true" />
        ) : !meetings || meetings.totalMeetings === 0 ? (
          <div className="chart-empty-state" data-chart-state="empty">
            <p className="muted chart-empty">No meetings recorded yet. Hours and the people you talk with show up here.</p>
            <button type="button" className="ghost" onClick={() => setNav("meetings")}>
              Go to Meetings
            </button>
          </div>
        ) : (
          <>
            <div className="stats-row meeting-numbers">
              <div className="stat">
                <div className="stat-n">{meetingHours(meetings)}</div>
                <div className="stat-l">hours in meetings</div>
              </div>
              <div className="stat">
                <div className="stat-n">{meetings.totalMeetings.toLocaleString()}</div>
                <div className="stat-l">recorded</div>
              </div>
              <div className="stat">
                <div className="stat-n">{meetings.meetingsLast7}</div>
                <div className="stat-l">last 7 days</div>
              </div>
              <div className="stat">
                <div className="stat-n">{meetings.segmentsIndexed.toLocaleString()}</div>
                <div className="stat-l">segments searchable</div>
              </div>
            </div>
            <ChartFrame
              id="people"
              title="Top people"
              state={people && people.length > 0 ? "ready" : "empty"}
              emptyCopy={
                people === null
                  ? "People show up here once Yap knows who is talking in your meetings."
                  : "No named speakers yet. Name a speaker in a meeting and they show up here."
              }
              labels={(people ?? []).map((p) => `${p.name}, ${formatMinutes(p.seconds / 60)} across ${p.meetings} ${p.meetings === 1 ? "meeting" : "meetings"}`)}
              minHeight={96}
              nested
            >
              {(active) => {
                const max = Math.max(1, ...(people ?? []).map((p) => p.seconds));
                return (
                  <div className="bars people" data-top-people>
                    {(people ?? []).map((p, i) => (
                      <div key={p.name} className={`bar-row${i === active ? " is-active" : ""}`}>
                        <span className="bar-label" title={p.name}>
                          {p.name}
                        </span>
                        <div className="bar-track">
                          <div className="bar-fill" style={{ width: `${(p.seconds / max) * 100}%` }} />
                        </div>
                        <span className="bar-n">{formatMinutes(p.seconds / 60)}</span>
                      </div>
                    ))}
                  </div>
                );
              }}
            </ChartFrame>
            <ul className="kv">
              <li>
                <span>Finished cleanly</span>
                <strong>
                  {meetings.completeMeetings} of {meetings.totalMeetings}
                  {meetings.partialMeetings > 0 && ` · ${meetings.partialMeetings} partial`}
                  {meetings.failedMeetings > 0 && ` · ${meetings.failedMeetings} failed`}
                </strong>
              </li>
              <li>
                <span>Audio still on disk</span>
                <strong>
                  {meetings.meetingsWithAudio} · {meetings.audioRetentionDays}-day retention
                </strong>
              </li>
              {meetings.daysToFirstMeeting != null && (
                <li>
                  <span>Dictation → first meeting</span>
                  <strong>{meetings.daysToFirstMeeting} days</strong>
                </li>
              )}
            </ul>
          </>
        )}
      </section>

      {/* yap-uiux-2026-10-16-I1 — the old "Engine & latency" panel sat
          beside the week chart and spoke in percentiles, "utt" and a
          paragraph about MLX. It is reference, not the point of the
          screen: it now lives last, closed, in plain words, and its
          summary line carries the one number worth seeing shut. */}
      {ins && (
        <details className="panel speed-setup" data-speed-setup>
          <summary style={{ cursor: "pointer", fontWeight: 600, fontSize: "0.95rem" }}>
            Speed and setup
            {ins.p50PipelineMs ? (
              <span className="muted" style={{ fontWeight: 400 }}>
                {" "}· text usually lands in {seconds(ins.p50PipelineMs)}
              </span>
            ) : null}
          </summary>
          <ul className="kv" style={{ marginTop: 12 }}>
            <li>
              <span>Usual wait for text</span>
              <strong>{ins.p50PipelineMs ? seconds(ins.p50PipelineMs) : "—"}</strong>
            </li>
            <li>
              <span>Slowest takes (1 in 20)</span>
              <strong>{ins.p95PipelineMs ? seconds(ins.p95PipelineMs) : "—"}</strong>
            </li>
            <li>
              <span>Turning speech into words</span>
              <strong>{ins.avgAsrSeconds.toFixed(1)} s per take</strong>
            </li>
            <li>
              <span>Speech measured</span>
              <strong>
                {spoken(ins.speechSecondsTotal ?? 0)} across {(ins.wpmSampleSessions ?? 0).toLocaleString()}{" "}
                {(ins.wpmSampleSessions ?? 0) === 1 ? "take" : "takes"}
              </strong>
            </li>
            <li>
              <span>Dictation key</span>
              <strong className={carbonRegistered(status) ? "ok" : "bad"}>
                {carbonRegistered(status)
                  ? `${dictateLabel(status)}${fnDelivering(status) ? ` · ${settings?.hotkeyLabel || "fn"}` : " · fn off"}`
                  : "not set up"}
              </strong>
            </li>
            <li>
              <span>Accessibility (lets Yap paste)</span>
              <strong className={status.accessibility ? "ok" : "bad"}>
                {status.accessibility ? "allowed" : "not allowed"}
              </strong>
            </li>
          </ul>
        </details>
      )}
    </div>
  );
}

/**
 * One chart's frame: title, a body at its final height in every state, and
 * the keyboard layer. The body is ONE tab stop; arrow keys / Home / End move
 * through the points and a polite live region announces each one
 * ("Tue, Nov 4, 1,204 words"). Error shows one sentence and Retry; the other
 * charts keep rendering.
 */
function ChartFrame({
  id,
  title,
  state,
  labels,
  minHeight,
  onRetry,
  emptyCopy = CHART_EMPTY_COPY,
  nested = false,
  children,
}: {
  id: string;
  title: string;
  state: ChartState;
  labels: string[];
  minHeight: number;
  onRetry?: () => void;
  emptyCopy?: string;
  nested?: boolean;
  children: (active: number) => ReactNode;
}) {
  const [active, setActive] = useState(-1);
  const titleId = `insights-chart-${id}`;

  useEffect(() => {
    if (state === "error") uiLog("insights_chart_error", { chart: id });
  }, [state, id]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const next = nextPointIndex(active, e.key, labels.length);
    if (next === null) return;
    e.preventDefault();
    setActive(next);
  };

  const Tag = nested ? "div" : "section";
  return (
    <Tag className={nested ? "chart-frame nested" : "panel chart-frame"} data-chart={id} data-chart-state={state} aria-labelledby={titleId}>
      {nested ? (
        <h4 id={titleId} className="chart-title">
          {title}
        </h4>
      ) : (
        <h3 id={titleId}>{title}</h3>
      )}
      <div className="chart-body" style={{ minHeight }}>
        {state === "loading" && <div className="chart-skeleton" style={{ height: minHeight }} aria-hidden="true" />}
        {state === "empty" && <p className="muted chart-empty">{emptyCopy}</p>}
        {state === "error" && (
          <div className="chart-error" role="alert">
            <p>{CHART_ERROR_COPY}</p>
            {onRetry && (
              <button
                type="button"
                className="ghost"
                onClick={() => {
                  uiLog("insights_chart_retry", { chart: id });
                  onRetry();
                }}
              >
                Retry
              </button>
            )}
          </div>
        )}
        {state === "ready" && (
          <div
            className="chart-points"
            tabIndex={labels.length > 0 ? 0 : -1}
            role="group"
            aria-roledescription="chart"
            aria-labelledby={titleId}
            aria-describedby={`${titleId}-hint`}
            onKeyDown={onKeyDown}
            onBlur={() => setActive(-1)}
          >
            <span id={`${titleId}-hint`} className="sr-only">
              Use the arrow keys to read each point.
            </span>
            {children(active)}
            <span className="sr-only" aria-live="polite">
              {active >= 0 ? labels[active] : ""}
            </span>
          </div>
        )}
      </div>
    </Tag>
  );
}

/** 640 -> "0.64 s"; 1840 -> "1.8 s". Plain seconds read faster than ms. */
function seconds(ms: number): string {
  return ms < 1000 ? `${(ms / 1000).toFixed(2)} s` : `${(ms / 1000).toFixed(1)} s`;
}

/** Total speech as a person says it: "45 s", "12 min", "2 h 5 min". */
function spoken(totalSeconds: number): string {
  const s = Math.round(totalSeconds);
  if (s < 60) return `${s} s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}
