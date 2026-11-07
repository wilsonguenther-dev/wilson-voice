/**
 * Y4-G — "see what changed".
 *
 * Formatting that silently rewrites what you said is only acceptable if you can
 * see it and revert it. Yap has stored `raw_text` for every take since YV10 and
 * has had an undo (⌃⌘Z / `undo_ai_edit`) since YV51 — what it has never had is
 * the middle panel: raw → formatted, word by word, with the stages that ran
 * NAMED so a surprising change is attributable to a stage rather than to "the
 * AI".
 *
 * Two things here are deliberate and easy to get wrong:
 *
 *  * The SILENT-SKIP line. Both polish failure modes are invisible by
 *    construction, and for a take over ~150 words they are the normal outcome.
 *    A user who installs a 1.12 GB model and picks High would otherwise get
 *    rules-only output forever with no sign the model was ever consulted. When
 *    the stage was enabled and produced nothing, this panel says so and says
 *    WHY.
 *  * The thumbs are LOCAL, FOREVER. They are written to SQLite and read by
 *    nothing. No analytics SDK, no network call on this path. They exist so
 *    a future rules change can be scored against real dissatisfaction instead of
 *    a guess.
 */
import { useEffect, useMemo, useState } from "react";
import { diffLogLine, diffSummary, isUnchanged, readableDiff, type DiffLayout, type DiffToken, type ReadableDiff } from "./diff";

/** The toggle's words (yap-1107-polish-diff-readable). */
export const SHOW_BEFORE_AFTER = "Show as before/after";

function diffLog(event: string, fields: Record<string, string | number | boolean>): void {
  try {
    console.info(diffLogLine(event, fields));
  } catch {
    /* logging never breaks a panel */
  }
}

/**
 * yap-1107-polish-diff-readable — a removed run right before an added run gets
 * a small gap (CSS `.diff-pair-gap`) so "my" + "your" never reads as "myyour".
 * Not needed when either side already carries the space.
 */
export function needsPairGap(prev: DiffToken | undefined, cur: DiffToken): boolean {
  return (
    cur.op === "added" &&
    prev !== undefined &&
    prev.op === "removed" &&
    !/\s$/.test(prev.text) &&
    !/^\s/.test(cur.text)
  );
}

/** Human names for the pipeline's stage tags (`dictation::FormattingTrace`). */
const STAGE_LABELS: Record<string, string> = {
  dictionary: "Dictionary",
  backtrack: "Backtrack",
  rules: "Formatting rules",
  polish: "AI polish",
};

/**
 * Human sentences for the closed set of polish skip tags. Anything unknown
 * falls back to the tag itself — a tag the user can quote is better than a
 * reassuring sentence that hides a reason we did not anticipate.
 */
const SKIP_REASONS: Record<string, string> = {
  no_model: "no polish model is installed",
  no_sidecar: "the polish helper was not available",
  saving_battery: "Yap kept the plain text to save battery or cool this Mac",
  deadline: "the model ran out of time for a take this long",
  client_error: "the model could not be reached",
  client_panic: "the model helper crashed",
  v1_empty: "the model returned nothing",
  v2_runaway: "the model's rewrite ran away",
  v2_truncated: "the model's rewrite was cut short",
  v3_retention: "the model dropped part of what you said",
  v4_numbers: "the model changed your numbers",
  v5_invented_contact: "the model invented a name or address",
  v6_template_leak: "the model leaked its own instructions",
  v6_preamble: "the model answered instead of rewriting",
  v7_script_drift: "the model drifted into another script",
  v8_code_mode: "code is never sent to the model",
  unknown: "the model produced nothing usable",
};

export interface DiffViewProps {
  /** The verbatim ASR transcript (`rawText`). */
  raw: string;
  /** What the pipeline produced and pasted (`text`). */
  formatted: string;
  /** Comma-separated stage tags from `stagesThatRan`, or null for legacy rows. */
  stages?: string | null;
  /** Why the LLM stage produced nothing, or null when it ran or was off. */
  polishSkipReason?: string | null;
  /** -1, 1 or null. */
  feedback?: number | null;
  /** Called with the NEW value (null clears). Omit to hide the thumbs. */
  onFeedback?: (value: number | null) => void;
  /** Re-paste the raw take. Omit to hide the undo button. */
  onUndo?: () => void;
  /**
   * WF-05 — where AI polish ran, in the route chip's own words ("Cloud" /
   * "On this Mac", `pill/cloudChip.ts`). Never a vendor or a model. Omit to
   * hide the line (History's panel, where the take may predate the route).
   */
  route?: string | null;
  /**
   * yap-1107-polish-diff-readable — the layout the user chose with "Show as
   * before/after", held by the owner for the card session (the polish card).
   * Omit (History) and the panel remembers the choice itself until the take
   * changes. Unset = the diff's own verdict (blocks when changes are dense).
   */
  layout?: DiffLayout | null;
  /** Called with the layout the toggle switched to. */
  onLayoutChange?: (layout: DiffLayout) => void;
}

export default function DiffView({
  raw,
  formatted,
  stages,
  polishSkipReason,
  feedback,
  onFeedback,
  onUndo,
  route,
  layout,
  onLayoutChange,
}: DiffViewProps) {
  const diff = useMemo(() => readableDiff(raw, formatted), [raw, formatted]);
  // The summary counts the EXACT word-level edits, as it always has.
  const summary = useMemo(() => diffSummary(diff.exact), [diff]);
  const unchanged = isUnchanged(diff.exact);
  // Uncontrolled choice, tied to the diff it was made on: a new take resets it.
  const [own, setOwn] = useState<{ diff: ReadableDiff; layout: DiffLayout } | null>(null);
  const shown: DiffLayout = unchanged
    ? "inline"
    : (layout ?? (own !== null && own.diff === diff ? own.layout : diff.layout));

  useEffect(() => {
    if (isUnchanged(diff.exact)) return;
    diffLog("shown", { layout: diff.layout, hunks: diff.hunks, share: Math.round(diff.changedShare * 100) });
  }, [diff]);

  const toggleLayout = () => {
    const next: DiffLayout = shown === "blocks" ? "inline" : "blocks";
    diffLog("layout_toggle", { to: next, auto: diff.layout, hunks: diff.hunks });
    if (layout == null) setOwn({ diff, layout: next });
    onLayoutChange?.(next);
  };
  const stageList = (stages ?? "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);

  return (
    <div className="diff-view">
      <div className="diff-head">
        <p className="diff-summary tiny">
          {unchanged
            ? "Nothing changed — this is exactly what you said."
            : `${summary.removed} removed · ${summary.added} added · ${summary.unchanged} unchanged`}
        </p>
        {unchanged ? null : (
          // A real <button>: Tab reaches it, Space/Enter press it.
          <button
            type="button"
            className="ghost diff-layout-toggle"
            aria-pressed={shown === "blocks"}
            title={shown === "blocks" ? "Show the changes inline" : "Show what you said and what Yap pasted, one under the other"}
            onClick={toggleLayout}
          >
            {SHOW_BEFORE_AFTER}
          </button>
        )}
      </div>

      {shown === "blocks" ? (
        // Changes too dense to read inline (or the user asked): two plain
        // paragraphs. "Show opens the before and after." (pill.html, Fix-up notices)
        <div className="diff-blocks" role="group" aria-label="Before and after" data-layout="blocks">
          <div className="diff-block diff-block-before">
            <p className="diff-block-label tiny">Before</p>
            <p className="diff-block-text">{raw}</p>
          </div>
          <div className="diff-block diff-block-after">
            <p className="diff-block-label tiny">After</p>
            <p className="diff-block-text">{formatted}</p>
          </div>
        </div>
      ) : (
        <p className="diff-body" aria-label="What formatting changed" data-layout="inline">
          {diff.runs.map((t, i) =>
            t.op === "equal" ? (
              <span key={i}>{t.text}</span>
            ) : (
              <span
                key={i}
                className={
                  t.op === "added"
                    ? needsPairGap(diff.runs[i - 1], t)
                      ? "diff-added diff-pair-gap"
                      : "diff-added"
                    : "diff-removed"
                }
                title={t.op === "added" ? "Added by Yap" : "Removed by Yap"}
              >
                {t.text}
              </span>
            ),
          )}
        </p>
      )}

      <p className="diff-stages tiny">
        {stageList.length > 0 ? (
          <>
            Stages that ran:{" "}
            {stageList.map((s) => STAGE_LABELS[s] ?? s).join(" → ")}
          </>
        ) : (
          <>Stages that ran: none recorded for this take</>
        )}
      </p>

      {route && stageList.includes("polish") ? (
        <p className="diff-route tiny">
          AI polish ran: <span className="diff-route-chip">{route}</span>
        </p>
      ) : null}

      {polishSkipReason ? (
        // The silent-skip signal. Never hidden behind a disclosure: the whole
        // point is that the user currently has no way to know this happened.
        <p className="diff-skip tiny" role="status">
          AI polish was on but did not change this take —{" "}
          {SKIP_REASONS[polishSkipReason] ?? polishSkipReason}. The text above is
          from the formatting rules only.
        </p>
      ) : null}

      <div className="diff-actions">
        {onUndo && !unchanged ? (
          <button
            className="ghost"
            onClick={onUndo}
            title="Paste exactly what you said, before auto-cleanup"
          >
            Undo — paste raw
          </button>
        ) : null}
        {onFeedback ? (
          <span className="diff-feedback">
            <button
              className={feedback === 1 ? "ghost on" : "ghost"}
              aria-pressed={feedback === 1}
              title="This formatting was good (stays on this Mac)"
              onClick={() => onFeedback(feedback === 1 ? null : 1)}
            >
              👍
            </button>
            <button
              className={feedback === -1 ? "ghost on" : "ghost"}
              aria-pressed={feedback === -1}
              title="This formatting was wrong (stays on this Mac)"
              onClick={() => onFeedback(feedback === -1 ? null : -1)}
            >
              👎
            </button>
            <span className="tiny">Stays on this Mac.</span>
          </span>
        ) : null}
      </div>
    </div>
  );
}
