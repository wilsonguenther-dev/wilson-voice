/**
 * yap-1106-partial-transcript-stream — the pill's live words, pure half.
 *
 * SP-05 (`stream_decode.rs`) decodes a take WHILE it is spoken and emits
 * `transcript_partial {take, committed, tentative}` with the text decoded so
 * far. This file decides what the pill does with it, so every rule is testable
 * without a window:
 *
 * * **Pill only, never history.** A partial is display state: it is folded
 *   into React state and drawn in the phase strip. Nothing here (or in the
 *   hook) invokes a command or writes anywhere; history is still written only
 *   from the final `transcript`, exactly as before.
 * * **Only during the take.** The words show in `listening` /
 *   `listening-locked` and nowhere else; a new take, a finished take, an error
 *   or a cancel clears them, and a late partial from an older take is dropped.
 * * **Off under load.** The resource governor at anything but `full` hides
 *   them (the backend also stops streaming then: `a_constrained_governor_
 *   degrades_to_batch`), so a hot or low-battery Mac does no extra drawing.
 * * **Throttled.** The pill redraws at most once every
 *   {@link PARTIAL_RENDER_MIN_MS} (trailing edge, so the newest words always
 *   land), on top of the backend's own 500 ms emit limit.
 * * **Newest words win.** The strip is one 192 px line; the tail of the text is
 *   shown, cut at a word, behind a leading ellipsis — never the stale head.
 *
 * Telemetry: one `pill partial take_end …` line per streamed take (counts and
 * timings only, never transcript text).
 */
import { isListening, SIDE_DOCK_TEXT_PX, STRIP_CHAR_PX, type LivePhase } from "./live";
import type { GovernorView } from "./governor";

/** Mirrors `stream_decode::TRANSCRIPT_PARTIAL_EVENT`. */
export const TRANSCRIPT_PARTIAL_EVENT = "transcript_partial";

/** The fastest the pill redraws the live words. */
export const PARTIAL_RENDER_MIN_MS = 500;

/** Mirrors `stream_decode::TranscriptPartial`. */
export interface TranscriptPartial {
  take: number;
  committed: string;
  tentative: string;
}

export interface PartialState {
  /** The take the shown words belong to; null = nothing shown. */
  take: number | null;
  /** committed + tentative, trimmed. */
  text: string;
  /** Partials from takes below this id are stale and dropped. */
  floor: number;
  /** The highest take id ever seen (the next take's floor is one above it). */
  seenMax: number;
  /** Partials folded in for the current take (telemetry). */
  updates: number;
}

export const PARTIAL_EMPTY: PartialState = { take: null, text: "", floor: 0, seenMax: -1, updates: 0 };

export type PartialEvent =
  | { type: "partial"; payload: unknown }
  | { type: "recording"; recording: boolean }
  /** The take finished, failed or was cancelled (`transcript`, `transcript_error`, `take_cancelled`). */
  | { type: "end" };

/** The event payload, validated; anything malformed is ignored. */
export function parsePartial(payload: unknown): TranscriptPartial | null {
  if (!payload || typeof payload !== "object") return null;
  const p = payload as Record<string, unknown>;
  if (typeof p.take !== "number" || !Number.isFinite(p.take) || p.take < 0) return null;
  if (typeof p.committed !== "string") return null;
  const tentative = typeof p.tentative === "string" ? p.tentative : "";
  return { take: p.take, committed: p.committed, tentative };
}

/** The words a partial carries: committed, then any tentative tail. */
export function partialText(p: TranscriptPartial): string {
  const committed = p.committed.trim();
  const tentative = p.tentative.trim();
  if (!tentative) return committed;
  return committed ? `${committed} ${tentative}` : tentative;
}

export function reducePartial(s: PartialState, e: PartialEvent): PartialState {
  switch (e.type) {
    case "partial": {
      const p = parsePartial(e.payload);
      if (!p || p.take < s.floor) return s;
      const seenMax = Math.max(s.seenMax, p.take);
      // A newer take than the one on screen replaces it outright.
      const updates = s.take === p.take ? s.updates + 1 : 1;
      return { ...s, take: p.take, text: partialText(p), seenMax, updates };
    }
    case "recording":
      // A take starting: last take's words go, and so does any straggler from it.
      // A take stopping keeps the words until the take really ends (the pill
      // is no longer listening, so they are hidden anyway).
      if (!e.recording) return s;
      return { ...s, take: null, text: "", floor: Math.max(s.floor, s.seenMax + 1), updates: 0 };
    case "end":
      return { ...s, take: null, text: "", floor: Math.max(s.floor, s.seenMax + 1), updates: 0 };
  }
}

/** Live words are allowed only while the governor runs at full. `null` = not answered yet (the backend still gates). */
export function governorAllowsPartials(view: GovernorView | null | undefined): boolean {
  return !view || view.level === "full";
}

/** Whether the phase strip draws the live words right now. */
export function partialShows(text: string, phase: LivePhase, governor: GovernorView | null | undefined): boolean {
  return text.trim() !== "" && isListening(phase) && governorAllowsPartials(governor);
}

/** Characters that fit the strip's one line, ellipsis included. */
export const PARTIAL_MAX_CHARS = Math.floor(SIDE_DOCK_TEXT_PX / STRIP_CHAR_PX);

/**
 * The newest words that fit `maxChars`: the tail, cut at a word boundary,
 * behind a leading "…". A single word longer than the line keeps its end.
 */
export function partialTail(text: string, maxChars: number = PARTIAL_MAX_CHARS): string {
  const t = text.replace(/\s+/g, " ").trim();
  if (t.length <= maxChars) return t;
  const room = Math.max(1, maxChars - 1);
  const cut = t.slice(t.length - room);
  const space = cut.indexOf(" ");
  const tail = space >= 0 && space < cut.length - 1 ? cut.slice(space + 1) : cut;
  return `…${tail}`;
}

/** Trailing-edge throttle: ms to wait before the next redraw (0 = draw now). */
export function nextRenderDelay(lastRenderAt: number | null, now: number, interval: number = PARTIAL_RENDER_MIN_MS): number {
  if (lastRenderAt === null) return 0;
  const since = now - lastRenderAt;
  return since >= interval ? 0 : interval - since;
}

/** Per-take counters for the one telemetry line. */
export interface PartialTakeStats {
  take: number;
  updates: number;
  /** ms from the take starting to the first partial; null = none arrived. */
  firstMs: number | null;
  /** Longest gap between two partials, ms. */
  maxGapMs: number;
  /** Partials that arrived while the governor hid them. */
  hiddenByGovernor: number;
}

/** `pill partial take_end take=… updates=… first_ms=… max_gap_ms=… hidden_governor=…` — never text. */
export function partialTakeLogLine(s: PartialTakeStats): string {
  const first = s.firstMs === null ? "none" : String(Math.round(s.firstMs));
  return (
    `pill partial take_end take=${s.take} updates=${s.updates} first_ms=${first}` +
    ` max_gap_ms=${Math.round(s.maxGapMs)} hidden_governor=${s.hiddenByGovernor}`
  );
}

/** A sample for the pill matrix (`float.html?phase=listening&partial=1`). */
export const SAMPLE_PARTIAL_TEXT =
  "so the plan for Monday is we ship the new onboarding first and then we look at pricing";

/** DEV-ONLY: `?partial=1` pins the sample words so the matrix can photograph them. */
export function forcedPartial(search: string): string | null {
  return new URLSearchParams(search).get("partial") === "1" ? SAMPLE_PARTIAL_TEXT : null;
}
