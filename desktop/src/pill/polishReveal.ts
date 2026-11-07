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
/**
 * yap-1107-reveal-stays-until-read — backend -> pill: close the reveal now.
 * Payload "esc" | "click_outside". The float panel is a non-activating NSPanel
 * that is normally never key, so the hover watch reads Esc and the left mouse
 * button from the window server instead. Mirrors `float_pill.rs`
 * `POLISH_REVEAL_CLOSE_EVENT`.
 */
export const POLISH_REVEAL_CLOSE_EVENT = "polish-reveal-close";
/** Pill -> backend: the reveal opened / closed (`lib.rs` `pill_reveal_state`). */
export const REVEAL_STATE_COMMAND = "pill_reveal_state";
/** "Use original" — `lib.rs` `use_original_take` (the Undo AI Edit paste). */
export const USE_ORIGINAL_COMMAND = "use_original_take";

/**
 * What a failed "Use original" says when Rust gave no sentence of its own.
 * Mirrors `lib.rs` `USE_ORIGINAL_FAILED_MESSAGE`.
 */
export const USE_ORIGINAL_FAILED = "Couldn’t paste the original. Try Use original again.";

/**
 * Mirrors `paste.rs` `REPLACE_NOT_FOUND_MESSAGE`: Use original could not find
 * the polished words right before the caret (the user typed after them, moved
 * the caret, or the field is unreadable), so nothing was typed and the original
 * went to the clipboard instead. Rust rejects with this sentence.
 */
export const USE_ORIGINAL_NOT_FOUND =
  "Couldn't find the polished words in that field. The original is on your clipboard.";

/**
 * yap-1107 — the ONLY rejections the card shows verbatim: every `Err` sentence
 * `lib.rs` `use_original_take` / `use_original_reply` can send. Anything not on
 * this list (Tauri's own runtime text, a join failure) becomes
 * `USE_ORIGINAL_FAILED`. A new Rust sentence must be added here to be shown.
 */
export const USE_ORIGINAL_SENTENCES: readonly string[] = [
  USE_ORIGINAL_FAILED,
  USE_ORIGINAL_NOT_FOUND,
  "Couldn’t read that take. Try Use original again.",
  "That take is gone from History.",
  "A newer take landed. Use History to paste this one.",
  "This take has no original to paste.",
];

/** Same sentence, ignoring surrounding space and straight vs curly apostrophes. */
function sameSentence(a: string, b: string): boolean {
  const norm = (s: string) => s.trim().replace(/[\u2018\u2019]/g, "'");
  return norm(a) === norm(b);
}
/** Unanswered and untouched, the toast leaves on its own no sooner than this (`revealDwell.ts`). */
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
  close: string;
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
    close: "Close",
    aria: `Polished your dictation, ${words} changed`,
  };
}

/**
 * Rust's refusal sentence when it is one of OUR sentences
 * (`USE_ORIGINAL_SENTENCES`), else the generic line. Anything else (a Tauri
 * runtime error such as "state not managed ...", a join failure, an Error
 * object) is never shown: it is logged for diagnosis and the card says
 * `USE_ORIGINAL_FAILED`.
 */
export function useOriginalError(e: unknown): string {
  if (typeof e === "string") {
    const known = USE_ORIGINAL_SENTENCES.find((s) => sameSentence(s, e));
    if (known) return known;
  }
  const raw = e instanceof Error ? e.message : String(e);
  console.warn(`pill polish_reveal_use_original failed=unknown_error raw=${JSON.stringify(raw.slice(0, 300))}`);
  return USE_ORIGINAL_FAILED;
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

/** The toast the dev-only matrix scenes draw (`float.html?phase=idle&polish=toast|card|toast-hover|card-hover`). */
export const SAMPLE_POLISH_REVEAL: PolishReveal = {
  id: "sample-take",
  raw: "so basically the report is kind of done now",
  text: "The report is now complete.",
  stages: "backtrack,rules,polish",
  changedWords: 5,
};

/**
 * yap-1107-polish-diff-readable — the capture sheet's takes. `fixture` is
 * Wilson's 2026-11-07 17:22Z take verbatim (the word-soup report); `one` is a
 * one-word change; `same` is a take polish left alone.
 */
export const POLISH_REVEAL_DIFF_SAMPLES: Record<"fixture" | "one" | "same", PolishReveal> = {
  fixture: {
    id: "sample-fixture",
    raw: "Go ahead and add the live onboarding No live training workshop to my calendar and then add the Wednesday onboarding book camp to my calendar.",
    text: "Add the live onboarding No live training workshop to your calendar. Add the Wednesday onboarding book camp to your calendar.",
    stages: "dictionary,backtrack,rules,polish",
    changedWords: 10,
  },
  one: {
    id: "sample-one-word",
    raw: "Send the deck to my team before the review.",
    text: "Send the deck to your team before the review.",
    stages: "rules,polish",
    changedWords: 1,
  },
  same: {
    id: "sample-unchanged",
    raw: "Ship the recovery item today.",
    text: "Ship the recovery item today.",
    stages: "rules,polish",
    changedWords: 0,
  },
};

/**
 * The forced matrix scene, or `null`. float-main only calls this when its
 * DEV-ONLY `FORCED` handle is compiled in (scripts/pill-matrix.mjs).
 * `&diff=fixture|one|same` swaps the sample take; `&layout=blocks|inline`
 * forces the card's layout (the "Show as before/after" toggle's two faces).
 */
export function forcedPolishReveal(
  search: string,
): { reveal: PolishReveal; view: PolishRevealFace; layout?: "inline" | "blocks"; hover?: true } | null {
  const q = new URLSearchParams(search);
  const raw = q.get("polish");
  // yap-1107-reveal-stays-until-read — `toast-hover` / `card-hover` force the
  // pointer-inside (held open) state of either face.
  const hover = raw === "toast-hover" || raw === "card-hover";
  const v = raw === "toast-hover" ? "toast" : raw === "card-hover" ? "card" : raw;
  if (v !== "toast" && v !== "card") return null;
  const d = q.get("diff");
  const reveal = d === "fixture" || d === "one" || d === "same" ? POLISH_REVEAL_DIFF_SAMPLES[d] : SAMPLE_POLISH_REVEAL;
  const l = q.get("layout");
  const base: { reveal: PolishReveal; view: PolishRevealFace; layout?: "inline" | "blocks"; hover?: true } =
    l === "blocks" || l === "inline" ? { reveal, view: v, layout: l } : { reveal, view: v };
  return hover ? { ...base, hover: true as const } : base;
}
