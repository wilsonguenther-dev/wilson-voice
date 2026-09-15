import { heatLevel, HEAT_FILL, formatMonth, monthName, formatDay, formatDayShort } from "../appTypes";
import { useAppCtx } from "../appShell";

export default function Insights() {
  const {
    daily30, hasActivity, heat, heatSummary, insights,
    maxApp, maxDaily30, maxWeek, monthlySeries, monthlyView,
    settings, status,
  } = useAppCtx();
  if (!insights) return null;
  return (
            <div className="insights">
              <div className="stats-row">
                <div className="stat big">
                  <div className="stat-n">
                    {insights.totalWords.toLocaleString()}
                  </div>
                  <div className="stat-l">total words</div>
                </div>
                <div className="stat big">
                  <div className="stat-n">
                    {insights.totalSessions.toLocaleString()}
                  </div>
                  <div className="stat-l">sessions</div>
                </div>
                <div className="stat big">
                  <div className="stat-n">{Math.round(insights.avgWpm)}</div>
                  <div className="stat-l">wpm</div>
                </div>
                <div className="stat big">
                  <div className="stat-n">{insights.streakDays}</div>
                  <div className="stat-l">
                    streak (best {insights.longestStreak})
                  </div>
                </div>
              </div>
              {/* YV94 / finding #29 — the Meetings strip. Yap ships no
                  telemetry, so without a local rollup there is no way to tell
                  whether the Notetaker is used, retained or trusted. It renders
                  only once a meeting exists: an all-zero row on a screen about
                  dictation is noise. */}
              {insights.meetings && insights.meetings.totalMeetings > 0 && (
                <div className="panel meeting-strip">
                  <h3>Meetings</h3>
                  <div className="stats-row">
                    <div className="stat">
                      <div className="stat-n">
                        {insights.meetings.totalMeetings.toLocaleString()}
                      </div>
                      <div className="stat-l">recorded</div>
                    </div>
                    <div className="stat">
                      <div className="stat-n">
                        {insights.meetings.meetingsLast7}
                      </div>
                      <div className="stat-l">last 7 days</div>
                    </div>
                    <div className="stat">
                      <div className="stat-n">
                        {Math.round(insights.meetings.totalSeconds / 60)}
                      </div>
                      <div className="stat-l">minutes captured</div>
                    </div>
                    <div className="stat">
                      <div className="stat-n">
                        {insights.meetings.segmentsIndexed.toLocaleString()}
                      </div>
                      <div className="stat-l">segments searchable</div>
                    </div>
                  </div>
                  <ul className="kv">
                    <li>
                      <span>Finished cleanly</span>
                      <strong>
                        {insights.meetings.completeMeetings} of{" "}
                        {insights.meetings.totalMeetings}
                        {insights.meetings.partialMeetings > 0 &&
                          ` · ${insights.meetings.partialMeetings} partial`}
                        {insights.meetings.failedMeetings > 0 &&
                          ` · ${insights.meetings.failedMeetings} failed`}
                      </strong>
                    </li>
                    <li>
                      <span>Audio still on disk</span>
                      <strong>
                        {insights.meetings.meetingsWithAudio} ·{" "}
                        {insights.meetings.audioRetentionDays}-day retention
                      </strong>
                    </li>
                    {insights.meetings.daysToFirstMeeting != null && (
                      <li>
                        <span>Dictation → first meeting</span>
                        <strong>
                          {insights.meetings.daysToFirstMeeting} days
                        </strong>
                      </li>
                    )}
                  </ul>
                </div>
              )}

              <div className="panel-grid">
                <div className="panel">
                  <h3>Last 7 days</h3>
                  <div className="bars">
                    {insights.wordsLast7.map((d) => (
                      <div key={d.date} className="bar-row">
                        <span className="bar-label">{formatDay(d.date)}</span>
                        <div className="bar-track">
                          <div
                            className="bar-fill"
                            style={{
                              width: `${(d.words / maxWeek) * 100}%`,
                            }}
                          />
                        </div>
                        <span className="bar-n">
                          {d.words.toLocaleString()}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
                <div className="panel">
                  <h3>Engine & latency</h3>
                  <ul className="kv">
                    <li>
                      <span>Avg ASR (model)</span>
                      <strong>{insights.avgAsrSeconds.toFixed(2)}s</strong>
                    </li>
                    <li>
                      <span>p50 hold→clipboard</span>
                      <strong>
                        {insights.p50PipelineMs
                          ? `${insights.p50PipelineMs}ms`
                          : "—"}
                      </strong>
                    </li>
                    <li>
                      <span>p95 hold→clipboard</span>
                      <strong>
                        {insights.p95PipelineMs
                          ? `${insights.p95PipelineMs}ms`
                          : "—"}
                      </strong>
                    </li>
                    <li>
                      <span>Speech sample</span>
                      <strong>
                        {(insights.speechSecondsTotal ?? 0).toFixed(0)}s ·{" "}
                        {insights.wpmSampleSessions ?? 0} utt
                      </strong>
                    </li>
                    <li>
                      <span>Hotkey</span>
                      <strong
                        className={status.hotkeyRegistered ? "ok" : "bad"}
                      >
                        {status.hotkeyRegistered
                          ? settings?.hotkeyLabel || "fn ok"
                          : "not registered"}
                      </strong>
                    </li>
                    <li>
                      <span>Accessibility</span>
                      <strong className={status.accessibility ? "ok" : "bad"}>
                        {status.accessibility ? "trusted" : "denied"}
                      </strong>
                    </li>
                  </ul>
                  <p className="muted" style={{ marginTop: 12, fontSize: 13 }}>
                    WPM uses audio duration only — never model latency. Base ASR
                    is OpenAI Whisper weights via MLX (not three proprietary
                    models). Target: p50 hold→clipboard &lt; 800ms on Fast.
                  </p>
                </div>
              </div>

              <div className="panel" style={{ marginTop: 16 }}>
                <h3>Daily words · last 30 days</h3>
                {daily30.some((d) => d.words > 0) ? (
                  <svg
                    className="chart-svg"
                    viewBox="0 0 640 160"
                    preserveAspectRatio="none"
                    role="img"
                    aria-label="Words dictated per day over the last 30 days"
                  >
                    {daily30.map((d, i) => {
                      const step = 640 / Math.max(1, daily30.length);
                      const bw = step * 0.66;
                      const h = (d.words / maxDaily30) * 148;
                      return (
                        <rect
                          key={d.date}
                          x={i * step + (step - bw) / 2}
                          y={156 - h}
                          width={bw}
                          height={Math.max(d.words > 0 ? 2 : 0, h)}
                          fill="var(--accent)"
                        >
                          <title>
                            {formatDay(d.date)} — {d.words.toLocaleString()} words
                          </title>
                        </rect>
                      );
                    })}
                  </svg>
                ) : (
                  <p className="muted chart-empty">
                    No dictation yet. Hold your hotkey and start talking — your
                    daily words will chart here.
                  </p>
                )}
              </div>

              <div className="panel" style={{ marginTop: 16 }}>
                <h3>Activity · last 365 days</h3>
                {hasActivity ? (
                  <div className="heatmap-wrap">
                    <p className="heat-summary">
                      <strong>{heatSummary.words.toLocaleString()}</strong> words
                      · <strong>{heatSummary.sessions.toLocaleString()}</strong>{" "}
                      sessions
                      {heatSummary.best && (
                        <>
                          {" "}
                          · best day{" "}
                          <strong>{formatDayShort(heatSummary.best.date)}</strong>
                        </>
                      )}
                    </p>
                    <svg
                      className="heatmap"
                      viewBox={`0 0 ${heat.cols * 13} ${7 * 13 + 14}`}
                      preserveAspectRatio="xMinYMid meet"
                      role="img"
                      aria-label="Daily dictation activity heatmap for the last year"
                    >
                      {heat.months.map((m) => (
                        <text
                          key={m.key}
                          className="heat-month"
                          x={m.col * 13}
                          y={10}
                        >
                          {m.label}
                        </text>
                      ))}
                      {heat.cells.map(({ d, col, row }) => {
                        const lvl = heatLevel(d.words, heat.max);
                        return (
                          <rect
                            key={d.date}
                            x={col * 13}
                            y={row * 13 + 14}
                            width={10}
                            height={10}
                            rx={2}
                            fill={HEAT_FILL[lvl]}
                          >
                            <title>
                              {formatDay(d.date)} — {d.words.toLocaleString()} words
                            </title>
                          </rect>
                        );
                      })}
                    </svg>
                    <div className="heat-legend">
                      <span>Less</span>
                      {[0, 1, 2, 3, 4].map((lvl) => (
                        <span
                          key={lvl}
                          className="heat-swatch"
                          style={{ background: HEAT_FILL[lvl] }}
                        />
                      ))}
                      <span>More</span>
                    </div>
                  </div>
                ) : (
                  <p className="muted chart-empty">
                    A year of dictation lights up here — one square per day, brighter
                    the more you say.
                  </p>
                )}
              </div>

              {monthlySeries.some((d) => d.words > 0) && (
                <div className="panel" style={{ marginTop: 16 }}>
                  <h3>Words by month · last 12 months</h3>
                  {monthlyView.quiet > 0 && (
                    <p className="quiet-months">
                      {monthlyView.quiet} quiet{" "}
                      {monthlyView.quiet === 1 ? "month" : "months"} before{" "}
                      {monthName(monthlyView.rows[0].date, "long")}
                    </p>
                  )}
                  <div className="bars">
                    {monthlyView.rows.map((d) => (
                      <div key={d.date} className="bar-row">
                        <span className="bar-label">{formatMonth(d.date)}</span>
                        <div className="bar-track">
                          <div
                            className="bar-fill"
                            style={{
                              width: `${(d.words / monthlyView.max) * 100}%`,
                            }}
                          />
                        </div>
                        <span className="bar-n">{d.words.toLocaleString()}</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {insights.topApps.length > 0 && (
                <div className="panel" style={{ marginTop: 16 }}>
                  <h3>Top apps</h3>
                  <div className="bars apps">
                    {insights.topApps.map((a) => (
                      <div key={a.app} className="bar-row">
                        <span className="bar-label" title={a.app}>
                          {a.app}
                        </span>
                        <div className="bar-track">
                          <div
                            className="bar-fill"
                            style={{ width: `${(a.words / maxApp) * 100}%` }}
                          />
                        </div>
                        <span className="bar-n">
                          {a.words.toLocaleString()} w · {a.sessions}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
  );
}
