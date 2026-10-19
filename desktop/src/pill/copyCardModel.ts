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
/** paste.rs `COPY_CARD_MESSAGE_NO_REPAIR`: a grant works, so no Repair is offered. */
export const COPY_CARD_HINT_NO_REPAIR = "Copied — press ⌘V.";
export const COPY_CARD_HINT_NO_REPAIR_UNCOPIED = "Press Copy, then ⌘V.";
/** The AX set was accepted but not seen to land: the text may already be there. */
export const COPY_CARD_HINT_UNVERIFIED = "It may already be in — if not, press ⌘V.";

/** float_pill.rs `COPY_CARD_CLEAR_EVENT`: a new take dropped the card. */
export const COPY_CARD_CLEAR_EVENT = "insert-copy-card-clear";

/** How long an untouched card stays up. */
export const COPY_CARD_TTL_MS = 30_000;

export type CopyCardReason = "no_permission" | "no_receipt" | "paste_failed" | "ax_unverified";

/** paste.rs `CopyCard` (serde camelCase). */
export interface CopyCardPayload {
  text: string;
  reason: CopyCardReason;
  detail: string;
  copied: boolean;
  /** `null` = no Repair button: a grant is working and Repair would reset it. */
  repairService: string | null;
}

export function isCopyCard(v: unknown): v is CopyCardPayload {
  if (!v || typeof v !== "object") return false;
  const c = v as Record<string, unknown>;
  return (
    typeof c.text === "string" &&
    c.text.length > 0 &&
    (typeof c.repairService === "string" || c.repairService === null)
  );
}

/** Only a card that names a service offers Repair. */
export function canRepair(card: Pick<CopyCardPayload, "repairService">): boolean {
  return typeof card.repairService === "string" && card.repairService.length > 0;
}

/** The card's one line under the text. */
export function cardHint(copied: boolean, repair = true, reason?: CopyCardReason): string {
  if (reason === "ax_unverified" && copied) return COPY_CARD_HINT_UNVERIFIED;
  if (!repair) return copied ? COPY_CARD_HINT_NO_REPAIR : COPY_CARD_HINT_NO_REPAIR_UNCOPIED;
  return copied ? COPY_CARD_HINT : COPY_CARD_HINT_UNCOPIED;
}

/** The card shows the take's opening; the Copy button carries all of it. */
export function cardPreview(text: string, max = 140): string {
  const t = text.replace(/\s+/g, " ").trim();
  return t.length <= max ? t : `${t.slice(0, max - 1).trimEnd()}…`;
}
