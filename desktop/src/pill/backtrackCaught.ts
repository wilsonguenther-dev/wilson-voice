// WF-06 — "Backtrack caught": the pill names the spoken self-correction Yap
// applied to the take that just landed ("Dropped ‘2’, kept ‘3’") and offers
// Undo, which pastes the take exactly as dictated. Rust raises the event only
// for a REAL correction (`dictation::clean_backtrack_report`); filler-only
// cleanup never reaches this file. Pure model — no hooks, no Tauri imports.

/** Mirrors `lib.rs` `BACKTRACK_CAUGHT_EVENT`. */
export const BACKTRACK_CAUGHT_EVENT = "backtrack_caught";
/** Undo goes through the existing explicit re-paste command. */
export const BACKTRACK_UNDO_COMMAND = "paste_entry";
/** Unanswered, the toast leaves on its own after this long. */
export const BACKTRACK_CAUGHT_TTL_MS = 6_000;
/** A longer retracted clause is shortened so the toast stays one line. */
export const BACKTRACK_SNIPPET_MAX = 28;

/** The `backtrack_caught` payload (`lib.rs` `backtrack_caught_payload`). */
export interface BacktrackCaught {
  /** The history row this take landed as. */
  id: string;
  /** What the speaker retracted. */
  dropped: string;
  /** What replaced it. */
  kept: string;
  /** The take as dictated — what Undo pastes. */
  raw: string;
}

const nonEmpty = (v: unknown): v is string => typeof v === "string" && v.trim().length > 0;

export function isBacktrackCaught(v: unknown): v is BacktrackCaught {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return nonEmpty(o.id) && nonEmpty(o.dropped) && nonEmpty(o.kept) && nonEmpty(o.raw);
}

/** Collapse whitespace and cut at a word boundary with an ellipsis. */
export function snippet(text: string, max = BACKTRACK_SNIPPET_MAX): string {
  const flat = text.replace(/\s+/g, " ").trim();
  if (flat.length <= max) return flat;
  const cut = flat.slice(0, max - 1);
  const space = cut.lastIndexOf(" ");
  return `${(space > max / 2 ? cut.slice(0, space) : cut).trimEnd()}…`;
}

export function backtrackCaughtLines(c: BacktrackCaught): {
  label: string;
  headline: string;
  undo: string;
  dismiss: string;
  undoFailed: string;
} {
  return {
    label: "Backtrack caught",
    headline: `Dropped ‘${snippet(c.dropped)}’, kept ‘${snippet(c.kept)}’`,
    undo: "Undo",
    dismiss: "Dismiss",
    undoFailed: "Couldn’t paste the original. Try Undo again.",
  };
}

/** The `invoke` arguments for Undo: paste the raw take. */
export function backtrackUndoArgs(c: BacktrackCaught): { text: string } {
  return { text: c.raw };
}

/** The toast the dev-only matrix scene draws (`float.html?phase=idle&backtrack=1`). */
export const SAMPLE_BACKTRACK_CAUGHT: BacktrackCaught = {
  id: "sample-take",
  dropped: "2",
  kept: "3",
  raw: "Let's do coffee at 2 actually 3",
};

/**
 * The forced matrix scene's toast, or `null`. float-main only calls this when
 * its DEV-ONLY `FORCED` handle is compiled in (scripts/pill-matrix.mjs).
 */
export function forcedBacktrackCaught(search: string): BacktrackCaught | null {
  return new URLSearchParams(search).get("backtrack") === "1" ? SAMPLE_BACKTRACK_CAUGHT : null;
}
