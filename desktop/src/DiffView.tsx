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
 *    nothing. No Sentry, no PostHog, no network call on this path. They exist so
 *    a future rules change can be scored against real dissatisfaction instead of
 *    a guess.
 */
import { useMemo } from "react";
import { diffWords, diffSummary, isUnchanged } from "./diff";

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
}

export default function DiffView({
  raw,
  formatted,
  stages,
  polishSkipReason,
  feedback,
  onFeedback,
  onUndo,
}: DiffViewProps) {
  const tokens = useMemo(() => diffWords(raw, formatted), [raw, formatted]);
  const summary = useMemo(() => diffSummary(tokens), [tokens]);
  const unchanged = isUnchanged(tokens);
  const stageList = (stages ?? "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);

  return (
    <div className="diff-view">
      <p className="diff-summary tiny">
        {unchanged
          ? "Nothing changed — this is exactly what you said."
          : `${summary.removed} removed · ${summary.added} added · ${summary.unchanged} unchanged`}
      </p>

      <p className="diff-body" aria-label="What formatting changed">
        {tokens.map((t, i) =>
          t.op === "equal" ? (
            <span key={i}>{t.text}</span>
          ) : (
            <span
              key={i}
              className={t.op === "added" ? "diff-added" : "diff-removed"}
              title={t.op === "added" ? "Added by Yap" : "Removed by Yap"}
            >
              {t.text}
            </span>
          ),
        )}
      </p>

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
