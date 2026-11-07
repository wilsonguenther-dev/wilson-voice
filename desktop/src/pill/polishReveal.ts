// WF-05 — "Polished your dictation — Reveal edit". After a take the polish
// stage really rewrote (on this Mac or in the cloud), the pill shows a short
// "Polished — Reveal" toast. Reveal opens a small card with the take's diff
// (the shared DiffView) and two answers: Use original (selects the polished
// text in its field and types the words as dictated over it, the same replace
// Undo AI Edit uses; when the polished words cannot be found it copies the
// original instead and says so) and Keep.
// Rust raises the event only when more than a word or so changed once
// punctuation and capitals are ignored (`dictation::polish_changed_words`).
// Pure model — no hooks, no Tauri imports.
import type { AppStatus } from "../appTypes";
import { cloudChip } from "./cloudChip";
import { REVEAL_EDIT_TTL_MS, type PolishResult } from "./polishResult";

/** Mirrors `lib.rs` `POLISH_REVEAL_EVENT`. */
export const POLISH_REVEAL_EVENT = "polish_reveal";
/** "Use original" — `lib.rs` `use_original_take` (the Undo AI Edit paste). */
export const USE_ORIGINAL_COMMAND = "use_original_take";
/** Unanswered, the toast leaves on its own after this long. */
export const POLISH_REVEAL_TTL_MS = REVEAL_EDIT_TTL_MS;
/** Where polish ran when the route chip has nothing to say (a default install). */
export const LOCAL_ROUTE_LABEL = "On this Mac";

/** The `polish_reveal` payload (`lib.rs` `polish_reveal_payload`). */
export interface PolishReveal {
  /** The history row this take landed as. */
  id: string;
  /** The take as dictated — what Use original pastes. */
  raw: string;
  /** What the pipeline pasted. */
  text: string;
  /** The take's `stagesThatRan`, for the diff's stage line. */
  stages: string | null;
  /** How many words polish changed (punctuation and capitals ignored). */
  changedWords: number;
}

const nonEmpty = (v: unknown): v is string => typeof v === "string" && v.trim().length > 0;

export function isPolishReveal(v: unknown): v is PolishReveal {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return (
    nonEmpty(o.id) &&
    nonEmpty(o.raw) &&
    nonEmpty(o.text) &&
    o.raw !== o.text &&
    (o.stages === null || o.stages === undefined || typeof o.stages === "string") &&
    typeof o.changedWords === "number" &&
    o.changedWords > 0
  );
}

/** The two faces of the card: the short toast, then the opened diff. */
export type PolishRevealFace = "toast" | "card";

export function polishRevealLines(r: PolishReveal): {
  label: string;
  reveal: string;
  title: string;
  useOriginal: string;
  keep: string;
  dismiss: string;
  aria: string;
} {
  const words = r.changedWords === 1 ? "1 word" : `${r.changedWords} words`;
  return {
    label: "Polished",
    reveal: "Reveal",
    title: "What polish changed",
    useOriginal: "Use original",
    keep: "Keep",
    dismiss: "Dismiss",
    aria: `Polished your dictation, ${words} changed`,
  };
}

/** What a failed "Use original" says when Rust gave no sentence of its own. */
export const USE_ORIGINAL_FAILED = "Couldn’t paste the original. Try Use original again.";

/**
 * Mirrors `paste.rs` `REPLACE_NOT_FOUND_MESSAGE`: Use original could not find
 * the polished words right before the caret (the user typed after them, moved
 * the caret, or the field is unreadable), so nothing was typed and the original
 * went to the clipboard instead. Rust rejects with this sentence.
 */
export const USE_ORIGINAL_NOT_FOUND =
  "Couldn't find the polished words in that field. The original is on your clipboard.";

/** Rust's refusal sentence when it sent one, else the generic line. */
export function useOriginalError(e: unknown): string {
  return typeof e === "string" && e.trim().length > 0 ? e : USE_ORIGINAL_FAILED;
}

/** The `invoke` arguments for Use original: the take's row id. */
export function useOriginalArgs(r: PolishReveal): { id: string } {
  return { id: r.id };
}

/**
 * Where polish ran, as the byom route chip words it ("Cloud" / "On this Mac").
 * Never a vendor or a model: the label is `cloudChip`'s, and a default install
 * (no chip) polished on this Mac.
 */
export function revealRouteLabel(status: Pick<AppStatus, "cloudRoute"> | null | undefined): string {
  return cloudChip(status)?.label ?? LOCAL_ROUTE_LABEL;
}

/**
 * Where THIS take's polish ran, from the byom `polish_result` payload that
 * Rust emits just before `polish_reveal`. Same words as the route chip
 * ("Cloud" / "On this Mac"); never a vendor or a model.
 */
export function resultRouteLabel(r: Pick<PolishResult, "route">): string {
  return r.route === "cloud" ? "Cloud" : LOCAL_ROUTE_LABEL;
}

/** The toast the dev-only matrix scenes draw (`float.html?phase=idle&polish=toast|card`). */
export const SAMPLE_POLISH_REVEAL: PolishReveal = {
  id: "sample-take",
  raw: "so basically the report is kind of done now",
  text: "The report is now complete.",
  stages: "backtrack,rules,polish",
  changedWords: 5,
};

/**
 * The forced matrix scene, or `null`. float-main only calls this when its
 * DEV-ONLY `FORCED` handle is compiled in (scripts/pill-matrix.mjs).
 */
export function forcedPolishReveal(search: string): { reveal: PolishReveal; view: PolishRevealFace } | null {
  const v = new URLSearchParams(search).get("polish");
  if (v === "toast" || v === "card") return { reveal: SAMPLE_POLISH_REVEAL, view: v };
  return null;
}
