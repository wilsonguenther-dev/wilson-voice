import { useEffect, useState, type ReactNode } from "react";
import { errorText } from "./errors";
import { type ViewId, loadingLabel, viewErrorText } from "./viewState";

/**
 * Y5-B — the three states, as real components, marked so they can be counted
 * and walked.
 *
 * `data-empty-state` / `data-loading-state` / `data-error-state` carry the
 * view id as their value, so Y0-E's structural smoke can assert "this view
 * rendered zero rows AND carried no empty marker" instead of counting
 * attributes. The counts in this item's acceptance are a cheap pre-flight;
 * the smoke is the gate that decides.
 *
 * The MARKERS live in the view modules, never in the App.tsx shell — Y5-G
 * already moved the views out, and putting state markers back in the shell
 * would undo that. These shared blocks sit at src/ root alongside DiffView and
 * StatusDot, because views/ is one-module-per-nav-value and Y5-G ships a test
 * that enforces exactly that.
 */

/* ────────────────────────────────────────────────────────────────────────
   Glyphs.

   The item forbids one generic illustration reused seven times: "Generic is
   the thing being fixed." So each empty state gets a glyph drawn for its own
   subject — a waveform for dictation, a room for meetings, a rising series
   for stats, a marked word for the dictionary, a folded page for notes.
   They share a stroke weight and a viewBox and nothing else.
   ──────────────────────────────────────────────────────────────────────── */

type GlyphProps = { className?: string };

const G = {
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  fill: "none",
};

/** Dictation — a held key and the waveform it produces. */
export function WaveGlyph({ className }: GlyphProps) {
  return (
    <svg viewBox="0 0 48 32" className={className} aria-hidden focusable="false">
      <rect x="1" y="9" width="13" height="13" rx="3" {...G} />
      <path d="M5 15.5h5M5 12.5h5" {...G} />
      <path d="M20 16h2M26 9v14M31 12v8M36 6v20M41 13v6M46 16h1" {...G} />
    </svg>
  );
}

/** Meetings — a table with people around it. */
export function RoomGlyph({ className }: GlyphProps) {
  return (
    <svg viewBox="0 0 48 32" className={className} aria-hidden focusable="false">
      <ellipse cx="24" cy="18" rx="13" ry="6" {...G} />
      <circle cx="24" cy="5" r="3" {...G} />
      <circle cx="8" cy="14" r="3" {...G} />
      <circle cx="40" cy="14" r="3" {...G} />
      <circle cx="24" cy="29" r="2.5" {...G} />
    </svg>
  );
}

/** Insights — a series that has not started yet. */
export function SeriesGlyph({ className }: GlyphProps) {
  return (
    <svg viewBox="0 0 48 32" className={className} aria-hidden focusable="false">
      <path d="M4 2v27h41" {...G} />
      <path d="M11 25h4M19 25h4M27 25h4M35 25h4" {...G} strokeDasharray="1 3" />
      <path d="M9 19l8-5 7 4 9-9 8 3" {...G} strokeDasharray="4 3" />
    </svg>
  );
}

/** Dictionary — a word with a correction mark over it. */
export function WordGlyph({ className }: GlyphProps) {
  return (
    <svg viewBox="0 0 48 32" className={className} aria-hidden focusable="false">
      <path d="M6 24L14 7l8 17M9 18h10" {...G} />
      <path d="M28 24V8h5a4 4 0 010 8h-5m0 0h6a4 4 0 010 8h-6" {...G} />
      <path d="M40 6l4 4-4 4" {...G} />
    </svg>
  );
}

/** Scratchpad — a page with a folded corner. */
export function PageGlyph({ className }: GlyphProps) {
  return (
    <svg viewBox="0 0 48 32" className={className} aria-hidden focusable="false">
      <path d="M15 2h13l6 6v22H15z" {...G} />
      <path d="M28 2v6h6" {...G} />
      <path d="M19 15h11M19 20h11M19 25h6" {...G} />
    </svg>
  );
}

/* ────────────────────────────────────────────────────────────────────────
   Empty
   ──────────────────────────────────────────────────────────────────────── */

/**
 * Every state block takes its marker as an explicit prop written at the CALL
 * SITE, in the view module — PANEL 2026-09-12: "Write the markers into the
 * view MODULES; never into the App.tsx shell." Greping for
 * `data-empty-state` therefore lands a reader in the view that owns the
 * state, not in this file, and Y0-E's structural smoke can bind the marker's
 * value to the view it asserted rendered zero rows.
 */

export function EmptyState({
  "data-empty-state": mark,
  glyph,
  title,
  body,
  actionLabel,
  onAction,
  secondary,
}: {
  "data-empty-state": string;
  glyph: ReactNode;
  title: string;
  body: string;
  /** The ONE primary action that creates the first thing. Never optional. */
  actionLabel: string;
  onAction: () => void;
  secondary?: ReactNode;
}) {
  return (
    <div className="view-state view-empty" data-empty-state={mark}>
      <div className="view-state-glyph">{glyph}</div>
      <h3>{title}</h3>
      <p>{body}</p>
      <div className="view-state-actions">
        <button type="button" className="primary" onClick={onAction}>
          {actionLabel}
        </button>
        {secondary}
      </div>
    </div>
  );
}

/* ────────────────────────────────────────────────────────────────────────
   Loading
   ──────────────────────────────────────────────────────────────────────── */

/**
 * The item's rule, verbatim: "Not a full-page spinner. Not a skeleton that
 * pulses forever (a skeleton with no timeout is how the Drivia audit found
 * nineteen pages 'still loading at 15s')."
 *
 * So this one has a deadline. After `STALL_MS` it stops pretending and says
 * so, with the button that does something about it. The bars keep their
 * shape; they just stop animating, because an animation that has been been
 * running for ten seconds is the lie.
 */
const STALL_MS = 8000;

export function LoadingState({
  "data-loading-state": mark,
  noun,
  expected,
  rows = 3,
  onRetry,
}: {
  "data-loading-state": string;
  noun: string;
  /** The count when it is knowable — determinate. `null` for the calm one. */
  expected?: number | null;
  rows?: number;
  onRetry?: () => void;
}) {
  const [stalled, setStalled] = useState(false);
  useEffect(() => {
    const t = setTimeout(() => setStalled(true), STALL_MS);
    return () => clearTimeout(t);
  }, []);

  return (
    <div
      className={
        stalled ? "view-state view-loading stalled" : "view-state view-loading"
      }
      data-loading-state={mark}
      data-loading-stalled={stalled ? "true" : "false"}
      role="status"
      aria-live="polite"
      aria-busy={!stalled}
    >
      <div className="skeleton-rows" aria-hidden>
        {Array.from({ length: rows }, (_, i) => (
          <div
            key={i}
            className="skeleton-row"
            style={{ width: `${92 - i * 14}%` }}
          />
        ))}
      </div>
      {stalled ? (
        <>
          <p className="view-state-label">
            This is taking longer than it should.
          </p>
          {onRetry && (
            <div className="view-state-actions">
              <button type="button" className="primary" onClick={onRetry}>
                Try again
              </button>
            </div>
          )}
        </>
      ) : (
        <p className="view-state-label">{loadingLabel(noun, expected ?? null)}</p>
      )}
    </div>
  );
}

/* ────────────────────────────────────────────────────────────────────────
   Error
   ──────────────────────────────────────────────────────────────────────── */

/**
 * What failed, in the user's terms, and the one button that fixes it.
 *
 * The headline comes from `viewErrorText(view)` — "Yap could not open its
 * history file", never an SQLite code. The raw rejection is still on screen,
 * small and last, because it is what goes in a support bundle.
 */
export function ErrorState({
  "data-error-state": mark,
  view,
  error,
  actionLabel = "Try again",
  onAction,
  secondary,
}: {
  "data-error-state": string;
  view: ViewId;
  error: unknown;
  actionLabel?: string;
  onAction: () => void;
  secondary?: ReactNode;
}) {
  const detail = errorText(error);
  const headline = viewErrorText(view);
  return (
    <div className="view-state view-error" data-error-state={mark} role="alert">
      <h3>{headline}</h3>
      <p>Nothing was lost. Try again, and the rest of Yap keeps working.</p>
      <div className="view-state-actions">
        <button type="button" className="primary" onClick={onAction}>
          {actionLabel}
        </button>
        {secondary}
      </div>
      {detail && detail !== "null" && detail !== "undefined" && (
        <p className="view-state-detail tiny muted">{detail}</p>
      )}
    </div>
  );
}
