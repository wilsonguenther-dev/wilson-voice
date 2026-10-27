/**
 * yap-notetaker-detect — the pill's "Record this meeting?" offer: its wire
 * shape, its copy, and the one call each answer makes. Pure, so vitest pins it.
 *
 * Rust owns the decision (`meeting_detect.rs`): the pill only shows what it is
 * handed and reports the answer. It never starts a recording on its own —
 * Record is the only path, and it goes through `meeting_offer_respond`.
 */
import { CONSENT_PILL_LINE } from "../meetings/consent";

/** Rust `meeting_detect::MEETING_OFFER_EVENT`. */
export const MEETING_OFFER_EVENT = "meeting_offer";
/** Rust `meeting_detect::MEETING_OFFER_CLEAR_EVENT`. */
export const MEETING_OFFER_CLEAR_EVENT = "meeting_offer_clear";
/** The command both buttons call. */
export const MEETING_OFFER_RESPOND = "meeting_offer_respond";

/** Mirrors `meeting_detect::MeetingOffer` (camelCase on the wire). */
export interface MeetingOffer {
  key: string;
  title: string | null;
  source: "calendar" | "app";
}

/** Mirrors `meeting_detect::OfferChoice`. "snooze" is the Not now button. */
export type OfferChoice = "record" | "snooze";

export const OFFER_HEADLINE = "Record this meeting?";

export function isMeetingOffer(v: unknown): v is MeetingOffer {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.key === "string" &&
    o.key.length > 0 &&
    (o.title === null || o.title === undefined || typeof o.title === "string") &&
    (o.source === "calendar" || o.source === "app")
  );
}

/** The lines the card draws, top to bottom. The title only when known. */
export function offerLines(offer: MeetingOffer): {
  headline: string;
  title: string | null;
  consent: string;
} {
  const title = offer.title?.trim() ? offer.title.trim() : null;
  return { headline: OFFER_HEADLINE, title, consent: CONSENT_PILL_LINE };
}

/** The invoke() arguments for an answer. */
export function respondArgs(offer: MeetingOffer, choice: OfferChoice) {
  return { key: offer.key, choice };
}
