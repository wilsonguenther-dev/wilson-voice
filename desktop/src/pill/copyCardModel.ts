/**
 * yap-p0-insertion-bridge-fallback DO 2 — the pill's Copy card, the pure half.
 *
 * When Yap cannot insert a take by itself (neither Accessibility nor PostEvent
 * granted, or a ⌘V that nothing read), paste.rs emits INSERT_COPY_CARD_EVENT
 * with the transcript and the pill shows it at once with a Copy button. The
 * event name and the payload shape are a contract with Rust that no compiler
 * checks: copyCardModel.test.ts reads the Rust side to pin them.
 */

/** paste.rs `INSERT_COPY_CARD_EVENT`. */
export const INSERT_COPY_CARD_EVENT = "insert-copy-card";

/** paste.rs `COPY_CARD_MESSAGE`, verbatim. */
export const COPY_CARD_HINT = "Copied — press ⌘V. Repair to auto-insert.";
/** When the clipboard write failed or the user's own copy won. */
export const COPY_CARD_HINT_UNCOPIED = "Press Copy, then ⌘V. Repair to auto-insert.";

/** How long an untouched card stays up. */
export const COPY_CARD_TTL_MS = 30_000;

export type CopyCardReason = "no_permission" | "no_receipt" | "paste_failed";

/** paste.rs `CopyCard` (serde camelCase). */
export interface CopyCardPayload {
  text: string;
  reason: CopyCardReason;
  detail: string;
  copied: boolean;
  repairService: string;
}

export function isCopyCard(v: unknown): v is CopyCardPayload {
  if (!v || typeof v !== "object") return false;
  const c = v as Record<string, unknown>;
  return typeof c.text === "string" && c.text.length > 0 && typeof c.repairService === "string";
}

/** The card's one line under the text. */
export function cardHint(copied: boolean): string {
  return copied ? COPY_CARD_HINT : COPY_CARD_HINT_UNCOPIED;
}

/** The card shows the take's opening; the Copy button carries all of it. */
export function cardPreview(text: string, max = 140): string {
  const t = text.replace(/\s+/g, " ").trim();
  return t.length <= max ? t : `${t.slice(0, max - 1).trimEnd()}…`;
}
